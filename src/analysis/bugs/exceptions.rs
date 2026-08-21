//! Bounded interprocedural exception-flow analysis.
//!
//! The analysis summarizes exception types that escape each represented
//! function, using the shared call-graph SCC solver. A second deterministic
//! pass dispatches final summaries through lexically enclosing handlers to
//! report uncaught entry propagation and handlers no represented throw reaches.

use std::collections::{BTreeMap, BTreeSet};
use std::hash::Hash;
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::{CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNodeKind, DfgEdgeKind, NodeId};

use super::super::{
    call_graph_sccs, function_scope_nodes, solve_summaries, CalleeRef, IfdsCapEvent, IfdsCaps,
    IfdsIssue, IfdsLattice, SummaryCtx, SummaryDomain,
};

/// One normalized exception type propagated by the analysis.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ExceptionType {
    /// A nominal type represented by CPG type evidence.
    Named(Arc<str>),
    /// A throw exists, but its nominal type is absent from the CPG.
    Unknown,
}

/// Monotone set of exception types that can escape one function.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ExceptionSummary {
    /// Possible escaping exception types in canonical order.
    pub escaping: BTreeSet<ExceptionType>,
}

impl IfdsLattice for ExceptionSummary {
    fn join(&mut self, other: &Self) -> bool {
        let before = self.escaping.len();
        self.escaping.extend(other.escaping.iter().cloned());
        self.escaping.len() != before
    }

    fn subsumes(&self, other: &Self) -> bool {
        self.escaping.is_superset(&other.escaping)
    }
}

/// Structural or type evidence that prevents a complete exception-flow claim.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ExceptionFlowIssue {
    /// The shared bounded summary solver rejected or truncated its input.
    Ifds(IfdsIssue),
    /// A throw expression had no represented nominal type.
    UnknownThrowType {
        /// Throw statement lacking type evidence.
        site: NodeId,
    },
    /// A call had more than one represented function target.
    AmbiguousCall {
        /// Ambiguous call site.
        site: NodeId,
    },
    /// A handler body lacked the builder's typed `Catch` entry edge.
    MissingCatchEdge {
        /// Catch clause with imprecise or absent CFG entry.
        catch: NodeId,
    },
    /// An escaping summary could not be connected to a represented throw.
    MissingWitness {
        /// Entry function carrying the summary.
        entry: NodeId,
        /// Exception type without a reconstructible origin.
        exception: ExceptionType,
    },
}

/// One exception type that can escape a root call-graph component.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct UncaughtException {
    /// Function in a root call-graph component.
    pub entry: NodeId,
    /// Local throw or call site at which the type escapes the entry function.
    pub origin: NodeId,
    /// Escaping exception type.
    pub exception: ExceptionType,
    /// Canonical entry/call/throw evidence ending at a direct throw.
    pub witness: Vec<NodeId>,
    /// Whether an explicit solver cap made the overall report partial.
    pub capped: bool,
}

/// One catch clause no represented direct or propagated throw reaches.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DeadCatch {
    /// Function containing the clause.
    pub function: NodeId,
    /// Catch-clause node.
    pub catch: NodeId,
    /// Nominal types accepted by the clause.
    pub accepted: BTreeSet<ExceptionType>,
    /// Whether the clause accepts every type, including unknown types.
    pub catch_all: bool,
    /// Whether an explicit solver cap made the overall report partial.
    pub capped: bool,
}

/// Exception summaries, findings, and completeness evidence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ExceptionFlowAnalysis {
    /// Escaping exception summary for every represented function.
    pub summaries: BTreeMap<NodeId, ExceptionSummary>,
    /// Uncaught propagation from root call-graph components.
    pub uncaught: Vec<UncaughtException>,
    /// Catch clauses no represented exception reaches.
    pub dead_catches: Vec<DeadCatch>,
    /// Types dispatched to each reached catch clause.
    pub reached_catches: BTreeMap<NodeId, BTreeSet<ExceptionType>>,
    /// Functions evaluated by the shared solver in deterministic order.
    pub processed_functions: Vec<NodeId>,
    /// Kleene rounds executed for each call-graph SCC.
    pub component_iterations: BTreeMap<usize, u32>,
    /// Shared summary-solver work consumed.
    pub worklist_steps: usize,
    /// Whether an explicit shared-solver cap stopped complete evaluation.
    pub capped: bool,
    /// Ordered resource-cap evidence from the shared solver.
    pub cap_events: BTreeSet<IfdsCapEvent>,
    /// Ordered structural, solver, and type-evidence issues.
    pub issues: BTreeSet<ExceptionFlowIssue>,
}

