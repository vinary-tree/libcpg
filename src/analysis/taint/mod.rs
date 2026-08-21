//! Deterministic, summary-based taint analysis over typed CPG relations.
//!
//! The analysis is advisory: a [`TaintFlow`] is evidence of a candidate flow,
//! never a security verdict. Procedure summaries are solved over call-graph
//! strongly connected components by the shared bounded IFDS framework. Local
//! transfer uses typed AST value composition and DFG edges; source text is
//! neither retained nor parsed by this module.

mod catalog;
mod nullflow;

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::builder::call_arguments;
use crate::{CodePropertyGraph, CpgEdgeKind, CpgNodeKind, DfgEdgeKind, NodeId};

use super::resolve::{call_callee_path, call_callee_path_evidence};
use super::{
    call_graph_sccs, function_scope_nodes, solve_summaries_with_resolver, CallGraphResolution,
    CalleeRef, Confidence, IfdsCapEvent, IfdsCaps, IfdsIssue, IfdsLattice, IfdsOutput, SummaryCtx,
    SummaryDomain,
};

pub use catalog::{
    CatalogClassification, CatalogMatch, CatalogRule, ExternalModel, LiteralPattern,
    LiteralSourceRule, Pattern, TaintCatalog,
};
pub use nullflow::{
    analyze_null_flows, analyze_null_flows_report, MaybeNull, NullAnalysis, NullFlow, NullFlowKey,
    NullOriginKind, NullSummary,
};

/// Open source-class name supplied by a [`TaintCatalog`].
pub type SourceClass = Arc<str>;

/// Open sink-class name supplied by a [`TaintCatalog`].
pub type SinkClass = Arc<str>;

/// One finite IFDS taint fact inside a procedure.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum TaintFact {
    /// Taint entering through positional formal parameter `i`.
    Param(u16),
    /// Taint introduced by a source in the named open class.
    Ambient(SourceClass),
}

/// Canonical predecessor-chain evidence retained in a summary.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TaintWitness {
    /// CPG nodes visited by the typed transfer path, in flow order.
    pub nodes: Vec<NodeId>,
    /// Minimum evidence score along the path, scaled to `0..=1000`.
    pub confidence_milli: u16,
    /// Whether a resource bound made this witness part of a partial result.
    pub capped: bool,
}

impl TaintWitness {
    /// Returns the evidence score in the inclusive interval `[0, 1]`.
    pub fn confidence(&self) -> f32 {
        f32::from(self.confidence_milli) / 1_000.0
    }

    fn at(node: NodeId, confidence_milli: u16) -> Self {
        Self {
            nodes: vec![node],
            confidence_milli,
            capped: false,
        }
    }

    fn append(&self, node: NodeId) -> Self {
        let mut result = self.clone();
        if result.nodes.last().copied() != Some(node) {
            result.nodes.push(node);
        }
        result
    }

    fn with_confidence(&self, confidence_milli: u16) -> Self {
        let mut result = self.clone();
        result.confidence_milli = result.confidence_milli.min(confidence_milli);
        result
    }

    fn compose(&self, suffix: &Self, confidence_milli: u16) -> Self {
        let mut result = self.clone();
        for &node in &suffix.nodes {
            if result.nodes.last().copied() != Some(node) {
                result.nodes.push(node);
            }
        }
        result.confidence_milli = result
            .confidence_milli
            .min(suffix.confidence_milli)
            .min(confidence_milli);
        result.capped |= suffix.capped;
        result
    }

    fn mark_capped(&mut self) {
        self.capped = true;
    }
}

/// Key for a formal-parameter-to-sink summary edge.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ParamSinkKey {
    /// Positional formal parameter.
    pub param: u16,
    /// Open sink class.
    pub sink: SinkClass,
    /// Optional Common Weakness Enumeration identifier.
    pub cwe: Option<u32>,
}

/// Key preserving one concrete source site that reaches a return value.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ReturnSourceKey {
    /// Open source class.
    pub class: SourceClass,
    /// Concrete source node retained for witness reconstruction.
    pub source: NodeId,
}

/// Key for one concrete ambient source-to-sink flow.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AmbientFlowKey {
    /// Open source class.
    pub source_class: SourceClass,
    /// Open sink class.
    pub sink_class: SinkClass,
    /// Optional Common Weakness Enumeration identifier.
    pub cwe: Option<u32>,
    /// Concrete source node.
    pub source: NodeId,
    /// Concrete sink node.
    pub sink: NodeId,
    /// Sink-argument position when the flow crosses a call boundary.
    pub param_index: Option<u16>,
}

