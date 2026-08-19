//! Public-API integration tests for loop invariants and induction variables.

#[cfg(feature = "lang-rust")]
use std::collections::BTreeSet;

use libcpg::{
    loop_forest, loop_induction, CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind,
    DfgEdgeKind, InductionVariable, IvDirection, Language, MethodSignature, NodeId, ScopeId,
    SourceRange, TripEvidence, Visibility,
};

fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(id), kind, SourceRange::default())
}

fn function(name: &str) -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: MethodSignature {
            name: name.into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        },
    }
}

fn variable(name: &str) -> CpgNodeKind {
    CpgNodeKind::Variable {
        name: name.into(),
        var_type: None,
        scope: ScopeId::GLOBAL,
        is_mutable: true,
    }
}

fn identifier(name: &str) -> CpgNodeKind {
    CpgNodeKind::Identifier {
        name: name.into(),
        definition: None,
    }
}

fn integer(value: i64) -> CpgNodeKind {
    CpgNodeKind::Literal {
        kind: libcpg::LiteralKind::Integer(value),
    }
}

fn binary(operator: &str) -> CpgNodeKind {
    CpgNodeKind::BinaryOp {
        operator: operator.into(),
    }
}

fn block() -> CpgNodeKind {
    CpgNodeKind::Block {
        scope: ScopeId::GLOBAL,
    }
}

fn ast(cpg: &mut CodePropertyGraph, parent: u32, child: u32) {
    let parent = NodeId::new(parent);
    let child = NodeId::new(child);
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("AST endpoints exist");
    cpg.node_mut(child).expect("child exists").parent = Some(parent);
}

fn cfg(cpg: &mut CodePropertyGraph, source: u32, target: u32, kind: CfgEdgeKind) {
    cpg.connect(
        NodeId::new(source),
        NodeId::new(target),
        CpgEdgeKind::ControlFlow(kind),
    )
    .expect("CFG endpoints exist");
}

fn def_use(cpg: &mut CodePropertyGraph, definition: u32, use_site: u32) {
    cpg.connect(
        NodeId::new(definition),
        NodeId::new(use_site),
        CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
    )
    .expect("DFG endpoints exist");
}

#[test]
fn root_api_analyzes_semantic_range_loops_without_language_features_or_mutation() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    for (id, kind) in [
        (0, function("range")),
        (1, variable("x")),
        (2, CpgNodeKind::For),
        (3, variable("i")),
        (4, binary("..")),
        (5, integer(0)),
        (6, identifier("x")),
        (7, block()),
        (8, identifier("i")),
        (9, CpgNodeKind::Return),
    ] {
        cpg.add_node_with_id(node(id, kind));
    }
    for (parent, child) in [
        (0, 1),
        (0, 2),
        (0, 9),
        (2, 3),
        (2, 4),
        (2, 7),
        (4, 5),
        (4, 6),
        (7, 8),
    ] {
        ast(&mut cpg, parent, child);
    }
    for (source, target, kind) in [
        (0, 1, CfgEdgeKind::Sequential),
        (1, 2, CfgEdgeKind::Sequential),
        (2, 7, CfgEdgeKind::ConditionalTrue),
        (7, 2, CfgEdgeKind::LoopBack),
        (2, 9, CfgEdgeKind::LoopExit),
    ] {
        cfg(&mut cpg, source, target, kind);
    }
    def_use(&mut cpg, 1, 6);
    def_use(&mut cpg, 3, 8);

    let before = (cpg.node_count(), cpg.edge_count());
    let forest = loop_forest(&cpg, NodeId::new(0)).expect("natural loop");
    let analysis = loop_induction(&cpg, NodeId::new(0), &forest).expect("induction analysis");
    assert_eq!(
        analysis.ivs,
        vec![InductionVariable {
            loop_id: 0,
            variable: "i".into(),
            update_node: NodeId::new(2),
            step: Some(1),
            direction: IvDirection::Increasing,
            init_defs: vec![NodeId::new(3)],
        }]
    );
    assert_eq!(
        analysis.bounds[0].trip,
        TripEvidence::Counted {
            iv: "i".into(),
            comparison: NodeId::new(4),
            bound_node: NodeId::new(6),
        }
    );
    assert!(analysis.invariants[0]
        .hoistable
        .iter()
        .all(|node| analysis.invariants[0].invariant_nodes.contains(node)));
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}

