#![cfg(feature = "rholang")]

//! Public and parser-backed integration tests for Rholang NameFlow analysis.

use libcpg::{
    analyze_deadlock_freedom, analyze_name_flow, deadlock_freedom, materialize_name_flow_overlay,
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, DeadlockWitness, Language,
    MethodSignature, MsgKind, NameFlowEdgeKind, NameOperationKind, NodeId, ScopeId, SourceRange,
    Visibility,
};

fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(
        NodeId::new(id),
        kind,
        SourceRange::from_bytes(id, id.saturating_add(1)),
    )
}

fn function(id: u32) -> CpgNode {
    node(
        id,
        CpgNodeKind::Function {
            signature: Box::new(MethodSignature {
                name: format!("p{id}").into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Public,
            }),
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

fn identifier(id: u32, definition: u32) -> CpgNode {
    node(
        id,
        CpgNodeKind::Identifier {
            name: format!("c{definition}").into(),
            definition: Some(NodeId::new(definition)),
        },
    )
}

fn connect_ast(cpg: &mut CodePropertyGraph, parent: u32, child: u32) {
    cpg.connect(
        NodeId::new(parent),
        NodeId::new(child),
        CpgEdgeKind::AstChild,
    );
}

fn add_fresh(cpg: &mut CodePropertyGraph, restriction: u32, id: u32) {
    cpg.add_node_with_id(
        node(
            id,
            CpgNodeKind::Variable {
                name: format!("c{id}").into(),
                var_type: None,
                scope: ScopeId::new(restriction),
                is_mutable: false,
            },
        )
        .with_name_operation(NameOperationKind::FreshBinder),
    );
    connect_ast(cpg, restriction, id);
}

fn add_event(cpg: &mut CodePropertyGraph, parent: u32, site: u32, kind: MsgKind, channel: u32) {
    cpg.add_node_with_id(event(site, kind));
    cpg.add_node_with_id(identifier(site + 1, channel));
    connect_ast(cpg, parent, site);
    connect_ast(cpg, site, site + 1);
}

#[test]
fn public_pipeline_materializes_typed_overlay_and_level_witnesses() {
    let mut cpg = CodePropertyGraph::new(Language::Rholang);
    cpg.add_node_with_id(
        node(
            1,
            CpgNodeKind::Block {
                scope: ScopeId::new(1),
            },
        )
        .with_name_operation(NameOperationKind::Restriction),
    );
    add_fresh(&mut cpg, 1, 10);
    add_fresh(&mut cpg, 1, 20);
    for process in [30, 40] {
        cpg.add_node_with_id(function(process));
        connect_ast(&mut cpg, 1, process);
    }
    add_event(&mut cpg, 30, 100, MsgKind::Recv, 10);
    add_event(&mut cpg, 30, 110, MsgKind::Send, 20);
    add_event(&mut cpg, 40, 200, MsgKind::Recv, 20);
    add_event(&mut cpg, 40, 210, MsgKind::Send, 10);

    let name_flow = analyze_name_flow(&cpg);
    assert!(name_flow.complete, "issues={:#?}", name_flow.issues);
    let added = materialize_name_flow_overlay(&mut cpg, &name_flow).expect("valid NameFlow");
    assert_eq!(added, name_flow.relations.len());
    assert!(cpg.name_flow_edges().all(|edge| edge.kind.is_name_flow()));
    assert!(cpg.name_flow_edges().any(|edge| matches!(
        edge.kind,
        CpgEdgeKind::NameFlow(NameFlowEdgeKind::Communicates)
    )));

    let analysis = analyze_deadlock_freedom(&cpg);
    assert!(analysis.complete, "issues={:#?}", analysis.issues);
    assert_eq!(
        analysis.cyclic_channels,
        vec![NodeId::new(10), NodeId::new(20)]
    );
    assert_eq!(deadlock_freedom(&cpg), analysis.witnesses);
    assert!(analysis
        .witnesses
        .iter()
        .all(|witness| matches!(witness, DeadlockWitness::ObligationCapability { .. })));
}

#[test]
fn real_rholang_frontend_retains_restriction_and_finds_mutual_wait() {
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

    assert_eq!(
        cpg.nodes()
            .filter(|node| node.name_operation == Some(NameOperationKind::Restriction))
            .count(),
        1
    );
    assert_eq!(
        cpg.nodes()
            .filter(|node| node.name_operation == Some(NameOperationKind::FreshBinder))
            .count(),
        2
    );
    let analysis = analyze_deadlock_freedom(&cpg);
    assert!(analysis.complete, "issues={:#?}", analysis.issues);
    assert_eq!(analysis.cyclic_channels.len(), 2);
    assert_eq!(analysis.witnesses.len(), 2);
}

#[test]
fn public_pipeline_is_constant_stack_at_one_hundred_thousand_ast_levels() {
    const DEPTH: u32 = 100_000;
    std::thread::Builder::new()
        .name("name-flow-depth-100000".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut cpg = CodePropertyGraph::new(Language::Rholang);
            cpg.add_node_with_id(
                node(
                    1,
                    CpgNodeKind::Block {
                        scope: ScopeId::new(1),
                    },
                )
                .with_name_operation(NameOperationKind::Restriction),
            );
            add_fresh(&mut cpg, 1, 10);
            add_fresh(&mut cpg, 1, 20);
            cpg.add_node_with_id(function(30));
            cpg.add_node_with_id(function(40));
            connect_ast(&mut cpg, 1, 30);
            connect_ast(&mut cpg, 1, 40);

            let first_wrapper = 1_000;
            let mut parent = 30;
            for offset in 0..DEPTH {
                let wrapper = first_wrapper + offset;
                cpg.add_node_with_id(node(
                    wrapper,
                    CpgNodeKind::Block {
                        scope: ScopeId::new(wrapper),
                    },
                ));
                connect_ast(&mut cpg, parent, wrapper);
                parent = wrapper;
            }
            let left_receive = first_wrapper + DEPTH + 10;
            let left_send = left_receive + 10;
            add_event(&mut cpg, parent, left_receive, MsgKind::Recv, 10);
            add_event(&mut cpg, parent, left_send, MsgKind::Send, 20);
            let right_receive = left_send + 100;
            let right_send = right_receive + 10;
            add_event(&mut cpg, 40, right_receive, MsgKind::Recv, 20);
            add_event(&mut cpg, 40, right_send, MsgKind::Send, 10);

            assert!(cpg.ast_depth() >= DEPTH as usize);
            let name_flow = analyze_name_flow(&cpg);
            assert!(name_flow.complete, "issues={:#?}", name_flow.issues);
            let analysis = analyze_deadlock_freedom(&cpg);
            assert!(analysis.complete, "issues={:#?}", analysis.issues);
            assert_eq!(analysis.witnesses.len(), 2);
            materialize_name_flow_overlay(&mut cpg, &name_flow).expect("valid deep overlay");
            assert_eq!(cpg.stats().name_flow_edges, name_flow.relations.len());
            drop(analysis);
            drop(name_flow);
            drop(cpg);
        })
        .expect("spawn small-stack integration thread")
        .join()
        .expect("NameFlow pipeline must not consume logical depth as native stack");
}
