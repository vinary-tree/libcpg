//! Public-API integration tests for deterministic spectral analysis.

use libcpg::{
    algebraic_connectivity, try_algebraic_connectivity, GraphProjection, NodeId, ProjectionKind,
    SpectralOptions,
};

fn external_path() -> GraphProjection {
    GraphProjection::from_edges(
        ProjectionKind::External,
        [10, 20, 30, 40].into_iter().map(NodeId::new),
        [
            (NodeId::new(10), NodeId::new(20), 1.0),
            (NodeId::new(20), NodeId::new(30), 1.0),
            (NodeId::new(30), NodeId::new(40), 1.0),
        ],
    )
}

#[test]
fn feature_free_root_api_is_aligned_canonical_deterministic_and_read_only() {
    let graph = external_path();
    let before = graph.clone();
    let options = SpectralOptions::default();
    let result = try_algebraic_connectivity(&graph, &options)
        .expect("valid external path")
        .expect("four nodes");
    let expected = 2.0 - 2.0 * (std::f64::consts::PI / 4.0).cos();

    assert_eq!(result.nodes, graph.nodes);
    assert!((result.algebraic_connectivity - expected).abs() <= 1.0e-8);
    assert!(result.converged);
    assert!(result.residual_norm <= 1.0e-7);
    assert_eq!(result.partition, vec![true, true, false, false]);
    assert!(result.fiedler[0] > 0.0);
    assert_eq!(
        result,
        algebraic_connectivity(&graph, &options).expect("total wrapper result")
    );
    assert_eq!(graph, before);
}

#[cfg(feature = "lang-rust")]
#[test]
fn parsed_call_path_has_the_path_four_fiedler_cut_without_cpg_mutation() {
    use libcpg::{
        call_graph_projection, CpgBuilder, CpgBuilderConfig, Language, TreeSitterCpgBuilder,
    };

    let source = r#"
        fn a() { b(); }
        fn b() { c(); }
        fn c() { d(); }
        fn d() {}
    "#;
    let cpg =
        TreeSitterCpgBuilder::with_config(CpgBuilderConfig::default().with_import_resolution(true))
            .build(source, Language::Rust)
            .expect("build and resolve Rust call targets");
    let shape = (cpg.node_count(), cpg.edge_count());
    let projection = call_graph_projection(&cpg);
    let result = try_algebraic_connectivity(&projection, &SpectralOptions::default())
        .expect("valid parsed call projection")
        .expect("four parsed functions");
    let expected = 2.0 - 2.0 * (std::f64::consts::PI / 4.0).cos();

    assert_eq!(result.nodes, projection.nodes);
    assert!((result.algebraic_connectivity - expected).abs() <= 1.0e-8);
    assert_eq!(result.partition.iter().filter(|&&side| side).count(), 2);
    assert_eq!((cpg.node_count(), cpg.edge_count()), shape);
}
