//! Public-API integration tests for advisory architectural-smell detection.

use libcpg::{
    architecture_smells, martin_metrics, module_graph_projection, projection_sccs, ArchSmellKind,
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, Language, ModuleAssignment, NodeId,
    SmellOptions, SourceRange,
};

fn module_node(id: u32, name: &str) -> CpgNode {
    CpgNode::new(
        NodeId::new(id),
        CpgNodeKind::Module { name: name.into() },
        SourceRange::default(),
    )
}

fn hub_fixture() -> (CodePropertyGraph, ModuleAssignment) {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let ids = [101, 211, 307, 401, 503, 601, 701];
    for (dense, id) in ids.into_iter().enumerate() {
        cpg.add_node_with_id(module_node(id, &format!("m{dense}")));
    }
    let hub = NodeId::new(101);
    for source in [211, 307, 401] {
        cpg.connect(NodeId::new(source), hub, CpgEdgeKind::Imports);
    }
    for target in [503, 601, 701] {
        cpg.connect(hub, NodeId::new(target), CpgEdgeKind::Imports);
    }
    let assignment = ModuleAssignment::from_pairs(
        ids.into_iter()
            .enumerate()
            .map(|(dense, id)| (NodeId::new(id), format!("m{dense}"))),
    );
    (cpg, assignment)
}

#[test]
fn feature_free_module_pipeline_finds_exact_hub_and_is_read_only() {
    let (cpg, assignment) = hub_fixture();
    let shape = (cpg.node_count(), cpg.edge_count());
    let view = module_graph_projection(&cpg);
    let scc = projection_sccs(&view);
    let martin = martin_metrics(&cpg, &assignment, &view);

    let first = architecture_smells(&view, &scc, &martin, &assignment, &SmellOptions::default());
    let second = architecture_smells(&view, &scc, &martin, &assignment, &SmellOptions::default());

    assert_eq!(first, second);
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].kind, ArchSmellKind::HubLikeDependency);
    assert_eq!(first[0].nodes, vec![NodeId::new(101)]);
    assert_eq!(first[0].evidence["fan_in"], 3.0);
    assert_eq!(first[0].evidence["fan_out"], 3.0);
    assert_eq!((cpg.node_count(), cpg.edge_count()), shape);
}

#[cfg(feature = "serde")]
#[test]
fn public_finding_serializes_with_stable_kind_and_evidence_names() {
    let (cpg, assignment) = hub_fixture();
    let view = module_graph_projection(&cpg);
    let scc = projection_sccs(&view);
    let martin = martin_metrics(&cpg, &assignment, &view);
    let findings = architecture_smells(&view, &scc, &martin, &assignment, &SmellOptions::default());

    let json = serde_json::to_value(&findings).expect("serialize public findings");
    assert_eq!(json[0]["kind"], "HubLikeDependency");
    assert_eq!(json[0]["evidence"]["fan_in"], 3.0);
    assert!(json[0]["severity"].as_f64().is_some_and(f64::is_finite));
}
