//! Infeasible-branch and redundant-condition detection over SCCP and intervals.
//!
//! The checker combines executable-edge facts from sparse conditional
//! constant propagation (SCCP) with path-sensitive refinement of integer
//! intervals. Compound boolean conditions are evaluated by a heap-resident
//! pushdown machine; no source parsing or native-stack recursion is involved.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::analysis::dataflow::{
    comparison_regions, guard_expression, node_value_ranges, reverse_comparison,
};
use crate::analysis::function_scope_nodes;
use crate::{
    sccp, AnalysisError, CfgEdgeKind, CodePropertyGraph, ConstValue, CpgEdgeKind, CpgNodeKind,
    Interval, LiteralKind, NodeId,
};

/// Hard ceiling on unique nodes in one normalized condition arena.
pub const MAX_CONDITION_NODES: usize = 200_000;
/// Hard ceiling on path alternatives created for one truth-value query.
pub const MAX_CONDITION_STATES: usize = 65_536;
/// Hard ceiling on pushdown-machine transitions for one truth-value query.
pub const MAX_CONDITION_STEPS: usize = 10_000_000;

/// Resource limits for [`analyze_infeasible_paths`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct InfeasiblePathConfig {
    /// Maximum unique typed AST nodes retained for one condition.
    pub max_condition_nodes: usize,
    /// Maximum path alternatives produced by logical disjunction.
    pub max_condition_states: usize,
    /// Maximum pushdown-machine transitions for either truth query.
    pub max_condition_steps: usize,
}

impl Default for InfeasiblePathConfig {
    fn default() -> Self {
        Self {
            max_condition_nodes: MAX_CONDITION_NODES,
            max_condition_states: 4_096,
            max_condition_steps: 2_000_000,
        }
    }
}

/// Boolean outcome labeling a conditional CFG edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum BranchOutcome {
    /// Edge taken when the condition evaluates to true.
    True,
    /// Edge taken when the condition evaluates to false.
    False,
}

impl BranchOutcome {
    fn value(self) -> bool {
        matches!(self, Self::True)
    }

    fn from_kind(kind: CfgEdgeKind) -> Option<Self> {
        match kind {
            CfgEdgeKind::ConditionalTrue => Some(Self::True),
            CfgEdgeKind::ConditionalFalse => Some(Self::False),
            _ => None,
        }
    }
}

/// Analysis substrate sufficient to prove a finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum FeasibilityProofSource {
    /// SCCP alone proved the CFG edge non-executable.
    Sccp,
    /// Interval refinement alone emptied every path alternative.
    Intervals,
    /// The proof uses both SCCP constants and interval refinement.
    SccpAndIntervals,
}

/// One normalized predicate applied along an interval-refinement alternative.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IntervalPredicateEvidence {
    /// Comparison AST node.
    pub node: NodeId,
    /// Compared variable name.
    pub variable: Arc<str>,
    /// Operator normalized with the variable on the left.
    pub operator: Arc<str>,
    /// Signed integer constant on the right.
    pub constant: i128,
    /// Truth value required of the predicate on this alternative.
    pub required_truth: bool,
    /// One disjoint admissible interval selected for this alternative.
    /// Empty means the predicate itself denotes the empty integer set.
    pub admissible: Vec<Interval>,
}

/// One exhaustive condition alternative that interval refinement proved empty.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IntervalContradiction {
    /// Predicates accumulated in evaluation order before the empty meet.
    pub predicates: Vec<IntervalPredicateEvidence>,
}

/// Semantic category of one feasibility finding.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum InfeasiblePathKind {
    /// One represented conditional CFG edge cannot execute.
    InfeasibleBranch {
        /// Boolean outcome labeling the edge.
        branch: BranchOutcome,
        /// Edge target.
        target: NodeId,
    },
    /// The condition always evaluates to the recorded value.
    RedundantCondition {
        /// Proven value of the complete condition.
        value: bool,
    },
    /// No integer state can make the complete condition true.
    ContradictoryCondition,
}

