//! Lazily built, deterministic CSR indices for CPG edge projections.
//!
//! The cache stores petgraph edge indices rather than copied edge payloads.
//! Forward and reverse rows are bucketed by overlay, then ordered by the
//! opposite endpoint and stable edge id. A second forward row order groups by
//! target so [`CodePropertyGraph::edges_between`](super::CodePropertyGraph::edges_between)
//! is a pair of binary searches followed by a borrowed slice walk.

use std::{fmt, ops::Index, sync::OnceLock};

use petgraph::graph::{DiGraph, EdgeIndex, NodeIndex};

use super::{CfgEdgeKind, CpgEdge, CpgEdgeKind, CpgNode, DfgEdgeKind, NodeId};

/// One borrowed, exact-size sequence of CPG edges.
///
/// Values are resolved from cached petgraph edge indices on demand. The view
/// owns no edge payload and performs no allocation. Cloning it only copies its
/// cursors and references.
#[derive(Clone, Copy)]
pub struct CpgEdges<'a> {
    graph: &'a DiGraph<CpgNode, CpgEdge>,
    indices: &'a [u32],
    front: usize,
    back: usize,
}

impl<'a> CpgEdges<'a> {
    fn new(graph: &'a DiGraph<CpgNode, CpgEdge>, indices: &'a [u32]) -> Self {
        Self {
            graph,
            indices,
            front: 0,
            back: indices.len(),
        }
    }

    /// Returns whether no edges remain in this view.
    pub fn is_empty(&self) -> bool {
        self.front == self.back
    }

    /// Returns an independent iterator over the remaining edges.
    pub fn iter(&self) -> Self {
        *self
    }

    fn edge_at(&self, relative: usize) -> &'a CpgEdge {
        let raw = self.indices[self.front + relative];
        self.graph
            .edge_weight(EdgeIndex::new(raw as usize))
            .expect("a projection-cache edge index resolves until topology mutation")
    }
}

impl fmt::Debug for CpgEdges<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.iter()).finish()
    }
}

impl<'a> Iterator for CpgEdges<'a> {
    type Item = &'a CpgEdge;

    fn next(&mut self) -> Option<Self::Item> {
        if self.front == self.back {
            return None;
        }
        let edge = self.edge_at(0);
        self.front += 1;
        Some(edge)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.back - self.front;
        (remaining, Some(remaining))
    }
}

impl DoubleEndedIterator for CpgEdges<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.front == self.back {
            return None;
        }
        self.back -= 1;
        let raw = self.indices[self.back];
        Some(
            self.graph
                .edge_weight(EdgeIndex::new(raw as usize))
                .expect("a projection-cache edge index resolves until topology mutation"),
        )
    }
}

impl ExactSizeIterator for CpgEdges<'_> {}

impl Index<usize> for CpgEdges<'_> {
    type Output = CpgEdge;

    fn index(&self, index: usize) -> &Self::Output {
        self.edge_at(index)
    }
}

/// Allocation-free CFG neighbors in deterministic target/edge-id order.
#[derive(Clone, Copy)]
pub struct CfgNeighbors<'a> {
    edges: CpgEdges<'a>,
    outgoing: bool,
}

impl<'a> CfgNeighbors<'a> {
    pub(crate) fn new(edges: CpgEdges<'a>, outgoing: bool) -> Self {
        Self { edges, outgoing }
    }

    /// Returns whether no CFG neighbors remain in this view.
    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    /// Returns an independent iterator over the remaining neighbors.
    pub fn iter(&self) -> Self {
        *self
    }

    /// Copies the deterministic view into an owned vector.
    pub fn to_vec(&self) -> Vec<(NodeId, CfgEdgeKind)> {
        self.iter().collect()
    }
}

impl fmt::Debug for CfgNeighbors<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.iter()).finish()
    }
}

impl<'a> Iterator for CfgNeighbors<'a> {
    type Item = (NodeId, CfgEdgeKind);

    fn next(&mut self) -> Option<Self::Item> {
        let edge = self.edges.next()?;
        let CpgEdgeKind::ControlFlow(kind) = edge.kind else {
            unreachable!("the CFG cache row contains only control-flow edges")
        };
        Some((
            if self.outgoing {
                edge.target
            } else {
                edge.source
            },
            kind,
        ))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.edges.size_hint()
    }
}

