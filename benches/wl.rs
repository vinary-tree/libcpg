use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::{structural_clone_classes, wl_labels, GraphProjection, NodeId, ProjectionKind};

#[derive(Clone, Copy)]
enum GraphShape {
    Ring,
    Star,
}

impl GraphShape {
    fn name(self) -> &'static str {
        match self {
            Self::Ring => "ring",
            Self::Star => "star",
        }
    }
}

fn projection(node_count: usize, shape: GraphShape) -> GraphProjection {
    let nodes = (0..node_count as u32).map(NodeId::new);
    let edges = match shape {
        GraphShape::Ring => (0..node_count)
            .map(|source| {
                (
                    NodeId::new(source as u32),
                    NodeId::new(((source + 1) % node_count) as u32),
                    1.0,
                )
            })
            .collect::<Vec<_>>(),
        GraphShape::Star => (1..node_count)
            .map(|target| (NodeId::new(0), NodeId::new(target as u32), 1.0))
            .collect(),
    };
    GraphProjection::from_edges(ProjectionKind::External, nodes, edges)
}

fn assert_oracle(graph: &GraphProjection, shape: GraphShape, iterations: usize) {
    let labels = wl_labels(graph, iterations);
    let classes = structural_clone_classes(graph, iterations, 2);
    assert_eq!(labels.len(), graph.node_count());
    match shape {
        GraphShape::Ring => {
            assert!(labels.iter().all(|&label| label == labels[0]));
            assert_eq!(classes.len(), 1);
            assert_eq!(classes[0], graph.nodes);
        }
        GraphShape::Star => {
            assert_ne!(labels[0], labels[1]);
            assert!(labels[1..].iter().all(|&label| label == labels[1]));
            assert_eq!(classes, vec![graph.nodes[1..].to_vec()]);
        }
    }
}

fn benchmark_wl(c: &mut Criterion) {
    const ITERATIONS: usize = 4;

    let mut labels = c.benchmark_group("wl_labels");
    labels.sample_size(20);
    labels.warm_up_time(Duration::from_secs(1));
    labels.measurement_time(Duration::from_secs(3));
    for node_count in [1_024usize, 8_192] {
        for shape in [GraphShape::Ring, GraphShape::Star] {
            let graph = projection(node_count, shape);
            assert_oracle(&graph, shape, ITERATIONS);
            labels.throughput(Throughput::Elements(node_count as u64));
            labels.bench_with_input(
                BenchmarkId::new(shape.name(), node_count),
                &graph,
                |benchmark, graph| {
                    benchmark.iter(|| black_box(wl_labels(black_box(graph), ITERATIONS)));
                },
            );
        }
    }
    labels.finish();

    let mut classes = c.benchmark_group("wl_clone_classes");
    classes.sample_size(20);
    classes.warm_up_time(Duration::from_secs(1));
    classes.measurement_time(Duration::from_secs(3));
    for node_count in [1_024usize, 8_192] {
        for shape in [GraphShape::Ring, GraphShape::Star] {
            let graph = projection(node_count, shape);
            assert_oracle(&graph, shape, ITERATIONS);
            classes.throughput(Throughput::Elements(node_count as u64));
            classes.bench_with_input(
                BenchmarkId::new(shape.name(), node_count),
                &graph,
                |benchmark, graph| {
                    benchmark.iter(|| {
                        black_box(structural_clone_classes(black_box(graph), ITERATIONS, 2))
                    });
                },
            );
        }
    }
    classes.finish();
}

criterion_group!(benches, benchmark_wl);
criterion_main!(benches);
