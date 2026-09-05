//! Proof-qualified semantic expression flow and cross-revision diff evidence.
//!
//! Equality and control-flow facts remain separate: Replete supplies
//! independently replayed equality proofs, while libcpg's monotone solver
//! supplies must-availability and must-anticipability. The module reports
//! advisory PRE/CSE candidates and four-way semantic diffs; it never mutates a
//! CPG or changes the legacy structural-diff taxonomy.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

#[cfg(feature = "serde")]
use serde::Serialize;

use crate::{CodePropertyGraph, CpgNodeKind, NodeId};

use super::dataflow::{
    solve, Confluence, DataflowAnalysis, DataflowSolution, Direction, MustBits, SolverConfig,
};
use super::expression_equivalence::{
    analyze_expression_equivalence, candidate_unsupported_reason, compare_expression_pair,
    evaluate_closed_ir, ClosedExpressionEvaluation, ExpressionPairComparison,
};
use super::{
    block_cfg, cfg_projection, dominator_tree, AlphaBindingCorrespondence, AnalysisError, BlockCfg,
    BlockId, DominatorTree, ExpressionEquivalenceCandidate, ExpressionEquivalenceCompletion,
    ExpressionEquivalenceKind, ExpressionEquivalenceLimits, ExpressionEquivalenceReport,
    ExpressionFactKind, ExpressionIrId, ExpressionLiteral, ExpressionProofError, ExpressionType,
    SemanticProfile, UnsupportedExpressionReason,
};

/// Hard ceiling on path-search and candidate-construction work.
pub const MAX_SEMANTIC_FLOW_WORK: usize = 64 * 1024 * 1024;
/// Hard ceiling on iterative closed-expression evaluation work per revision.
pub const MAX_SEMANTIC_DIFF_EVALUATION_WORK: usize = 4 * 1024 * 1024;

/// Resource limits for semantic PRE/CSE evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SemanticExpressionFlowLimits {
    /// Limits governing the proof-producing equivalence pass.
    pub equivalence: ExpressionEquivalenceLimits,
    /// Maximum worklist extractions for any one block and direction.
    pub max_visits_per_block: u32,
    /// Maximum exact path-search and report-construction work.
    pub max_candidate_work: usize,
}

impl Default for SemanticExpressionFlowLimits {
    fn default() -> Self {
        Self {
            equivalence: ExpressionEquivalenceLimits::default(),
            max_visits_per_block: 256,
            max_candidate_work: 8 * 1024 * 1024,
        }
    }
}

impl SemanticExpressionFlowLimits {
    fn validate(self) -> Result<Self, SemanticExpressionFlowError> {
        if self.max_visits_per_block == 0 {
            return Err(SemanticExpressionFlowError::InvalidLimit(
                "max_visits_per_block",
            ));
        }
        if self.max_candidate_work == 0 || self.max_candidate_work > MAX_SEMANTIC_FLOW_WORK {
            return Err(SemanticExpressionFlowError::InvalidLimit(
                "max_candidate_work",
            ));
        }
        Ok(self)
    }
}

/// One deterministic exact semantic class.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SemanticExpressionClass {
    /// Smallest stable source identifier in the class.
    pub representative: NodeId,
    /// Reachable expression occurrences in ascending source order.
    pub occurrences: Vec<NodeId>,
    /// Union of exact resolved binding dependencies in canonical order.
    pub dependencies: Vec<super::BindingIdentity>,
}

/// Why an equality candidate did not enter semantic dataflow.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub enum SemanticFlowExclusionReason {
    /// The proof adapter rejected the candidate.
    Equivalence(UnsupportedExpressionReason),
    /// The source is outside the selected function's reachable CFG.
    UnreachableSource,
}

/// One candidate excluded from flow composition.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SemanticFlowExclusion {
    /// Stable candidate source.
    pub source: NodeId,
    /// Exact exclusion reason.
    pub reason: SemanticFlowExclusionReason,
}

/// Completion of one monotone flow direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub enum SemanticDataflowCompletion {
    /// The deterministic work queue reached its fixed point.
    Complete,
    /// A block exceeded the configured visit cap.
    IterationLimited,
}

impl SemanticDataflowCompletion {
    /// Returns whether absence from this direction is meaningful.
    pub const fn is_complete(self) -> bool {
        matches!(self, Self::Complete)
    }
}