/// Monotone taint jump function for one represented procedure.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TaintSummary {
    /// Number of positional formal parameters observed in the procedure.
    pub param_count: usize,
    /// `param_sinks[i]` contains every sink class reachable from parameter `i`.
    pub param_sinks: Vec<BTreeSet<SinkClass>>,
    /// Bit `i` is set when parameter `i` may reach the return value.
    pub param_to_return: u64,
    /// Source classes that may reach the return value.
    pub return_sources: BTreeSet<SourceClass>,
    /// Canonical witnesses for formal-parameter-to-sink summary edges.
    #[cfg_attr(feature = "serde", serde(with = "super::serde_util::ordered_map"))]
    pub param_sink_sites: BTreeMap<ParamSinkKey, TaintWitness>,
    /// Canonical witnesses for formal-parameter-to-return summary edges.
    pub param_return_sites: BTreeMap<u16, TaintWitness>,
    /// Canonical witnesses for concrete source-to-return summary edges.
    #[cfg_attr(feature = "serde", serde(with = "super::serde_util::ordered_map"))]
    pub return_source_sites: BTreeMap<ReturnSourceKey, TaintWitness>,
    /// Concrete source-to-sink findings discovered in this procedure or callees.
    #[cfg_attr(feature = "serde", serde(with = "super::serde_util::ordered_map"))]
    pub ambient_flows: BTreeMap<AmbientFlowKey, TaintWitness>,
    /// Whether more parameters existed than the fixed-width return mask can encode.
    pub param_mask_truncated: bool,
    /// Whether a local or transitive resource cap made the summary partial.
    pub capped: bool,
}

impl TaintSummary {
    fn add_param_sink(
        &mut self,
        param: u16,
        sink: SinkClass,
        cwe: Option<u32>,
        witness: TaintWitness,
    ) {
        let index = usize::from(param);
        if self.param_sinks.len() <= index {
            self.param_sinks.resize_with(index + 1, BTreeSet::new);
        }
        self.param_sinks[index].insert(sink.clone());
        self.param_count = self.param_count.max(index.saturating_add(1));
        insert_witness(
            &mut self.param_sink_sites,
            ParamSinkKey { param, sink, cwe },
            witness,
        );
    }

    fn mark_capped(&mut self) {
        self.capped = true;
        for witness in self
            .param_sink_sites
            .values_mut()
            .chain(self.param_return_sites.values_mut())
            .chain(self.return_source_sites.values_mut())
            .chain(self.ambient_flows.values_mut())
        {
            witness.mark_capped();
        }
    }
}

impl IfdsLattice for TaintSummary {
    fn join(&mut self, other: &Self) -> bool {
        let before = self.clone();
        self.param_count = self.param_count.max(other.param_count);
        if self.param_sinks.len() < other.param_sinks.len() {
            self.param_sinks
                .resize_with(other.param_sinks.len(), BTreeSet::new);
        }
        for (index, sinks) in other.param_sinks.iter().enumerate() {
            self.param_sinks[index].extend(sinks.iter().cloned());
        }
        self.param_to_return |= other.param_to_return;
        self.return_sources
            .extend(other.return_sources.iter().cloned());
        join_witness_map(&mut self.param_sink_sites, &other.param_sink_sites);
        join_witness_map(&mut self.param_return_sites, &other.param_return_sites);
        join_witness_map(&mut self.return_source_sites, &other.return_source_sites);
        join_witness_map(&mut self.ambient_flows, &other.ambient_flows);
        self.param_mask_truncated |= other.param_mask_truncated;
        self.capped |= other.capped;
        *self != before
    }

    fn subsumes(&self, other: &Self) -> bool {
        self.param_count >= other.param_count
            && sets_by_index_subsume(&self.param_sinks, &other.param_sinks)
            && self.param_to_return | other.param_to_return == self.param_to_return
            && self.return_sources.is_superset(&other.return_sources)
            && witness_map_subsumes(&self.param_sink_sites, &other.param_sink_sites)
            && witness_map_subsumes(&self.param_return_sites, &other.param_return_sites)
            && witness_map_subsumes(&self.return_source_sites, &other.return_source_sites)
            && witness_map_subsumes(&self.ambient_flows, &other.ambient_flows)
            && (self.param_mask_truncated || !other.param_mask_truncated)
            && (self.capped || !other.capped)
    }
}

/// One advisory candidate taint flow with ordered path evidence.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TaintFlow {
    /// Open source class.
    pub source_class: SourceClass,
    /// Open sink class.
    pub sink_class: SinkClass,
    /// Optional Common Weakness Enumeration identifier.
    pub cwe: Option<u32>,
    /// Concrete source node.
    pub source: NodeId,
    /// Concrete sink node.
    pub sink: NodeId,
    /// Sink-argument position when known.
    pub param_index: Option<u16>,
    /// Canonical typed-transfer predecessor chain from source to sink.
    pub witness: Vec<NodeId>,
    /// Minimum classification and resolution evidence score along the path.
    pub confidence: f32,
    /// Whether any relevant resource bound made this finding partial.
    pub capped: bool,
}

/// Taint findings, summaries, and completeness evidence for one analysis run.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TaintAnalysis {
    /// Candidate flows in deterministic key order.
    pub flows: Vec<TaintFlow>,
    /// Function summaries ordered by stable function identifier.
    pub summaries: BTreeMap<NodeId, TaintSummary>,
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