/// One advisory infeasible-path, redundant-condition, or contradiction fact.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct InfeasiblePathFinding {
    /// Function containing the condition.
    pub function: NodeId,
    /// CFG source carrying conditional edges.
    pub source: NodeId,
    /// Typed AST expression controlling those edges.
    pub condition: NodeId,
    /// Semantic category.
    pub kind: InfeasiblePathKind,
    /// Analysis substrate sufficient for the proof.
    pub proof: FeasibilityProofSource,
    /// Boolean SCCP value of the complete guard, when one was proved.
    pub sccp_value: Option<bool>,
    /// Named intervals immediately before the CFG source.
    pub entry_intervals: Vec<(Arc<str>, Interval)>,
    /// Exhaustive interval alternatives that became empty.
    pub interval_contradictions: Vec<IntervalContradiction>,
}

/// One reachable condition that entered the advisory analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AnalyzedCondition {
    /// Containing function.
    pub function: NodeId,
    /// Conditional CFG source.
    pub source: NodeId,
    /// Guard expression.
    pub condition: NodeId,
}

/// Explicit evidence gaps that prevent a complete negative claim.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum InfeasiblePathIssue {
    /// A reachable conditional source had no typed guard expression.
    MissingCondition {
        /// Containing function.
        function: NodeId,
        /// Conditional CFG source.
        source: NodeId,
    },
    /// A condition referenced a node absent from the CPG.
    MissingConditionNode {
        /// Containing function.
        function: NodeId,
        /// Guard root.
        condition: NodeId,
        /// Missing node.
        node: NodeId,
    },
    /// A condition AST contains a directed cycle.
    CyclicCondition {
        /// Containing function.
        function: NodeId,
        /// Guard root.
        condition: NodeId,
        /// Re-entered active node.
        node: NodeId,
    },
    /// A condition exceeded its configured unique-node budget.
    ConditionNodeCap {
        /// Containing function.
        function: NodeId,
        /// Guard root.
        condition: NodeId,
        /// First required count beyond the cap.
        needed: usize,
        /// Configured cap.
        cap: usize,
    },
    /// Logical path splitting exceeded the configured state budget.
    ConditionStateCap {
        /// Containing function.
        function: NodeId,
        /// Guard root.
        condition: NodeId,
        /// First required state count beyond the cap.
        needed: usize,
        /// Configured cap.
        cap: usize,
    },
    /// Condition evaluation exceeded the configured transition budget.
    ConditionStepCap {
        /// Containing function.
        function: NodeId,
        /// Guard root.
        condition: NodeId,
        /// Configured cap.
        cap: usize,
    },
    /// A reachable CFG source was absent from the interval node-state view.
    MissingIntervalState {
        /// Containing function.
        function: NodeId,
        /// Conditional CFG source.
        source: NodeId,
    },
    /// A typed condition node was outside the supported boolean/interval vocabulary.
    UnsupportedConditionNode {
        /// Containing function.
        function: NodeId,
        /// Guard root.
        condition: NodeId,
        /// Unsupported node.
        node: NodeId,
    },
}

/// Complete out-of-graph feasibility report.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct InfeasiblePathAnalysis {
    /// Canonically ordered advisory findings.
    pub findings: Vec<InfeasiblePathFinding>,
    /// Functions with at least one reachable boolean condition.
    pub analyzed_functions: Vec<NodeId>,
    /// Reachable conditions admitted to SCCP/interval evaluation.
    pub analyzed_conditions: Vec<AnalyzedCondition>,
    /// Ordered completeness gaps.
    pub issues: BTreeSet<InfeasiblePathIssue>,
}

impl InfeasiblePathAnalysis {
    /// Returns whether every reachable boolean condition had complete bounded evidence.
    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Debug, Clone, Copy)]
struct ConditionalEdge {
    source: NodeId,
    target: NodeId,
    branch: BranchOutcome,
}

#[derive(Debug, Clone)]
struct Predicate {
    node: NodeId,
    variable: Arc<str>,
    operator: Arc<str>,
    constant: i128,
}

