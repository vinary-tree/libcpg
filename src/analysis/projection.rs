//! Deterministic, feature-free graph projections over CPG node identifiers.
//!
//! A [`GraphProjection`] is a read-only compressed-sparse-row (CSR) view. It
//! copies only node identifiers, edge endpoints, and optional weights; it never
//! clones [`CpgNode`] payloads or mutates the source [`CodePropertyGraph`].

use std::{
    cmp::{Ordering, Reverse},
    collections::{BTreeMap, BinaryHeap},
};

use crate::{CfgEdgeKind, CodePropertyGraph, CpgEdge, CpgEdgeKind, CpgNode, CpgNodeKind, NodeId};

use super::scc::{call_graph_edges, function_scope_nodes, sorted_nodes};
use super::SccDecomposition;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// The CPG overlay or external graph represented by a projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ProjectionKind {
    /// Resolved function-to-function calls across the complete CPG.
    CallGraph,
    /// Root/module import dependencies across the complete CPG.
    ModuleGraph,
    /// Intraprocedural control flow for one function.
    ControlFlow {
        /// Function whose control-flow edges are represented.
        function: NodeId,
    },
    /// Data flow, optionally restricted to one AST scope.
    DataFlow {
        /// Scope root, or `None` for the complete DFG overlay.
        function: Option<NodeId>,
    },
    /// Program dependence for one function.
    ProgramDependence {
        /// Function whose dependence edges are represented.
        function: NodeId,
    },
    /// Directed acyclic graph obtained by contracting SCCs.
    Condensation,
    /// A caller-supplied graph that does not originate from a CPG overlay.
    External,
}

/// A deterministic directed graph in forward and reverse CSR form.
///
/// `nodes[d]` maps a dense identifier `d` back to its stable [`NodeId`]. The
/// half-open range `offsets[d]..offsets[d + 1]` indexes `targets` and, when
/// present, `weights`. `rev_offsets` and `rev_targets` encode predecessors.
/// Nodes and every adjacency slice are sorted ascending. Parallel edges are
/// represented once with their weights summed.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct GraphProjection {
    /// CPG overlay or external graph represented by this value.
    pub kind: ProjectionKind,
    /// Stable node identifiers, sorted ascending and deduplicated.
    pub nodes: Vec<NodeId>,
    /// Forward-CSR offsets; its length is `nodes.len() + 1`.
    pub offsets: Vec<u32>,
    /// Dense successor identifiers grouped by source node.
    pub targets: Vec<u32>,
    /// Reverse-CSR offsets; its length is `nodes.len() + 1`.
    pub rev_offsets: Vec<u32>,
    /// Dense predecessor identifiers grouped by target node.
    pub rev_targets: Vec<u32>,
    /// Forward edge weights aligned with `targets`, or `None` when all are 1.
    pub weights: Option<Vec<f64>>,
}

impl GraphProjection {
    /// Builds a deterministic projection and merges parallel edges.
    ///
    /// Edges whose source or target is absent from `nodes` are ignored. Within
    /// each `(source, target)` group, weights are sorted with [`f64::total_cmp`]
    /// before addition, so the result is independent of input iteration order.
    /// Unit weights are implicit: `weights` is `None` when every merged edge
    /// has weight `1.0`.
    ///
    /// # Panics
    ///
    /// Panics only if the projection is too large for its documented `u32`
    /// dense-index or CSR-offset representation.
    pub fn from_edges(
        kind: ProjectionKind,
        nodes: impl IntoIterator<Item = NodeId>,
        edges: impl IntoIterator<Item = (NodeId, NodeId, f64)>,
    ) -> Self {
        let nodes = sorted_nodes(nodes);
        let mut dense_by_node = BTreeMap::new();
        for (dense, node) in nodes.iter().copied().enumerate() {
            dense_by_node.insert(
                node,
                u32::try_from(dense)
                    .expect("projection node count exceeds the u32 dense-index domain"),
            );
        }

        let mut dense_edges = Vec::new();
        for (source, target, weight) in edges {
            if let (Some(&source), Some(&target)) =
                (dense_by_node.get(&source), dense_by_node.get(&target))
            {
                dense_edges.push((source, target, weight));
            }
        }
        let dense_edges = merge_dense_edges(dense_edges);
        let (offsets, targets) = csr_from_sorted_edges(nodes.len(), &dense_edges);

        let reverse_edges = merge_dense_edges(
            dense_edges
                .iter()
                .map(|&(source, target, weight)| (target, source, weight))
                .collect(),
        );
        let (rev_offsets, rev_targets) = csr_from_sorted_edges(nodes.len(), &reverse_edges);

        let weights = if dense_edges.iter().all(|edge| edge.2 == 1.0) {
            None
        } else {
            Some(dense_edges.iter().map(|edge| edge.2).collect())
        };

        Self {
            kind,
            nodes,
            offsets,
            targets,
            rev_offsets,
            rev_targets,
            weights,
        }
    }

