mod support;

use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::{structural_clone_classes, wl_labels, GraphProjection, NodeId, ProjectionKind};

fn ring(node_count: usize) -> GraphProjection {
    GraphProjection::from_edges(
        ProjectionKind::External,
        (0..node_count as u32).map(NodeId::new),
        (0..node_count).map(|source| {
            (
                NodeId::new(source as u32),
                NodeId::new(((source + 1) % node_count) as u32),
                1.0,
            )
        }),
    )
}

fn benchmark_similarity_wl(c: &mut Criterion) {
    const ITERATIONS: usize = 4;

    let mut group = c.benchmark_group("similarity_wl");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
    for node_count in [1_024usize, 8_192] {
        let graph = ring(node_count);
        let labels = wl_labels(&graph, ITERATIONS);
        let classes = structural_clone_classes(&graph, ITERATIONS, 2);
        assert_eq!(labels.len(), node_count, "WL label cardinality oracle");
        assert!(
            labels.iter().all(|label| *label == labels[0]),
            "ring symmetry oracle"
        );
        assert_eq!(classes, vec![graph.nodes.clone()], "WL class oracle");

        group.throughput(Throughput::Elements(node_count as u64));
        group.bench_with_input(
            BenchmarkId::new("labels", node_count),
            &graph,
            |benchmark, graph| {
                benchmark.iter(|| black_box(wl_labels(black_box(graph), ITERATIONS)));
            },
        );
        group.bench_with_input(
            BenchmarkId::new("clone_classes", node_count),
            &graph,
            |benchmark, graph| {
                benchmark
                    .iter(|| black_box(structural_clone_classes(black_box(graph), ITERATIONS, 2)));
            },
        );
    }
    group.finish();
}

criterion_group!(benches, benchmark_similarity_wl);
criterion_main!(benches);