#[derive(Debug, Clone)]
enum ConditionExpr {
    Boolean(bool),
    Not(NodeId),
    And(NodeId, NodeId),
    Or(NodeId, NodeId),
    Predicate(Predicate),
    Unsupported,
}

#[derive(Debug)]
struct ConditionArena {
    expressions: BTreeMap<NodeId, ConditionExpr>,
}

enum ArenaFrame {
    Enter(NodeId),
    Exit(NodeId),
}

#[derive(Debug)]
enum ArenaError {
    Missing(NodeId),
    Cycle(NodeId),
    Cap { needed: usize },
}

#[derive(Debug, Clone)]
struct MachineState {
    intervals: BTreeMap<Arc<str>, Interval>,
    predicates: Vec<IntervalPredicateEvidence>,
    pending: Vec<(NodeId, bool)>,
    used_sccp: bool,
}

#[derive(Debug)]
struct FailedAlternative {
    predicates: Vec<IntervalPredicateEvidence>,
    used_sccp: bool,
}

#[derive(Debug)]
struct ConditionOutcome {
    possible: bool,
    unknown_nodes: BTreeSet<NodeId>,
    failures: Vec<FailedAlternative>,
}

impl ConditionOutcome {
    fn is_impossible(&self) -> bool {
        !self.possible && self.unknown_nodes.is_empty()
    }

    fn proof_source(&self) -> FeasibilityProofSource {
        if self.failures.iter().any(|failure| failure.used_sccp) {
            FeasibilityProofSource::SccpAndIntervals
        } else {
            FeasibilityProofSource::Intervals
        }
    }

    fn contradictions(&self) -> Vec<IntervalContradiction> {
        let mut contradictions: Vec<_> = self
            .failures
            .iter()
            .map(|failure| IntervalContradiction {
                predicates: failure.predicates.clone(),
            })
            .collect();
        contradictions.sort_by(compare_contradictions);
        contradictions.dedup();
        contradictions
    }
}

#[derive(Debug)]
enum MachineError {
    States { needed: usize },
    Steps,
}