#[test]
fn root_api_recognizes_compound_basic_iv_and_direction_compatible_guard() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    for (id, kind) in [
        (0, function("while_counted")),
        (1, variable("limit")),
        (2, variable("i")),
        (3, identifier("i")),
        (4, integer(0)),
        (5, CpgNodeKind::While),
        (6, binary("<=")),
        (7, identifier("i")),
        (8, identifier("limit")),
        (9, block()),
        (
            10,
            CpgNodeKind::Assignment {
                operator: "+=".into(),
            },
        ),
        (11, identifier("i")),
        (12, integer(2)),
        (13, CpgNodeKind::Return),
    ] {
        cpg.add_node_with_id(node(id, kind));
    }
    for (parent, child) in [
        (0, 1),
        (0, 2),
        (0, 5),
        (0, 13),
        (2, 3),
        (2, 4),
        (5, 6),
        (5, 9),
        (6, 7),
        (6, 8),
        (9, 10),
        (10, 11),
        (10, 12),
    ] {
        ast(&mut cpg, parent, child);
    }
    for (source, target, kind) in [
        (0, 1, CfgEdgeKind::Sequential),
        (1, 2, CfgEdgeKind::Sequential),
        (2, 5, CfgEdgeKind::Sequential),
        (5, 10, CfgEdgeKind::ConditionalTrue),
        (10, 5, CfgEdgeKind::LoopBack),
        (5, 13, CfgEdgeKind::LoopExit),
    ] {
        cfg(&mut cpg, source, target, kind);
    }
    def_use(&mut cpg, 1, 8);
    for definition in [2, 10] {
        def_use(&mut cpg, definition, 7);
        def_use(&mut cpg, definition, 11);
    }

    let forest = loop_forest(&cpg, NodeId::new(0)).expect("while forest");
    let analysis = loop_induction(&cpg, NodeId::new(0), &forest).expect("while analysis");
    let iv = analysis
        .ivs
        .iter()
        .find(|iv| &*iv.variable == "i")
        .expect("basic i IV");
    assert_eq!(iv.step, Some(2));
    assert_eq!(iv.direction, IvDirection::Increasing);
    assert_eq!(iv.init_defs, vec![NodeId::new(2)]);
    assert!(matches!(
        analysis.bounds[0].trip,
        TripEvidence::Counted { .. }
    ));
}

#[test]
fn root_api_evaluates_a_hundred_thousand_unary_steps_on_a_small_native_stack() {
    const UNARY_DEPTH: u32 = 100_000;
    const FUNCTION: u32 = 0;
    const INITIAL: u32 = 1;
    const INITIAL_NAME: u32 = 2;
    const INITIAL_VALUE: u32 = 3;
    const HEADER: u32 = 4;
    const BODY: u32 = 5;
    const UPDATE: u32 = 6;
    const TARGET: u32 = 7;
    const RETURN: u32 = 8;
    const LITERAL: u32 = 9;
    const FIRST_UNARY: u32 = 10;
    const LAST_UNARY: u32 = FIRST_UNARY + UNARY_DEPTH - 1;

    std::thread::Builder::new()
        .name("induction-expression-small-stack".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            for (id, kind) in [
                (FUNCTION, function("deep_step")),
                (INITIAL, variable("i")),
                (INITIAL_NAME, identifier("i")),
                (INITIAL_VALUE, integer(0)),
                (HEADER, CpgNodeKind::While),
                (BODY, block()),
                (
                    UPDATE,
                    CpgNodeKind::Assignment {
                        operator: "+=".into(),
                    },
                ),
                (TARGET, identifier("i")),
                (RETURN, CpgNodeKind::Return),
                (LITERAL, integer(1)),
            ] {
                cpg.add_node_with_id(node(id, kind));
            }
            // Parent-before-child ids model the pre-order allocation used by
            // ordinary AST builders. Stack safety and summary reuse must not
            // depend on a favorable child-before-parent numbering scheme.
            for id in FIRST_UNARY..=LAST_UNARY {
                cpg.add_node_with_id(node(
                    id,
                    CpgNodeKind::UnaryOp {
                        operator: "-".into(),
                    },
                ));
            }

            for (parent, child) in [
                (FUNCTION, INITIAL),
                (FUNCTION, HEADER),
                (FUNCTION, RETURN),
                (INITIAL, INITIAL_NAME),
                (INITIAL, INITIAL_VALUE),
                (HEADER, BODY),
                (BODY, UPDATE),
                (UPDATE, TARGET),
                (UPDATE, FIRST_UNARY),
            ] {
                ast(&mut cpg, parent, child);
            }
            for id in FIRST_UNARY..=LAST_UNARY {
                let child = if id == LAST_UNARY { LITERAL } else { id + 1 };
                ast(&mut cpg, id, child);
            }

            for (source, target, kind) in [
                (FUNCTION, INITIAL, CfgEdgeKind::Sequential),
                (INITIAL, HEADER, CfgEdgeKind::Sequential),
                (HEADER, UPDATE, CfgEdgeKind::ConditionalTrue),
                (UPDATE, FIRST_UNARY, CfgEdgeKind::Sequential),
            ] {
                cfg(&mut cpg, source, target, kind);
            }
            for id in FIRST_UNARY..=LAST_UNARY {
                let target = if id == LAST_UNARY { LITERAL } else { id + 1 };
                cfg(&mut cpg, id, target, CfgEdgeKind::Sequential);
            }
            cfg(&mut cpg, LITERAL, HEADER, CfgEdgeKind::LoopBack);
            cfg(&mut cpg, HEADER, RETURN, CfgEdgeKind::LoopExit);
            cpg.add_cfg_entry(NodeId::new(FUNCTION));
            cpg.add_cfg_exit(NodeId::new(RETURN));

            for definition in [INITIAL, UPDATE] {
                def_use(&mut cpg, definition, TARGET);
            }

            let before = (cpg.node_count(), cpg.edge_count());
            let forest = loop_forest(&cpg, NodeId::new(FUNCTION)).expect("deep loop forest");
            let analysis = loop_induction(&cpg, NodeId::new(FUNCTION), &forest)
                .expect("stack-safe induction analysis");
            let iv = analysis
                .ivs
                .iter()
                .find(|iv| &*iv.variable == "i")
                .expect("deep unary step retains the basic IV");

            assert_eq!(iv.update_node, NodeId::new(UPDATE));
            assert_eq!(iv.step, Some(1));
            assert_eq!(iv.direction, IvDirection::Increasing);
            assert_eq!(iv.init_defs, vec![NodeId::new(INITIAL)]);
            assert_eq!((cpg.node_count(), cpg.edge_count()), before);
        })
        .expect("spawn small-stack induction test")
        .join()
        .expect("induction analysis must not overflow the native stack");
}