/// Aggregate terminal cause for semantic-flow candidate discovery.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub enum SemanticExpressionFlowCompletion {
    /// Every input, proof, flow direction, and candidate check completed.
    Complete,
    /// One or more inputs were explicitly excluded.
    UnsupportedInputs,
    /// Equality saturation, provenance, or pair construction was limited.
    EquivalenceLimited(ExpressionEquivalenceCompletion),
    /// Forward must-availability did not reach a fixed point.
    AvailableDataflowLimited,
    /// Backward must-anticipability did not reach a fixed point.
    VeryBusyDataflowLimited,
    /// Candidate path checks exhausted their explicit work cap.
    CandidateWorkLimited,
}

impl SemanticExpressionFlowCompletion {
    /// Returns whether absence from every candidate collection is meaningful.
    pub const fn is_complete(self) -> bool {
        matches!(self, Self::Complete)
    }
}

/// One proof-qualified common-subexpression-elimination candidate.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SemanticCseCandidate {
    /// Dominating expression occurrence whose value could be reused.
    pub producer: NodeId,
    /// Later equivalent occurrence.
    pub use_node: NodeId,
    /// Stable semantic-class representative.
    pub class: NodeId,
    /// Index of the exact replayable proof in the nested equivalence report.
    pub proof_pair: usize,
}

/// One proof-qualified partial-redundancy-elimination placement candidate.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SemanticPreCandidate {
    /// Stable semantic-class representative.
    pub class: NodeId,
    /// Nearest common dominator at which the class is anticipable.
    pub placement: NodeId,
    /// Reachable occurrences dominated by `placement`.
    pub occurrences: Vec<NodeId>,
    /// Exact proof edges spanning the class in the nested report.
    pub proof_pairs: Vec<usize>,
}

/// Deterministic work accounting for one semantic-flow request.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SemanticExpressionFlowMetrics {
    /// Exact semantic classes admitted to flow.
    pub classes: usize,
    /// Reachable admitted occurrences.
    pub occurrences: usize,
    /// Resolved dependency identities across classes.
    pub dependencies: usize,
    /// Explicit candidate/path-check work charged.
    pub candidate_work: usize,
    /// CSE candidates emitted.
    pub cse_candidates: usize,
    /// PRE candidates emitted.
    pub pre_candidates: usize,
}

/// Complete additive report for proof-qualified semantic flow.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SemanticExpressionFlowReport {
    /// Selected function.
    pub function: NodeId,
    /// Aggregate completion; positives remain valid when this is limited.
    pub completion: SemanticExpressionFlowCompletion,
    /// Forward must-analysis completion.
    pub available_completion: SemanticDataflowCompletion,
    /// Backward must-analysis completion.
    pub very_busy_completion: SemanticDataflowCompletion,
    /// Canonical semantic classes.
    pub classes: Vec<SemanticExpressionClass>,
    /// Class representatives available immediately before each reachable node.
    pub available_in: BTreeMap<NodeId, Vec<NodeId>>,
    /// Class representatives very busy immediately after each reachable node.
    pub busy_out: BTreeMap<NodeId, Vec<NodeId>>,
    /// Conservative proof-and-flow-qualified CSE candidates.
    pub cse_candidates: Vec<SemanticCseCandidate>,
    /// Conservative proof-and-flow-qualified PRE candidates.
    pub pre_candidates: Vec<SemanticPreCandidate>,
    /// Candidates excluded from semantic flow.
    pub excluded: Vec<SemanticFlowExclusion>,
    /// Underlying proof-producing equality report.
    pub equivalence: ExpressionEquivalenceReport,
    /// Exact deterministic accounting.
    pub metrics: SemanticExpressionFlowMetrics,
    /// Limits governing the request.
    pub limits: SemanticExpressionFlowLimits,
}

