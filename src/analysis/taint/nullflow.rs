//! Interprocedural maybe-null flow analysis.
//!
//! Null literals, uninitialized variables, and typed nullable returns are
//! propagated through the same typed AST/DFG substrate as taint. Dereference
//! findings are advisory and retain a canonical predecessor-chain witness.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::builder::call_arguments;
use crate::{CodePropertyGraph, CpgEdgeKind, CpgNodeKind, DfgEdgeKind, LiteralKind, NodeId};

use super::{
    call_confidence, compare_witness, insert_witness, join_witness_map, local_function,
    resolve_call, returned_values, witness_map_subsumes, CallGraphResolution, CalleeRef,
    IfdsCapEvent, IfdsCaps, IfdsIssue, IfdsLattice, IfdsOutput, LocalFunction, SummaryCtx,
    SummaryDomain, TaintWitness,
};
use crate::analysis::{call_graph_sccs, solve_summaries_with_resolver};

/// Why one concrete CPG value is conservatively maybe-null.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum NullOriginKind {
    /// An explicit null literal.
    Literal,
    /// A variable declaration with no represented initializer.
    UninitializedVariable,
    /// A call whose represented return type admits null.
    NullableReturn,
}

/// One finite maybe-null IFDS fact.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum MaybeNull {
    /// Maybe-null value entering through positional formal parameter `i`.
    Param(u16),
    /// Concrete maybe-null origin in the represented CPG.
    Origin {
        /// Why the origin is maybe-null.
        kind: NullOriginKind,
        /// Concrete CPG source node.
        node: NodeId,
    },
}

/// Stable key for one concrete maybe-null-to-dereference flow.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NullFlowKey {
    /// Why the value is maybe-null.
    pub origin_kind: NullOriginKind,
    /// Concrete maybe-null source node.
    pub origin: NodeId,
    /// Dereference node.
    pub sink: NodeId,
    /// Callee parameter position when the sink is interprocedural.
    pub param_index: Option<u16>,
}

/// Monotone maybe-null jump function for one represented procedure.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NullSummary {
    /// Number of positional formal parameters observed in the procedure.
    pub param_count: usize,
    /// Bit `i` is set when parameter `i` may reach the return value.
    pub param_to_return: u64,
    /// Whether any represented execution may return null.
    pub returns_maybe_null: bool,
    /// Canonical parameter-to-dereference witnesses keyed by `(parameter, sink)`.
    #[cfg_attr(
        feature = "serde",
        serde(with = "crate::analysis::serde_util::ordered_map")
    )]
    pub param_deref_sites: BTreeMap<(u16, NodeId), TaintWitness>,
    /// Canonical parameter or concrete-origin witnesses reaching a return value.
    #[cfg_attr(
        feature = "serde",
        serde(with = "crate::analysis::serde_util::ordered_map")
    )]
    pub return_sites: BTreeMap<MaybeNull, TaintWitness>,
    /// Concrete maybe-null-to-dereference candidate flows.
    #[cfg_attr(
        feature = "serde",
        serde(with = "crate::analysis::serde_util::ordered_map")
    )]
    pub flows: BTreeMap<NullFlowKey, TaintWitness>,
    /// Whether more parameters existed than the fixed-width return mask can encode.
    pub param_mask_truncated: bool,
    /// Whether a local or transitive resource cap made the summary partial.
    pub capped: bool,
}

impl NullSummary {
    fn mark_capped(&mut self) {
        self.capped = true;
        for witness in self
            .param_deref_sites
            .values_mut()
            .chain(self.return_sites.values_mut())
            .chain(self.flows.values_mut())
        {
            witness.mark_capped();
        }
    }
}

impl IfdsLattice for NullSummary {
    fn join(&mut self, other: &Self) -> bool {
        let before = self.clone();
        self.param_count = self.param_count.max(other.param_count);
        self.param_to_return |= other.param_to_return;
        self.returns_maybe_null |= other.returns_maybe_null;
        join_witness_map(&mut self.param_deref_sites, &other.param_deref_sites);
        join_witness_map(&mut self.return_sites, &other.return_sites);
        join_witness_map(&mut self.flows, &other.flows);
        self.param_mask_truncated |= other.param_mask_truncated;
        self.capped |= other.capped;
        *self != before
    }

