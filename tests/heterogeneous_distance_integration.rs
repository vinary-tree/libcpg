//! Public-API integration coverage for heterogeneous export and fuzzing distance.

use libcpg::{
    distance_to_targets, export_heterogeneous_gnn, CfgEdgeKind, CodePropertyGraph, CpgEdgeKind,
    CpgNode, CpgNodeKind, Language, NodeId, SourceRange, EDGE_KIND_CLASS_COUNT,
    HETERO_NODE_FEATURE_COUNT,
};

fn node(id: u32) -> CpgNode {
    CpgNode::new(
        NodeId::new(id),
        CpgNodeKind::Return,
        SourceRange::from_bytes(id, id.saturating_add(1)),
    )
}

fn connect(cpg: &mut CodePropertyGraph, source: u32, target: u32, kind: CpgEdgeKind) {
    cpg.connect(NodeId::new(source), NodeId::new(target), kind)
        .expect("public FT.8 fixture endpoints exist");
}

#[test]
fn public_apis_preserve_graph_and_cross_cfg_call_boundaries() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    for id in [50, 10, 40, 20, 30, 99] {
        cpg.add_node_with_id(node(id));
    }
    connect(
        &mut cpg,
        10,
        20,
        CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
    );
    connect(&mut cpg, 20, 30, CpgEdgeKind::CallSite);
    connect(
        &mut cpg,
        30,
        40,
        CpgEdgeKind::ControlFlow(CfgEdgeKind::CallReturn),
    );
    connect(&mut cpg, 40, 50, CpgEdgeKind::StaticCall);
    connect(&mut cpg, 99, 10, CpgEdgeKind::DataDependence);

    let before = (cpg.node_count(), cpg.edge_count());
    let export = export_heterogeneous_gnn(&cpg);
    let distance = distance_to_targets(&cpg, &[NodeId::new(50)]);

    assert_eq!(export.node_ids, [10, 20, 30, 40, 50, 99].map(NodeId::new));
    assert_eq!(export.relations.len(), usize::from(EDGE_KIND_CLASS_COUNT));
    assert_eq!(export.node_features.len(), cpg.node_count());
    assert!(export
        .node_features
        .iter()
        .all(|row| row.len() == HETERO_NODE_FEATURE_COUNT));
    assert_eq!(export.relation(&CpgEdgeKind::CallSite).edge_index, [(1, 2)]);
    assert_eq!(distance[&NodeId::new(10)], 4.0);
    assert_eq!(distance[&NodeId::new(50)], 0.0);
    assert!(distance[&NodeId::new(99)].is_infinite());
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}

#[test]
fn both_public_apis_handle_depth_100_000_on_a_64_kib_stack() {
    const DEPTH: u32 = 100_000;

    std::thread::Builder::new()
        .name("heterogeneous-distance-small-stack".into())
        .stack_size(64 * 1024)
        .spawn(|| {
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            for id in 0..DEPTH {
                cpg.add_node_with_id(node(id));
            }
            for id in 0..DEPTH - 1 {
                connect(
                    &mut cpg,
                    id,
                    id + 1,
                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                );
            }

            let distance = distance_to_targets(&cpg, &[NodeId::new(DEPTH - 1)]);
            assert_eq!(distance[&NodeId::new(0)], f64::from(DEPTH - 1));
            let export = export_heterogeneous_gnn(&cpg);
            assert_eq!(export.node_ids.len(), DEPTH as usize);
            assert_eq!(
                export
                    .relation(&CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential))
                    .edge_index
                    .len(),
                DEPTH as usize - 1
            );
        })
        .expect("spawn 64 KiB stack-safety thread")
        .join()
        .expect("stack-safe FT.8 analyses complete");
}

#[cfg(feature = "lang-rust")]
#[test]
fn parser_built_rust_cpg_exports_typed_relations_and_finite_cfg_distance() {
    use libcpg::{CpgBuilder, CpgBuilderConfig, TreeSitterCpgBuilder};

    let source = "fn classify(x: i32) -> i32 { if x > 0 { x + 1 } else { 0 } }";
    let cpg =
        TreeSitterCpgBuilder::with_config(CpgBuilderConfig::new().with_cfg(true).with_dfg(true))
            .build(source, Language::Rust)
            .expect("build parser-backed Rust CPG");
    let target = *cpg.cfg_exits().first().expect("parsed CFG has an exit");

    let export = export_heterogeneous_gnn(&cpg);
    let distance = distance_to_targets(&cpg, &[target]);
    let cfg_edge_count: usize = export.relations[4..18]
        .iter()
        .map(|relation| relation.edge_index.len())
        .sum();

    assert_eq!(export.node_ids.len(), cpg.node_count());
    assert!(cfg_edge_count > 0);
    assert_eq!(distance[&target], 0.0);
    assert!(distance
        .iter()
        .any(|(node, value)| *node != target && value.is_finite() && *value > 0.0));
}
