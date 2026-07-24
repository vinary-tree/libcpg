//! Graph similarity metrics.
//!
//! This module provides various metrics for measuring similarity
//! between Code Property Graphs.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::{CodePropertyGraph, NodeId};
use super::NodeKindTag;

/// Graph similarity calculator.
#[derive(Debug, Clone, Default)]
pub struct GraphSimilarity {
    /// Metric to use for comparison.
    metric: SimilarityMetric,
    /// Weight for structural similarity.
    structural_weight: f64,
    /// Weight for label similarity.
    label_weight: f64,
}

impl GraphSimilarity {
    /// Creates a new similarity calculator with default settings.
    pub fn new() -> Self {
        Self {
            metric: SimilarityMetric::Jaccard,
            structural_weight: 0.7,
            label_weight: 0.3,
        }
    }

    /// Sets the similarity metric.
    pub fn with_metric(mut self, metric: SimilarityMetric) -> Self {
        self.metric = metric;
        self
    }

    /// Sets the structural weight.
    pub fn with_structural_weight(mut self, weight: f64) -> Self {
        self.structural_weight = weight;
        self
    }

    /// Sets the label weight.
    pub fn with_label_weight(mut self, weight: f64) -> Self {
        self.label_weight = weight;
        self
    }

    /// Computes the similarity between two graphs.
    pub fn similarity(&self, g1: &CodePropertyGraph, g2: &CodePropertyGraph) -> f64 {
        match self.metric {
            SimilarityMetric::Jaccard => self.jaccard_similarity(g1, g2),
            SimilarityMetric::Cosine => self.cosine_similarity(g1, g2),
            SimilarityMetric::WeisfeilerLehman => self.wl_similarity(g1, g2),
            SimilarityMetric::GraphEdit => self.graph_edit_similarity(g1, g2),
        }
    }

    /// Jaccard similarity based on node and edge multisets.
    fn jaccard_similarity(&self, g1: &CodePropertyGraph, g2: &CodePropertyGraph) -> f64 {
        // Count node kinds
        let kinds1 = node_kind_multiset(g1);
        let kinds2 = node_kind_multiset(g2);

        // Jaccard index: |A ∩ B| / |A ∪ B|
        let intersection: usize = kinds1
            .iter()
            .map(|(k, &c1)| {
                let c2 = kinds2.get(k).copied().unwrap_or(0);
                c1.min(c2)
            })
            .sum();

        let union: usize = kinds1
            .iter()
            .map(|(k, &c1)| {
                let c2 = kinds2.get(k).copied().unwrap_or(0);
                c1.max(c2)
            })
            .sum::<usize>()
            + kinds2
                .iter()
                .filter(|(k, _)| !kinds1.contains_key(k))
                .map(|(_, &c)| c)
                .sum::<usize>();

        if union == 0 {
            1.0 // Both empty
        } else {
            intersection as f64 / union as f64
        }
    }

    /// Cosine similarity based on feature vectors.
    fn cosine_similarity(&self, g1: &CodePropertyGraph, g2: &CodePropertyGraph) -> f64 {
        let v1 = graph_feature_vector(g1);
        let v2 = graph_feature_vector(g2);

        let dot: f64 = v1.iter().zip(v2.iter()).map(|(a, b)| a * b).sum();
        let mag1: f64 = v1.iter().map(|x| x * x).sum::<f64>().sqrt();
        let mag2: f64 = v2.iter().map(|x| x * x).sum::<f64>().sqrt();

        if mag1 == 0.0 || mag2 == 0.0 {
            0.0
        } else {
            // In exact arithmetic `dot / (‖v₁‖‖v₂‖) ∈ [-1, 1]`, and `= 1`
            // exactly when the vectors are parallel — which is the case
            // whenever a graph is compared with itself. In floating point the
            // two square roots and the division each round, so the quotient can
            // land just outside the interval (observed: `1.0000000000000002`
            // for a self-comparison). Feature counts are non-negative, so the
            // true value is in `[0, 1]`; clamping restores the documented range
            // without changing any value that was already inside it.
            (dot / (mag1 * mag2)).clamp(0.0, 1.0)
        }
    }

