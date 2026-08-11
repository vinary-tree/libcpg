//! Public-API integration tests for DSM visibility and core-periphery analysis.

use libcpg::{
    analyze_dsm, call_graph_projection, classify_core_periphery, projection_sccs,
    CodePropertyGraph, CorePeriphery, CpgEdgeKind, CpgNode, CpgNodeKind, Language, MethodSignature,
    NodeId, SccProjection, SourceRange, Visibility,
};

fn function_node(name: &str) -> CpgNode {
    CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Function {
            signature: MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            },
        },
        SourceRange::default(),
    )
}

#[test]
fn root_api_analyzes_a_call_graph_without_mutating_the_cpg() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let left_control = cpg.add_node(function_node("left_control"));
    let right_control = cpg.add_node(function_node("right_control"));
    let hub = cpg.add_node(function_node("hub"));
    let left_shared = cpg.add_node(function_node("left_shared"));
    let right_shared = cpg.add_node(function_node("right_shared"));
    for (source, target) in [
        (left_control, hub),
        (right_control, hub),
        (hub, left_shared),
        (hub, right_shared),
    ] {
        cpg.connect(source, target, CpgEdgeKind::CallSite);
    }

    let graph_shape = (cpg.node_count(), cpg.edge_count());
    let projection = call_graph_projection(&cpg);
    let dsm = analyze_dsm(&projection);
    let classification = classify_core_periphery(&projection, &dsm);
    let decomposition = projection_sccs(&projection);

    assert!(dsm.complete);
    assert!(!dsm.capped);
    assert_eq!(dsm.visibility_fan_out, vec![3, 3, 2, 0, 0]);
    assert_eq!(dsm.visibility_fan_in, vec![0, 0, 2, 3, 3]);
    assert!((dsm.propagation_cost - 0.32).abs() < f64::EPSILON);
    assert_eq!(
        classification.classes,
        vec![
            CorePeriphery::Control,
            CorePeriphery::Control,
            CorePeriphery::Core,
            CorePeriphery::Shared,
            CorePeriphery::Shared,
        ]
    );
    assert!(classification.cyclic_core.is_empty());
    assert_eq!(decomposition.projection, SccProjection::CallGraph);
    assert!(decomposition.is_acyclic());
    assert_eq!((cpg.node_count(), cpg.edge_count()), graph_shape);
}

#[cfg(feature = "lang-rust")]
#[test]
fn parsed_rust_resolution_projection_dsm_and_scc_pipeline_is_exact() {
    use libcpg::{CpgBuilder, CpgBuilderConfig, TreeSitterCpgBuilder};

    let source = r#"
        fn left() { right(); }
        fn right() { left(); }
        fn root() { left(); }
    "#;
    let cpg =
        TreeSitterCpgBuilder::with_config(CpgBuilderConfig::default().with_import_resolution(true))
            .build(source, Language::Rust)
            .expect("build and resolve the Rust call graph");
    let function_id = |name: &str| {
        cpg.functions()
            .find(|node| node.name() == Some(name))
            .map(|node| node.id)
            .expect("named parsed function")
    };
    let left = function_id("left");
    let right = function_id("right");
    let root = function_id("root");

    let graph_shape = (cpg.node_count(), cpg.edge_count());
    let projection = call_graph_projection(&cpg);
    let dsm = analyze_dsm(&projection);
    let classification = classify_core_periphery(&projection, &dsm);
    let left_dense = projection.dense_index(left).expect("left in call graph") as usize;
    let right_dense = projection.dense_index(right).expect("right in call graph") as usize;
    let root_dense = projection.dense_index(root).expect("root in call graph") as usize;
    let mut expected_core = vec![left_dense, right_dense];
    expected_core.sort_unstable();

    assert!(dsm.complete);
    assert!((dsm.propagation_cost - (4.0 / 9.0)).abs() < 1e-12);
    assert_eq!(classification.cyclic_core, expected_core);
    assert_eq!(classification.classes[left_dense], CorePeriphery::Core);
    assert_eq!(classification.classes[right_dense], CorePeriphery::Core);
    assert_eq!(classification.classes[root_dense], CorePeriphery::Control);
    assert_eq!((cpg.node_count(), cpg.edge_count()), graph_shape);
}
