//! Public-API integration tests for projection WL labels and clone classes.

use libcpg::{
    call_graph_projection, structural_clone_classes, wl_labels, CodePropertyGraph, CpgEdgeKind,
    CpgNode, CpgNodeKind, Language, MethodSignature, NodeId, SourceRange, Visibility,
    MAX_WL_ITERATIONS,
};

fn function_node(name: &str) -> CpgNode {
    CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Function {
            signature: Box::new(MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            }),
        },
        SourceRange::default(),
    )
}

#[test]
fn root_api_finds_two_isomorphic_call_cycles_without_mutating_the_cpg() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let left = [
        cpg.add_node(function_node("left_a")),
        cpg.add_node(function_node("left_b")),
        cpg.add_node(function_node("left_c")),
    ];
    let right = [
        cpg.add_node(function_node("right_a")),
        cpg.add_node(function_node("right_b")),
        cpg.add_node(function_node("right_c")),
    ];
    for cycle in [left, right] {
        for index in 0..cycle.len() {
            cpg.connect(
                cycle[index],
                cycle[(index + 1) % cycle.len()],
                CpgEdgeKind::CallSite,
            );
        }
    }

    let graph_shape = (cpg.node_count(), cpg.edge_count());
    let projection = call_graph_projection(&cpg);
    let labels = wl_labels(&projection, 2);
    let classes = structural_clone_classes(&projection, 2, 2);

    assert!(projection.is_well_formed());
    assert_eq!(MAX_WL_ITERATIONS, 16);
    assert_eq!(labels.len(), 6);
    assert!(labels.iter().all(|&label| label == labels[0]));
    assert_eq!(classes, vec![(0..6).map(NodeId::new).collect::<Vec<_>>()]);
    assert!(structural_clone_classes(&projection, 2, 7).is_empty());
    assert_eq!((cpg.node_count(), cpg.edge_count()), graph_shape);
}

#[cfg(feature = "lang-rust")]
#[test]
fn parser_built_call_path_matches_public_wl_semantics() {
    use libcpg::{CpgBuilder, CpgBuilderConfig, TreeSitterCpgBuilder};

    let source = r#"
        fn first() { second(); }
        fn second() { third(); }
        fn third() { fourth(); }
        fn fourth() {}
    "#;
    let cpg =
        TreeSitterCpgBuilder::with_config(CpgBuilderConfig::default().with_import_resolution(true))
            .build(source, Language::Rust)
            .expect("build and resolve parser-backed Rust calls");
    let shape = (cpg.node_count(), cpg.edge_count());
    let projection = call_graph_projection(&cpg);

    let zero_round_labels = wl_labels(&projection, 0);
    let refined_labels = wl_labels(&projection, 4);
    assert_eq!(projection.node_count(), 4);
    assert_eq!(zero_round_labels.len(), 4);
    assert_eq!(refined_labels.len(), 4);
    assert_ne!(zero_round_labels[0], zero_round_labels[1]);
    assert_eq!(refined_labels, wl_labels(&projection, 4));
    assert_eq!((cpg.node_count(), cpg.edge_count()), shape);
}
