mod support;

use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::{dominator_tree, post_dominator_tree, NodeId};

fn benchmark_dominator_analysis(c: &mut Criterion) {
    let mut group = c.benchmark_group("dominator_analysis");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    for stages in [256usize, 2_048] {
        let (projection, cpg, function) = support::dominator_diamond_fixture(stages);
        support::assert_dominator_diamond_fixture(&projection, &cpg, function, stages);
        group.throughput(Throughput::Elements(projection.node_count() as u64));

        group.bench_with_input(
            BenchmarkId::new("dominators", stages),
            &projection,
            |benchmark, projection| {
                benchmark.iter(|| {
                    black_box(
                        dominator_tree(black_box(projection), NodeId::new(0))
                            .expect("benchmark dominators"),
                    )
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("post_dominators", stages),
            &(&cpg, function),
            |benchmark, (cpg, function)| {
                benchmark.iter(|| {
                    black_box(
                        post_dominator_tree(black_box(cpg), black_box(*function))
                            .expect("benchmark post-dominators"),
                    )
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, benchmark_dominator_analysis);
criterion_main!(benches);