impl DoubleEndedIterator for CfgNeighbors<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let edge = self.edges.next_back()?;
        let CpgEdgeKind::ControlFlow(kind) = edge.kind else {
            unreachable!("the CFG cache row contains only control-flow edges")
        };
        Some((
            if self.outgoing {
                edge.target
            } else {
                edge.source
            },
            kind,
        ))
    }
}

impl ExactSizeIterator for CfgNeighbors<'_> {}

impl PartialEq<Vec<(NodeId, CfgEdgeKind)>> for CfgNeighbors<'_> {
    fn eq(&self, other: &Vec<(NodeId, CfgEdgeKind)>) -> bool {
        self.iter().eq(other.iter().copied())
    }
}

/// Allocation-free DFG neighbors in deterministic target/edge-id order.
#[derive(Clone, Copy)]
pub struct DfgNeighbors<'a> {
    edges: CpgEdges<'a>,
    outgoing: bool,
}

impl<'a> DfgNeighbors<'a> {
    pub(crate) fn new(edges: CpgEdges<'a>, outgoing: bool) -> Self {
        Self { edges, outgoing }
    }

    /// Returns whether no DFG neighbors remain in this view.
    pub fn is_empty(&self) -> bool {
        self.edges.is_empty()
    }

    /// Returns an independent iterator over the remaining neighbors.
    pub fn iter(&self) -> Self {
        *self
    }

    /// Copies the deterministic view into an owned vector.
    pub fn to_vec(&self) -> Vec<(NodeId, DfgEdgeKind)> {
        self.iter().collect()
    }
}

impl fmt::Debug for DfgNeighbors<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.iter()).finish()
    }
}

impl<'a> Iterator for DfgNeighbors<'a> {
    type Item = (NodeId, DfgEdgeKind);

    fn next(&mut self) -> Option<Self::Item> {
        let edge = self.edges.next()?;
        let CpgEdgeKind::DataFlow(kind) = edge.kind else {
            unreachable!("the DFG cache row contains only data-flow edges")
        };
        Some((
            if self.outgoing {
                edge.target
            } else {
                edge.source
            },
            kind,
        ))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.edges.size_hint()
    }
}

impl DoubleEndedIterator for DfgNeighbors<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let edge = self.edges.next_back()?;
        let CpgEdgeKind::DataFlow(kind) = edge.kind else {
            unreachable!("the DFG cache row contains only data-flow edges")
        };
        Some((
            if self.outgoing {
                edge.target
            } else {
                edge.source
            },
            kind,
        ))
    }
}

impl ExactSizeIterator for DfgNeighbors<'_> {}

