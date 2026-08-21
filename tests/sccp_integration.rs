//! Public-API integration coverage for sparse conditional constant propagation.

use libcpg::{
    sccp, unreachable_code, CfgEdgeKind, CodePropertyGraph, ConstValue, CpgEdgeKind, CpgNode,
    CpgNodeKind, Language, LiteralKind, MethodSignature, NodeId, ScopeId, SourceRange, Visibility,
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
            visibility: Visibility::Private,
        }),
    }
}

fn block() -> CpgNodeKind {
    CpgNodeKind::Block {
        scope: ScopeId::GLOBAL,
    }
}

fn connect_ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild);
}

fn connect_cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId, kind: CfgEdgeKind) {
    cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind));
}

#[test]
fn public_sccp_pipeline_refines_plain_reachability_without_mutating_the_cpg() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = cpg.add_node_with_id(node(10, function("public_pipeline")));
    let guard = cpg.add_node_with_id(node(
        20,
        CpgNodeKind::Literal {
            kind: LiteralKind::Bool(false),
        },
    ));
    let true_target = cpg.add_node_with_id(node(30, block()));
    let false_target = cpg.add_node_with_id(node(40, block()));
    for child in [guard, true_target, false_target] {
        connect_ast(&mut cpg, function, child);
    }
    connect_cfg(&mut cpg, function, guard, CfgEdgeKind::Sequential);
    connect_cfg(&mut cpg, guard, true_target, CfgEdgeKind::ConditionalTrue);
    connect_cfg(&mut cpg, guard, false_target, CfgEdgeKind::ConditionalFalse);

    let before = (cpg.node_count(), cpg.edge_count());
    let constants = sccp(&cpg, function).expect("public SCCP analysis");
    let plain = unreachable_code(&cpg, function).expect("plain reachability");
    let refined = plain
        .refine_with_sccp(&constants)
        .expect("same-function SCCP refinement");

    assert_eq!(
        constants.constants.get(&guard),
        Some(&ConstValue::Bool(false))
    );
    assert_eq!(constants.unreachable_nodes, vec![true_target]);
    assert_eq!(refined.unreachable, vec![true_target]);
    assert!(!refined.is_reachable(true_target));
    assert!(refined.is_reachable(false_target));
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}

#[test]
fn public_sccp_handles_a_function_without_cfg_edges() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = cpg.add_node_with_id(node(7, function("empty")));
    let result = sccp(&cpg, function).expect("empty function is valid");
    assert!(result.constants.is_empty());
    assert!(result.executable_edges.is_empty());
    assert!(result.dead_edges.is_empty());
    assert!(result.unreachable_nodes.is_empty());
}

#[cfg(feature = "lang-rust")]
#[test]
fn parser_built_cfg_and_dfg_prove_a_rust_constant_branch() {
    use libcpg::{CpgBuilder, TreeSitterCpgBuilder};

    let source = r#"
        fn choose() {
            let x = 1 + 2;
            if x == 3 {
                let selected = 4;
            } else {
                let discarded = 5;
            }
        }
    "#;
    let cpg = TreeSitterCpgBuilder::new()
        .build(source, Language::Rust)
        .expect("build parser-backed CPG");
    let function = cpg.functions().next().expect("choose function").id;
    let result = sccp(&cpg, function).expect("parser-backed SCCP");

    let x_definition = cpg
        .nodes()
        .find(|node| matches!(&node.kind, CpgNodeKind::Variable { name, .. } if &**name == "x"))
        .expect("x definition")
        .id;
    let addition = cpg
        .nodes()
        .find(|node| matches!(&node.kind, CpgNodeKind::BinaryOp { operator } if &**operator == "+"))
        .expect("addition")
        .id;
    let comparison = cpg
        .nodes()
        .find(
            |node| matches!(&node.kind, CpgNodeKind::BinaryOp { operator } if &**operator == "=="),
        )
        .expect("comparison")
        .id;
    assert_eq!(result.constants.get(&addition), Some(&ConstValue::Int(3)));
    assert_eq!(
        result.constants.get(&x_definition),
        Some(&ConstValue::Int(3))
    );
    assert_eq!(
        result.constants.get(&comparison),
        Some(&ConstValue::Bool(true))
    );
    assert!(result.dead_edges.iter().any(|edge| {
        matches!(edge.2, CfgEdgeKind::ConditionalFalse)
            && cpg
                .ast_descendants(edge.1)
                .into_iter()
                .chain(std::iter::once(edge.1))
                .filter_map(|node| cpg.node(node))
                .any(|node| {
                    matches!(
                        &node.kind,
                        CpgNodeKind::Variable { name, .. } if &**name == "discarded"
                    )
                })
    }));
}
