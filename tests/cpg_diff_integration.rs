//! Public-API integration tests for deterministic CPG edit scripts.

use libcpg::{
    cpg_diff, ChangeKind, CodePropertyGraph, CpgDiffOptions, CpgEdge, CpgEdgeKind, CpgElement,
    CpgNode, CpgNodeKind, EdgeId, Language, NodeId, ScopeId, SourceRange,
};

fn renamed_fixture(new: bool) -> CodePropertyGraph {
    let mut graph = CodePropertyGraph::new(Language::Rust);
    let (root, binding, use_site) = if new { (101, 202, 303) } else { (1, 2, 3) };
    let spelling = if new { "total" } else { "sum" };
    graph.add_node_with_id(CpgNode::new(
        NodeId::new(root),
        CpgNodeKind::Root,
        SourceRange::default(),
    ));
    graph.add_node_with_id(
        CpgNode::new(
            NodeId::new(binding),
            CpgNodeKind::Variable {
                name: spelling.into(),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: false,
            },
            SourceRange::default(),
        )
        .with_text(spelling),
    );
    graph.add_node_with_id(
        CpgNode::new(
            NodeId::new(use_site),
            CpgNodeKind::Identifier {
                name: spelling.into(),
                definition: Some(NodeId::new(binding)),
            },
            SourceRange::default(),
        )
        .with_text(spelling),
    );
    for (id, source, target, kind) in [
        (1, root, binding, CpgEdgeKind::AstChild),
        (2, root, use_site, CpgEdgeKind::AstChild),
        (3, use_site, binding, CpgEdgeKind::Reference),
    ] {
        graph
            .add_edge_with_id(CpgEdge::new(
                EdgeId::new(id),
                NodeId::new(source),
                NodeId::new(target),
                kind,
            ))
            .expect("fixture endpoints exist");
    }
    graph
}

#[test]
fn feature_free_public_diff_distinguishes_rename_from_dependence_change() {
    let old = renamed_fixture(false);
    let renamed = renamed_fixture(true);
    let old_shape = (old.node_count(), old.edge_count());
    let renamed_shape = (renamed.node_count(), renamed.edge_count());

    let rename = cpg_diff(&old, &renamed, &CpgDiffOptions::default()).expect("rename diff");
    assert_eq!(rename.classification, ChangeKind::Refactoring);
    assert!(rename.added.is_empty());
    assert!(rename.removed.is_empty());
    assert_eq!(rename.changed.len(), 2);

    let mut guarded = renamed_fixture(true);
    let guard = guarded.add_node_with_id(CpgNode::new(
        NodeId::new(404),
        CpgNodeKind::If,
        SourceRange::default(),
    ));
    guarded
        .add_edge_with_id(CpgEdge::new(
            EdgeId::new(44),
            guard,
            NodeId::new(303),
            CpgEdgeKind::ControlDependence,
        ))
        .expect("guard dependence endpoints exist");
    let behavior = cpg_diff(&renamed, &guarded, &CpgDiffOptions::default()).expect("guard diff");
    assert_eq!(behavior.classification, ChangeKind::BehaviorChanging);
    assert!(behavior.added.iter().any(|element| matches!(
        element,
        CpgElement::Edge(edge) if edge.kind == CpgEdgeKind::ControlDependence
    )));
    assert_eq!((old.node_count(), old.edge_count()), old_shape);
    assert_eq!((renamed.node_count(), renamed.edge_count()), renamed_shape);
}

#[cfg(feature = "lang-rust")]
#[test]
fn parser_built_pdg_exposes_an_added_guard_as_behavior_changing() {
    use libcpg::{CpgBuilder, PdgBuilder, TreeSitterCpgBuilder};

    fn build(source: &str) -> CodePropertyGraph {
        let mut graph = TreeSitterCpgBuilder::new()
            .build(source, Language::Rust)
            .expect("parse Rust fixture");
        let function = graph
            .nodes()
            .find(|node| matches!(node.kind, CpgNodeKind::Function { .. }))
            .map(|node| node.id)
            .expect("function node");
        PdgBuilder::new().build(&mut graph, function);
        graph
    }

    let old = build("fn choose(x: i32) -> i32 { x + 1 }");
    let guarded = build("fn choose(x: i32) -> i32 { if x > 0 { x + 1 } else { 0 } }");
    assert!(guarded.edges().any(|edge| edge.kind.is_pdg()));

    let diff = cpg_diff(&old, &guarded, &CpgDiffOptions::default()).expect("parsed PDG diff");
    assert_eq!(diff.classification, ChangeKind::BehaviorChanging);
    assert!(
        diff.added.iter().any(|element| matches!(
            element,
            CpgElement::Edge(edge) if edge.kind.is_pdg()
        )) || diff.changed.iter().any(|change| matches!(
            change,
            libcpg::CpgChange::Edge { old, new, .. }
                if old.kind.is_pdg() || new.kind.is_pdg()
        ))
    );
}