    /// Weisfeiler-Lehman graph kernel similarity.
    fn wl_similarity(&self, g1: &CodePropertyGraph, g2: &CodePropertyGraph) -> f64 {
        const ITERATIONS: usize = 3;

        let labels1 = wl_labels(g1, ITERATIONS);
        let labels2 = wl_labels(g2, ITERATIONS);

        // Compare label distributions
        let all_labels: FxHashSet<_> = labels1.keys().chain(labels2.keys()).copied().collect();

        let mut dot = 0.0;
        let mut mag1 = 0.0;
        let mut mag2 = 0.0;

        for label in all_labels {
            let c1 = labels1.get(&label).copied().unwrap_or(0) as f64;
            let c2 = labels2.get(&label).copied().unwrap_or(0) as f64;
            dot += c1 * c2;
            mag1 += c1 * c1;
            mag2 += c2 * c2;
        }

        if mag1 == 0.0 || mag2 == 0.0 {
            0.0
        } else {
            // Same normalized-dot-product form as `cosine_similarity`, and so
            // the same floating-point overshoot at the `= 1` endpoint; clamped
            // for the same reason.
            (dot / (mag1.sqrt() * mag2.sqrt())).clamp(0.0, 1.0)
        }
    }

    /// Approximate graph edit distance similarity.
    fn graph_edit_similarity(&self, g1: &CodePropertyGraph, g2: &CodePropertyGraph) -> f64 {
        // Use a simple approximation based on node/edge differences
        let n1 = g1.node_count();
        let n2 = g2.node_count();
        let e1 = g1.edge_count();
        let e2 = g2.edge_count();

        let node_diff = (n1 as i64 - n2 as i64).unsigned_abs() as f64;
        let edge_diff = (e1 as i64 - e2 as i64).unsigned_abs() as f64;

        let max_nodes = n1.max(n2) as f64;
        let max_edges = e1.max(e2).max(1) as f64;

        // Combine structural similarity with label similarity
        let node_sim = 1.0 - (node_diff / max_nodes.max(1.0));
        let edge_sim = 1.0 - (edge_diff / max_edges);

        // Weight structural similarity more
        self.structural_weight * (0.6 * node_sim + 0.4 * edge_sim)
            + self.label_weight * self.jaccard_similarity(g1, g2)
    }
}

/// Similarity metric to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SimilarityMetric {
    /// Jaccard index based on node/edge multisets.
    #[default]
    Jaccard,
    /// Cosine similarity of feature vectors.
    Cosine,
    /// Weisfeiler-Lehman graph kernel.
    WeisfeilerLehman,
    /// Approximate graph edit distance.
    GraphEdit,
}

/// Creates a multiset of node kinds.
fn node_kind_multiset(graph: &CodePropertyGraph) -> FxHashMap<NodeKindTag, usize> {
    let mut counts = FxHashMap::default();
    for node in graph.nodes() {
        let tag = NodeKindTag::from_kind(&node.kind);
        *counts.entry(tag).or_insert(0) += 1;
    }
    counts
}

/// Creates a feature vector for a graph.
fn graph_feature_vector(graph: &CodePropertyGraph) -> Vec<f64> {
    // Feature vector contains:
    // - Node count (normalized)
    // - Edge count (normalized)
    // - AST edge ratio
    // - CFG edge ratio
    // - DFG edge ratio
    // - Function count
    // - Class count
    // - Average depth (estimated)
    // - Cyclomatic complexity

    let node_count = graph.node_count() as f64;
    let edge_count = graph.edge_count() as f64;

    let ast_edges = graph.edges().filter(|e| e.kind.is_ast()).count() as f64;
    let cfg_edges = graph.edges().filter(|e| e.kind.is_cfg()).count() as f64;
    let dfg_edges = graph.edges().filter(|e| e.kind.is_dfg()).count() as f64;

    let func_count = graph.functions().count() as f64;
    let class_count = graph.classes().count() as f64;

    let depth = graph.ast_depth() as f64;
    let complexity = graph.cyclomatic_complexity() as f64;

    // Normalize to [0, 1] range approximately
    vec![
        (node_count / 1000.0).min(1.0),
        (edge_count / 2000.0).min(1.0),
        if edge_count > 0.0 { ast_edges / edge_count } else { 0.0 },
        if edge_count > 0.0 { cfg_edges / edge_count } else { 0.0 },
        if edge_count > 0.0 { dfg_edges / edge_count } else { 0.0 },
        (func_count / 100.0).min(1.0),
        (class_count / 50.0).min(1.0),
        (depth / 50.0).min(1.0),
        (complexity / 100.0).min(1.0),
    ]
}

