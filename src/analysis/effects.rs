//! Deterministic interprocedural constant and side-effect summaries.
//!
//! The analysis is an out-of-graph [`SummaryDomain`] over the canonical call
//! graph condensation. It classifies only evidence represented by CPG node and
//! edge kinds: parameter flow, global and field access, resolved callees, and
//! scalar AST expressions. It never parses source text or dispatches on a
//! language.

use std::collections::{BTreeMap, BTreeSet};
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
        let mut dependency_seen = BTreeSet::new();
        summary.param_to_return |= parameter_dependencies(
            cpg,
            expression,
            local,
            ctx,
            0,
            &mut dependency_seen,
            &mut budget,
        );
        let mut on_path = BTreeSet::new();
        let value = evaluate_constant(cpg, expression, ctx, 0, &mut on_path, &mut budget);
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
    if !seen.insert(node) {
        return BTreeSet::new();
    }
    let result = match cpg.node(node).map(|node| &node.kind) {
        Some(CpgNodeKind::Identifier { .. }) => places_for_identifier(cpg, node, local),
        Some(CpgNodeKind::Parameter { .. }) => local
            .params
            .get(&node)
            .copied()
            .map_or_else(BTreeSet::new, |index| BTreeSet::from([Place::Param(index)])),
        Some(CpgNodeKind::Field { name, .. } | CpgNodeKind::MemberAccess { member: name }) => {
            BTreeSet::from([Place::Field(name.clone())])
        }
        Some(CpgNodeKind::IndexAccess) => cpg
            .ast_children(node)
            .first()
            .copied()
            .map_or_else(BTreeSet::new, |base| {
                places_for_value(cpg, base, local, seen)
            }),
        _ => BTreeSet::new(),
    };
    seen.remove(&node);
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

