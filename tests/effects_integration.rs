use libcpg::{
    analyze_effects, call_graph_sccs, effect_summaries, CodePropertyGraph, ConstValue, CpgEdgeKind,
    CpgNode, CpgNodeKind, IfdsCaps, Language, LiteralKind, MethodSignature, NodeId, Place, Purity,
    ScopeId, SourceRange, Visibility,
};

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

fn add(cpg: &mut CodePropertyGraph, kind: CpgNodeKind) -> NodeId {
    cpg.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()))
}

fn connect_ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("public fixture AST endpoints exist");
}

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let child = add(cpg, kind);
    connect_ast(cpg, parent, child);
    child
}

#[test]
fn public_api_reports_a_complete_constant_summary() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add(&mut cpg, function("answer"));
    let returned = child(&mut cpg, function, CpgNodeKind::Return);
    let value = child(
        &mut cpg,
        returned,
        CpgNodeKind::Literal {
            kind: LiteralKind::Integer(42),
        },
    );

    let call_graph = call_graph_sccs(&cpg);
    let analysis = analyze_effects(&cpg, &call_graph, &IfdsCaps::default());
    assert!(analysis.is_complete());
    assert_eq!(analysis.summaries[&function].purity, Purity::Pure);
    assert_eq!(
        analysis.summaries[&function].const_return,
        Some(ConstValue::Int(42))
    );
    assert_eq!(
        effect_summaries(&cpg, &call_graph, &IfdsCaps::default()),
        analysis.summaries
    );
    assert_eq!(cpg.ast_parent(value), Some(returned));
}

#[test]
fn public_api_maps_a_callee_parameter_write_to_the_callers_global() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let global = add(
        &mut cpg,
        CpgNodeKind::Variable {
            name: "STATE".into(),
            var_type: None,
            scope: ScopeId::new(0),
            is_mutable: true,
        },
    );
    let caller = add(&mut cpg, function("caller"));
    let callee = add(&mut cpg, function("mutate"));
    let parameter = child(
        &mut cpg,
        callee,
        CpgNodeKind::Parameter {
            name: "value".into(),
            param_type: None,
            is_variadic: false,
        },
    );
    let assignment = child(
        &mut cpg,
        callee,
        CpgNodeKind::Assignment {
            operator: "=".into(),
        },
    );
    child(
        &mut cpg,
        assignment,
        CpgNodeKind::Identifier {
            name: "value".into(),
            definition: Some(parameter),
        },
    );
    child(
        &mut cpg,
        assignment,
        CpgNodeKind::Literal {
            kind: LiteralKind::Integer(1),
        },
    );

    let argument = add(
        &mut cpg,
        CpgNodeKind::Identifier {
            name: "STATE".into(),
            definition: Some(global),
        },
    );
    let call = child(
        &mut cpg,
        caller,
        CpgNodeKind::Call {
            target: Some(callee),
            is_method: false,
        },
    );
    let callee_syntax = child(
        &mut cpg,
        call,
        CpgNodeKind::Identifier {
            name: "mutate".into(),
            definition: Some(callee),
        },
    );
    connect_ast(&mut cpg, call, argument);

    let analysis = analyze_effects(&cpg, &call_graph_sccs(&cpg), &IfdsCaps::default());
    assert!(analysis.is_complete());
    assert_eq!(analysis.summaries[&callee].purity, Purity::Pure);
    assert_eq!(analysis.summaries[&caller].purity, Purity::WritesGlobal);
    assert!(analysis.summaries[&caller]
        .writes
        .contains(&Place::Global("STATE".into())));
    assert_eq!(cpg.ast_parent(callee_syntax), Some(call));
}

#[test]
fn public_effect_evaluators_handle_depth_100_000_on_a_small_stack() {
    std::thread::Builder::new()
        .name("effect-evaluation-small-stack".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            const DEPTH: usize = 100_000;
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            let function = add(&mut cpg, function("deep_identity"));
            let parameter = child(
                &mut cpg,
                function,
                CpgNodeKind::Parameter {
                    name: "value".into(),
                    param_type: None,
                    is_variadic: false,
                },
            );
            let mut expression = add(
                &mut cpg,
                CpgNodeKind::Identifier {
                    name: "value".into(),
                    definition: Some(parameter),
                },
            );
            for _ in 0..DEPTH {
                let parent = add(
                    &mut cpg,
                    CpgNodeKind::UnaryOp {
                        operator: "-".into(),
                    },
                );
                connect_ast(&mut cpg, parent, expression);
                expression = parent;
            }
            let returned = child(&mut cpg, function, CpgNodeKind::Return);
            connect_ast(&mut cpg, returned, expression);

            let caps = IfdsCaps {
                max_call_depth: u32::try_from(DEPTH + 1).expect("depth fits u32"),
                max_worklist_steps: 1_000_000,
                ..IfdsCaps::default()
            };
            let analysis = analyze_effects(&cpg, &call_graph_sccs(&cpg), &caps);
            assert!(analysis.is_complete(), "{analysis:?}");
            let summary = &analysis.summaries[&function];
            assert_eq!(summary.param_to_return, 1);
            assert!(summary.const_return_overdefined);
            assert!(!summary.capped);
            assert_eq!(cpg.ast_parent(expression), Some(returned));
        })
        .expect("spawn small-stack effect test")
        .join()
        .expect("small-stack effect analysis");
}
