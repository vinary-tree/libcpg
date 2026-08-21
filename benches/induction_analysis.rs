use std::{collections::BTreeSet, time::Duration};

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::{
    loop_forest, loop_induction, variable_facts, CfgEdgeKind, CodePropertyGraph, CpgEdgeKind,
    CpgNode, CpgNodeKind, DfgEdgeKind, InductionAnalysis, IvDirection, Language, LiteralKind,
    LoopForest, MethodSignature, NodeId, ScopeId, SourceRange, TripEvidence, Visibility,
};

#[cfg(feature = "algorithm-detection")]
use libcpg::algorithms::{detection::ComplexityAnalyzer, ComplexityClass};

struct Fixture {
    cpg: CodePropertyGraph,
    function: NodeId,
    forest: LoopForest,
    invariant_definitions: Vec<NodeId>,
    update: NodeId,
    comparison: NodeId,
    bound_use: NodeId,
}

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn function() -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: Box::new(MethodSignature {
            name: "benchmark_induction".into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        }),
    }
}

fn variable(name: &str) -> CpgNodeKind {
    CpgNodeKind::Variable {
        name: name.into(),
        var_type: None,
        is_mutable: true,
        scope: ScopeId::GLOBAL,
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
        kind: LiteralKind::Integer(value),
    }
}

fn binary(operator: &str) -> CpgNodeKind {
    CpgNodeKind::BinaryOp {
        operator: operator.into(),
    }
}

fn add(cpg: &mut CodePropertyGraph, kind: CpgNodeKind) -> NodeId {
    cpg.add_node(node(kind))
}

fn connect_ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("benchmark AST endpoints exist");
}

fn connect_cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId, kind: CfgEdgeKind) {
    cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind))
        .expect("benchmark CFG endpoints exist");
}

fn connect_def_use(cpg: &mut CodePropertyGraph, definition: NodeId, use_site: NodeId) {
    cpg.connect(
        definition,
        use_site,
        CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
    )
    .expect("benchmark DFG endpoints exist");
}

fn initialized_variable(cpg: &mut CodePropertyGraph, name: &str, initializer: NodeId) -> NodeId {
    let definition = add(cpg, variable(name));
    let binder = add(cpg, identifier(name));
    connect_ast(cpg, definition, binder);
    connect_ast(cpg, definition, initializer);
    definition
}