impl TaintAnalysis {
    /// Returns whether all candidate flows and summaries were computed completely.
    pub fn is_complete(&self) -> bool {
        !self.capped && self.issues.is_empty()
    }
}

/// Computes candidate taint flows while preserving summaries and cap evidence.
pub fn analyze_taint_report(
    cpg: &CodePropertyGraph,
    resolution: &CallGraphResolution,
    catalog: &TaintCatalog,
    caps: &IfdsCaps,
) -> TaintAnalysis {
    let domain = TaintDomain::new(cpg, resolution, catalog, caps.max_call_depth);
    let call_graph = call_graph_sccs(cpg);
    let resolver = |call| resolve_call(cpg, resolution, call, caps.max_call_depth);
    let output = solve_summaries_with_resolver(cpg, &domain, &call_graph, caps, &resolver);
    taint_output(output)
}

/// Computes deterministic advisory candidate flows.
///
/// Consumers making negative security claims must use
/// [`analyze_taint_report`] and require [`TaintAnalysis::is_complete`], because
/// this convenience projection cannot expose an empty-but-capped result.
pub fn analyze_taint(
    cpg: &CodePropertyGraph,
    resolution: &CallGraphResolution,
    catalog: &TaintCatalog,
    caps: &IfdsCaps,
) -> Vec<TaintFlow> {
    analyze_taint_report(cpg, resolution, catalog, caps).flows
}

fn taint_output(output: IfdsOutput<TaintDomain<'_>>) -> TaintAnalysis {
    let domain_capped_functions = output
        .summaries
        .iter()
        .filter_map(|(&function, summary)| summary.capped.then_some(function))
        .collect::<BTreeSet<_>>();
    let capped = output.capped || !domain_capped_functions.is_empty();
    let mut flows = BTreeMap::new();
    for summary in output.summaries.values() {
        join_witness_map(&mut flows, &summary.ambient_flows);
    }
    let flows = flows
        .into_iter()
        .map(|(key, mut witness)| {
            witness.capped |= capped;
            TaintFlow {
                source_class: key.source_class,
                sink_class: key.sink_class,
                cwe: key.cwe,
                source: key.source,
                sink: key.sink,
                param_index: key.param_index,
                confidence: witness.confidence(),
                witness: witness.nodes,
                capped: witness.capped,
            }
        })
        .collect();
    TaintAnalysis {
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct TokenKey {
    fact: TaintFact,
    origin: NodeId,
}

type TokenSet = BTreeMap<TokenKey, TaintWitness>;
type TokenEnv = BTreeMap<NodeId, TokenSet>;

#[derive(Debug, Clone)]
struct LocalFunction {
    function: NodeId,
    scope: BTreeSet<NodeId>,
    nodes: Vec<NodeId>,
    params: Vec<NodeId>,
    calls: Vec<NodeId>,
    returns: Vec<NodeId>,
    dfg_edges: Vec<(NodeId, NodeId)>,
}

#[derive(Debug)]
struct TaintDomain<'a> {
    resolution: &'a CallGraphResolution,
    catalog: &'a TaintCatalog,
    max_call_depth: u32,
    functions: BTreeMap<NodeId, LocalFunction>,
}

impl<'a> TaintDomain<'a> {
    fn new(
        cpg: &CodePropertyGraph,
        resolution: &'a CallGraphResolution,
        catalog: &'a TaintCatalog,
        max_call_depth: u32,
    ) -> Self {
        let functions = cpg
            .functions()
            .map(|node| (node.id, local_function(cpg, node.id)))
            .collect();
        Self {
            resolution,
            catalog,
            max_call_depth,
            functions,
        }
    }
}

impl SummaryDomain for TaintDomain<'_> {
    type Fact = TaintFact;
    type Summary = TaintSummary;

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
                    .map(|index| (parameter, TaintFact::Param(index)))
            })
            .collect::<Vec<_>>();
        for &call in &local.calls {
            let Some(path) = callee_path(cpg, self.resolution, call, self.max_call_depth) else {
                continue;
            };
            let classification = self.catalog.classify(&path);
            if !classification.sanitizers.is_empty() {
                continue;
            }
            let has_sources = !classification.sources.is_empty();
            let has_sinks = !classification.sinks.is_empty();
            for source in &classification.sources {
                seeds.push((call, TaintFact::Ambient(source.class.clone())));
            }
            if !has_sources && !has_sinks {
                if let ExternalModel::Source { class } = self.catalog.external_model(&path) {
                    seeds.push((call, TaintFact::Ambient(class)));
                }
            }
        }
        for &node in &local.nodes {
            let Some(CpgNodeKind::Literal { kind }) = cpg.node(node).map(|node| &node.kind) else {
                continue;
            };
            for source in self.catalog.classify_literal(kind) {
                seeds.push((node, TaintFact::Ambient(source.class)));
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
            let mut summary = TaintSummary::default();
            summary.mark_capped();
            return summary;
        };
        summarize_function(cpg, local, self.resolution, self.catalog, ctx)
    }
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
    let returns = nodes
        .iter()
        .copied()
        .filter(|&node| {
            cpg.node(node)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Return))
        })
        .collect();
    let mut dfg_edges = cpg
        .edges()
        .filter_map(|edge| {
            let CpgEdgeKind::DataFlow(kind) = edge.kind else {
                return None;
            };
            (scope.contains(&edge.source)
                && scope.contains(&edge.target)
                && !matches!(kind, DfgEdgeKind::UseDef))
            .then_some((edge.source, edge.target))
        })
        .collect::<Vec<_>>();
    dfg_edges.sort_unstable();
    dfg_edges.dedup();
    LocalFunction {
        function,
        scope,
        nodes,
        params,
        calls,
        returns,
        dfg_edges,
    }
}