    /// Returns the number of vertices in the projection.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Returns the number of directed edges after parallel-edge merging.
    pub fn edge_count(&self) -> usize {
        self.targets.len()
    }

    /// Returns true when the projection contains no vertices.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Returns whether the public fields satisfy the constructor's CSR invariants.
    ///
    /// This validates strictly sorted stable nodes, forward and reverse CSR
    /// shape, in-range sorted adjacency, optional-weight alignment, and exact
    /// forward/reverse edge agreement. Analyses that accept deserialized or
    /// caller-mutated projections can use this check before indexed traversal.
    pub fn is_well_formed(&self) -> bool {
        let n = self.node_count();
        if self.nodes.windows(2).any(|pair| pair[0] >= pair[1])
            || !csr_is_well_formed(&self.offsets, &self.targets, n)
            || !csr_is_well_formed(&self.rev_offsets, &self.rev_targets, n)
            || self.targets.len() != self.rev_targets.len()
            || self
                .weights
                .as_ref()
                .is_some_and(|weights| weights.len() != self.targets.len())
        {
            return false;
        }

        for source in 0..n {
            let start = self.offsets[source] as usize;
            let end = self.offsets[source + 1] as usize;
            for &target in &self.targets[start..end] {
                let target = target as usize;
                let reverse_start = self.rev_offsets[target] as usize;
                let reverse_end = self.rev_offsets[target + 1] as usize;
                if self.rev_targets[reverse_start..reverse_end]
                    .binary_search(&(source as u32))
                    .is_err()
                {
                    return false;
                }
            }
        }
        true
    }

    /// Maps a stable CPG node identifier to its dense projection identifier.
    pub fn dense_index(&self, id: NodeId) -> Option<u32> {
        self.nodes.binary_search(&id).ok().map(|index| index as u32)
    }

    /// Maps a dense projection identifier back to its stable CPG node id.
    ///
    /// # Panics
    ///
    /// Panics when `dense` is outside `0..self.node_count()`.
    pub fn node_id(&self, dense: u32) -> NodeId {
        self.nodes[dense as usize]
    }

    /// Returns the sorted dense successors of `dense` without allocation.
    ///
    /// # Panics
    ///
    /// Panics when `dense` is outside `0..self.node_count()`.
    pub fn successors(&self, dense: u32) -> &[u32] {
        csr_slice(&self.offsets, &self.targets, dense)
    }

    /// Returns the sorted dense predecessors of `dense` without allocation.
    ///
    /// # Panics
    ///
    /// Panics when `dense` is outside `0..self.node_count()`.
    pub fn predecessors(&self, dense: u32) -> &[u32] {
        csr_slice(&self.rev_offsets, &self.rev_targets, dense)
    }

    /// Returns weights aligned with [`Self::successors`], if weights are not
    /// uniformly one.
    ///
    /// # Panics
    ///
    /// Panics when `dense` is outside `0..self.node_count()`.
    pub fn successor_weights(&self, dense: u32) -> Option<&[f64]> {
        let start = self.offsets[dense as usize] as usize;
        let end = self.offsets[dense as usize + 1] as usize;
        let weights = self.weights.as_deref()?;
        Some(&weights[start..end])
    }

    /// Reverses every edge while retaining stable node identifiers and kind.
    pub fn transpose(&self) -> GraphProjection {
        let edges = self
            .edges()
            .map(|(source, target, weight)| (self.node_id(target), self.node_id(source), weight));
        Self::from_edges(self.kind, self.nodes.iter().copied(), edges)
    }

    /// Returns an undirected projection formed by forgetting edge direction.
    ///
    /// Reciprocal directed edges become one undirected edge whose weight is the
    /// deterministic sum of both directions. A self-loop remains one self-loop
    /// rather than being counted twice.
    pub fn undirected(&self) -> UndirectedProjection {
        let pairs = self.edges().map(|(source, target, weight)| {
            if source <= target {
                (source, target, weight)
            } else {
                (target, source, weight)
            }
        });
        let pairs = merge_dense_edges(pairs.collect());
        let mut symmetric = Vec::with_capacity(pairs.len().saturating_mul(2));
        for (source, target, weight) in pairs {
            symmetric.push((source, target, weight));
            if source != target {
                symmetric.push((target, source, weight));
            }
        }
        let symmetric = merge_dense_edges(symmetric);
        let (offsets, targets) = csr_from_sorted_edges(self.nodes.len(), &symmetric);
        let weights = if symmetric.iter().all(|edge| edge.2 == 1.0) {
            None
        } else {
            Some(symmetric.iter().map(|edge| edge.2).collect())
        };

        UndirectedProjection {
            kind: self.kind,
            nodes: self.nodes.clone(),
            offsets,
            targets,
            weights,
        }
    }

