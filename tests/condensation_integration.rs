//! Public-API integration tests for SCC condensation-DAG analytics.

use libcpg::{
    call_graph_sccs, condensation_metrics, CodePropertyGraph, CondensationOptions, CpgEdgeKind,
    CpgNode, CpgNodeKind, Language, MethodSignature, NodeId, SourceRange, Visibility,
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
fn root_api_composes_call_sccs_with_weighted_metrics_and_cg_layers() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let first = cpg.add_node(function_node("first"));
    let second = cpg.add_node(function_node("second"));
    let middle = cpg.add_node(function_node("middle"));
    let left_sink = cpg.add_node(function_node("left_sink"));
    let right_sink = cpg.add_node(function_node("right_sink"));
    for (source, target) in [
        (first, second),
        (second, first),
        (second, middle),
        (middle, left_sink),
        (middle, right_sink),
    ] {
        cpg.connect(source, target, CpgEdgeKind::CallSite);
    }

    let graph_shape = (cpg.node_count(), cpg.edge_count());
    let decomposition = call_graph_sccs(&cpg);
    let metrics = condensation_metrics(
        &decomposition,
        &CondensationOptions {
            coffman_graham_width: Some(2),
            ..CondensationOptions::default()
        },
    );

    assert!(metrics.complete);
    assert!(!metrics.capped);
    assert_eq!(decomposition.components[0].nodes, vec![first, second]);
    assert_eq!(metrics.levels, vec![0, 1, 2, 2]);
    assert_eq!(metrics.critical_path, vec![0, 1, 2]);
    assert_eq!(metrics.critical_path_weight, 4);
    assert_eq!((metrics.height, metrics.width), (3, 2));

    let layering = metrics.coffman_graham.expect("requested CG layering");
    assert_eq!(layering.layers, vec![vec![2, 3], vec![1], vec![0]]);
    assert_eq!(layering.layer_of, vec![2, 1, 0, 0]);
    assert_eq!((layering.height(), layering.width()), (3, 2));
    assert_eq!((cpg.node_count(), cpg.edge_count()), graph_shape);
}

#[cfg(feature = "lang-rust")]
#[test]
fn parsed_rust_resolution_scc_and_condensation_pipeline_is_exact() {
    use libcpg::{CpgBuilder, CpgBuilderConfig, TreeSitterCpgBuilder};

    let source = r#"
        fn leaf() {}
        fn left() { leaf(); }
        fn right() { leaf(); }
        fn root() { left(); right(); }
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
    let leaf = function_id("leaf");
    let left = function_id("left");
    let right = function_id("right");
    let root = function_id("root");

    let graph_shape = (cpg.node_count(), cpg.edge_count());
    let decomposition = call_graph_sccs(&cpg);
    let metrics = condensation_metrics(
        &decomposition,
        &CondensationOptions {
            coffman_graham_width: Some(2),
            ..CondensationOptions::default()
        },
    );
    let component = |function| decomposition.component_by_node[&function];

    assert!(metrics.complete);
    assert_eq!(decomposition.components.len(), 4);
    assert_eq!(metrics.levels[component(root)], 0);
    assert_eq!(metrics.levels[component(left)], 1);
    assert_eq!(metrics.levels[component(right)], 1);
    assert_eq!(metrics.levels[component(leaf)], 2);
    assert_eq!((metrics.height, metrics.width), (3, 2));
    assert_eq!(metrics.critical_path_weight, 3);

    let layering = metrics.coffman_graham.expect("requested CG layering");
    assert!(layering.layer_of[component(root)] > layering.layer_of[component(left)]);
    assert!(layering.layer_of[component(root)] > layering.layer_of[component(right)]);
    assert!(layering.layer_of[component(left)] > layering.layer_of[component(leaf)]);
    assert!(layering.layer_of[component(right)] > layering.layer_of[component(leaf)]);
    assert_eq!((cpg.node_count(), cpg.edge_count()), graph_shape);
}
