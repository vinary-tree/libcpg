use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::{simple_cycles, CycleOptions, GraphProjection, NodeId, ProjectionKind};

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

fn path(node_count: usize) -> GraphProjection {
    projection(node_count, (1..node_count).map(|node| (node - 1, node)))
}

fn disjoint_triangles(node_count: usize) -> GraphProjection {
    assert_eq!(node_count % 3, 0);
    projection(
        node_count,
        (0..node_count)
            .step_by(3)
            .flat_map(|base| [(base, base + 1), (base + 1, base + 2), (base + 2, base)]),
    )
}

fn complete_directed(node_count: usize) -> GraphProjection {
    projection(
        node_count,
        (0..node_count)
            .flat_map(|source| (0..node_count).map(move |target| (source, target)))
            .filter(|(source, target)| source != target),
    )
}

fn assert_complete(graph: &GraphProjection, options: &CycleOptions, expected: usize) {
    let first = simple_cycles(graph, options).expect("benchmark projection is valid");
    let second = simple_cycles(graph, options).expect("repeat benchmark projection");
    assert_eq!(first, second, "repeatability oracle");
    assert_eq!(first.nodes, graph.nodes, "alignment oracle");
    assert_eq!(first.cycles.len(), expected);
    assert!(!first.truncated);
    assert_eq!(first.dropped_cycles, 0);
    for cycle in &first.cycles {
        assert_eq!(cycle[0], *cycle.iter().min().expect("nonempty cycle"));
        for edge in 0..cycle.len() {
            let source = graph.dense_index(cycle[edge]).expect("cycle source");
            let target = graph
                .dense_index(cycle[(edge + 1) % cycle.len()])
                .expect("cycle target");
            assert!(graph.successors(source).binary_search(&target).is_ok());
        }
    }
}

fn assert_truncated(graph: &GraphProjection, options: &CycleOptions) {
    let result = simple_cycles(graph, options).expect("benchmark projection is valid");
    assert_eq!(result.cycles.len(), options.max_cycles);
    assert!(result.truncated);
    assert_eq!(result.dropped_cycles, 1);
    assert!(result.first_dropped_cycle.is_some());
}

fn benchmark_cycles(c: &mut Criterion) {
    let mut group = c.benchmark_group("simple_cycles");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    for node_count in [1_024usize, 8_192] {
        let graph = path(node_count);
        let options = CycleOptions::default();
        assert_complete(&graph, &options, 0);
        group.throughput(Throughput::Elements(graph.edge_count() as u64));
        group.bench_with_input(
            BenchmarkId::new("acyclic_path", node_count),
            &graph,
            |bencher, graph| {
                bencher.iter(|| {
                    black_box(
                        simple_cycles(black_box(graph), black_box(&options))
                            .expect("timed projection remains valid"),
                    )
                });
            },
        );
    }

    for node_count in [1_023usize, 8_190] {
        let graph = disjoint_triangles(node_count);
        let options = CycleOptions {
            max_length: 3,
            max_cycles: 100_000,
        };
        assert_complete(&graph, &options, node_count / 3);
        group.throughput(Throughput::Elements(graph.edge_count() as u64));
        group.bench_with_input(
            BenchmarkId::new("disjoint_triangles", node_count),
            &graph,
            |bencher, graph| {
                bencher.iter(|| {
                    black_box(
                        simple_cycles(black_box(graph), black_box(&options))
                            .expect("timed projection remains valid"),
                    )
                });
            },
        );
    }

    for node_count in [8usize, 10] {
        let graph = complete_directed(node_count);
        let options = CycleOptions {
            max_length: node_count,
            max_cycles: 1_000,
        };
        assert_truncated(&graph, &options);
        group.throughput(Throughput::Elements(graph.edge_count() as u64));
        group.bench_with_input(
            BenchmarkId::new("complete_directed_truncated", node_count),
            &graph,
            |bencher, graph| {
                bencher.iter(|| {
                    black_box(
                        simple_cycles(black_box(graph), black_box(&options))
                            .expect("timed projection remains valid"),
                    )
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, benchmark_cycles);
criterion_main!(benches);
