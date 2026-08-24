//! Public-API integration tests for communication and lock-order deadlocks.

use libcpg::{
    communication_deadlocks, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind,
    DeadlockFindingKind, DeadlockWitness, Language, LockKind, MethodSignature, MsgKind, NodeId,
    ScopeId, SourceRange, Visibility,
};

fn function(id: u32, name: &str) -> CpgNode {
    CpgNode::new(
        NodeId::new(id),
        CpgNodeKind::Function {
            signature: Box::new(MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Public,
            }),
        },
        SourceRange::from_bytes(id, id + 1),
    )
}

fn variable(id: u32, name: &str) -> CpgNode {
    CpgNode::new(
        NodeId::new(id),
        CpgNodeKind::Variable {
            name: name.into(),
            var_type: None,
            scope: ScopeId::GLOBAL,
            is_mutable: false,
        },
        SourceRange::from_bytes(id, id + 1),
    )
}

fn add_typed_event(
    cpg: &mut CodePropertyGraph,
    process: u32,
    site: u32,
    resource: u32,
    message: Option<MsgKind>,
    lock: Option<LockKind>,
) {
    let mut call = CpgNode::new(
        NodeId::new(site),
        CpgNodeKind::Call {
            target: None,
            is_method: lock.is_some(),
        },
        SourceRange::from_bytes(site, site + 1),
    );
    call.message_kind = message;
    call.lock_kind = lock;
    cpg.add_node_with_id(call);
    cpg.add_node_with_id(CpgNode::new(
        NodeId::new(site + 1),
        CpgNodeKind::Identifier {
            name: format!("r{resource}").into(),
            definition: Some(NodeId::new(resource)),
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

#[test]
fn public_api_reports_mutual_channel_wait_with_valid_ranges() {
    let mut cpg = CodePropertyGraph::new(Language::Rholang);
    for node in [
        function(10, "p1"),
        function(20, "p2"),
        variable(100, "c1"),
        variable(200, "c2"),
    ] {
        cpg.add_node_with_id(node);
    }
    add_typed_event(&mut cpg, 10, 1_000, 100, Some(MsgKind::Recv), None);
    add_typed_event(&mut cpg, 10, 1_010, 200, Some(MsgKind::Send), None);
    add_typed_event(&mut cpg, 20, 2_000, 200, Some(MsgKind::Recv), None);
    add_typed_event(&mut cpg, 20, 2_010, 100, Some(MsgKind::SendSync), None);

    let findings = communication_deadlocks(&cpg);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, DeadlockFindingKind::CommunicationCycle);
    assert_eq!(
        findings[0].processes,
        vec![NodeId::new(10), NodeId::new(20)]
    );
    assert!(findings[0].witnesses.iter().all(|witness| match witness {
        DeadlockWitness::Communication {
            wait_site,
            wait_range,
            producer_site,
            producer_range,
            ..
        } => {
            cpg.contains_node(*wait_site)
                && cpg.contains_node(*producer_site)
                && *wait_range == cpg.node(*wait_site).expect("wait").range
                && *producer_range == cpg.node(*producer_site).expect("send").range
        }
        #[cfg(feature = "rholang")]
        DeadlockWitness::ObligationCapability { .. } => false,
        DeadlockWitness::LockOrder { .. } => false,
    }));
}

#[test]
fn public_api_reports_typed_ab_ba_lock_cycle_without_name_matching() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    for node in [
        function(10, "left"),
        function(20, "right"),
        variable(100, "alpha"),
        variable(200, "beta"),
    ] {
        cpg.add_node_with_id(node);
    }
    add_typed_event(&mut cpg, 10, 1_000, 100, None, Some(LockKind::AcquireWrite));
    add_typed_event(&mut cpg, 10, 1_010, 200, None, Some(LockKind::AcquireWrite));
    add_typed_event(&mut cpg, 20, 2_000, 200, None, Some(LockKind::AcquireWrite));
    add_typed_event(&mut cpg, 20, 2_010, 100, None, Some(LockKind::AcquireWrite));

    let findings = communication_deadlocks(&cpg);
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].kind, DeadlockFindingKind::LockOrderCycle);
    assert_eq!(
        findings[0].resources,
        vec![NodeId::new(100), NodeId::new(200)]
    );
    assert!(findings[0]
        .witnesses
        .iter()
        .all(|witness| matches!(witness, DeadlockWitness::LockOrder { .. })));
}

#[cfg(feature = "rholang")]
#[test]
fn rholang_frontend_drives_mutual_wait_without_analyzer_source_parsing() {
    let source = r#"
new c1, c2 in {
  contract c1() = { for (@x <- c2) { c1!(x) } } |
  contract c2() = { for (@y <- c1) { c2!(y) } }
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
    let findings = communication_deadlocks(&cpg);
    assert_eq!(
        findings.len(),
        1,
        "mutual initial receives must cycle; events={events:#?}"
    );
    assert_eq!(findings[0].kind, DeadlockFindingKind::CommunicationCycle);
    assert_eq!(findings[0].processes.len(), 2);
    assert_eq!(findings[0].resources.len(), 2);
}