fn summarize_function(
    cpg: &CodePropertyGraph,
    local: &LocalFunction,
    resolution: &CallGraphResolution,
    catalog: &TaintCatalog,
    ctx: &SummaryCtx<'_, TaintDomain<'_>>,
) -> TaintSummary {
    let (tokens, local_capped) = compute_tokens(cpg, local, resolution, catalog, ctx);
    let mut summary = TaintSummary {
        param_count: local.params.len(),
        param_sinks: vec![BTreeSet::new(); local.params.len()],
        param_mask_truncated: local.params.len() > u64::BITS as usize,
        capped: local.params.len() > u64::BITS as usize,
        ..TaintSummary::default()
    };

    for &call in &local.calls {
        summarize_call_sinks(cpg, call, &tokens, resolution, catalog, ctx, &mut summary);
    }
    for value in returned_values(cpg, local) {
        for (token, witness) in tokens_for_value(cpg, &tokens, value) {
            match token.fact {
                TaintFact::Param(index) => {
                    if index < 64 {
                        summary.param_to_return |= 1u64 << index;
                        insert_witness(
                            &mut summary.param_return_sites,
                            index,
                            witness.append(value),
                        );
                    } else {
                        summary.param_mask_truncated = true;
                        summary.capped = true;
                    }
                }
                TaintFact::Ambient(class) => {
                    summary.return_sources.insert(class.clone());
                    insert_witness(
                        &mut summary.return_source_sites,
                        ReturnSourceKey {
                            class,
                            source: token.origin,
                        },
                        witness.append(value),
                    );
                }
            }
        }
    }

    if local_capped
        || summary
            .param_sink_sites
            .values()
            .chain(summary.param_return_sites.values())
            .chain(summary.return_source_sites.values())
            .chain(summary.ambient_flows.values())
            .any(|witness| witness.capped)
    {
        summary.mark_capped();
    }
    summary
}

fn compute_tokens(
    cpg: &CodePropertyGraph,
    local: &LocalFunction,
    resolution: &CallGraphResolution,
    catalog: &TaintCatalog,
    ctx: &SummaryCtx<'_, TaintDomain<'_>>,
) -> (TokenEnv, bool) {
    let mut tokens = TokenEnv::new();
    let mut capped = false;
    for &call in &local.calls {
        capped |= callee_path_evidence(cpg, resolution, call, ctx.caps.max_call_depth).capped;
    }
    for (index, &parameter) in local.params.iter().enumerate() {
        let Ok(index) = u16::try_from(index) else {
            capped = true;
            continue;
        };
        let token = TokenKey {
            fact: TaintFact::Param(index),
            origin: parameter,
        };
        add_token(
            &mut tokens,
            parameter,
            token,
            TaintWitness::at(parameter, 1_000),
            ctx.caps,
            &mut capped,
        );
    }

    let mut work = 0usize;
    for &node in &local.nodes {
        let Some(CpgNodeKind::Literal { kind }) = cpg.node(node).map(|node| &node.kind) else {
            continue;
        };
        for source in catalog.classify_literal(kind) {
            let token = TokenKey {
                fact: TaintFact::Ambient(source.class),
                origin: node,
            };
            add_token(
                &mut tokens,
                node,
                token,
                TaintWitness::at(node, confidence_milli(source.confidence)),
                ctx.caps,
                &mut capped,
            );
        }
    }
    loop {
        let mut changed = false;
        for &node in &local.nodes {
            if !consume_local_work(&mut work, ctx.caps, &mut capped) {
                mark_env_capped(&mut tokens);
                return (tokens, true);
            }
            match cpg.node(node).map(|node| &node.kind) {
                Some(CpgNodeKind::Call { .. }) => {
                    for (token, witness) in
                        call_result_tokens(cpg, node, &tokens, resolution, catalog, ctx)
                    {
                        changed |= add_token(
                            &mut tokens,
                            node,
                            token,
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
                        changed |= copy_tokens(
                            &mut tokens,
                            definition,
                            node,
                            1_000,
                            ctx.caps,
                            &mut capped,
                        );
                    }
                }
                Some(CpgNodeKind::Parameter { .. } | CpgNodeKind::Function { .. }) => {}
                Some(CpgNodeKind::Assignment { .. }) => {
                    for &child in cpg.ast_children(node).iter().skip(1) {
                        changed |=
                            copy_value_tokens(cpg, &mut tokens, child, node, ctx.caps, &mut capped);
                    }
                }
                Some(_) => {
                    for &child in cpg.ast_children(node) {
                        changed |=
                            copy_tokens(&mut tokens, child, node, 1_000, ctx.caps, &mut capped);
                    }
                }
                None => {}
            }
        }

        for &(source, target) in &local.dfg_edges {
            if !consume_local_work(&mut work, ctx.caps, &mut capped) {
                mark_env_capped(&mut tokens);
                return (tokens, true);
            }
            if cpg
                .node(target)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Call { .. }))
            {
                continue;
            }
            changed |= copy_tokens(&mut tokens, source, target, 1_000, ctx.caps, &mut capped);
        }

        if !changed {
            break;
        }
    }
    if capped {
        mark_env_capped(&mut tokens);
    }
    (tokens, capped)
}

