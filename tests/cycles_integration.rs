//! Public-API integration tests for bounded simple-cycle enumeration.

use libcpg::{simple_cycles, CycleOptions, GraphProjection, NodeId, ProjectionKind};

fn external_projection() -> GraphProjection {
    GraphProjection::from_edges(
        ProjectionKind::External,
        [10, 30, 70, 90].into_iter().map(NodeId::new),
        [
            (NodeId::new(10), NodeId::new(30), 1.0),
            (NodeId::new(30), NodeId::new(70), 1.0),
            (NodeId::new(70), NodeId::new(10), 1.0),
            (NodeId::new(30), NodeId::new(90), 1.0),
            (NodeId::new(90), NodeId::new(30), 1.0),
        ],
    )
}

#[test]
fn feature_free_root_api_returns_canonical_nonconsecutive_node_ids_read_only() {
    let graph = external_projection();
    let before = graph.clone();
    let options = CycleOptions {
        max_length: 4,
        max_cycles: 16,
    };
    let result = simple_cycles(&graph, &options).expect("valid external projection");

    assert_eq!(
        result.cycles,
        vec![
            vec![NodeId::new(30), NodeId::new(90)],
            vec![NodeId::new(10), NodeId::new(30), NodeId::new(70)],
        ]
    );
    assert_eq!(result.nodes, graph.nodes);
    assert!(!result.truncated);
    assert_eq!(result, simple_cycles(&graph, &options).expect("repeat"));
    assert_eq!(graph, before);
}

#[cfg(feature = "lang-rust")]
#[test]
fn parsed_mutual_recursion_produces_one_exact_call_cycle_without_cpg_mutation() {
    use libcpg::{
        call_graph_projection, CpgBuilder, CpgBuilderConfig, Language, TreeSitterCpgBuilder,
    };

    let source = r#"
        fn alpha() { beta(); }
        fn beta() { alpha(); }
        fn leaf() {}
    "#;
    let cpg =
        TreeSitterCpgBuilder::with_config(CpgBuilderConfig::default().with_import_resolution(true))
            .build(source, Language::Rust)
            .expect("build and resolve Rust calls");
    let shape = (cpg.node_count(), cpg.edge_count());
    let functions: Vec<_> = cpg.functions().map(|node| node.id).collect();
    let projection = call_graph_projection(&cpg);
    let result =
        simple_cycles(&projection, &CycleOptions::default()).expect("parsed call graph projection");

    assert_eq!(functions.len(), 3);
    assert_eq!(result.cycles, vec![vec![functions[0], functions[1]]]);
    assert_eq!(result.nodes, projection.nodes);
    assert!(!result.truncated);
    assert_eq!((cpg.node_count(), cpg.edge_count()), shape);
}