/// Semantic-flow or cross-revision proof failure.
#[derive(Debug, Error)]
pub enum SemanticExpressionFlowError {
    /// A configured limit is zero or exceeds its hard safety ceiling.
    #[error("invalid semantic-expression-flow limit: {0}")]
    InvalidLimit(&'static str),
    /// A structural analysis failed.
    #[error(transparent)]
    Analysis(#[from] AnalysisError),
    /// Equality construction or independent proof replay failed.
    #[error(transparent)]
    Proof(#[from] ExpressionProofError),
    /// Internal typed evidence violated a checked construction invariant.
    #[error("semantic-expression-flow invariant failed: {0}")]
    Invariant(&'static str),
    /// Equality proof and exact closed evaluation disagree.
    #[error("replayed equality proof contradicts exact closed evaluation")]
    ContradictoryEvidence,
}

#[derive(Clone, Debug)]
struct SemanticTransfer {
    width: usize,
    generated: BTreeMap<NodeId, Vec<u32>>,
    killed: BTreeMap<NodeId, Vec<u32>>,
}

impl SemanticTransfer {
    fn transfer_node(&self, node: NodeId, state: &mut MustBits) {
        for &class in self.killed.get(&node).map_or(&[][..], Vec::as_slice) {
            state.remove(class);
        }
        for &class in self.generated.get(&node).map_or(&[][..], Vec::as_slice) {
            state.insert(class);
        }
    }
}

struct SemanticAvailable<'transfer> {
    transfer: &'transfer SemanticTransfer,
}

impl DataflowAnalysis for SemanticAvailable<'_> {
    type Domain = MustBits;

    const NAME: &'static str = "semantic available expressions";
    const DIRECTION: Direction = Direction::Forward;
    const CONFLUENCE: Confluence = Confluence::Must;

    fn bottom(&self, _cfg: &BlockCfg) -> Self::Domain {
        MustBits::universe(self.transfer.width)
    }

    fn boundary(&self, _cfg: &BlockCfg) -> Self::Domain {
        MustBits::empty(self.transfer.width)
    }

    fn transfer_block(
        &self,
        _cpg: &CodePropertyGraph,
        cfg: &BlockCfg,
        block: BlockId,
        state: &mut Self::Domain,
    ) {
        let block = cfg
            .block(block)
            .expect("the solver transfers only represented blocks");
        for &node in &block.nodes {
            self.transfer.transfer_node(node, state);
        }
    }
}

struct SemanticVeryBusy<'transfer> {
    transfer: &'transfer SemanticTransfer,
}

impl DataflowAnalysis for SemanticVeryBusy<'_> {
    type Domain = MustBits;

    const NAME: &'static str = "semantic very-busy expressions";
    const DIRECTION: Direction = Direction::Backward;
    const CONFLUENCE: Confluence = Confluence::Must;

    fn bottom(&self, _cfg: &BlockCfg) -> Self::Domain {
        MustBits::universe(self.transfer.width)
    }

    fn boundary(&self, _cfg: &BlockCfg) -> Self::Domain {
        MustBits::empty(self.transfer.width)
    }

    fn transfer_block(
        &self,
        _cpg: &CodePropertyGraph,
        cfg: &BlockCfg,
        block: BlockId,
        state: &mut Self::Domain,
    ) {
        let block = cfg
            .block(block)
            .expect("the solver transfers only represented blocks");
        for &node in block.nodes.iter().rev() {
            self.transfer.transfer_node(node, state);
        }
    }
}

#[derive(Debug)]
struct WorkBudget {
    used: usize,
    cap: usize,
}

impl WorkBudget {
    const fn new(cap: usize) -> Self {
        Self { used: 0, cap }
    }

    fn charge(&mut self, amount: usize) -> bool {
        let Some(next) = self.used.checked_add(amount) else {
            return false;
        };
        if next > self.cap {
            return false;
        }
        self.used = next;
        true
    }
}