    /// Iterates dense directed edges in canonical `(source, target)` order.
    pub fn edges(&self) -> impl Iterator<Item = (u32, u32, f64)> + '_ {
        self.offsets
            .windows(2)
            .enumerate()
            .flat_map(move |(source, range)| {
                let start = range[0] as usize;
                let end = range[1] as usize;
                self.targets[start..end].iter().copied().enumerate().map(
                    move |(relative, target)| {
                        let edge = start + relative;
                        let weight = self.weights.as_ref().map_or(1.0, |weights| weights[edge]);
                        (source as u32, target, weight)
                    },
                )
            })
    }
}

/// A deterministic undirected CSR view derived from a [`GraphProjection`].
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct UndirectedProjection {
    /// Kind of the directed projection from which this view was derived.
    pub kind: ProjectionKind,
    /// Stable node identifiers, sorted ascending and deduplicated.
    pub nodes: Vec<NodeId>,
    /// CSR offsets; its length is `nodes.len() + 1`.
    pub offsets: Vec<u32>,
    /// Sorted dense neighbor identifiers.
    pub targets: Vec<u32>,
    /// Adjacency weights aligned with `targets`, or `None` when all are 1.
    pub weights: Option<Vec<f64>>,
}

impl UndirectedProjection {
    /// Returns the number of vertices in the projection.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Returns the number of undirected edges, counting a self-loop once.
    pub fn edge_count(&self) -> usize {
        let directed_entries = self.targets.len();
        let self_loops = self
            .offsets
            .windows(2)
            .enumerate()
            .filter(|(node, range)| {
                self.targets[range[0] as usize..range[1] as usize]
                    .binary_search(&(*node as u32))
                    .is_ok()
            })
            .count();
        (directed_entries + self_loops) / 2
    }

    /// Maps a stable CPG node identifier to its dense projection identifier.
    pub fn dense_index(&self, id: NodeId) -> Option<u32> {
        self.nodes.binary_search(&id).ok().map(|index| index as u32)
    }

    /// Maps a dense projection identifier back to its stable CPG node id.
    ///
    /// # Panics
    ///
    /// Panics when `dense` is outside `0..self.node_count()`.
    pub fn node_id(&self, dense: u32) -> NodeId {
        self.nodes[dense as usize]
    }

    /// Returns the sorted dense neighbors of `dense` without allocation.
    ///
    /// # Panics
    ///
    /// Panics when `dense` is outside `0..self.node_count()`.
    pub fn neighbors(&self, dense: u32) -> &[u32] {
        csr_slice(&self.offsets, &self.targets, dense)
    }

    /// Returns weights aligned with [`Self::neighbors`], if non-unit weights
    /// are present.
    ///
    /// # Panics
    ///
    /// Panics when `dense` is outside `0..self.node_count()`.
    pub fn neighbor_weights(&self, dense: u32) -> Option<&[f64]> {
        let start = self.offsets[dense as usize] as usize;
        let end = self.offsets[dense as usize + 1] as usize;
        let weights = self.weights.as_deref()?;
        Some(&weights[start..end])
    }
}

/// Errors caused by selecting an invalid CPG projection scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProjectionError {
    /// The requested node does not exist in the CPG.
    #[error("projection node does not exist: {0:?}")]
    UnknownNode(NodeId),
    /// The requested node exists but is not a function.
    #[error("function-scoped projection requires a function node: {0:?}")]
    NotAFunction(NodeId),
}

/// Builds the whole CPG's resolved function-to-function call graph.
pub fn call_graph_projection(cpg: &CodePropertyGraph) -> GraphProjection {
    let nodes = sorted_nodes(cpg.functions().map(|node| node.id));
    let edges = call_graph_edges(cpg, &nodes)
        .into_iter()
        .map(|(source, target)| (source, target, 1.0));
    GraphProjection::from_edges(ProjectionKind::CallGraph, nodes, edges)
}

