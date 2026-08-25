//! Canonical heterogeneous-graph tensors over typed CPG relations.

use std::{
    cmp::Reverse,
    collections::{BTreeMap, BinaryHeap},
};

use crate::{CodePropertyGraph, CpgEdgeKind, NodeId, SourceRange};

use super::export::{
    edge_kind_class, node_kind_class, semantic_token, EDGE_KIND_CLASS_COUNT, EDGE_KIND_CLASS_NAMES,
    NODE_KIND_CLASS_COUNT,
};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Width of the deterministic signed feature-hash token embedding.
pub const HETERO_TOKEN_EMBEDDING_DIM: usize = 64;

/// Number of columns in every [`HeteroGnnExport::node_features`] row.
pub const HETERO_NODE_FEATURE_COUNT: usize = NODE_KIND_CLASS_COUNT + HETERO_TOKEN_EMBEDDING_DIM;

/// Dense edge coordinates for one exact [`CpgEdgeKind`] relation class.
///
/// Empty buckets are retained so consumers always observe the complete stable
/// relation schema. Parallel CPG edges remain parallel coordinates.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct HeteroRelationEdges {
    /// Stable relation class in `0..EDGE_KIND_CLASS_COUNT`.
    pub relation_class: u16,
    /// Stable human-readable relation name.
    pub relation_name: String,
    /// Canonically ordered `(source_dense, target_dense)` coordinates.
    pub edge_index: Vec<(u32, u32)>,
}

/// Feature-free heterogeneous-GNN input derived from one CPG.
///
/// `node_ids[d]`, `source_ranges[d]`, `node_types[d]`, `tokens[d]`, and
/// `node_features[d]` describe the same node. The first 45 feature columns are
/// an exact node-kind one-hot vector; the final 64 columns are a deterministic
/// signed feature hash of UTF-8 token byte n-grams of lengths one through
/// three. No learned weights or machine-learning runtime are required.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct HeteroGnnExport {
    /// Stable CPG identifiers in ascending dense order.
    pub node_ids: Vec<NodeId>,
    /// Source ranges aligned one-for-one with [`Self::node_ids`].
    pub source_ranges: Vec<SourceRange>,
    /// Stable [`crate::CpgNodeKind`] class for every dense node.
    pub node_types: Vec<u16>,
    /// Exact normalized token used to initialize every dense node.
    pub tokens: Vec<String>,
    /// Node-kind one-hot vectors followed by token feature hashes.
    pub node_features: Vec<Vec<f32>>,
    /// All stable typed relation buckets, including empty buckets.
    pub relations: Vec<HeteroRelationEdges>,
}

impl HeteroGnnExport {
    /// Returns the dense index of a stable node identifier.
    pub fn dense_index(&self, node: NodeId) -> Option<u32> {
        self.node_ids
            .binary_search(&node)
            .ok()
            .map(|index| index as u32)
    }

    /// Returns the relation bucket for a stable relation class.
    pub fn relation_by_class(&self, relation_class: u16) -> Option<&HeteroRelationEdges> {
        self.relations
            .get(usize::from(relation_class))
            .filter(|relation| relation.relation_class == relation_class)
    }

    /// Returns the relation bucket for an exact CPG edge kind.
    pub fn relation(&self, kind: &CpgEdgeKind) -> &HeteroRelationEdges {
        self.relation_by_class(edge_kind_class(kind))
            .expect("every CPG edge kind has a stable heterogeneous relation")
    }
}

/// Exports a CPG as canonical typed tensors for a heterogeneous GNN.
///
/// This is an iterative, heap-backed transformation. Node and edge insertion
/// order cannot affect the returned dense identifiers, features, or relation
/// coordinates.
pub fn export_heterogeneous_gnn(cpg: &CodePropertyGraph) -> HeteroGnnExport {
    let mut node_heap = BinaryHeap::with_capacity(cpg.node_count());
    node_heap.extend(cpg.node_ids().map(Reverse));
    let mut node_ids = Vec::with_capacity(node_heap.len());
    while let Some(Reverse(node)) = node_heap.pop() {
        node_ids.push(node);
    }

    let dense: BTreeMap<_, _> = node_ids
        .iter()
        .copied()
        .enumerate()
        .map(|(index, node)| (node, index as u32))
        .collect();
    let mut relations: Vec<_> = EDGE_KIND_CLASS_NAMES
        .iter()
        .enumerate()
        .map(|(relation_class, relation_name)| HeteroRelationEdges {
            relation_class: relation_class as u16,
            relation_name: (*relation_name).to_owned(),
            edge_index: Vec::new(),
        })
        .collect();

    let mut edge_heap = BinaryHeap::with_capacity(cpg.edge_count());
    for edge in cpg.edges() {
        let (Some(&source), Some(&target)) = (dense.get(&edge.source), dense.get(&edge.target))
        else {
            continue;
        };
        edge_heap.push(Reverse((edge_kind_class(&edge.kind), source, target)));
    }
    while let Some(Reverse((relation_class, source, target))) = edge_heap.pop() {
        relations[usize::from(relation_class)]
            .edge_index
            .push((source, target));
    }

    debug_assert_eq!(relations.len(), usize::from(EDGE_KIND_CLASS_COUNT));
    let mut source_ranges = Vec::with_capacity(node_ids.len());
    let mut node_types = Vec::with_capacity(node_ids.len());
    let mut tokens = Vec::with_capacity(node_ids.len());
    let mut node_features = Vec::with_capacity(node_ids.len());
    for &node_id in &node_ids {
        let node = cpg
            .node(node_id)
            .expect("node_ids are enumerated from existing CPG nodes");
        let node_type = node_kind_class(&node.kind);
        let token = semantic_token(node);
        let mut features = vec![0.0; HETERO_NODE_FEATURE_COUNT];
        features[usize::from(node_type)] = 1.0;
        embed_token(&token, &mut features[NODE_KIND_CLASS_COUNT..]);

        source_ranges.push(node.range);
        node_types.push(node_type);
        tokens.push(token);
        node_features.push(features);
    }

    HeteroGnnExport {
        node_ids,
        source_ranges,
        node_types,
        tokens,
        node_features,
        relations,
    }
}

fn embed_token(token: &str, output: &mut [f32]) {
    debug_assert_eq!(output.len(), HETERO_TOKEN_EMBEDDING_DIM);
    let bytes = token.as_bytes();
    for width in 1..=3 {
        if width > bytes.len() {
            break;
        }
        for gram in bytes.windows(width) {
            let mut hash = 0xcbf2_9ce4_8422_2325_u64;
            hash ^= width as u64;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            for &byte in gram {
                hash ^= u64::from(byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
            let bucket = (hash as usize) & (HETERO_TOKEN_EMBEDDING_DIM - 1);
            let contribution = if hash & (1 << 63) == 0 { 1.0 } else { -1.0 };
            output[bucket] += contribution;
        }
    }
}