/// Combines exact proof evidence with forward/backward must dataflow.
///
/// Positive candidates remain sound even when `completion` is limited;
/// absence is meaningful only when [`SemanticExpressionFlowCompletion::Complete`].
pub fn analyze_semantic_expression_flow(
    cpg: &CodePropertyGraph,
    function: NodeId,
    analysis_domain: NodeId,
    profile: &SemanticProfile,
    candidates: &[ExpressionEquivalenceCandidate<'_>],
    limits: SemanticExpressionFlowLimits,
) -> Result<SemanticExpressionFlowReport, SemanticExpressionFlowError> {
    let limits = limits.validate()?;
    let equivalence =
        analyze_expression_equivalence(analysis_domain, profile, candidates, limits.equivalence)?;
    let cfg = block_cfg(cpg, function)?;
    let projection = cfg_projection(cpg, function).map_err(AnalysisError::from)?;
    let dominators = dominator_tree(&projection, function)?;

    let mut source_counts = BTreeMap::<NodeId, usize>::new();
    for candidate in candidates {
        *source_counts.entry(candidate.source).or_default() += 1;
    }
    let mut ordered = candidates.to_vec();
    ordered.sort_unstable_by_key(|candidate| (candidate.source, candidate.binding_context));
    let mut admitted = BTreeMap::new();
    let mut excluded = BTreeMap::new();
    for candidate in ordered {
        let duplicate = source_counts.get(&candidate.source).copied().unwrap_or(0) > 1;
        if let Some(reason) = candidate_unsupported_reason(candidate, profile, duplicate) {
            excluded
                .entry(candidate.source)
                .or_insert(SemanticFlowExclusionReason::Equivalence(reason));
        } else if !cfg.block_of_node.contains_key(&candidate.source) {
            excluded
                .entry(candidate.source)
                .or_insert(SemanticFlowExclusionReason::UnreachableSource);
        } else {
            admitted.entry(candidate.source).or_insert(candidate);
        }
    }

    let mut exact_edges = BTreeMap::<(NodeId, NodeId), usize>::new();
    let mut adjacency = BTreeMap::<NodeId, Vec<NodeId>>::new();
    for (pair_index, pair) in equivalence.pairs.iter().enumerate() {
        if pair.kind != ExpressionEquivalenceKind::Exact
            || !admitted.contains_key(&pair.left)
            || !admitted.contains_key(&pair.right)
        {
            continue;
        }
        pair.proof.replay()?;
        exact_edges.insert((pair.left, pair.right), pair_index);
        adjacency.entry(pair.left).or_default().push(pair.right);
        adjacency.entry(pair.right).or_default().push(pair.left);
    }
    for neighbours in adjacency.values_mut() {
        neighbours.sort_unstable();
        neighbours.dedup();
    }

    let mut classes = Vec::new();
    let mut source_to_class = BTreeMap::<NodeId, u32>::new();
    let mut seen = BTreeSet::new();
    for &source in admitted.keys() {
        if !seen.insert(source) {
            continue;
        }
        let mut pending = vec![source];
        let mut occurrences = Vec::new();
        while let Some(current) = pending.pop() {
            occurrences.push(current);
            if let Some(neighbours) = adjacency.get(&current) {
                for &neighbour in neighbours.iter().rev() {
                    if seen.insert(neighbour) {
                        pending.push(neighbour);
                    }
                }
            }
        }
        occurrences.sort_unstable();
        let mut dependencies = BTreeSet::new();
        for occurrence in &occurrences {
            let candidate = admitted
                .get(occurrence)
                .expect("class traversal starts from admitted candidates");
            for index in 0..candidate.ir.metrics().nodes {
                let id = ExpressionIrId(u32::try_from(index).map_err(|_| {
                    SemanticExpressionFlowError::Invariant("IR identifier overflow")
                })?);
                let node = candidate
                    .ir
                    .node(id)
                    .ok_or(SemanticExpressionFlowError::Invariant(
                        "complete IR node is missing",
                    ))?;
                if let ExpressionFactKind::Binding(binding) = node.kind {
                    dependencies.insert(binding);
                }
            }
        }
        let class_id = u32::try_from(classes.len()).map_err(|_| {
            SemanticExpressionFlowError::Invariant("semantic class identifier overflow")
        })?;
        for &occurrence in &occurrences {
            source_to_class.insert(occurrence, class_id);
        }
        classes.push(SemanticExpressionClass {
            representative: occurrences[0],
            occurrences,
            dependencies: dependencies.into_iter().collect(),
        });
    }

    let transfer = build_transfer(cpg, &cfg, &classes, &source_to_class, &admitted);
    let solver_config = SolverConfig {
        max_visits_per_block: limits.max_visits_per_block,
        widen_after: None,
        narrow_passes: 0,
    };
    let (available_completion, available_solution) = solve_direction(solve(
        cpg,
        &cfg,
        &SemanticAvailable {
            transfer: &transfer,
        },
        &solver_config,
        None,
    ))?;
    let (very_busy_completion, busy_solution) = solve_direction(solve(
        cpg,
        &cfg,
        &SemanticVeryBusy {
            transfer: &transfer,
        },
        &solver_config,
        None,
    ))?;

    let available_states = available_solution
        .as_ref()
        .map(|solution| forward_node_inputs(&cfg, &transfer, solution))
        .unwrap_or_default();
    let busy_states = busy_solution
        .as_ref()
        .map(|solution| backward_node_outputs(&cfg, &transfer, solution))
        .unwrap_or_default();
    let available_in = public_states(&available_states, &classes);
    let busy_out = public_states(&busy_states, &classes);

    let mut budget = WorkBudget::new(limits.max_candidate_work);
    let mut candidate_limited = false;
    let mut cse_candidates = Vec::new();
    if available_completion.is_complete() {
        for (&(left, right), &proof_pair) in &exact_edges {
            if !budget.charge(1) {
                candidate_limited = true;
                break;
            }
            let Some(&class) = source_to_class.get(&left) else {
                continue;
            };
            let (producer, use_node) = if dominators.dominates(left, right) {
                (left, right)
            } else if dominators.dominates(right, left) {
                (right, left)
            } else {
                continue;
            };
            if !available_states
                .get(&use_node)
                .is_some_and(|state| state.contains(class))
            {
                continue;
            }
            match has_intervening_kill(
                &projection,
                &transfer,
                class,
                producer,
                use_node,
                &mut budget,
            ) {
                Some(true) => continue,
                Some(false) => {}
                None => {
                    candidate_limited = true;
                    break;
                }
            }
            cse_candidates.push(SemanticCseCandidate {
                producer,
                use_node,
                class: classes[class as usize].representative,
                proof_pair,
            });
        }
    }
    cse_candidates.sort_unstable_by_key(|candidate| {
        (
            candidate.producer,
            candidate.use_node,
            candidate.class,
            candidate.proof_pair,
        )
    });
    cse_candidates.dedup();

    let mut pre_candidates = Vec::new();
    if very_busy_completion.is_complete() && !candidate_limited {
        for (class_index, class) in classes.iter().enumerate() {
            if class.occurrences.len() < 2 || !budget.charge(class.occurrences.len()) {
                candidate_limited = true;
                break;
            }
            let Some(placement) = nearest_common_dominator(&dominators, &class.occurrences) else {
                continue;
            };
            let Some(mut before) = busy_solution.as_ref().and_then(|solution| {
                solution.state_at(&cfg, placement, Direction::Backward, |node, state| {
                    transfer.transfer_node(node, state);
                })
            }) else {
                continue;
            };
            transfer.transfer_node(placement, &mut before);
            if !before.contains(class_index as u32) {
                continue;
            }
            let occurrence_set: BTreeSet<_> = class.occurrences.iter().copied().collect();
            let proof_pairs = exact_edges
                .iter()
                .filter(|((left, right), _)| {
                    occurrence_set.contains(left) && occurrence_set.contains(right)
                })
                .map(|(_, &index)| index)
                .collect::<Vec<_>>();
            if proof_pairs.len() + 1 < class.occurrences.len() {
                return Err(SemanticExpressionFlowError::Invariant(
                    "semantic class has no spanning proof edges",
                ));
            }
            pre_candidates.push(SemanticPreCandidate {
                class: class.representative,
                placement,
                occurrences: class.occurrences.clone(),
                proof_pairs,
            });
        }
    }
    pre_candidates.sort_unstable_by_key(|candidate| (candidate.class, candidate.placement));

    let excluded = excluded
        .into_iter()
        .map(|(source, reason)| SemanticFlowExclusion { source, reason })
        .collect::<Vec<_>>();
    let completion = aggregate_flow_completion(
        equivalence.exact_completion,
        available_completion,
        very_busy_completion,
        candidate_limited,
        !excluded.is_empty(),
    );
    let metrics = SemanticExpressionFlowMetrics {
        classes: classes.len(),
        occurrences: classes.iter().map(|class| class.occurrences.len()).sum(),
        dependencies: classes.iter().map(|class| class.dependencies.len()).sum(),
        candidate_work: budget.used,
        cse_candidates: cse_candidates.len(),
        pre_candidates: pre_candidates.len(),
    };

    Ok(SemanticExpressionFlowReport {
        function,
        completion,
        available_completion,
        very_busy_completion,
        classes,
        available_in,
        busy_out,
        cse_candidates,
        pre_candidates,
        excluded,
        equivalence,
        metrics,
        limits,
    })
}

fn solve_direction(
    result: Result<DataflowSolution<MustBits>, AnalysisError>,
) -> Result<
    (
        SemanticDataflowCompletion,
        Option<DataflowSolution<MustBits>>,
    ),
    SemanticExpressionFlowError,
> {
    match result {
        Ok(solution) => Ok((SemanticDataflowCompletion::Complete, Some(solution))),
        Err(AnalysisError::IterationLimitExceeded { .. }) => {
            Ok((SemanticDataflowCompletion::IterationLimited, None))
        }
        Err(error) => Err(error.into()),
    }
}

fn aggregate_flow_completion(
    equivalence: ExpressionEquivalenceCompletion,
    available: SemanticDataflowCompletion,
    busy: SemanticDataflowCompletion,
    candidate_limited: bool,
    unsupported: bool,
) -> SemanticExpressionFlowCompletion {
    if equivalence != ExpressionEquivalenceCompletion::Complete {
        if equivalence == ExpressionEquivalenceCompletion::UnsupportedInputs {
            SemanticExpressionFlowCompletion::UnsupportedInputs
        } else {
            SemanticExpressionFlowCompletion::EquivalenceLimited(equivalence)
        }
    } else if !available.is_complete() {
        SemanticExpressionFlowCompletion::AvailableDataflowLimited
    } else if !busy.is_complete() {
        SemanticExpressionFlowCompletion::VeryBusyDataflowLimited
    } else if candidate_limited {
        SemanticExpressionFlowCompletion::CandidateWorkLimited
    } else if unsupported {
        SemanticExpressionFlowCompletion::UnsupportedInputs
    } else {
        SemanticExpressionFlowCompletion::Complete
    }
}

fn build_transfer(
    cpg: &CodePropertyGraph,
    cfg: &BlockCfg,
    classes: &[SemanticExpressionClass],
    source_to_class: &BTreeMap<NodeId, u32>,
    admitted: &BTreeMap<NodeId, ExpressionEquivalenceCandidate<'_>>,
) -> SemanticTransfer {
    let generated = source_to_class
        .iter()
        .map(|(&source, &class)| (source, vec![class]))
        .collect();
    let mut by_definition = BTreeMap::<NodeId, BTreeSet<u32>>::new();
    for (class, facts) in classes.iter().enumerate() {
        for dependency in &facts.dependencies {
            by_definition
                .entry(dependency.definition)
                .or_default()
                .insert(class as u32);
        }
    }
    let all_classes = (0..classes.len() as u32).collect::<Vec<_>>();
    let mut killed = BTreeMap::new();
    for block in &cfg.blocks {
        for &node in &block.nodes {
            let Some(kind) = cpg.node(node).map(|node| &node.kind) else {
                continue;
            };
            let mut at_node = BTreeSet::new();
            let mut kill_all = false;
            match kind {
                CpgNodeKind::Variable { .. } | CpgNodeKind::Parameter { .. } => {
                    if let Some(classes) = by_definition.get(&node) {
                        at_node.extend(classes);
                    }
                }
                CpgNodeKind::Assignment { .. } => {
                    let definition = cpg.ast_children(node).first().and_then(|target| {
                        match cpg.node(*target).map(|node| &node.kind) {
                            Some(CpgNodeKind::Identifier { definition, .. }) => *definition,
                            _ => None,
                        }
                    });
                    if let Some(definition) = definition {
                        if let Some(classes) = by_definition.get(&definition) {
                            at_node.extend(classes);
                        }
                    } else {
                        kill_all = true;
                    }
                }
                CpgNodeKind::Call { .. } if !admitted.contains_key(&node) => {
                    // A call root admitted as a pure ExpressionIr carries an
                    // explicit effect certificate. Unmodeled calls remain a
                    // conservative barrier for every semantic class.
                    kill_all = true;
                }
                CpgNodeKind::Await | CpgNodeKind::Yield => kill_all = true,
                _ => {}
            }
            if kill_all {
                killed.insert(node, all_classes.clone());
            } else if !at_node.is_empty() {
                killed.insert(node, at_node.into_iter().collect());
            }
        }
    }
    SemanticTransfer {
        width: classes.len(),
        generated,
        killed,
    }
}

fn forward_node_inputs(
    cfg: &BlockCfg,
    transfer: &SemanticTransfer,
    solution: &DataflowSolution<MustBits>,
) -> BTreeMap<NodeId, MustBits> {
    let mut states = BTreeMap::new();
    for block in &cfg.blocks {
        let mut state = solution.before[block.id.0 as usize].clone();
        for &node in &block.nodes {
            states.insert(node, state.clone());
            transfer.transfer_node(node, &mut state);
        }
    }
    states
}

fn backward_node_outputs(
    cfg: &BlockCfg,
    transfer: &SemanticTransfer,
    solution: &DataflowSolution<MustBits>,
) -> BTreeMap<NodeId, MustBits> {
    let mut states = BTreeMap::new();
    for block in &cfg.blocks {
        let mut state = solution.after[block.id.0 as usize].clone();
        for &node in block.nodes.iter().rev() {
            states.insert(node, state.clone());
            transfer.transfer_node(node, &mut state);
        }
    }
    states
}

fn public_states(
    states: &BTreeMap<NodeId, MustBits>,
    classes: &[SemanticExpressionClass],
) -> BTreeMap<NodeId, Vec<NodeId>> {
    states
        .iter()
        .map(|(&node, state)| {
            (
                node,
                state
                    .iter()
                    .map(|class| classes[class as usize].representative)
                    .collect(),
            )
        })
        .collect()
}

fn reaches_without_passing(
    graph: &super::GraphProjection,
    start: NodeId,
    target: NodeId,
    barrier: Option<NodeId>,
    budget: &mut WorkBudget,
) -> Option<bool> {
    let (Some(start), Some(target)) = (graph.dense_index(start), graph.dense_index(target)) else {
        return Some(false);
    };
    let barrier = barrier.and_then(|node| graph.dense_index(node));
    let mut seen = vec![false; graph.node_count()];
    let mut pending = vec![start];
    seen[start as usize] = true;
    while let Some(node) = pending.pop() {
        if !budget.charge(1) {
            return None;
        }
        if node == target {
            return Some(true);
        }
        for &successor in graph.successors(node).iter().rev() {
            if Some(successor) == barrier && successor != target {
                continue;
            }
            if !seen[successor as usize] {
                seen[successor as usize] = true;
                pending.push(successor);
            }
        }
    }
    Some(false)
}

fn has_intervening_kill(
    graph: &super::GraphProjection,
    transfer: &SemanticTransfer,
    class: u32,
    producer: NodeId,
    use_node: NodeId,
    budget: &mut WorkBudget,
) -> Option<bool> {
    for (&kill, classes) in &transfer.killed {
        if kill == producer || kill == use_node || classes.binary_search(&class).is_err() {
            continue;
        }
        if reaches_without_passing(graph, producer, kill, Some(use_node), budget)?
            && reaches_without_passing(graph, kill, use_node, None, budget)?
        {
            return Some(true);
        }
    }
    Some(false)
}

fn nearest_common_dominator(dominators: &DominatorTree, occurrences: &[NodeId]) -> Option<NodeId> {
    let first = *occurrences.first()?;
    dominators.dominators_of(first).find(|&candidate| {
        occurrences
            .iter()
            .all(|&node| dominators.dominates(candidate, node))
    })
}

/// Resource limits for one cross-revision semantic comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SemanticExpressionDiffLimits {
    /// Limits for proof-producing cross-revision equality.
    pub equivalence: ExpressionEquivalenceLimits,
    /// Maximum iterative evaluation work for each revision root.
    pub max_evaluation_work: usize,
}