    fn subsumes(&self, other: &Self) -> bool {
        self.param_count >= other.param_count
            && self.param_to_return | other.param_to_return == self.param_to_return
            && (self.returns_maybe_null || !other.returns_maybe_null)
            && witness_map_subsumes(&self.param_deref_sites, &other.param_deref_sites)
            && witness_map_subsumes(&self.return_sites, &other.return_sites)
            && witness_map_subsumes(&self.flows, &other.flows)
            && (self.param_mask_truncated || !other.param_mask_truncated)
            && (self.capped || !other.capped)
    }
}

/// One advisory maybe-null dereference candidate.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NullFlow {
    /// Why the source value is maybe-null.
    pub origin_kind: NullOriginKind,
    /// Concrete maybe-null source node.
    pub origin: NodeId,
    /// Concrete dereference sink node.
    pub sink: NodeId,
    /// Callee parameter position when the sink is interprocedural.
    pub param_index: Option<u16>,
    /// Canonical typed-transfer predecessor chain from source to sink.
    pub witness: Vec<NodeId>,
    /// Minimum type and call-resolution evidence score along the path.
    pub confidence: f32,
    /// Whether any relevant resource bound made this finding partial.
    pub capped: bool,
}

/// Maybe-null findings, summaries, and completeness evidence for one run.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NullAnalysis {
    /// Candidate flows in deterministic key order.
    pub flows: Vec<NullFlow>,
    /// Function summaries ordered by stable function identifier.
    pub summaries: BTreeMap<NodeId, NullSummary>,
    /// Functions evaluated in deterministic callees-first order.
    pub processed_functions: Vec<NodeId>,
    /// Kleene rounds executed for each call-graph component.
    pub component_iterations: BTreeMap<usize, u32>,
    /// Shared solver work consumed.
    pub worklist_steps: usize,
    /// Whether the shared solver or a domain-local traversal was capped.
    pub capped: bool,
    /// Functions whose domain summary carries cap evidence.
    pub domain_capped_functions: BTreeSet<NodeId>,
    /// Ordered shared-solver cap evidence.
    pub cap_events: BTreeSet<IfdsCapEvent>,
    /// Ordered structural or lattice-law issues.
    pub issues: BTreeSet<IfdsIssue>,
}

impl NullAnalysis {
    /// Returns whether all findings and summaries were computed completely.
    pub fn is_complete(&self) -> bool {
        !self.capped && self.issues.is_empty()
    }
}

/// Computes maybe-null dereference candidates with completeness evidence.
pub fn analyze_null_flows_report(
    cpg: &CodePropertyGraph,
    resolution: &CallGraphResolution,
    caps: &IfdsCaps,
) -> NullAnalysis {
    let domain = NullDomain::new(cpg, resolution);
    let call_graph = call_graph_sccs(cpg);
    let resolver = |call| resolve_call(cpg, resolution, call, caps.max_call_depth);
    let output = solve_summaries_with_resolver(cpg, &domain, &call_graph, caps, &resolver);
    null_output(output)
}

/// Computes deterministic advisory maybe-null dereference candidates.
///
/// Consumers making negative claims must use [`analyze_null_flows_report`] and
/// require [`NullAnalysis::is_complete`] so an empty capped result is visible.
pub fn analyze_null_flows(
    cpg: &CodePropertyGraph,
    resolution: &CallGraphResolution,
    caps: &IfdsCaps,
) -> Vec<NullFlow> {
    analyze_null_flows_report(cpg, resolution, caps).flows
}

