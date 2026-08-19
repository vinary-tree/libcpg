//! Feature-free I8 inconsistent-clone and I9 infeasible-path example.

use libcpg::{
    analyze_inconsistent_clones, analyze_infeasible_paths, BranchOutcome, CfgEdgeKind,
    CloneBugConfig, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, DfgEdgeKind,
    InfeasiblePathConfig, InfeasiblePathKind, Language, LiteralKind, MethodSignature, NodeId,
    ScopeId, SourceRange, Visibility,
};

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn add(cpg: &mut CodePropertyGraph, kind: CpgNodeKind) -> NodeId {
    cpg.add_node(node(kind))
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

fn ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("example AST endpoints exist");
}

fn cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId, kind: CfgEdgeKind) {
    cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind))
        .expect("example CFG endpoints exist");
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

fn clone_fixture() -> CodePropertyGraph {
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

fn feasibility_fixture() -> CodePropertyGraph {
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
            .expect("example DFG endpoints exist");
    }
    cfg(&mut cpg, owner, parameter, CfgEdgeKind::Sequential);
    cfg(&mut cpg, parameter, guard, CfgEdgeKind::Sequential);
    cfg(&mut cpg, guard, then_block, CfgEdgeKind::ConditionalTrue);
    cfg(&mut cpg, guard, else_block, CfgEdgeKind::ConditionalFalse);
    cpg
}

fn main() {
    let clones = analyze_inconsistent_clones(&clone_fixture(), &CloneBugConfig::default())
        .expect("the clone fixture and configuration are valid");
    assert!(clones.is_complete());
    assert_eq!(clones.findings.len(), 1);
    for finding in &clones.findings {
        println!(
            "clone {:?} vs {:?}: {:?}; AST edits {}/{}; GED {}",
            finding.left_function,
            finding.right_function,
            finding.kind,
            finding.structural_edits,
            finding.structural_size,
            finding.ged.upper,
        );
    }

    let feasibility =
        analyze_infeasible_paths(&feasibility_fixture(), &InfeasiblePathConfig::default())
            .expect("the feasibility fixture and configuration are valid");
    assert!(feasibility.is_complete());
    assert_eq!(feasibility.findings.len(), 3);
    assert!(feasibility.findings.iter().any(|finding| {
        matches!(
            finding.kind,
            InfeasiblePathKind::InfeasibleBranch {
                branch: BranchOutcome::True,
                ..
            }
        )
    }));
    for finding in &feasibility.findings {
        println!(
            "condition {:?}: {:?}, proof {:?}, {} empty interval alternative(s)",
            finding.condition,
            finding.kind,
            finding.proof,
            finding.interval_contradictions.len(),
        );
    }
}
