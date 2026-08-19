use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::{
    analyze_inconsistent_clones, analyze_infeasible_paths, CfgEdgeKind, CloneBugConfig,
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, DfgEdgeKind, InfeasiblePathConfig,
    InfeasiblePathKind, Language, LiteralKind, MethodSignature, NodeId, ScopeId, SourceRange,
    Visibility,
};

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn add(cpg: &mut CodePropertyGraph, kind: CpgNodeKind) -> NodeId {
    cpg.add_node(node(kind))
}

fn function(name: String) -> CpgNodeKind {
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
        .expect("benchmark AST endpoints exist");
}

fn cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId, kind: CfgEdgeKind) {
    cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind))
        .expect("benchmark CFG endpoints exist");
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

fn clone_fixture(function_count: usize) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    for index in 0..function_count {
        let owner = add(&mut cpg, function(format!("clone_{index}")));
        let guard = add(
            &mut cpg,
            CpgNodeKind::BinaryOp {
                operator: if index % 2 == 0 { ">" } else { "<" }.into(),
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

fn feasibility_fixture(function_count: usize) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    for index in 0..function_count {
        let owner = add(&mut cpg, function(format!("impossible_{index}")));
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
                .expect("benchmark DFG endpoints exist");
        }
        cfg(&mut cpg, owner, parameter, CfgEdgeKind::Sequential);
        cfg(&mut cpg, parameter, guard, CfgEdgeKind::Sequential);
        cfg(&mut cpg, guard, then_block, CfgEdgeKind::ConditionalTrue);
        cfg(&mut cpg, guard, else_block, CfgEdgeKind::ConditionalFalse);
    }
    cpg
}

fn benchmark_advisory_bugs(c: &mut Criterion) {
    let clone_config = CloneBugConfig::default();
    let infeasible_config = InfeasiblePathConfig::default();
    let mut group = c.benchmark_group("advisory_bugs");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    for function_count in [8usize, 32] {
        let clones = clone_fixture(function_count);
        let clone_oracle = analyze_inconsistent_clones(&clones, &clone_config)
            .expect("valid clone benchmark fixture");
        assert!(clone_oracle.is_complete());
        assert!(!clone_oracle.findings.is_empty());
        group.throughput(Throughput::Elements(function_count as u64));
        group.bench_with_input(
            BenchmarkId::new("inconsistent_clones", function_count),
            &clones,
            |benchmark, graph| {
                benchmark.iter(|| {
                    black_box(
                        analyze_inconsistent_clones(black_box(graph), black_box(&clone_config))
                            .expect("valid clone benchmark fixture"),
                    )
                });
            },
        );

        let feasibility = feasibility_fixture(function_count);
        let feasibility_oracle = analyze_infeasible_paths(&feasibility, &infeasible_config)
            .expect("valid feasibility benchmark fixture");
        assert!(feasibility_oracle.is_complete());
        assert_eq!(
            feasibility_oracle
                .findings
                .iter()
                .filter(|finding| finding.kind == InfeasiblePathKind::ContradictoryCondition)
                .count(),
            function_count
        );
        group.bench_with_input(
            BenchmarkId::new("infeasible_paths", function_count),
            &feasibility,
            |benchmark, graph| {
                benchmark.iter(|| {
                    black_box(
                        analyze_infeasible_paths(black_box(graph), black_box(&infeasible_config))
                            .expect("valid feasibility benchmark fixture"),
                    )
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, benchmark_advisory_bugs);
criterion_main!(benches);
