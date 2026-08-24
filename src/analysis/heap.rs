//! Stack-safe compositional heap summaries and advisory memory-safety checks.
//!
//! The analysis interprets allocation metadata, typed dereference edges, and a
//! bounded release-call vocabulary as a small separation-logic footprint.
//! Procedure summaries are solved callees-first over the existing call-graph
//! condensation. All source- and graph-shaped control lives in heap-owned
//! queues, maps, and sets; no input depth is mapped to native recursion.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::builder::call_arguments;
use crate::{
    CodePropertyGraph, CpgEdgeKind, CpgNodeKind, DfgEdgeKind, HeapEdgeKind, LiteralKind, NodeId,
};

use super::cfg_scope::{function_cfg_view, FunctionCfgView};
use super::ifds::{
    CalleeRef, IfdsCapEvent, IfdsCaps, IfdsIssue, IfdsOutput, Lattice, SummaryCtx, SummaryDomain,
};
use super::resolve::call_callee_path_evidence;
use super::{function_scope_nodes, solve_summaries, SccDecomposition};

/// Symbolic heap cell used in a procedure summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum HeapLocation {
    /// Zero-based formal-parameter position.
    Parameter(u16),
    /// Allocation-site abstraction for one locally created object.
    Allocation(NodeId),
}

/// Spatial assertion inferred at a procedure boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum HeapPredicate {
    /// The cell is allocated and may be dereferenced or released.
    Valid,
    /// The cell has been released and must not be dereferenced again.
    Invalid,
}

/// One precondition (anti-frame) or postcondition with canonical evidence.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct HeapAssertion {
    /// Spatial predicate.
    pub predicate: HeapPredicate,
    /// Symbolic cell constrained by the predicate.
    pub location: HeapLocation,
    /// Operation or exit that established the assertion.
    pub site: NodeId,
    /// Ordered interprocedural program-point witness.
    pub witness: Vec<NodeId>,
}

/// One append-only relation suitable for the CPG Heap overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct HeapRelation {
    /// Existing allocation, parameter, value, or function node.
    pub source: NodeId,
    /// Existing heap operation or abstract-object node.
    pub target: NodeId,
    /// Exact separation-logic relation.
    pub kind: HeapEdgeKind,
}

/// Transactional Heap-overlay materialization failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum HeapOverlayError {
    /// A relation source is absent from the target CPG.
    #[error("heap relation source does not exist: {relation:?}")]
    UnknownSource {
        /// Rejected relation.
        relation: HeapRelation,
    },
    /// A relation target is absent from the target CPG.
    #[error("heap relation target does not exist: {relation:?}")]
    UnknownTarget {
        /// Rejected relation.
        relation: HeapRelation,
    },
}

/// Advisory memory-safety finding class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum HeapViolationKind {
    /// A concrete null origin reaches a dereference requirement.
    NullDereference,
    /// A represented object can be dereferenced after a release.
    UseAfterFree,
    /// A local allocation can reach a procedure exit while still live.
    Leak,
    /// Caller-supplied summaries referenced nodes outside the analyzed CPG.
    InvalidSummaryReference,
}

/// One deterministic advisory heap violation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct HeapViolation {
    /// Violation class.
    pub kind: HeapViolationKind,
    /// Procedure whose summary owns the finding.
    pub function: NodeId,
    /// Allocation site or formal parameter when one is represented.
    pub object: Option<NodeId>,
    /// Dereference or exit at which the violation is observed.
    pub operation: NodeId,
    /// Canonical allocation/release/operation witness.
    pub witness: Vec<NodeId>,
    /// Whether an explicit resource bound made the evidence partial.
    pub capped: bool,
}

/// Monotone compositional summary for one procedure.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct HeapSummary {
    /// Summarized function; absent only for lattice bottom.
    pub function: Option<NodeId>,
    /// Synthesized anti-frame requirements.
    pub preconditions: BTreeSet<HeapAssertion>,
    /// Spatial facts guaranteed by represented local exits.
    pub postconditions: BTreeSet<HeapAssertion>,
    /// Local allocation-site objects.
    pub allocations: BTreeSet<NodeId>,
    /// Local or composed callees whose effects were consumed.
    pub composed_callees: BTreeSet<NodeId>,
    /// Typed append-only overlay relations.
    pub relations: BTreeSet<HeapRelation>,
    /// Advisory findings owned by this function.
    pub violations: BTreeSet<HeapViolation>,
    /// Whether local fact/work bounds made this summary partial.
    pub capped: bool,
}

impl HeapSummary {
    /// Returns whether this procedure summary is locally complete.
    pub fn is_complete(&self) -> bool {
        !self.capped
    }
}

impl Lattice for HeapSummary {
    fn join(&mut self, other: &Self) -> bool {
        let before = self.clone();
        if self.function.is_none() {
            self.function = other.function;
        }
        self.preconditions
            .extend(other.preconditions.iter().cloned());
        self.postconditions
            .extend(other.postconditions.iter().cloned());
        self.allocations.extend(other.allocations.iter().copied());
        self.composed_callees
            .extend(other.composed_callees.iter().copied());
        self.relations.extend(other.relations.iter().copied());
        self.violations.extend(other.violations.iter().cloned());
        self.capped |= other.capped;
        *self != before
    }