/// Builds the root/module import graph.
///
/// Only `Imports` edges whose two endpoints are `Root` or `Module` nodes are
/// retained. Import-statement AST nodes therefore do not become module
/// vertices accidentally.
pub fn module_graph_projection(cpg: &CodePropertyGraph) -> GraphProjection {
    projection_of(
        cpg,
        ProjectionKind::ModuleGraph,
        |node| matches!(node.kind, CpgNodeKind::Root | CpgNodeKind::Module { .. }),
        |edge| matches!(edge.kind, CpgEdgeKind::Imports),
        |_| 1.0,
    )
}

/// Builds one function's intraprocedural control-flow projection.
///
/// Nested functions and CFG `Call` edges are excluded. The selected function
/// is retained even when it has no incident CFG edge; other AST-only nodes are
/// omitted.
pub fn cfg_projection(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<GraphProjection, ProjectionError> {
    validate_function(cpg, function)?;
    let scope = function_scope_nodes(cpg, function);
    let mut nodes = vec![function];
    let mut edges = Vec::new();
    for edge in cpg.cfg_overlay_edges() {
        if edge.kind.is_cfg()
            && !matches!(edge.kind, CpgEdgeKind::ControlFlow(CfgEdgeKind::Call))
            && scope.contains(&edge.source)
            && scope.contains(&edge.target)
        {
            nodes.extend([edge.source, edge.target]);
            edges.push((edge.source, edge.target, 1.0));
        }
    }
    Ok(GraphProjection::from_edges(
        ProjectionKind::ControlFlow { function },
        nodes,
        edges,
    ))
}

/// Builds a data-flow projection, optionally restricted to one AST scope.
///
/// With a scope root, the root is retained as an isolated vertex and only DFG
/// edges whose endpoints lie in that subtree are selected. An unknown scope
/// produces an empty projection because this intentionally non-validating API
/// accepts any AST scope, not only function nodes.
pub fn dfg_projection(cpg: &CodePropertyGraph, function: Option<NodeId>) -> GraphProjection {
    let scope = function.and_then(|root| {
        cpg.contains_node(root)
            .then(|| function_scope_nodes(cpg, root))
    });
    let mut nodes = function
        .filter(|&root| cpg.contains_node(root))
        .into_iter()
        .collect::<Vec<_>>();
    let mut edges = Vec::new();
    for edge in cpg.dfg_overlay_edges() {
        let in_scope = scope
            .as_ref()
            .is_none_or(|scope| scope.contains(&edge.source) && scope.contains(&edge.target));
        if edge.kind.is_dfg() && in_scope {
            nodes.extend([edge.source, edge.target]);
            edges.push((edge.source, edge.target, 1.0));
        }
    }
    GraphProjection::from_edges(ProjectionKind::DataFlow { function }, nodes, edges)
}

/// Builds one function's program-dependence projection.
pub fn pdg_projection(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<GraphProjection, ProjectionError> {
    validate_function(cpg, function)?;
    let scope = function_scope_nodes(cpg, function);
    let mut nodes = vec![function];
    let mut edges = Vec::new();
    for edge in cpg.pdg_overlay_edges() {
        if edge.kind.is_pdg() && scope.contains(&edge.source) && scope.contains(&edge.target) {
            nodes.extend([edge.source, edge.target]);
            edges.push((edge.source, edge.target, 1.0));
        }
    }
    Ok(GraphProjection::from_edges(
        ProjectionKind::ProgramDependence { function },
        nodes,
        edges,
    ))
}

/// Projects caller-selected nodes and edges without cloning node payloads.
///
/// `node_filter` defines the vertex universe. `edge_filter` and `weight`
/// define its directed edge relation; edges with an endpoint outside the
/// selected universe are ignored by [`GraphProjection::from_edges`].
pub fn projection_of(
    cpg: &CodePropertyGraph,
    kind: ProjectionKind,
    node_filter: impl Fn(&CpgNode) -> bool,
    edge_filter: impl Fn(&CpgEdge) -> bool,
    weight: impl Fn(&CpgEdge) -> f64,
) -> GraphProjection {
    let nodes = cpg
        .nodes()
        .filter(|node| node_filter(node))
        .map(|node| node.id);
    let edges = cpg
        .edges()
        .filter(|edge| edge_filter(edge))
        .map(|edge| (edge.source, edge.target, weight(edge)));
    GraphProjection::from_edges(kind, nodes, edges)
}

impl SccDecomposition {
    /// Projects the SCC condensation DAG using each component's smallest node
    /// identifier as its stable representative.
    ///
    /// For decompositions produced by libcpg, component representatives are
    /// ascending, so a representative's dense projection id equals the SCC id.
    pub fn condensation_projection(&self) -> GraphProjection {
        let representatives: Vec<Option<NodeId>> = self
            .components
            .iter()
            .map(|component| component.nodes.first().copied())
            .collect();
        let nodes = representatives.iter().flatten().copied();
        let edges = self.condensation_edges.iter().filter_map(|edge| {
            let source = representatives.get(edge.source).copied().flatten()?;
            let target = representatives.get(edge.target).copied().flatten()?;
            Some((source, target, 1.0))
        });
        GraphProjection::from_edges(ProjectionKind::Condensation, nodes, edges)
    }
}

fn validate_function(cpg: &CodePropertyGraph, function: NodeId) -> Result<(), ProjectionError> {
    let Some(node) = cpg.node(function) else {
        return Err(ProjectionError::UnknownNode(function));
    };
    if !matches!(node.kind, CpgNodeKind::Function { .. }) {
        return Err(ProjectionError::NotAFunction(function));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct OrderedDenseEdge {
    source: u32,
    target: u32,
    weight: f64,
}

impl PartialEq for OrderedDenseEdge {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for OrderedDenseEdge {}

impl PartialOrd for OrderedDenseEdge {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedDenseEdge {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.source, self.target)
            .cmp(&(other.source, other.target))
            .then_with(|| self.weight.total_cmp(&other.weight))
    }
}

/// Orders and merges dense edges without a native-stack sorting recursion.
///
/// The binary heap owns its traversal state. Popping its reversed total order
/// preserves the prior `(source, target, weight.total_cmp())` contract,
/// including deterministic addition of parallel floating-point weights.
fn merge_dense_edges(edges: Vec<(u32, u32, f64)>) -> Vec<(u32, u32, f64)> {
    let edge_count = edges.len();
    let mut ordered = BinaryHeap::with_capacity(edge_count);
    for (source, target, weight) in edges {
        ordered.push(Reverse(OrderedDenseEdge {
            source,
            target,
            weight,
        }));
    }

    let mut merged: Vec<(u32, u32, f64)> = Vec::with_capacity(edge_count);
    while let Some(Reverse(OrderedDenseEdge {
        source,
        target,
        weight,
    })) = ordered.pop()
    {
        if let Some(last) = merged
            .last_mut()
            .filter(|last| last.0 == source && last.1 == target)
        {
            last.2 += weight;
        } else {
            merged.push((source, target, weight));
        }
    }
    merged
}

fn csr_from_sorted_edges(node_count: usize, edges: &[(u32, u32, f64)]) -> (Vec<u32>, Vec<u32>) {
    let mut offsets = Vec::with_capacity(node_count + 1);
    let mut targets = Vec::with_capacity(edges.len());
    let mut edge_index = 0usize;
    offsets.push(0);
    for source in 0..node_count {
        while edge_index < edges.len() && edges[edge_index].0 as usize == source {
            targets.push(edges[edge_index].1);
            edge_index += 1;
        }
        offsets.push(
            u32::try_from(targets.len())
                .expect("projection edge count exceeds the u32 CSR-offset domain"),
        );
    }
    debug_assert_eq!(edge_index, edges.len());
    (offsets, targets)
}

fn csr_slice<'a>(offsets: &[u32], values: &'a [u32], dense: u32) -> &'a [u32] {
    let start = offsets[dense as usize] as usize;
    let end = offsets[dense as usize + 1] as usize;
    &values[start..end]
}

fn csr_is_well_formed(offsets: &[u32], targets: &[u32], node_count: usize) -> bool {
    if offsets.len() != node_count + 1
        || offsets.first() != Some(&0)
        || offsets.last().copied().map(|end| end as usize) != Some(targets.len())
        || offsets.windows(2).any(|pair| pair[0] > pair[1])
        || targets.iter().any(|&target| target as usize >= node_count)
    {
        return false;
    }

    offsets.windows(2).all(|range| {
        targets[range[0] as usize..range[1] as usize]
            .windows(2)
            .all(|pair| pair[0] < pair[1])
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use proptest::prelude::*;

    use super::*;
    use crate::{DfgEdgeKind, Language, MethodSignature, ScopeId, SourceRange, Visibility};

    fn node(kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(0), kind, SourceRange::default())
    }

    fn function(name: &str) -> CpgNodeKind {
        CpgNodeKind::Function {
            signature: Box::new(MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            }),
        }
    }

    fn edge_set(projection: &GraphProjection) -> BTreeSet<(NodeId, NodeId)> {
        projection
            .edges()
            .map(|(source, target, _)| (projection.node_id(source), projection.node_id(target)))
            .collect()
    }

    #[test]
    fn csr_sorts_nodes_merges_parallel_edges_and_builds_reverse_index() {
        let projection = GraphProjection::from_edges(
            ProjectionKind::External,
            [
                NodeId::new(9),
                NodeId::new(2),
                NodeId::new(5),
                NodeId::new(2),
            ],
            [
                (NodeId::new(9), NodeId::new(2), 3.0),
                (NodeId::new(2), NodeId::new(9), 2.0),
                (NodeId::new(9), NodeId::new(2), 1.0),
                (NodeId::new(5), NodeId::new(5), 1.0),
                (NodeId::new(99), NodeId::new(2), 8.0),
            ],
        );

        assert_eq!(
            projection.nodes,
            vec![NodeId::new(2), NodeId::new(5), NodeId::new(9)]
        );
        assert_eq!(projection.offsets, vec![0, 1, 2, 3]);
        assert_eq!(projection.targets, vec![2, 1, 0]);
        assert_eq!(projection.rev_offsets, vec![0, 1, 2, 3]);
        assert_eq!(projection.rev_targets, vec![2, 1, 0]);
        assert_eq!(projection.weights, Some(vec![2.0, 1.0, 4.0]));
        assert_eq!(projection.dense_index(NodeId::new(5)), Some(1));
        assert_eq!(projection.dense_index(NodeId::new(99)), None);
        assert_eq!(projection.node_id(2), NodeId::new(9));
        assert_eq!(projection.successors(0), &[2]);
        assert_eq!(projection.predecessors(0), &[2]);
        assert_eq!(projection.successor_weights(1), Some(&[1.0][..]));
        assert!(projection.is_well_formed());
    }

    #[test]
    fn unit_weights_are_implicit_and_empty_projection_is_well_formed() {
        let unit = GraphProjection::from_edges(
            ProjectionKind::External,
            [NodeId::new(0), NodeId::new(1)],
            [(NodeId::new(0), NodeId::new(1), 1.0)],
        );
        assert_eq!(unit.weights, None);
        assert_eq!(unit.node_count(), 2);
        assert_eq!(unit.edge_count(), 1);
        assert!(!unit.is_empty());
        assert_eq!(unit.successor_weights(0), None);

        let empty = GraphProjection::from_edges(
            ProjectionKind::External,
            std::iter::empty(),
            std::iter::empty(),
        );
        assert!(empty.is_empty());
        assert_eq!(empty.offsets, vec![0]);
        assert_eq!(empty.rev_offsets, vec![0]);
        assert!(empty.is_well_formed());

        let mut malformed = unit;
        malformed.rev_targets[0] = 1;
        assert!(!malformed.is_well_formed());
    }

    #[test]
    fn public_projection_validation_rejects_each_csr_invariant_violation() {
        let graph = GraphProjection::from_edges(
            ProjectionKind::External,
            [NodeId::new(0), NodeId::new(1), NodeId::new(2)],
            [
                (NodeId::new(0), NodeId::new(1), 2.0),
                (NodeId::new(0), NodeId::new(2), 3.0),
            ],
        );
        assert!(graph.is_well_formed());

        let mut cases = Vec::new();

        let mut duplicate_nodes = graph.clone();
        duplicate_nodes.nodes[1] = duplicate_nodes.nodes[0];
        cases.push(duplicate_nodes);

        let mut short_offsets = graph.clone();
        short_offsets.offsets.pop();
        cases.push(short_offsets);

        let mut nonzero_start = graph.clone();
        nonzero_start.offsets[0] = 1;
        cases.push(nonzero_start);

        let mut wrong_end = graph.clone();
        *wrong_end.offsets.last_mut().expect("nonempty offsets") = 1;
        cases.push(wrong_end);

        let mut descending_offsets = graph.clone();
        descending_offsets.offsets = vec![0, 2, 1, 2];
        cases.push(descending_offsets);

        let mut out_of_range_target = graph.clone();
        out_of_range_target.targets[0] = 3;
        cases.push(out_of_range_target);

        let mut duplicate_target = graph.clone();
        duplicate_target.targets[1] = duplicate_target.targets[0];
        cases.push(duplicate_target);

        let mut short_reverse = graph.clone();
        short_reverse.rev_offsets.pop();
        cases.push(short_reverse);

        let mut unequal_edge_counts = graph.clone();
        unequal_edge_counts.rev_targets.pop();
        *unequal_edge_counts
            .rev_offsets
            .last_mut()
            .expect("nonempty reverse offsets") -= 1;
        cases.push(unequal_edge_counts);

        let mut mismatched_reverse = graph.clone();
        mismatched_reverse.rev_targets[1] = 1;
        cases.push(mismatched_reverse);

        let mut short_weights = graph.clone();
        short_weights
            .weights
            .as_mut()
            .expect("weighted fixture")
            .pop();
        cases.push(short_weights);

        for malformed in cases {
            assert!(!malformed.is_well_formed());
        }
    }

    #[test]
    fn transpose_and_undirected_views_preserve_weights_and_self_loops() {
        let graph = GraphProjection::from_edges(
            ProjectionKind::External,
            [NodeId::new(0), NodeId::new(1), NodeId::new(2)],
            [
                (NodeId::new(0), NodeId::new(1), 2.0),
                (NodeId::new(1), NodeId::new(0), 3.0),
                (NodeId::new(2), NodeId::new(2), 4.0),
            ],
        );
        assert_eq!(graph.transpose().transpose(), graph);

        let undirected = graph.undirected();
        assert_eq!(undirected.node_count(), 3);
        assert_eq!(undirected.edge_count(), 2);
        assert_eq!(undirected.neighbors(0), &[1]);
        assert_eq!(undirected.neighbors(1), &[0]);
        assert_eq!(undirected.neighbors(2), &[2]);
        assert_eq!(undirected.neighbor_weights(0), Some(&[5.0][..]));
        assert_eq!(undirected.neighbor_weights(2), Some(&[4.0][..]));
        assert_eq!(undirected.dense_index(NodeId::new(2)), Some(2));
        assert_eq!(undirected.node_id(1), NodeId::new(1));
    }

    #[test]
    fn overlay_constructors_match_their_direct_edge_relations() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let root = cpg.add_node(node(CpgNodeKind::Root));
        let module = cpg.add_node(node(CpgNodeKind::Module { name: "m".into() }));
        let caller = cpg.add_node(node(function("caller")));
        let block = cpg.add_node(node(CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        }));
        let statement = cpg.add_node(node(CpgNodeKind::Return));
        let call = cpg.add_node(node(CpgNodeKind::Call {
            target: None,
            is_method: false,
        }));
        let callee = cpg.add_node(node(function("callee")));
        for child in [block, statement, call] {
            cpg.connect(caller, child, CpgEdgeKind::AstChild);
        }
        cpg.connect(root, module, CpgEdgeKind::Imports);
        cpg.connect(
            caller,
            block,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        );
        cpg.connect(
            block,
            statement,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Return),
        );
        cpg.connect(call, callee, CpgEdgeKind::ControlFlow(CfgEdgeKind::Call));
        cpg.connect(block, statement, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse));
        cpg.connect(
            callee,
            callee,
            CpgEdgeKind::DataFlow(DfgEdgeKind::ReturnValue),
        );
        cpg.connect(block, statement, CpgEdgeKind::ControlDependence);
        cpg.connect(call, callee, CpgEdgeKind::StaticCall);
        cpg.connect(call, callee, CpgEdgeKind::DynamicCall);
        cpg.connect(call, callee, CpgEdgeKind::CallSite);
        cpg.node_mut(call).expect("call node").kind = CpgNodeKind::Call {
            target: Some(callee),
            is_method: false,
        };

        assert_eq!(
            edge_set(&module_graph_projection(&cpg)),
            BTreeSet::from([(root, module)])
        );
        assert_eq!(
            edge_set(&cfg_projection(&cpg, caller).expect("caller CFG")),
            BTreeSet::from([(caller, block), (block, statement)])
        );
        assert_eq!(
            edge_set(&dfg_projection(&cpg, Some(caller))),
            BTreeSet::from([(block, statement)])
        );
        assert_eq!(
            edge_set(&dfg_projection(&cpg, None)),
            BTreeSet::from([(block, statement), (callee, callee)])
        );
        assert_eq!(
            edge_set(&pdg_projection(&cpg, caller).expect("caller PDG")),
            BTreeSet::from([(block, statement)])
        );
        assert_eq!(
            edge_set(&call_graph_projection(&cpg)),
            BTreeSet::from([(caller, callee)])
        );
    }

    #[test]
    fn scoped_projection_errors_are_precise_and_reads_do_not_mutate_the_cpg() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function = cpg.add_node(node(function("f")));
        let block = cpg.add_node(node(CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        }));
        cpg.connect(function, block, CpgEdgeKind::AstChild);
        let before_nodes = cpg.node_count();
        let before_edges = cpg.edge_count();

        assert_eq!(
            cfg_projection(&cpg, NodeId::new(999)),
            Err(ProjectionError::UnknownNode(NodeId::new(999)))
        );
        assert_eq!(
            pdg_projection(&cpg, block),
            Err(ProjectionError::NotAFunction(block))
        );
        let missing_dfg = dfg_projection(&cpg, Some(NodeId::new(999)));
        assert!(missing_dfg.is_empty());
        let _ = cfg_projection(&cpg, function).expect("valid CFG");
        let _ = call_graph_projection(&cpg);
        let _ = module_graph_projection(&cpg);

        assert_eq!(cpg.node_count(), before_nodes);
        assert_eq!(cpg.edge_count(), before_edges);
        assert!(cpg.contains_node(function));
        assert!(cpg.contains_node(block));
    }

    #[test]
    fn projection_of_applies_node_edge_and_weight_functions() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let first = cpg.add_node(node(CpgNodeKind::Return));
        let second = cpg.add_node(node(CpgNodeKind::Return));
        let excluded = cpg.add_node(node(CpgNodeKind::Break));
        cpg.connect(first, second, CpgEdgeKind::ControlDependence);
        cpg.connect(second, excluded, CpgEdgeKind::ControlDependence);

        let projection = projection_of(
            &cpg,
            ProjectionKind::External,
            |node| matches!(node.kind, CpgNodeKind::Return),
            |edge| edge.kind.is_pdg(),
            |_| 2.5,
        );
        assert_eq!(projection.nodes, vec![first, second]);
        assert_eq!(edge_set(&projection), BTreeSet::from([(first, second)]));
        assert_eq!(projection.weights, Some(vec![2.5]));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn csr_is_deterministic_under_edge_permutation(
            node_count in 0usize..24,
            raw_edges in prop::collection::vec((0usize..24, 0usize..24, -8i16..=8), 0..128),
        ) {
            let nodes: Vec<NodeId> = (0..node_count as u32).map(NodeId::new).collect();
            let edges: Vec<_> = raw_edges
                .into_iter()
                .filter(|(source, target, _)| *source < node_count && *target < node_count)
                .map(|(source, target, weight)| (nodes[source], nodes[target], f64::from(weight)))
                .collect();
            let forward = GraphProjection::from_edges(
                ProjectionKind::External,
                nodes.iter().copied(),
                edges.iter().copied(),
            );
            let repeated = GraphProjection::from_edges(
                ProjectionKind::External,
                nodes.iter().copied(),
                edges.iter().copied(),
            );
            let reversed = GraphProjection::from_edges(
                ProjectionKind::External,
                nodes.iter().rev().copied(),
                edges.iter().rev().copied(),
            );

            prop_assert_eq!(&forward, &repeated);
            prop_assert_eq!(&forward, &reversed);
            prop_assert_eq!(forward.offsets.len(), forward.node_count() + 1);
            prop_assert_eq!(forward.rev_offsets.len(), forward.node_count() + 1);
            prop_assert_eq!(forward.offsets.last().copied(), Some(forward.edge_count() as u32));
            prop_assert_eq!(forward.rev_offsets.last().copied(), Some(forward.edge_count() as u32));
            prop_assert!(forward.is_well_formed());
            if let Some(weights) = &forward.weights {
                prop_assert_eq!(weights.len(), forward.edge_count());
            }
            for dense in 0..forward.node_count() as u32 {
                prop_assert!(forward.successors(dense).windows(2).all(|pair| pair[0] < pair[1]));
                prop_assert!(forward.predecessors(dense).windows(2).all(|pair| pair[0] < pair[1]));
                for &target in forward.successors(dense) {
                    prop_assert!(forward.predecessors(target).binary_search(&dense).is_ok());
                }
            }
            let transposed_twice = forward.transpose().transpose();
            prop_assert_eq!(&transposed_twice, &forward);

            let undirected = forward.undirected();
            for dense in 0..undirected.node_count() as u32 {
                for &neighbor in undirected.neighbors(dense) {
                    prop_assert!(undirected.neighbors(neighbor).binary_search(&dense).is_ok());
                }
            }
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn projection_round_trips_through_json() {
        let projection = GraphProjection::from_edges(
            ProjectionKind::External,
            [NodeId::new(3), NodeId::new(8)],
            [(NodeId::new(3), NodeId::new(8), 2.5)],
        );
        let encoded = serde_json::to_string(&projection).expect("serialize projection");
        let decoded: GraphProjection =
            serde_json::from_str(&encoded).expect("deserialize projection");
        assert_eq!(decoded, projection);
    }
}
