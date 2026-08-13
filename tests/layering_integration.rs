//! Public-API integration tests for deterministic architectural layering.

use libcpg::{
    condensation_metrics, infer_layering, module_graph_projection, projection_sccs,
    CodePropertyGraph, CondensationOptions, CpgEdgeKind, CpgNode, CpgNodeKind, Language,
    LayeringViolationKind, NodeId, SourceRange,
};

fn module_node(name: &str) -> CpgNode {
    CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Module { name: name.into() },
        SourceRange::default(),
    )
}

#[test]
fn root_api_finds_cycle_and_skip_in_a_module_architecture() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let facade = cpg.add_node(module_node("facade"));
    let facade_helper = cpg.add_node(module_node("facade_helper"));
    let service = cpg.add_node(module_node("service"));
    let repository = cpg.add_node(module_node("repository"));
    for (source, target) in [
        (facade, facade_helper),
        (facade_helper, facade),
        (facade_helper, service),
        (service, repository),
        (facade, repository),
    ] {
        cpg.connect(source, target, CpgEdgeKind::Imports);
    }

    let graph_shape = (cpg.node_count(), cpg.edge_count());
    let projection = module_graph_projection(&cpg);
    let decomposition = projection_sccs(&projection);
    let metrics = condensation_metrics(&decomposition, &CondensationOptions::default());
    let report = infer_layering(&decomposition, &metrics);
    let facade_component = decomposition.component_by_node[&facade];
    let service_component = decomposition.component_by_node[&service];
    let repository_component = decomposition.component_by_node[&repository];

    assert!(report.complete);
    assert_eq!(report.layer_of[facade_component], 2);
    assert_eq!(report.layer_of[service_component], 1);
    assert_eq!(report.layer_of[repository_component], 0);
    assert!(report.violations.iter().any(|violation| {
        violation.kind == LayeringViolationKind::CyclicComponent
            && violation.source_component == facade_component
    }));
    assert!(report.violations.iter().any(|violation| {
        violation.kind == LayeringViolationKind::SkipLayer
            && violation.source_component == facade_component
            && violation.target_component == repository_component
            && violation.layers_crossed == 2
    }));
    assert_eq!((cpg.node_count(), cpg.edge_count()), graph_shape);
}

#[cfg(feature = "serde")]
#[test]
fn public_report_serializes_with_stable_violation_kinds() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let source = cpg.add_node(module_node("source"));
    let target = cpg.add_node(module_node("target"));
    cpg.connect(source, target, CpgEdgeKind::Imports);
    cpg.connect(source, source, CpgEdgeKind::Imports);

    let projection = module_graph_projection(&cpg);
    let decomposition = projection_sccs(&projection);
    let metrics = condensation_metrics(&decomposition, &CondensationOptions::default());
    let report = infer_layering(&decomposition, &metrics);
    let json = serde_json::to_value(&report).expect("serialize public report");

    assert_eq!(json["complete"], true);
    assert_eq!(json["capped"], false);
    assert_eq!(json["violations"][0]["kind"], "CyclicComponent");
}
