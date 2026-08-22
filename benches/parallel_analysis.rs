use std::time::Duration;

use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput,
};
use libcpg::pattern::Vf2Matcher;
use libcpg::{CfgExtractor, CodePropertyGraph, DfgExtractor, PdgBuilder};

mod support;

const WORKERS: [usize; 6] = [1, 2, 4, 8, 16, 32];
const FUNCTION_COUNT: usize = 512;
const STATEMENTS_PER_FUNCTION: usize = 16;
const PATTERN_COUNT: usize = 128;

fn serial_pipeline(cpg: &mut CodePropertyGraph) {
    CfgExtractor::new().extract(cpg);
    DfgExtractor::new().extract(cpg);
    PdgBuilder::new().build_all(cpg);
}

fn parallel_pipeline(cpg: &mut CodePropertyGraph) {
    CfgExtractor::new().extract_parallel(cpg);
    DfgExtractor::new().extract_parallel(cpg);
    PdgBuilder::new().build_all_parallel(cpg);
}

fn benchmark_parallel_analysis(c: &mut Criterion) {
    let template = support::parallel_analysis_graph(FUNCTION_COUNT, STATEMENTS_PER_FUNCTION);
    assert_eq!(template.functions().count(), FUNCTION_COUNT);
    let mut serial_oracle = template.clone();
    serial_pipeline(&mut serial_oracle);
    assert!(serial_oracle.stats().cfg_edges > 0);
    assert!(serial_oracle.stats().dfg_edges > 0);
    assert!(serial_oracle.edges().any(|edge| edge.kind.is_pdg()));

    let mut group = c.benchmark_group("parallel_analysis");
    group.sample_size(15);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
    group.throughput(Throughput::Elements(template.node_count() as u64));
    group.bench_function("serial", |benchmark| {
        benchmark.iter_batched(
            || template.clone(),
            |mut graph| {
                serial_pipeline(&mut graph);
                black_box(graph)
            },
            BatchSize::SmallInput,
        );
    });

    for workers in WORKERS {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .stack_size(256 * 1024)
            .build()
            .expect("bounded benchmark Rayon pool");
        let mut oracle = template.clone();
        pool.install(|| parallel_pipeline(&mut oracle));
        assert_eq!(
            oracle.edges().cloned().collect::<Vec<_>>(),
            serial_oracle.edges().cloned().collect::<Vec<_>>()
        );
        group.bench_with_input(
            BenchmarkId::new("parallel", workers),
            &workers,
            |benchmark, _| {
                benchmark.iter_batched(
                    || template.clone(),
                    |mut graph| {
                        pool.install(|| parallel_pipeline(&mut graph));
                        black_box(graph)
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();

    let target = serial_oracle;
    let first_function = target.functions().next().expect("fixture function").id;
    let mut pattern_nodes = vec![first_function];
    pattern_nodes.extend(target.ast_descendants(first_function).into_iter().take(8));
    let pattern = target.subgraph(&pattern_nodes);
    let patterns: Vec<_> = (0..PATTERN_COUNT).map(|_| pattern.clone()).collect();
    let matcher = Vf2Matcher::new()
        .with_strict_kinds(true)
        .with_strict_edges(true)
        .with_max_matches(1);
    let serial_matches = matcher.find_matches_many(&patterns, &target);
    assert_eq!(serial_matches.len(), PATTERN_COUNT);
    assert!(serial_matches.iter().all(|matches| matches.len() == 1));

    let mut group = c.benchmark_group("parallel_vf2_batch");
    group.sample_size(15);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
    group.throughput(Throughput::Elements(PATTERN_COUNT as u64));
    group.bench_function("serial", |benchmark| {
        benchmark.iter(|| {
            black_box(matcher.find_matches_many(black_box(&patterns), black_box(&target)))
        });
    });
    for workers in WORKERS {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .stack_size(256 * 1024)
            .build()
            .expect("bounded benchmark Rayon pool");
        group.bench_with_input(
            BenchmarkId::new("parallel", workers),
            &workers,
            |benchmark, _| {
                benchmark.iter(|| {
                    black_box(pool.install(|| {
                        matcher.find_matches_many_parallel(black_box(&patterns), black_box(&target))
                    }))
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, benchmark_parallel_analysis);
criterion_main!(benches);
