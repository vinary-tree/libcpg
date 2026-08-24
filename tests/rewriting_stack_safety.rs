#![cfg(feature = "metta")]

use libcpg::{
    dependency_pairs, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, Language,
    MethodSignature, NodeId, SourceRange, TerminationVerdict, Visibility,
};

fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(id), kind, SourceRange::default())
}

fn function(name: &str) -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: Box::new(MethodSignature {
            name: name.into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Public,
        }),
    }
}

fn identifier(name: &str) -> CpgNodeKind {
    CpgNodeKind::Identifier {
        name: name.into(),
        definition: None,
    }
}

#[test]
fn dependency_analysis_handles_one_hundred_thousand_nested_terms_on_a_small_stack() {
    const DEPTH: u32 = 100_000;
    let worker = std::thread::Builder::new()
        .name("rewrite-flat-arena-small-stack".into())
        .stack_size(64 * 1024)
        .spawn(|| {
            let mut cpg = CodePropertyGraph::new(Language::MeTTa);
            cpg.add_node_with_id(node(0, function("f")));
            cpg.add_node_with_id(node(1, CpgNodeKind::Unknown { kind: "=".into() }));
            cpg.add_node_with_id(node(
                2,
                CpgNodeKind::Call {
                    target: None,
                    is_method: false,
                },
            ));
            cpg.add_node_with_id(node(3, identifier("f")));
            cpg.add_node_with_id(node(4, identifier("Zero")));

            for level in 0..DEPTH {
                let call = 5 + level * 2;
                cpg.add_node_with_id(node(
                    call,
                    CpgNodeKind::Call {
                        target: None,
                        is_method: false,
                    },
                ));
                cpg.add_node_with_id(node(call + 1, identifier("S")));
            }
            let terminal = 5 + DEPTH * 2;
            cpg.add_node_with_id(node(terminal, identifier("Zero")));

            cpg.connect(NodeId::new(0), NodeId::new(1), CpgEdgeKind::AstChild);
            cpg.connect(NodeId::new(0), NodeId::new(2), CpgEdgeKind::AstChild);
            cpg.connect(NodeId::new(0), NodeId::new(5), CpgEdgeKind::AstChild);
            cpg.connect(NodeId::new(2), NodeId::new(3), CpgEdgeKind::AstChild);
            cpg.connect(NodeId::new(2), NodeId::new(4), CpgEdgeKind::AstChild);

            for level in 0..DEPTH {
                let call = 5 + level * 2;
                let child = if level + 1 == DEPTH {
                    terminal
                } else {
                    call + 2
                };
                cpg.connect(
                    NodeId::new(call),
                    NodeId::new(call + 1),
                    CpgEdgeKind::AstChild,
                );
                cpg.connect(NodeId::new(call), NodeId::new(child), CpgEdgeKind::AstChild);
            }

            let graph = dependency_pairs(&cpg);
            assert!(graph.complete, "unexpected issues: {:?}", graph.issues);
            assert!(graph.pairs.is_empty());
            assert!(matches!(
                libcpg::terminates(&cpg),
                TerminationVerdict::Terminates(_)
            ));
        })
        .expect("spawn small-stack rewrite worker");

    worker
        .join()
        .expect("flat rewrite analysis must not overflow");
}
