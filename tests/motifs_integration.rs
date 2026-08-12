//! Public-API integration tests for directed motif census.

use libcpg::{motif_census, GraphProjection, MotifOptions, NodeId, ProjectionKind, TRIAD_NAMES};

fn external_projection() -> GraphProjection {
    GraphProjection::from_edges(
        ProjectionKind::External,
        [10, 30, 70, 90].into_iter().map(NodeId::new),
        [
            (NodeId::new(10), NodeId::new(30), 1.0),
            (NodeId::new(10), NodeId::new(70), 1.0),
            (NodeId::new(30), NodeId::new(70), 1.0),
        ],
    )
}

#[test]
fn feature_free_root_api_counts_sparse_ids_deterministically_and_read_only() {
    let graph = external_projection();
    let before = graph.clone();
    let result = motif_census(&graph, &MotifOptions::default()).expect("valid projection");

    let one_arc = TRIAD_NAMES
        .iter()
        .position(|name| *name == "012")
        .expect("012 is public");
    let transitive = TRIAD_NAMES
        .iter()
        .position(|name| *name == "030T")
        .expect("030T is public");
    assert_eq!(result.triads[one_arc], 3);
    assert_eq!(result.triads[transitive], 1);
    assert_eq!(result.triads.iter().sum::<u64>(), 4);
    assert_eq!(result.graphlets_4, [0, 0]);
    assert_eq!(
        result,
        motif_census(&graph, &MotifOptions::default()).expect("repeat")
    );
    assert_eq!(graph, before);
}

#[cfg(feature = "lang-rust")]
#[test]
fn parsed_call_dag_is_one_transitive_triad_without_cpg_mutation() {
    use libcpg::{
        call_graph_projection, CpgBuilder, CpgBuilderConfig, Language, TreeSitterCpgBuilder,
    };

    let source = r#"
        fn alpha() { beta(); gamma(); }
        fn beta() { gamma(); }
        fn gamma() {}
    "#;
    let cpg =
        TreeSitterCpgBuilder::with_config(CpgBuilderConfig::default().with_import_resolution(true))
            .build(source, Language::Rust)
            .expect("build and resolve Rust calls");
    let shape = (cpg.node_count(), cpg.edge_count());
    let projection = call_graph_projection(&cpg);
    let result = motif_census(&projection, &MotifOptions::default()).expect("three functions");
    let transitive = TRIAD_NAMES
        .iter()
        .position(|name| *name == "030T")
        .expect("030T is public");

    assert_eq!(projection.node_count(), 3);
    assert_eq!(result.triads[transitive], 1);
    assert_eq!(result.triads.iter().sum::<u64>(), 1);
    assert_eq!(result.graphlets_4, [0, 0]);
    assert_eq!((cpg.node_count(), cpg.edge_count()), shape);
}
