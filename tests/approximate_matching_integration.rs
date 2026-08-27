#![cfg(feature = "approximate-matching")]

use libcpg::{
    approximate_revision_alignments, ApproximateCancellation, ApproximateCompletion,
    ApproximateConfig, ApproximateId, ApproximateMethod, ApproximateRecord, ApproximateTape,
};

#[cfg(all(feature = "lang-rust", feature = "lang-python"))]
#[test]
fn parser_built_rust_query_aligns_with_python_function() {
    use libcpg::{CpgBuilder, Language, TreeSitterCpgBuilder};

    let rust = TreeSitterCpgBuilder::new()
        .build(
            "fn calculate_total(values: &[i32]) -> i32 { values.iter().sum() }",
            Language::Rust,
        )
        .expect("parser-built Rust query graph");
    let python = TreeSitterCpgBuilder::new()
        .build(
            "def calculate_totl(values):\n    return sum(values)\n",
            Language::Python,
        )
        .expect("parser-built Python candidate graph");
    let rust_function = rust
        .nodes()
        .find(|node| node.name() == Some("calculate_total"))
        .expect("Rust function name");
    let query = ApproximateRecord::new(
        ApproximateId::CpgNode {
            graph: 1,
            node: rust_function.id,
        },
        ApproximateTape::text("calculate_total"),
    );
    let config = ApproximateConfig {
        method: ApproximateMethod::DamerauLevenshtein,
        max_distance: 2,
        ..ApproximateConfig::default()
    };
    let before = (python.node_count(), python.edge_count());
    let report = approximate_revision_alignments(
        &query,
        &python,
        2,
        &config,
        &ApproximateCancellation::default(),
    )
    .expect("bounded cross-language candidate generation");

    assert_eq!(report.completion, ApproximateCompletion::Complete);
    let candidate = report.evidence.first().expect("one close Python name");
    let ApproximateId::CpgNode { graph, node } = candidate.candidate else {
        panic!("parser-built candidate should retain its CPG identity");
    };
    assert_eq!(graph, 2);
    assert_eq!(
        python.node(node).and_then(|item| item.name()),
        Some("calculate_totl")
    );
    assert_eq!((python.node_count(), python.edge_count()), before);
}

#[test]
fn public_text_search_is_unicode_scalar_based() {
    let query = ApproximateRecord::new(
        ApproximateId::External { corpus: 1, item: 0 },
        ApproximateTape::text("café"),
    );
    let candidates = [ApproximateRecord::new(
        ApproximateId::External { corpus: 1, item: 1 },
        ApproximateTape::text("cafe"),
    )];
    let report = libcpg::approximate_candidates(
        &query,
        &candidates,
        libcpg::ApproximateConsumer::RevisionAlignment,
        &ApproximateConfig::default(),
        &ApproximateCancellation::default(),
    )
    .expect("Unicode candidate search");

    assert_eq!(report.evidence[0].raw_cost.numerator, 1);
}
