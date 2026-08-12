use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::pattern::{ged_upper_bound, GedOptions};
use libcpg::{CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, Language, NodeId, SourceRange};

#[derive(Clone, Copy)]
enum Case {
    IdenticalPath,
    RelabelledIsolates,
    RetypedPath,
}

impl Case {
    fn name(self) -> &'static str {
        match self {
            Self::IdenticalPath => "identical_path",
            Self::RelabelledIsolates => "relabelled_isolates",
            Self::RetypedPath => "retyped_path",
        }
    }
}

fn graph(
    node_count: usize,
    label: CpgNodeKind,
    edge_kind: Option<CpgEdgeKind>,
) -> CodePropertyGraph {
    let mut graph = CodePropertyGraph::new(Language::Rust);
    for _ in 0..node_count {
        graph.add_node(CpgNode::new(
            NodeId::new(0),
            label.clone(),
            SourceRange::default(),
        ));
    }
    if let Some(kind) = edge_kind {
        for node in 1..node_count {
            graph.connect(
                NodeId::new((node - 1) as u32),
                NodeId::new(node as u32),
                kind.clone(),
            );
        }
    }
    graph
}

fn fixtures(node_count: usize, case: Case) -> (CodePropertyGraph, CodePropertyGraph, f64) {
    match case {
        Case::IdenticalPath => {
            let first = graph(node_count, CpgNodeKind::Root, Some(CpgEdgeKind::AstChild));
            (first.clone(), first, 0.0)
        }
        Case::RelabelledIsolates => (
            graph(node_count, CpgNodeKind::If, None),
            graph(node_count, CpgNodeKind::While, None),
            node_count as f64,
        ),
        Case::RetypedPath => (
            graph(node_count, CpgNodeKind::Root, Some(CpgEdgeKind::AstChild)),
            graph(node_count, CpgNodeKind::Root, Some(CpgEdgeKind::Reference)),
            node_count.saturating_sub(1) as f64,
        ),
    }
}

fn benchmark_ged(c: &mut Criterion) {
    let options = GedOptions::default();
    let mut group = c.benchmark_group("ged_upper_bound");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    for node_count in [16usize, 48] {
        for case in [
            Case::IdenticalPath,
            Case::RelabelledIsolates,
            Case::RetypedPath,
        ] {
            let (first, second, expected) = fixtures(node_count, case);
            let oracle =
                ged_upper_bound(&first, &second, &options).expect("valid benchmark fixture");
            assert_eq!(oracle.upper, expected);
            assert!(!oracle.capped);
            group.throughput(Throughput::Elements(node_count as u64));
            group.bench_with_input(
                BenchmarkId::new(case.name(), node_count),
                &(first, second),
                |benchmark, (first, second)| {
                    benchmark.iter(|| {
                        black_box(
                            ged_upper_bound(
                                black_box(first),
                                black_box(second),
                                black_box(&options),
                            )
                            .expect("valid benchmark fixture"),
                        )
                    });
                },
            );
        }
    }
    group.finish();
}

criterion_group!(benches, benchmark_ged);
criterion_main!(benches);