impl PartialEq<Vec<(NodeId, DfgEdgeKind)>> for DfgNeighbors<'_> {
    fn eq(&self, other: &Vec<(NodeId, DfgEdgeKind)>) -> bool {
        self.iter().eq(other.iter().copied())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub(crate) enum EdgeLayer {
    Ast,
    Cfg,
    Dfg,
    Pdg,
    Heap,
    NameFlow,
    Call,
    Other,
}

#[derive(Debug)]
struct EdgeCsr {
    offsets: Vec<u32>,
    indices: Vec<u32>,
}

impl EdgeCsr {
    fn row(&self, node_index: usize) -> &[u32] {
        let start = self.offsets[node_index] as usize;
        let end = self.offsets[node_index + 1] as usize;
        &self.indices[start..end]
    }

    fn row_bounds(&self, node_index: usize) -> (usize, usize) {
        (
            self.offsets[node_index] as usize,
            self.offsets[node_index + 1] as usize,
        )
    }
}

/// Heap-owned cache behind `CodePropertyGraph`'s `OnceLock`.
#[derive(Debug)]
pub(crate) struct CpgProjectionCache {
    node_indices: Vec<usize>,
    dense_node_metadata: OnceLock<Box<DenseNodeMetadata>>,
    forward: EdgeCsr,
    reverse: EdgeCsr,
    by_target: EdgeCsr,
    by_target_nodes: Vec<NodeId>,
}

impl CpgProjectionCache {
    pub(crate) fn build(graph: &DiGraph<CpgNode, CpgEdge>) -> Self {
        let mut ordered_nodes = Vec::with_capacity(graph.node_count());
        for index in graph.node_indices() {
            let node = graph
                .node_weight(index)
                .expect("a graph node index resolves while the cache is built");
            ordered_nodes.push((node.id, index.index()));
        }
        ordered_nodes.sort_unstable_by_key(|&(node, _)| node);
        let node_indices = ordered_nodes.into_iter().map(|(_, index)| index).collect();

        let mut raw_edges = Vec::with_capacity(graph.edge_count());
        for index in graph.edge_indices() {
            let (source, target) = graph
                .edge_endpoints(index)
                .expect("a graph edge index has endpoints while the cache is built");
            let edge = graph
                .edge_weight(index)
                .expect("a graph edge index resolves while the cache is built");
            raw_edges.push(RawEdge {
                source: u32::try_from(source.index())
                    .expect("CPG node count exceeds the u32 projection domain"),
                target: u32::try_from(target.index())
                    .expect("CPG node count exceeds the u32 projection domain"),
                source_id: edge.source,
                target_id: edge.target,
                layer: edge_layer(&edge.kind),
                edge_id: edge.id.as_u32(),
                edge_index: u32::try_from(index.index())
                    .expect("CPG edge count exceeds the u32 projection domain"),
            });
        }

        let forward = adjacency_csr(graph.node_count(), &raw_edges, CsrOrder::Forward);
        let reverse = adjacency_csr(graph.node_count(), &raw_edges, CsrOrder::Reverse);
        let by_target = adjacency_csr(graph.node_count(), &raw_edges, CsrOrder::ByTarget);
        let by_target_nodes = by_target
            .indices
            .iter()
            .map(|&raw| edge_target(graph, raw))
            .collect();

        Self {
            node_indices,
            dense_node_metadata: OnceLock::new(),
            forward,
            reverse,
            by_target,
            by_target_nodes,
        }
    }

    pub(crate) fn dense_nodes(&self, graph: &DiGraph<CpgNode, CpgEdge>) -> DenseNodeView<'_> {
        let metadata = self
            .dense_node_metadata
            .get_or_init(|| Box::new(self.build_dense_node_metadata(graph)));
        DenseNodeView {
            ids: &metadata.ids,
            unique_out_degree: &metadata.outgoing,
            unique_in_degree: &metadata.incoming,
        }
    }

    fn build_dense_node_metadata(&self, graph: &DiGraph<CpgNode, CpgEdge>) -> DenseNodeMetadata {
        let ids: Vec<_> = self
            .node_indices
            .iter()
            .map(|&index| {
                graph
                    .node_weight(NodeIndex::new(index))
                    .expect("a projection-cache node index resolves until mutation")
                    .id
            })
            .collect();
        let raw_node_bound = self
            .node_indices
            .iter()
            .copied()
            .max()
            .map_or(0, |index| index + 1);
        let mut dense_by_raw_node = vec![u32::MAX; raw_node_bound];
        for (dense_index, &raw_index) in self.node_indices.iter().enumerate() {
            dense_by_raw_node[raw_index] =
                u32::try_from(dense_index).expect("a dense node index fits u32");
        }
        let mut outgoing = vec![0u32; ids.len()];
        let mut incoming = vec![0u32; ids.len()];
        for (dense_source, &graph_source) in self.node_indices.iter().enumerate() {
            let (start, end) = self.by_target.row_bounds(graph_source);
            let mut previous = None;
            for offset in start..end {
                let target = self.by_target_nodes[offset];
                if previous == Some(target) {
                    continue;
                }
                outgoing[dense_source] = outgoing[dense_source]
                    .checked_add(1)
                    .expect("CPG unique out-degree exceeds the u32 projection domain");
                let edge = EdgeIndex::new(self.by_target.indices[offset] as usize);
                let (_, raw_target) = graph
                    .edge_endpoints(edge)
                    .expect("a cached edge index resolves until mutation");
                let dense_target = dense_by_raw_node[raw_target.index()];
                assert_ne!(
                    dense_target,
                    u32::MAX,
                    "a cached edge target has dense node metadata"
                );
                let dense_target = dense_target as usize;
                incoming[dense_target] = incoming[dense_target]
                    .checked_add(1)
                    .expect("CPG unique in-degree exceeds the u32 projection domain");
                previous = Some(target);
            }
        }
        DenseNodeMetadata {
            ids,
            outgoing,
            incoming,
        }
    }

    pub(crate) fn outgoing<'a>(
        &'a self,
        graph: &'a DiGraph<CpgNode, CpgEdge>,
        node: Option<NodeIndex>,
    ) -> CpgEdges<'a> {
        CpgEdges::new(graph, self.row(&self.forward, node))
    }

    pub(crate) fn incoming<'a>(
        &'a self,
        graph: &'a DiGraph<CpgNode, CpgEdge>,
        node: Option<NodeIndex>,
    ) -> CpgEdges<'a> {
        CpgEdges::new(graph, self.row(&self.reverse, node))
    }

    pub(crate) fn outgoing_layer<'a>(
        &'a self,
        graph: &'a DiGraph<CpgNode, CpgEdge>,
        node: Option<NodeIndex>,
        layer: EdgeLayer,
    ) -> CpgEdges<'a> {
        let row = self.row(&self.forward, node);
        CpgEdges::new(graph, layer_slice(graph, row, layer))
    }

    pub(crate) fn incoming_layer<'a>(
        &'a self,
        graph: &'a DiGraph<CpgNode, CpgEdge>,
        node: Option<NodeIndex>,
        layer: EdgeLayer,
    ) -> CpgEdges<'a> {
        let row = self.row(&self.reverse, node);
        CpgEdges::new(graph, layer_slice(graph, row, layer))
    }

    pub(crate) fn layer<'a>(
        &'a self,
        graph: &'a DiGraph<CpgNode, CpgEdge>,
        layer: EdgeLayer,
    ) -> impl Iterator<Item = &'a CpgEdge> + 'a {
        self.node_indices
            .iter()
            .flat_map(move |&node| self.outgoing_layer_index(graph, node, layer))
    }

    pub(crate) fn between<'a>(
        &'a self,
        graph: &'a DiGraph<CpgNode, CpgEdge>,
        source: Option<NodeIndex>,
        target: NodeId,
    ) -> CpgEdges<'a> {
        let Some(source) = source else {
            return CpgEdges::new(graph, &[]);
        };
        let source = source.index();
        let (row_start, row_end) = self.by_target.row_bounds(source);
        let targets = &self.by_target_nodes[row_start..row_end];
        let start = targets.partition_point(|&node| node < target);
        let end = targets.partition_point(|&node| node <= target);
        CpgEdges::new(
            graph,
            &self.by_target.indices[row_start + start..row_start + end],
        )
    }

    fn row<'a>(&'a self, csr: &'a EdgeCsr, node: Option<NodeIndex>) -> &'a [u32] {
        node.map_or(&[], |index| csr.row(index.index()))
    }

    fn outgoing_layer_index<'a>(
        &'a self,
        graph: &'a DiGraph<CpgNode, CpgEdge>,
        node_index: usize,
        layer: EdgeLayer,
    ) -> CpgEdges<'a> {
        CpgEdges::new(
            graph,
            layer_slice(graph, self.forward.row(node_index), layer),
        )
    }
}