impl Default for SemanticExpressionDiffLimits {
    fn default() -> Self {
        Self {
            equivalence: ExpressionEquivalenceLimits::default(),
            max_evaluation_work: 1_000_000,
        }
    }
}

impl SemanticExpressionDiffLimits {
    fn validate(self) -> Result<Self, SemanticExpressionFlowError> {
        if self.max_evaluation_work == 0
            || self.max_evaluation_work > MAX_SEMANTIC_DIFF_EVALUATION_WORK
        {
            return Err(SemanticExpressionFlowError::InvalidLimit(
                "max_evaluation_work",
            ));
        }
        Ok(self)
    }
}

/// Exact typed value from the independent bounded evaluator.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SemanticExpressionValue {
    /// Canonical literal value.
    pub literal: ExpressionLiteral,
    /// Exact result type under the semantic profile.
    pub value_type: ExpressionType,
}

/// Four-way semantic-diff status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub enum SemanticExpressionDiffStatus {
    /// A Replete equality proof replayed independently.
    ProvenEquivalent,
    /// Exact bounded evaluation produced unequal typed closed values.
    ProvenDifferent,
    /// No decisive witness exists and at least one relevant boundary limited.
    Limited,
    /// Complete checked evidence proves neither equality nor inequality.
    Unknown,
}