fn null_output(output: IfdsOutput<NullDomain<'_>>) -> NullAnalysis {
    let domain_capped_functions = output
        .summaries
        .iter()
        .filter_map(|(&function, summary)| summary.capped.then_some(function))
        .collect::<BTreeSet<_>>();
    let capped = output.capped || !domain_capped_functions.is_empty();
    let mut flows = BTreeMap::new();
    for summary in output.summaries.values() {
        join_witness_map(&mut flows, &summary.flows);
    }
    let flows = flows
        .into_iter()
        .map(|(key, mut witness)| {
            witness.capped |= capped;
            NullFlow {
                origin_kind: key.origin_kind,
                origin: key.origin,
                sink: key.sink,
                param_index: key.param_index,
                confidence: witness.confidence(),
                witness: witness.nodes,
                capped: witness.capped,
            }
        })
        .collect();
    NullAnalysis {
        flows,
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

type NullEnv = BTreeMap<NodeId, BTreeMap<MaybeNull, TaintWitness>>;

#[derive(Debug)]
struct NullDomain<'a> {
    resolution: &'a CallGraphResolution,
    functions: BTreeMap<NodeId, LocalFunction>,
}

impl<'a> NullDomain<'a> {
    fn new(cpg: &CodePropertyGraph, resolution: &'a CallGraphResolution) -> Self {
        let functions = cpg
            .functions()
            .map(|node| (node.id, local_function(cpg, node.id)))
            .collect();
        Self {
            resolution,
            functions,
        }
    }
}

impl SummaryDomain for NullDomain<'_> {
    type Fact = MaybeNull;
    type Summary = NullSummary;

    fn seed_facts(&self, cpg: &CodePropertyGraph, function: NodeId) -> Vec<(NodeId, Self::Fact)> {
        let Some(local) = self.functions.get(&function) else {
            return Vec::new();
        };
        let mut seeds = local
            .params
            .iter()
            .enumerate()
            .filter_map(|(index, &parameter)| {
                u16::try_from(index)
                    .ok()
                    .map(|index| (parameter, MaybeNull::Param(index)))
            })
            .collect::<Vec<_>>();
        for &node in &local.nodes {
            if let Some(kind) = concrete_null_origin(cpg, node) {
                seeds.push((node, MaybeNull::Origin { kind, node }));
            }
        }
        for &call in &local.calls {
            if nullable_call(cpg, self.resolution, call) {
                seeds.push((
                    call,
                    MaybeNull::Origin {
                        kind: NullOriginKind::NullableReturn,
                        node: call,
                    },
                ));
            }
        }
        seeds
    }

    fn summarize(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
        ctx: &SummaryCtx<'_, Self>,
    ) -> Self::Summary {
        let Some(local) = self.functions.get(&function) else {
            let mut summary = NullSummary::default();
            summary.mark_capped();
            return summary;
        };
        summarize_null_function(cpg, function, local, self.resolution, ctx)
    }
}

fn summarize_null_function(
    cpg: &CodePropertyGraph,
    function: NodeId,
    local: &LocalFunction,
    resolution: &CallGraphResolution,
    ctx: &SummaryCtx<'_, NullDomain<'_>>,
) -> NullSummary {
    let (nulls, local_capped) = compute_nulls(cpg, local, resolution, ctx);
    let mut summary = NullSummary {
        param_count: local.params.len(),
        returns_maybe_null: function_declared_nullable(cpg, function),
        param_mask_truncated: local.params.len() > u64::BITS as usize,
        capped: local.params.len() > u64::BITS as usize,
        ..NullSummary::default()
    };

    for (sink, operands) in dereference_sinks(cpg, local) {
        for operand in operands {
            for (fact, witness) in nulls_for_value(cpg, &nulls, operand) {
                record_null_sink(&mut summary, fact, sink, None, witness.append(sink));
            }
        }
    }

    for &call in &local.calls {
        if let CalleeRef::Resolved(callee) = (ctx.resolve)(call) {
            if let Some(callee_summary) = ctx.summaries.get(&callee) {
                let arguments = call_arguments(cpg, call);
                let confidence = call_confidence(resolution, call);
                for (&(param, sink), suffix) in &callee_summary.param_deref_sites {
                    let Some(&argument) = arguments.get(usize::from(param)) else {
                        continue;
                    };
                    for (fact, witness) in nulls_for_value(cpg, &nulls, argument) {
                        record_null_sink(
                            &mut summary,
                            fact,
                            sink,
                            Some(param),
                            witness.compose(suffix, confidence),
                        );
                    }
                }
                join_witness_map(&mut summary.flows, &callee_summary.flows);
                if callee_summary.capped {
                    summary.mark_capped();
                }
            }
        }
    }

    for value in returned_values(cpg, local) {
        for (fact, witness) in nulls_for_value(cpg, &nulls, value) {
            match fact {
                MaybeNull::Param(index) if index < 64 => {
                    summary.param_to_return |= 1u64 << index;
                    insert_witness(
                        &mut summary.return_sites,
                        MaybeNull::Param(index),
                        witness.append(value),
                    );
                }
                MaybeNull::Param(_) => {
                    summary.param_mask_truncated = true;
                    summary.capped = true;
                }
                origin @ MaybeNull::Origin { .. } => {
                    summary.returns_maybe_null = true;
                    insert_witness(&mut summary.return_sites, origin, witness.append(value));
                }
            }
        }
    }

    if local_capped
        || summary
            .param_deref_sites
            .values()
            .chain(summary.return_sites.values())
            .chain(summary.flows.values())
            .any(|witness| witness.capped)
    {
        summary.mark_capped();
    }
    summary
}

