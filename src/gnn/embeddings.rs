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

    // --- NodeEmbedding edge cases -------------------------------------------

    #[test]
    fn test_node_embedding_dim_mismatch_is_zero() {
        // `dim` is derived from the vector length at construction, so unequal
        // lengths take the early-return `0.0` branch in `cosine_similarity`.
        let a = NodeEmbedding::new(NodeId::new(1), array![1.0, 2.0]);
        let b = NodeEmbedding::new(NodeId::new(2), array![1.0, 2.0, 3.0]);
        assert_eq!(a.dim, 2);
        assert_eq!(b.dim, 3);
        assert_eq!(a.cosine_similarity(&b), 0.0);
        assert_eq!(b.cosine_similarity(&a), 0.0);
    }

    #[test]
    fn test_node_embedding_zero_norm_is_zero() {
        let zero = NodeEmbedding::new(NodeId::new(1), array![0.0, 0.0, 0.0]);
        let nonzero = NodeEmbedding::new(NodeId::new(2), array![1.0, 2.0, 3.0]);
        assert_eq!(zero.norm(), 0.0);
        // A zero-norm operand (on either side) yields cosine 0.0, never NaN.
        assert_eq!(zero.cosine_similarity(&nonzero), 0.0);
        assert_eq!(nonzero.cosine_similarity(&zero), 0.0);
    }

    // --- SubgraphEmbedding: new / norm / cosine_similarity / node_count -----

    #[test]
    fn test_subgraph_embedding_new_norm_and_node_count() {
        let ids = vec![NodeId::new(3), NodeId::new(4)];
        let emb = SubgraphEmbedding::new(ids.clone(), array![3.0, 4.0], AggregationMethod::Mean);
        assert_eq!(emb.dim, 2);
        assert_eq!(emb.node_ids, ids);
        assert_eq!(emb.node_count(), 2);
        assert_eq!(emb.aggregation, AggregationMethod::Mean);
        // 3-4-5 right triangle: L2 norm is exactly 5.
        assert!((emb.norm() - 5.0).abs() < 1e-4);
    }

    #[test]
    fn test_subgraph_cosine_orthogonal_and_identical() {
        let a = SubgraphEmbedding::new(vec![], array![1.0, 0.0], AggregationMethod::Sum);
        let b = SubgraphEmbedding::new(vec![], array![0.0, 1.0], AggregationMethod::Sum);
        // Orthogonal ⇒ 0.
        assert!(a.cosine_similarity(&b).abs() < 1e-6);
        // Identical non-zero ⇒ 1.
        let c = SubgraphEmbedding::new(vec![], array![1.0, 0.0], AggregationMethod::Max);
        assert!((a.cosine_similarity(&c) - 1.0).abs() < 1e-6);
        assert_eq!(a.node_count(), 0);
    }

    #[test]
    fn test_subgraph_cosine_dim_mismatch_is_zero() {
        let a = SubgraphEmbedding::new(vec![], array![1.0, 2.0, 3.0], AggregationMethod::Mean);
        let b = SubgraphEmbedding::new(vec![], array![1.0, 2.0], AggregationMethod::Mean);
        assert_eq!(a.cosine_similarity(&b), 0.0);
        assert_eq!(b.cosine_similarity(&a), 0.0);
    }

    #[test]
    fn test_subgraph_cosine_zero_norm_is_zero() {
        let zero = SubgraphEmbedding::new(vec![], array![0.0, 0.0], AggregationMethod::Mean);
        let nonzero = SubgraphEmbedding::new(vec![], array![1.0, 1.0], AggregationMethod::Mean);
        assert_eq!(zero.norm(), 0.0);
        assert_eq!(zero.cosine_similarity(&nonzero), 0.0);
        assert_eq!(nonzero.cosine_similarity(&zero), 0.0);
    }

    // --- AggregationMethod: Default + variants ------------------------------

    #[test]
    fn test_aggregation_method_default_and_variants() {
        // `#[default]` is `Mean`.
        assert_eq!(AggregationMethod::default(), AggregationMethod::Mean);

        let variants = [
            AggregationMethod::Mean,
            AggregationMethod::Sum,
            AggregationMethod::Max,
            AggregationMethod::Attention,
            AggregationMethod::Hierarchical,
        ];
        // Reflexivity of the derived `PartialEq`/`Eq`.
        for (i, v) in variants.iter().enumerate() {
            assert_eq!(*v, variants[i]);
        }
        // Distinct variants are unequal.
        assert_ne!(AggregationMethod::Mean, AggregationMethod::Sum);
        assert_ne!(AggregationMethod::Attention, AggregationMethod::Hierarchical);
        // `Copy` semantics: a bitwise copy compares equal to its source.
        let m = AggregationMethod::Max;
        let copied = m;
        assert_eq!(m, copied);
    }
}

