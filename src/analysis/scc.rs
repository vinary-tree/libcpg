//! Strongly-connected-component analysis for CFG and call-graph projections.
//!
//! The implementation uses an iterative Kosaraju decomposition. Both graph
//! traversals and AST scope discovery use explicit work stacks, so deeply
//! nested source or long graph paths cannot overflow the native stack.

use std::collections::BTreeMap;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::{CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNodeKind, NodeId};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// The directed graph projection decomposed into SCCs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum SccProjection {
    /// One function's intraprocedural control-flow graph.
    ControlFlow {
        /// Function whose CFG was analyzed.
        function: NodeId,
    },
    /// The function-to-function call graph for the whole CPG.
    CallGraph,
}

/// One maximal strongly-connected component.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SccComponent {
    /// Stable index into [`SccDecomposition::components`].
    pub id: usize,
    /// Component members, sorted by [`NodeId::as_u32`].
    pub nodes: Vec<NodeId>,
    /// Whether the component contains a directed cycle.
    ///
    /// Every component with multiple nodes is cyclic. A singleton is cyclic
    /// only when the projected graph contains a self-loop.
    pub cyclic: bool,
}

impl SccComponent {
    /// Returns true for a one-node component with a self-loop.
    pub fn is_self_cycle(&self) -> bool {
        self.cyclic && self.nodes.len() == 1
    }

    /// Returns true for a cycle containing multiple nodes.
    pub fn is_multi_node_cycle(&self) -> bool {
        self.nodes.len() > 1
    }
}

/// One edge in the SCC condensation graph.
///
/// Condensation edges connect distinct component ids. The condensation graph
/// is always a directed acyclic graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SccCondensationEdge {
    /// Source component id.
    pub source: usize,
    /// Target component id.
    pub target: usize,
}

/// Complete SCC partition and its acyclic condensation graph.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SccDecomposition {
    /// Projection that was analyzed.
    pub projection: SccProjection,
    /// Maximal SCCs, deterministically ordered by their smallest node id.
    pub components: Vec<SccComponent>,
    /// Node-to-component lookup. Every analyzed node appears exactly once.
    pub component_by_node: BTreeMap<NodeId, usize>,
    /// Deduplicated condensation-DAG edges, sorted by `(source, target)`.
    pub condensation_edges: Vec<SccCondensationEdge>,
}

impl SccDecomposition {
    /// Returns the component containing `node`.
    pub fn component_of(&self, node: NodeId) -> Option<&SccComponent> {
        let id = *self.component_by_node.get(&node)?;
        self.components.get(id)
    }

    /// Iterates only the components that contain a directed cycle.
    pub fn cyclic_components(&self) -> impl Iterator<Item = &SccComponent> {
        self.components.iter().filter(|component| component.cyclic)
    }

    /// Returns true when the projected graph contains no directed cycle.
    pub fn is_acyclic(&self) -> bool {
        self.components.iter().all(|component| !component.cyclic)
    }

    /// Number of projected graph nodes in the partition.
    pub fn node_count(&self) -> usize {
        self.component_by_node.len()
    }
}

/// Errors reported by function-scoped SCC analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SccAnalysisError {
    /// The requested node does not exist in the CPG.
    #[error("SCC analysis node does not exist: {0:?}")]
    UnknownNode(NodeId),
    /// The requested node exists but is not a function.
    #[error("control-flow SCC analysis requires a function node: {0:?}")]
    NotAFunction(NodeId),
}

