mod support;

use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::pattern::Vf2Matcher;
use libcpg::SubgraphMatcher;

use support::{assert_linear_graph, linear_graph};

fn benchmark_pattern_vf2(c: &mut Criterion) {
    const PATTERN_NODES: usize = 4;

    let pattern = linear_graph(PATTERN_NODES);
    assert_linear_graph(&pattern, PATTERN_NODES);
    let matcher = Vf2Matcher::new().with_max_matches(1);
    let mut group = c.benchmark_group("pattern_vf2");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    for target_nodes in [256usize, 4_096] {
        let target = linear_graph(target_nodes);
        assert_linear_graph(&target, target_nodes);
        assert_eq!(
            matcher.find_matches(&pattern, &target).len(),
            1,
            "bounded VF2 oracle"
        );
        group.throughput(Throughput::Elements(target_nodes as u64));
        group.bench_with_input(
            BenchmarkId::new("linear_subgraph", target_nodes),
            &target,
            |benchmark, target| {
                benchmark.iter(|| {
                    black_box(matcher.find_matches(black_box(&pattern), black_box(target)))
                });
            },
        );
    }

    group.finish();
}

criterion_group!(benches, benchmark_pattern_vf2);
criterion_main!(benches);
