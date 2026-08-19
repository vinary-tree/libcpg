//! Public-API, parser-pipeline, and invariance gates for T6.4.

use libcpg::{
    analyze_inconsistent_clones, analyze_infeasible_paths, inconsistent_clone_bugs,
    infeasible_paths, BranchOutcome, CfgEdgeKind, CloneBugConfig, CloneGuardValue,
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, DfgEdgeKind, InconsistentCloneKind,
    InfeasiblePathConfig, InfeasiblePathKind, Language, LiteralKind, MethodSignature, NodeId,
    ScopeId, SourceRange, Visibility,
};

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
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

fn add(cpg: &mut CodePropertyGraph, kind: CpgNodeKind) -> NodeId {
    cpg.add_node(node(kind))
}

fn ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("integration AST endpoints exist");
}

fn cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId, kind: CfgEdgeKind) {
    cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind))
        .expect("integration CFG endpoints exist");
}

fn identifier(cpg: &mut CodePropertyGraph, name: &str) -> NodeId {
    add(
        cpg,
        CpgNodeKind::Identifier {
            name: name.into(),
            definition: None,
        },
    )
}

fn integer(cpg: &mut CodePropertyGraph, value: i64) -> NodeId {
    add(
        cpg,
        CpgNodeKind::Literal {
            kind: LiteralKind::Integer(value),
        },
    )
}

fn guard_clone_fixture() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    for (name, operator) in [("left", ">"), ("right", "<")] {
        let owner = add(&mut cpg, function(name));
        let guard = add(
            &mut cpg,
            CpgNodeKind::BinaryOp {
                operator: operator.into(),
            },
        );
        let operand = identifier(&mut cpg, "x");
        let limit = integer(&mut cpg, 5);
        let then_block = add(
            &mut cpg,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        let else_block = add(
            &mut cpg,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        for child in [guard, then_block, else_block] {
            ast(&mut cpg, owner, child);
        }
        ast(&mut cpg, guard, operand);
        ast(&mut cpg, guard, limit);
        cfg(&mut cpg, guard, then_block, CfgEdgeKind::ConditionalTrue);
        cfg(&mut cpg, guard, else_block, CfgEdgeKind::ConditionalFalse);
    }
    cpg
}

fn contradiction_fixture() -> (CodePropertyGraph, NodeId, NodeId) {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let owner = add(&mut cpg, function("impossible"));
    let parameter = add(
        &mut cpg,
        CpgNodeKind::Parameter {
            name: "x".into(),
            param_type: None,
            is_variadic: false,
        },
    );
    let guard = add(
        &mut cpg,
        CpgNodeKind::BinaryOp {
            operator: "&&".into(),
        },
    );
    let greater = add(
        &mut cpg,
        CpgNodeKind::BinaryOp {
            operator: ">".into(),
        },
    );
    let greater_use = identifier(&mut cpg, "x");
    let five = integer(&mut cpg, 5);
    let less = add(
        &mut cpg,
        CpgNodeKind::BinaryOp {
            operator: "<".into(),
        },
    );
    let less_use = identifier(&mut cpg, "x");
    let three = integer(&mut cpg, 3);
    let then_block = add(
        &mut cpg,
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        },
    );
    let else_block = add(
        &mut cpg,
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        },
    );
    for child in [parameter, guard, then_block, else_block] {
        ast(&mut cpg, owner, child);
    }
    for child in [greater, less] {
        ast(&mut cpg, guard, child);
    }
    for (comparison, usage, limit) in [(greater, greater_use, five), (less, less_use, three)] {
        ast(&mut cpg, comparison, usage);
        ast(&mut cpg, comparison, limit);
        cpg.connect(parameter, usage, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
            .expect("integration DFG endpoints exist");
    }
    cfg(&mut cpg, owner, parameter, CfgEdgeKind::Sequential);
    cfg(&mut cpg, parameter, guard, CfgEdgeKind::Sequential);
    cfg(&mut cpg, guard, then_block, CfgEdgeKind::ConditionalTrue);
    cfg(&mut cpg, guard, else_block, CfgEdgeKind::ConditionalFalse);
    (cpg, guard, then_block)
}