/// Explicit reason a semantic diff is incomplete.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub enum SemanticExpressionDiffLimitation {
    /// Equality proof search did not complete.
    Equivalence(ExpressionEquivalenceCompletion),
    /// Old-revision evaluation exhausted its work cap.
    OldEvaluationWork,
    /// New-revision evaluation exhausted its work cap.
    NewEvaluationWork,
}

/// Additive cross-revision semantic evidence.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct SemanticExpressionDiff {
    /// Old-revision expression source.
    pub old: NodeId,
    /// New-revision expression source.
    pub new: NodeId,
    /// Four-way result; never inferred from structural alignment alone.
    pub status: SemanticExpressionDiffStatus,
    /// Equality qualification when a proof exists.
    pub equivalence_kind: Option<ExpressionEquivalenceKind>,
    /// Explicit cross-revision alpha-binding bijection.
    pub alpha_bindings: Vec<AlphaBindingCorrespondence>,
    /// Independently replayable proof for `ProvenEquivalent`.
    pub proof: Option<super::ExpressionEquivalenceProof>,
    /// Exact old value when closed bounded evaluation succeeded.
    pub old_value: Option<SemanticExpressionValue>,
    /// Exact new value when closed bounded evaluation succeeded.
    pub new_value: Option<SemanticExpressionValue>,
    /// Every relevant incomplete boundary in declaration order.
    pub limitations: Vec<SemanticExpressionDiffLimitation>,
    /// Limits governing the comparison.
    pub limits: SemanticExpressionDiffLimits,
}