/// Computes Weisfeiler-Lehman labels for a graph.
fn wl_labels(graph: &CodePropertyGraph, iterations: usize) -> FxHashMap<u64, usize> {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Initialize labels with node kind hashes
    let mut labels: FxHashMap<NodeId, u64> = graph
        .nodes()
        .map(|n| {
            let mut hasher = DefaultHasher::new();
            NodeKindTag::from_kind(&n.kind).hash(&mut hasher);
            (n.id, hasher.finish())
        })
        .collect();

    // Collect label counts
    let mut all_counts: FxHashMap<u64, usize> = FxHashMap::default();

    for _ in 0..iterations {
        let mut new_labels = FxHashMap::default();

        for node in graph.nodes() {
            // Get neighbor labels
            let mut neighbor_labels: Vec<u64> = graph
                .outgoing_edges(node.id)
                .filter_map(|e| labels.get(&e.target).copied())
                .collect();

            neighbor_labels.extend(
                graph
                    .incoming_edges(node.id)
                    .filter_map(|e| labels.get(&e.source).copied()),
            );

            neighbor_labels.sort_unstable();

            // Hash current label + sorted neighbor labels
            let mut hasher = DefaultHasher::new();
            labels.get(&node.id).hash(&mut hasher);
            for nl in neighbor_labels {
                nl.hash(&mut hasher);
            }

            let new_label = hasher.finish();
            new_labels.insert(node.id, new_label);

            *all_counts.entry(new_label).or_insert(0) += 1;
        }

        labels = new_labels;
    }

    // Add final labels to counts
    for label in labels.values() {
        *all_counts.entry(*label).or_insert(0) += 1;
    }

    all_counts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CpgNode, CpgNodeKind, SourceRange, Language, CpgEdgeKind};

    #[test]
    fn test_identical_graphs() {
        let mut g1 = CodePropertyGraph::new(Language::Rust);
        g1.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::Root, SourceRange::default()));
        g1.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::If, SourceRange::default()));

        let mut g2 = CodePropertyGraph::new(Language::Rust);
        g2.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::Root, SourceRange::default()));
        g2.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::If, SourceRange::default()));

        let similarity = GraphSimilarity::new();
        let score = similarity.similarity(&g1, &g2);

        assert!(score > 0.9, "Identical graphs should have high similarity: {}", score);
    }

    #[test]
    fn test_different_graphs() {
        let mut g1 = CodePropertyGraph::new(Language::Rust);
        g1.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::Root, SourceRange::default()));

        let mut g2 = CodePropertyGraph::new(Language::Rust);
        g2.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::If, SourceRange::default()));
        g2.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::While, SourceRange::default()));
        g2.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::For, SourceRange::default()));

        let similarity = GraphSimilarity::new();
        let score = similarity.similarity(&g1, &g2);

        assert!(score < 0.5, "Different graphs should have low similarity: {}", score);
    }

    #[test]
    fn test_empty_graphs() {
        let g1 = CodePropertyGraph::new(Language::Rust);
        let g2 = CodePropertyGraph::new(Language::Rust);

        let similarity = GraphSimilarity::new();
        let score = similarity.similarity(&g1, &g2);

        assert_eq!(score, 1.0, "Empty graphs should be identical");
    }

    #[test]
    fn test_different_metrics() {
        // Create two similar graphs with same node kinds
        let mut g1 = CodePropertyGraph::new(Language::Rust);
        let n1 = g1.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::Root, SourceRange::default()));
        let n2 = g1.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::If, SourceRange::default()));
        let n3 = g1.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::While, SourceRange::default()));
        g1.connect(n1, n2, CpgEdgeKind::AstChild);
        g1.connect(n1, n3, CpgEdgeKind::AstChild);

        let mut g2 = CodePropertyGraph::new(Language::Rust);
        let n4 = g2.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::Root, SourceRange::default()));
        let n5 = g2.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::If, SourceRange::default()));
        let n6 = g2.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::While, SourceRange::default()));
        g2.connect(n4, n5, CpgEdgeKind::AstChild);
        g2.connect(n4, n6, CpgEdgeKind::AstChild);

        // Test different metrics on similar graphs
        let jaccard = GraphSimilarity::new().with_metric(SimilarityMetric::Jaccard).similarity(&g1, &g2);
        let cosine = GraphSimilarity::new().with_metric(SimilarityMetric::Cosine).similarity(&g1, &g2);
        let wl = GraphSimilarity::new().with_metric(SimilarityMetric::WeisfeilerLehman).similarity(&g1, &g2);

        // Similar graphs should have high similarity across all metrics
        assert!(jaccard > 0.9, "Jaccard: {}", jaccard);
        assert!(cosine > 0.9, "Cosine: {}", cosine);
        assert!(wl > 0.9, "WL: {}", wl);
    }

    // ==================== added coverage: example-based ====================

    /// The `GraphEdit` metric (the 4th metric) scores identical graphs as 1.0.
    #[test]
    fn test_graph_edit_identical_is_one() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let a = g.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::Root, SourceRange::default()));
        let b = g.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::If, SourceRange::default()));
        g.connect(a, b, CpgEdgeKind::AstChild);

        let ge = GraphSimilarity::new().with_metric(SimilarityMetric::GraphEdit);
        let s = ge.similarity(&g, &g.clone());
        assert!((s - 1.0).abs() < 1e-9, "GraphEdit of identical graphs should be 1.0, got {s}");
    }

    /// `with_structural_weight` / `with_label_weight` genuinely reshape the
    /// `GraphEdit` score: the label term is exactly the Jaccard component, the
    /// structural term is the node/edge-count blend, and the two disagree.
    #[test]
    fn test_graph_edit_weights_affect_score() {
        // g1: 2 nodes, 1 edge.
        let mut g1 = CodePropertyGraph::new(Language::Rust);
        let a = g1.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::Root, SourceRange::default()));
        let b = g1.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::If, SourceRange::default()));
        g1.connect(a, b, CpgEdgeKind::AstChild);

        // g2: 3 nodes, 2 edges.
        let mut g2 = CodePropertyGraph::new(Language::Rust);
        let c = g2.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::Root, SourceRange::default()));
        let d = g2.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::If, SourceRange::default()));
        let e = g2.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::While, SourceRange::default()));
        g2.connect(c, d, CpgEdgeKind::AstChild);
        g2.connect(c, e, CpgEdgeKind::AstChild);

        let jaccard = GraphSimilarity::new()
            .with_metric(SimilarityMetric::Jaccard)
            .similarity(&g1, &g2);

        // Label-only GraphEdit collapses to the Jaccard term.
        let label_only = GraphSimilarity::new()
            .with_metric(SimilarityMetric::GraphEdit)
            .with_structural_weight(0.0)
            .with_label_weight(1.0)
            .similarity(&g1, &g2);
        assert!(
            (label_only - jaccard).abs() < 1e-9,
            "label-only GraphEdit ({label_only}) should equal Jaccard ({jaccard})"
        );

        // Structural-only GraphEdit = 0.6*node_sim + 0.4*edge_sim.
        // node_sim = 1 - |2-3|/3 = 2/3 ; edge_sim = 1 - |1-2|/2 = 1/2.
        let struct_only = GraphSimilarity::new()
            .with_metric(SimilarityMetric::GraphEdit)
            .with_structural_weight(1.0)
            .with_label_weight(0.0)
            .similarity(&g1, &g2);
        let expected_struct = 0.6 * (2.0 / 3.0) + 0.4 * 0.5;
        assert!(
            (struct_only - expected_struct).abs() < 1e-9,
            "structural-only GraphEdit ({struct_only}) != expected ({expected_struct})"
        );

        // The two weightings disagree, so the weights genuinely matter.
        assert!((struct_only - label_only).abs() > 1e-9);
    }

    /// All four metrics are reflexive (1.0) on a non-empty graph and stay within
    /// [0, 1] against a different graph.
    #[test]
    fn test_all_metrics_reflexive_and_bounded() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let a = g.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::Root, SourceRange::default()));
        let b = g.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::If, SourceRange::default()));
        let c = g.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Block { scope: crate::ScopeId::GLOBAL },
            SourceRange::default(),
        ));
        g.connect(a, b, CpgEdgeKind::AstChild);
        g.connect(b, c, CpgEdgeKind::ControlFlow(crate::CfgEdgeKind::Sequential));

        let metrics = [
            SimilarityMetric::Jaccard,
            SimilarityMetric::Cosine,
            SimilarityMetric::WeisfeilerLehman,
            SimilarityMetric::GraphEdit,
        ];
        for metric in metrics {
            let sim = GraphSimilarity::new().with_metric(metric);
            let self_score = sim.similarity(&g, &g);
            assert!(
                (self_score - 1.0).abs() < 1e-9,
                "{metric:?} self-similarity should be 1.0 (non-empty), got {self_score}"
            );

            let mut h = CodePropertyGraph::new(Language::Rust);
            h.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::While, SourceRange::default()));
            let s = sim.similarity(&g, &h);
            assert!((-1e-9..=1.0 + 1e-9).contains(&s), "{metric:?} out of range: {s}");
        }
    }

    /// The empty-graph carve-out: `Jaccard`/`GraphEdit` treat two empty graphs
    /// as identical (1.0), while `WeisfeilerLehman` degenerates to 0.0 (no
    /// labels exist to compare).
    #[test]
    fn test_empty_graph_carve_out() {
        let empty = CodePropertyGraph::new(Language::Rust);

        for metric in [SimilarityMetric::Jaccard, SimilarityMetric::GraphEdit] {
            let s = GraphSimilarity::new().with_metric(metric).similarity(&empty, &empty);
            assert!((s - 1.0).abs() < 1e-9, "{metric:?} empty-vs-empty should be 1.0, got {s}");
        }

        let wl = GraphSimilarity::new()
            .with_metric(SimilarityMetric::WeisfeilerLehman)
            .similarity(&empty, &empty);
        assert_eq!(wl, 0.0, "WL of empty graphs degenerates to 0.0");
    }
}