fn compute_nulls(
    cpg: &CodePropertyGraph,
    local: &LocalFunction,
    resolution: &CallGraphResolution,
    ctx: &SummaryCtx<'_, NullDomain<'_>>,
) -> (NullEnv, bool) {
    let mut nulls = NullEnv::new();
    let mut capped = false;
    for (index, &parameter) in local.params.iter().enumerate() {
        let Ok(index) = u16::try_from(index) else {
            capped = true;
            continue;
        };
        add_null(
            &mut nulls,
            parameter,
            MaybeNull::Param(index),
            TaintWitness::at(parameter, 1_000),
            ctx.caps,
            &mut capped,
        );
    }
    for &node in &local.nodes {
        if let Some(kind) = concrete_null_origin(cpg, node) {
            add_null(
                &mut nulls,
                node,
                MaybeNull::Origin { kind, node },
                TaintWitness::at(node, 1_000),
                ctx.caps,
                &mut capped,
            );
        }
    }

    let mut work = 0usize;
    loop {
        let mut changed = false;
        for &node in &local.nodes {
            if work >= ctx.caps.max_worklist_steps {
                mark_null_env_capped(&mut nulls);
                return (nulls, true);
            }
            work = work.saturating_add(1);
            match cpg.node(node).map(|node| &node.kind) {
                Some(CpgNodeKind::Call { .. }) => {
                    for (fact, witness) in call_result_nulls(cpg, node, &nulls, resolution, ctx) {
                        changed |= add_null(
                            &mut nulls,
                            node,
                            fact,
                            witness.append(node),
                            ctx.caps,
                            &mut capped,
                        );
                    }
                }
                Some(CpgNodeKind::Identifier { definition, .. }) => {
                    let mut definitions = cpg.reaching_definitions(node);
                    if let Some(definition) = definition {
                        definitions.push(*definition);
                    }
                    definitions.sort_unstable();
                    definitions.dedup();
                    for definition in definitions {
                        changed |=
                            copy_nulls(&mut nulls, definition, node, 1_000, ctx.caps, &mut capped);
                    }
                }
                Some(CpgNodeKind::Parameter { .. } | CpgNodeKind::Function { .. }) => {}
                Some(CpgNodeKind::Assignment { .. }) => {
                    for &child in cpg.ast_children(node).iter().skip(1) {
                        changed |=
                            copy_value_nulls(cpg, &mut nulls, child, node, ctx.caps, &mut capped);
                    }
                }
                Some(_) => {
                    for &child in cpg.ast_children(node) {
                        changed |=
                            copy_nulls(&mut nulls, child, node, 1_000, ctx.caps, &mut capped);
                    }
                }
                None => {}
            }
        }
        for &(source, target) in &local.dfg_edges {
            if work >= ctx.caps.max_worklist_steps {
                mark_null_env_capped(&mut nulls);
                return (nulls, true);
            }
            work = work.saturating_add(1);
            if cpg
                .node(target)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Call { .. }))
            {
                continue;
            }
            changed |= copy_nulls(&mut nulls, source, target, 1_000, ctx.caps, &mut capped);
        }
        if !changed {
            break;
        }
    }
    if capped {
        mark_null_env_capped(&mut nulls);
    }
    (nulls, capped)
}