    fn subsumes(&self, other: &Self) -> bool {
        (other.function.is_none() || self.function == other.function)
            && self.preconditions.is_superset(&other.preconditions)
            && self.postconditions.is_superset(&other.postconditions)
            && self.allocations.is_superset(&other.allocations)
            && self.composed_callees.is_superset(&other.composed_callees)
            && self.relations.is_superset(&other.relations)
            && self.violations.is_superset(&other.violations)
            && (self.capped || !other.capped)
    }
}

/// Language-neutral classification of calls that invalidate a heap cell.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct HeapModel {
    /// Exact full or terminal callee names treated as release operations.
    pub release_calls: BTreeSet<Arc<str>>,
}

impl Default for HeapModel {
    fn default() -> Self {
        Self {
            release_calls: ["free", "dealloc", "delete", "operator_delete"]
                .into_iter()
                .map(Arc::from)
                .collect(),
        }
    }
}

impl HeapModel {
    /// Adds one exact normalized full or terminal release-call name.
    pub fn with_release_call(mut self, name: impl Into<Arc<str>>) -> Self {
        self.release_calls.insert(name.into());
        self
    }

    fn is_release(&self, path: &str) -> bool {
        self.release_calls.contains(path)
            || path
                .rsplit("::")
                .next()
                .is_some_and(|terminal| self.release_calls.contains(terminal))
    }
}

/// Complete heap-analysis report, including solver trust evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct HeapAnalysis {
    /// Function summaries ordered by stable identifier.
    pub summaries: BTreeMap<NodeId, HeapSummary>,
    /// Functions evaluated in deterministic callees-first order.
    pub processed_functions: Vec<NodeId>,
    /// Kleene rounds per call-graph component.
    pub component_iterations: BTreeMap<usize, u32>,
    /// Summary-solver work consumed.
    pub worklist_steps: usize,
    /// Whether the solver or a local summary reached a resource boundary.
    pub capped: bool,
    /// Functions whose local heap analysis reached a resource boundary.
    pub domain_capped_functions: BTreeSet<NodeId>,
    /// Exact framework cap events.
    pub cap_events: BTreeSet<IfdsCapEvent>,
    /// Structural call-graph or lattice issues.
    pub issues: BTreeSet<IfdsIssue>,
}

impl HeapAnalysis {
    /// Returns whether negative memory-safety claims may use this report.
    pub fn is_complete(&self) -> bool {
        !self.capped && self.issues.is_empty()
    }

    /// Returns all canonical advisory findings.
    pub fn violations(&self, cpg: &CodePropertyGraph) -> Vec<HeapViolation> {
        memory_safety(cpg, &self.summaries)
    }
}

/// Computes compositional heap summaries with the default release model.
pub fn analyze_heap(
    cpg: &CodePropertyGraph,
    call_graph: &SccDecomposition,
    caps: &IfdsCaps,
) -> HeapAnalysis {
    analyze_heap_with_model(cpg, call_graph, caps, &HeapModel::default())
}

/// Computes compositional heap summaries with an explicit release model.
pub fn analyze_heap_with_model(
    cpg: &CodePropertyGraph,
    call_graph: &SccDecomposition,
    caps: &IfdsCaps,
    model: &HeapModel,
) -> HeapAnalysis {
    let domain = HeapDomain::new(cpg, call_graph, model);
    heap_output(solve_summaries(cpg, &domain, call_graph, caps))
}

/// Returns the summary map named by the FT.4 API contract.
///
/// Consumers that need to make a negative claim must call [`analyze_heap`]
/// and require [`HeapAnalysis::is_complete`], because a bare map cannot expose
/// solver-level cap and malformed-input evidence.
pub fn biabduction_summaries(
    cpg: &CodePropertyGraph,
    call_graph: &SccDecomposition,
    caps: &IfdsCaps,
) -> BTreeMap<NodeId, HeapSummary> {
    analyze_heap(cpg, call_graph, caps).summaries
}

/// Projects canonical null-dereference, use-after-free, and leak findings.
pub fn memory_safety(
    cpg: &CodePropertyGraph,
    summaries: &BTreeMap<NodeId, HeapSummary>,
) -> Vec<HeapViolation> {
    let mut output = BTreeSet::new();
    for (&function, summary) in summaries {
        for finding in &summary.violations {
            let mut finding = finding.clone();
            finding.capped |= summary.capped;
            let references_are_valid = cpg
                .node(function)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Function { .. }))
                && cpg.contains_node(finding.operation)
                && finding
                    .object
                    .is_none_or(|object| cpg.contains_node(object))
                && finding.witness.iter().all(|&node| cpg.contains_node(node));
            if !references_are_valid || finding.function != function {
                finding.kind = HeapViolationKind::InvalidSummaryReference;
                finding.function = function;
                finding.capped = true;
            }
            output.insert(finding);
        }
    }
    output.into_iter().collect()
}