#[cfg(feature = "lang-rust")]
#[test]
fn parser_generated_rust_range_loop_satisfies_the_acceptance_oracle() {
    use libcpg::{variable_facts, CpgBuilder, TreeSitterCpgBuilder};

    let source = r#"
        fn consume(_: usize) {}
        fn counted(x: usize) {
            for i in 0..x {
                consume(i);
            }
        }
    "#;
    let cpg = TreeSitterCpgBuilder::new()
        .build(source, Language::Rust)
        .expect("build Rust CFG and DFG");
    let function = cpg
        .functions()
        .find(|node| matches!(&node.kind, CpgNodeKind::Function { signature } if &*signature.name == "counted"))
        .expect("counted function")
        .id;
    let before = (cpg.node_count(), cpg.edge_count());
    let forest = loop_forest(&cpg, function).expect("parser-built loop forest");
    let analysis = loop_induction(&cpg, function, &forest).expect("parser-built induction");

    let iv = analysis
        .ivs
        .iter()
        .find(|iv| &*iv.variable == "i")
        .expect("Rust range binder is an IV");
    assert_eq!(iv.step, Some(1));
    assert_eq!(iv.direction, IvDirection::Increasing);
    assert!(forest.loops[iv.loop_id].body.contains(&iv.update_node));
    assert!(matches!(
        analysis.bounds[iv.loop_id].trip,
        TripEvidence::Counted { ref iv, .. } if &**iv == "i"
    ));

    let range = cpg
        .nodes()
        .find(
            |node| matches!(&node.kind, CpgNodeKind::BinaryOp { operator } if &**operator == ".."),
        )
        .expect("semantic range expression")
        .id;
    let for_header = cpg
        .ast_ancestors(range)
        .into_iter()
        .find(|node| {
            matches!(
                cpg.node(*node).map(|node| &node.kind),
                Some(CpgNodeKind::For)
            )
        })
        .expect("range belongs to for header");
    let binder = cpg
        .ast_children(for_header)
        .into_iter()
        .find(|node| matches!(cpg.node(*node).map(|node| &node.kind), Some(CpgNodeKind::Variable { name, .. }) if &**name == "i"))
        .expect("semantic for binding");
    assert_eq!(iv.init_defs, vec![binder]);
    assert_eq!(iv.update_node, for_header);

    let body_region: BTreeSet<_> = forest.loops[iv.loop_id]
        .body
        .iter()
        .copied()
        .flat_map(|node| std::iter::once(node).chain(cpg.ast_descendants(node)))
        .collect();
    let facts = variable_facts(&cpg, function);
    let invariant: BTreeSet<_> = analysis.invariants[iv.loop_id]
        .invariant_nodes
        .iter()
        .copied()
        .collect();
    for &candidate in &analysis.invariants[iv.loop_id].invariant_nodes {
        for use_site in std::iter::once(candidate)
            .chain(cpg.ast_descendants(candidate))
            .filter(|node| !facts.uses_at(*node).is_empty())
        {
            let reaching: Vec<_> = cpg
                .dfg_predecessors(use_site)
                .into_iter()
                .filter_map(|(source, kind)| (kind == DfgEdgeKind::DefUse).then_some(source))
                .collect();
            assert!(!reaching.is_empty());
            assert!(reaching
                .iter()
                .all(|source| !body_region.contains(source) || invariant.contains(source)));
        }
    }
    assert!(analysis.invariants[iv.loop_id]
        .hoistable
        .iter()
        .all(|node| invariant.contains(node)));
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}
