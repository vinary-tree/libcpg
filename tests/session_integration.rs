//! Public-API and real-frontend integration tests for session sketches.

#![cfg(feature = "rholang")]

use libcpg::{
    session_sketches, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, Language, LiteralKind,
    MethodSignature, MsgKind, NodeId, ScopeId, SessionConformance, SessionIssueKind,
    SessionPayloadType, SourceRange, TypeInfo, Visibility,
};

fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(
        NodeId::new(id),
        kind,
        SourceRange::from_bytes(id * 2, id * 2 + 1),
    )
}

fn function(id: u32, name: &str) -> CpgNode {
    node(
        id,
        CpgNodeKind::Function {
            signature: MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Public,
            },
        },
    )
}

fn event(id: u32, kind: MsgKind) -> CpgNode {
    node(
        id,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    )
    .with_message_kind(kind)
}

fn connect_ast(cpg: &mut CodePropertyGraph, parent: u32, child: u32) {
    cpg.connect(
        NodeId::new(parent),
        NodeId::new(child),
        CpgEdgeKind::AstChild,
    );
}

#[test]
fn root_api_proves_a_typed_binary_oracle_conformant() {
    let mut cpg = CodePropertyGraph::new(Language::Rholang);
    for item in [
        node(
            1,
            CpgNodeKind::Variable {
                name: "channel".into(),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: false,
            },
        ),
        function(10, "producer"),
        node(
            11,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        ),
        event(100, MsgKind::Send),
        node(
            101,
            CpgNodeKind::Identifier {
                name: "channel".into(),
                definition: Some(NodeId::new(1)),
            },
        ),
        node(
            102,
            CpgNodeKind::Literal {
                kind: LiteralKind::Integer(42),
            },
        ),
        function(20, "consumer"),
        node(
            21,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        ),
        event(200, MsgKind::Recv),
        node(
            201,
            CpgNodeKind::Identifier {
                name: "channel".into(),
                definition: Some(NodeId::new(1)),
            },
        ),
        node(
            202,
            CpgNodeKind::Parameter {
                name: "value".into(),
                param_type: Some(TypeInfo::new("Int")),
                is_variadic: false,
            },
        ),
    ] {
        cpg.add_node_with_id(item);
    }
    for (parent, child) in [
        (10, 11),
        (11, 100),
        (100, 101),
        (100, 102),
        (20, 21),
        (21, 200),
        (200, 201),
        (200, 202),
    ] {
        connect_ast(&mut cpg, parent, child);
    }

    let before = (cpg.node_count(), cpg.edge_count());
    let sketches = session_sketches(&cpg);
    assert_eq!(sketches.len(), 1);
    assert_eq!(sketches[0].conformance, SessionConformance::Conformant);
    assert_eq!(
        sketches[0].endpoints[0].actions[0].payload_types,
        vec![SessionPayloadType::Integer]
    );
    assert_eq!(
        sketches[0].endpoints[1].actions[0].payload_types,
        vec![SessionPayloadType::Integer]
    );
    assert!(sketches[0].issues.is_empty());
    assert_eq!(before, (cpg.node_count(), cpg.edge_count()));
}

#[test]
fn real_rholang_graph_yields_input_backed_advisory_evidence() {
    let source = r#"
contract @"main"() = {
  new channel in {
    channel!(42) |
    for (@value <- channel) { Nil }
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

    let sketches = session_sketches(&cpg);
    assert_eq!(sketches.len(), 1);
    let sketch = &sketches[0];
    assert_eq!(sketch.endpoints.len(), 1);
    assert_eq!(sketch.conformance, SessionConformance::Inconclusive);
    assert_eq!(sketch.issues.len(), 1);
    assert_eq!(sketch.issues[0].kind, SessionIssueKind::EndpointCardinality);
    assert_eq!(
        sketch.endpoints[0]
            .actions
            .iter()
            .map(|action| action.message_kind)
            .collect::<Vec<_>>(),
        vec![MsgKind::Send, MsgKind::Recv]
    );
    assert!(sketch
        .endpoints
        .iter()
        .all(|endpoint| cpg.contains_node(endpoint.process)));
    assert!(sketch
        .endpoints
        .iter()
        .flat_map(|endpoint| &endpoint.actions)
        .all(|action| cpg.contains_node(action.site)));
}
