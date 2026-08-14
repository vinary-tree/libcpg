//! Public-API integration tests for channel topology and linearity analysis.

use libcpg::{
    channel_report, extract_events, ChannelFindingKind, CodePropertyGraph, CpgEdgeKind, CpgNode,
    CpgNodeKind, DfgEdgeKind, Language, MethodSignature, MsgKind, NodeId, ScopeId, SourceRange,
    Visibility,
};

fn function(id: u32, name: &str) -> CpgNode {
    CpgNode::new(
        NodeId::new(id),
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
        SourceRange::default(),
    )
}

#[test]
fn public_api_links_distinct_spelling_uses_to_one_channel_definition() {
    let mut cpg = CodePropertyGraph::new(Language::Rholang);
    cpg.add_node_with_id(function(10, "producer"));
    cpg.add_node_with_id(function(20, "consumer"));
    cpg.add_node_with_id(CpgNode::new(
        NodeId::new(30),
        CpgNodeKind::Variable {
            name: "original".into(),
            var_type: None,
            scope: ScopeId::GLOBAL,
            is_mutable: false,
        },
        SourceRange::default(),
    ));
    for (site, use_site, process, spelling, kind) in [
        (40, 41, 10, "producer_alias", MsgKind::Send),
        (50, 51, 20, "consumer_alias", MsgKind::Recv),
    ] {
        cpg.add_node_with_id(
            CpgNode::new(
                NodeId::new(site),
                CpgNodeKind::Call {
                    target: None,
                    is_method: false,
                },
                SourceRange::from_bytes(site, site + 1),
            )
            .with_message_kind(kind),
        );
        cpg.add_node_with_id(CpgNode::new(
            NodeId::new(use_site),
            CpgNodeKind::Identifier {
                name: spelling.into(),
                definition: None,
            },
            SourceRange::from_bytes(use_site, use_site + 1),
        ));
        cpg.connect(
            NodeId::new(process),
            NodeId::new(site),
            CpgEdgeKind::AstChild,
        );
        cpg.connect(
            NodeId::new(site),
            NodeId::new(use_site),
            CpgEdgeKind::AstChild,
        );
        cpg.connect(
            NodeId::new(30),
            NodeId::new(use_site),
            CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
        );
    }

    let events = extract_events(&cpg);
    assert_eq!(events[&NodeId::new(10)][0].channel, Some(NodeId::new(30)));
    assert_eq!(events[&NodeId::new(20)][0].channel, Some(NodeId::new(30)));
    assert!(channel_report(&cpg).is_empty());
}

#[test]
fn public_api_reports_linear_contention_with_stable_evidence() {
    let mut cpg = CodePropertyGraph::new(Language::Rholang);
    for (process, kind, site) in [
        (10, MsgKind::Send, 100),
        (20, MsgKind::SendSync, 110),
        (30, MsgKind::Recv, 120),
    ] {
        cpg.add_node_with_id(function(process, &format!("p{process}")));
        cpg.add_node_with_id(
            CpgNode::new(
                NodeId::new(site),
                CpgNodeKind::Call {
                    target: None,
                    is_method: false,
                },
                SourceRange::from_bytes(site, site + 1),
            )
            .with_message_kind(kind),
        );
        cpg.add_node_with_id(CpgNode::new(
            NodeId::new(site + 1),
            CpgNodeKind::Identifier {
                name: "c".into(),
                definition: Some(NodeId::new(90)),
            },
            SourceRange::from_bytes(site + 1, site + 2),
        ));
        cpg.connect(
            NodeId::new(process),
            NodeId::new(site),
            CpgEdgeKind::AstChild,
        );
        cpg.connect(
            NodeId::new(site),
            NodeId::new(site + 1),
            CpgEdgeKind::AstChild,
        );
    }
    cpg.add_node_with_id(CpgNode::new(
        NodeId::new(90),
        CpgNodeKind::Variable {
            name: "c".into(),
            var_type: None,
            scope: ScopeId::GLOBAL,
            is_mutable: false,
        },
        SourceRange::default(),
    ));

    let findings = channel_report(&cpg);
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].kind,
        ChannelFindingKind::NondeterministicConsumption
    );
    assert_eq!(
        findings[0].sites,
        vec![NodeId::new(100), NodeId::new(110), NodeId::new(120)]
    );
}

#[cfg(feature = "rholang")]
#[test]
fn rholang_mode_b_normalizes_every_channel_syntax_without_source_parsing() {
    let source = r#"
contract @"main"() = {
  new c in {
    c!(1) |
    c!?(2). |
    for (@linear <- c) { Nil } |
    for (@persistent <= c) { Nil } |
    for (@observed <<- c) { Nil } |
    select { @selected <- c => Nil }
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

    let events = extract_events(&cpg);
    assert_eq!(events.len(), 1);
    let events = events.values().next().expect("main contract events");
    for kind in [
        MsgKind::Send,
        MsgKind::SendSync,
        MsgKind::Recv,
        MsgKind::RecvPersistent,
        MsgKind::Peek,
        MsgKind::Select,
        MsgKind::Spawn,
    ] {
        assert!(
            events.iter().any(|event| event.kind == kind),
            "missing normalized {kind:?} event"
        );
    }
    let channels: std::collections::BTreeSet<_> = events
        .iter()
        .filter(|event| event.kind.uses_channel())
        .map(|event| event.channel.expect("typed def-use channel identity"))
        .collect();
    assert_eq!(channels.len(), 1, "all operations resolve to new-bound c");
}