#[test]
fn public_advisory_detectors_report_exact_witnesses_without_mutating_the_cpg() {
    let clones = guard_clone_fixture();
    let clone_before = (clones.node_count(), clones.edge_count());
    let clone_report =
        analyze_inconsistent_clones(&clones, &CloneBugConfig::default()).expect("clone report");
    assert!(clone_report.is_complete(), "{clone_report:#?}");
    assert_eq!(clone_report.findings.len(), 1, "{clone_report:#?}");
    assert_eq!(
        clone_report.findings[0].kind,
        InconsistentCloneKind::GuardDivergence {
            left: CloneGuardValue::BinaryOperator(">".into()),
            right: CloneGuardValue::BinaryOperator("<".into()),
        }
    );
    assert_eq!(
        inconsistent_clone_bugs(&clones, &CloneBugConfig::default()).expect("finding projection"),
        clone_report.findings
    );
    assert_eq!((clones.node_count(), clones.edge_count()), clone_before);

    let (feasibility, guard, true_target) = contradiction_fixture();
    let feasibility_before = (feasibility.node_count(), feasibility.edge_count());
    let feasibility_report =
        analyze_infeasible_paths(&feasibility, &InfeasiblePathConfig::default())
            .expect("feasibility report");
    assert!(feasibility_report.is_complete(), "{feasibility_report:#?}");
    assert!(feasibility_report.findings.iter().any(|finding| {
        finding.source == guard
            && finding.kind
                == InfeasiblePathKind::InfeasibleBranch {
                    branch: BranchOutcome::True,
                    target: true_target,
                }
    }));
    assert!(feasibility_report
        .findings
        .iter()
        .any(|finding| finding.kind == InfeasiblePathKind::ContradictoryCondition));
    assert!(feasibility_report.findings.iter().any(|finding| {
        finding.kind == InfeasiblePathKind::RedundantCondition { value: false }
    }));
    assert_eq!(
        infeasible_paths(&feasibility, &InfeasiblePathConfig::default())
            .expect("finding projection"),
        feasibility_report.findings
    );
    assert_eq!(
        (feasibility.node_count(), feasibility.edge_count()),
        feasibility_before
    );
}

#[cfg(feature = "lang-rust")]
#[test]
fn parser_built_rust_cpg_exposes_clone_and_feasibility_advisories() {
    use libcpg::{CpgBuilder, TreeSitterCpgBuilder};

    let source = r#"
        fn clone_left(x: i32) -> bool {
            if x > 5 { true } else { false }
        }
        fn clone_right(x: i32) -> bool {
            if x < 5 { true } else { false }
        }
        fn impossible(x: i32) -> i32 {
            if x > 5 && x < 3 { 1 } else { 0 }
        }
        fn redundant() -> i32 {
            if true { 1 } else { 0 }
        }
    "#;
    let cpg = TreeSitterCpgBuilder::new()
        .build(source, Language::Rust)
        .expect("parser-built Rust CPG");
    let before = (cpg.node_count(), cpg.edge_count());

    let clone_report =
        analyze_inconsistent_clones(&cpg, &CloneBugConfig::default()).expect("clone report");
    let feasibility_report = analyze_infeasible_paths(&cpg, &InfeasiblePathConfig::default())
        .expect("feasibility report");
    let function_name = |function| cpg.node(function).and_then(|node| node.name());

    assert!(clone_report.is_complete(), "{clone_report:#?}");
    assert!(
        clone_report.findings.iter().any(|finding| {
            function_name(finding.left_function) == Some("clone_left")
                && function_name(finding.right_function) == Some("clone_right")
                && finding.kind
                    == InconsistentCloneKind::GuardDivergence {
                        left: CloneGuardValue::BinaryOperator(">".into()),
                        right: CloneGuardValue::BinaryOperator("<".into()),
                    }
        }),
        "{clone_report:#?}"
    );

    assert!(feasibility_report.is_complete(), "{feasibility_report:#?}");
    assert!(
        feasibility_report.findings.iter().any(|finding| {
            function_name(finding.function) == Some("impossible")
                && finding.kind == InfeasiblePathKind::ContradictoryCondition
        }),
        "{feasibility_report:#?}"
    );
    assert!(
        feasibility_report.findings.iter().any(|finding| {
            function_name(finding.function) == Some("redundant")
                && finding.kind == InfeasiblePathKind::RedundantCondition { value: true }
        }),
        "{feasibility_report:#?}"
    );
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}
