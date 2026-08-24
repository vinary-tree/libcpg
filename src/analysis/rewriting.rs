//! Stack-safe dependency-pair termination and critical-pair confluence analysis.
//!
//! MeTTa rules are consumed from the normalized Code Property Graph (CPG), not
//! reparsed from retained source. Terms use a preorder flat arena whose
//! `subtree_end` links make every subterm a contiguous slice. Extraction,
//! unification, substitution, rewriting, and comparison therefore use explicit
//! heap worklists and remain safe for deeply nested input on a small native
//! thread stack.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use libdictenstein::double_array_trie::char::DoubleArrayTrieChar;

use crate::{
    projection_sccs, CodePropertyGraph, CpgEdgeKind, CpgNodeKind, GraphProjection, Language,
    LiteralKind, NodeId, ProjectionKind, RewriteDepEdgeKind,
};

/// Default maximum number of normalized MeTTa rules.
pub const MAX_REWRITE_RULES: usize = 100_000;
/// Default maximum number of flat term nodes across all rules.
pub const MAX_REWRITE_TERM_NODES: usize = 2_000_000;
/// Default maximum number of dependency pairs.
pub const MAX_REWRITE_DEPENDENCY_PAIRS: usize = 2_000_000;
/// Default maximum number of unification worklist steps.
pub const MAX_REWRITE_UNIFICATION_STEPS: usize = 8_000_000;
/// Default maximum number of reported non-joinable critical pairs.
pub const MAX_CRITICAL_PAIRS: usize = 1_000_000;
/// Default maximum number of states explored by bounded joinability searches.
pub const MAX_JOINABILITY_STATES: usize = 1_000_000;

/// Resource limits shared by termination and confluence analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RewriteAnalysisCaps {
    /// Maximum rules retained from the CPG.
    pub max_rules: usize,
    /// Maximum flat term nodes retained across all rules.
    pub max_term_nodes: usize,
    /// Maximum dependency pairs retained.
    pub max_dependency_pairs: usize,
    /// Maximum aggregate unification and occurs-check worklist steps.
    pub max_unification_steps: usize,
    /// Maximum non-joinable critical pairs retained.
    pub max_critical_pairs: usize,
    /// Maximum terms retained by joinability searches.
    pub max_joinability_states: usize,
}

impl Default for RewriteAnalysisCaps {
    fn default() -> Self {
        Self {
            max_rules: MAX_REWRITE_RULES,
            max_term_nodes: MAX_REWRITE_TERM_NODES,
            max_dependency_pairs: MAX_REWRITE_DEPENDENCY_PAIRS,
            max_unification_steps: MAX_REWRITE_UNIFICATION_STEPS,
            max_critical_pairs: MAX_CRITICAL_PAIRS,
            max_joinability_states: MAX_JOINABILITY_STATES,
        }
    }
}

/// Explicit reason a rewrite analysis is incomplete.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum RewriteAnalysisIssue {
    /// The analyzed graph is not a MeTTa CPG.
    UnsupportedLanguage {
        /// Language recorded by the CPG.
        language: String,
    },
    /// A function-shaped rule does not contain exactly one left- and right-hand side.
    MalformedRule {
        /// Function node rejected as a rule.
        rule: NodeId,
    },
    /// A right-hand-side variable lacks unique typed definition evidence.
    UnresolvedVariable {
        /// Identifier node whose binder could not be established.
        node: NodeId,
    },
    /// A right-hand-side variable has conflicting typed definitions.
    AmbiguousVariable {
        /// Identifier node with conflicting evidence.
        node: NodeId,
        /// Sorted candidate binders.
        definitions: Vec<NodeId>,
    },
    /// The rule bound was reached.
    RuleLimitExceeded {
        /// Configured maximum.
        limit: usize,
    },
    /// The aggregate flat-term-node bound was reached.
    TermNodeLimitExceeded {
        /// Configured maximum.
        limit: usize,
    },
    /// The dependency-pair bound was reached.
    DependencyPairLimitExceeded {
        /// Configured maximum.
        limit: usize,
    },
    /// The aggregate unification-work bound was reached.
    UnificationLimitExceeded {
        /// Configured maximum.
        limit: usize,
    },
    /// The critical-pair result bound was reached.
    CriticalPairLimitExceeded {
        /// Configured maximum.
        limit: usize,
    },
    /// The joinability-state bound was reached.
    JoinabilityLimitExceeded {
        /// Configured maximum.
        limit: usize,
    },
}

/// One rule-to-rule dependency pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DependencyPair {
    /// Rule whose right-hand side contains the defined-symbol call.
    pub source_rule: NodeId,
    /// Rule whose left-hand side unifies with the call.
    pub target_rule: NodeId,
    /// Existing CPG node at the root of the right-hand-side call.
    pub call_site: NodeId,
}

/// Deterministic dependency-pair graph and its iterative-Tarjan decomposition.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DpGraph {
    /// Sorted normalized rule-function nodes.
    pub rules: Vec<NodeId>,
    /// Sorted, deduplicated dependency pairs.
    pub pairs: Vec<DependencyPair>,
    /// Rule-to-rule compressed-sparse-row projection.
    pub projection: GraphProjection,
    /// Strongly connected components computed by libcpg's iterative Tarjan engine.
    pub sccs: super::SccDecomposition,
    /// Whether all rules and candidate pairs were processed within the limits.
    pub complete: bool,
    /// Sorted reasons the result is incomplete.
    pub issues: Vec<RewriteAnalysisIssue>,
}

/// A cyclic dependency component oriented by a polynomial reduction pair.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ReductionPairProof {
    /// Sorted rules in the strongly connected component.
    pub rules: Vec<NodeId>,
    /// Number of rewrite rules weakly oriented by the polynomial interpretation.
    pub weakly_oriented_rules: usize,
    /// Number of internal dependency pairs strictly decreased by the proof.
    pub strictly_oriented_pairs: usize,
}

/// Certificate that every cyclic dependency component is well-founded.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TerminationProof {
    /// Per-component polynomial reduction-pair certificates.
    pub reduction_pairs: Vec<ReductionPairProof>,
}

/// Concrete repeating rewrite witness that refutes termination.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NonTerminationWitness {
    /// Rule cycle in execution order, with the first rule repeated at the end.
    pub rule_cycle: Vec<NodeId>,
    /// Dependency-pair call sites realizing the cycle.
    pub call_sites: Vec<NodeId>,
}

/// Evidence explaining why termination was neither proved nor refuted.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TerminationUnknown {
    /// Cyclic components not oriented by the available reduction pair.
    pub unoriented_components: Vec<Vec<NodeId>>,
    /// Extraction or resource-limit issues that invalidate a complete proof.
    pub issues: Vec<RewriteAnalysisIssue>,
}

/// Three-valued termination result.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TerminationVerdict {
    /// Every dependency cycle is strictly oriented by a sound reduction pair.
    Terminates(TerminationProof),
    /// A concrete cycle repeats under root rewriting.
    DoesNotTerminate(NonTerminationWitness),
    /// Available evidence cannot decide the property.
    Unknown(TerminationUnknown),
}