fn counted_loop(invariant_count: usize) -> Fixture {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add(&mut cpg, function());

    let bound_literal = add(&mut cpg, integer(invariant_count as i64 + 1));
    let bound_definition = initialized_variable(&mut cpg, "limit", bound_literal);
    let initial_literal = add(&mut cpg, integer(0));
    let initial_i = initialized_variable(&mut cpg, "i", initial_literal);

    let header = add(&mut cpg, CpgNodeKind::While);
    let comparison = add(&mut cpg, binary("<"));
    let comparison_i = add(&mut cpg, identifier("i"));
    let bound_use = add(&mut cpg, identifier("limit"));
    let body = add(
        &mut cpg,
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        },
    );
    connect_ast(&mut cpg, comparison, comparison_i);
    connect_ast(&mut cpg, comparison, bound_use);
    connect_ast(&mut cpg, header, comparison);
    connect_ast(&mut cpg, header, body);

    let mut invariant_definitions = Vec::with_capacity(invariant_count);
    let mut invariant_uses = Vec::with_capacity(invariant_count);
    for index in 0..invariant_count {
        let literal = add(&mut cpg, integer(index as i64));
        let rhs = add(&mut cpg, binary("+"));
        let limit_use = add(&mut cpg, identifier("limit"));
        connect_ast(&mut cpg, rhs, limit_use);
        connect_ast(&mut cpg, rhs, literal);
        let definition = initialized_variable(&mut cpg, &format!("t{index}"), rhs);
        connect_ast(&mut cpg, body, definition);
        invariant_definitions.push(definition);
        invariant_uses.push(limit_use);
    }

    let update = add(
        &mut cpg,
        CpgNodeKind::Assignment {
            operator: "+=".into(),
        },
    );
    let update_i = add(&mut cpg, identifier("i"));
    let step = add(&mut cpg, integer(1));
    connect_ast(&mut cpg, update, update_i);
    connect_ast(&mut cpg, update, step);
    connect_ast(&mut cpg, body, update);

    let exit = add(&mut cpg, CpgNodeKind::Return);
    for child in [bound_definition, initial_i, header, exit] {
        connect_ast(&mut cpg, function, child);
    }

    connect_cfg(
        &mut cpg,
        function,
        bound_definition,
        CfgEdgeKind::Sequential,
    );
    connect_cfg(
        &mut cpg,
        bound_definition,
        initial_i,
        CfgEdgeKind::Sequential,
    );
    connect_cfg(&mut cpg, initial_i, header, CfgEdgeKind::Sequential);
    let first_body_node = invariant_definitions.first().copied().unwrap_or(update);
    connect_cfg(
        &mut cpg,
        header,
        first_body_node,
        CfgEdgeKind::ConditionalTrue,
    );
    for endpoints in invariant_definitions.windows(2) {
        connect_cfg(
            &mut cpg,
            endpoints[0],
            endpoints[1],
            CfgEdgeKind::Sequential,
        );
    }
    if let Some(&last) = invariant_definitions.last() {
        connect_cfg(&mut cpg, last, update, CfgEdgeKind::Sequential);
    }
    connect_cfg(&mut cpg, update, header, CfgEdgeKind::LoopBack);
    connect_cfg(&mut cpg, header, exit, CfgEdgeKind::LoopExit);
    cpg.add_cfg_entry(function);
    cpg.add_cfg_exit(exit);

    connect_def_use(&mut cpg, bound_definition, bound_use);
    for use_site in invariant_uses {
        connect_def_use(&mut cpg, bound_definition, use_site);
    }
    for definition in [initial_i, update] {
        connect_def_use(&mut cpg, definition, comparison_i);
        connect_def_use(&mut cpg, definition, update_i);
    }

    let forest = loop_forest(&cpg, function).expect("benchmark natural-loop forest");
    Fixture {
        cpg,
        function,
        forest,
        invariant_definitions,
        update,
        comparison,
        bound_use,
    }
}

fn assert_oracle(fixture: &Fixture, analysis: &InductionAnalysis) {
    assert_eq!(
        analysis.function, fixture.function,
        "function identity oracle"
    );
    assert_eq!(fixture.forest.loops.len(), 1, "single-loop fixture oracle");
    assert_eq!(analysis.invariants.len(), 1, "invariant report oracle");
    assert_eq!(analysis.bounds.len(), 1, "bound report oracle");
    let loop_id = fixture.forest.loops[0].id;
    assert!(fixture.forest.loops[0].body.contains(&fixture.update));

    let iv = analysis
        .ivs
        .iter()
        .find(|iv| &*iv.variable == "i")
        .expect("benchmark basic induction variable");
    assert_eq!(iv.loop_id, loop_id);
    assert_eq!(iv.update_node, fixture.update);
    assert_eq!(iv.step, Some(1));
    assert_eq!(iv.direction, IvDirection::Increasing);
    assert_eq!(
        analysis.bounds[0].trip,
        TripEvidence::Counted {
            iv: "i".into(),
            comparison: fixture.comparison,
            bound_node: fixture.bound_use,
        }
    );

    let invariant: BTreeSet<_> = analysis.invariants[0]
        .invariant_nodes
        .iter()
        .copied()
        .collect();
    assert!(fixture
        .invariant_definitions
        .iter()
        .all(|node| invariant.contains(node)));
    assert!(analysis.invariants[0]
        .hoistable
        .iter()
        .all(|node| invariant.contains(node)));

    let facts = variable_facts(&fixture.cpg, fixture.function);
    let region: BTreeSet<_> = fixture.forest.loops[0]
        .body
        .iter()
        .copied()
        .flat_map(|node| std::iter::once(node).chain(fixture.cpg.ast_descendants(node)))
        .collect();
    for &candidate in &analysis.invariants[0].invariant_nodes {
        for use_site in std::iter::once(candidate)
            .chain(fixture.cpg.ast_descendants(candidate))
            .filter(|node| !facts.uses_at(*node).is_empty())
        {
            let reaching: Vec<_> = fixture
                .cpg
                .dfg_predecessors(use_site)
                .into_iter()
                .filter_map(|(source, kind)| (kind == DfgEdgeKind::DefUse).then_some(source))
                .collect();
            assert!(!reaching.is_empty(), "every invariant use has DFG evidence");
            assert!(reaching
                .iter()
                .all(|source| !region.contains(source) || invariant.contains(source)));
        }
    }
}

