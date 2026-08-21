use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::{
    analyze_taint_report, CallGraphResolution, CodePropertyGraph, CpgEdgeKind, CpgNode,
    CpgNodeKind, DfgEdgeKind, IfdsCaps, Language, MethodSignature, NodeId, SourceRange,
    TaintCatalog, Visibility,
};

#[derive(Clone, Copy)]
enum Shape {
    Path,
    Ring,
    Clustered,
}

impl Shape {
    const fn name(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Ring => "ring",
            Self::Clustered => "clustered",
        }
    }
}

fn node(cpg: &mut CodePropertyGraph, kind: CpgNodeKind) -> NodeId {
    cpg.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()))
}

fn ast(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let child = node(cpg, kind);
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("taint benchmark AST endpoints exist");
    child
}

fn function(cpg: &mut CodePropertyGraph, name: String) -> NodeId {
    node(
        cpg,
        CpgNodeKind::Function {
            signature: Box::new(MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            }),
        },
    )
}

fn call(cpg: &mut CodePropertyGraph, parent: NodeId, name: &str, target: Option<NodeId>) -> NodeId {
    let call = ast(
        cpg,
        parent,
        CpgNodeKind::Call {
            target,
            is_method: false,
        },
    );
    ast(
        cpg,
        call,
        CpgNodeKind::Identifier {
            name: name.into(),
            definition: target,
        },
    );
    if let Some(target) = target {
        cpg.connect(call, target, CpgEdgeKind::CallSite)
            .expect("taint benchmark call topology endpoints exist");
    }
    call
}

fn argument(
    cpg: &mut CodePropertyGraph,
    call: NodeId,
    source: NodeId,
    parameter: Option<NodeId>,
) -> NodeId {
    let argument = ast(
        cpg,
        call,
        CpgNodeKind::Identifier {
            name: "value".into(),
            definition: Some(source),
        },
    );
    cpg.connect(source, argument, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
        .expect("taint benchmark argument DFG endpoints exist");
    if let Some(parameter) = parameter {
        cpg.connect(
            argument,
            parameter,
            CpgEdgeKind::DataFlow(DfgEdgeKind::Parameter),
        )
        .expect("taint benchmark parameter DFG endpoints exist");
    }
    argument
}

fn fixture(worker_count: usize, shape: Shape) -> (CodePropertyGraph, NodeId, NodeId) {
    const CLUSTER: usize = 8;

    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let entry = function(&mut cpg, "entry".into());
    let workers: Vec<_> = (0..worker_count)
        .map(|index| function(&mut cpg, format!("worker_{index}")))
        .collect();
    let params: Vec<_> = workers
        .iter()
        .map(|&worker| {
            ast(
                &mut cpg,
                worker,
                CpgNodeKind::Parameter {
                    name: "value".into(),
                    param_type: None,
                    is_variadic: false,
                },
            )
        })
        .collect();

    let mut call_edges = Vec::new();
    match shape {
        Shape::Path => {
            call_edges.extend((0..worker_count - 1).map(|index| (index, index + 1)));
        }
        Shape::Ring => {
            call_edges.extend((0..worker_count).map(|index| (index, (index + 1) % worker_count)));
        }
        Shape::Clustered => {
            for start in (0..worker_count).step_by(CLUSTER) {
                let end = (start + CLUSTER).min(worker_count);
                for index in start..end {
                    call_edges.push((index, if index + 1 == end { start } else { index + 1 }));
                }
                if end < worker_count {
                    call_edges.push((end - 1, end));
                }
            }
        }
    }
    for (caller, callee) in call_edges {
        let site = call(
            &mut cpg,
            workers[caller],
            &format!("worker_{callee}"),
            Some(workers[callee]),
        );
        argument(&mut cpg, site, params[caller], Some(params[callee]));
    }

    let sink = call(&mut cpg, workers[worker_count - 1], "os.system", None);
    argument(&mut cpg, sink, params[worker_count - 1], None);

    let source = call(&mut cpg, entry, "std::env::var", None);
    let entry_call = call(&mut cpg, entry, "worker_0", Some(workers[0]));
    argument(&mut cpg, entry_call, source, Some(params[0]));
    (cpg, source, sink)
}

fn benchmark_ifds_taint(c: &mut Criterion) {
    let mut group = c.benchmark_group("ifds_taint");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    for worker_count in [32usize, 128] {
        for shape in [Shape::Path, Shape::Ring, Shape::Clustered] {
            let (cpg, source, sink) = fixture(worker_count, shape);
            let catalog = TaintCatalog::builtin_generic();
            let resolution = CallGraphResolution::default();
            let caps = IfdsCaps {
                max_scc_iters: u32::try_from(worker_count)
                    .expect("benchmark worker count fits u32")
                    .saturating_add(2),
                ..IfdsCaps::default()
            };

            // The exact source-to-sink oracle runs before the timed region.
            let oracle = analyze_taint_report(&cpg, &resolution, &catalog, &caps);
            assert!(oracle.is_complete());
            assert!(oracle
                .flows
                .iter()
                .any(|flow| flow.source == source && flow.sink == sink));
            assert!(oracle
                .flows
                .iter()
                .all(|flow| flow.witness.first() == Some(&flow.source)));

            group.throughput(Throughput::Elements(worker_count as u64));
            group.bench_with_input(
                BenchmarkId::new(shape.name(), worker_count),
                &worker_count,
                |benchmark, _| {
                    benchmark.iter(|| {
                        black_box(analyze_taint_report(
                            black_box(&cpg),
                            black_box(&resolution),
                            black_box(&catalog),
                            black_box(&caps),
                        ))
                    });
                },
            );
        }
    }
    group.finish();
}

criterion_group!(benches, benchmark_ifds_taint);
criterion_main!(benches);
