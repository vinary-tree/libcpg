use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::{motif_census, GraphProjection, MotifOptions, NodeId, ProjectionKind};

fn projection(
    node_count: usize,
    edges: impl IntoIterator<Item = (usize, usize)>,
) -> GraphProjection {
    GraphProjection::from_edges(
        ProjectionKind::External,
        (0..node_count as u32).map(NodeId::new),
        edges
            .into_iter()
            .map(|(source, target)| (NodeId::new(source as u32), NodeId::new(target as u32), 1.0)),
    )
}

fn choose(n: usize, k: usize) -> u64 {
    match k {
        2 if n >= 2 => (n * (n - 1) / 2) as u64,
        3 if n >= 3 => (n * (n - 1) * (n - 2) / 6) as u64,
        4 if n >= 4 => (n * (n - 1) * (n - 2) * (n - 3) / 24) as u64,
        _ => 0,
    }
}

fn path(node_count: usize) -> GraphProjection {
    projection(node_count, (1..node_count).map(|node| (node - 1, node)))
}

fn complete_bidirected(node_count: usize) -> GraphProjection {
    projection(
        node_count,
        (0..node_count)
            .flat_map(|source| (0..node_count).map(move |target| (source, target)))
            .filter(|(source, target)| source != target),
    )
}

fn out_star(node_count: usize) -> GraphProjection {
    projection(node_count, (1..node_count).map(|leaf| (0, leaf)))
}

fn assert_path(graph: &GraphProjection) {
    let result = motif_census(graph, &MotifOptions::default()).expect("path is within caps");
    assert_eq!(
        result.triads.iter().sum::<u64>(),
        choose(graph.node_count(), 3)
    );
    assert_eq!(result.graphlets_4, [0, 0]);
    assert_eq!(
        result,
        motif_census(graph, &MotifOptions::default()).expect("repeat path")
    );
}

fn assert_complete(graph: &GraphProjection) {
    let result = motif_census(graph, &MotifOptions::default()).expect("clique is within caps");
    assert_eq!(result.triads[15], choose(graph.node_count(), 3));
    assert_eq!(
        result.triads.iter().sum::<u64>(),
        choose(graph.node_count(), 3)
    );
    assert_eq!(result.graphlets_4, [choose(graph.node_count(), 4), 0]);
}

fn assert_star(graph: &GraphProjection) {
    let leaves = graph.node_count() - 1;
    let result = motif_census(graph, &MotifOptions::default()).expect("star is within caps");
    assert_eq!(result.triads[0], choose(leaves, 3));
    assert_eq!(result.triads[3], choose(leaves, 2));
    assert_eq!(
        result.triads.iter().sum::<u64>(),
        choose(graph.node_count(), 3)
    );
    assert_eq!(result.graphlets_4, [0, choose(leaves, 3)]);
}

fn benchmark_motifs(c: &mut Criterion) {
    let mut group = c.benchmark_group("motif_census");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    for node_count in [24usize, 50] {
        let graph = path(node_count);
        assert_path(&graph);
        group.throughput(Throughput::Elements(
            choose(node_count, 3) + choose(node_count, 4),
        ));
        group.bench_with_input(
            BenchmarkId::new("directed_path", node_count),
            &graph,
            |bencher, graph| {
                bencher.iter(|| {
                    black_box(
                        motif_census(black_box(graph), black_box(&MotifOptions::default()))
                            .expect("timed path remains within caps"),
                    )
                });
            },
        );
    }

    for node_count in [16usize, 30] {
        let graph = complete_bidirected(node_count);
        assert_complete(&graph);
        group.throughput(Throughput::Elements(
            choose(node_count, 3) + choose(node_count, 4),
        ));
        group.bench_with_input(
            BenchmarkId::new("complete_bidirected", node_count),
            &graph,
            |bencher, graph| {
                bencher.iter(|| {
                    black_box(
                        motif_census(black_box(graph), black_box(&MotifOptions::default()))
                            .expect("timed clique remains within caps"),
                    )
                });
            },
        );
    }

    for node_count in [24usize, 50] {
        let graph = out_star(node_count);
        assert_star(&graph);
        group.throughput(Throughput::Elements(
            choose(node_count, 3) + choose(node_count, 4),
        ));
        group.bench_with_input(
            BenchmarkId::new("out_star", node_count),
            &graph,
            |bencher, graph| {
                bencher.iter(|| {
                    black_box(
                        motif_census(black_box(graph), black_box(&MotifOptions::default()))
                            .expect("timed star remains within caps"),
                    )
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, benchmark_motifs);
criterion_main!(benches);