/// Proves infeasible branches and redundant conditions without mutating the CPG.
///
/// Only reachable sources with `ConditionalTrue`/`ConditionalFalse` edges are
/// considered. SCCP supplies whole-guard constants and dead edges. The
/// interval machine evaluates both truth values with exact short-circuit
/// partitions: false conjunction and true disjunction split into exhaustive
/// alternatives, while conjunction and false disjunction refine sequentially.
pub fn analyze_infeasible_paths(
    cpg: &CodePropertyGraph,
    config: &InfeasiblePathConfig,
) -> Result<InfeasiblePathAnalysis, AnalysisError> {
    validate_config(config)?;
    let mut analysis = InfeasiblePathAnalysis::default();
    let functions: BTreeSet<_> = cpg.functions().map(|function| function.id).collect();
    for function in functions {
        let edges = conditional_edges(cpg, function);
        if edges.is_empty() {
            continue;
        }
        let sccp_result = sccp(cpg, function)?;
        let intervals = node_value_ranges(cpg, function)?;
        let unreachable: BTreeSet<_> = sccp_result.unreachable_nodes.iter().copied().collect();
        let dead: BTreeSet<_> = sccp_result
            .dead_edges
            .iter()
            .filter_map(|&(source, target, kind)| {
                BranchOutcome::from_kind(kind).map(|branch| (source, target, branch))
            })
            .collect();
        let by_source = group_edges(edges);
        let mut function_analyzed = false;

        for (source, source_edges) in by_source {
            if unreachable.contains(&source) {
                continue;
            }
            let Some(condition) = guard_expression(cpg, source) else {
                analysis
                    .issues
                    .insert(InfeasiblePathIssue::MissingCondition { function, source });
                continue;
            };
            function_analyzed = true;
            analysis.analyzed_conditions.push(AnalyzedCondition {
                function,
                source,
                condition,
            });
            let entry = intervals.intervals_before(source).map(<[_]>::to_vec);
            if entry.is_none() {
                analysis
                    .issues
                    .insert(InfeasiblePathIssue::MissingIntervalState { function, source });
            }
            let arena = match build_condition_arena(cpg, condition, config.max_condition_nodes) {
                Ok(arena) => Some(arena),
                Err(error) => {
                    record_arena_issue(&mut analysis.issues, function, condition, error, config);
                    None
                }
            };
            let mut outcomes = BTreeMap::new();
            if let (Some(entry), Some(arena)) = (entry.as_deref(), arena.as_ref()) {
                for branch in [BranchOutcome::True, BranchOutcome::False] {
                    match evaluate_condition(
                        arena,
                        condition,
                        branch.value(),
                        entry,
                        &sccp_result.constants,
                        config,
                    ) {
                        Ok(outcome) => {
                            for &node in &outcome.unknown_nodes {
                                analysis.issues.insert(
                                    InfeasiblePathIssue::UnsupportedConditionNode {
                                        function,
                                        condition,
                                        node,
                                    },
                                );
                            }
                            outcomes.insert(branch, outcome);
                        }
                        Err(error) => record_machine_issue(
                            &mut analysis.issues,
                            function,
                            condition,
                            error,
                            config,
                        ),
                    }
                }
            }

            let sccp_value = boolean_constant(&sccp_result.constants, condition)
                .or_else(|| boolean_constant(&sccp_result.constants, source));
            for edge in &source_edges {
                let sccp_impossible = dead.contains(&(edge.source, edge.target, edge.branch));
                let interval_outcome = outcomes.get(&edge.branch);
                let interval_impossible =
                    interval_outcome.is_some_and(ConditionOutcome::is_impossible);
                if !sccp_impossible && !interval_impossible {
                    continue;
                }
                analysis.findings.push(InfeasiblePathFinding {
                    function,
                    source,
                    condition,
                    kind: InfeasiblePathKind::InfeasibleBranch {
                        branch: edge.branch,
                        target: edge.target,
                    },
                    proof: combine_proofs(sccp_impossible, interval_outcome),
                    sccp_value,
                    entry_intervals: entry.clone().unwrap_or_default(),
                    interval_contradictions: interval_outcome
                        .filter(|outcome| outcome.is_impossible())
                        .map_or_else(Vec::new, ConditionOutcome::contradictions),
                });
            }

            let true_impossible = branch_is_impossible(
                BranchOutcome::True,
                &source_edges,
                &dead,
                outcomes.get(&BranchOutcome::True),
            );
            let false_impossible = branch_is_impossible(
                BranchOutcome::False,
                &source_edges,
                &dead,
                outcomes.get(&BranchOutcome::False),
            );
            let redundant_value = match (true_impossible, false_impossible) {
                (true, false) => Some(false),
                (false, true) => Some(true),
                _ => None,
            };
            if let Some(value) = redundant_value {
                let impossible_branch = if value {
                    BranchOutcome::False
                } else {
                    BranchOutcome::True
                };
                let interval_outcome = outcomes.get(&impossible_branch);
                let sccp_impossible = sccp_value.is_some_and(|constant| constant == value)
                    || branch_edges_all_dead(impossible_branch, &source_edges, &dead);
                analysis.findings.push(InfeasiblePathFinding {
                    function,
                    source,
                    condition,
                    kind: InfeasiblePathKind::RedundantCondition { value },
                    proof: combine_proofs(sccp_impossible, interval_outcome),
                    sccp_value,
                    entry_intervals: entry.clone().unwrap_or_default(),
                    interval_contradictions: interval_outcome
                        .filter(|outcome| outcome.is_impossible())
                        .map_or_else(Vec::new, ConditionOutcome::contradictions),
                });
            }

            if outcomes.get(&BranchOutcome::True).is_some_and(|outcome| {
                outcome.is_impossible()
                    && outcome
                        .failures
                        .iter()
                        .any(|failure| !failure.predicates.is_empty())
            }) {
                let outcome = &outcomes[&BranchOutcome::True];
                analysis.findings.push(InfeasiblePathFinding {
                    function,
                    source,
                    condition,
                    kind: InfeasiblePathKind::ContradictoryCondition,
                    proof: if sccp_value == Some(false) {
                        FeasibilityProofSource::SccpAndIntervals
                    } else {
                        outcome.proof_source()
                    },
                    sccp_value,
                    entry_intervals: entry.clone().unwrap_or_default(),
                    interval_contradictions: outcome.contradictions(),
                });
            }
        }
        if function_analyzed {
            analysis.analyzed_functions.push(function);
        }
    }
    analysis.analyzed_conditions.sort();
    analysis.analyzed_conditions.dedup();
    analysis.findings.sort_by(|left, right| {
        (left.function, left.source, &left.kind).cmp(&(right.function, right.source, &right.kind))
    });
    analysis.findings.dedup_by(|left, right| {
        left.function == right.function && left.source == right.source && left.kind == right.kind
    });
    Ok(analysis)
}