fn call_result_tokens(
    cpg: &CodePropertyGraph,
    call: NodeId,
    tokens: &TokenEnv,
    resolution: &CallGraphResolution,
    catalog: &TaintCatalog,
    ctx: &SummaryCtx<'_, TaintDomain<'_>>,
) -> TokenSet {
    let path = callee_path(cpg, resolution, call, ctx.caps.max_call_depth).unwrap_or_default();
    let classification = catalog.classify(&path);
    if !classification.sanitizers.is_empty() {
        return TokenSet::new();
    }

    let mut output = TokenSet::new();
    if !classification.sources.is_empty() {
        for source in classification.sources {
            let token = TokenKey {
                fact: TaintFact::Ambient(source.class),
                origin: call,
            };
            insert_witness(
                &mut output,
                token,
                TaintWitness::at(call, confidence_milli(source.confidence)),
            );
        }
        return output;
    }

    let arguments = call_arguments(cpg, call);
    if !classification.sinks.is_empty() {
        return argument_tokens(cpg, tokens, &arguments, 1_000);
    }

    match (ctx.resolve)(call) {
        CalleeRef::Resolved(callee) => {
            let Some(summary) = ctx.summaries.get(&callee) else {
                return argument_tokens(cpg, tokens, &arguments, call_confidence(resolution, call));
            };
            let resolution_confidence = call_confidence(resolution, call);
            for (&index, suffix) in &summary.param_return_sites {
                let Some(&argument) = arguments.get(usize::from(index)) else {
                    continue;
                };
                for (token, witness) in tokens_for_value(cpg, tokens, argument) {
                    insert_witness(
                        &mut output,
                        token,
                        witness.compose(suffix, resolution_confidence).append(call),
                    );
                }
            }
            if summary.param_mask_truncated {
                for &argument in arguments.iter().skip(64) {
                    for (token, mut witness) in tokens_for_value(cpg, tokens, argument) {
                        witness.mark_capped();
                        insert_witness(
                            &mut output,
                            token,
                            witness.with_confidence(resolution_confidence).append(call),
                        );
                    }
                }
            }
            for (key, suffix) in &summary.return_source_sites {
                let token = TokenKey {
                    fact: TaintFact::Ambient(key.class.clone()),
                    origin: key.source,
                };
                insert_witness(
                    &mut output,
                    token,
                    suffix.with_confidence(resolution_confidence).append(call),
                );
            }
        }
        CalleeRef::External(_) | CalleeRef::Unresolved(_) => {
            let passthrough_inputs = opaque_call_inputs(cpg, call, &arguments);
            let argument_tokens = argument_tokens(cpg, tokens, &passthrough_inputs, 1_000);
            let external = catalog.external_match(&path);
            let model = external
                .as_ref()
                .map_or_else(ExternalModel::default, |(model, _)| model.clone());
            let model_confidence =
                external.map_or(500, |(_, confidence)| confidence_milli(confidence));
            match model {
                ExternalModel::Passthrough => {
                    for (token, witness) in argument_tokens {
                        insert_witness(
                            &mut output,
                            token,
                            witness.with_confidence(model_confidence).append(call),
                        );
                    }
                }
                ExternalModel::Source { class } => {
                    let token = TokenKey {
                        fact: TaintFact::Ambient(class),
                        origin: call,
                    };
                    insert_witness(&mut output, token, TaintWitness::at(call, model_confidence));
                }
                ExternalModel::Havoc | ExternalModel::Sink { .. } | ExternalModel::Sanitizer => {}
            }
        }
    }
    output
}