/// One proven non-joinable overlap between two rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CriticalPair {
    /// Rule applied at the enclosing redex.
    pub outer_rule: NodeId,
    /// Rule applied at the overlapping subterm.
    pub inner_rule: NodeId,
    /// Existing CPG node at the overlapped non-variable subterm.
    pub overlap_site: NodeId,
    /// Stable structural fingerprint of the outer-rule reduct.
    pub outer_reduct_fingerprint: u64,
    /// Stable structural fingerprint of the inner-rule reduct.
    pub inner_reduct_fingerprint: u64,
}

/// Complete or explicitly partial critical-pair analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CriticalPairAnalysis {
    /// Sorted, deduplicated proven non-joinable pairs.
    pub pairs: Vec<CriticalPair>,
    /// Whether every overlap and joinability search completed within the limits.
    pub complete: bool,
    /// Sorted reasons the result is incomplete.
    pub issues: Vec<RewriteAnalysisIssue>,
}

/// Transactional RewriteDep-overlay materialization failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RewriteDepOverlayError {
    /// A dependency or conflict source rule is absent from the target CPG.
    #[error("RewriteDep source rule does not exist: {rule:?}")]
    UnknownSource {
        /// Missing source rule.
        rule: NodeId,
    },
    /// A dependency or conflict target rule is absent from the target CPG.
    #[error("RewriteDep target rule does not exist: {rule:?}")]
    UnknownTarget {
        /// Missing target rule.
        rule: NodeId,
    },
    /// A present source endpoint is not a normalized rewrite-rule function.
    #[error("RewriteDep source is not a rule function: {rule:?}")]
    SourceNotRule {
        /// Non-function source endpoint.
        rule: NodeId,
    },
    /// A present target endpoint is not a normalized rewrite-rule function.
    #[error("RewriteDep target is not a rule function: {rule:?}")]
    TargetNotRule {
        /// Non-function target endpoint.
        rule: NodeId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum FlatSymbol {
    Variable(u64),
    Identifier(Arc<str>),
    Operator(Arc<str>),
    Integer(i64),
    Float(u64),
    String(Arc<str>),
    Char(char),
    Bool(bool),
    Null,
    Structural(Arc<str>),
}

impl FlatSymbol {
    fn index_key(&self) -> Option<String> {
        match self {
            Self::Variable(_) => None,
            Self::Identifier(value) => Some(format!("i{}:{value}", value.len())),
            Self::Operator(value) => Some(format!("o{}:{value}", value.len())),
            Self::Integer(value) => Some(format!("n:{value}")),
            Self::Float(value) => Some(format!("f:{value:016x}")),
            Self::String(value) => Some(format!("s{}:{value}", value.len())),
            Self::Char(value) => Some(format!("c:{:08x}", *value as u32)),
            Self::Bool(value) => Some(format!("b:{}", u8::from(*value))),
            Self::Null => Some("z".to_owned()),
            Self::Structural(value) => Some(format!("t{}:{value}", value.len())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FlatNode {
    symbol: FlatSymbol,
    source: NodeId,
    arity: u32,
    subtree_end: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FlatTerm {
    nodes: Vec<FlatNode>,
}

impl FlatTerm {
    fn root(&self) -> usize {
        0
    }

    fn child_roots(&self, root: usize) -> Vec<usize> {
        let mut children = Vec::with_capacity(self.nodes[root].arity as usize);
        let mut next = root + 1;
        for _ in 0..self.nodes[root].arity {
            children.push(next);
            next = self.nodes[next].subtree_end as usize;
        }
        children
    }

    fn subterm_roots(&self, root: usize) -> std::ops::Range<usize> {
        root..self.nodes[root].subtree_end as usize
    }
}

#[derive(Debug, Clone)]
struct Rule {
    node: NodeId,
    lhs: FlatTerm,
    rhs: FlatTerm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum TermSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct TermRef {
    side: TermSide,
    root: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct VariableKey {
    side: TermSide,
    variable: u64,
}

type Substitution = BTreeMap<VariableKey, TermRef>;

#[derive(Default)]
struct VariableNormalizer {
    normalized: BTreeMap<VariableKey, u64>,
    next: u64,
}

impl VariableNormalizer {
    fn preserving_left(term: &FlatTerm) -> Self {
        let mut normalized = BTreeMap::new();
        let mut next = 0u64;
        for node in &term.nodes {
            if let FlatSymbol::Variable(variable) = node.symbol {
                normalized.insert(
                    VariableKey {
                        side: TermSide::Left,
                        variable,
                    },
                    variable,
                );
                next = next.max(variable.saturating_add(1));
            }
        }
        Self { normalized, next }
    }

    fn normalize(&mut self, key: VariableKey) -> u64 {
        if let Some(normalized) = self.normalized.get(&key) {
            return *normalized;
        }
        let normalized = self.next;
        self.next = self
            .next
            .checked_add(1)
            .expect("flat variable domain exhausted");
        self.normalized.insert(key, normalized);
        normalized
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct InternalPair {
    public: DependencyPair,
    source_index: usize,
    target_index: usize,
    rhs_root: usize,
}

#[derive(Debug)]
struct RewriteSystem {
    rules: Vec<Rule>,
    pairs: Vec<InternalPair>,
    extraction_issues: BTreeSet<RewriteAnalysisIssue>,
    issues: BTreeSet<RewriteAnalysisIssue>,
}

struct RuleDiscriminationIndex {
    exact: DoubleArrayTrieChar<Vec<u32>>,
    wildcard: Vec<u32>,
}

/// Builds the dependency-pair graph with default resource limits.
pub fn dependency_pairs(cpg: &CodePropertyGraph) -> DpGraph {
    dependency_pairs_with_caps(cpg, RewriteAnalysisCaps::default())
}

/// Builds the dependency-pair graph with explicit resource limits.
pub fn dependency_pairs_with_caps(cpg: &CodePropertyGraph, caps: RewriteAnalysisCaps) -> DpGraph {
    let system = analyze_system(cpg, caps);
    public_dp_graph(&system)
}

/// Decides termination when a dependency-pair proof or repeating witness exists.
pub fn terminates(cpg: &CodePropertyGraph) -> TerminationVerdict {
    terminates_with_caps(cpg, RewriteAnalysisCaps::default())
}

/// Decides termination with explicit resource limits.
pub fn terminates_with_caps(
    cpg: &CodePropertyGraph,
    caps: RewriteAnalysisCaps,
) -> TerminationVerdict {
    let system = analyze_system(cpg, caps);
    termination_from_system(&system)
}

fn analyze_system(cpg: &CodePropertyGraph, caps: RewriteAnalysisCaps) -> RewriteSystem {
    let mut issues = BTreeSet::new();
    if cpg.language() != Language::MeTTa {
        issues.insert(RewriteAnalysisIssue::UnsupportedLanguage {
            language: cpg.language().name().to_owned(),
        });
        return RewriteSystem {
            rules: Vec::new(),
            pairs: Vec::new(),
            extraction_issues: issues.clone(),
            issues,
        };
    }

    let mut rule_nodes: Vec<_> = cpg.functions().map(|node| node.id).collect();
    rule_nodes = super::scc::heap_sorted(rule_nodes);
    if rule_nodes.len() > caps.max_rules {
        rule_nodes.truncate(caps.max_rules);
        issues.insert(RewriteAnalysisIssue::RuleLimitExceeded {
            limit: caps.max_rules,
        });
    }

    let mut term_nodes = 0usize;
    let mut rules = Vec::with_capacity(rule_nodes.len());
    for rule_node in rule_nodes {
        let Some((lhs_root, rhs_root)) = rule_sides(cpg, rule_node) else {
            issues.insert(RewriteAnalysisIssue::MalformedRule { rule: rule_node });
            continue;
        };
        let Some(lhs) = extract_term(
            cpg,
            lhs_root,
            true,
            &mut term_nodes,
            caps.max_term_nodes,
            &mut issues,
        ) else {
            break;
        };
        let Some(rhs) = extract_term(
            cpg,
            rhs_root,
            false,
            &mut term_nodes,
            caps.max_term_nodes,
            &mut issues,
        ) else {
            break;
        };
        rules.push(Rule {
            node: rule_node,
            lhs,
            rhs,
        });
    }

    let extraction_issues = issues.clone();
    let index = RuleDiscriminationIndex::new(&rules);
    let mut unification_steps = 0usize;
    let mut pair_set = BTreeSet::new();
    let mut pairs = Vec::new();
    'rules: for (source_index, rule) in rules.iter().enumerate() {
        for rhs_root in rule.rhs.subterm_roots(rule.rhs.root()) {
            let Some(key) = rule.rhs.nodes[rhs_root].symbol.index_key() else {
                continue;
            };
            let mut candidates = index.exact.get_value(&key).unwrap_or_default();
            candidates.extend(index.wildcard.iter().copied());
            candidates = super::scc::heap_sorted(candidates);
            candidates.dedup();
            for target_index in candidates {
                let target_index = target_index as usize;
                let target = &rules[target_index];
                match unify(
                    &rule.rhs,
                    rhs_root,
                    &target.lhs,
                    target.lhs.root(),
                    &mut unification_steps,
                    caps.max_unification_steps,
                ) {
                    UnifyResult::Unified(_) => {
                        let public = DependencyPair {
                            source_rule: rule.node,
                            target_rule: target.node,
                            call_site: rule.rhs.nodes[rhs_root].source,
                        };
                        if pair_set.insert(public) {
                            if pairs.len() == caps.max_dependency_pairs {
                                issues.insert(RewriteAnalysisIssue::DependencyPairLimitExceeded {
                                    limit: caps.max_dependency_pairs,
                                });
                                break 'rules;
                            }
                            pairs.push(InternalPair {
                                public,
                                source_index,
                                target_index,
                                rhs_root,
                            });
                        }
                    }
                    UnifyResult::NotUnified => {}
                    UnifyResult::LimitExceeded => {
                        issues.insert(RewriteAnalysisIssue::UnificationLimitExceeded {
                            limit: caps.max_unification_steps,
                        });
                        break 'rules;
                    }
                }
            }
        }
    }
    pairs = super::scc::heap_sorted(pairs);

    RewriteSystem {
        rules,
        pairs,
        extraction_issues,
        issues,
    }
}

fn public_dp_graph(system: &RewriteSystem) -> DpGraph {
    let rules: Vec<_> = system.rules.iter().map(|rule| rule.node).collect();
    let pairs: Vec<_> = system.pairs.iter().map(|pair| pair.public).collect();
    let projection = GraphProjection::from_edges(
        ProjectionKind::RewriteDependency,
        rules.iter().copied(),
        pairs
            .iter()
            .map(|pair| (pair.source_rule, pair.target_rule, 1.0)),
    );
    let sccs = projection_sccs(&projection);
    DpGraph {
        rules,
        pairs,
        projection,
        sccs,
        complete: system.issues.is_empty(),
        issues: system.issues.iter().cloned().collect(),
    }
}

fn termination_from_system(system: &RewriteSystem) -> TerminationVerdict {
    let graph = public_dp_graph(system);
    if !graph.complete {
        return TerminationVerdict::Unknown(TerminationUnknown {
            unoriented_components: graph
                .sccs
                .cyclic_components()
                .map(|component| component.nodes.clone())
                .collect(),
            issues: graph.issues,
        });
    }

    if let Some(witness) = repeating_self_cycle(system) {
        return TerminationVerdict::DoesNotTerminate(witness);
    }

    let all_rules_weakly_oriented = system.rules.iter().all(polynomial_weakly_orients_rule);
    let mut proofs = Vec::new();
    let mut unknown = Vec::new();
    for component in graph.sccs.cyclic_components() {
        let members: BTreeSet<_> = component.nodes.iter().copied().collect();
        let internal: Vec<_> = system
            .pairs
            .iter()
            .filter(|pair| {
                members.contains(&pair.public.source_rule)
                    && members.contains(&pair.public.target_rule)
            })
            .collect();
        if all_rules_weakly_oriented
            && !internal.is_empty()
            && internal
                .iter()
                .all(|pair| polynomial_strictly_orients_pair(system, pair))
        {
            proofs.push(ReductionPairProof {
                rules: component.nodes.clone(),
                weakly_oriented_rules: system.rules.len(),
                strictly_oriented_pairs: internal.len(),
            });
        } else {
            unknown.push(component.nodes.clone());
        }
    }

    if unknown.is_empty() {
        TerminationVerdict::Terminates(TerminationProof {
            reduction_pairs: proofs,
        })
    } else {
        TerminationVerdict::Unknown(TerminationUnknown {
            unoriented_components: unknown,
            issues: Vec::new(),
        })
    }
}

fn repeating_self_cycle(system: &RewriteSystem) -> Option<NonTerminationWitness> {
    for pair in &system.pairs {
        if pair.source_index != pair.target_index {
            continue;
        }
        let rule = &system.rules[pair.source_index];
        if alpha_equivalent(&rule.rhs, pair.rhs_root, &rule.lhs, rule.lhs.root()) {
            return Some(NonTerminationWitness {
                rule_cycle: vec![rule.node, rule.node],
                call_sites: vec![pair.public.call_site],
            });
        }
    }
    None
}

fn polynomial_weakly_orients_rule(rule: &Rule) -> bool {
    let (lhs_constant, lhs_variables) = polynomial(&rule.lhs, rule.lhs.root());
    let (rhs_constant, rhs_variables) = polynomial(&rule.rhs, rule.rhs.root());
    lhs_constant >= rhs_constant
        && polynomial_variable_coefficients_dominate(&lhs_variables, &rhs_variables)
}

fn polynomial_strictly_orients_pair(system: &RewriteSystem, pair: &InternalPair) -> bool {
    let lhs = &system.rules[pair.source_index].lhs;
    let rhs = &system.rules[pair.source_index].rhs;
    let (lhs_constant, lhs_variables) = polynomial(lhs, lhs.root());
    let (rhs_constant, rhs_variables) = polynomial(rhs, pair.rhs_root);
    lhs_constant > rhs_constant
        && polynomial_variable_coefficients_dominate(&lhs_variables, &rhs_variables)
}

fn polynomial_variable_coefficients_dominate(
    greater: &BTreeMap<u64, u64>,
    lesser: &BTreeMap<u64, u64>,
) -> bool {
    lesser
        .iter()
        .all(|(variable, count)| greater.get(variable).copied().unwrap_or(0) >= *count)
}

fn polynomial(term: &FlatTerm, root: usize) -> (u64, BTreeMap<u64, u64>) {
    let mut constant = 0u64;
    let mut variables = BTreeMap::new();
    for node in &term.nodes[term.subterm_roots(root)] {
        match node.symbol {
            FlatSymbol::Variable(variable) => {
                *variables.entry(variable).or_insert(0) += 1;
            }
            _ => constant += 1,
        }
    }
    (constant, variables)
}

fn rule_sides(cpg: &CodePropertyGraph, rule: NodeId) -> Option<(NodeId, NodeId)> {
    let sides: Vec<_> = cpg
        .ast_children(rule)
        .iter()
        .copied()
        .filter(|child| !is_rule_operator(cpg, *child))
        .collect();
    (sides.len() == 2).then(|| (sides[0], sides[1]))
}

fn is_rule_operator(cpg: &CodePropertyGraph, node: NodeId) -> bool {
    matches!(
        cpg.node(node).map(|node| &node.kind),
        Some(CpgNodeKind::Unknown { kind })
            if matches!(kind.as_ref(), "=" | ":=" | "assignment_operator" | "rule_definition_operator")
    )
}

enum ExtractTask {
    Enter(NodeId),
    Exit(usize),
}

fn extract_term(
    cpg: &CodePropertyGraph,
    root: NodeId,
    lhs: bool,
    total_nodes: &mut usize,
    max_nodes: usize,
    issues: &mut BTreeSet<RewriteAnalysisIssue>,
) -> Option<FlatTerm> {
    let mut nodes: Vec<FlatNode> = Vec::new();
    let mut tasks = vec![ExtractTask::Enter(root)];
    while let Some(task) = tasks.pop() {
        match task {
            ExtractTask::Exit(index) => {
                nodes[index].subtree_end = nodes.len() as u32;
            }
            ExtractTask::Enter(source) => {
                if *total_nodes == max_nodes {
                    issues.insert(RewriteAnalysisIssue::TermNodeLimitExceeded { limit: max_nodes });
                    return None;
                }
                let (symbol, children) = semantic_shape(cpg, source, lhs, issues)?;
                let arity = u32::try_from(children.len()).ok()?;
                let index = nodes.len();
                nodes.push(FlatNode {
                    symbol,
                    source,
                    arity,
                    subtree_end: 0,
                });
                *total_nodes += 1;
                tasks.push(ExtractTask::Exit(index));
                for child in children.into_iter().rev() {
                    tasks.push(ExtractTask::Enter(child));
                }
            }
        }
    }
    (!nodes.is_empty()).then_some(FlatTerm { nodes })
}

fn semantic_shape(
    cpg: &CodePropertyGraph,
    source: NodeId,
    lhs: bool,
    issues: &mut BTreeSet<RewriteAnalysisIssue>,
) -> Option<(FlatSymbol, Vec<NodeId>)> {
    let node = cpg.node(source)?;
    let children = cpg.ast_children(source);
    match &node.kind {
        CpgNodeKind::Parameter { .. } => {
            Some((FlatSymbol::Variable(u64::from(source.as_u32())), Vec::new()))
        }
        CpgNodeKind::Identifier { name, definition } if name.as_ref() == "_" => {
            Some((FlatSymbol::Variable(u64::from(source.as_u32())), Vec::new()))
        }
        CpgNodeKind::Identifier { name, definition } if name.starts_with('$') => {
            if lhs {
                return Some((FlatSymbol::Variable(u64::from(source.as_u32())), Vec::new()));
            }
            let mut definitions = BTreeSet::new();
            if let Some(definition) = definition {
                definitions.insert(*definition);
            }
            for definition in cpg.reaching_definitions(source) {
                if cpg
                    .node(definition)
                    .is_some_and(|node| matches!(node.kind, CpgNodeKind::Parameter { .. }))
                {
                    definitions.insert(definition);
                }
            }
            match definitions.len() {
                1 => Some((
                    FlatSymbol::Variable(u64::from(
                        definitions.iter().next().expect("one definition").as_u32(),
                    )),
                    Vec::new(),
                )),
                0 => {
                    issues.insert(RewriteAnalysisIssue::UnresolvedVariable { node: source });
                    Some((FlatSymbol::Variable(u64::from(source.as_u32())), Vec::new()))
                }
                _ => {
                    issues.insert(RewriteAnalysisIssue::AmbiguousVariable {
                        node: source,
                        definitions: definitions.into_iter().collect(),
                    });
                    Some((FlatSymbol::Variable(u64::from(source.as_u32())), Vec::new()))
                }
            }
        }
        CpgNodeKind::Identifier { name, .. } => {
            Some((FlatSymbol::Identifier(name.clone()), Vec::new()))
        }
        CpgNodeKind::Literal { kind } => Some((literal_symbol(kind), Vec::new())),
        CpgNodeKind::Unknown { kind } if children.is_empty() => {
            Some((FlatSymbol::Operator(kind.clone()), Vec::new()))
        }
        CpgNodeKind::Call { .. }
        | CpgNodeKind::If
        | CpgNodeKind::Match
        | CpgNodeKind::Block { .. } => application_shape(cpg, children),
        CpgNodeKind::BinaryOp { operator } | CpgNodeKind::UnaryOp { operator } => {
            Some((FlatSymbol::Operator(operator.clone()), children.to_vec()))
        }
        other => Some((
            FlatSymbol::Structural(Arc::from(format!("{:?}", other.tag()))),
            children.to_vec(),
        )),
    }
}

fn application_shape(
    cpg: &CodePropertyGraph,
    children: &[NodeId],
) -> Option<(FlatSymbol, Vec<NodeId>)> {
    let Some((&head, arguments)) = children.split_first() else {
        return Some((FlatSymbol::Structural(Arc::from("application")), Vec::new()));
    };
    let symbol = atomic_symbol(cpg, head);
    match symbol {
        Some(FlatSymbol::Variable(_)) | None => Some((
            FlatSymbol::Structural(Arc::from("apply")),
            children.to_vec(),
        )),
        Some(symbol) => Some((symbol, arguments.to_vec())),
    }
}

fn atomic_symbol(cpg: &CodePropertyGraph, node: NodeId) -> Option<FlatSymbol> {
    match &cpg.node(node)?.kind {
        CpgNodeKind::Identifier { name, definition } if name.starts_with('$') => Some(
            FlatSymbol::Variable(u64::from(definition.unwrap_or(node).as_u32())),
        ),
        CpgNodeKind::Parameter { .. } => Some(FlatSymbol::Variable(u64::from(node.as_u32()))),
        CpgNodeKind::Identifier { name, .. } => Some(FlatSymbol::Identifier(name.clone())),
        CpgNodeKind::Unknown { kind } => Some(FlatSymbol::Operator(kind.clone())),
        CpgNodeKind::Literal { kind } => Some(literal_symbol(kind)),
        _ => None,
    }
}

fn literal_symbol(kind: &LiteralKind) -> FlatSymbol {
    match kind {
        LiteralKind::Integer(value) => FlatSymbol::Integer(*value),
        LiteralKind::Float(value) => FlatSymbol::Float(value.to_bits()),
        LiteralKind::String(value) | LiteralKind::Regex(value) => FlatSymbol::String(value.clone()),
        LiteralKind::Char(value) => FlatSymbol::Char(*value),
        LiteralKind::Bool(value) => FlatSymbol::Bool(*value),
        LiteralKind::Null => FlatSymbol::Null,
        LiteralKind::Array => FlatSymbol::Structural(Arc::from("array")),
        LiteralKind::Object => FlatSymbol::Structural(Arc::from("object")),
    }
}

impl RuleDiscriminationIndex {
    fn new(rules: &[Rule]) -> Self {
        let mut by_symbol: BTreeMap<String, Vec<u32>> = BTreeMap::new();
        let mut wildcard = Vec::new();
        for (index, rule) in rules.iter().enumerate() {
            let index = u32::try_from(index).expect("rule cap fits the u32 trie value domain");
            match rule.lhs.nodes[rule.lhs.root()].symbol.index_key() {
                Some(key) => by_symbol.entry(key).or_default().push(index),
                None => wildcard.push(index),
            }
        }
        let exact = DoubleArrayTrieChar::from_sorted_terms_with_values(by_symbol);
        Self { exact, wildcard }
    }
}

enum UnifyResult {
    Unified(Substitution),
    NotUnified,
    LimitExceeded,
}

fn unify(
    left: &FlatTerm,
    left_root: usize,
    right: &FlatTerm,
    right_root: usize,
    steps: &mut usize,
    max_steps: usize,
) -> UnifyResult {
    let mut substitution: Substitution = BTreeMap::new();
    let mut work = vec![(
        TermRef {
            side: TermSide::Left,
            root: left_root,
        },
        TermRef {
            side: TermSide::Right,
            root: right_root,
        },
    )];
    while let Some((left_ref, right_ref)) = work.pop() {
        if !take_step(steps, max_steps) {
            return UnifyResult::LimitExceeded;
        }
        let left_ref = dereference(left, right, left_ref, &substitution, steps, max_steps);
        let right_ref = dereference(left, right, right_ref, &substitution, steps, max_steps);
        let (Some(left_ref), Some(right_ref)) = (left_ref, right_ref) else {
            return UnifyResult::LimitExceeded;
        };
        let left_node = term_node(left, right, left_ref);
        let right_node = term_node(left, right, right_ref);
        match (&left_node.symbol, &right_node.symbol) {
            (FlatSymbol::Variable(left_variable), FlatSymbol::Variable(right_variable))
                if (VariableKey {
                    side: left_ref.side,
                    variable: *left_variable,
                }) == (VariableKey {
                    side: right_ref.side,
                    variable: *right_variable,
                }) => {}
            (FlatSymbol::Variable(variable), _) => {
                let variable = VariableKey {
                    side: left_ref.side,
                    variable: *variable,
                };
                match occurs(
                    left,
                    right,
                    variable,
                    right_ref,
                    &substitution,
                    steps,
                    max_steps,
                ) {
                    OccursResult::No => {
                        substitution.insert(variable, right_ref);
                    }
                    OccursResult::Yes => return UnifyResult::NotUnified,
                    OccursResult::LimitExceeded => return UnifyResult::LimitExceeded,
                }
            }
            (_, FlatSymbol::Variable(variable)) => {
                let variable = VariableKey {
                    side: right_ref.side,
                    variable: *variable,
                };
                match occurs(
                    left,
                    right,
                    variable,
                    left_ref,
                    &substitution,
                    steps,
                    max_steps,
                ) {
                    OccursResult::No => {
                        substitution.insert(variable, left_ref);
                    }
                    OccursResult::Yes => return UnifyResult::NotUnified,
                    OccursResult::LimitExceeded => return UnifyResult::LimitExceeded,
                }
            }
            (left_symbol, right_symbol)
                if left_symbol == right_symbol && left_node.arity == right_node.arity =>
            {
                let left_children = term(left, right, left_ref).child_roots(left_ref.root);
                let right_children = term(left, right, right_ref).child_roots(right_ref.root);
                for (&left_child, &right_child) in left_children.iter().zip(&right_children).rev() {
                    work.push((
                        TermRef {
                            side: left_ref.side,
                            root: left_child,
                        },
                        TermRef {
                            side: right_ref.side,
                            root: right_child,
                        },
                    ));
                }
            }
            _ => return UnifyResult::NotUnified,
        }
    }
    UnifyResult::Unified(substitution)
}

fn take_step(steps: &mut usize, max_steps: usize) -> bool {
    if *steps == max_steps {
        false
    } else {
        *steps += 1;
        true
    }
}

fn dereference(
    left: &FlatTerm,
    right: &FlatTerm,
    mut reference: TermRef,
    substitution: &Substitution,
    steps: &mut usize,
    max_steps: usize,
) -> Option<TermRef> {
    let mut visited = BTreeSet::new();
    loop {
        let FlatSymbol::Variable(variable) = term_node(left, right, reference).symbol else {
            return Some(reference);
        };
        let key = VariableKey {
            side: reference.side,
            variable,
        };
        let Some(next) = substitution.get(&key).copied() else {
            return Some(reference);
        };
        if !take_step(steps, max_steps) || !visited.insert(key) {
            return None;
        }
        reference = next;
    }
}

enum OccursResult {
    Yes,
    No,
    LimitExceeded,
}

fn occurs(
    left: &FlatTerm,
    right: &FlatTerm,
    variable: VariableKey,
    reference: TermRef,
    substitution: &Substitution,
    steps: &mut usize,
    max_steps: usize,
) -> OccursResult {
    let mut work = vec![reference];
    while let Some(reference) = work.pop() {
        if !take_step(steps, max_steps) {
            return OccursResult::LimitExceeded;
        }
        let Some(reference) = dereference(left, right, reference, substitution, steps, max_steps)
        else {
            return OccursResult::LimitExceeded;
        };
        match term_node(left, right, reference).symbol {
            FlatSymbol::Variable(candidate)
                if (VariableKey {
                    side: reference.side,
                    variable: candidate,
                }) == variable =>
            {
                return OccursResult::Yes;
            }
            FlatSymbol::Variable(_) => {}
            _ => {
                let children = term(left, right, reference).child_roots(reference.root);
                for child in children.into_iter().rev() {
                    work.push(TermRef {
                        side: reference.side,
                        root: child,
                    });
                }
            }
        }
    }
    OccursResult::No
}

fn term<'a>(left: &'a FlatTerm, right: &'a FlatTerm, reference: TermRef) -> &'a FlatTerm {
    match reference.side {
        TermSide::Left => left,
        TermSide::Right => right,
    }
}

fn term_node<'a>(left: &'a FlatTerm, right: &'a FlatTerm, reference: TermRef) -> &'a FlatNode {
    &term(left, right, reference).nodes[reference.root]
}

fn alpha_equivalent(
    left: &FlatTerm,
    left_root: usize,
    right: &FlatTerm,
    right_root: usize,
) -> bool {
    let mut forward = BTreeMap::new();
    let mut reverse = BTreeMap::new();
    let mut work = vec![(left_root, right_root)];
    while let Some((left_root, right_root)) = work.pop() {
        let left_node = &left.nodes[left_root];
        let right_node = &right.nodes[right_root];
        match (&left_node.symbol, &right_node.symbol) {
            (FlatSymbol::Variable(left_var), FlatSymbol::Variable(right_var)) => {
                if forward
                    .get(left_var)
                    .is_some_and(|mapped| mapped != right_var)
                    || reverse
                        .get(right_var)
                        .is_some_and(|mapped| mapped != left_var)
                {
                    return false;
                }
                forward.insert(*left_var, *right_var);
                reverse.insert(*right_var, *left_var);
            }
            (left_symbol, right_symbol)
                if left_symbol == right_symbol && left_node.arity == right_node.arity =>
            {
                let left_children = left.child_roots(left_root);
                let right_children = right.child_roots(right_root);
                for pair in left_children.into_iter().zip(right_children).rev() {
                    work.push(pair);
                }
            }
            _ => return false,
        }
    }
    true
}

/// Finds proven non-joinable critical pairs with default resource limits.
pub fn critical_pairs(cpg: &CodePropertyGraph) -> Vec<CriticalPair> {
    analyze_critical_pairs(cpg).pairs
}

/// Performs critical-pair analysis with default resource limits.
pub fn analyze_critical_pairs(cpg: &CodePropertyGraph) -> CriticalPairAnalysis {
    analyze_critical_pairs_with_caps(cpg, RewriteAnalysisCaps::default())
}

/// Performs critical-pair analysis with explicit resource limits.
pub fn analyze_critical_pairs_with_caps(
    cpg: &CodePropertyGraph,
    caps: RewriteAnalysisCaps,
) -> CriticalPairAnalysis {
    let system = analyze_system(cpg, caps);
    critical_pairs_from_system(&system, caps)
}

fn critical_pairs_from_system(
    system: &RewriteSystem,
    caps: RewriteAnalysisCaps,
) -> CriticalPairAnalysis {
    let mut issues = system.extraction_issues.clone();
    let mut pairs = BTreeSet::new();
    let mut steps = 0usize;
    let mut joinability_states = 0usize;
    'outer: for (outer_index, outer) in system.rules.iter().enumerate() {
        for overlap_root in outer.lhs.subterm_roots(outer.lhs.root()) {
            if matches!(
                outer.lhs.nodes[overlap_root].symbol,
                FlatSymbol::Variable(_)
            ) {
                continue;
            }
            for (inner_index, inner) in system.rules.iter().enumerate() {
                if outer_index == inner_index && overlap_root == outer.lhs.root() {
                    continue;
                }
                let substitution = match unify(
                    &outer.lhs,
                    overlap_root,
                    &inner.lhs,
                    inner.lhs.root(),
                    &mut steps,
                    caps.max_unification_steps,
                ) {
                    UnifyResult::Unified(substitution) => substitution,
                    UnifyResult::NotUnified => continue,
                    UnifyResult::LimitExceeded => {
                        issues.insert(RewriteAnalysisIssue::UnificationLimitExceeded {
                            limit: caps.max_unification_steps,
                        });
                        break 'outer;
                    }
                };
                let mut normalized_variables = VariableNormalizer::default();
                let Some(outer_reduct) = instantiate(
                    &outer.rhs,
                    outer.rhs.root(),
                    TermSide::Left,
                    InstantiationEnvironment {
                        substitution_left: &outer.lhs,
                        substitution_right: &inner.lhs,
                        substitution: &substitution,
                        replacement: None,
                        max_nodes: caps.max_term_nodes,
                    },
                    &mut normalized_variables,
                ) else {
                    issues.insert(RewriteAnalysisIssue::TermNodeLimitExceeded {
                        limit: caps.max_term_nodes,
                    });
                    break 'outer;
                };
                let Some(inner_reduct) = instantiate(
                    &outer.lhs,
                    outer.lhs.root(),
                    TermSide::Left,
                    InstantiationEnvironment {
                        substitution_left: &outer.lhs,
                        substitution_right: &inner.lhs,
                        substitution: &substitution,
                        replacement: Some(TermReplacement {
                            source_root: overlap_root,
                            term: &inner.rhs,
                            term_root: inner.rhs.root(),
                            namespace: TermSide::Right,
                        }),
                        max_nodes: caps.max_term_nodes,
                    },
                    &mut normalized_variables,
                ) else {
                    issues.insert(RewriteAnalysisIssue::TermNodeLimitExceeded {
                        limit: caps.max_term_nodes,
                    });
                    break 'outer;
                };
                match joinability(
                    outer_reduct.clone(),
                    inner_reduct.clone(),
                    system,
                    caps,
                    &mut steps,
                    &mut joinability_states,
                ) {
                    Joinability::Joinable => {}
                    Joinability::NonJoinable => {
                        let pair = CriticalPair {
                            outer_rule: outer.node,
                            inner_rule: inner.node,
                            overlap_site: outer.lhs.nodes[overlap_root].source,
                            outer_reduct_fingerprint: fingerprint(&outer_reduct),
                            inner_reduct_fingerprint: fingerprint(&inner_reduct),
                        };
                        if pairs.contains(&pair) {
                            continue;
                        }
                        if pairs.len() == caps.max_critical_pairs {
                            issues.insert(RewriteAnalysisIssue::CriticalPairLimitExceeded {
                                limit: caps.max_critical_pairs,
                            });
                            break 'outer;
                        }
                        pairs.insert(pair);
                    }
                    Joinability::UnificationLimitExceeded => {
                        issues.insert(RewriteAnalysisIssue::UnificationLimitExceeded {
                            limit: caps.max_unification_steps,
                        });
                        break 'outer;
                    }
                    Joinability::StateLimitExceeded => {
                        issues.insert(RewriteAnalysisIssue::JoinabilityLimitExceeded {
                            limit: caps.max_joinability_states,
                        });
                        break 'outer;
                    }
                    Joinability::TermNodeLimitExceeded => {
                        issues.insert(RewriteAnalysisIssue::TermNodeLimitExceeded {
                            limit: caps.max_term_nodes,
                        });
                        break 'outer;
                    }
                }
            }
        }
    }
    CriticalPairAnalysis {
        pairs: pairs.into_iter().collect(),
        complete: issues.is_empty(),
        issues: issues.into_iter().collect(),
    }
}

enum InstantiateTask<'a> {
    Enter {
        term: &'a FlatTerm,
        root: usize,
        namespace: TermSide,
        may_replace: bool,
    },
    Exit(usize),
}

#[derive(Clone, Copy)]
struct TermReplacement<'a> {
    source_root: usize,
    term: &'a FlatTerm,
    term_root: usize,
    namespace: TermSide,
}

#[derive(Clone, Copy)]
struct InstantiationEnvironment<'a> {
    substitution_left: &'a FlatTerm,
    substitution_right: &'a FlatTerm,
    substitution: &'a Substitution,
    replacement: Option<TermReplacement<'a>>,
    max_nodes: usize,
}

fn instantiate(
    source: &FlatTerm,
    root: usize,
    source_namespace: TermSide,
    environment: InstantiationEnvironment<'_>,
    normalized_variables: &mut VariableNormalizer,
) -> Option<FlatTerm> {
    let mut nodes: Vec<FlatNode> = Vec::new();
    let mut tasks = vec![InstantiateTask::Enter {
        term: source,
        root,
        namespace: source_namespace,
        may_replace: environment.replacement.is_some(),
    }];
    while let Some(task) = tasks.pop() {
        match task {
            InstantiateTask::Exit(index) => nodes[index].subtree_end = nodes.len() as u32,
            InstantiateTask::Enter {
                mut term,
                mut root,
                mut namespace,
                mut may_replace,
            } => {
                if may_replace
                    && environment
                        .replacement
                        .is_some_and(|value| value.source_root == root)
                {
                    let replacement = environment.replacement.expect("checked");
                    term = replacement.term;
                    root = replacement.term_root;
                    namespace = replacement.namespace;
                    may_replace = false;
                }
                let mut visited = BTreeSet::new();
                while let FlatSymbol::Variable(variable) = term.nodes[root].symbol {
                    let key = VariableKey {
                        side: namespace,
                        variable,
                    };
                    let Some(reference) = environment.substitution.get(&key).copied() else {
                        break;
                    };
                    if !visited.insert(key) {
                        break;
                    }
                    term = match reference.side {
                        TermSide::Left => environment.substitution_left,
                        TermSide::Right => environment.substitution_right,
                    };
                    root = reference.root;
                    namespace = reference.side;
                    may_replace = false;
                }
                if nodes.len() == environment.max_nodes {
                    return None;
                }
                let source_node = &term.nodes[root];
                let children = term.child_roots(root);
                let symbol = match source_node.symbol {
                    FlatSymbol::Variable(variable) => {
                        let key = VariableKey {
                            side: namespace,
                            variable,
                        };
                        FlatSymbol::Variable(normalized_variables.normalize(key))
                    }
                    _ => source_node.symbol.clone(),
                };
                let index = nodes.len();
                nodes.push(FlatNode {
                    symbol,
                    source: source_node.source,
                    arity: source_node.arity,
                    subtree_end: 0,
                });
                tasks.push(InstantiateTask::Exit(index));
                for child in children.into_iter().rev() {
                    tasks.push(InstantiateTask::Enter {
                        term,
                        root: child,
                        namespace,
                        may_replace,
                    });
                }
            }
        }
    }
    Some(FlatTerm { nodes })
}

enum MatchResult {
    Matched(Substitution),
    NotMatched,
    LimitExceeded,
}

fn match_rule(
    subject: &FlatTerm,
    subject_root: usize,
    pattern: &FlatTerm,
    pattern_root: usize,
    steps: &mut usize,
    max_steps: usize,
) -> MatchResult {
    let mut substitution: Substitution = BTreeMap::new();
    let mut work = vec![(subject_root, pattern_root)];
    while let Some((subject_root, pattern_root)) = work.pop() {
        if !take_step(steps, max_steps) {
            return MatchResult::LimitExceeded;
        }
        let subject_node = &subject.nodes[subject_root];
        let pattern_node = &pattern.nodes[pattern_root];
        if let FlatSymbol::Variable(variable) = pattern_node.symbol {
            let key = VariableKey {
                side: TermSide::Right,
                variable,
            };
            let candidate = TermRef {
                side: TermSide::Left,
                root: subject_root,
            };
            if let Some(existing) = substitution.get(&key).copied() {
                if !subterms_structurally_equal(subject, existing.root, subject, subject_root) {
                    return MatchResult::NotMatched;
                }
            } else {
                substitution.insert(key, candidate);
            }
            continue;
        }
        if subject_node.symbol != pattern_node.symbol || subject_node.arity != pattern_node.arity {
            return MatchResult::NotMatched;
        }
        let subject_children = subject.child_roots(subject_root);
        let pattern_children = pattern.child_roots(pattern_root);
        for (&subject_child, &pattern_child) in subject_children.iter().zip(&pattern_children).rev()
        {
            work.push((subject_child, pattern_child));
        }
    }
    MatchResult::Matched(substitution)
}

fn rewrite_successors(
    term: &FlatTerm,
    system: &RewriteSystem,
    caps: RewriteAnalysisCaps,
    steps: &mut usize,
    max_successors: usize,
) -> Result<Vec<FlatTerm>, Joinability> {
    let mut successors = BTreeMap::new();
    for root in term.subterm_roots(term.root()) {
        for rule in &system.rules {
            let substitution = match match_rule(
                term,
                root,
                &rule.lhs,
                rule.lhs.root(),
                steps,
                caps.max_unification_steps,
            ) {
                MatchResult::Matched(substitution) => substitution,
                MatchResult::NotMatched => continue,
                MatchResult::LimitExceeded => {
                    return Err(Joinability::UnificationLimitExceeded);
                }
            };
            let mut normalized_variables = VariableNormalizer::preserving_left(term);
            let Some(successor) = instantiate(
                term,
                term.root(),
                TermSide::Left,
                InstantiationEnvironment {
                    substitution_left: term,
                    substitution_right: &rule.lhs,
                    substitution: &substitution,
                    replacement: Some(TermReplacement {
                        source_root: root,
                        term: &rule.rhs,
                        term_root: rule.rhs.root(),
                        namespace: TermSide::Right,
                    }),
                    max_nodes: caps.max_term_nodes,
                },
                &mut normalized_variables,
            ) else {
                return Err(Joinability::TermNodeLimitExceeded);
            };
            let key = structural_key(&successor);
            if !successors.contains_key(&key) {
                if successors.len() == max_successors {
                    return Err(Joinability::StateLimitExceeded);
                }
                successors.insert(key, successor);
            }
        }
    }
    Ok(successors.into_values().collect())
}

enum Joinability {
    Joinable,
    NonJoinable,
    UnificationLimitExceeded,
    StateLimitExceeded,
    TermNodeLimitExceeded,
}

fn joinability(
    left: FlatTerm,
    right: FlatTerm,
    system: &RewriteSystem,
    caps: RewriteAnalysisCaps,
    steps: &mut usize,
    states: &mut usize,
) -> Joinability {
    let left_key = structural_key(&left);
    let right_key = structural_key(&right);
    if left_key == right_key {
        return Joinability::Joinable;
    }
    if caps.max_joinability_states < 2 || *states > caps.max_joinability_states.saturating_sub(2) {
        return Joinability::StateLimitExceeded;
    }
    *states += 2;
    let mut left_seen = BTreeSet::from([left_key]);
    let mut right_seen = BTreeSet::from([right_key]);
    let mut left_queue = VecDeque::from([left]);
    let mut right_queue = VecDeque::from([right]);

    loop {
        let mut progressed = false;
        if let Some(term) = left_queue.pop_front() {
            progressed = true;
            let successors = match rewrite_successors(
                &term,
                system,
                caps,
                steps,
                caps.max_joinability_states.saturating_sub(*states),
            ) {
                Ok(successors) => successors,
                Err(limit) => return limit,
            };
            for successor in successors {
                let key = structural_key(&successor);
                if right_seen.contains(&key) {
                    return Joinability::Joinable;
                }
                if left_seen.insert(key) {
                    if *states == caps.max_joinability_states {
                        return Joinability::StateLimitExceeded;
                    }
                    *states += 1;
                    left_queue.push_back(successor);
                }
            }
        }
        if let Some(term) = right_queue.pop_front() {
            progressed = true;
            let successors = match rewrite_successors(
                &term,
                system,
                caps,
                steps,
                caps.max_joinability_states.saturating_sub(*states),
            ) {
                Ok(successors) => successors,
                Err(limit) => return limit,
            };
            for successor in successors {
                let key = structural_key(&successor);
                if left_seen.contains(&key) {
                    return Joinability::Joinable;
                }
                if right_seen.insert(key) {
                    if *states == caps.max_joinability_states {
                        return Joinability::StateLimitExceeded;
                    }
                    *states += 1;
                    right_queue.push_back(successor);
                }
            }
        }
        if !progressed {
            return Joinability::NonJoinable;
        }
    }
}

fn structural_key(term: &FlatTerm) -> Vec<(FlatSymbol, u32)> {
    term.nodes
        .iter()
        .map(|node| (node.symbol.clone(), node.arity))
        .collect()
}

fn subterms_structurally_equal(
    left: &FlatTerm,
    left_root: usize,
    right: &FlatTerm,
    right_root: usize,
) -> bool {
    let left_nodes = &left.nodes[left.subterm_roots(left_root)];
    let right_nodes = &right.nodes[right.subterm_roots(right_root)];
    left_nodes.len() == right_nodes.len()
        && left_nodes
            .iter()
            .zip(right_nodes)
            .all(|(left, right)| left.symbol == right.symbol && left.arity == right.arity)
}

fn fingerprint(term: &FlatTerm) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for node in &term.nodes {
        hash_bytes(&mut hash, &node.arity.to_le_bytes());
        match &node.symbol {
            FlatSymbol::Variable(value) => hash_tagged(&mut hash, 0, &value.to_le_bytes()),
            FlatSymbol::Identifier(value) => hash_tagged(&mut hash, 1, value.as_bytes()),
            FlatSymbol::Operator(value) => hash_tagged(&mut hash, 2, value.as_bytes()),
            FlatSymbol::Integer(value) => hash_tagged(&mut hash, 3, &value.to_le_bytes()),
            FlatSymbol::Float(value) => hash_tagged(&mut hash, 4, &value.to_le_bytes()),
            FlatSymbol::String(value) => hash_tagged(&mut hash, 5, value.as_bytes()),
            FlatSymbol::Char(value) => hash_tagged(&mut hash, 6, &(*value as u32).to_le_bytes()),
            FlatSymbol::Bool(value) => hash_tagged(&mut hash, 7, &[u8::from(*value)]),
            FlatSymbol::Null => hash_tagged(&mut hash, 8, &[]),
            FlatSymbol::Structural(value) => hash_tagged(&mut hash, 9, value.as_bytes()),
        }
    }
    hash
}

fn hash_tagged(hash: &mut u64, tag: u8, bytes: &[u8]) {
    hash_bytes(hash, &[tag]);
    hash_bytes(hash, &(bytes.len() as u64).to_le_bytes());
    hash_bytes(hash, bytes);
}

fn hash_bytes(hash: &mut u64, bytes: &[u8]) {
    for byte in bytes {
        *hash ^= u64::from(*byte);
        *hash = hash.wrapping_mul(0x100000001b3);
    }
}

/// Appends dependency-pair and critical-conflict edges idempotently.
///
/// Endpoint validation is transactional: an invalid endpoint adds no edge.
pub fn materialize_rewrite_dep_overlay(
    cpg: &mut CodePropertyGraph,
    dependencies: &DpGraph,
    critical: &CriticalPairAnalysis,
) -> Result<usize, RewriteDepOverlayError> {
    for pair in &dependencies.pairs {
        validate_endpoint(cpg, pair.source_rule, true)?;
        validate_endpoint(cpg, pair.target_rule, false)?;
    }
    for pair in &critical.pairs {
        validate_endpoint(cpg, pair.outer_rule, true)?;
        validate_endpoint(cpg, pair.inner_rule, false)?;
    }
    let before = cpg.rewrite_dep_edges().count();
    for pair in &dependencies.pairs {
        cpg.connect_unique(
            pair.source_rule,
            pair.target_rule,
            CpgEdgeKind::RewriteDep(RewriteDepEdgeKind::DependencyPair),
        );
    }
    for pair in &critical.pairs {
        cpg.connect_unique(
            pair.outer_rule,
            pair.inner_rule,
            CpgEdgeKind::RewriteDep(RewriteDepEdgeKind::CriticalConflict),
        );
    }
    Ok(cpg.rewrite_dep_edges().count().saturating_sub(before))
}

fn validate_endpoint(
    cpg: &CodePropertyGraph,
    rule: NodeId,
    source: bool,
) -> Result<(), RewriteDepOverlayError> {
    let Some(node) = cpg.node(rule) else {
        return if source {
            Err(RewriteDepOverlayError::UnknownSource { rule })
        } else {
            Err(RewriteDepOverlayError::UnknownTarget { rule })
        };
    };
    if matches!(node.kind, CpgNodeKind::Function { .. }) {
        return Ok(());
    }
    if source {
        Err(RewriteDepOverlayError::SourceNotRule { rule })
    } else {
        Err(RewriteDepOverlayError::TargetNotRule { rule })
    }
}

#[cfg(test)]
mod rewriting_tests;