fn benchmark_induction_analysis(c: &mut Criterion) {
    let cases = [
        (64usize, counted_loop(64)),
        (512, counted_loop(512)),
        (2_048, counted_loop(2_048)),
    ];
    let mut group = c.benchmark_group("induction_analysis");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    for (size, fixture) in cases {
        let before = (fixture.cpg.node_count(), fixture.cpg.edge_count());
        let expected = loop_induction(&fixture.cpg, fixture.function, &fixture.forest)
            .expect("benchmark induction preflight");
        assert_oracle(&fixture, &expected);
        assert_eq!(
            (fixture.cpg.node_count(), fixture.cpg.edge_count()),
            before,
            "analysis is out-of-graph"
        );

        group.throughput(Throughput::Elements(fixture.cpg.node_count() as u64));
        group.bench_with_input(
            BenchmarkId::new("single_counted_loop", size),
            &fixture,
            |benchmark, fixture| {
                benchmark.iter(|| {
                    black_box(
                        loop_induction(
                            black_box(&fixture.cpg),
                            black_box(fixture.function),
                            black_box(&fixture.forest),
                        )
                        .expect("induction analysis during benchmark"),
                    )
                });
            },
        );
    }

    group.finish();
}

#[cfg(feature = "algorithm-detection")]
fn benchmark_complexity_evidence(c: &mut Criterion) {
    let cases = [
        (64usize, counted_loop(64)),
        (512, counted_loop(512)),
        (2_048, counted_loop(2_048)),
    ];
    let analyzer = ComplexityAnalyzer::new();
    let mut group = c.benchmark_group("complexity_evidence");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    for (size, fixture) in cases {
        let before = (fixture.cpg.node_count(), fixture.cpg.edge_count());
        let expected =
            analyzer.estimate_time_complexity_with_evidence(&fixture.cpg, fixture.function);
        assert_eq!(expected.class, ComplexityClass::Linear);
        assert_eq!(expected.confidence, 1.0);
        assert!(expected.justification.contains("counted IV i"));
        assert!(expected
            .justification
            .contains(&format!("{:?}", fixture.forest.loops[0].header)));
        assert!(expected
            .justification
            .contains(&format!("{:?}", fixture.comparison)));
        assert!(expected
            .justification
            .contains(&format!("{:?}", fixture.bound_use)));
        assert_eq!(
            (fixture.cpg.node_count(), fixture.cpg.edge_count()),
            before,
            "complexity evidence is out-of-graph"
        );

        group.throughput(Throughput::Elements(fixture.cpg.node_count() as u64));
        group.bench_with_input(
            BenchmarkId::new("single_counted_loop", size),
            &fixture,
            |benchmark, fixture| {
                benchmark.iter(|| {
                    black_box(analyzer.estimate_time_complexity_with_evidence(
                        black_box(&fixture.cpg),
                        black_box(fixture.function),
                    ))
                });
            },
        );
    }

    group.finish();
}

#[cfg(feature = "algorithm-detection")]
criterion_group!(
    benches,
    benchmark_induction_analysis,
    benchmark_complexity_evidence
);
#[cfg(not(feature = "algorithm-detection"))]
criterion_group!(benches, benchmark_induction_analysis);
criterion_main!(benches);
