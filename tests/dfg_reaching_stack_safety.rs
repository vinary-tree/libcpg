//! Public-API stack-safety gate for AST-ordered reaching definitions.

use libcpg::{
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, DfgEdgeKind, DfgExtractor, Language,
    MethodSignature, NodeId, ScopeId, SourceRange, Visibility,
};

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let node = cpg
        .add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()).with_parent(parent));
    cpg.connect_unique(parent, node, CpgEdgeKind::AstChild);
    node
}

#[test]
fn public_reaching_definitions_cross_depth_100_000_on_a_small_stack() {
    const DEPTH: usize = 100_000;

    std::thread::Builder::new()
        .name("dfg-reaching-small-stack".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            let function = cpg.add_node(CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::Function {
                    signature: MethodSignature {
                        name: "deep".into(),
                        params: Default::default(),
                        return_type: None,
                        is_static: false,
                        is_async: false,
                        visibility: Visibility::Private,
                    },
                },
                SourceRange::default(),
            ));
            let body = child(
                &mut cpg,
                function,
                CpgNodeKind::Block {
                    scope: ScopeId::GLOBAL,
                },
            );
            let definition = child(
                &mut cpg,
                body,
                CpgNodeKind::Variable {
                    name: "value".into(),
                    var_type: None,
                    scope: ScopeId::GLOBAL,
                    is_mutable: false,
                },
            );

            let mut parent = body;
            for _ in 0..DEPTH {
                parent = child(
                    &mut cpg,
                    parent,
                    CpgNodeKind::Block {
                        scope: ScopeId::GLOBAL,
                    },
                );
            }
            let use_site = child(
                &mut cpg,
                parent,
                CpgNodeKind::Identifier {
                    name: "value".into(),
                    definition: None,
                },
            );

            let node_count = cpg.node_count();
            let ast_edges = cpg
                .edges()
                .filter(|edge| edge.kind == CpgEdgeKind::AstChild)
                .count();

            DfgExtractor::new().extract_function_dfg(&mut cpg, function);

            assert_eq!(cpg.reaching_definitions(use_site), vec![definition]);
            assert!(cpg
                .edges_between(definition, use_site)
                .iter()
                .any(|edge| { edge.kind == CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse) }));
            assert_eq!(cpg.node_count(), node_count);
            assert_eq!(
                cpg.edges()
                    .filter(|edge| edge.kind == CpgEdgeKind::AstChild)
                    .count(),
                ast_edges
            );
        })
        .expect("spawn DFG reaching-definitions small-stack test")
        .join()
        .expect("reaching-definitions machine completes on a small native stack");
}