/// Deterministic dense node metadata aligned by ascending [`NodeId`].
#[derive(Debug, Clone, Copy)]
pub(crate) struct DenseNodeView<'a> {
    pub(crate) ids: &'a [NodeId],
    pub(crate) unique_out_degree: &'a [u32],
    pub(crate) unique_in_degree: &'a [u32],
}

#[derive(Debug)]
struct DenseNodeMetadata {
    ids: Vec<NodeId>,
    outgoing: Vec<u32>,
    incoming: Vec<u32>,
}

#[derive(Debug, Clone, Copy)]
struct RawEdge {
    source: u32,
    target: u32,
    source_id: NodeId,
    target_id: NodeId,
    layer: EdgeLayer,
    edge_id: u32,
    edge_index: u32,
}

#[derive(Clone, Copy)]
enum CsrOrder {
    Forward,
    Reverse,
    ByTarget,
}

fn adjacency_csr(node_count: usize, edges: &[RawEdge], order: CsrOrder) -> EdgeCsr {
    let owner = |edge: &RawEdge| match order {
        CsrOrder::Forward | CsrOrder::ByTarget => edge.source,
        CsrOrder::Reverse => edge.target,
    };

    let mut offsets = vec![0u32; node_count + 1];
    for edge in edges {
        offsets[owner(edge) as usize + 1] += 1;
    }
    for node in 1..offsets.len() {
        offsets[node] = offsets[node]
            .checked_add(offsets[node - 1])
            .expect("CPG edge count exceeds the u32 projection domain");
    }

    let mut next = offsets[..node_count].to_vec();
    let mut indices = vec![0u32; edges.len()];
    for (position, edge) in edges.iter().enumerate() {
        let owner = owner(edge) as usize;
        let slot = next[owner] as usize;
        indices[slot] =
            u32::try_from(position).expect("CPG edge count exceeds the u32 projection domain");
        next[owner] += 1;
    }

    for node in 0..node_count {
        let start = offsets[node] as usize;
        let end = offsets[node + 1] as usize;
        if end - start < 2 {
            continue;
        }
        indices[start..end].sort_unstable_by(|&left, &right| {
            let left = &edges[left as usize];
            let right = &edges[right as usize];
            match order {
                CsrOrder::Forward => (left.layer, left.target_id, left.edge_id, left.edge_index)
                    .cmp(&(
                        right.layer,
                        right.target_id,
                        right.edge_id,
                        right.edge_index,
                    )),
                CsrOrder::Reverse => (left.layer, left.source_id, left.edge_id, left.edge_index)
                    .cmp(&(
                        right.layer,
                        right.source_id,
                        right.edge_id,
                        right.edge_index,
                    )),
                CsrOrder::ByTarget => (left.target_id, left.layer, left.edge_id, left.edge_index)
                    .cmp(&(
                        right.target_id,
                        right.layer,
                        right.edge_id,
                        right.edge_index,
                    )),
            }
        });
    }
    for position in &mut indices {
        *position = edges[*position as usize].edge_index;
    }

    EdgeCsr { offsets, indices }
}