/// Materializes every valid summary relation as an idempotent Heap overlay.
///
/// No node is created or removed. All endpoints are validated before the first
/// mutation, so malformed caller-supplied summaries fail transactionally.
pub fn materialize_heap_overlay(
    cpg: &mut CodePropertyGraph,
    summaries: &BTreeMap<NodeId, HeapSummary>,
) -> Result<usize, HeapOverlayError> {
    let before = cpg.heap_edges().count();
    let relations: BTreeSet<_> = summaries
        .values()
        .flat_map(|summary| summary.relations.iter().copied())
        .collect();
    for &relation in &relations {
        if !cpg.contains_node(relation.source) {
            return Err(HeapOverlayError::UnknownSource { relation });
        }
        if !cpg.contains_node(relation.target) {
            return Err(HeapOverlayError::UnknownTarget { relation });
        }
    }
    for relation in relations {
        cpg.connect_unique(
            relation.source,
            relation.target,
            CpgEdgeKind::Heap(relation.kind),
        );
    }
    Ok(cpg.heap_edges().count().saturating_sub(before))
}

fn heap_output(output: IfdsOutput<HeapDomain<'_>>) -> HeapAnalysis {
    let domain_capped_functions = output
        .summaries
        .iter()
        .filter_map(|(&function, summary)| summary.capped.then_some(function))
        .collect::<BTreeSet<_>>();
    let capped = output.capped || !domain_capped_functions.is_empty();
    HeapAnalysis {
        summaries: output.summaries,
        processed_functions: output.processed_functions,
        component_iterations: output.component_iterations,
        worklist_steps: output.worklist_steps,
        capped,
        domain_capped_functions,
        cap_events: output.cap_events,
        issues: output.issues,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum HeapOrigin {
    Parameter(u16),
    Allocation(NodeId),
    Null(NodeId),
}

#[derive(Debug)]
struct LocalFunction {
    scope: BTreeSet<NodeId>,
    nodes: Vec<NodeId>,
    params: Vec<NodeId>,
    calls: Vec<NodeId>,
    dfg_edges: Vec<(NodeId, NodeId)>,
}

#[derive(Debug)]
struct HeapDomain<'a> {
    model: &'a HeapModel,
    functions: BTreeMap<NodeId, LocalFunction>,
    recursive_component_by_function: BTreeMap<NodeId, usize>,
}

impl<'a> HeapDomain<'a> {
    fn new(cpg: &CodePropertyGraph, call_graph: &SccDecomposition, model: &'a HeapModel) -> Self {
        let functions = cpg
            .functions()
            .map(|node| (node.id, local_function(cpg, node.id)))
            .collect();
        let mut recursive_component_by_function = BTreeMap::new();
        for component in &call_graph.components {
            if component.cyclic {
                for &function in &component.nodes {
                    recursive_component_by_function.insert(function, component.id);
                }
            }
        }
        Self {
            model,
            functions,
            recursive_component_by_function,
        }
    }

    fn same_recursive_component(&self, caller: NodeId, callee: NodeId) -> bool {
        self.recursive_component_by_function.contains_key(&caller)
            && self.recursive_component_by_function.get(&caller)
                == self.recursive_component_by_function.get(&callee)
    }
}

impl SummaryDomain for HeapDomain<'_> {
    type Fact = HeapOrigin;
    type Summary = HeapSummary;

    fn seed_facts(&self, cpg: &CodePropertyGraph, function: NodeId) -> Vec<(NodeId, Self::Fact)> {
        let Some(local) = self.functions.get(&function) else {
            return Vec::new();
        };
        let mut facts = Vec::new();
        for (index, &parameter) in local.params.iter().enumerate() {
            if let Ok(index) = u16::try_from(index) {
                facts.push((parameter, HeapOrigin::Parameter(index)));
            }
        }
        for &node in &local.nodes {
            if cpg
                .node(node)
                .is_some_and(|node| node.allocation_type.is_some())
            {
                facts.push((node, HeapOrigin::Allocation(node)));
            }
            if matches!(
                cpg.node(node).map(|node| &node.kind),
                Some(CpgNodeKind::Literal {
                    kind: LiteralKind::Null
                })
            ) {
                facts.push((node, HeapOrigin::Null(node)));
            }
        }
        facts
    }

    fn summarize(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
        ctx: &SummaryCtx<'_, Self>,
    ) -> Self::Summary {
        let Some(local) = self.functions.get(&function) else {
            return HeapSummary {
                function: Some(function),
                capped: true,
                ..HeapSummary::default()
            };
        };
        summarize_function(cpg, function, local, self, ctx)
    }
}

type OriginEnv = BTreeMap<NodeId, BTreeSet<HeapOrigin>>;
type EventMap = BTreeMap<NodeId, BTreeSet<HeapOrigin>>;