/// REGRESSION: the normalized-dot-product metrics (`Cosine`, `WeisfeilerLehman`)
/// are `= 1` in exact arithmetic when a graph is compared with itself, but
/// floating-point rounding in the two square roots and the division can push
/// the quotient just past 1 (observed: `1.0000000000000002`), breaking the
/// documented `[0, 1]` range. Found by the `tests/robustness.rs` suite.
#[cfg(test)]
mod range_regression {
    use super::*;
    use crate::testutil::arb_cpg_raw;
    use proptest::prelude::*;

    const METRICS: [SimilarityMetric; 4] = [
        SimilarityMetric::Jaccard,
        SimilarityMetric::Cosine,
        SimilarityMetric::GraphEdit,
        SimilarityMetric::WeisfeilerLehman,
    ];

    /// The exact graph shape that overshot: a function whose body holds a loop
    /// with a condition, a variable, a call, an identifier, and a return.
    #[test]
    fn self_similarity_never_exceeds_one() {
        use crate::testutil::{build_well_formed, StmtSpec};
        let cpg = build_well_formed(vec![
            StmtSpec::While(vec![
                StmtSpec::Let("x".into()),
                StmtSpec::CallStmt("g".into()),
                StmtSpec::Use("x".into()),
            ]),
            StmtSpec::Return,
        ]);

        for metric in METRICS {
            let s = GraphSimilarity::new().with_metric(metric).similarity(&cpg, &cpg);
            assert!(s <= 1.0, "{metric:?} self-similarity overshot 1.0: {s}");
            assert!(s >= 0.0, "{metric:?} self-similarity went negative: {s}");
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        /// No metric ever leaves `[0, 1]`, on any pair of graphs.
        #[test]
        fn prop_similarity_stays_in_the_unit_interval(
            a in arb_cpg_raw(),
            b in arb_cpg_raw(),
        ) {
            for metric in METRICS {
                let sim = GraphSimilarity::new().with_metric(metric);
                for (x, y) in [(&a, &b), (&b, &a), (&a, &a), (&b, &b)] {
                    let s = sim.similarity(x, y);
                    prop_assert!(s.is_finite(), "{:?} produced {}", metric, s);
                    prop_assert!((0.0..=1.0).contains(&s), "{:?} produced {}", metric, s);
                }
            }
        }
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::testutil::*;
    use proptest::prelude::*;

    const METRICS: [SimilarityMetric; 4] = [
        SimilarityMetric::Jaccard,
        SimilarityMetric::Cosine,
        SimilarityMetric::WeisfeilerLehman,
        SimilarityMetric::GraphEdit,
    ];

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        /// For every metric, similarity is bounded in [0, 1] and symmetric.
        #[test]
        fn prop_similarity_bounded_and_symmetric(a in arb_cpg_raw(), b in arb_cpg_raw()) {
            for metric in METRICS {
                let sim = GraphSimilarity::new().with_metric(metric);
                let ab = sim.similarity(&a, &b);
                let ba = sim.similarity(&b, &a);
                prop_assert!(ab >= -1e-9, "{:?} out of range: {}", metric, ab);
                prop_assert!(ab <= 1.0 + 1e-9, "{:?} out of range: {}", metric, ab);
                prop_assert!((ab - ba).abs() < 1e-9, "{:?} asymmetric: {} vs {}", metric, ab, ba);
            }
        }

        /// Reflexivity. `arb_cpg_raw` is always non-empty, so every metric
        /// (including Cosine/WL, which carve out the empty graph) scores 1.0 on
        /// the diagonal; a distinct-but-identical clone gives Jaccard exactly 1.0.
        #[test]
        fn prop_similarity_reflexive(a in arb_cpg_raw()) {
            for metric in METRICS {
                let sim = GraphSimilarity::new().with_metric(metric);
                let s = sim.similarity(&a, &a);
                prop_assert!((s - 1.0).abs() < 1e-9, "{:?} not reflexive: {}", metric, s);
            }
            let jac = GraphSimilarity::new().with_metric(SimilarityMetric::Jaccard);
            prop_assert_eq!(jac.similarity(&a, &a.clone()), 1.0);
        }
    }
}