/// Decomposes one function's intraprocedural CFG into SCCs.
///
/// Nested functions are separate scopes and are not folded into the enclosing
/// function. Interprocedural `CfgEdgeKind::Call` edges whose target is outside
/// the function are likewise excluded. The function entry is retained as an
/// isolated singleton when it has no CFG edges.
pub fn control_flow_sccs(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<SccDecomposition, SccAnalysisError> {
    let Some(function_node) = cpg.node(function) else {
        return Err(SccAnalysisError::UnknownNode(function));
    };
    if !matches!(function_node.kind, CpgNodeKind::Function { .. }) {
        return Err(SccAnalysisError::NotAFunction(function));
    }

    let scope = function_scope_nodes(cpg, function);
    let mut node_set = FxHashSet::default();
    node_set.insert(function);

    for edge in cpg.edges().filter(|edge| edge.kind.is_cfg()) {
        if scope.contains(&edge.source) && scope.contains(&edge.target) {
            node_set.insert(edge.source);
            node_set.insert(edge.target);
        }
    }

    let nodes = sorted_nodes(node_set);
    let adjacency = adjacency_from_edges(
        &nodes,
        cpg.edges()
            .filter(|edge| edge.kind.is_cfg())
            .map(|edge| (edge.source, edge.target)),
    );
    Ok(decompose(
        SccProjection::ControlFlow { function },
        nodes,
        adjacency,
    ))
}

/// Decomposes the whole CPG's resolved function call graph into SCCs.
///
/// Call-site-to-callee edges (`CallSite`, `StaticCall`, and `DynamicCall`) are
/// collapsed to caller-function-to-callee-function edges. Resolved
/// `CpgNodeKind::Call::target` values and CFG `Call` edges are accepted too,
/// because the built-in CFG extractor represents resolved calls that way.
/// Unresolved calls and edges to non-function targets do not invent vertices.
pub fn call_graph_sccs(cpg: &CodePropertyGraph) -> SccDecomposition {
    let nodes = sorted_nodes(cpg.functions().map(|node| node.id));
    let node_set: FxHashSet<NodeId> = nodes.iter().copied().collect();
    let mut adjacency: FxHashMap<NodeId, Vec<NodeId>> = nodes
        .iter()
        .copied()
        .map(|node| (node, Vec::new()))
        .collect();

    for &caller in &nodes {
        for source in function_scope_nodes(cpg, caller) {
            if let Some(node) = cpg.node(source) {
                if let CpgNodeKind::Call {
                    target: Some(target),
                    ..
                } = &node.kind
                {
                    if node_set.contains(target) {
                        adjacency.entry(caller).or_default().push(*target);
                    }
                }
            }

            for edge in cpg.outgoing_edges(source) {
                let is_resolved_call = edge.kind.is_call()
                    || matches!(edge.kind, CpgEdgeKind::ControlFlow(CfgEdgeKind::Call));
                if is_resolved_call && node_set.contains(&edge.target) {
                    adjacency.entry(caller).or_default().push(edge.target);
                }
            }
        }
    }

    normalize_adjacency(&mut adjacency);
    decompose(SccProjection::CallGraph, nodes, adjacency)
}

/// Returns a function and its AST descendants, stopping before nested
/// functions. The traversal follows edges rather than parent pointers so it is
/// correct for both parser-built and hand-built CPGs.
fn function_scope_nodes(cpg: &CodePropertyGraph, function: NodeId) -> FxHashSet<NodeId> {
    let mut scope = FxHashSet::default();
    let mut stack = vec![function];

    while let Some(node) = stack.pop() {
        if !scope.insert(node) {
            continue;
        }
        let mut children = cpg.ast_children(node);
        children.reverse();
        for child in children {
            if child != function
                && cpg
                    .node(child)
                    .is_some_and(|node| matches!(node.kind, CpgNodeKind::Function { .. }))
            {
                continue;
            }
            stack.push(child);
        }
    }

    scope
}

fn sorted_nodes(nodes: impl IntoIterator<Item = NodeId>) -> Vec<NodeId> {
    let mut nodes: Vec<NodeId> = nodes.into_iter().collect();
    nodes.sort_unstable_by_key(|node| node.as_u32());
    nodes.dedup();
    nodes
}

fn adjacency_from_edges(
    nodes: &[NodeId],
    edges: impl IntoIterator<Item = (NodeId, NodeId)>,
) -> FxHashMap<NodeId, Vec<NodeId>> {
    let node_set: FxHashSet<NodeId> = nodes.iter().copied().collect();
    let mut adjacency: FxHashMap<NodeId, Vec<NodeId>> = nodes
        .iter()
        .copied()
        .map(|node| (node, Vec::new()))
        .collect();
    for (source, target) in edges {
        if node_set.contains(&source) && node_set.contains(&target) {
            adjacency.entry(source).or_default().push(target);
        }
    }
    normalize_adjacency(&mut adjacency);
    adjacency
}

fn normalize_adjacency(adjacency: &mut FxHashMap<NodeId, Vec<NodeId>>) {
    for successors in adjacency.values_mut() {
        successors.sort_unstable_by_key(|node| node.as_u32());
        successors.dedup();
    }
}

/// Iterative Kosaraju decomposition over a normalized adjacency map.
fn decompose(
    projection: SccProjection,
    nodes: Vec<NodeId>,
    adjacency: FxHashMap<NodeId, Vec<NodeId>>,
) -> SccDecomposition {
    let mut reverse: FxHashMap<NodeId, Vec<NodeId>> = nodes
        .iter()
        .copied()
        .map(|node| (node, Vec::new()))
        .collect();
    for (&source, targets) in &adjacency {
        for &target in targets {
            reverse.entry(target).or_default().push(source);
        }
    }
    normalize_adjacency(&mut reverse);

    let mut visited = FxHashSet::default();
    let mut finish_order = Vec::with_capacity(nodes.len());
    for &start in &nodes {
        if visited.contains(&start) {
            continue;
        }
        let mut stack = vec![(start, false)];
        while let Some((node, exiting)) = stack.pop() {
            if exiting {
                finish_order.push(node);
                continue;
            }
            if !visited.insert(node) {
                continue;
            }
            stack.push((node, true));
            if let Some(successors) = adjacency.get(&node) {
                for &successor in successors.iter().rev() {
                    if !visited.contains(&successor) {
                        stack.push((successor, false));
                    }
                }
            }
        }
    }

    visited.clear();
    let mut component_nodes = Vec::new();
    for &start in finish_order.iter().rev() {
        if !visited.insert(start) {
            continue;
        }
        let mut component = Vec::new();
        let mut stack = vec![start];
        while let Some(node) = stack.pop() {
            component.push(node);
            if let Some(predecessors) = reverse.get(&node) {
                for &predecessor in predecessors.iter().rev() {
                    if visited.insert(predecessor) {
                        stack.push(predecessor);
                    }
                }
            }
        }
        component.sort_unstable_by_key(|node| node.as_u32());
        component_nodes.push(component);
    }

    component_nodes
        .sort_unstable_by_key(|component| component.first().map_or(u32::MAX, |node| node.as_u32()));

    let mut components = Vec::with_capacity(component_nodes.len());
    let mut component_by_node = BTreeMap::new();
    for (id, component_nodes) in component_nodes.into_iter().enumerate() {
        let cyclic = component_nodes.len() > 1
            || component_nodes.first().is_some_and(|node| {
                adjacency
                    .get(node)
                    .is_some_and(|successors| successors.contains(node))
            });
        for &node in &component_nodes {
            component_by_node.insert(node, id);
        }
        components.push(SccComponent {
            id,
            nodes: component_nodes,
            cyclic,
        });
    }

    let mut condensation_set = FxHashSet::default();
    for (&source, targets) in &adjacency {
        let Some(&source_component) = component_by_node.get(&source) else {
            continue;
        };
        for target in targets {
            let Some(&target_component) = component_by_node.get(target) else {
                continue;
            };
            if source_component != target_component {
                condensation_set.insert((source_component, target_component));
            }
        }
    }
    let mut condensation_edges: Vec<SccCondensationEdge> = condensation_set
        .into_iter()
        .map(|(source, target)| SccCondensationEdge { source, target })
        .collect();
    condensation_edges.sort_unstable_by_key(|edge| (edge.source, edge.target));

    SccDecomposition {
        projection,
        components,
        component_by_node,
        condensation_edges,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CfgEdgeKind, CpgNode, Language, MethodSignature, ScopeId, SourceRange, Visibility,
    };
    use proptest::prelude::*;

    fn node(kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(0), kind, SourceRange::default())
    }

    fn function(name: &str) -> CpgNodeKind {
        CpgNodeKind::Function {
            signature: MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            },
        }
    }

    fn ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
        cpg.connect(parent, child, CpgEdgeKind::AstChild);
    }

    fn cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId) {
        cpg.connect(
            source,
            target,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        );
    }

    #[test]
    fn cfg_partition_finds_loop_self_cycle_and_condensation_edges() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node(node(function("f")));
        let block = cpg.add_node(node(CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        }));
        let head = cpg.add_node(node(CpgNodeKind::While));
        let body = cpg.add_node(node(CpgNodeKind::Assignment {
            operator: "=".into(),
        }));
        let tail = cpg.add_node(node(CpgNodeKind::Return));
        for child in [block, head, body, tail] {
            ast(&mut cpg, function, child);
        }
        cfg(&mut cpg, function, block);
        cfg(&mut cpg, block, head);
        cfg(&mut cpg, head, body);
        cfg(&mut cpg, body, head);
        cfg(&mut cpg, head, tail);
        cfg(&mut cpg, tail, tail);

        let result = control_flow_sccs(&cpg, function).expect("valid function");
        assert_eq!(result.node_count(), 5);
        assert_eq!(result.components.len(), 4);
        assert_eq!(
            result.component_of(head).expect("loop component").nodes,
            vec![head, body]
        );
        assert!(result.component_of(head).expect("loop component").cyclic);
        assert!(result
            .component_of(tail)
            .expect("tail component")
            .is_self_cycle());
        assert_eq!(result.cyclic_components().count(), 2);
        assert!(!result.is_acyclic());
        assert!(result
            .condensation_edges
            .iter()
            .all(|edge| edge.source != edge.target));
    }

    #[test]
    fn cfg_scope_excludes_nested_functions_and_interprocedural_call_edges() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let outer = cpg.add_node(node(function("outer")));
        let outer_body = cpg.add_node(node(CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        }));
        let call = cpg.add_node(node(CpgNodeKind::Call {
            target: None,
            is_method: false,
        }));
        let nested = cpg.add_node(node(function("nested")));
        let nested_body = cpg.add_node(node(CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        }));
        ast(&mut cpg, outer, outer_body);
        ast(&mut cpg, outer_body, call);
        ast(&mut cpg, outer_body, nested);
        ast(&mut cpg, nested, nested_body);
        cfg(&mut cpg, outer, outer_body);
        cfg(&mut cpg, outer_body, call);
        cpg.connect(call, nested, CpgEdgeKind::ControlFlow(CfgEdgeKind::Call));
        cfg(&mut cpg, nested, nested_body);
        cfg(&mut cpg, nested_body, nested);

        let outer_result = control_flow_sccs(&cpg, outer).expect("outer CFG");
        assert_eq!(outer_result.node_count(), 3);
        assert!(outer_result.component_of(nested).is_none());
        assert!(outer_result.is_acyclic());

        let nested_result = control_flow_sccs(&cpg, nested).expect("nested CFG");
        assert_eq!(nested_result.node_count(), 2);
        assert_eq!(nested_result.cyclic_components().count(), 1);
    }

    #[test]
    fn empty_cfg_retains_the_function_as_an_isolated_component() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node(node(function("empty")));

        let result = control_flow_sccs(&cpg, function).expect("empty function CFG");
        assert_eq!(result.node_count(), 1);
        assert_eq!(result.components[0].nodes, vec![function]);
        assert!(result.is_acyclic());
    }

    #[test]
    fn cfg_rejects_unknown_and_non_function_nodes() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let block = cpg.add_node(node(CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        }));
        assert_eq!(
            control_flow_sccs(&cpg, NodeId::new(999)),
            Err(SccAnalysisError::UnknownNode(NodeId::new(999)))
        );
        assert_eq!(
            control_flow_sccs(&cpg, block),
            Err(SccAnalysisError::NotAFunction(block))
        );
    }

    #[test]
    fn call_graph_finds_mutual_and_direct_recursion_from_both_edge_encodings() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let a = cpg.add_node(node(function("a")));
        let b = cpg.add_node(node(function("b")));
        let c = cpg.add_node(node(function("c")));
        let isolated = cpg.add_node(node(function("isolated")));
        let call_ab = cpg.add_node(node(CpgNodeKind::Call {
            target: Some(b),
            is_method: false,
        }));
        let call_ba = cpg.add_node(node(CpgNodeKind::Call {
            target: None,
            is_method: false,
        }));
        let call_cc = cpg.add_node(node(CpgNodeKind::Call {
            target: Some(c),
            is_method: false,
        }));
        ast(&mut cpg, a, call_ab);
        ast(&mut cpg, b, call_ba);
        ast(&mut cpg, c, call_cc);
        cpg.connect(call_ba, a, CpgEdgeKind::StaticCall);

        let result = call_graph_sccs(&cpg);
        assert_eq!(result.node_count(), 4);
        assert_eq!(result.components.len(), 3);
        assert_eq!(
            result.component_of(a).expect("mutual component").nodes,
            vec![a, b]
        );
        assert!(result
            .component_of(a)
            .expect("mutual component")
            .is_multi_node_cycle());
        assert!(result
            .component_of(c)
            .expect("direct component")
            .is_self_cycle());
        assert!(
            !result
                .component_of(isolated)
                .expect("isolated component")
                .cyclic
        );
    }

    #[test]
    fn call_graph_does_not_attribute_nested_calls_to_the_outer_function() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let outer = cpg.add_node(node(function("outer")));
        let nested = cpg.add_node(node(function("nested")));
        let target = cpg.add_node(node(function("target")));
        let call = cpg.add_node(node(CpgNodeKind::Call {
            target: Some(target),
            is_method: false,
        }));
        ast(&mut cpg, outer, nested);
        ast(&mut cpg, nested, call);
        cpg.connect(target, nested, CpgEdgeKind::CallSite);

        let result = call_graph_sccs(&cpg);
        assert!(!result.component_of(outer).expect("outer").cyclic);
        assert_eq!(
            result.component_of(nested).expect("nested/target").nodes,
            vec![nested, target]
        );
    }

    #[test]
    fn iterative_decomposition_handles_a_deep_graph_without_native_recursion() {
        const NODE_COUNT: u32 = 50_000;
        let nodes: Vec<NodeId> = (0..NODE_COUNT).map(NodeId::new).collect();
        let adjacency: FxHashMap<NodeId, Vec<NodeId>> = nodes
            .iter()
            .copied()
            .map(|node| {
                let successors = if node.as_u32() + 1 < NODE_COUNT {
                    vec![NodeId::new(node.as_u32() + 1)]
                } else {
                    Vec::new()
                };
                (node, successors)
            })
            .collect();

        let result = decompose(SccProjection::CallGraph, nodes, adjacency);
        assert_eq!(result.node_count(), NODE_COUNT as usize);
        assert_eq!(result.components.len(), NODE_COUNT as usize);
        assert!(result.is_acyclic());
        assert_eq!(result.condensation_edges.len(), NODE_COUNT as usize - 1);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// Differential oracle: for arbitrary small directed graphs, the
        /// iterative implementation returns exactly petgraph's Tarjan
        /// partition, including singleton self-loop components.
        #[test]
        fn iterative_kosaraju_matches_petgraph_tarjan(
            node_count in 1usize..24,
            raw_edges in prop::collection::vec((0usize..24, 0usize..24), 0..96),
        ) {
            let nodes: Vec<NodeId> = (0..node_count as u32).map(NodeId::new).collect();
            let edges: Vec<(usize, usize)> = raw_edges
                .into_iter()
                .filter(|(source, target)| *source < node_count && *target < node_count)
                .collect();
            let adjacency = adjacency_from_edges(
                &nodes,
                edges
                    .iter()
                    .map(|&(source, target)| (nodes[source], nodes[target])),
            );

            let actual = decompose(SccProjection::CallGraph, nodes.clone(), adjacency);
            let mut graph = petgraph::graph::DiGraph::<(), ()>::new();
            let graph_nodes: Vec<_> = (0..node_count).map(|_| graph.add_node(())).collect();
            for &(source, target) in &edges {
                graph.add_edge(graph_nodes[source], graph_nodes[target], ());
            }

            let mut expected: Vec<Vec<u32>> = petgraph::algo::tarjan_scc(&graph)
                .into_iter()
                .map(|mut component| {
                    let mut ids: Vec<u32> = component
                        .drain(..)
                        .map(|node| node.index() as u32)
                        .collect();
                    ids.sort_unstable();
                    ids
                })
                .collect();
            expected.sort_unstable();
            let mut found: Vec<Vec<u32>> = actual
                .components
                .iter()
                .map(|component| component.nodes.iter().map(|node| node.as_u32()).collect())
                .collect();
            found.sort_unstable();

            prop_assert_eq!(found, expected);
            prop_assert_eq!(actual.node_count(), node_count);
            prop_assert_eq!(
                actual.components.iter().map(|component| component.nodes.len()).sum::<usize>(),
                node_count,
            );

            let mut condensation = petgraph::graph::DiGraph::<(), ()>::new();
            let condensation_nodes: Vec<_> = (0..actual.components.len())
                .map(|_| condensation.add_node(()))
                .collect();
            for edge in &actual.condensation_edges {
                condensation.add_edge(
                    condensation_nodes[edge.source],
                    condensation_nodes[edge.target],
                    (),
                );
            }
            prop_assert!(!petgraph::algo::is_cyclic_directed(&condensation));
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn decomposition_round_trips_through_json() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node(node(function("f")));
        cfg(&mut cpg, function, function);
        let result = control_flow_sccs(&cpg, function).expect("CFG");

        let encoded = serde_json::to_string(&result).expect("serialize SCC result");
        let decoded: SccDecomposition =
            serde_json::from_str(&encoded).expect("deserialize SCC result");
        assert_eq!(decoded, result);
    }
}