fn summarize_function(
    cpg: &CodePropertyGraph,
    function: NodeId,
    local: &LocalFunction,
    domain: &HeapDomain<'_>,
    ctx: &SummaryCtx<'_, HeapDomain<'_>>,
) -> HeapSummary {
    let (origins, origins_capped) = compute_origins(cpg, local, ctx.caps);
    let mut summary = HeapSummary {
        function: Some(function),
        capped: origins_capped || local.params.len() > usize::from(u16::MAX),
        ..HeapSummary::default()
    };
    let mut dereferences = dereference_events(cpg, local, &origins, ctx.caps, &mut summary.capped);
    let mut releases = release_events(
        cpg,
        local,
        &origins,
        domain.model,
        ctx.caps,
        &mut summary.capped,
    );

    for &node in &local.nodes {
        if cpg
            .node(node)
            .is_some_and(|node| node.allocation_type.is_some())
        {
            summary.allocations.insert(node);
            insert_relation(
                &mut summary,
                function,
                node,
                HeapEdgeKind::Allocates,
                ctx.caps,
            );
        }
    }

    for &call in &local.calls {
        let CalleeRef::Resolved(callee) = (ctx.resolve)(call) else {
            continue;
        };
        let Some(callee_summary) = ctx.summaries.get(&callee) else {
            continue;
        };
        summary.composed_callees.insert(callee);
        summary.capped |= callee_summary.capped;
        let arguments = call_arguments(cpg, call);
        compose_preconditions(
            cpg,
            local,
            call,
            callee_summary,
            &arguments,
            &origins,
            &mut dereferences,
            &mut summary,
            ctx.caps,
        );
        if !domain.same_recursive_component(function, callee) {
            compose_postconditions(
                cpg,
                local,
                call,
                callee_summary,
                &arguments,
                &origins,
                &mut releases,
                &mut summary,
                ctx.caps,
            );
        } else if callee_summary
            .postconditions
            .iter()
            .any(|assertion| assertion.predicate == HeapPredicate::Invalid)
        {
            // A recursive must-invalidate fact is not safe to consume before
            // its greatest fixed point is known. Preserve the advisory facts
            // but prevent a complete negative verdict instead of silently
            // treating the release as definite or absent.
            summary.capped = true;
        }
    }

    record_spatial_evidence(local, &dereferences, &releases, &mut summary, ctx.caps);
    analyze_lifetimes(
        cpg,
        function,
        local,
        &dereferences,
        &releases,
        &mut summary,
        ctx.caps,
    );
    summary
}

