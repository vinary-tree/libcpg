mod support;

use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

use support::{assert_linear_graph, linear_graph};

fn benchmark_cpg_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("cpg_construction");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    for node_count in [1_000, 10_000] {
        let oracle_graph = linear_graph(node_count);
        assert_linear_graph(&oracle_graph, node_count);

        group.throughput(Throughput::Elements(node_count as u64));
        group.bench_with_input(
            BenchmarkId::new("linear_ast_overlay", node_count),
            &node_count,
            |benchmark, &node_count| {
                benchmark.iter(|| black_box(linear_graph(black_box(node_count))));
            },
        );
    }

    group.finish();
}

criterion_group!(benches, benchmark_cpg_construction);
criterion_main!(benches);
