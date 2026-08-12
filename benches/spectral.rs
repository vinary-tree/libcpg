use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::{
    try_algebraic_connectivity, GraphProjection, NodeId, ProjectionKind, SpectralOptions,
};

fn projection(
    node_count: usize,
    edges: impl IntoIterator<Item = (usize, usize, f64)>,
) -> GraphProjection {
    GraphProjection::from_edges(
        ProjectionKind::External,
        (0..node_count as u32).map(NodeId::new),
        edges.into_iter().map(|(source, target, weight)| {
            (
                NodeId::new(source as u32),
                NodeId::new(target as u32),
                weight,
            )
        }),
    )
}

fn star(node_count: usize) -> GraphProjection {
    projection(node_count, (1..node_count).map(|leaf| (0, leaf, 1.0)))
}

fn complete_bipartite(part_size: usize) -> GraphProjection {
    projection(
        2 * part_size,
        (0..part_size)
            .flat_map(|left| (part_size..2 * part_size).map(move |right| (left, right, 1.0))),
    )
}

fn disconnected_stars(node_count: usize) -> GraphProjection {
    assert_eq!(node_count % 2, 0);
    let half = node_count / 2;
    projection(
        node_count,
        (1..half)
            .map(|leaf| (0, leaf, 1.0))
            .chain((half + 1..node_count).map(move |leaf| (half, leaf, 1.0))),
    )
}

fn assert_connected_oracle(graph: &GraphProjection, expected: f64) {
    let first = try_algebraic_connectivity(graph, &SpectralOptions::default())
        .expect("benchmark projection is valid")
        .expect("benchmark has at least two nodes");
    let second = try_algebraic_connectivity(graph, &SpectralOptions::default())
        .expect("repeat benchmark projection")
        .expect("repeat benchmark result");
    assert_eq!(first, second, "repeatability oracle");
    assert_eq!(first.nodes, graph.nodes, "alignment oracle");
    assert_eq!(
        first.starts,
        graph.node_count().min(libcpg::MAX_SPECTRAL_STARTS),
        "bounded-start oracle"
    );
    assert!(first.converged, "closed-form fixture must converge");
    assert!((first.algebraic_connectivity - expected).abs() <= 1.0e-8);
    assert!(
        first.residual_norm <= 1.0e-5,
        "residual {} for expected eigenvalue {expected}",
        first.residual_norm
    );
    assert!(first.fiedler[0] > 0.0, "canonical sign oracle");
}

fn assert_disconnected_oracle(graph: &GraphProjection) {
    let result = try_algebraic_connectivity(graph, &SpectralOptions::default())
        .expect("benchmark projection is valid")
        .expect("benchmark has at least two nodes");
    assert_eq!(result.algebraic_connectivity, 0.0);
    assert_eq!(result.iterations, 0);
    assert_eq!(result.starts, 0);
    assert!(result.converged);
    assert!(result.residual_norm <= 1.0e-12);
    assert_eq!(
        result.partition.iter().filter(|&&side| side).count(),
        graph.node_count() / 2
    );
}

fn benchmark_spectral(c: &mut Criterion) {
    let mut group = c.benchmark_group("spectral_analysis");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    for node_count in [1_024usize, 8_192] {
        let graph = star(node_count);
        assert_connected_oracle(&graph, 1.0);
        group.throughput(Throughput::Elements(graph.edge_count() as u64));
        group.bench_with_input(
            BenchmarkId::new("star", node_count),
            &graph,
            |bencher, graph| {
                bencher.iter(|| {
                    black_box(
                        try_algebraic_connectivity(
                            black_box(graph),
                            black_box(&SpectralOptions::default()),
                        )
                        .expect("timed projection remains valid"),
                    )
                });
            },
        );
    }

    for part_size in [32usize, 64] {
        let graph = complete_bipartite(part_size);
        assert_connected_oracle(&graph, part_size as f64);
        group.throughput(Throughput::Elements(graph.edge_count() as u64));
        group.bench_with_input(
            BenchmarkId::new("complete_bipartite", 2 * part_size),
            &graph,
            |bencher, graph| {
                bencher.iter(|| {
                    black_box(
                        try_algebraic_connectivity(
                            black_box(graph),
                            black_box(&SpectralOptions::default()),
                        )
                        .expect("timed projection remains valid"),
                    )
                });
            },
        );
    }

    for node_count in [1_024usize, 8_192] {
        let graph = disconnected_stars(node_count);
        assert_disconnected_oracle(&graph);
        group.throughput(Throughput::Elements(graph.edge_count() as u64));
        group.bench_with_input(
            BenchmarkId::new("disconnected_stars", node_count),
            &graph,
            |bencher, graph| {
                bencher.iter(|| {
                    black_box(
                        try_algebraic_connectivity(
                            black_box(graph),
                            black_box(&SpectralOptions::default()),
                        )
                        .expect("timed projection remains valid"),
                    )
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, benchmark_spectral);
criterion_main!(benches);
