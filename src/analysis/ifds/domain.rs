//! Generic domains and evidence types for summary-based IFDS/IDE solving.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hash;
use std::sync::Arc;

use crate::{CodePropertyGraph, NodeId};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Join-semilattice used by an interprocedural summary domain.
///
/// `Default::default()` is the bottom element. Implementations must make
/// [`join`](Self::join) monotone and idempotent, and
/// [`subsumes`](Self::subsumes) must describe the same partial order.
pub trait Lattice: Clone + Default {
    /// Joins `other` into `self`, returning whether the semantic value changed.
    fn join(&mut self, other: &Self) -> bool;

    /// Returns whether `self` contains every fact represented by `other`.
    fn subsumes(&self, other: &Self) -> bool;
}

/// Resolution of one CPG call site at the interprocedural boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CalleeRef {
    /// One represented function is the definite call target.
    Resolved(NodeId),
    /// The callee is known but is outside the represented program.
    External(Arc<str>),
    /// No supported evidence identifies the callee.
    Unresolved(Arc<str>),
}

/// Explicit resource bounds for one summary-solving run.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IfdsCaps {
    /// Maximum Kleene rounds for one cyclic strongly connected component.
    pub max_scc_iters: u32,
    /// Maximum distinct seed facts retained at one CPG node.
    pub max_facts_per_node: usize,
    /// Maximum condensation-order and summary-evaluation worklist steps.
    pub max_worklist_steps: usize,
    /// Call/name traversal depth available to summary domains and the default resolver.
    pub max_call_depth: u32,
    /// Maximum represented functions admitted by the solver.
    pub max_functions: usize,
}

impl Default for IfdsCaps {
    fn default() -> Self {
        Self {
            max_scc_iters: 64,
            max_facts_per_node: 4_096,
            max_worklist_steps: 1_000_000,
            max_call_depth: 128,
            max_functions: 100_000,
        }
    }
}

/// Read-only context supplied to one domain summary evaluation.
pub struct SummaryCtx<'a, D: SummaryDomain + ?Sized> {
    /// Current monotone summaries, including bottom values for active cycles.
    pub summaries: &'a BTreeMap<NodeId, D::Summary>,
    /// Evidence-bounded call-site resolver.
    pub resolve: &'a dyn Fn(NodeId) -> CalleeRef,
    /// Resource bounds shared by the run and the domain implementation.
    pub caps: &'a IfdsCaps,
}

/// Analysis-specific fact and procedure-summary operations.
pub trait SummaryDomain {
    /// Finite exploded-supergraph fact.
    type Fact: Clone + Eq + Hash + Ord;
    /// Monotone summary value for one function.
    type Summary: Lattice;

    /// Returns initial `(program_point, fact)` pairs for one function.
    fn seed_facts(&self, cpg: &CodePropertyGraph, function: NodeId) -> Vec<(NodeId, Self::Fact)>;

    /// Recomputes one function summary from local flow and current callees.
    fn summarize(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
        ctx: &SummaryCtx<'_, Self>,
    ) -> Self::Summary;
}

/// The resource boundary that made a result partial.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum IfdsCapEvent {
    /// The represented function inventory exceeded `max_functions`.
    Functions {
        /// Number of represented functions observed.
        observed: usize,
        /// Configured maximum.
        cap: usize,
    },
    /// Condensation ordering or summary evaluation exhausted the work budget.
    WorklistSteps {
        /// First step that could not be performed.
        needed: usize,
        /// Configured maximum.
        cap: usize,
    },
    /// One node produced more distinct seed facts than may be retained.
    FactsPerNode {
        /// Function whose seeds were collected.
        function: NodeId,
        /// Program point carrying the facts.
        node: NodeId,
        /// Distinct facts observed before deterministic truncation.
        observed: usize,
        /// Configured maximum.
        cap: usize,
    },
    /// A cyclic component changed on its final permitted Kleene round.
    SccIterations {
        /// Stable component id in the supplied SCC decomposition.
        component: usize,
        /// Rounds executed.
        iterations: u32,
        /// Configured maximum.
        cap: u32,
    },
}

