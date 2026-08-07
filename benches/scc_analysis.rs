use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::{
    call_graph_sccs, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, Language,
    MethodSignature, SourceRange, Visibility,
};

#[derive(Clone, Copy)]
enum GraphShape {
    Path,
    Ring,
    Clustered,
}

impl GraphShape {
    fn name(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Ring => "ring",
            Self::Clustered => "clustered",
        }
    }
}

fn function_node() -> CpgNode {
    CpgNode::new(
        0.into(),
        CpgNodeKind::Function {
            signature: MethodSignature {
                name: "f".into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            },
        },
        SourceRange::default(),
    )
}

fn call_graph(node_count: usize, shape: GraphShape) -> CodePropertyGraph {
    const CLUSTER_SIZE: usize = 8;

    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let nodes: Vec<_> = (0..node_count)
        .map(|_| cpg.add_node(function_node()))
        .collect();

    match shape {
        GraphShape::Path => {
            for edge in nodes.windows(2) {
                cpg.connect(edge[0], edge[1], CpgEdgeKind::CallSite);
            }
        }
        GraphShape::Ring => {
            for index in 0..nodes.len() {
                cpg.connect(
                    nodes[index],
                    nodes[(index + 1) % nodes.len()],
                    CpgEdgeKind::CallSite,
                );
            }
        }
        GraphShape::Clustered => {
            for cluster in nodes.chunks(CLUSTER_SIZE) {
                for index in 0..cluster.len() {
                    cpg.connect(
                        cluster[index],
                        cluster[(index + 1) % cluster.len()],
                        CpgEdgeKind::CallSite,
                    );
                }
            }
            for boundary in (CLUSTER_SIZE..nodes.len()).step_by(CLUSTER_SIZE) {
                cpg.connect(nodes[boundary - 1], nodes[boundary], CpgEdgeKind::CallSite);
            }
        }
    }

    cpg
}

fn benchmark_scc_analysis(c: &mut Criterion) {
    let mut group = c.benchmark_group("call_graph_sccs");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    for node_count in [1_000, 10_000] {
        for shape in [GraphShape::Path, GraphShape::Ring, GraphShape::Clustered] {
            let cpg = call_graph(node_count, shape);
            group.throughput(Throughput::Elements(node_count as u64));
            group.bench_with_input(
                BenchmarkId::new(shape.name(), node_count),
                &cpg,
                |benchmark, cpg| {
                    benchmark.iter(|| black_box(call_graph_sccs(black_box(cpg))));
                },
            );
        }
    }

    group.finish();
}

criterion_group!(benches, benchmark_scc_analysis);
criterion_main!(benches);
