//! End-to-end integration tests exercising libcpg through its PUBLIC API only,
//! as a downstream consumer would. Every section is feature-gated, so this file
//! compiles (to an empty binary if nothing is enabled) under any feature subset.
//!
//! Run the full set with: `cargo test --features "full rholang metta"`.

#![allow(clippy::items_after_test_module)]

#[cfg(feature = "lang-rust")]
mod rust_pipeline {
    use libcpg::{backward_slice, forward_slice, CpgBuilder, Language, PdgBuilder, TreeSitterCpgBuilder};

    const SRC: &str = "fn f(x: i32) -> i32 { let y = x + 1; if y > 0 { y } else { 0 } }";

    fn build() -> libcpg::CodePropertyGraph {
        TreeSitterCpgBuilder::new()
            .build(SRC, Language::Rust)
            .expect("build a Rust CPG")
    }

    #[test]
    fn builds_a_nonempty_graph_with_a_function() {
        let cpg = build();
        assert!(cpg.node_count() > 1);
        assert!(cpg.edge_count() >= 1);
        assert_eq!(cpg.language(), Language::Rust);
        assert_eq!(cpg.functions().count(), 1);
        let stats = cpg.stats();
        assert_eq!(stats.node_count, cpg.node_count());
        assert!(stats.cyclomatic_complexity >= 1);
    }

    #[test]
    fn pdg_then_slice_is_bounded_and_idempotent() {
        let mut cpg = build();
        let func = cpg.functions().map(|n| n.id).next().expect("a function");
        PdgBuilder::new().build(&mut cpg, func);
        let back = backward_slice(&cpg, func, 64);
        let fwd = forward_slice(&cpg, func, 64);
        assert!(back.len() <= 64 && fwd.len() <= 64);
        assert!(back.contains(&func));
        // Re-running PdgBuilder is idempotent (no duplicate edges).
        let before = cpg.edge_count();
        PdgBuilder::new().build(&mut cpg, func);
        assert_eq!(before, cpg.edge_count());
    }
}

#[cfg(feature = "lang-rust")]
mod matching {
    use libcpg::pattern::{GraphSimilarity, SimilarityMetric, SubgraphMatcher, Vf2Matcher};
    use libcpg::{CpgBuilder, Language, TreeSitterCpgBuilder};

    #[test]
    fn vf2_self_match_and_similarity_reflexive() {
        let cpg = TreeSitterCpgBuilder::new()
            .build("fn g(a: i32) -> i32 { a * 2 }", Language::Rust)
            .expect("build");
        let matches = Vf2Matcher::new().find_matches(&cpg, &cpg);
        assert!(!matches.is_empty(), "a graph matches itself");
        let sim = GraphSimilarity::new().with_metric(SimilarityMetric::Jaccard);
        assert!((sim.similarity(&cpg, &cpg) - 1.0).abs() < 1e-9);
    }
}

#[cfg(all(feature = "lang-rust", feature = "design-patterns"))]
mod gof {
    use libcpg::patterns::design::{GofPatternDetector, PatternDetector};
    use libcpg::{CpgBuilder, Language, TreeSitterCpgBuilder};

    #[test]
    fn detector_runs_and_orders_by_confidence() {
        let cpg = TreeSitterCpgBuilder::new()
            .build("struct S; impl S { fn new() -> S { S } }", Language::Rust)
            .expect("build");
        let matches = GofPatternDetector::new().detect(&cpg);
        for w in matches.windows(2) {
            assert!(w[0].confidence >= w[1].confidence, "sorted descending");
        }
    }
}

#[cfg(all(feature = "lang-rust", feature = "algorithm-detection"))]
mod algorithms {
    use libcpg::algorithms::detection::DefaultAlgorithmDetector;
    use libcpg::algorithms::AlgorithmDetector;
    use libcpg::{CpgBuilder, Language, TreeSitterCpgBuilder};

    #[test]
    fn detect_runs_per_function() {
        let cpg = TreeSitterCpgBuilder::new()
            .build(
                "fn sort(a: &mut [i32]) { for i in 0..a.len() { for j in 0..a.len() { if a[i] < a[j] { a.swap(i, j); } } } }",
                Language::Rust,
            )
            .expect("build");
        let func = cpg.functions().map(|n| n.id).next().expect("a function");
        let detector = DefaultAlgorithmDetector::new();
        let found = detector.detect(&cpg, func); // exercises the pipeline end to end
        for w in found.windows(2) {
            assert!(w[0].confidence >= w[1].confidence, "sorted descending");
        }
    }
}

#[cfg(all(feature = "lang-rust", feature = "gnn"))]
mod gnn {
    use libcpg::gnn::{CpgGnn, GraphNeuralNetwork};
    use libcpg::{CpgBuilder, Language, TreeSitterCpgBuilder};

    #[test]
    fn propagate_embeds_every_node() {
        let cpg = TreeSitterCpgBuilder::new()
            .build("fn f(x: i32) -> i32 { x + 1 }", Language::Rust)
            .expect("build");
        let ids: Vec<_> = cpg.node_ids().collect();
        let mut gnn = CpgGnn::new(cpg).with_embedding_dim(16).with_num_layers(2);
        gnn.propagate(2);
        assert!(gnn.is_initialized());
        for id in ids {
            assert!(gnn.node_embedding(id).is_some());
        }
    }
}

#[cfg(all(feature = "lang-rust", feature = "serde"))]
mod serialization {
    use libcpg::{CpgBuilder, Language, TreeSitterCpgBuilder};

    #[test]
    fn whole_graph_json_round_trip_preserves_counts() {
        let cpg = TreeSitterCpgBuilder::new()
            .build("fn f(x: i32) -> i32 { x + 1 }", Language::Rust)
            .expect("build");
        let json = serde_json::to_string(&cpg).expect("serialize");
        let back: libcpg::CodePropertyGraph = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.node_count(), cpg.node_count());
        assert_eq!(back.edge_count(), cpg.edge_count());
    }
}
