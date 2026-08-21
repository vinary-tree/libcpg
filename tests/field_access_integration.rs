//! Public-surface integration tests for resolved field/index data flow.

use libcpg::{
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, DfgEdgeKind, DfgExtractor,
    DfgExtractorConfig, Language, MethodSignature, NodeId, ScopeId, SourceRange, Visibility,
};

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let id = cpg
        .add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()).with_parent(parent));
    cpg.connect(parent, id, CpgEdgeKind::AstChild);
    id
}

fn has_edge(cpg: &CodePropertyGraph, source: NodeId, target: NodeId, kind: &CpgEdgeKind) -> bool {
    cpg.edges_between(source, target)
        .into_iter()
        .any(|edge| &edge.kind == kind)
}

fn class_assignment_fixture() -> (CodePropertyGraph, NodeId, NodeId, NodeId, NodeId) {
    let mut cpg = CodePropertyGraph::new(Language::Java);
    let class = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Class {
            name: "Counter".into(),
            is_abstract: false,
        },
        SourceRange::default(),
    ));
    let class_body = child(
        &mut cpg,
        class,
        CpgNodeKind::Unknown {
            kind: "class_body".into(),
        },
    );
    let field = child(
        &mut cpg,
        class_body,
        CpgNodeKind::Field {
            name: "value".into(),
            field_type: None,
            visibility: Visibility::Private,
        },
    );
    let method = child(
        &mut cpg,
        class_body,
        CpgNodeKind::Function {
            signature: MethodSignature {
                name: "set".into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Public,
            },
        },
    );
    let body = child(
        &mut cpg,
        method,
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        },
    );
    let assignment = child(
        &mut cpg,
        body,
        CpgNodeKind::Assignment {
            operator: "=".into(),
        },
    );
    let access = child(
        &mut cpg,
        assignment,
        CpgNodeKind::MemberAccess {
            member: "value".into(),
        },
    );
    let receiver = child(
        &mut cpg,
        access,
        CpgNodeKind::Unknown {
            kind: "this".into(),
        },
    );
    child(
        &mut cpg,
        assignment,
        CpgNodeKind::Literal {
            kind: libcpg::LiteralKind::Integer(1),
        },
    );
    (cpg, method, field, assignment, receiver)
}

#[test]
fn public_field_access_contract_is_gated_additive_and_idempotent() {
    let (cpg, method, field, assignment, receiver) = class_assignment_fixture();
    let access = cpg.ast_children(assignment)[0];

    let mut disabled = cpg.clone();
    DfgExtractor::with_config(DfgExtractorConfig {
        include_field_access: false,
        ..DfgExtractorConfig::default()
    })
    .extract_function_dfg(&mut disabled, method);
    assert!(!disabled.edges().any(|edge| matches!(
        edge.kind,
        CpgEdgeKind::Reference
            | CpgEdgeKind::DataFlow(DfgEdgeKind::FieldRead | DfgEdgeKind::FieldWrite)
    )));

    let mut enabled = cpg;
    let nodes_before = enabled.node_count();
    let edges_before = enabled.edge_count();
    DfgExtractor::new().extract_function_dfg(&mut enabled, method);
    assert_eq!(enabled.node_count(), nodes_before);
    assert_eq!(enabled.edge_count(), edges_before + 3);
    assert_eq!(enabled.stats().dfg_edges, 2);
    assert!(has_edge(
        &enabled,
        receiver,
        access,
        &CpgEdgeKind::DataFlow(DfgEdgeKind::FieldRead)
    ));
    assert!(has_edge(
        &enabled,
        assignment,
        access,
        &CpgEdgeKind::DataFlow(DfgEdgeKind::FieldWrite)
    ));
    assert!(has_edge(&enabled, access, field, &CpgEdgeKind::Reference));

    let edges_after_first = enabled.edge_count();
    DfgExtractor::new().extract_function_dfg(&mut enabled, method);
    assert_eq!(enabled.edge_count(), edges_after_first);
}

#[test]
#[cfg(feature = "lang-rust")]
fn mode_a_rust_self_assignment_resolves_to_declared_field() {
    use libcpg::{CpgBuilder, TreeSitterCpgBuilder};

    let cpg = TreeSitterCpgBuilder::new()
        .build(
            "struct S { x: i32 } impl S { fn set(&mut self) { self.x = 1; } }",
            Language::Rust,
        )
        .expect("Rust Mode-A build");
    let field = cpg
        .nodes()
        .find(|node| matches!(&node.kind, CpgNodeKind::Field { name, .. } if name.as_ref() == "x"))
        .expect("declared field")
        .id;
    let access = cpg
        .nodes()
        .find(|node| {
            matches!(&node.kind, CpgNodeKind::MemberAccess { member } if member.as_ref() == "x")
        })
        .expect("member access")
        .id;
    let assignment = cpg
        .nodes()
        .find(|node| matches!(node.kind, CpgNodeKind::Assignment { .. }))
        .expect("assignment")
        .id;

    assert!(has_edge(
        &cpg,
        assignment,
        access,
        &CpgEdgeKind::DataFlow(DfgEdgeKind::FieldWrite)
    ));
    assert!(has_edge(&cpg, access, field, &CpgEdgeKind::Reference));
}