fn layer_slice<'a>(
    graph: &DiGraph<CpgNode, CpgEdge>,
    row: &'a [u32],
    layer: EdgeLayer,
) -> &'a [u32] {
    let start = row.partition_point(|&raw| edge_layer(&edge(graph, raw).kind) < layer);
    let end = row.partition_point(|&raw| edge_layer(&edge(graph, raw).kind) <= layer);
    &row[start..end]
}

fn edge_target(graph: &DiGraph<CpgNode, CpgEdge>, raw: u32) -> NodeId {
    edge(graph, raw).target
}

fn edge(graph: &DiGraph<CpgNode, CpgEdge>, raw: u32) -> &CpgEdge {
    graph
        .edge_weight(EdgeIndex::new(raw as usize))
        .expect("a projection-cache edge index resolves until topology mutation")
}

fn edge_layer(kind: &CpgEdgeKind) -> EdgeLayer {
    match kind {
        CpgEdgeKind::AstChild
        | CpgEdgeKind::AstParent
        | CpgEdgeKind::AstNextSibling
        | CpgEdgeKind::AstPrevSibling => EdgeLayer::Ast,
        CpgEdgeKind::ControlFlow(_) => EdgeLayer::Cfg,
        CpgEdgeKind::DataFlow(_) => EdgeLayer::Dfg,
        CpgEdgeKind::ControlDependence | CpgEdgeKind::DataDependence => EdgeLayer::Pdg,
        CpgEdgeKind::Heap(_) => EdgeLayer::Heap,
        CpgEdgeKind::NameFlow(_) => EdgeLayer::NameFlow,
        CpgEdgeKind::StaticCall | CpgEdgeKind::DynamicCall | CpgEdgeKind::CallSite => {
            EdgeLayer::Call
        }
        _ => EdgeLayer::Other,
    }
}

#[cfg(test)]
mod tests {
    use std::mem::size_of;

    use super::*;

    #[test]
    fn cached_edge_views_are_pointer_sized_metadata_only() {
        assert!(size_of::<CpgEdges<'_>>() <= 5 * size_of::<usize>());
        assert!(size_of::<CfgNeighbors<'_>>() <= 6 * size_of::<usize>());
        assert!(size_of::<DfgNeighbors<'_>>() <= 6 * size_of::<usize>());
    }
}