fn call_result_nulls(
    cpg: &CodePropertyGraph,
    call: NodeId,
    nulls: &NullEnv,
    resolution: &CallGraphResolution,
    ctx: &SummaryCtx<'_, NullDomain<'_>>,
) -> BTreeMap<MaybeNull, TaintWitness> {
    let arguments = call_arguments(cpg, call);
    let confidence = call_confidence(resolution, call);
    let mut result = BTreeMap::new();
    match (ctx.resolve)(call) {
        CalleeRef::Resolved(callee) => {
            if let Some(summary) = ctx.summaries.get(&callee) {
                for (&index, suffix) in
                    summary
                        .return_sites
                        .iter()
                        .filter_map(|(fact, witness)| match fact {
                            MaybeNull::Param(index) => Some((index, witness)),
                            MaybeNull::Origin { .. } => None,
                        })
                {
                    let Some(&argument) = arguments.get(usize::from(index)) else {
                        continue;
                    };
                    for (fact, witness) in nulls_for_value(cpg, nulls, argument) {
                        insert_witness(
                            &mut result,
                            fact,
                            witness.compose(suffix, confidence).append(call),
                        );
                    }
                }
                for (fact, suffix) in &summary.return_sites {
                    if matches!(fact, MaybeNull::Origin { .. }) {
                        insert_witness(
                            &mut result,
                            fact.clone(),
                            suffix.with_confidence(confidence).append(call),
                        );
                    }
                }
            }
            if function_declared_nullable(cpg, callee) && result.is_empty() {
                insert_witness(
                    &mut result,
                    MaybeNull::Origin {
                        kind: NullOriginKind::NullableReturn,
                        node: call,
                    },
                    TaintWitness::at(call, confidence.min(850)),
                );
            }
        }
        CalleeRef::External(_) | CalleeRef::Unresolved(_) => {
            if nullable_call(cpg, resolution, call) {
                insert_witness(
                    &mut result,
                    MaybeNull::Origin {
                        kind: NullOriginKind::NullableReturn,
                        node: call,
                    },
                    TaintWitness::at(call, 750),
                );
            }
        }
    }
    result
}