/// Returns advisory findings while discarding completeness evidence.
///
/// Consumers making negative claims must use [`analyze_infeasible_paths`] and
/// require [`InfeasiblePathAnalysis::is_complete`].
pub fn infeasible_paths(
    cpg: &CodePropertyGraph,
    config: &InfeasiblePathConfig,
) -> Result<Vec<InfeasiblePathFinding>, AnalysisError> {
    Ok(analyze_infeasible_paths(cpg, config)?.findings)
}

fn validate_config(config: &InfeasiblePathConfig) -> Result<(), AnalysisError> {
    for (input, value, hard_cap) in [
        (
            "max_condition_nodes",
            config.max_condition_nodes,
            MAX_CONDITION_NODES,
        ),
        (
            "max_condition_states",
            config.max_condition_states,
            MAX_CONDITION_STATES,
        ),
        (
            "max_condition_steps",
            config.max_condition_steps,
            MAX_CONDITION_STEPS,
        ),
    ] {
        if value == 0 || value > hard_cap {
            return Err(AnalysisError::InvalidInput {
                analysis: "infeasible-path detection",
                input,
                reason: "must be positive and no greater than its documented hard ceiling",
            });
        }
    }
    Ok(())
}

fn conditional_edges(cpg: &CodePropertyGraph, function: NodeId) -> Vec<ConditionalEdge> {
    let scope = function_scope_nodes(cpg, function);
    let mut edges = BTreeMap::new();
    for &source in &scope {
        for edge in cpg.outgoing_edges(source) {
            let CpgEdgeKind::ControlFlow(kind) = edge.kind else {
                continue;
            };
            let Some(branch) = BranchOutcome::from_kind(kind) else {
                continue;
            };
            edges.insert(
                (source, branch, edge.target),
                ConditionalEdge {
                    source,
                    target: edge.target,
                    branch,
                },
            );
        }
    }
    edges.into_values().collect()
}

fn group_edges(edges: Vec<ConditionalEdge>) -> BTreeMap<NodeId, Vec<ConditionalEdge>> {
    let mut grouped = BTreeMap::<_, Vec<_>>::new();
    for edge in edges {
        grouped.entry(edge.source).or_default().push(edge);
    }
    grouped
}

fn branch_is_impossible(
    branch: BranchOutcome,
    edges: &[ConditionalEdge],
    dead: &BTreeSet<(NodeId, NodeId, BranchOutcome)>,
    interval: Option<&ConditionOutcome>,
) -> bool {
    interval.is_some_and(ConditionOutcome::is_impossible)
        || branch_edges_all_dead(branch, edges, dead)
}

fn branch_edges_all_dead(
    branch: BranchOutcome,
    edges: &[ConditionalEdge],
    dead: &BTreeSet<(NodeId, NodeId, BranchOutcome)>,
) -> bool {
    let mut found = false;
    for edge in edges.iter().filter(|edge| edge.branch == branch) {
        found = true;
        if !dead.contains(&(edge.source, edge.target, edge.branch)) {
            return false;
        }
    }
    found
}

fn boolean_constant(constants: &BTreeMap<NodeId, ConstValue>, node: NodeId) -> Option<bool> {
    match constants.get(&node) {
        Some(ConstValue::Bool(value)) => Some(*value),
        _ => None,
    }
}