/// Returns values that may influence an opaque call's result.
///
/// Positional arguments are always included. An unresolved/external method
/// call also receives its expression receiver: wrappers such as
/// `source().unwrap()` commonly return a value derived from that receiver even
/// though it is not represented inside the grammar's argument list. The walk
/// is shallow, ordered, and restricted to the typed callee subtree.
fn opaque_call_inputs(cpg: &CodePropertyGraph, call: NodeId, arguments: &[NodeId]) -> Vec<NodeId> {
    let mut inputs = Vec::with_capacity(arguments.len().saturating_add(1));
    let is_method = cpg.node(call).is_some_and(|node| {
        matches!(
            node.kind,
            CpgNodeKind::Call {
                is_method: true,
                ..
            }
        )
    });
    if is_method {
        let mut call_children = cpg.ast_children(call).to_vec();
        call_children.sort_unstable_by_key(|&node| source_key(cpg, node));
        if let Some(callee) = call_children.first().copied() {
            let mut callee_children = cpg.ast_children(callee).to_vec();
            callee_children.sort_unstable_by_key(|&node| source_key(cpg, node));
            if let Some(receiver) = callee_children.first().copied() {
                inputs.push(receiver);
            }
        }
    }
    inputs.extend(arguments.iter().copied());
    inputs.dedup();
    inputs
}

fn summarize_call_sinks(
    cpg: &CodePropertyGraph,
    call: NodeId,
    tokens: &TokenEnv,
    resolution: &CallGraphResolution,
    catalog: &TaintCatalog,
    ctx: &SummaryCtx<'_, TaintDomain<'_>>,
    summary: &mut TaintSummary,
) {
    let path = callee_path(cpg, resolution, call, ctx.caps.max_call_depth).unwrap_or_default();
    let classification = catalog.classify(&path);
    if !classification.sanitizers.is_empty() || !classification.sources.is_empty() {
        return;
    }
    let arguments = call_arguments(cpg, call);
    if !classification.sinks.is_empty() {
        for (index, &argument) in arguments.iter().enumerate() {
            let param_index = u16::try_from(index).ok();
            for sink in &classification.sinks {
                for (token, witness) in tokens_for_value(cpg, tokens, argument) {
                    record_sink_token(
                        summary,
                        token,
                        sink.class.clone(),
                        sink.cwe,
                        call,
                        param_index,
                        witness
                            .with_confidence(confidence_milli(sink.confidence))
                            .append(call),
                    );
                }
            }
        }
        return;
    }

    match (ctx.resolve)(call) {
        CalleeRef::Resolved(callee) => {
            let Some(callee_summary) = ctx.summaries.get(&callee) else {
                summary.mark_capped();
                return;
            };
            let resolution_confidence = call_confidence(resolution, call);
            for (key, suffix) in &callee_summary.param_sink_sites {
                let Some(&argument) = arguments.get(usize::from(key.param)) else {
                    continue;
                };
                for (token, witness) in tokens_for_value(cpg, tokens, argument) {
                    record_sink_token(
                        summary,
                        token,
                        key.sink.clone(),
                        key.cwe,
                        suffix.nodes.last().copied().unwrap_or(call),
                        Some(key.param),
                        witness.compose(suffix, resolution_confidence),
                    );
                }
            }
            join_witness_map(&mut summary.ambient_flows, &callee_summary.ambient_flows);
            if callee_summary.capped {
                summary.mark_capped();
            }
        }
        CalleeRef::External(_) | CalleeRef::Unresolved(_) => {
            let Some((ExternalModel::Sink { class, cwe }, confidence)) =
                catalog.external_match(&path)
            else {
                return;
            };
            for (index, &argument) in arguments.iter().enumerate() {
                let param_index = u16::try_from(index).ok();
                for (token, witness) in tokens_for_value(cpg, tokens, argument) {
                    record_sink_token(
                        summary,
                        token,
                        class.clone(),
                        cwe,
                        call,
                        param_index,
                        witness
                            .with_confidence(confidence_milli(confidence))
                            .append(call),
                    );
                }
            }
        }
    }
}

fn record_sink_token(
    summary: &mut TaintSummary,
    token: TokenKey,
    sink: SinkClass,
    cwe: Option<u32>,
    sink_node: NodeId,
    param_index: Option<u16>,
    witness: TaintWitness,
) {
    match token.fact {
        TaintFact::Param(param) => summary.add_param_sink(param, sink, cwe, witness),
        TaintFact::Ambient(source_class) => {
            insert_witness(
                &mut summary.ambient_flows,
                AmbientFlowKey {
                    source_class,
                    sink_class: sink,
                    cwe,
                    source: token.origin,
                    sink: sink_node,
                    param_index,
                },
                witness,
            );
        }
    }
}

fn argument_tokens(
    cpg: &CodePropertyGraph,
    tokens: &TokenEnv,
    arguments: &[NodeId],
    confidence_milli: u16,
) -> TokenSet {
    let mut result = TokenSet::new();
    for &argument in arguments {
        for (token, witness) in tokens_for_value(cpg, tokens, argument) {
            insert_witness(
                &mut result,
                token,
                witness.with_confidence(confidence_milli),
            );
        }
    }
    result
}

