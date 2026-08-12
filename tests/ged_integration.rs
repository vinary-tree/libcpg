//! Public-API integration tests for deterministic graph edit distance.

use libcpg::pattern::{
    ged_upper_bound, GedOptions, GraphSimilarity, SimilarityMetric, MAX_GED_NODES,
};
use libcpg::{
    CodePropertyGraph, CpgEdge, CpgEdgeKind, CpgNode, CpgNodeKind, EdgeId, Language, NodeId,
    SourceRange,
};

fn fixture(reverse_storage: bool, changed: bool) -> CodePropertyGraph {
    let mut graph = CodePropertyGraph::new(Language::Rust);
    let order: &[u32] = if reverse_storage {
        &[91, 37, 11]
    } else {
        &[11, 37, 91]
    };
    for &id in order {
        let kind = match (id, changed) {
            (37, true) => CpgNodeKind::If,
            (37, false) => CpgNodeKind::While,
            _ => CpgNodeKind::Root,
        };
        graph.add_node_with_id(CpgNode::new(NodeId::new(id), kind, SourceRange::default()));
    }
    let edges = if changed {
        [
            (NodeId::new(11), NodeId::new(37), CpgEdgeKind::AstChild),
            (NodeId::new(37), NodeId::new(91), CpgEdgeKind::Reference),
        ]
    } else {
        [
            (NodeId::new(11), NodeId::new(37), CpgEdgeKind::AstChild),
            (NodeId::new(37), NodeId::new(91), CpgEdgeKind::AstChild),
        ]
    };
    for (index, (source, target, kind)) in edges.into_iter().enumerate() {
        graph
            .add_edge(CpgEdge::new(
                EdgeId::new(index as u32),
                source,
                target,
                kind,
            ))
            .expect("fixture endpoints exist");
    }
    graph
}

#[test]
fn feature_free_public_api_is_typed_symmetric_deterministic_and_read_only() {
    assert_eq!(MAX_GED_NODES, 512);
    let first = fixture(false, false);
    let reordered = fixture(true, false);
    let changed = fixture(true, true);
    let first_shape = (first.node_count(), first.edge_count());
    let changed_shape = (changed.node_count(), changed.edge_count());
    let options = GedOptions::default();

    assert_eq!(
        ged_upper_bound(&first, &reordered, &options).expect("isomorphic fixture"),
        libcpg::pattern::GedBound {
            upper: 0.0,
            normalized: 0.0,
            capped: false,
        }
    );
    let bound = ged_upper_bound(&first, &changed, &options).expect("single typed change");
    assert_eq!(
        bound.upper, 2.0,
        "one relabel plus one typed-edge edit pair"
    );
    assert_eq!(bound.normalized, 0.25);
    assert!(!bound.capped);
    assert_eq!(
        bound,
        ged_upper_bound(&changed, &first, &options).expect("reverse bound")
    );
    assert_eq!(
        bound,
        ged_upper_bound(&first, &changed, &options).expect("repeated bound")
    );
    assert_eq!(
        GraphSimilarity::new()
            .with_metric(SimilarityMetric::GraphEdit)
            .similarity(&first, &changed),
        1.0 - bound.normalized
    );
    assert_eq!((first.node_count(), first.edge_count()), first_shape);
    assert_eq!((changed.node_count(), changed.edge_count()), changed_shape);
}

#[cfg(feature = "lang-rust")]
#[test]
fn parsed_rust_cpgs_use_the_bound_for_graph_edit_similarity_without_mutation() {
    use libcpg::{CpgBuilder, TreeSitterCpgBuilder};

    let first = TreeSitterCpgBuilder::new()
        .build(
            "fn classify(x: i32) -> i32 { if x > 0 { x } else { 0 } }",
            Language::Rust,
        )
        .expect("parse first Rust fixture");
    let second = TreeSitterCpgBuilder::new()
        .build(
            "fn classify(x: i32) -> i32 { while x > 0 { return x; } 0 }",
            Language::Rust,
        )
        .expect("parse second Rust fixture");
    let first_shape = (first.node_count(), first.edge_count());
    let second_shape = (second.node_count(), second.edge_count());
    let identical =
        ged_upper_bound(&first, &first, &GedOptions::default()).expect("identical parsed graph");
    let changed =
        ged_upper_bound(&first, &second, &GedOptions::default()).expect("changed parsed graph");

    assert_eq!(identical.upper, 0.0);
    assert!(!identical.capped);
    assert!(changed.upper > 0.0);
    assert!(!changed.capped);
    let similarity = GraphSimilarity::new()
        .with_metric(SimilarityMetric::GraphEdit)
        .similarity(&first, &second);
    assert_eq!(similarity, 1.0 - changed.normalized);
    assert_eq!((first.node_count(), first.edge_count()), first_shape);
    assert_eq!((second.node_count(), second.edge_count()), second_shape);
}