impl ExceptionFlowAnalysis {
    /// Returns whether every represented exception flow was evaluated completely.
    pub fn is_complete(&self) -> bool {
        !self.capped && self.issues.is_empty()
    }
}

/// Computes bounded interprocedural exception flow without mutating the CPG.
pub fn analyze_exception_flow(cpg: &CodePropertyGraph, caps: &IfdsCaps) -> ExceptionFlowAnalysis {
    let call_graph = call_graph_sccs(cpg);
    let hierarchy = ExceptionHierarchy::new(cpg);
    let domain = ExceptionDomain {
        hierarchy: hierarchy.clone(),
    };
    let output = solve_summaries(cpg, &domain, &call_graph, caps);
    let mut analysis = ExceptionFlowAnalysis {
        summaries: output.summaries,
        processed_functions: output.processed_functions,
        component_iterations: output.component_iterations,
        worklist_steps: output.worklist_steps,
        capped: output.capped,
        cap_events: output.cap_events,
        issues: output
            .issues
            .into_iter()
            .map(ExceptionFlowIssue::Ifds)
            .collect(),
        ..ExceptionFlowAnalysis::default()
    };

    let processed: BTreeSet<_> = analysis.processed_functions.iter().copied().collect();
    collect_input_issues(cpg, &processed, &mut analysis.issues);
    let entries = root_component_functions(&call_graph);
    let resolver = |call| represented_callee(cpg, call);
    let mut uncaught = BTreeSet::new();
    for &function in &processed {
        let origins = exception_origins(cpg, function, &analysis.summaries, &resolver);
        for origin in origins {
            match dispatch_exception(cpg, function, origin.site, &origin.exception, &hierarchy) {
                Some(catch) => {
                    analysis
                        .reached_catches
                        .entry(catch)
                        .or_default()
                        .insert(origin.exception);
                }
                None if entries.contains(&function) => {
                    let witness = exception_witness(
                        cpg,
                        function,
                        &origin.exception,
                        &analysis.summaries,
                        &hierarchy,
                    );
                    if witness.len() == 1 {
                        analysis.issues.insert(ExceptionFlowIssue::MissingWitness {
                            entry: function,
                            exception: origin.exception.clone(),
                        });
                    }
                    uncaught.insert(UncaughtException {
                        entry: function,
                        origin: origin.site,
                        exception: origin.exception,
                        witness,
                        capped: analysis.capped,
                    });
                }
                None => {}
            }
        }
    }
    analysis.uncaught = uncaught.into_iter().collect();

    let mut dead = BTreeSet::new();
    for &function in &processed {
        for catch in catch_clauses(cpg, function) {
            if analysis.reached_catches.contains_key(&catch) {
                continue;
            }
            let matcher = catch_matcher(cpg, catch);
            dead.insert(DeadCatch {
                function,
                catch,
                accepted: matcher.accepted(),
                catch_all: matches!(matcher, CatchMatcher::Any),
                capped: analysis.capped,
            });
        }
    }
    analysis.dead_catches = dead.into_iter().collect();
    analysis
}

/// Returns uncaught-entry findings while discarding completeness evidence.
///
/// Consumers making negative claims must use [`analyze_exception_flow`] and
/// require [`ExceptionFlowAnalysis::is_complete`].
pub fn uncaught_exceptions(cpg: &CodePropertyGraph, caps: &IfdsCaps) -> Vec<UncaughtException> {
    analyze_exception_flow(cpg, caps).uncaught
}

