//! Public-API integration tests for may-happen-in-parallel analysis.

use libcpg::{
    may_happen_in_parallel, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, Language,
    MsgKind, NodeId, ScopeId, SourceRange,
};

fn block(id: u32, parallel: bool) -> CpgNode {
    let node = CpgNode::new(
        NodeId::new(id),
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        },
        SourceRange::from_bytes(id, id + 1),
    );
    if parallel {
        node.with_message_kind(MsgKind::Spawn)
    } else {
        node
    }
}

fn call(id: u32) -> CpgNode {
    CpgNode::new(
        NodeId::new(id),
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
        SourceRange::from_bytes(id, id + 1),
    )
}

fn connect_ast(cpg: &mut CodePropertyGraph, parent: u32, child: u32) {
    cpg.connect(
        NodeId::new(parent),
        NodeId::new(child),
        CpgEdgeKind::AstChild,
    );
    cpg.node_mut(NodeId::new(child))
        .expect("child exists")
        .parent = Some(NodeId::new(parent));
}

#[test]
fn feature_free_root_api_returns_a_symmetric_read_only_relation() {
    let mut cpg = CodePropertyGraph::new(Language::Rholang);
    for node in [
        block(10, true),
        block(20, false),
        block(30, false),
        call(21),
        call(22),
        call(31),
    ] {
        cpg.add_node_with_id(node);
    }
    for (parent, child) in [(10, 20), (10, 30), (20, 21), (20, 22), (30, 31)] {
        connect_ast(&mut cpg, parent, child);
    }

    let before = (cpg.node_count(), cpg.edge_count());
    let relation = may_happen_in_parallel(&cpg);
    assert_eq!(relation.len(), 2);
    assert!(relation.contains(NodeId::new(21), NodeId::new(31)));
    assert!(relation.contains(NodeId::new(31), NodeId::new(21)));
    assert!(!relation.contains(NodeId::new(21), NodeId::new(22)));
    assert_eq!(before, (cpg.node_count(), cpg.edge_count()));
}

#[cfg(feature = "rholang")]
#[test]
fn rholang_frontend_marks_parallel_send_operands_without_analyzer_source_parsing() {
    let source = r#"
contract @"main"() = {
  new left, right in {
    left!(1) | right!?(2).
  }
}
"#;
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rholang::LANGUAGE.into())
        .expect("set Rholang grammar");
    let tree = parser.parse(source, None).expect("parse Rholang");
    assert!(!tree.root_node().has_error(), "fixture must parse cleanly");
    let cpg = libcpg::TreeSitterCpgBuilder::new()
        .build_from_tree(&tree, source, Language::Rholang)
        .expect("build Rholang CPG");

    let events = libcpg::extract_events(&cpg);
    let process_events = events.values().next().expect("main contract events");
    let send = process_events
        .iter()
        .find(|event| event.kind == MsgKind::Send)
        .expect("asynchronous send")
        .site;
    let send_sync = process_events
        .iter()
        .find(|event| event.kind == MsgKind::SendSync)
        .expect("synchronous send")
        .site;

    let relation = may_happen_in_parallel(&cpg);
    assert!(
        relation.contains(send, send_sync),
        "send={send:?} send_sync={send_sync:?} relation={relation:#?} spawn_nodes={:#?}",
        cpg.nodes()
            .filter(|node| node.message_kind == Some(MsgKind::Spawn))
            .map(|node| (node.id, node.kind.clone(), cpg.ast_children(node.id)))
            .collect::<Vec<_>>()
    );
    assert!(relation.pairs.iter().all(|pair| {
        cpg.contains_node(pair.first) && cpg.contains_node(pair.second) && pair.first < pair.second
    }));
}
