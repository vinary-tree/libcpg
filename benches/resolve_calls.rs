mod support;

use std::time::Duration;

use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput,
};
use libcpg::{build_symbol_table, resolve_calls};

use support::{assert_resolver_fixture, resolver_fixture, ResolverCase};

fn benchmark_resolver(c: &mut Criterion) {
    let exact_cases = [
        (256usize, resolver_fixture(256, 256, ResolverCase::Exact)),
        (4_096, resolver_fixture(4_096, 4_096, ResolverCase::Exact)),
    ];
    let mut construction = c.benchmark_group("resolve_calls/symbol_table");
    construction.sample_size(10);
    construction.warm_up_time(Duration::from_secs(1));
    construction.measurement_time(Duration::from_secs(3));
    for (size, fixture) in &exact_cases {
        let mut oracle_graph = fixture.graph.clone();
        let table = build_symbol_table(&mut oracle_graph, &fixture.config);
        assert_eq!(table.functions_by_name.len(), fixture.symbol_count + 1);
        assert_eq!(
            table.functions_by_name.len(),
            table.functions_by_scope[&NodeId::new(0)].len(),
        );

        construction.throughput(Throughput::Elements(fixture.graph.node_count() as u64));
        construction.bench_with_input(
            BenchmarkId::from_parameter(size),
            fixture,
            |benchmark, fixture| {
                benchmark.iter_batched(
                    || fixture.graph.clone(),
                    |mut graph| {
                        black_box(build_symbol_table(
                            black_box(&mut graph),
                            black_box(&fixture.config),
                        ))
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }
    construction.finish();

    let cases = [
        resolver_fixture(256, 256, ResolverCase::Exact),
        resolver_fixture(4_096, 4_096, ResolverCase::Exact),
        resolver_fixture(256, 256, ResolverCase::Unresolved),
        resolver_fixture(4_096, 4_096, ResolverCase::Unresolved),
        resolver_fixture(256, 256, ResolverCase::Ambiguous),
        resolver_fixture(4_096, 4_096, ResolverCase::Ambiguous),
        resolver_fixture(1_024, 1_024, ResolverCase::DeepExact { scope_depth: 64 }),
    ];
    let mut resolution = c.benchmark_group("resolve_calls/full");
    resolution.sample_size(10);
    resolution.warm_up_time(Duration::from_secs(1));
    resolution.measurement_time(Duration::from_secs(3));
    for fixture in cases {
        let mut oracle_graph = fixture.graph.clone();
        let oracle = resolve_calls(&mut oracle_graph, &fixture.config);
        assert_resolver_fixture(&fixture, &oracle_graph, &oracle);
        let label = match fixture.case {
            ResolverCase::DeepExact { scope_depth } => {
                format!(
                    "{}/{}/depth_{scope_depth}",
                    fixture.case.name(),
                    fixture.symbol_count
                )
            }
            _ => format!("{}/{}", fixture.case.name(), fixture.symbol_count),
        };

        resolution.throughput(Throughput::Elements(fixture.calls.len() as u64));
        resolution.bench_with_input(
            BenchmarkId::from_parameter(label),
            &fixture,
            |benchmark, fixture| {
                benchmark.iter_batched(
                    || fixture.graph.clone(),
                    |mut graph| {
                        black_box(resolve_calls(
                            black_box(&mut graph),
                            black_box(&fixture.config),
                        ))
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }
    resolution.finish();
}

use libcpg::NodeId;

criterion_group!(benches, benchmark_resolver);
criterion_main!(benches);
