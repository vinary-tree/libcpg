//! Embedding types for GNN outputs.
//!
//! This module defines wrapper types for node and subgraph embeddings
//! produced by GNN message passing.

use crate::NodeId;

#[cfg(feature = "gnn")]
use ndarray::Array1;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Embedding for a single node.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NodeEmbedding {
    /// The node this embedding represents.
    pub node_id: NodeId,
    /// The embedding vector (skipped during serialization, recomputed at load).
    #[cfg(feature = "gnn")]
    #[cfg_attr(feature = "serde", serde(skip))]
    pub vector: Array1<f32>,
    /// Embedding dimensionality.
    pub dim: usize,
}

impl NodeEmbedding {
    /// Creates a new node embedding.
    #[cfg(feature = "gnn")]
    pub fn new(node_id: NodeId, vector: Array1<f32>) -> Self {
        let dim = vector.len();
        Self { node_id, vector, dim }
    }

    /// Returns the L2 norm of the embedding.
    #[cfg(feature = "gnn")]
    pub fn norm(&self) -> f32 {
        self.vector.iter().map(|x| x * x).sum::<f32>().sqrt()
    }

    /// Computes cosine similarity with another embedding.
    #[cfg(feature = "gnn")]
    pub fn cosine_similarity(&self, other: &NodeEmbedding) -> f32 {
        if self.dim != other.dim {
            return 0.0;
        }

        let dot: f32 = self.vector.iter()
            .zip(other.vector.iter())
            .map(|(a, b)| a * b)
            .sum();

        let norm_self = self.norm();
        let norm_other = other.norm();

        if norm_self == 0.0 || norm_other == 0.0 {
            0.0
        } else {
            dot / (norm_self * norm_other)
        }
    }
}

/// Embedding for a subgraph (e.g., function, class).
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SubgraphEmbedding {
    /// The nodes in this subgraph.
    pub node_ids: Vec<NodeId>,
    /// The aggregated embedding vector (skipped during serialization, recomputed at load).
    #[cfg(feature = "gnn")]
    #[cfg_attr(feature = "serde", serde(skip))]
    pub vector: Array1<f32>,
    /// Embedding dimensionality.
    pub dim: usize,
    /// Aggregation method used.
    pub aggregation: AggregationMethod,
}

impl SubgraphEmbedding {
    /// Creates a new subgraph embedding.
    #[cfg(feature = "gnn")]
    pub fn new(node_ids: Vec<NodeId>, vector: Array1<f32>, aggregation: AggregationMethod) -> Self {
        let dim = vector.len();
        Self { node_ids, vector, dim, aggregation }
    }

    /// Returns the L2 norm of the embedding.
    #[cfg(feature = "gnn")]
    pub fn norm(&self) -> f32 {
        self.vector.iter().map(|x| x * x).sum::<f32>().sqrt()
    }

    /// Computes cosine similarity with another subgraph embedding.
    #[cfg(feature = "gnn")]
    pub fn cosine_similarity(&self, other: &SubgraphEmbedding) -> f32 {
        if self.dim != other.dim {
            return 0.0;
        }

        let dot: f32 = self.vector.iter()
            .zip(other.vector.iter())
            .map(|(a, b)| a * b)
            .sum();

        let norm_self = self.norm();
        let norm_other = other.norm();

        if norm_self == 0.0 || norm_other == 0.0 {
            0.0
        } else {
            dot / (norm_self * norm_other)
        }
    }

    /// Returns the number of nodes in the subgraph.
    pub fn node_count(&self) -> usize {
        self.node_ids.len()
    }
}

/// Method used to aggregate node embeddings into subgraph embedding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum AggregationMethod {
    /// Mean of all node embeddings.
    #[default]
    Mean,
    /// Sum of all node embeddings.
    Sum,
    /// Element-wise maximum.
    Max,
    /// Attention-weighted average.
    Attention,
    /// Hierarchical aggregation following AST structure.
    Hierarchical,
}

#[cfg(all(test, feature = "gnn"))]
mod tests {
    use super::*;
    use ndarray::array;

    #[test]
    fn test_node_embedding() {
        let emb1 = NodeEmbedding::new(NodeId::new(1), array![1.0, 0.0, 0.0]);
        let emb2 = NodeEmbedding::new(NodeId::new(2), array![0.0, 1.0, 0.0]);

        assert_eq!(emb1.dim, 3);
        assert!((emb1.norm() - 1.0).abs() < 0.001);
        assert!((emb1.cosine_similarity(&emb2)).abs() < 0.001);
    }

    #[test]
    fn test_identical_embeddings() {
        let emb1 = NodeEmbedding::new(NodeId::new(1), array![0.5, 0.5, 0.5]);
        let emb2 = NodeEmbedding::new(NodeId::new(2), array![0.5, 0.5, 0.5]);

        let sim = emb1.cosine_similarity(&emb2);
        assert!((sim - 1.0).abs() < 0.001);
    }
}