#[cfg(all(test, feature = "gnn"))]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    /// A finite, overflow-free vector of the given length (elements in
    /// `[-100, 100]`, so sums of squares stay well below `f32::MAX`).
    fn arb_bounded_vec(len: usize) -> impl Strategy<Value = Vec<f32>> {
        prop::collection::vec(-100.0f32..100.0f32, len)
    }

    /// Two vectors of the SAME (1..=7) length.
    fn arb_same_len_pair() -> impl Strategy<Value = (Vec<f32>, Vec<f32>)> {
        (1usize..8).prop_flat_map(|len| (arb_bounded_vec(len), arb_bounded_vec(len)))
    }

    /// A vector guaranteed to have non-zero L2 norm (≥ 1 in some component).
    fn arb_nonzero_vec() -> impl Strategy<Value = Vec<f32>> {
        prop::collection::vec(-100.0f32..100.0f32, 1..8)
            .prop_filter("non-zero norm", |v| v.iter().any(|x| x.abs() >= 1.0))
    }

    /// Two vectors of DIFFERENT lengths (so `dim` differs).
    fn arb_diff_len_pair() -> impl Strategy<Value = (Vec<f32>, Vec<f32>)> {
        (1usize..6, 1usize..6).prop_flat_map(|(a, b)| {
            let lb = if a == b { a + 1 } else { b };
            (arb_bounded_vec(a), arb_bounded_vec(lb))
        })
    }

    /// A length plus a same-length companion vector.
    fn arb_len_and_vec() -> impl Strategy<Value = (usize, Vec<f32>)> {
        (1usize..8).prop_flat_map(|len| (Just(len), arb_bounded_vec(len)))
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        // ---- NodeEmbedding cosine algebra ----

        #[test]
        fn prop_node_cosine_bounded_symmetric_and_norm_nonneg((a, b) in arb_same_len_pair()) {
            let ea = NodeEmbedding::new(NodeId::new(1), Array1::from_vec(a));
            let eb = NodeEmbedding::new(NodeId::new(2), Array1::from_vec(b));
            let s1 = ea.cosine_similarity(&eb);
            let s2 = eb.cosine_similarity(&ea);
            // Cauchy-Schwarz bounds the ratio to [-1, 1]; allow f32 rounding slack.
            prop_assert!(s1 >= -1.0 - 1e-3, "cosine out of range: {s1}");
            prop_assert!(s1 <= 1.0 + 1e-3, "cosine out of range: {s1}");
            // Symmetry.
            prop_assert!((s1 - s2).abs() <= 1e-4, "asymmetric: {s1} vs {s2}");
            // L2 norm is non-negative.
            prop_assert!(ea.norm() >= 0.0);
            prop_assert!(eb.norm() >= 0.0);
        }

        #[test]
        fn prop_node_cosine_identical_is_one(v in arb_nonzero_vec()) {
            let ea = NodeEmbedding::new(NodeId::new(1), Array1::from_vec(v.clone()));
            let eb = NodeEmbedding::new(NodeId::new(2), Array1::from_vec(v));
            let s = ea.cosine_similarity(&eb);
            prop_assert!((s - 1.0).abs() <= 1e-3, "self-cosine != 1: {s}");
        }

        #[test]
        fn prop_node_cosine_dim_mismatch_is_zero((a, b) in arb_diff_len_pair()) {
            let ea = NodeEmbedding::new(NodeId::new(1), Array1::from_vec(a));
            let eb = NodeEmbedding::new(NodeId::new(2), Array1::from_vec(b));
            prop_assert_eq!(ea.cosine_similarity(&eb), 0.0);
            prop_assert_eq!(eb.cosine_similarity(&ea), 0.0);
        }

        #[test]
        fn prop_node_cosine_zero_vector_is_zero((len, other) in arb_len_and_vec()) {
            let z = NodeEmbedding::new(NodeId::new(1), Array1::zeros(len));
            let o = NodeEmbedding::new(NodeId::new(2), Array1::from_vec(other));
            prop_assert_eq!(z.norm(), 0.0);
            prop_assert_eq!(z.cosine_similarity(&o), 0.0);
            prop_assert_eq!(o.cosine_similarity(&z), 0.0);
        }

        // ---- SubgraphEmbedding cosine algebra (same invariants) ----

        #[test]
        fn prop_subgraph_cosine_bounded_symmetric_and_norm_nonneg((a, b) in arb_same_len_pair()) {
            let ea = SubgraphEmbedding::new(vec![], Array1::from_vec(a), AggregationMethod::Mean);
            let eb = SubgraphEmbedding::new(vec![], Array1::from_vec(b), AggregationMethod::Mean);
            let s1 = ea.cosine_similarity(&eb);
            let s2 = eb.cosine_similarity(&ea);
            prop_assert!(s1 >= -1.0 - 1e-3, "cosine out of range: {s1}");
            prop_assert!(s1 <= 1.0 + 1e-3, "cosine out of range: {s1}");
            prop_assert!((s1 - s2).abs() <= 1e-4, "asymmetric: {s1} vs {s2}");
            prop_assert!(ea.norm() >= 0.0);
            prop_assert!(eb.norm() >= 0.0);
        }

        #[test]
        fn prop_subgraph_cosine_identical_is_one(v in arb_nonzero_vec()) {
            let ea = SubgraphEmbedding::new(vec![], Array1::from_vec(v.clone()), AggregationMethod::Sum);
            let eb = SubgraphEmbedding::new(vec![], Array1::from_vec(v), AggregationMethod::Sum);
            let s = ea.cosine_similarity(&eb);
            prop_assert!((s - 1.0).abs() <= 1e-3, "self-cosine != 1: {s}");
        }

        #[test]
        fn prop_subgraph_cosine_dim_mismatch_is_zero((a, b) in arb_diff_len_pair()) {
            let ea = SubgraphEmbedding::new(vec![], Array1::from_vec(a), AggregationMethod::Max);
            let eb = SubgraphEmbedding::new(vec![], Array1::from_vec(b), AggregationMethod::Max);
            prop_assert_eq!(ea.cosine_similarity(&eb), 0.0);
            prop_assert_eq!(eb.cosine_similarity(&ea), 0.0);
        }

        #[test]
        fn prop_subgraph_cosine_zero_vector_is_zero((len, other) in arb_len_and_vec()) {
            let z = SubgraphEmbedding::new(vec![], Array1::zeros(len), AggregationMethod::Mean);
            let o = SubgraphEmbedding::new(vec![], Array1::from_vec(other), AggregationMethod::Mean);
            prop_assert_eq!(z.norm(), 0.0);
            prop_assert_eq!(z.cosine_similarity(&o), 0.0);
            prop_assert_eq!(o.cosine_similarity(&z), 0.0);
        }
    }
}
