//! Public-API integration tests for Martin package metrics.

use libcpg::{
    martin_metrics, sdp_violations, CodePropertyGraph, CpgNode, CpgNodeKind, GraphProjection,
    Language, ModuleAssignment, NodeId, ProjectionKind, SourceRange,
};

fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(id), kind, SourceRange::default())
}

#[test]
fn feature_free_public_api_is_exact_deterministic_and_read_only() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    for (id, kind) in [
        (101, CpgNodeKind::Module { name: "api".into() }),
        (
            307,
            CpgNodeKind::Module {
                name: "domain".into(),
            },
        ),
        (911, CpgNodeKind::Module { name: "app".into() }),
        (
            103,
            CpgNodeKind::Trait {
                name: "Port".into(),
            },
        ),
        (
            311,
            CpgNodeKind::Class {
                name: "Entity".into(),
                is_abstract: false,
            },
        ),
        (
            919,
            CpgNodeKind::Struct {
                name: "Main".into(),
            },
        ),
    ] {
        cpg.add_node_with_id(node(id, kind));
    }
    let assignment = ModuleAssignment::from_pairs([
        (NodeId::new(101), "api"),
        (NodeId::new(103), "api"),
        (NodeId::new(307), "domain"),
        (NodeId::new(311), "domain"),
        (NodeId::new(911), "app"),
        (NodeId::new(919), "app"),
    ]);
    let edges = [
        (NodeId::new(101), NodeId::new(307), 1.0),
        (NodeId::new(101), NodeId::new(911), 2.0),
        (NodeId::new(307), NodeId::new(911), 3.0),
        (NodeId::new(911), NodeId::new(101), 4.0),
    ];
    let view = GraphProjection::from_edges(
        ProjectionKind::External,
        [NodeId::new(911), NodeId::new(101), NodeId::new(307)],
        edges,
    );
    let shape = (cpg.node_count(), cpg.edge_count());
    let metrics = martin_metrics(&cpg, &assignment, &view);
    let repeated = martin_metrics(&cpg, &assignment, &view);

    assert_eq!(metrics, repeated);
    assert_eq!(
        metrics
            .iter()
            .map(|metric| metric.module.as_ref())
            .collect::<Vec<_>>(),
        ["api", "app", "domain"]
    );
    let api = &metrics[0];
    assert_eq!((api.ca, api.ce, api.abstractness), (1, 2, 1.0));
    let app = &metrics[1];
    assert_eq!((app.ca, app.ce, app.abstractness), (2, 1, 0.0));
    let domain = &metrics[2];
    assert_eq!((domain.ca, domain.ce, domain.abstractness), (1, 1, 0.0));

    let violations = sdp_violations(&metrics, &view, &assignment);
    assert_eq!(violations.len(), 1);
    assert_eq!(violations[0].source_module.as_ref(), "app");
    assert_eq!(violations[0].target_module.as_ref(), "api");
    assert_eq!(violations[0].source, NodeId::new(911));
    assert_eq!(violations[0].target, NodeId::new(101));
    assert_eq!((cpg.node_count(), cpg.edge_count()), shape);
}

#[cfg(feature = "lang-rust")]
#[test]
fn parsed_rust_modules_supply_qualified_buckets_and_real_abstractness() {
    use libcpg::{CpgBuilder, TreeSitterCpgBuilder};

    let cpg = TreeSitterCpgBuilder::new()
        .build(
            "mod api { pub trait Port {} mod nested { pub enum Event { Ready } } }\n\
             mod implementation { pub struct Adapter; }",
            Language::Rust,
        )
        .expect("parse Rust module fixture");
    let assignment = ModuleAssignment::by_module_nodes(&cpg);
    let module_nodes: Vec<_> = cpg
        .nodes()
        .filter(|node| matches!(node.kind, CpgNodeKind::Module { .. }))
        .map(|node| node.id)
        .collect();
    let view = GraphProjection::from_edges(
        ProjectionKind::ModuleGraph,
        module_nodes,
        std::iter::empty(),
    );
    let shape = (cpg.node_count(), cpg.edge_count());
    let metrics = martin_metrics(&cpg, &assignment, &view);

    let api = metrics
        .iter()
        .find(|metric| metric.module.as_ref() == "api")
        .expect("api module metric");
    assert_eq!(api.abstractness, 1.0, "one trait and no concrete type");
    let nested = metrics
        .iter()
        .find(|metric| metric.module.as_ref() == "api::nested")
        .expect("qualified nested module metric");
    assert_eq!(nested.abstractness, 0.0, "enum is concrete");
    let implementation = metrics
        .iter()
        .find(|metric| metric.module.as_ref() == "implementation")
        .expect("implementation module metric");
    assert_eq!(implementation.abstractness, 0.0, "struct is concrete");
    assert_eq!((cpg.node_count(), cpg.edge_count()), shape);
}
