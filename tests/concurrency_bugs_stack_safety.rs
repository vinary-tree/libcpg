//! Small-stack regression for deep concurrency-evidence parent walks.

use libcpg::{
    atomicity_candidates, may_happen_in_parallel, race_candidates, CodePropertyGraph, CpgEdgeKind,
    CpgNode, CpgNodeKind, DfgEdgeKind, Language, LockKind, MsgKind, NodeId, ScopeId, SourceRange,
    Visibility,
};

const DEPTH: u32 = 100_000;

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

fn add_ast(cpg: &mut CodePropertyGraph, parent: u32, child: u32) {
    cpg.connect(
        NodeId::new(parent),
        NodeId::new(child),
        CpgEdgeKind::AstChild,
    );
}

fn add_lock(cpg: &mut CodePropertyGraph, strand: u32, site: u32, kind: LockKind) {
    cpg.add_node_with_id(call(site).with_lock_kind(kind));
    cpg.add_node_with_id(node(
        site + 1_000_000,
        CpgNodeKind::Identifier {
            name: "guard".into(),
            definition: Some(NodeId::new(900_000)),
        },
    ));
    add_ast(cpg, strand, site);
    add_ast(cpg, site, site + 1_000_000);
}

fn add_access(cpg: &mut CodePropertyGraph, strand: u32, site: u32, kind: DfgEdgeKind) {
    let expression = site + 2_000_000;
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
        NodeId::new(900_001),
        CpgEdgeKind::Reference,
    );
}

fn deep_fixture() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    for item in [
        block(1, true),
        block(10, false),
        block(20, false),
        node(
            900_000,
            CpgNodeKind::Variable {
                name: "guard".into(),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: true,
            },
        ),
        node(
            900_001,
            CpgNodeKind::Field {
                name: "value".into(),
                field_type: None,
                visibility: Visibility::Private,
            },
        ),
    ] {
        cpg.add_node_with_id(item);
    }
    add_ast(&mut cpg, 1, 10);
    add_ast(&mut cpg, 1, 20);

    let first = 1_000;
    cpg.add_node_with_id(block(first, false));
    add_ast(&mut cpg, 10, first);
    let mut parent = first;
    for offset in 1..DEPTH {
        let child = first + offset;
        cpg.add_node_with_id(block(child, false));
        add_ast(&mut cpg, parent, child);
        parent = child;
    }

    add_lock(&mut cpg, parent, 200_000, LockKind::AcquireWrite);
    add_access(&mut cpg, parent, 200_001, DfgEdgeKind::FieldRead);
    add_lock(&mut cpg, parent, 200_002, LockKind::Release);
    add_lock(&mut cpg, parent, 200_003, LockKind::AcquireWrite);
    add_access(&mut cpg, parent, 200_004, DfgEdgeKind::FieldWrite);
    add_access(&mut cpg, 20, 200_010, DfgEdgeKind::FieldWrite);
    cpg
}

#[test]
fn deep_parent_walks_remain_stack_safe_on_a_small_thread_stack() {
    std::thread::Builder::new()
        .name("concurrency-evidence-small-stack".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let cpg = deep_fixture();
            let mhp = may_happen_in_parallel(&cpg);
            let races = race_candidates(&cpg, &mhp);
            let atomicity = atomicity_candidates(&cpg, &mhp);
            assert_eq!(races.len(), 2);
            assert_eq!(atomicity.len(), 1);
        })
        .expect("spawn small-stack worker")
        .join()
        .expect("iterative detector must not overflow the native stack");
}
