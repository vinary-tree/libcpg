#[path = "../benches/support/mod.rs"]
mod support;

use libcpg::CpgEdgeKind;
use proptest::prelude::*;

use support::{
    assert_synthetic_graph, assert_traversal_graph, synthetic_graph, traversal_graph,
    SyntheticShape, CROSS_LANGUAGE, RUST_LARGE, RUST_MEDIUM, RUST_SMALL,
};

fn edge_snapshot(graph: &libcpg::CodePropertyGraph) -> Vec<(u32, u32, CpgEdgeKind)> {
    let mut edges: Vec<_> = graph
        .edges()
        .map(|edge| {
            (
                edge.source.as_u32(),
                edge.target.as_u32(),
                edge.kind.clone(),
            )
        })
        .collect();
    edges.sort_unstable_by_key(|(source, target, kind)| (*source, *target, format!("{kind:?}")));
    edges
}

#[test]
fn committed_corpora_stay_in_the_declared_size_classes() {
    let small_lines = RUST_SMALL.source.lines().count();
    let medium_lines = RUST_MEDIUM.source.lines().count();
    let large_lines: usize = RUST_LARGE
        .iter()
        .map(|file| file.source.lines().count())
        .sum();

    assert!((40..=80).contains(&small_lines));
    assert!((450..=600).contains(&medium_lines));
    assert!((4_800..=5_500).contains(&large_lines));
    assert_eq!(CROSS_LANGUAGE.len(), 4);
    assert!(CROSS_LANGUAGE
        .iter()
        .all(|file| !file.source.trim().is_empty()));
}

#[test]
fn every_synthetic_shape_satisfies_its_exact_oracle() {
    for shape in SyntheticShape::ALL {
        let graph = synthetic_graph(shape, 17, support::FIXTURE_SEED);
        assert_synthetic_graph(&graph, shape, 17);
    }
}

#[test]
fn traversal_fixture_covers_every_timed_primitive() {
    let (graph, nodes) = traversal_graph(64);
    assert_traversal_graph(&graph, &nodes);
}

#[cfg(all(
    feature = "lang-rust",
    feature = "lang-python",
    feature = "lang-javascript",
    feature = "lang-go",
    feature = "lang-c"
))]
#[test]
fn cross_language_corpus_builds_end_to_end() {
    use libcpg::{CpgBuilder, TreeSitterCpgBuilder};

    for file in CROSS_LANGUAGE {
        let graph = TreeSitterCpgBuilder::new()
            .build(file.source, file.language)
            .unwrap_or_else(|error| panic!("{} corpus failed: {error}", file.name));
        assert!(graph.node_count() > 0, "{} nodes", file.name);
        assert!(graph.edge_count() > 0, "{} edges", file.name);
    }
}

proptest! {
    #[test]
    fn seeded_generators_are_repeatable_and_cardinality_exact(
        node_count in 1usize..48,
        seed in any::<u64>(),
        shape_index in 0usize..SyntheticShape::ALL.len(),
    ) {
        let shape = SyntheticShape::ALL[shape_index];
        let first = synthetic_graph(shape, node_count, seed);
        let second = synthetic_graph(shape, node_count, seed);
        assert_synthetic_graph(&first, shape, node_count);
        prop_assert_eq!(edge_snapshot(&first), edge_snapshot(&second));
    }
}