#[allow(clippy::too_many_arguments)]
fn parameter_dependencies(
    cpg: &CodePropertyGraph,
    node: NodeId,
    local: &LocalEffects,
    ctx: &SummaryCtx<'_, EffectDomain>,
    depth: u32,
    seen: &mut BTreeSet<NodeId>,
    budget: &mut TraversalBudget,
) -> u64 {
    if !budget.enter(depth, ctx.caps) {
        return 0;
    }
    if !seen.insert(node) {
        budget.capped = true;
        return u64::MAX;
    }
    let mut mask = parameter_mask_for_node(cpg, node, local);
    if matches!(
        cpg.node(node).map(|node| &node.kind),
        Some(CpgNodeKind::Call { .. })
    ) {
        let arguments = call_arguments(cpg, node);
        match (ctx.resolve)(node) {
            CalleeRef::Resolved(callee) => {
                if let Some(callee_summary) = ctx.summaries.get(&callee) {
                    for (index, argument) in arguments.into_iter().enumerate() {
                        if callee_summary.param_mask_truncated {
                            budget.capped = true;
                        }
                        let represented_dependency =
                            index < 64 && callee_summary.param_to_return & (1u64 << index) != 0;
                        if represented_dependency || callee_summary.param_mask_truncated {
                            mask |= parameter_dependencies(
                                cpg,
                                argument,
                                local,
                                ctx,
                                depth.saturating_add(1),
                                seen,
                                budget,
                            );
                        }
                    }
                }
            }
            CalleeRef::External(_) | CalleeRef::Unresolved(_) => {
                for argument in arguments {
                    mask |= parameter_dependencies(
                        cpg,
                        argument,
                        local,
                        ctx,
                        depth.saturating_add(1),
                        seen,
                        budget,
                    );
                }
            }
        }
    } else {
        for child in cpg.ast_children(node) {
            mask |= parameter_dependencies(
                cpg,
                child,
                local,
                ctx,
                depth.saturating_add(1),
                seen,
                budget,
            );
        }
        for definition in cpg.reaching_definitions(node) {
            mask |= parameter_dependencies(
                cpg,
                definition,
                local,
                ctx,
                depth.saturating_add(1),
                seen,
                budget,
            );
        }
    }
    seen.remove(&node);
    mask
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

#[allow(clippy::too_many_arguments)]
fn evaluate_constant(
    cpg: &CodePropertyGraph,
    node: NodeId,
    ctx: &SummaryCtx<'_, EffectDomain>,
    depth: u32,
    on_path: &mut BTreeSet<NodeId>,
    budget: &mut TraversalBudget,
) -> ConstLattice {
    if !budget.enter(depth, ctx.caps) {
        return ConstLattice::Overdefined;
    }
    if !on_path.insert(node) {
        return ConstLattice::Overdefined;
    }
    let value =
        match cpg.node(node).map(|node| &node.kind) {
            Some(CpgNodeKind::Literal { kind }) => literal_value(kind),
            Some(CpgNodeKind::Call { .. }) => match (ctx.resolve)(node) {
                CalleeRef::Resolved(callee) => ctx
                    .summaries
                    .get(&callee)
                    .map_or(ConstLattice::Overdefined, const_lattice),
                CalleeRef::External(_) | CalleeRef::Unresolved(_) => ConstLattice::Overdefined,
            },
            Some(CpgNodeKind::Identifier { .. }) => {
                let mut definitions = cpg.reaching_definitions(node);
                if let Some(CpgNodeKind::Identifier {
                    definition: Some(definition),
                    ..
                }) = cpg.node(node).map(|node| &node.kind)
                {
                    definitions.push(*definition);
                }
                definitions.sort_unstable();
                definitions.dedup();
                if definitions.is_empty() {
                    ConstLattice::Overdefined
                } else {
                    let mut result = ConstLattice::Unknown;
                    for definition in definitions {
                        let definition_value = evaluate_constant(
                            cpg,
                            definition,
                            ctx,
                            depth.saturating_add(1),
                            on_path,
                            budget,
                        );
                        DataflowLattice::join(&mut result, &definition_value);
                    }
                    result
                }
            }
            Some(CpgNodeKind::BinaryOp { operator }) => {
                let children = cpg.ast_children(node);
                match children.as_slice() {
                    [left, right, ..] => {
                        let left = evaluate_constant(
                            cpg,
                            *left,
                            ctx,
                            depth.saturating_add(1),
                            on_path,
                            budget,
                        );
                        let right = evaluate_constant(
                            cpg,
                            *right,
                            ctx,
                            depth.saturating_add(1),
                            on_path,
                            budget,
                        );
                        fold_binary(operator, &left, &right)
                    }
                    _ => ConstLattice::Overdefined,
                }
            }
            Some(CpgNodeKind::UnaryOp { operator }) => cpg
                .ast_children(node)
                .first()
                .copied()
                .map_or(ConstLattice::Overdefined, |operand| {
                    let operand = evaluate_constant(
                        cpg,
                        operand,
                        ctx,
                        depth.saturating_add(1),
                        on_path,
                        budget,
                    );
                    fold_unary(operator, &operand)
                }),
            Some(CpgNodeKind::Assignment { .. }) => cpg
                .ast_children(node)
                .get(1)
                .copied()
                .map_or(ConstLattice::Overdefined, |rhs| {
                    evaluate_constant(cpg, rhs, ctx, depth.saturating_add(1), on_path, budget)
                }),
            Some(CpgNodeKind::Variable { name, .. }) => cpg
                .ast_children(node)
                .into_iter()
                .rev()
                .find(|&child| {
                    !matches!(
                        cpg.node(child).map(|node| &node.kind),
                        Some(CpgNodeKind::Identifier { name: child_name, .. }) if child_name == name
                    )
                })
                .map_or(ConstLattice::Overdefined, |initializer| {
                    evaluate_constant(
                        cpg,
                        initializer,
                        ctx,
                        depth.saturating_add(1),
                        on_path,
                        budget,
                    )
                }),
            Some(CpgNodeKind::Return) => cpg.ast_children(node).first().copied().map_or(
                ConstLattice::Overdefined,
                |returned| {
                    evaluate_constant(cpg, returned, ctx, depth.saturating_add(1), on_path, budget)
                },
            ),
            Some(CpgNodeKind::Parameter { .. }) => ConstLattice::Overdefined,
            _ => ConstLattice::Overdefined,
        };
    on_path.remove(&node);
    value
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