fn tokens_for_value(cpg: &CodePropertyGraph, tokens: &TokenEnv, value: NodeId) -> TokenSet {
    let mut result = tokens.get(&value).cloned().unwrap_or_default();
    if cpg
        .node(value)
        .is_some_and(|node| matches!(node.kind, CpgNodeKind::Call { .. }))
    {
        return result;
    }

    let mut children = cpg.ast_children(value).to_vec();
    children.sort_unstable_by_key(|&node| source_key(cpg, node));
    children.reverse();
    let mut stack = children;
    let mut visited = BTreeSet::new();
    while let Some(descendant) = stack.pop() {
        if !visited.insert(descendant) {
            continue;
        }
        let Some(node) = cpg.node(descendant) else {
            continue;
        };
        if matches!(node.kind, CpgNodeKind::Function { .. }) {
            continue;
        }
        if let Some(found) = tokens.get(&descendant) {
            join_witness_map(&mut result, found);
        }
        // A nested call's result has already been computed by
        // `call_result_tokens`; descending into its arguments would bypass a
        // sanitizer or a resolved procedure summary.
        if matches!(node.kind, CpgNodeKind::Call { .. }) {
            continue;
        }
        let mut nested = cpg.ast_children(descendant).to_vec();
        nested.sort_unstable_by_key(|&node| source_key(cpg, node));
        nested.reverse();
        stack.extend(nested);
    }
    result
}

fn copy_value_tokens(
    cpg: &CodePropertyGraph,
    tokens: &mut TokenEnv,
    source: NodeId,
    target: NodeId,
    caps: &IfdsCaps,
    capped: &mut bool,
) -> bool {
    let found = tokens_for_value(cpg, tokens, source);
    add_tokens(tokens, target, found, 1_000, caps, capped)
}

fn copy_tokens(
    tokens: &mut TokenEnv,
    source: NodeId,
    target: NodeId,
    confidence_milli: u16,
    caps: &IfdsCaps,
    capped: &mut bool,
) -> bool {
    let found = tokens.get(&source).cloned().unwrap_or_default();
    add_tokens(tokens, target, found, confidence_milli, caps, capped)
}

fn add_tokens(
    tokens: &mut TokenEnv,
    target: NodeId,
    found: TokenSet,
    confidence_milli: u16,
    caps: &IfdsCaps,
    capped: &mut bool,
) -> bool {
    let mut changed = false;
    for (token, witness) in found {
        changed |= add_token(
            tokens,
            target,
            token,
            witness.with_confidence(confidence_milli).append(target),
            caps,
            capped,
        );
    }
    changed
}

fn add_token(
    tokens: &mut TokenEnv,
    node: NodeId,
    token: TokenKey,
    witness: TaintWitness,
    caps: &IfdsCaps,
    capped: &mut bool,
) -> bool {
    let node_tokens = tokens.entry(node).or_default();
    if let Some(current) = node_tokens.get_mut(&token) {
        if witness_preferred(&witness, current) {
            *current = witness;
            return true;
        }
        return false;
    }
    if node_tokens.len() >= caps.max_facts_per_node {
        *capped = true;
        return false;
    }
    node_tokens.insert(token, witness);
    true
}

fn consume_local_work(work: &mut usize, caps: &IfdsCaps, capped: &mut bool) -> bool {
    if *work >= caps.max_worklist_steps {
        *capped = true;
        false
    } else {
        *work = work.saturating_add(1);
        true
    }
}

fn mark_env_capped(tokens: &mut TokenEnv) {
    for witness in tokens.values_mut().flat_map(BTreeMap::values_mut) {
        witness.mark_capped();
    }
}

fn return_operands(cpg: &CodePropertyGraph, returned: NodeId) -> Vec<NodeId> {
    let mut children = cpg.ast_children(returned).to_vec();
    children.sort_unstable_by_key(|&node| source_key(cpg, node));
    children
}

fn returned_values(cpg: &CodePropertyGraph, local: &LocalFunction) -> Vec<NodeId> {
    let mut values = local
        .returns
        .iter()
        .flat_map(|&returned| return_operands(cpg, returned))
        .collect::<Vec<_>>();
    if let Some(body) = cpg
        .ast_children(local.function)
        .iter()
        .copied()
        .find(|&node| {
            cpg.node(node)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Block { .. }))
        })
    {
        if let Some(tail) = cpg.ast_children(body).last().copied() {
            if value_like(cpg, tail) {
                values.push(tail);
            }
        }
    }
    values.sort_unstable_by_key(|&node| source_key(cpg, node));
    values.dedup();
    values
}