fn combine_proofs(
    sccp_impossible: bool,
    interval: Option<&ConditionOutcome>,
) -> FeasibilityProofSource {
    let interval_impossible = interval.is_some_and(ConditionOutcome::is_impossible);
    match (sccp_impossible, interval_impossible) {
        (true, true) => FeasibilityProofSource::SccpAndIntervals,
        (true, false) => FeasibilityProofSource::Sccp,
        (false, true) => interval.expect("an interval proof exists").proof_source(),
        (false, false) => unreachable!("a finding always has a proof"),
    }
}

fn build_condition_arena(
    cpg: &CodePropertyGraph,
    root: NodeId,
    cap: usize,
) -> Result<ConditionArena, ArenaError> {
    let mut expressions = BTreeMap::new();
    let mut active = BTreeSet::new();
    let mut discovered = BTreeSet::new();
    let mut frames = vec![ArenaFrame::Enter(root)];
    while let Some(frame) = frames.pop() {
        match frame {
            ArenaFrame::Enter(node) => {
                if expressions.contains_key(&node) {
                    continue;
                }
                if active.contains(&node) {
                    return Err(ArenaError::Cycle(node));
                }
                if !discovered.insert(node) {
                    continue;
                }
                let needed = discovered.len();
                if needed > cap {
                    return Err(ArenaError::Cap { needed });
                }
                let Some(payload) = cpg.node(node) else {
                    return Err(ArenaError::Missing(node));
                };
                active.insert(node);
                frames.push(ArenaFrame::Exit(node));
                let children = semantic_children(cpg, node, &payload.kind);
                for child in children.into_iter().rev() {
                    frames.push(ArenaFrame::Enter(child));
                }
            }
            ArenaFrame::Exit(node) => {
                active.remove(&node);
                expressions.insert(node, condition_expr(cpg, node));
            }
        }
    }
    Ok(ConditionArena { expressions })
}

fn semantic_children(cpg: &CodePropertyGraph, node: NodeId, kind: &CpgNodeKind) -> Vec<NodeId> {
    let children = cpg.ast_children(node);
    match kind {
        CpgNodeKind::UnaryOp { operator } if matches!(&**operator, "!" | "not") => {
            children.first().copied().into_iter().collect()
        }
        CpgNodeKind::BinaryOp { operator } if matches!(&**operator, "&&" | "and" | "||" | "or") => {
            children.into_iter().take(2).collect()
        }
        _ => Vec::new(),
    }
}

fn condition_expr(cpg: &CodePropertyGraph, node: NodeId) -> ConditionExpr {
    let Some(payload) = cpg.node(node) else {
        return ConditionExpr::Unsupported;
    };
    match &payload.kind {
        CpgNodeKind::Literal {
            kind: LiteralKind::Bool(value),
        } => ConditionExpr::Boolean(*value),
        CpgNodeKind::UnaryOp { operator } if matches!(&**operator, "!" | "not") => cpg
            .ast_children(node)
            .first()
            .copied()
            .map_or(ConditionExpr::Unsupported, ConditionExpr::Not),
        CpgNodeKind::BinaryOp { operator } if matches!(&**operator, "&&" | "and" | "||" | "or") => {
            let children = cpg.ast_children(node);
            let [left, right, ..] = children.as_slice() else {
                return ConditionExpr::Unsupported;
            };
            if matches!(&**operator, "&&" | "and") {
                ConditionExpr::And(*left, *right)
            } else {
                ConditionExpr::Or(*left, *right)
            }
        }
        CpgNodeKind::BinaryOp { operator } => normalized_predicate(cpg, node, operator)
            .map_or(ConditionExpr::Unsupported, ConditionExpr::Predicate),
        _ => ConditionExpr::Unsupported,
    }
}

