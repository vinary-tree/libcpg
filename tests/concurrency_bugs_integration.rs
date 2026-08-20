//! Public-API integration tests for advisory race and atomicity analysis.

use libcpg::{
    atomicity_candidates, may_happen_in_parallel, race_candidates, CodePropertyGraph,
    ConcurrentAccessKind, CpgEdgeKind, CpgNode, CpgNodeKind, DfgEdgeKind, Language, LockKind,
    MsgKind, NodeId, ScopeId, SourceRange, Visibility,
};

fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(id), kind, SourceRange::from_bytes(id, id + 1))
}

fn block(id: u32, parallel: bool) -> CpgNode {
    let node = node(
        id,
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        },
    );
    if parallel {
        node.with_message_kind(MsgKind::Spawn)
    } else {
        node
    }
}

fn call(id: u32) -> CpgNode {
    node(
        id,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    )
}

fn identifier(id: u32, definition: u32) -> CpgNode {
    node(
        id,
        CpgNodeKind::Identifier {
            name: format!("v{definition}").into(),
            definition: Some(NodeId::new(definition)),
        },
    )
}

fn add_ast(cpg: &mut CodePropertyGraph, parent: u32, child: u32) {
    cpg.connect(
        NodeId::new(parent),
        NodeId::new(child),
        CpgEdgeKind::AstChild,
    );
    cpg.node_mut(NodeId::new(child)).expect("child").parent = Some(NodeId::new(parent));
}

fn add_lock(cpg: &mut CodePropertyGraph, strand: u32, site: u32, kind: LockKind) {
    cpg.add_node_with_id(call(site).with_lock_kind(kind));
    cpg.add_node_with_id(identifier(site + 10_000, 200));
    add_ast(cpg, strand, site);
    add_ast(cpg, site, site + 10_000);
}

fn add_access(cpg: &mut CodePropertyGraph, strand: u32, site: u32, kind: DfgEdgeKind) {
    let expression = site + 1_000;
    cpg.add_node_with_id(call(site));
    cpg.add_node_with_id(node(
        expression,
        CpgNodeKind::MemberAccess {
            member: "value".into(),
        },
    ));
    add_ast(cpg, strand, site);
    add_ast(cpg, site, expression);
    cpg.connect(
        NodeId::new(site),
        NodeId::new(expression),
        CpgEdgeKind::DataFlow(kind),
    );
    cpg.connect(
        NodeId::new(expression),
        NodeId::new(100),
        CpgEdgeKind::Reference,
    );
}

fn public_fixture() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    for item in [
        block(1, true),
        block(10, false),
        block(20, false),
        node(
            100,
            CpgNodeKind::Field {
                name: "value".into(),
                field_type: None,
                visibility: Visibility::Private,
            },
        ),
        node(
            200,
            CpgNodeKind::Variable {
                name: "guard".into(),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: true,
            },
        ),
    ] {
        cpg.add_node_with_id(item);
    }
    add_ast(&mut cpg, 1, 10);
    add_ast(&mut cpg, 1, 20);

    add_lock(&mut cpg, 10, 11, LockKind::AcquireWrite);
    add_access(&mut cpg, 10, 12, DfgEdgeKind::FieldRead);
    add_lock(&mut cpg, 10, 13, LockKind::Release);
    add_lock(&mut cpg, 10, 14, LockKind::AcquireWrite);
    add_access(&mut cpg, 10, 15, DfgEdgeKind::FieldWrite);
    add_access(&mut cpg, 20, 22, DfgEdgeKind::FieldWrite);
    cpg
}

#[test]
fn public_feature_free_api_composes_mhp_locksets_races_and_atomicity() {
    let cpg = public_fixture();
    let before = (cpg.node_count(), cpg.edge_count());
    let mhp = may_happen_in_parallel(&cpg);

    let races = race_candidates(&cpg, &mhp);
    assert_eq!(races.len(), 2);
    assert!(races.iter().all(|candidate| {
        candidate.location == NodeId::new(100)
            && candidate.access_b == NodeId::new(22)
            && candidate.witness.kind_b == ConcurrentAccessKind::Write
    }));

    let atomicity = atomicity_candidates(&cpg, &mhp);
    assert_eq!(atomicity.len(), 1);
    assert_eq!(atomicity[0].read, NodeId::new(12));
    assert_eq!(atomicity[0].write, NodeId::new(15));
    assert_eq!(atomicity[0].witness.interfering_access, NodeId::new(22));
    assert_eq!(atomicity[0].witness.boundary_lock, NodeId::new(200));
    assert_eq!(before, (cpg.node_count(), cpg.edge_count()));
}