/// A malformed input or domain-law observation that prevents a completeness claim.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum IfdsIssue {
    /// The supplied SCC result did not describe a call graph.
    NotCallGraph,
    /// A component id did not equal its dense vector position.
    ComponentId {
        /// Expected dense id.
        expected: usize,
        /// Id stored in the component.
        actual: usize,
    },
    /// A component contained no function.
    EmptyComponent {
        /// Empty component id.
        component: usize,
    },
    /// A component member list was not strictly increasing.
    NonCanonicalMembers {
        /// Noncanonical component id.
        component: usize,
    },
    /// One node occurred in more than one component.
    DuplicateMember {
        /// Duplicated node.
        node: NodeId,
    },
    /// The SCC membership map was absent or disagreed with its component.
    Membership {
        /// Affected node.
        node: NodeId,
        /// Component that owns the node in the component vector, when present.
        component: Option<usize>,
    },
    /// A decomposed node was absent or was not a function in the CPG.
    NotAFunction {
        /// Invalid function selector.
        node: NodeId,
    },
    /// A CPG function was missing from the decomposition.
    MissingFunction {
        /// Missing function node.
        node: NodeId,
    },
    /// A condensation edge had an out-of-range endpoint or a self-loop.
    CondensationEdge {
        /// Source component id.
        source: usize,
        /// Target component id.
        target: usize,
    },
    /// Condensation edges were not strictly sorted and deduplicated.
    NonCanonicalCondensation,
    /// The purported condensation relation contained a directed cycle.
    CyclicCondensation,
    /// The supplied decomposition did not equal the CPG's current call-graph SCCs.
    StaleCallGraph,
    /// A seed fact named a program point absent from the CPG.
    UnknownSeedNode {
        /// Function being seeded.
        function: NodeId,
        /// Unknown program point.
        node: NodeId,
    },
    /// A seed fact named a program point outside its function's AST scope.
    SeedOutsideFunction {
        /// Function being seeded.
        function: NodeId,
        /// Out-of-scope program point.
        node: NodeId,
    },
    /// A domain's `join` and `subsumes` implementations disagreed.
    LatticeLaw {
        /// Function whose summary exposed the disagreement.
        function: NodeId,
    },
}

/// Deterministic summaries, seed facts, and completeness evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(bound(
        serialize = "D::Fact: Serialize, D::Summary: Serialize",
        deserialize = "D::Fact: Deserialize<'de>, D::Summary: Deserialize<'de>"
    ))
)]
pub struct IfdsOutput<D: SummaryDomain> {
    /// Function summaries ordered by stable function id.
    pub summaries: BTreeMap<NodeId, D::Summary>,
    /// Retained seed facts ordered by function, program point, and fact.
    pub seed_facts: BTreeMap<NodeId, BTreeMap<NodeId, BTreeSet<D::Fact>>>,
    /// Functions actually evaluated, in deterministic callees-first order.
    pub processed_functions: Vec<NodeId>,
    /// Kleene rounds executed for each processed component.
    pub component_iterations: BTreeMap<usize, u32>,
    /// Condensation-order and summary-evaluation work consumed.
    pub worklist_steps: usize,
    /// Whether at least one explicit resource cap prevented complete evaluation.
    pub capped: bool,
    /// Ordered resource-cap evidence.
    pub cap_events: BTreeSet<IfdsCapEvent>,
    /// Ordered malformed-input or domain-law evidence.
    pub issues: BTreeSet<IfdsIssue>,
}

impl<D: SummaryDomain> Default for IfdsOutput<D> {
    fn default() -> Self {
        Self {
            summaries: BTreeMap::new(),
            seed_facts: BTreeMap::new(),
            processed_functions: Vec::new(),
            component_iterations: BTreeMap::new(),
            worklist_steps: 0,
            capped: false,
            cap_events: BTreeSet::new(),
            issues: BTreeSet::new(),
        }
    }
}

impl<D: SummaryDomain> IfdsOutput<D> {
    /// Returns whether the solver established an uncapped, structurally valid result.
    pub fn is_complete(&self) -> bool {
        !self.capped && self.issues.is_empty()
    }
}
