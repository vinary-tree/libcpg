//! Deterministic interprocedural constant and side-effect summaries.
//!
//! The analysis is an out-of-graph [`SummaryDomain`] over the canonical call
//! graph condensation. It classifies only evidence represented by CPG node and
//! edge kinds: parameter flow, global and field access, resolved callees, and
//! scalar AST expressions. It never parses source text or dispatches on a
//! language.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use crate::{CodePropertyGraph, CpgEdgeKind, CpgNodeKind, DfgEdgeKind, NodeId};

use super::dataflow::{
    fold_binary, fold_unary, literal_value, ConstLattice, ConstValue, Lattice as DataflowLattice,
};
use super::{
    function_scope_nodes, solve_summaries, CalleeRef, IfdsCapEvent, IfdsCaps, IfdsIssue,
    IfdsLattice, IfdsOutput, SccDecomposition, SummaryCtx, SummaryDomain,
};
use crate::builder::call_arguments;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// An externally observable storage place.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Place {
    /// A variable declaration outside the summarized function.
    Global(Arc<str>),
    /// A zero-based formal parameter.
    Param(u16),
    /// A named class, struct, or object field.
    Field(Arc<str>),
}

/// Worst observed effect class, ordered from most to least pure.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Purity {
    /// No represented externally observable effect.
    #[default]
    Pure,
    /// Reads global or field state but does not write it.
    ReadsGlobal,
    /// Writes global or field state.
    WritesGlobal,
    /// Contains an unresolved/external call or another conservatively impure operation.
    Impure,
}

/// Monotone procedure effect summary.
///
/// `const_return = None` has two distinct meanings. When
/// `const_return_overdefined` is false it is lattice bottom (no return evidence
/// yet); when true the function is known not to have one scalar constant
/// return. Keeping the explicit bit preserves a lawful three-level constant
/// lattice while retaining the task's ergonomic `Option<ConstValue>` field.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct EffectSummary {
    /// Observable places read directly or through represented callees.
    pub reads: BTreeSet<Place>,
    /// Observable places written directly or through represented callees.
    pub writes: BTreeSet<Place>,
    /// Worst direct or transitive purity class.
    pub purity: Purity,
    /// Bit `i` is set when formal parameter `i` may flow to the return value.
    pub param_to_return: u64,
    /// One scalar return constant when all represented return paths agree.
    pub const_return: Option<ConstValue>,
    /// Whether return evidence is overdefined rather than lattice bottom.
    pub const_return_overdefined: bool,
    /// Whether the function has more formal parameters than the `u64` mask can encode.
    pub param_mask_truncated: bool,
    /// Whether a local bound or fixed-width parameter mask forced conservative widening.
    pub capped: bool,
}

impl Default for EffectSummary {
    fn default() -> Self {
        Self {
            reads: BTreeSet::new(),
            writes: BTreeSet::new(),
            purity: Purity::Pure,
            param_to_return: 0,
            const_return: None,
            const_return_overdefined: false,
            param_mask_truncated: false,
            capped: false,
        }
    }
}

impl EffectSummary {
    fn constant_state(&self) -> ConstantState {
        if self.const_return_overdefined {
            ConstantState::Overdefined
        } else if let Some(value) = &self.const_return {
            ConstantState::Constant(value.clone())
        } else {
            ConstantState::Bottom
        }
    }

    fn set_constant_state(&mut self, state: ConstantState) {
        match state {
            ConstantState::Bottom => {
                self.const_return = None;
                self.const_return_overdefined = false;
            }
            ConstantState::Constant(value) => {
                self.const_return = Some(value);
                self.const_return_overdefined = false;
            }
            ConstantState::Overdefined => {
                self.const_return = None;
                self.const_return_overdefined = true;
            }
        }
    }
}

impl IfdsLattice for EffectSummary {
    fn join(&mut self, other: &Self) -> bool {
        let before = self.clone();
        self.reads.extend(other.reads.iter().cloned());
        self.writes.extend(other.writes.iter().cloned());
        self.purity = self.purity.max(other.purity);
        self.param_to_return |= other.param_to_return;
        self.param_mask_truncated |= other.param_mask_truncated;
        self.capped |= other.capped;
        self.set_constant_state(self.constant_state().join(other.constant_state()));
        *self != before
    }