fn normalized_predicate(
    cpg: &CodePropertyGraph,
    node: NodeId,
    operator: &Arc<str>,
) -> Option<Predicate> {
    if !matches!(&**operator, "<" | "<=" | ">" | ">=" | "==" | "!=") {
        return None;
    }
    let children = cpg.ast_children(node);
    let [left, right, ..] = children.as_slice() else {
        return None;
    };
    match (&cpg.node(*left)?.kind, &cpg.node(*right)?.kind) {
        (
            CpgNodeKind::Identifier { name, .. },
            CpgNodeKind::Literal {
                kind: LiteralKind::Integer(constant),
            },
        ) => Some(Predicate {
            node,
            variable: name.clone(),
            operator: operator.clone(),
            constant: i128::from(*constant),
        }),
        (
            CpgNodeKind::Literal {
                kind: LiteralKind::Integer(constant),
            },
            CpgNodeKind::Identifier { name, .. },
        ) => Some(Predicate {
            node,
            variable: name.clone(),
            operator: reverse_comparison(operator)?.into(),
            constant: i128::from(*constant),
        }),
        _ => None,
    }
}

fn evaluate_condition(
    arena: &ConditionArena,
    root: NodeId,
    desired: bool,
    entry: &[(Arc<str>, Interval)],
    constants: &BTreeMap<NodeId, ConstValue>,
    config: &InfeasiblePathConfig,
) -> Result<ConditionOutcome, MachineError> {
    let initial = MachineState {
        intervals: entry.iter().cloned().collect(),
        predicates: Vec::new(),
        pending: vec![(root, desired)],
        used_sccp: false,
    };
    let mut queue = VecDeque::from([initial]);
    let mut generated = 1usize;
    let mut steps = 0usize;
    let mut outcome = ConditionOutcome {
        possible: false,
        unknown_nodes: BTreeSet::new(),
        failures: Vec::new(),
    };

    while let Some(mut state) = queue.pop_front() {
        loop {
            let Some((node, truth)) = state.pending.pop() else {
                outcome.possible = true;
                break;
            };
            steps = steps.saturating_add(1);
            if steps > config.max_condition_steps {
                return Err(MachineError::Steps);
            }
            if let Some(value) = boolean_constant(constants, node) {
                state.used_sccp = true;
                if value == truth {
                    continue;
                }
                outcome.failures.push(FailedAlternative {
                    predicates: state.predicates,
                    used_sccp: true,
                });
                break;
            }
            let Some(expression) = arena.expressions.get(&node) else {
                outcome.unknown_nodes.insert(node);
                outcome.possible = true;
                break;
            };
            match expression {
                ConditionExpr::Boolean(value) => {
                    if *value == truth {
                        continue;
                    }
                    outcome.failures.push(FailedAlternative {
                        predicates: state.predicates,
                        used_sccp: state.used_sccp,
                    });
                    break;
                }
                ConditionExpr::Not(child) => state.pending.push((*child, !truth)),
                ConditionExpr::And(left, right) if truth => {
                    state.pending.push((*right, true));
                    state.pending.push((*left, true));
                }
                ConditionExpr::And(left, right) => {
                    let mut left_false = state.clone();
                    left_false.pending.push((*left, false));
                    state.pending.push((*right, false));
                    state.pending.push((*left, true));
                    enqueue_alternatives(
                        &mut queue,
                        [left_false, state],
                        &mut generated,
                        config.max_condition_states,
                    )?;
                    break;
                }
                ConditionExpr::Or(left, right) if !truth => {
                    state.pending.push((*right, false));
                    state.pending.push((*left, false));
                }
                ConditionExpr::Or(left, right) => {
                    let mut left_true = state.clone();
                    left_true.pending.push((*left, true));
                    state.pending.push((*right, true));
                    state.pending.push((*left, false));
                    enqueue_alternatives(
                        &mut queue,
                        [left_true, state],
                        &mut generated,
                        config.max_condition_states,
                    )?;
                    break;
                }
                ConditionExpr::Predicate(predicate) => {
                    let Some(regions) =
                        comparison_regions(&predicate.operator, predicate.constant, truth)
                    else {
                        outcome.unknown_nodes.insert(node);
                        outcome.possible = true;
                        break;
                    };
                    if regions.is_empty() {
                        let mut predicates = state.predicates;
                        predicates.push(predicate_evidence(predicate, truth, Vec::new()));
                        outcome.failures.push(FailedAlternative {
                            predicates,
                            used_sccp: state.used_sccp,
                        });
                        break;
                    }
                    let mut feasible = Vec::new();
                    for region in regions {
                        let current = state
                            .intervals
                            .get(&predicate.variable)
                            .copied()
                            .unwrap_or_else(Interval::top);
                        let mut branch = state.clone();
                        branch
                            .predicates
                            .push(predicate_evidence(predicate, truth, vec![region]));
                        if let Some(refined) = current.meet(region) {
                            branch.intervals.insert(predicate.variable.clone(), refined);
                            feasible.push(branch);
                        } else {
                            outcome.failures.push(FailedAlternative {
                                predicates: branch.predicates,
                                used_sccp: branch.used_sccp,
                            });
                        }
                    }
                    if feasible.is_empty() {
                        break;
                    }
                    if feasible.len() == 1 {
                        state = feasible.pop().expect("one feasible interval state");
                        continue;
                    }
                    enqueue_alternatives(
                        &mut queue,
                        feasible,
                        &mut generated,
                        config.max_condition_states,
                    )?;
                    break;
                }
                ConditionExpr::Unsupported => {
                    outcome.unknown_nodes.insert(node);
                    outcome.possible = true;
                    break;
                }
            }
        }
    }
    Ok(outcome)
}