/// Compares two typed expression roots from independent CPG revisions.
///
/// The source identifiers belong to different coordinate systems and may be
/// numerically equal. The implementation never merges the CPGs and never
/// exposes its private comparison slots.
pub fn analyze_semantic_expression_diff(
    profile: &SemanticProfile,
    old: ExpressionEquivalenceCandidate<'_>,
    new: ExpressionEquivalenceCandidate<'_>,
    limits: SemanticExpressionDiffLimits,
) -> Result<SemanticExpressionDiff, SemanticExpressionFlowError> {
    let limits = limits.validate()?;
    let ExpressionPairComparison {
        completion,
        kind,
        alpha_bindings,
        proof,
    } = compare_expression_pair(profile, old, new, limits.equivalence)?;
    if let Some(proof) = &proof {
        proof.replay()?;
    }

    let old_evaluation = evaluate_closed_ir(old.ir, limits.max_evaluation_work);
    let new_evaluation = evaluate_closed_ir(new.ir, limits.max_evaluation_work);
    let old_value = public_evaluation(&old_evaluation);
    let new_value = public_evaluation(&new_evaluation);
    let evaluated_different = old_value
        .as_ref()
        .zip(new_value.as_ref())
        .is_some_and(|(old, new)| old != new);
    if proof.is_some() && evaluated_different {
        return Err(SemanticExpressionFlowError::ContradictoryEvidence);
    }

    let mut limitations = Vec::new();
    if completion != ExpressionEquivalenceCompletion::Complete {
        limitations.push(SemanticExpressionDiffLimitation::Equivalence(completion));
    }
    if matches!(old_evaluation, ClosedExpressionEvaluation::WorkLimited) {
        limitations.push(SemanticExpressionDiffLimitation::OldEvaluationWork);
    }
    if matches!(new_evaluation, ClosedExpressionEvaluation::WorkLimited) {
        limitations.push(SemanticExpressionDiffLimitation::NewEvaluationWork);
    }
    let status = if proof.is_some() {
        SemanticExpressionDiffStatus::ProvenEquivalent
    } else if evaluated_different {
        SemanticExpressionDiffStatus::ProvenDifferent
    } else if limitations.is_empty() {
        SemanticExpressionDiffStatus::Unknown
    } else {
        SemanticExpressionDiffStatus::Limited
    };

    Ok(SemanticExpressionDiff {
        old: old.source,
        new: new.source,
        status,
        equivalence_kind: kind,
        alpha_bindings,
        proof,
        old_value,
        new_value,
        limitations,
        limits,
    })
}

fn public_evaluation(evaluation: &ClosedExpressionEvaluation) -> Option<SemanticExpressionValue> {
    match evaluation {
        ClosedExpressionEvaluation::Value(value) => Some(SemanticExpressionValue {
            literal: value.literal.clone(),
            value_type: value.value_type,
        }),
        ClosedExpressionEvaluation::Unsupported | ClosedExpressionEvaluation::WorkLimited => None,
    }
}

#[cfg(test)]
#[path = "semantic_expression_flow_tests.rs"]
mod tests;