fn dereference_sinks(cpg: &CodePropertyGraph, local: &LocalFunction) -> Vec<(NodeId, Vec<NodeId>)> {
    let mut sinks = BTreeMap::<NodeId, BTreeSet<NodeId>>::new();
    for edge in cpg.edges() {
        if matches!(edge.kind, CpgEdgeKind::DataFlow(DfgEdgeKind::Dereference))
            && local.scope.contains(&edge.source)
            && local.scope.contains(&edge.target)
        {
            sinks.entry(edge.target).or_default().insert(edge.source);
            sinks.entry(edge.target).or_default().insert(edge.target);
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
    sinks
        .into_iter()
        .map(|(sink, operands)| (sink, operands.into_iter().collect()))
        .collect()
}

fn record_null_sink(
    summary: &mut NullSummary,
    fact: MaybeNull,
    sink: NodeId,
    param_index: Option<u16>,
    witness: TaintWitness,
) {
    match fact {
        MaybeNull::Param(param) => {
            insert_witness(&mut summary.param_deref_sites, (param, sink), witness);
        }
        MaybeNull::Origin { kind, node } => {
            insert_witness(
                &mut summary.flows,
                NullFlowKey {
                    origin_kind: kind,
                    origin: node,
                    sink,
                    param_index,
                },
                witness,
            );
        }
    }
}

fn concrete_null_origin(cpg: &CodePropertyGraph, node: NodeId) -> Option<NullOriginKind> {
    match cpg.node(node).map(|node| &node.kind) {
        Some(CpgNodeKind::Literal {
            kind: LiteralKind::Null,
        }) => Some(NullOriginKind::Literal),
        Some(CpgNodeKind::Variable { name, .. })
            if variable_initializer_nodes(cpg, node, name).is_empty() =>
        {
            Some(NullOriginKind::UninitializedVariable)
        }
        _ => None,
    }
}

fn variable_initializer_nodes(
    cpg: &CodePropertyGraph,
    variable: NodeId,
    name: &str,
) -> Vec<NodeId> {
    cpg.ast_children(variable)
        .iter()
        .copied()
        .filter(|&child| {
            !matches!(
                cpg.node(child).map(|node| &node.kind),
                Some(CpgNodeKind::Identifier { name: child_name, .. })
                    if child_name.as_ref() == name
            )
        })
        .collect()
}

fn nullable_call(cpg: &CodePropertyGraph, resolution: &CallGraphResolution, call: NodeId) -> bool {
    if let CalleeRef::Resolved(target) = resolve_call(cpg, resolution, call, 128) {
        return function_declared_nullable(cpg, target);
    }
    cpg.outgoing_edges(call).any(|edge| {
        matches!(edge.kind, CpgEdgeKind::TypeOf)
            && cpg.node(edge.target).is_some_and(|node| match &node.kind {
                CpgNodeKind::TypeAnnotation { type_info } => nullable_type(&type_info.name),
                _ => false,
            })
    })
}

fn function_declared_nullable(cpg: &CodePropertyGraph, function: NodeId) -> bool {
    matches!(
        cpg.node(function).map(|node| &node.kind),
        Some(CpgNodeKind::Function { signature })
            if signature
                .return_type
                .as_ref()
                .is_some_and(|type_info| nullable_type(&type_info.name))
    )
}

fn nullable_type(name: &str) -> bool {
    let lowercase = name.to_ascii_lowercase();
    lowercase.ends_with('?')
        || lowercase.starts_with('*')
        || lowercase
            .split(|character: char| !character.is_alphanumeric() && character != '_')
            .any(|segment| {
                matches!(
                    segment,
                    "option" | "optional" | "nullable" | "maybe" | "null"
                )
            })
}

fn nulls_for_value(
    cpg: &CodePropertyGraph,
    nulls: &NullEnv,
    value: NodeId,
) -> BTreeMap<MaybeNull, TaintWitness> {
    let mut result = nulls.get(&value).cloned().unwrap_or_default();
    let mut descendants = cpg.ast_descendants(value);
    descendants.sort_unstable();
    for descendant in descendants {
        if let Some(found) = nulls.get(&descendant) {
            join_witness_map(&mut result, found);
        }
    }
    result
}

fn copy_value_nulls(
    cpg: &CodePropertyGraph,
    nulls: &mut NullEnv,
    source: NodeId,
    target: NodeId,
    caps: &IfdsCaps,
    capped: &mut bool,
) -> bool {
    let found = nulls_for_value(cpg, nulls, source);
    add_nulls(nulls, target, found, 1_000, caps, capped)
}

fn copy_nulls(
    nulls: &mut NullEnv,
    source: NodeId,
    target: NodeId,
    confidence: u16,
    caps: &IfdsCaps,
    capped: &mut bool,
) -> bool {
    let found = nulls.get(&source).cloned().unwrap_or_default();
    add_nulls(nulls, target, found, confidence, caps, capped)
}

fn add_nulls(
    nulls: &mut NullEnv,
    target: NodeId,
    found: BTreeMap<MaybeNull, TaintWitness>,
    confidence: u16,
    caps: &IfdsCaps,
    capped: &mut bool,
) -> bool {
    let mut changed = false;
    for (fact, witness) in found {
        changed |= add_null(
            nulls,
            target,
            fact,
            witness.with_confidence(confidence).append(target),
            caps,
            capped,
        );
    }
    changed
}

fn add_null(
    nulls: &mut NullEnv,
    node: NodeId,
    fact: MaybeNull,
    witness: TaintWitness,
    caps: &IfdsCaps,
    capped: &mut bool,
) -> bool {
    let node_nulls = nulls.entry(node).or_default();
    if let Some(current) = node_nulls.get_mut(&fact) {
        if compare_witness(&witness, current) == Ordering::Less {
            *current = witness;
            return true;
        }
        return false;
    }
    if node_nulls.len() >= caps.max_facts_per_node {
        *capped = true;
        return false;
    }
    node_nulls.insert(fact, witness);
    true
}

fn mark_null_env_capped(nulls: &mut NullEnv) {
    for witness in nulls.values_mut().flat_map(BTreeMap::values_mut) {
        witness.mark_capped();
    }
}

#[cfg(test)]
#[path = "nullflow_tests.rs"]
mod tests;