fn enqueue_alternatives(
    queue: &mut VecDeque<MachineState>,
    alternatives: impl IntoIterator<Item = MachineState>,
    generated: &mut usize,
    cap: usize,
) -> Result<(), MachineError> {
    let alternatives: Vec<_> = alternatives.into_iter().collect();
    let additional = alternatives.len().saturating_sub(1);
    let needed = generated.saturating_add(additional);
    if needed > cap {
        return Err(MachineError::States { needed });
    }
    *generated = needed;
    queue.extend(alternatives);
    Ok(())
}

fn predicate_evidence(
    predicate: &Predicate,
    required_truth: bool,
    admissible: Vec<Interval>,
) -> IntervalPredicateEvidence {
    IntervalPredicateEvidence {
        node: predicate.node,
        variable: predicate.variable.clone(),
        operator: predicate.operator.clone(),
        constant: predicate.constant,
        required_truth,
        admissible,
    }
}

fn record_arena_issue(
    issues: &mut BTreeSet<InfeasiblePathIssue>,
    function: NodeId,
    condition: NodeId,
    error: ArenaError,
    config: &InfeasiblePathConfig,
) {
    issues.insert(match error {
        ArenaError::Missing(node) => InfeasiblePathIssue::MissingConditionNode {
            function,
            condition,
            node,
        },
        ArenaError::Cycle(node) => InfeasiblePathIssue::CyclicCondition {
            function,
            condition,
            node,
        },
        ArenaError::Cap { needed } => InfeasiblePathIssue::ConditionNodeCap {
            function,
            condition,
            needed,
            cap: config.max_condition_nodes,
        },
    });
}

fn record_machine_issue(
    issues: &mut BTreeSet<InfeasiblePathIssue>,
    function: NodeId,
    condition: NodeId,
    error: MachineError,
    config: &InfeasiblePathConfig,
) {
    issues.insert(match error {
        MachineError::States { needed } => InfeasiblePathIssue::ConditionStateCap {
            function,
            condition,
            needed,
            cap: config.max_condition_states,
        },
        MachineError::Steps => InfeasiblePathIssue::ConditionStepCap {
            function,
            condition,
            cap: config.max_condition_steps,
        },
    });
}

fn compare_contradictions(
    left: &IntervalContradiction,
    right: &IntervalContradiction,
) -> std::cmp::Ordering {
    let keys = |contradiction: &IntervalContradiction| {
        contradiction
            .predicates
            .iter()
            .map(|predicate| {
                (
                    predicate.node,
                    predicate.variable.clone(),
                    predicate.operator.clone(),
                    predicate.constant,
                    predicate.required_truth,
                )
            })
            .collect::<Vec<_>>()
    };
    keys(left).cmp(&keys(right))
}

#[cfg(test)]
#[path = "infeasible/tests.rs"]
mod tests;