fn value_like(cpg: &CodePropertyGraph, node: NodeId) -> bool {
    matches!(
        cpg.node(node).map(|node| &node.kind),
        Some(
            CpgNodeKind::BinaryOp { .. }
                | CpgNodeKind::UnaryOp { .. }
                | CpgNodeKind::Call { .. }
                | CpgNodeKind::MemberAccess { .. }
                | CpgNodeKind::IndexAccess
                | CpgNodeKind::Identifier { .. }
                | CpgNodeKind::Literal { .. }
                | CpgNodeKind::Lambda { .. }
                | CpgNodeKind::Await
                | CpgNodeKind::Yield
        )
    )
}

fn source_key(cpg: &CodePropertyGraph, node: NodeId) -> (u32, u32, NodeId) {
    cpg.node(node)
        .map(|node| (node.range.start, node.range.end, node.id))
        .unwrap_or((u32::MAX, u32::MAX, node))
}

fn callee_path(
    cpg: &CodePropertyGraph,
    resolution: &CallGraphResolution,
    call: NodeId,
    max_depth: u32,
) -> Option<String> {
    callee_path_evidence(cpg, resolution, call, max_depth).path
}

fn callee_path_evidence(
    cpg: &CodePropertyGraph,
    resolution: &CallGraphResolution,
    call: NodeId,
    max_depth: u32,
) -> super::resolve::CalleePathEvidence {
    let reconstructed = call_callee_path_evidence(cpg, call, max_depth);
    if reconstructed.path.is_some() {
        return reconstructed;
    }
    if let Some(path) = resolution
        .resolutions
        .get(&call)
        .and_then(|record| record.callee_name.clone())
    {
        return super::resolve::CalleePathEvidence {
            path: Some(path),
            capped: false,
        };
    }
    reconstructed
}

fn resolve_call(
    cpg: &CodePropertyGraph,
    resolution: &CallGraphResolution,
    call: NodeId,
    max_depth: u32,
) -> CalleeRef {
    if let Some(record) = resolution.resolutions.get(&call) {
        if let Some(target) = record.target {
            return CalleeRef::Resolved(target);
        }
        let path = callee_path(cpg, resolution, call, max_depth)
            .unwrap_or_default()
            .into();
        return if record.confidence == Confidence::External {
            CalleeRef::External(path)
        } else {
            CalleeRef::Unresolved(path)
        };
    }
    if let Some(CpgNodeKind::Call {
        target: Some(target),
        ..
    }) = cpg.node(call).map(|node| &node.kind)
    {
        return CalleeRef::Resolved(*target);
    }
    if let Some(target) = cpg.callees(call).first().copied() {
        return CalleeRef::Resolved(target);
    }
    CalleeRef::Unresolved(
        call_callee_path(cpg, call, max_depth)
            .unwrap_or_default()
            .into(),
    )
}

fn call_confidence(resolution: &CallGraphResolution, call: NodeId) -> u16 {
    resolution
        .resolutions
        .get(&call)
        .map_or(900, |record| confidence_milli(record.confidence.score()))
}

fn confidence_milli(confidence: f32) -> u16 {
    (confidence.clamp(0.0, 1.0) * 1_000.0).round() as u16
}

fn witness_preferred(candidate: &TaintWitness, current: &TaintWitness) -> bool {
    compare_witness(candidate, current) == Ordering::Less
}

fn compare_witness(left: &TaintWitness, right: &TaintWitness) -> Ordering {
    left.capped
        .cmp(&right.capped)
        .then_with(|| right.confidence_milli.cmp(&left.confidence_milli))
        .then_with(|| left.nodes.len().cmp(&right.nodes.len()))
        .then_with(|| left.nodes.cmp(&right.nodes))
}

fn insert_witness<K: Ord>(
    map: &mut BTreeMap<K, TaintWitness>,
    key: K,
    witness: TaintWitness,
) -> bool {
    match map.get_mut(&key) {
        Some(current) if witness_preferred(&witness, current) => {
            *current = witness;
            true
        }
        Some(_) => false,
        None => {
            map.insert(key, witness);
            true
        }
    }
}

fn join_witness_map<K: Ord + Clone>(
    target: &mut BTreeMap<K, TaintWitness>,
    source: &BTreeMap<K, TaintWitness>,
) -> bool {
    let mut changed = false;
    for (key, witness) in source {
        changed |= insert_witness(target, key.clone(), witness.clone());
    }
    changed
}

fn witness_map_subsumes<K: Ord>(
    left: &BTreeMap<K, TaintWitness>,
    right: &BTreeMap<K, TaintWitness>,
) -> bool {
    right.iter().all(|(key, right_witness)| {
        left.get(key).is_some_and(|left_witness| {
            compare_witness(left_witness, right_witness) != Ordering::Greater
        })
    })
}

fn sets_by_index_subsume(left: &[BTreeSet<SinkClass>], right: &[BTreeSet<SinkClass>]) -> bool {
    right.iter().enumerate().all(|(index, sinks)| {
        left.get(index)
            .is_some_and(|left_sinks| left_sinks.is_superset(sinks))
            || sinks.is_empty()
    })
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
