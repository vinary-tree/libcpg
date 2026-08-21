mod support;

use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use support::{assert_traversal_graph, traversal_graph};

fn benchmark_traversal_primitives(c: &mut Criterion) {
    let mut group = c.benchmark_group("traversal_primitives");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    for node_count in [1_024usize, 8_192] {
        let (graph, nodes) = traversal_graph(node_count);
        assert_traversal_graph(&graph, &nodes);
        group.throughput(Throughput::Elements(node_count as u64));

        group.bench_with_input(
            BenchmarkId::new("node", node_count),
            &(&graph, &nodes),
            |benchmark, (graph, nodes)| {
                benchmark.iter(|| {
                    for &node in *nodes {
                        black_box(graph.node(black_box(node)));
                    }
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("ast_children", node_count),
            &(&graph, &nodes),
            |benchmark, (graph, nodes)| {
                benchmark.iter(|| {
                    for &node in *nodes {
                        black_box(graph.ast_children(black_box(node)));
                    }
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("cfg_successors", node_count),
            &(&graph, &nodes),
            |benchmark, (graph, nodes)| {
                benchmark.iter(|| {
                    for &node in *nodes {
                        black_box(graph.cfg_successors(black_box(node)));
                    }
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("dfg_successors", node_count),
            &(&graph, &nodes),
            |benchmark, (graph, nodes)| {
                benchmark.iter(|| {
                    for &node in *nodes {
                        black_box(graph.dfg_successors(black_box(node)));
                    }
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("edges_between", node_count),
            &(&graph, &nodes),
            |benchmark, (graph, nodes)| {
                benchmark.iter(|| {
                    for endpoints in nodes.windows(2) {
                        black_box(
                            graph.edges_between(black_box(endpoints[0]), black_box(endpoints[1])),
                        );
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, benchmark_traversal_primitives);
criterion_main!(benches);
