use std::{collections::BTreeMap, sync::Arc, time::Duration};

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::{
    block_cfg, liveness, variable_facts, BitSetDomain, CfgEdgeKind, CodePropertyGraph, CpgEdgeKind,
    CpgNode, CpgNodeKind, Language, Lattice, LivenessResult, MethodSignature, NodeId, ScopeId,
    SourceRange, Visibility,
};

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn function() -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: Box::new(MethodSignature {
            name: "benchmark".into(),
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

fn connect_ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild);
}

fn add_child(cpg: &mut CodePropertyGraph, function: NodeId, kind: CpgNodeKind) -> NodeId {
    let child = cpg.add_node(node(kind));
    connect_ast(cpg, function, child);
    child
}

fn connect_cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId, kind: CfgEdgeKind) {
    cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind));
}

fn chain(statement_count: usize) -> (CodePropertyGraph, NodeId) {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = cpg.add_node(node(function()));
    let mut previous = function;
    for index in 0..statement_count {
        let name = format!("v{}", (index / 2) % 64);
        let kind = if index % 2 == 0 {
            variable(&name)
        } else {
            identifier(&name)
        };
        let current = add_child(&mut cpg, function, kind);
        connect_cfg(&mut cpg, previous, current, CfgEdgeKind::Sequential);
        previous = current;
    }
    (cpg, function)
}

fn diamond_ladder(rungs: usize) -> (CodePropertyGraph, NodeId) {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = cpg.add_node(node(function()));
    let mut previous = function;
    for rung in 0..rungs {
        let guard = add_child(
            &mut cpg,
            function,
            identifier(&format!("guard{}", rung % 8)),
        );
        let name = format!("x{}", rung % 64);
        let left = add_child(&mut cpg, function, variable(&name));
        let right = add_child(&mut cpg, function, variable(&name));
        let join = add_child(&mut cpg, function, identifier(&name));
        connect_cfg(&mut cpg, previous, guard, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, guard, left, CfgEdgeKind::ConditionalTrue);
        connect_cfg(&mut cpg, guard, right, CfgEdgeKind::ConditionalFalse);
        connect_cfg(&mut cpg, left, join, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, right, join, CfgEdgeKind::Sequential);
        previous = join;
    }
    (cpg, function)
}

fn nested_loop(body_nodes: usize) -> (CodePropertyGraph, NodeId) {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = cpg.add_node(node(function()));
    let outer_guard = add_child(&mut cpg, function, identifier("outer_guard"));
    let inner_guard = add_child(&mut cpg, function, identifier("inner_guard"));
    let mut body = Vec::with_capacity(body_nodes);
    for index in 0..body_nodes {
        let name = format!("loop{}", (index / 2) % 64);
        body.push(add_child(
            &mut cpg,
            function,
            if index % 2 == 0 {
                variable(&name)
            } else {
                identifier(&name)
            },
        ));
    }
    let outer_latch = add_child(&mut cpg, function, identifier("outer_guard"));
    let exit = add_child(&mut cpg, function, CpgNodeKind::Return);

    connect_cfg(&mut cpg, function, outer_guard, CfgEdgeKind::Sequential);
    connect_cfg(
        &mut cpg,
        outer_guard,
        inner_guard,
        CfgEdgeKind::ConditionalTrue,
    );
    connect_cfg(&mut cpg, outer_guard, exit, CfgEdgeKind::ConditionalFalse);
    connect_cfg(&mut cpg, inner_guard, body[0], CfgEdgeKind::ConditionalTrue);
    connect_cfg(
        &mut cpg,
        inner_guard,
        outer_latch,
        CfgEdgeKind::ConditionalFalse,
    );
    for endpoints in body.windows(2) {
        connect_cfg(
            &mut cpg,
            endpoints[0],
            endpoints[1],
            CfgEdgeKind::Sequential,
        );
    }
    connect_cfg(
        &mut cpg,
        *body.last().expect("non-empty loop body"),
        inner_guard,
        CfgEdgeKind::LoopBack,
    );
    connect_cfg(&mut cpg, outer_latch, outer_guard, CfgEdgeKind::LoopBack);
    (cpg, function)
}

fn naive_round_robin(cpg: &CodePropertyGraph, function: NodeId) -> LivenessResult {
    let cfg = block_cfg(cpg, function).expect("benchmark block CFG");
    let facts = variable_facts(cpg, function);
    let width = facts.universe.len();
    let nodes: Vec<_> = cfg.block_of_node.keys().copied().collect();
    let mut live_in: BTreeMap<_, _> = nodes
        .iter()
        .copied()
        .map(|node| (node, BitSetDomain::new(width)))
        .collect();
    let mut live_out = live_in.clone();
    let max_passes = nodes.len().saturating_mul(width.max(1)).saturating_add(1);

    for _ in 0..max_passes {
        let mut changed = false;
        for &node in nodes.iter().rev() {
            let mut output = BitSetDomain::new(width);
            for (successor, _) in cpg.cfg_successors(node) {
                if let Some(input) = live_in.get(&successor) {
                    output.join(input);
                }
            }
            let mut input = output.clone();
            for definition in facts.definitions_at(node) {
                input.remove(definition.variable);
            }
            for &variable in facts.uses_at(node) {
                input.insert(variable);
            }
            changed |= live_out[&node] != output;
            changed |= live_in[&node] != input;
            live_out.insert(node, output);
            live_in.insert(node, input);
        }
        if !changed {
            let to_names = |states: BTreeMap<NodeId, BitSetDomain>| {
                states
                    .into_iter()
                    .map(|(node, state)| {
                        let names = state
                            .iter()
                            .filter_map(|variable| facts.name(variable).cloned())
                            .collect::<Vec<Arc<str>>>();
                        (node, names)
                    })
                    .collect()
            };
            return LivenessResult {
                function,
                live_in: to_names(live_in),
                live_out: to_names(live_out),
            };
        }
    }
    panic!("benchmark round-robin oracle exceeded its finite lattice bound");
}

fn benchmark_dataflow_liveness(c: &mut Criterion) {
    let cases = [
        ("chain", 512usize, chain(512)),
        ("chain", 4_096, chain(4_096)),
        ("diamond_ladder", 64, diamond_ladder(64)),
        ("diamond_ladder", 512, diamond_ladder(512)),
        ("nested_loop", 128, nested_loop(128)),
        ("nested_loop", 1_024, nested_loop(1_024)),
    ];
    let mut group = c.benchmark_group("dataflow_liveness");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    for (shape, size, (cpg, function)) in cases {
        let expected = naive_round_robin(&cpg, function);
        let actual = liveness(&cpg, function).expect("benchmark liveness");
        assert_eq!(actual, expected, "{shape}/{size} pre-timing oracle");

        group.throughput(Throughput::Elements(cpg.node_count() as u64));
        group.bench_with_input(
            BenchmarkId::new(shape, size),
            &(cpg, function),
            |benchmark, (cpg, function)| {
                benchmark.iter(|| {
                    black_box(
                        liveness(black_box(cpg), black_box(*function))
                            .expect("liveness during benchmark"),
                    )
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, benchmark_dataflow_liveness);
criterion_main!(benches);