#[allow(clippy::too_many_arguments)]
fn compose_preconditions(
    cpg: &CodePropertyGraph,
    local: &LocalFunction,
    call: NodeId,
    callee: &HeapSummary,
    arguments: &[NodeId],
    origins: &OriginEnv,
    dereferences: &mut EventMap,
    summary: &mut HeapSummary,
    caps: &IfdsCaps,
) {
    for assertion in &callee.preconditions {
        let (HeapPredicate::Valid, HeapLocation::Parameter(index)) =
            (assertion.predicate, assertion.location)
        else {
            continue;
        };
        let Some(&argument) = arguments.get(usize::from(index)) else {
            summary.capped = true;
            continue;
        };
        let (argument_origins, capped) = origins_for_value(cpg, origins, argument, caps);
        summary.capped |= capped;
        for origin in argument_origins {
            match origin {
                HeapOrigin::Parameter(caller_index) => {
                    let Some(&parameter) = local.params.get(usize::from(caller_index)) else {
                        summary.capped = true;
                        continue;
                    };
                    insert_assertion(
                        &mut summary.preconditions,
                        HeapPredicate::Valid,
                        HeapLocation::Parameter(caller_index),
                        assertion.site,
                        composed_witness(parameter, call, &assertion.witness),
                        caps,
                        &mut summary.capped,
                    );
                    insert_relation(summary, parameter, call, HeapEdgeKind::RequiresValid, caps);
                }
                HeapOrigin::Allocation(_) => {
                    dereferences.entry(call).or_default().insert(origin);
                }
                HeapOrigin::Null(origin) => insert_violation(
                    summary,
                    HeapViolationKind::NullDereference,
                    Some(origin),
                    call,
                    composed_witness(origin, call, &assertion.witness),
                    caps,
                ),
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn compose_postconditions(
    cpg: &CodePropertyGraph,
    local: &LocalFunction,
    call: NodeId,
    callee: &HeapSummary,
    arguments: &[NodeId],
    origins: &OriginEnv,
    releases: &mut EventMap,
    summary: &mut HeapSummary,
    caps: &IfdsCaps,
) {
    for assertion in &callee.postconditions {
        let (HeapPredicate::Invalid, HeapLocation::Parameter(index)) =
            (assertion.predicate, assertion.location)
        else {
            continue;
        };
        let Some(&argument) = arguments.get(usize::from(index)) else {
            summary.capped = true;
            continue;
        };
        let (argument_origins, capped) = origins_for_value(cpg, origins, argument, caps);
        summary.capped |= capped;
        for origin in argument_origins {
            releases.entry(call).or_default().insert(origin);
            if let HeapOrigin::Parameter(caller_index) = origin {
                let Some(&parameter) = local.params.get(usize::from(caller_index)) else {
                    summary.capped = true;
                    continue;
                };
                insert_assertion(
                    &mut summary.postconditions,
                    HeapPredicate::Invalid,
                    HeapLocation::Parameter(caller_index),
                    assertion.site,
                    composed_witness(parameter, call, &assertion.witness),
                    caps,
                    &mut summary.capped,
                );
            }
        }
    }
}

fn composed_witness(origin: NodeId, call: NodeId, suffix: &[NodeId]) -> Vec<NodeId> {
    let mut witness = vec![origin];
    if call != origin {
        witness.push(call);
    }
    for &node in suffix {
        if witness.last().copied() != Some(node) {
            witness.push(node);
        }
    }
    witness
}

fn record_spatial_evidence(
    local: &LocalFunction,
    dereferences: &EventMap,
    releases: &EventMap,
    summary: &mut HeapSummary,
    caps: &IfdsCaps,
) {
    for (&site, origins) in dereferences {
        for &origin in origins {
            match origin {
                HeapOrigin::Parameter(index) => {
                    let Some(&parameter) = local.params.get(usize::from(index)) else {
                        summary.capped = true;
                        continue;
                    };
                    insert_assertion(
                        &mut summary.preconditions,
                        HeapPredicate::Valid,
                        HeapLocation::Parameter(index),
                        site,
                        vec![parameter, site],
                        caps,
                        &mut summary.capped,
                    );
                    insert_relation(summary, parameter, site, HeapEdgeKind::RequiresValid, caps);
                    insert_relation(summary, parameter, site, HeapEdgeKind::Dereferences, caps);
                }
                HeapOrigin::Allocation(allocation) => {
                    insert_relation(summary, allocation, site, HeapEdgeKind::Dereferences, caps)
                }
                HeapOrigin::Null(origin) => insert_violation(
                    summary,
                    HeapViolationKind::NullDereference,
                    Some(origin),
                    site,
                    vec![origin, site],
                    caps,
                ),
            }
        }
    }

    for (&site, origins) in releases {
        for &origin in origins {
            match origin {
                HeapOrigin::Parameter(index) => {
                    let Some(&parameter) = local.params.get(usize::from(index)) else {
                        summary.capped = true;
                        continue;
                    };
                    insert_relation(summary, parameter, site, HeapEdgeKind::Releases, caps);
                    insert_relation(summary, parameter, site, HeapEdgeKind::EnsuresInvalid, caps);
                }
                HeapOrigin::Allocation(allocation) => {
                    insert_relation(summary, allocation, site, HeapEdgeKind::Releases, caps);
                    insert_relation(
                        summary,
                        allocation,
                        site,
                        HeapEdgeKind::EnsuresInvalid,
                        caps,
                    );
                }
                HeapOrigin::Null(_) => {}
            }
        }
    }
}

fn analyze_lifetimes(
    cpg: &CodePropertyGraph,
    function: NodeId,
    local: &LocalFunction,
    dereferences: &EventMap,
    releases: &EventMap,
    summary: &mut HeapSummary,
    caps: &IfdsCaps,
) {
    let Ok(view) = function_cfg_view(cpg, function) else {
        summary.capped = true;
        return;
    };

    let allocations: Vec<_> = summary.allocations.iter().copied().collect();
    for allocation in allocations {
        let outcome = lifetime_for(
            &view,
            HeapOrigin::Allocation(allocation),
            Some(allocation),
            dereferences,
            releases,
            caps,
        );
        record_lifetime_outcome(local, outcome, summary, caps);
    }
    for (index, _) in local.params.iter().enumerate() {
        let Ok(index) = u16::try_from(index) else {
            summary.capped = true;
            break;
        };
        let origin = HeapOrigin::Parameter(index);
        if !event_mentions(dereferences, origin) && !event_mentions(releases, origin) {
            continue;
        }
        let outcome = lifetime_for(&view, origin, None, dereferences, releases, caps);
        record_lifetime_outcome(local, outcome, summary, caps);
        if summary.capped {
            break;
        }
    }
}

const ABSENT: u8 = 1;
const LIVE: u8 = 2;
const FREED: u8 = 4;

struct LifetimeOutcome {
    origin: HeapOrigin,
    live_exits: Vec<NodeId>,
    freed_exits: Vec<NodeId>,
    use_after_free: Vec<(NodeId, NodeId)>,
    capped: bool,
}

fn lifetime_for(
    view: &FunctionCfgView,
    origin: HeapOrigin,
    allocation: Option<NodeId>,
    dereferences: &EventMap,
    releases: &EventMap,
    caps: &IfdsCaps,
) -> LifetimeOutcome {
    let mut inputs = vec![0u8; view.projection.node_count()];
    inputs[view.entry as usize] = if allocation.is_some() { ABSENT } else { LIVE };
    let mut queue = VecDeque::from([view.entry]);
    let mut queued = vec![false; view.projection.node_count()];
    queued[view.entry as usize] = true;
    let mut work = 0usize;
    let mut use_after_free = BTreeSet::new();

    while let Some(dense) = queue.pop_front() {
        queued[dense as usize] = false;
        if work >= caps.max_worklist_steps {
            return LifetimeOutcome {
                origin,
                live_exits: Vec::new(),
                freed_exits: Vec::new(),
                use_after_free: use_after_free.into_iter().collect(),
                capped: true,
            };
        }
        work = work.saturating_add(1);
        let node = view.projection.node_id(dense);
        let input = inputs[dense as usize];
        if input & FREED != 0 && event_at(dereferences, node, origin) {
            let release = releases
                .iter()
                .find_map(|(&site, origins)| origins.contains(&origin).then_some(site))
                .unwrap_or(node);
            use_after_free.insert((release, node));
        }
        let output = transfer_lifetime(node, input, origin, allocation, releases);
        for &successor in view.projection.successors(dense) {
            let joined = inputs[successor as usize] | output;
            if joined != inputs[successor as usize] {
                inputs[successor as usize] = joined;
                if !queued[successor as usize] {
                    queued[successor as usize] = true;
                    queue.push_back(successor);
                }
            }
        }
    }

    let mut live_exits = Vec::new();
    let mut freed_exits = Vec::new();
    for &dense in &view.exit_like {
        let node = view.projection.node_id(dense);
        let output = transfer_lifetime(node, inputs[dense as usize], origin, allocation, releases);
        if output & LIVE != 0 {
            live_exits.push(node);
        }
        if output & FREED != 0 {
            freed_exits.push(node);
        }
    }
    LifetimeOutcome {
        origin,
        live_exits,
        freed_exits,
        use_after_free: use_after_free.into_iter().collect(),
        capped: false,
    }
}

fn transfer_lifetime(
    node: NodeId,
    input: u8,
    origin: HeapOrigin,
    allocation: Option<NodeId>,
    releases: &EventMap,
) -> u8 {
    let mut output = if allocation == Some(node) {
        LIVE
    } else {
        input
    };
    if event_at(releases, node, origin) {
        output = (output & ABSENT)
            | if output & (LIVE | FREED) != 0 {
                FREED
            } else {
                0
            };
    }
    output
}

fn record_lifetime_outcome(
    local: &LocalFunction,
    outcome: LifetimeOutcome,
    summary: &mut HeapSummary,
    caps: &IfdsCaps,
) {
    summary.capped |= outcome.capped;
    let object = origin_node(local, outcome.origin);
    for (release, dereference) in outcome.use_after_free {
        insert_violation(
            summary,
            HeapViolationKind::UseAfterFree,
            Some(object),
            dereference,
            vec![object, release, dereference],
            caps,
        );
    }
    match outcome.origin {
        HeapOrigin::Allocation(allocation) => {
            let has_live_exit = !outcome.live_exits.is_empty();
            for exit in outcome.live_exits {
                insert_assertion(
                    &mut summary.postconditions,
                    HeapPredicate::Valid,
                    HeapLocation::Allocation(allocation),
                    exit,
                    vec![allocation, exit],
                    caps,
                    &mut summary.capped,
                );
                insert_violation(
                    summary,
                    HeapViolationKind::Leak,
                    Some(allocation),
                    exit,
                    vec![allocation, exit],
                    caps,
                );
            }
            if !has_live_exit {
                for exit in outcome.freed_exits {
                    insert_assertion(
                        &mut summary.postconditions,
                        HeapPredicate::Invalid,
                        HeapLocation::Allocation(allocation),
                        exit,
                        vec![allocation, exit],
                        caps,
                        &mut summary.capped,
                    );
                }
            }
        }
        HeapOrigin::Parameter(index) => {
            if outcome.live_exits.is_empty() && !outcome.freed_exits.is_empty() {
                for exit in outcome.freed_exits {
                    insert_assertion(
                        &mut summary.postconditions,
                        HeapPredicate::Invalid,
                        HeapLocation::Parameter(index),
                        exit,
                        vec![object, exit],
                        caps,
                        &mut summary.capped,
                    );
                }
            }
        }
        HeapOrigin::Null(_) => {}
    }
}

fn event_mentions(events: &EventMap, origin: HeapOrigin) -> bool {
    events.values().any(|origins| origins.contains(&origin))
}

fn event_at(events: &EventMap, node: NodeId, origin: HeapOrigin) -> bool {
    events
        .get(&node)
        .is_some_and(|origins| origins.contains(&origin))
}

fn origin_node(local: &LocalFunction, origin: HeapOrigin) -> NodeId {
    match origin {
        HeapOrigin::Parameter(index) => local
            .params
            .get(usize::from(index))
            .copied()
            .expect("heap origins only contain represented local parameters"),
        HeapOrigin::Allocation(node) | HeapOrigin::Null(node) => node,
    }
}

fn compute_origins(
    cpg: &CodePropertyGraph,
    local: &LocalFunction,
    caps: &IfdsCaps,
) -> (OriginEnv, bool) {
    let mut origins = OriginEnv::new();
    let mut capped = false;
    for (index, &parameter) in local.params.iter().enumerate() {
        let Ok(index) = u16::try_from(index) else {
            capped = true;
            continue;
        };
        add_origin(
            &mut origins,
            parameter,
            HeapOrigin::Parameter(index),
            caps,
            &mut capped,
        );
    }
    for &node in &local.nodes {
        if cpg
            .node(node)
            .is_some_and(|node| node.allocation_type.is_some())
        {
            add_origin(
                &mut origins,
                node,
                HeapOrigin::Allocation(node),
                caps,
                &mut capped,
            );
        }
        if matches!(
            cpg.node(node).map(|node| &node.kind),
            Some(CpgNodeKind::Literal {
                kind: LiteralKind::Null
            })
        ) {
            add_origin(
                &mut origins,
                node,
                HeapOrigin::Null(node),
                caps,
                &mut capped,
            );
        }
    }

    let mut work = 0usize;
    loop {
        let mut changed = false;
        for &node in &local.nodes {
            if work >= caps.max_worklist_steps {
                return (origins, true);
            }
            work = work.saturating_add(1);
            match cpg.node(node).map(|node| &node.kind) {
                Some(CpgNodeKind::Call { .. }) => {}
                Some(CpgNodeKind::Identifier { definition, .. }) => {
                    let mut definitions = cpg.reaching_definitions(node);
                    definitions.extend(definition.iter().copied());
                    definitions.sort_unstable();
                    definitions.dedup();
                    for definition in definitions {
                        changed |= copy_origins(&mut origins, definition, node, caps, &mut capped);
                    }
                }
                Some(CpgNodeKind::Assignment { .. }) => {
                    for &child in cpg.ast_children(node).iter().skip(1) {
                        changed |= copy_origins(&mut origins, child, node, caps, &mut capped);
                    }
                }
                Some(CpgNodeKind::Parameter { .. } | CpgNodeKind::Function { .. }) => {}
                Some(_) => {
                    for &child in cpg.ast_children(node) {
                        changed |= copy_origins(&mut origins, child, node, caps, &mut capped);
                    }
                }
                None => {}
            }
        }
        for &(source, target) in &local.dfg_edges {
            if work >= caps.max_worklist_steps {
                return (origins, true);
            }
            work = work.saturating_add(1);
            if cpg
                .node(target)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Call { .. }))
            {
                continue;
            }
            changed |= copy_origins(&mut origins, source, target, caps, &mut capped);
        }
        if !changed {
            break;
        }
    }
    (origins, capped)
}

fn add_origin(
    origins: &mut OriginEnv,
    node: NodeId,
    origin: HeapOrigin,
    caps: &IfdsCaps,
    capped: &mut bool,
) -> bool {
    let entry = origins.entry(node).or_default();
    if entry.contains(&origin) {
        return false;
    }
    if entry.len() >= caps.max_facts_per_node {
        *capped = true;
        return false;
    }
    entry.insert(origin)
}

fn copy_origins(
    origins: &mut OriginEnv,
    source: NodeId,
    target: NodeId,
    caps: &IfdsCaps,
    capped: &mut bool,
) -> bool {
    let source_origins = origins.get(&source).cloned().unwrap_or_default();
    let mut changed = false;
    for origin in source_origins {
        changed |= add_origin(origins, target, origin, caps, capped);
    }
    changed
}

fn origins_for_value(
    cpg: &CodePropertyGraph,
    origins: &OriginEnv,
    value: NodeId,
    caps: &IfdsCaps,
) -> (BTreeSet<HeapOrigin>, bool) {
    let mut result = origins.get(&value).cloned().unwrap_or_default();
    let mut stack = vec![value];
    let mut visited = BTreeSet::new();
    let mut work = 0usize;
    while let Some(node) = stack.pop() {
        if !visited.insert(node) {
            continue;
        }
        if work >= caps.max_worklist_steps {
            return (result, true);
        }
        work = work.saturating_add(1);
        if let Some(found) = origins.get(&node) {
            for &origin in found {
                if result.len() >= caps.max_facts_per_node && !result.contains(&origin) {
                    return (result, true);
                }
                result.insert(origin);
            }
        }
        let mut children = cpg.ast_children(node).to_vec();
        children.reverse();
        stack.extend(children);
    }
    (result, false)
}

fn dereference_events(
    cpg: &CodePropertyGraph,
    local: &LocalFunction,
    origins: &OriginEnv,
    caps: &IfdsCaps,
    capped: &mut bool,
) -> EventMap {
    let mut sinks = BTreeMap::<NodeId, BTreeSet<NodeId>>::new();
    for edge in cpg.edges() {
        if matches!(edge.kind, CpgEdgeKind::DataFlow(DfgEdgeKind::Dereference))
            && local.scope.contains(&edge.source)
            && local.scope.contains(&edge.target)
        {
            sinks.entry(edge.target).or_default().insert(edge.source);
        }
    }
    for &node in &local.nodes {
        if matches!(
            cpg.node(node).map(|node| &node.kind),
            Some(CpgNodeKind::UnaryOp { operator }) if operator.as_ref() == "*"
        ) {
            sinks
                .entry(node)
                .or_default()
                .extend(cpg.ast_children(node));
        }
    }
    let mut events = EventMap::new();
    for (sink, operands) in sinks {
        for operand in operands {
            let (found, hit_cap) = origins_for_value(cpg, origins, operand, caps);
            *capped |= hit_cap;
            events.entry(sink).or_default().extend(found);
        }
    }
    events
}

fn release_events(
    cpg: &CodePropertyGraph,
    local: &LocalFunction,
    origins: &OriginEnv,
    model: &HeapModel,
    caps: &IfdsCaps,
    capped: &mut bool,
) -> EventMap {
    let mut events = EventMap::new();
    for &call in &local.calls {
        let evidence = call_callee_path_evidence(cpg, call, caps.max_call_depth);
        *capped |= evidence.capped;
        let Some(path) = evidence.path else {
            continue;
        };
        if !model.is_release(&path) {
            continue;
        }
        let Some(argument) = call_arguments(cpg, call).first().copied() else {
            *capped = true;
            continue;
        };
        let (found, hit_cap) = origins_for_value(cpg, origins, argument, caps);
        *capped |= hit_cap;
        events.entry(call).or_default().extend(found);
    }
    events
}

fn insert_relation(
    summary: &mut HeapSummary,
    source: NodeId,
    target: NodeId,
    kind: HeapEdgeKind,
    caps: &IfdsCaps,
) {
    if summary.relations.len() >= caps.max_facts_per_node
        && !summary.relations.contains(&HeapRelation {
            source,
            target,
            kind,
        })
    {
        summary.capped = true;
        return;
    }
    summary.relations.insert(HeapRelation {
        source,
        target,
        kind,
    });
}

#[allow(clippy::too_many_arguments)]
fn insert_assertion(
    assertions: &mut BTreeSet<HeapAssertion>,
    predicate: HeapPredicate,
    location: HeapLocation,
    site: NodeId,
    mut witness: Vec<NodeId>,
    caps: &IfdsCaps,
    capped: &mut bool,
) {
    if witness.len() > caps.max_call_depth as usize {
        witness.truncate(caps.max_call_depth as usize);
        *capped = true;
    }
    let assertion = HeapAssertion {
        predicate,
        location,
        site,
        witness,
    };
    if assertions.len() >= caps.max_facts_per_node && !assertions.contains(&assertion) {
        *capped = true;
        return;
    }
    assertions.insert(assertion);
}

fn insert_violation(
    summary: &mut HeapSummary,
    kind: HeapViolationKind,
    object: Option<NodeId>,
    operation: NodeId,
    mut witness: Vec<NodeId>,
    caps: &IfdsCaps,
) {
    if witness.len() > caps.max_call_depth as usize {
        witness.truncate(caps.max_call_depth as usize);
        summary.capped = true;
    }
    let finding = HeapViolation {
        kind,
        function: summary
            .function
            .expect("a concrete heap summary records its owning function"),
        object,
        operation,
        witness,
        capped: summary.capped,
    };
    if summary.violations.len() >= caps.max_facts_per_node && !summary.violations.contains(&finding)
    {
        summary.capped = true;
        return;
    }
    summary.violations.insert(finding);
}

fn local_function(cpg: &CodePropertyGraph, function: NodeId) -> LocalFunction {
    let scope: BTreeSet<_> = function_scope_nodes(cpg, function).into_iter().collect();
    let mut nodes: Vec<_> = scope.iter().copied().collect();
    nodes.sort_unstable();
    let mut params: Vec<_> = nodes
        .iter()
        .copied()
        .filter(|&node| {
            cpg.node(node)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Parameter { .. }))
        })
        .collect();
    params.sort_unstable_by_key(|&node| source_key(cpg, node));
    let calls = nodes
        .iter()
        .copied()
        .filter(|&node| {
            cpg.node(node)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Call { .. }))
        })
        .collect();
    let mut dfg_edges = cpg
        .edges()
        .filter_map(|edge| {
            matches!(edge.kind, CpgEdgeKind::DataFlow(_))
                .then_some((edge.source, edge.target))
                .filter(|(source, target)| scope.contains(source) && scope.contains(target))
        })
        .collect::<Vec<_>>();
    dfg_edges.sort_unstable();
    dfg_edges.dedup();
    LocalFunction {
        scope,
        nodes,
        params,
        calls,
        dfg_edges,
    }
}

fn source_key(cpg: &CodePropertyGraph, node: NodeId) -> (u32, u32, NodeId) {
    cpg.node(node)
        .map(|node| (node.range.start, node.range.end, node.id))
        .unwrap_or((u32::MAX, u32::MAX, node))
}

#[cfg(test)]
#[path = "heap/tests.rs"]
mod tests;