/// Returns dead-handler findings while discarding completeness evidence.
///
/// Consumers making negative claims must use [`analyze_exception_flow`] and
/// require [`ExceptionFlowAnalysis::is_complete`].
pub fn dead_catches(cpg: &CodePropertyGraph, caps: &IfdsCaps) -> Vec<DeadCatch> {
    analyze_exception_flow(cpg, caps).dead_catches
}

#[derive(Debug, Clone)]
struct ExceptionDomain {
    hierarchy: ExceptionHierarchy,
}

impl SummaryDomain for ExceptionDomain {
    type Fact = ExceptionType;
    type Summary = ExceptionSummary;

    fn seed_facts(&self, cpg: &CodePropertyGraph, function: NodeId) -> Vec<(NodeId, Self::Fact)> {
        direct_throws(cpg, function)
            .into_iter()
            .flat_map(|site| {
                throw_types(cpg, site)
                    .into_iter()
                    .map(move |exception| (site, exception))
            })
            .collect()
    }

    fn summarize(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
        ctx: &SummaryCtx<'_, Self>,
    ) -> Self::Summary {
        let mut escaping = BTreeSet::new();
        for origin in exception_origins(cpg, function, ctx.summaries, ctx.resolve) {
            if dispatch_exception(
                cpg,
                function,
                origin.site,
                &origin.exception,
                &self.hierarchy,
            )
            .is_none()
            {
                escaping.insert(origin.exception);
            }
        }
        ExceptionSummary { escaping }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ExceptionOrigin {
    site: NodeId,
    exception: ExceptionType,
}

fn exception_origins(
    cpg: &CodePropertyGraph,
    function: NodeId,
    summaries: &BTreeMap<NodeId, ExceptionSummary>,
    resolve: &dyn Fn(NodeId) -> CalleeRef,
) -> Vec<ExceptionOrigin> {
    let scope: BTreeSet<_> = function_scope_nodes(cpg, function).into_iter().collect();
    let mut origins = BTreeSet::new();
    for site in direct_throws(cpg, function) {
        for exception in throw_types(cpg, site) {
            origins.insert(ExceptionOrigin { site, exception });
        }
    }
    for site in scope {
        if !cpg
            .node(site)
            .is_some_and(|node| matches!(node.kind, CpgNodeKind::Call { .. }))
        {
            continue;
        }
        let CalleeRef::Resolved(callee) = resolve(site) else {
            continue;
        };
        let Some(summary) = summaries.get(&callee) else {
            continue;
        };
        for exception in &summary.escaping {
            origins.insert(ExceptionOrigin {
                site,
                exception: exception.clone(),
            });
        }
    }
    origins.into_iter().collect()
}

fn direct_throws(cpg: &CodePropertyGraph, function: NodeId) -> Vec<NodeId> {
    function_scope_nodes(cpg, function)
        .into_iter()
        .filter(|&node| {
            cpg.node(node)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Throw))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn throw_types(cpg: &CodePropertyGraph, throw_site: NodeId) -> BTreeSet<ExceptionType> {
    let mut types = BTreeSet::new();
    for &value in cpg.ast_children(throw_site) {
        types.extend(value_types(cpg, value));
    }
    if types.is_empty() {
        types.insert(ExceptionType::Unknown);
    }
    types
}

fn value_types(cpg: &CodePropertyGraph, value: NodeId) -> BTreeSet<ExceptionType> {
    let mut result = BTreeSet::new();
    let mut queue = BTreeSet::from([value]);
    let mut visited = BTreeSet::new();
    while let Some(node) = queue.pop_first() {
        if !visited.insert(node) {
            continue;
        }
        if let Some(cpg_node) = cpg.node(node) {
            result.extend(type_names(&cpg_node.kind));
            match &cpg_node.kind {
                CpgNodeKind::Identifier {
                    definition: Some(definition),
                    ..
                } => {
                    queue.insert(*definition);
                }
                CpgNodeKind::Call {
                    target: Some(target),
                    ..
                } => {
                    queue.insert(*target);
                }
                CpgNodeKind::Unknown { .. } => {
                    queue.extend(cpg.ast_children(node));
                }
                _ => {}
            }
        }
        queue.extend(cpg.reaching_definitions(node));
        for edge in cpg.outgoing_edges(node) {
            if matches!(
                edge.kind,
                CpgEdgeKind::TypeOf
                    | CpgEdgeKind::Reference
                    | CpgEdgeKind::Definition
                    | CpgEdgeKind::Declaration
                    | CpgEdgeKind::DataFlow(DfgEdgeKind::UseDef)
            ) {
                queue.insert(edge.target);
            }
        }
        for edge in cpg.incoming_edges(node) {
            if matches!(
                edge.kind,
                CpgEdgeKind::Reference
                    | CpgEdgeKind::Definition
                    | CpgEdgeKind::Declaration
                    | CpgEdgeKind::DataFlow(
                        DfgEdgeKind::DefUse | DfgEdgeKind::ReachingDef | DfgEdgeKind::UseDef
                    )
            ) {
                queue.insert(edge.source);
            }
        }
    }
    result
}

fn type_names(kind: &CpgNodeKind) -> BTreeSet<ExceptionType> {
    let mut names = BTreeSet::new();
    let type_info = match kind {
        CpgNodeKind::Variable { var_type, .. } => var_type.as_deref(),
        CpgNodeKind::Parameter { param_type, .. } => param_type.as_deref(),
        CpgNodeKind::Field { field_type, .. } => field_type.as_deref(),
        CpgNodeKind::TypeAnnotation { type_info } => Some(type_info.as_ref()),
        CpgNodeKind::Function { signature } => signature.return_type.as_ref(),
        CpgNodeKind::Class { name, .. }
        | CpgNodeKind::Struct { name }
        | CpgNodeKind::Enum { name }
        | CpgNodeKind::Trait { name } => {
            if !name.trim().is_empty() {
                names.insert(ExceptionType::Named(name.clone()));
            }
            None
        }
        _ => None,
    };
    if let Some(type_info) = type_info.filter(|type_info| !type_info.name.trim().is_empty()) {
        names.insert(ExceptionType::Named(type_info.name.clone()));
    }
    names
}

#[derive(Debug, Clone)]
enum CatchMatcher {
    Any,
    Typed(BTreeSet<ExceptionType>),
}

impl CatchMatcher {
    fn accepts(&self, exception: &ExceptionType, hierarchy: &ExceptionHierarchy) -> bool {
        match self {
            Self::Any => true,
            Self::Typed(accepted) => accepted
                .iter()
                .any(|candidate| hierarchy.is_assignable(exception, candidate)),
        }
    }

    fn accepted(&self) -> BTreeSet<ExceptionType> {
        match self {
            Self::Any => BTreeSet::new(),
            Self::Typed(accepted) => accepted.clone(),
        }
    }
}

fn catch_matcher(cpg: &CodePropertyGraph, catch: NodeId) -> CatchMatcher {
    let children = cpg.ast_children(catch);
    let mut accepted = BTreeSet::new();
    for child in children.iter().copied().filter(|&child| {
        !cpg.node(child)
            .is_some_and(|node| matches!(node.kind, CpgNodeKind::Block { .. }))
    }) {
        accepted.extend(value_types(cpg, child));
        if let Some(CpgNodeKind::Parameter { name, .. }) = cpg.node(child).map(|node| &node.kind) {
            for descendant in cpg.ast_descendants(child) {
                if let Some(CpgNodeKind::Identifier {
                    name: candidate, ..
                }) = cpg.node(descendant).map(|node| &node.kind)
                {
                    if candidate != name && !candidate.is_empty() {
                        accepted.insert(ExceptionType::Named(candidate.clone()));
                    }
                }
            }
        }
    }
    accepted.remove(&ExceptionType::Unknown);
    if accepted.is_empty() {
        CatchMatcher::Any
    } else {
        CatchMatcher::Typed(accepted)
    }
}

fn dispatch_exception(
    cpg: &CodePropertyGraph,
    function: NodeId,
    site: NodeId,
    exception: &ExceptionType,
    hierarchy: &ExceptionHierarchy,
) -> Option<NodeId> {
    for handlers in handler_layers(cpg, function, site) {
        for catch in handlers {
            if catch_matcher(cpg, catch).accepts(exception, hierarchy) {
                return Some(catch);
            }
        }
    }
    None
}

fn handler_layers(cpg: &CodePropertyGraph, function: NodeId, site: NodeId) -> Vec<Vec<NodeId>> {
    let mut layers = Vec::new();
    let mut child = site;
    for ancestor in cpg.ast_ancestors(site) {
        if ancestor == function {
            break;
        }
        if cpg
            .node(ancestor)
            .is_some_and(|node| matches!(node.kind, CpgNodeKind::Try))
        {
            let children = cpg.ast_children(ancestor);
            let protected = children.iter().copied().find(|&node| {
                cpg.node(node)
                    .is_some_and(|node| matches!(node.kind, CpgNodeKind::Block { .. }))
            });
            if protected == Some(child) {
                let catches = children
                    .iter()
                    .copied()
                    .filter(|&node| {
                        cpg.node(node)
                            .is_some_and(|node| matches!(node.kind, CpgNodeKind::Catch))
                    })
                    .collect::<Vec<_>>();
                if !catches.is_empty() {
                    layers.push(catches);
                }
            }
        }
        child = ancestor;
    }
    layers
}

fn catch_clauses(cpg: &CodePropertyGraph, function: NodeId) -> Vec<NodeId> {
    function_scope_nodes(cpg, function)
        .into_iter()
        .filter(|&node| {
            cpg.node(node)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Catch))
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

#[derive(Debug, Clone, Default)]
struct ExceptionHierarchy {
    supertypes: BTreeMap<Arc<str>, BTreeSet<Arc<str>>>,
}

impl ExceptionHierarchy {
    fn new(cpg: &CodePropertyGraph) -> Self {
        let mut hierarchy = Self::default();
        for edge in cpg.edges() {
            if !matches!(edge.kind, CpgEdgeKind::Inherits | CpgEdgeKind::Implements) {
                continue;
            }
            let sources = cpg
                .node(edge.source)
                .map(|node| type_names(&node.kind))
                .unwrap_or_default();
            let targets = cpg
                .node(edge.target)
                .map(|node| type_names(&node.kind))
                .unwrap_or_default();
            for source in &sources {
                let ExceptionType::Named(source) = source else {
                    continue;
                };
                for target in &targets {
                    if let ExceptionType::Named(target) = target {
                        hierarchy
                            .supertypes
                            .entry(source.clone())
                            .or_default()
                            .insert(target.clone());
                    }
                }
            }
        }
        hierarchy
    }

    fn is_assignable(&self, thrown: &ExceptionType, accepted: &ExceptionType) -> bool {
        let (ExceptionType::Named(thrown), ExceptionType::Named(accepted)) = (thrown, accepted)
        else {
            return false;
        };
        if thrown == accepted {
            return true;
        }
        let mut queue = BTreeSet::from([thrown.clone()]);
        let mut visited = BTreeSet::new();
        while let Some(current) = queue.pop_first() {
            if !visited.insert(current.clone()) {
                continue;
            }
            for parent in self.supertypes.get(&current).into_iter().flatten() {
                if parent == accepted {
                    return true;
                }
                queue.insert(parent.clone());
            }
        }
        false
    }
}

fn represented_callee(cpg: &CodePropertyGraph, call: NodeId) -> CalleeRef {
    let targets = represented_targets(cpg, call);
    if targets.len() == 1 {
        CalleeRef::Resolved(*targets.first().expect("one represented target"))
    } else {
        CalleeRef::Unresolved(Arc::from("unresolved represented call"))
    }
}

fn represented_targets(cpg: &CodePropertyGraph, call: NodeId) -> BTreeSet<NodeId> {
    let mut targets = BTreeSet::new();
    if let Some(CpgNodeKind::Call {
        target: Some(target),
        ..
    }) = cpg.node(call).map(|node| &node.kind)
    {
        if cpg
            .node(*target)
            .is_some_and(|node| matches!(node.kind, CpgNodeKind::Function { .. }))
        {
            targets.insert(*target);
        }
    }
    for edge in cpg.outgoing_edges(call) {
        if (edge.kind.is_call() || matches!(edge.kind, CpgEdgeKind::ControlFlow(CfgEdgeKind::Call)))
            && cpg
                .node(edge.target)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Function { .. }))
        {
            targets.insert(edge.target);
        }
    }
    targets
}

fn root_component_functions(call_graph: &super::super::SccDecomposition) -> BTreeSet<NodeId> {
    let mut indegree = vec![0usize; call_graph.components.len()];
    for edge in &call_graph.condensation_edges {
        if let Some(degree) = indegree.get_mut(edge.target) {
            *degree = degree.saturating_add(1);
        }
    }
    call_graph
        .components
        .iter()
        .filter(|component| indegree.get(component.id).copied() == Some(0))
        .flat_map(|component| component.nodes.iter().copied())
        .collect()
}

fn collect_input_issues(
    cpg: &CodePropertyGraph,
    processed: &BTreeSet<NodeId>,
    issues: &mut BTreeSet<ExceptionFlowIssue>,
) {
    for &function in processed {
        let scope = function_scope_nodes(cpg, function);
        for node in scope {
            match cpg.node(node).map(|node| &node.kind) {
                Some(CpgNodeKind::Throw) => {
                    if throw_types(cpg, node).contains(&ExceptionType::Unknown) {
                        issues.insert(ExceptionFlowIssue::UnknownThrowType { site: node });
                    }
                }
                Some(CpgNodeKind::Call { .. }) => {
                    if represented_targets(cpg, node).len() > 1 {
                        issues.insert(ExceptionFlowIssue::AmbiguousCall { site: node });
                    }
                }
                Some(CpgNodeKind::Catch) => {
                    let body = cpg.ast_children(node).iter().copied().find(|&child| {
                        cpg.node(child)
                            .is_some_and(|node| matches!(node.kind, CpgNodeKind::Block { .. }))
                    });
                    if let Some(body) = body {
                        let typed_entry = cpg.outgoing_edges(node).any(|edge| {
                            edge.target == body
                                && matches!(edge.kind, CpgEdgeKind::ControlFlow(CfgEdgeKind::Catch))
                        });
                        if !typed_entry {
                            issues.insert(ExceptionFlowIssue::MissingCatchEdge { catch: node });
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct WitnessState {
    distance: usize,
    path: Vec<NodeId>,
    function: NodeId,
}

fn exception_witness(
    cpg: &CodePropertyGraph,
    entry: NodeId,
    exception: &ExceptionType,
    summaries: &BTreeMap<NodeId, ExceptionSummary>,
    hierarchy: &ExceptionHierarchy,
) -> Vec<NodeId> {
    let mut queue = BTreeSet::from([WitnessState {
        distance: 0,
        path: vec![entry],
        function: entry,
    }]);
    let mut visited = BTreeSet::new();
    while let Some(state) = queue.pop_first() {
        if !visited.insert(state.function) {
            continue;
        }
        for site in direct_throws(cpg, state.function) {
            if throw_types(cpg, site).contains(exception)
                && dispatch_exception(cpg, state.function, site, exception, hierarchy).is_none()
            {
                let mut witness = state.path;
                witness.push(site);
                return witness;
            }
        }
        let scope: BTreeSet<_> = function_scope_nodes(cpg, state.function)
            .into_iter()
            .collect();
        for call in scope {
            let CalleeRef::Resolved(callee) = represented_callee(cpg, call) else {
                continue;
            };
            if !summaries
                .get(&callee)
                .is_some_and(|summary| summary.escaping.contains(exception))
                || dispatch_exception(cpg, state.function, call, exception, hierarchy).is_some()
            {
                continue;
            }
            let mut path = state.path.clone();
            path.push(call);
            path.push(callee);
            queue.insert(WitnessState {
                distance: state.distance.saturating_add(1),
                path,
                function: callee,
            });
        }
    }
    vec![entry]
}

#[cfg(test)]
#[path = "exceptions/tests.rs"]
mod tests;