    fn subsumes(&self, other: &Self) -> bool {
        self.reads.is_superset(&other.reads)
            && self.writes.is_superset(&other.writes)
            && self.purity >= other.purity
            && self.param_to_return | other.param_to_return == self.param_to_return
            && (self.param_mask_truncated || !other.param_mask_truncated)
            && (self.capped || !other.capped)
            && self.constant_state().subsumes(&other.constant_state())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ConstantState {
    Bottom,
    Constant(ConstValue),
    Overdefined,
}

impl ConstantState {
    fn join(self, other: Self) -> Self {
        match (self, other) {
            (Self::Bottom, value) | (value, Self::Bottom) => value,
            (Self::Constant(left), Self::Constant(right)) if left == right => Self::Constant(left),
            (Self::Constant(_), Self::Constant(_))
            | (Self::Overdefined, _)
            | (_, Self::Overdefined) => Self::Overdefined,
        }
    }

    fn subsumes(&self, other: &Self) -> bool {
        matches!(
            (self, other),
            (Self::Overdefined, _)
                | (Self::Constant(_), Self::Bottom)
                | (Self::Bottom, Self::Bottom)
        ) || matches!((self, other), (Self::Constant(left), Self::Constant(right)) if left == right)
    }
}

/// Effect summaries plus completeness and cap evidence from the shared solver.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct EffectAnalysis {
    /// Function summaries ordered by stable function id.
    pub summaries: BTreeMap<NodeId, EffectSummary>,
    /// Functions evaluated in deterministic callees-first order.
    pub processed_functions: Vec<NodeId>,
    /// Kleene rounds executed for each call-graph component.
    pub component_iterations: BTreeMap<usize, u32>,
    /// Shared solver work consumed.
    pub worklist_steps: usize,
    /// Whether the shared solver or a domain-local traversal was capped.
    pub capped: bool,
    /// Functions whose summary carries direct or transitive domain-cap evidence.
    pub domain_capped_functions: BTreeSet<NodeId>,
    /// Ordered shared-solver cap evidence.
    pub cap_events: BTreeSet<IfdsCapEvent>,
    /// Ordered malformed-input or lattice-law evidence.
    pub issues: BTreeSet<IfdsIssue>,
}

impl EffectAnalysis {
    /// Returns whether all summaries were computed without a cap or issue.
    pub fn is_complete(&self) -> bool {
        !self.capped && self.issues.is_empty()
    }
}

/// Computes effect summaries and preserves all completeness evidence.
pub fn analyze_effects(
    cpg: &CodePropertyGraph,
    call_graph: &SccDecomposition,
    caps: &IfdsCaps,
) -> EffectAnalysis {
    let domain = EffectDomain::new(cpg);
    let output = solve_summaries(cpg, &domain, call_graph, caps);
    effect_output(output)
}

/// Computes the deterministic function-to-summary map.
///
/// This convenience projection matches the original C5 API. Consumers that
/// make negative claims must use [`analyze_effects`] and require
/// [`EffectAnalysis::is_complete`], because this map may be a capped prefix.
pub fn effect_summaries(
    cpg: &CodePropertyGraph,
    call_graph: &SccDecomposition,
    caps: &IfdsCaps,
) -> BTreeMap<NodeId, EffectSummary> {
    analyze_effects(cpg, call_graph, caps).summaries
}

fn effect_output(output: IfdsOutput<EffectDomain>) -> EffectAnalysis {
    let domain_capped_functions = output
        .summaries
        .iter()
        .filter_map(|(&function, summary)| summary.capped.then_some(function))
        .collect::<BTreeSet<_>>();
    let capped = output.capped || !domain_capped_functions.is_empty();
    EffectAnalysis {
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

#[derive(Debug, Clone)]
struct LocalEffects {
    scope: BTreeSet<NodeId>,
    params: BTreeMap<NodeId, u16>,
    param_count: usize,
    calls: Vec<NodeId>,
    returns: Vec<NodeId>,
    reads: BTreeSet<Place>,
    writes: BTreeSet<Place>,
    purity: Purity,
}

#[derive(Debug, Clone, Default)]
struct EffectDomain {
    functions: BTreeMap<NodeId, LocalEffects>,
}

impl EffectDomain {
    fn new(cpg: &CodePropertyGraph) -> Self {
        let functions = cpg
            .functions()
            .map(|node| (node.id, local_effects(cpg, node.id)))
            .collect();
        Self { functions }
    }
}

impl SummaryDomain for EffectDomain {
    type Fact = NodeId;
    type Summary = EffectSummary;

    fn seed_facts(&self, _cpg: &CodePropertyGraph, _function: NodeId) -> Vec<(NodeId, Self::Fact)> {
        Vec::new()
    }

    fn summarize(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
        ctx: &SummaryCtx<'_, Self>,
    ) -> Self::Summary {
        let Some(local) = self.functions.get(&function) else {
            return EffectSummary {
                purity: Purity::Impure,
                const_return_overdefined: true,
                capped: true,
                ..EffectSummary::default()
            };
        };
        summarize_function(cpg, local, ctx)
    }
}

fn summarize_function(
    cpg: &CodePropertyGraph,
    local: &LocalEffects,
    ctx: &SummaryCtx<'_, EffectDomain>,
) -> EffectSummary {
    let mut summary = EffectSummary {
        reads: local.reads.clone(),
        writes: local.writes.clone(),
        purity: local.purity,
        param_mask_truncated: local.param_count > u64::BITS as usize,
        capped: local.param_count > u64::BITS as usize,
        ..EffectSummary::default()
    };

    for &call in &local.calls {
        match (ctx.resolve)(call) {
            CalleeRef::Resolved(callee) => {
                let Some(callee_summary) = ctx.summaries.get(&callee) else {
                    summary.purity = Purity::Impure;
                    continue;
                };
                compose_callee(cpg, call, local, callee_summary, &mut summary);
            }
            CalleeRef::External(_) | CalleeRef::Unresolved(_) => {
                summary.purity = Purity::Impure;
            }
        }
    }

    let mut budget = TraversalBudget::default();
    let mut return_state = ConstLattice::Unknown;
    if local.returns.is_empty() {
        return_state = ConstLattice::Overdefined;
    }
    for &returned in &local.returns {
        let Some(expression) = cpg.ast_children(returned).first().copied() else {
            DataflowLattice::join(&mut return_state, &ConstLattice::Overdefined);
            continue;
        };
        summary.param_to_return |= parameter_dependencies(cpg, expression, local, ctx, &mut budget);
        let value = evaluate_constant(cpg, expression, ctx, &mut budget);
        DataflowLattice::join(&mut return_state, &value);
    }
    if budget.capped {
        summary.capped = true;
        summary.param_to_return = u64::MAX;
        return_state = ConstLattice::Overdefined;
    }
    summary.set_constant_state(constant_state(return_state));
    summary.purity = summary.purity.max(purity_from_places(&summary));
    summary
}

fn compose_callee(
    cpg: &CodePropertyGraph,
    call: NodeId,
    local: &LocalEffects,
    callee: &EffectSummary,
    summary: &mut EffectSummary,
) {
    let arguments = call_arguments(cpg, call);
    for place in &callee.reads {
        summary
            .reads
            .extend(map_callee_place(cpg, place, &arguments, local));
    }
    for place in &callee.writes {
        summary
            .writes
            .extend(map_callee_place(cpg, place, &arguments, local));
    }
    summary.purity = summary.purity.max(callee.purity);
    summary.capped |= callee.capped;
}

fn map_callee_place(
    cpg: &CodePropertyGraph,
    place: &Place,
    arguments: &[NodeId],
    local: &LocalEffects,
) -> BTreeSet<Place> {
    match place {
        Place::Global(_) | Place::Field(_) => BTreeSet::from([place.clone()]),
        Place::Param(index) => arguments
            .get(usize::from(*index))
            .map_or_else(BTreeSet::new, |&argument| {
                places_for_value(cpg, argument, local, &mut BTreeSet::new())
            }),
    }
}

fn local_effects(cpg: &CodePropertyGraph, function: NodeId) -> LocalEffects {
    let scope: BTreeSet<_> = function_scope_nodes(cpg, function).into_iter().collect();
    let mut nodes: Vec<_> = scope.iter().copied().collect();
    nodes.sort_unstable();
    let mut parameters: Vec<_> = nodes
        .iter()
        .copied()
        .filter(|&node| {
            cpg.node(node)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Parameter { .. }))
        })
        .collect();
    parameters.sort_unstable_by_key(|&node| source_key(cpg, node));
    let params = parameters
        .iter()
        .copied()
        .enumerate()
        .filter_map(|(index, node)| u16::try_from(index).ok().map(|index| (node, index)))
        .collect::<BTreeMap<_, _>>();

    let mut local = LocalEffects {
        scope,
        params,
        param_count: parameters.len(),
        calls: Vec::new(),
        returns: Vec::new(),
        reads: BTreeSet::new(),
        writes: BTreeSet::new(),
        purity: Purity::Pure,
    };

    for node in nodes {
        let Some(kind) = cpg.node(node).map(|node| &node.kind) else {
            continue;
        };
        match kind {
            CpgNodeKind::Call { .. } => local.calls.push(node),
            CpgNodeKind::Return => local.returns.push(node),
            CpgNodeKind::Throw | CpgNodeKind::Await | CpgNodeKind::Yield => {
                local.purity = Purity::Impure;
            }
            CpgNodeKind::Identifier { .. } if !is_binder(cpg, node) => {
                let places = places_for_identifier(cpg, node, &local);
                if let Some(reads_before_write) = assignment_target(cpg, node) {
                    if reads_before_write {
                        local.reads.extend(places.iter().cloned());
                    }
                    local.writes.extend(places);
                } else {
                    local.reads.extend(places);
                }
            }
            CpgNodeKind::MemberAccess { member } => {
                let place = resolved_field_name(cpg, node).unwrap_or_else(|| member.clone());
                let (reads, writes) = incident_access_kinds(cpg, node);
                if reads {
                    local.reads.insert(Place::Field(place.clone()));
                }
                if writes {
                    local.writes.insert(Place::Field(place));
                }
            }
            CpgNodeKind::IndexAccess => {
                let (reads, writes) = incident_access_kinds(cpg, node);
                if let Some(base) = cpg.ast_children(node).first().copied() {
                    let places = places_for_value(cpg, base, &local, &mut BTreeSet::new());
                    if reads {
                        local.reads.extend(places.iter().cloned());
                    }
                    if writes {
                        local.writes.extend(places);
                    }
                }
            }
            _ => {}
        }
    }
    local.calls.sort_unstable();
    local.calls.dedup();
    local.returns.sort_unstable();
    local.returns.dedup();
    local.purity = local
        .purity
        .max(purity_from_places_raw(&local.reads, &local.writes));
    local
}

fn source_key(cpg: &CodePropertyGraph, node: NodeId) -> (u32, u32, NodeId) {
    cpg.node(node)
        .map(|node| (node.range.start, node.range.end, node.id))
        .unwrap_or((u32::MAX, u32::MAX, node))
}

fn is_binder(cpg: &CodePropertyGraph, node: NodeId) -> bool {
    let Some(identifier_name) = cpg.node(node).and_then(|node| node.name()) else {
        return false;
    };
    let Some(parent) = cpg.ast_parent(node) else {
        return false;
    };
    matches!(
        cpg.node(parent).map(|node| &node.kind),
        Some(CpgNodeKind::Variable { name, .. } | CpgNodeKind::Parameter { name, .. })
            if &**name == identifier_name
    )
}

fn assignment_target(cpg: &CodePropertyGraph, node: NodeId) -> Option<bool> {
    let parent = cpg.ast_parent(node)?;
    let Some(CpgNodeKind::Assignment { operator }) = cpg.node(parent).map(|node| &node.kind) else {
        return None;
    };
    (cpg.ast_children(parent).first().copied() == Some(node)).then_some(operator.as_ref() != "=")
}

fn places_for_identifier(
    cpg: &CodePropertyGraph,
    identifier: NodeId,
    local: &LocalEffects,
) -> BTreeSet<Place> {
    let mut definitions = BTreeSet::new();
    if let Some(CpgNodeKind::Identifier {
        definition: Some(definition),
        ..
    }) = cpg.node(identifier).map(|node| &node.kind)
    {
        definitions.insert(*definition);
    }
    definitions.extend(cpg.reaching_definitions(identifier));

    definitions
        .into_iter()
        .filter_map(|definition| place_for_definition(cpg, definition, local))
        .collect()
}

fn place_for_definition(
    cpg: &CodePropertyGraph,
    definition: NodeId,
    local: &LocalEffects,
) -> Option<Place> {
    if let Some(&index) = local.params.get(&definition) {
        return Some(Place::Param(index));
    }
    match cpg.node(definition).map(|node| &node.kind) {
        Some(CpgNodeKind::Field { name, .. }) => Some(Place::Field(name.clone())),
        Some(CpgNodeKind::Variable { name, .. }) if !local.scope.contains(&definition) => {
            Some(Place::Global(name.clone()))
        }
        _ => None,
    }
}

fn places_for_value(
    cpg: &CodePropertyGraph,
    node: NodeId,
    local: &LocalEffects,
    seen: &mut BTreeSet<NodeId>,
) -> BTreeSet<Place> {
    let mut entered = Vec::new();
    let mut current = node;
    let result = loop {
        if !seen.insert(current) {
            break BTreeSet::new();
        }
        entered.push(current);
        match cpg.node(current).map(|node| &node.kind) {
            Some(CpgNodeKind::Identifier { .. }) => {
                break places_for_identifier(cpg, current, local)
            }
            Some(CpgNodeKind::Parameter { .. }) => {
                break local
                    .params
                    .get(&current)
                    .copied()
                    .map_or_else(BTreeSet::new, |index| BTreeSet::from([Place::Param(index)]))
            }
            Some(CpgNodeKind::Field { name, .. } | CpgNodeKind::MemberAccess { member: name }) => {
                break BTreeSet::from([Place::Field(name.clone())])
            }
            Some(CpgNodeKind::IndexAccess) => {
                let Some(base) = cpg.ast_children(current).first().copied() else {
                    break BTreeSet::new();
                };
                current = base;
            }
            _ => break BTreeSet::new(),
        }
    };
    for entered_node in entered {
        seen.remove(&entered_node);
    }
    result
}

fn resolved_field_name(cpg: &CodePropertyGraph, access: NodeId) -> Option<Arc<str>> {
    cpg.outgoing_edges(access).find_map(|edge| {
        matches!(edge.kind, CpgEdgeKind::Reference)
            .then(|| cpg.node(edge.target))
            .flatten()
            .and_then(|node| match &node.kind {
                CpgNodeKind::Field { name, .. } => Some(name.clone()),
                _ => None,
            })
    })
}

fn incident_access_kinds(cpg: &CodePropertyGraph, access: NodeId) -> (bool, bool) {
    let mut reads = false;
    let mut writes = false;
    for edge in cpg.incoming_edges(access).chain(cpg.outgoing_edges(access)) {
        match edge.kind {
            CpgEdgeKind::DataFlow(
                DfgEdgeKind::FieldRead | DfgEdgeKind::IndexRead | DfgEdgeKind::Dereference,
            ) => reads = true,
            CpgEdgeKind::DataFlow(DfgEdgeKind::FieldWrite | DfgEdgeKind::IndexWrite) => {
                writes = true;
            }
            _ => {}
        }
    }
    (reads, writes)
}

fn purity_from_places(summary: &EffectSummary) -> Purity {
    purity_from_places_raw(&summary.reads, &summary.writes)
}

fn purity_from_places_raw(reads: &BTreeSet<Place>, writes: &BTreeSet<Place>) -> Purity {
    if writes.iter().any(|place| !matches!(place, Place::Param(_))) {
        Purity::WritesGlobal
    } else if reads.iter().any(|place| !matches!(place, Place::Param(_))) {
        Purity::ReadsGlobal
    } else {
        Purity::Pure
    }
}

#[derive(Default)]
struct TraversalBudget {
    steps: usize,
    capped: bool,
}

impl TraversalBudget {
    fn enter(&mut self, depth: u32, caps: &IfdsCaps) -> bool {
        if depth > caps.max_call_depth || self.steps >= caps.max_worklist_steps {
            self.capped = true;
            false
        } else {
            self.steps = self.steps.saturating_add(1);
            true
        }
    }
}

fn parameter_dependencies(
    cpg: &CodePropertyGraph,
    node: NodeId,
    local: &LocalEffects,
    ctx: &SummaryCtx<'_, EffectDomain>,
    budget: &mut TraversalBudget,
) -> u64 {
    let mut stats = EvaluationMachineStats::default();
    parameter_dependencies_machine(cpg, node, local, ctx, budget, true, &mut stats)
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct EvaluationMachineStats {
    max_frames: usize,
    cache_hits: usize,
}

enum DependencyFrame {
    Enter {
        node: NodeId,
        depth: u32,
    },
    Visit {
        node: NodeId,
        depth: u32,
        children: Vec<NodeId>,
        next: usize,
        mask: u64,
        cacheable: bool,
    },
    Combine {
        node: NodeId,
        depth: u32,
        children: Vec<NodeId>,
        next: usize,
        mask: u64,
        cacheable: bool,
    },
    Exit {
        node: NodeId,
        mask: u64,
        cacheable: bool,
    },
}

#[allow(clippy::too_many_arguments)]
fn parameter_dependencies_machine(
    cpg: &CodePropertyGraph,
    root: NodeId,
    local: &LocalEffects,
    ctx: &SummaryCtx<'_, EffectDomain>,
    budget: &mut TraversalBudget,
    cache_enabled: bool,
    stats: &mut EvaluationMachineStats,
) -> u64 {
    let mut frames = Vec::new();
    let mut values = Vec::new();
    let mut on_path = HashSet::new();
    let mut terminal_cache: HashMap<NodeId, u64> = HashMap::new();
    if !push_machine_frame(
        &mut frames,
        DependencyFrame::Enter {
            node: root,
            depth: 0,
        },
        budget,
        stats,
    ) {
        return u64::MAX;
    }

    while let Some(frame) = frames.pop() {
        match frame {
            DependencyFrame::Enter { node, depth } => {
                if !budget.enter(depth, ctx.caps) {
                    if !push_machine_value(&mut values, 0, budget) {
                        return u64::MAX;
                    }
                    continue;
                }
                if on_path.contains(&node) {
                    budget.capped = true;
                    if !push_machine_value(&mut values, u64::MAX, budget) {
                        return u64::MAX;
                    }
                    continue;
                }
                if on_path.try_reserve(1).is_err() {
                    budget.capped = true;
                    return u64::MAX;
                }
                on_path.insert(node);
                if cache_enabled {
                    if let Some(&cached) = terminal_cache.get(&node) {
                        stats.cache_hits = stats.cache_hits.saturating_add(1);
                        on_path.remove(&node);
                        if !push_machine_value(&mut values, cached, budget) {
                            return u64::MAX;
                        }
                        continue;
                    }
                }
                let Some(children) = dependency_children(cpg, node, ctx, budget) else {
                    on_path.remove(&node);
                    if !push_machine_value(&mut values, u64::MAX, budget) {
                        return u64::MAX;
                    }
                    continue;
                };
                let cacheable = children.is_empty();
                if !push_machine_frame(
                    &mut frames,
                    DependencyFrame::Visit {
                        node,
                        depth,
                        children,
                        next: 0,
                        mask: parameter_mask_for_node(cpg, node, local),
                        cacheable,
                    },
                    budget,
                    stats,
                ) {
                    on_path.clear();
                    return u64::MAX;
                }
            }
            DependencyFrame::Visit {
                node,
                depth,
                children,
                next,
                mask,
                cacheable,
            } => {
                if let Some(&child) = children.get(next) {
                    if !push_machine_frame(
                        &mut frames,
                        DependencyFrame::Combine {
                            node,
                            depth,
                            children,
                            next: next + 1,
                            mask,
                            cacheable,
                        },
                        budget,
                        stats,
                    ) || !push_machine_frame(
                        &mut frames,
                        DependencyFrame::Enter {
                            node: child,
                            depth: depth.saturating_add(1),
                        },
                        budget,
                        stats,
                    ) {
                        on_path.clear();
                        return u64::MAX;
                    }
                } else if !push_machine_frame(
                    &mut frames,
                    DependencyFrame::Exit {
                        node,
                        mask,
                        cacheable,
                    },
                    budget,
                    stats,
                ) {
                    on_path.clear();
                    return u64::MAX;
                }
            }
            DependencyFrame::Combine {
                node,
                depth,
                children,
                next,
                mask,
                cacheable,
            } => {
                let child = values.pop().unwrap_or(u64::MAX);
                if !push_machine_frame(
                    &mut frames,
                    DependencyFrame::Visit {
                        node,
                        depth,
                        children,
                        next,
                        mask: mask | child,
                        cacheable,
                    },
                    budget,
                    stats,
                ) {
                    on_path.clear();
                    return u64::MAX;
                }
            }
            DependencyFrame::Exit {
                node,
                mask,
                cacheable,
            } => {
                if cache_enabled && cacheable && terminal_cache.try_reserve(1).is_ok() {
                    terminal_cache.insert(node, mask);
                }
                on_path.remove(&node);
                if !push_machine_value(&mut values, mask, budget) {
                    return u64::MAX;
                }
            }
        }
    }
    values.pop().unwrap_or(u64::MAX)
}

fn dependency_children(
    cpg: &CodePropertyGraph,
    node: NodeId,
    ctx: &SummaryCtx<'_, EffectDomain>,
    budget: &mut TraversalBudget,
) -> Option<Vec<NodeId>> {
    if matches!(
        cpg.node(node).map(|node| &node.kind),
        Some(CpgNodeKind::Call { .. })
    ) {
        let mut arguments = call_arguments(cpg, node);
        match (ctx.resolve)(node) {
            CalleeRef::Resolved(callee) => {
                let Some(callee_summary) = ctx.summaries.get(&callee) else {
                    return Some(Vec::new());
                };
                if callee_summary.param_mask_truncated && !arguments.is_empty() {
                    budget.capped = true;
                }
                let mut retained = 0usize;
                for index in 0..arguments.len() {
                    let represented =
                        index < 64 && callee_summary.param_to_return & (1u64 << index) != 0;
                    if represented || callee_summary.param_mask_truncated {
                        arguments.swap(retained, index);
                        retained += 1;
                    }
                }
                arguments.truncate(retained);
                Some(arguments)
            }
            CalleeRef::External(_) | CalleeRef::Unresolved(_) => Some(arguments),
        }
    } else {
        let mut children = cpg.ast_children(node);
        let definitions = cpg.reaching_definitions(node);
        if children.try_reserve(definitions.len()).is_err() {
            budget.capped = true;
            return None;
        }
        children.extend(definitions);
        Some(children)
    }
}

fn push_machine_frame<T>(
    frames: &mut Vec<T>,
    frame: T,
    budget: &mut TraversalBudget,
    stats: &mut EvaluationMachineStats,
) -> bool {
    if frames.try_reserve(1).is_err() {
        budget.capped = true;
        return false;
    }
    frames.push(frame);
    stats.max_frames = stats.max_frames.max(frames.len());
    true
}

fn push_machine_value<T>(values: &mut Vec<T>, value: T, budget: &mut TraversalBudget) -> bool {
    if values.try_reserve(1).is_err() {
        budget.capped = true;
        return false;
    }
    values.push(value);
    true
}

fn parameter_mask_for_node(cpg: &CodePropertyGraph, node: NodeId, local: &LocalEffects) -> u64 {
    let direct = local.params.get(&node).copied().map(usize::from);
    let referenced = match cpg.node(node).map(|node| &node.kind) {
        Some(CpgNodeKind::Identifier {
            definition: Some(definition),
            ..
        }) => local.params.get(definition).copied().map(usize::from),
        _ => None,
    };
    direct
        .or(referenced)
        .filter(|&index| index < 64)
        .map_or(0, |index| 1u64 << index)
}

fn evaluate_constant(
    cpg: &CodePropertyGraph,
    node: NodeId,
    ctx: &SummaryCtx<'_, EffectDomain>,
    budget: &mut TraversalBudget,
) -> ConstLattice {
    let mut stats = EvaluationMachineStats::default();
    evaluate_constant_machine(cpg, node, ctx, budget, true, &mut stats)
}

enum ConstantFrame {
    Enter {
        node: NodeId,
        depth: u32,
    },
    JoinNext {
        node: NodeId,
        depth: u32,
        definitions: Vec<NodeId>,
        next: usize,
        value: ConstLattice,
    },
    JoinCombine {
        node: NodeId,
        depth: u32,
        definitions: Vec<NodeId>,
        next: usize,
        value: ConstLattice,
    },
    BinaryLeft {
        node: NodeId,
        depth: u32,
        operator: Arc<str>,
        right: NodeId,
    },
    BinaryRight {
        node: NodeId,
        operator: Arc<str>,
        left: ConstLattice,
    },
    Unary {
        node: NodeId,
        operator: Arc<str>,
    },
    Forward {
        node: NodeId,
    },
    Exit {
        node: NodeId,
        value: ConstLattice,
        cacheable: bool,
    },
}

fn evaluate_constant_machine(
    cpg: &CodePropertyGraph,
    root: NodeId,
    ctx: &SummaryCtx<'_, EffectDomain>,
    budget: &mut TraversalBudget,
    cache_enabled: bool,
    stats: &mut EvaluationMachineStats,
) -> ConstLattice {
    let mut frames = Vec::new();
    let mut values = Vec::new();
    let mut on_path = HashSet::new();
    let mut terminal_cache: HashMap<NodeId, ConstLattice> = HashMap::new();
    if !push_machine_frame(
        &mut frames,
        ConstantFrame::Enter {
            node: root,
            depth: 0,
        },
        budget,
        stats,
    ) {
        return ConstLattice::Overdefined;
    }

    while let Some(frame) = frames.pop() {
        match frame {
            ConstantFrame::Enter { node, depth } => {
                if !budget.enter(depth, ctx.caps) {
                    if !push_machine_value(&mut values, ConstLattice::Overdefined, budget) {
                        return ConstLattice::Overdefined;
                    }
                    continue;
                }
                if on_path.contains(&node) {
                    if !push_machine_value(&mut values, ConstLattice::Overdefined, budget) {
                        return ConstLattice::Overdefined;
                    }
                    continue;
                }
                if on_path.try_reserve(1).is_err() {
                    budget.capped = true;
                    return ConstLattice::Overdefined;
                }
                on_path.insert(node);
                if cache_enabled {
                    if let Some(cached) = terminal_cache.get(&node) {
                        stats.cache_hits = stats.cache_hits.saturating_add(1);
                        let cached = cached.clone();
                        on_path.remove(&node);
                        if !push_machine_value(&mut values, cached, budget) {
                            return ConstLattice::Overdefined;
                        }
                        continue;
                    }
                }
                let kind = cpg.node(node).map(|node| &node.kind);
                let frame = match kind {
                    Some(CpgNodeKind::Literal { kind }) => ConstantFrame::Exit {
                        node,
                        value: literal_value(kind),
                        cacheable: true,
                    },
                    Some(CpgNodeKind::Call { .. }) => {
                        let value = match (ctx.resolve)(node) {
                            CalleeRef::Resolved(callee) => ctx
                                .summaries
                                .get(&callee)
                                .map_or(ConstLattice::Overdefined, const_lattice),
                            CalleeRef::External(_) | CalleeRef::Unresolved(_) => {
                                ConstLattice::Overdefined
                            }
                        };
                        ConstantFrame::Exit {
                            node,
                            value,
                            cacheable: true,
                        }
                    }
                    Some(CpgNodeKind::Identifier { .. }) => {
                        let mut definitions = cpg.reaching_definitions(node);
                        if let Some(CpgNodeKind::Identifier {
                            definition: Some(definition),
                            ..
                        }) = kind
                        {
                            if definitions.try_reserve(1).is_err() {
                                budget.capped = true;
                                on_path.clear();
                                return ConstLattice::Overdefined;
                            }
                            definitions.push(*definition);
                        }
                        definitions.sort_unstable();
                        definitions.dedup();
                        if definitions.is_empty() {
                            ConstantFrame::Exit {
                                node,
                                value: ConstLattice::Overdefined,
                                cacheable: true,
                            }
                        } else {
                            ConstantFrame::JoinNext {
                                node,
                                depth,
                                definitions,
                                next: 0,
                                value: ConstLattice::Unknown,
                            }
                        }
                    }
                    Some(CpgNodeKind::BinaryOp { operator }) => {
                        let children = cpg.ast_children(node);
                        if let [left, right, ..] = children.as_slice() {
                            if !push_machine_frame(
                                &mut frames,
                                ConstantFrame::BinaryLeft {
                                    node,
                                    depth,
                                    operator: operator.clone(),
                                    right: *right,
                                },
                                budget,
                                stats,
                            ) || !push_machine_frame(
                                &mut frames,
                                ConstantFrame::Enter {
                                    node: *left,
                                    depth: depth.saturating_add(1),
                                },
                                budget,
                                stats,
                            ) {
                                on_path.clear();
                                return ConstLattice::Overdefined;
                            }
                            continue;
                        }
                        ConstantFrame::Exit {
                            node,
                            value: ConstLattice::Overdefined,
                            cacheable: true,
                        }
                    }
                    Some(CpgNodeKind::UnaryOp { operator }) => {
                        if let Some(operand) = cpg.ast_children(node).first().copied() {
                            if !push_machine_frame(
                                &mut frames,
                                ConstantFrame::Unary {
                                    node,
                                    operator: operator.clone(),
                                },
                                budget,
                                stats,
                            ) || !push_machine_frame(
                                &mut frames,
                                ConstantFrame::Enter {
                                    node: operand,
                                    depth: depth.saturating_add(1),
                                },
                                budget,
                                stats,
                            ) {
                                on_path.clear();
                                return ConstLattice::Overdefined;
                            }
                            continue;
                        }
                        ConstantFrame::Exit {
                            node,
                            value: ConstLattice::Overdefined,
                            cacheable: true,
                        }
                    }
                    Some(CpgNodeKind::Assignment { .. }) => {
                        if let Some(rhs) = cpg.ast_children(node).get(1).copied() {
                            if !push_machine_frame(
                                &mut frames,
                                ConstantFrame::Forward { node },
                                budget,
                                stats,
                            ) || !push_machine_frame(
                                &mut frames,
                                ConstantFrame::Enter {
                                    node: rhs,
                                    depth: depth.saturating_add(1),
                                },
                                budget,
                                stats,
                            ) {
                                on_path.clear();
                                return ConstLattice::Overdefined;
                            }
                            continue;
                        }
                        ConstantFrame::Exit {
                            node,
                            value: ConstLattice::Overdefined,
                            cacheable: true,
                        }
                    }
                    Some(CpgNodeKind::Variable { name, .. }) => {
                        let initializer = cpg.ast_children(node).into_iter().rev().find(|&child| {
                            !matches!(
                                cpg.node(child).map(|node| &node.kind),
                                Some(CpgNodeKind::Identifier { name: child_name, .. }) if child_name == name
                            )
                        });
                        if let Some(initializer) = initializer {
                            if !push_machine_frame(
                                &mut frames,
                                ConstantFrame::Forward { node },
                                budget,
                                stats,
                            ) || !push_machine_frame(
                                &mut frames,
                                ConstantFrame::Enter {
                                    node: initializer,
                                    depth: depth.saturating_add(1),
                                },
                                budget,
                                stats,
                            ) {
                                on_path.clear();
                                return ConstLattice::Overdefined;
                            }
                            continue;
                        }
                        ConstantFrame::Exit {
                            node,
                            value: ConstLattice::Overdefined,
                            cacheable: true,
                        }
                    }
                    Some(CpgNodeKind::Return) => {
                        if let Some(returned) = cpg.ast_children(node).first().copied() {
                            if !push_machine_frame(
                                &mut frames,
                                ConstantFrame::Forward { node },
                                budget,
                                stats,
                            ) || !push_machine_frame(
                                &mut frames,
                                ConstantFrame::Enter {
                                    node: returned,
                                    depth: depth.saturating_add(1),
                                },
                                budget,
                                stats,
                            ) {
                                on_path.clear();
                                return ConstLattice::Overdefined;
                            }
                            continue;
                        }
                        ConstantFrame::Exit {
                            node,
                            value: ConstLattice::Overdefined,
                            cacheable: true,
                        }
                    }
                    Some(CpgNodeKind::Parameter { .. }) | None | Some(_) => ConstantFrame::Exit {
                        node,
                        value: ConstLattice::Overdefined,
                        cacheable: true,
                    },
                };
                if !push_machine_frame(&mut frames, frame, budget, stats) {
                    on_path.clear();
                    return ConstLattice::Overdefined;
                }
            }
            ConstantFrame::JoinNext {
                node,
                depth,
                definitions,
                next,
                value,
            } => {
                if let Some(&definition) = definitions.get(next) {
                    if !push_machine_frame(
                        &mut frames,
                        ConstantFrame::JoinCombine {
                            node,
                            depth,
                            definitions,
                            next: next + 1,
                            value,
                        },
                        budget,
                        stats,
                    ) || !push_machine_frame(
                        &mut frames,
                        ConstantFrame::Enter {
                            node: definition,
                            depth: depth.saturating_add(1),
                        },
                        budget,
                        stats,
                    ) {
                        on_path.clear();
                        return ConstLattice::Overdefined;
                    }
                } else if !push_machine_frame(
                    &mut frames,
                    ConstantFrame::Exit {
                        node,
                        value,
                        cacheable: false,
                    },
                    budget,
                    stats,
                ) {
                    on_path.clear();
                    return ConstLattice::Overdefined;
                }
            }
            ConstantFrame::JoinCombine {
                node,
                depth,
                definitions,
                next,
                mut value,
            } => {
                let child = values.pop().unwrap_or(ConstLattice::Overdefined);
                DataflowLattice::join(&mut value, &child);
                if !push_machine_frame(
                    &mut frames,
                    ConstantFrame::JoinNext {
                        node,
                        depth,
                        definitions,
                        next,
                        value,
                    },
                    budget,
                    stats,
                ) {
                    on_path.clear();
                    return ConstLattice::Overdefined;
                }
            }
            ConstantFrame::BinaryLeft {
                node,
                depth,
                operator,
                right,
            } => {
                let left = values.pop().unwrap_or(ConstLattice::Overdefined);
                if !push_machine_frame(
                    &mut frames,
                    ConstantFrame::BinaryRight {
                        node,
                        operator,
                        left,
                    },
                    budget,
                    stats,
                ) || !push_machine_frame(
                    &mut frames,
                    ConstantFrame::Enter {
                        node: right,
                        depth: depth.saturating_add(1),
                    },
                    budget,
                    stats,
                ) {
                    on_path.clear();
                    return ConstLattice::Overdefined;
                }
            }
            ConstantFrame::BinaryRight {
                node,
                operator,
                left,
            } => {
                let right = values.pop().unwrap_or(ConstLattice::Overdefined);
                if !push_machine_frame(
                    &mut frames,
                    ConstantFrame::Exit {
                        node,
                        value: fold_binary(&operator, &left, &right),
                        cacheable: false,
                    },
                    budget,
                    stats,
                ) {
                    on_path.clear();
                    return ConstLattice::Overdefined;
                }
            }
            ConstantFrame::Unary { node, operator } => {
                let operand = values.pop().unwrap_or(ConstLattice::Overdefined);
                if !push_machine_frame(
                    &mut frames,
                    ConstantFrame::Exit {
                        node,
                        value: fold_unary(&operator, &operand),
                        cacheable: false,
                    },
                    budget,
                    stats,
                ) {
                    on_path.clear();
                    return ConstLattice::Overdefined;
                }
            }
            ConstantFrame::Forward { node } => {
                let value = values.pop().unwrap_or(ConstLattice::Overdefined);
                if !push_machine_frame(
                    &mut frames,
                    ConstantFrame::Exit {
                        node,
                        value,
                        cacheable: false,
                    },
                    budget,
                    stats,
                ) {
                    on_path.clear();
                    return ConstLattice::Overdefined;
                }
            }
            ConstantFrame::Exit {
                node,
                value,
                cacheable,
            } => {
                if cache_enabled && cacheable && terminal_cache.try_reserve(1).is_ok() {
                    terminal_cache.insert(node, value.clone());
                }
                on_path.remove(&node);
                if !push_machine_value(&mut values, value, budget) {
                    return ConstLattice::Overdefined;
                }
            }
        }
    }
    values.pop().unwrap_or(ConstLattice::Overdefined)
}

fn const_lattice(summary: &EffectSummary) -> ConstLattice {
    if summary.const_return_overdefined {
        ConstLattice::Overdefined
    } else {
        summary
            .const_return
            .clone()
            .map_or(ConstLattice::Unknown, ConstLattice::Constant)
    }
}

fn constant_state(value: ConstLattice) -> ConstantState {
    match value {
        ConstLattice::Unknown => ConstantState::Bottom,
        ConstLattice::Constant(value) => ConstantState::Constant(value),
        ConstLattice::Overdefined => ConstantState::Overdefined,
    }
}

#[cfg(test)]
#[path = "effects/tests.rs"]
mod tests;
