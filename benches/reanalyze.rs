mod support;

use std::fmt::Write as _;
use std::time::Duration;

use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput,
};
use libcpg::builder::{
    CpgBuilder, CpgBuilderConfig, FunctionAnalysisCache, FunctionAnalysisCacheLimits,
    TreeSitterCpgBuilder,
};
use libcpg::Language;

const FUNCTION_COUNT: usize = 64;
const STATEMENTS_PER_FUNCTION: usize = 32;

fn many_function_source(
    function_count: usize,
    statements_per_function: usize,
    edited_functions: &[usize],
) -> String {
    let mut source = String::new();
    for function in 0..function_count {
        writeln!(&mut source, "fn function_{function}(seed: i64) -> i64 {{")
            .expect("writing to a String cannot fail");
        writeln!(&mut source, "    let mut value = seed;")
            .expect("writing to a String cannot fail");
        for statement in 0..statements_per_function {
            writeln!(
                &mut source,
                "    value = value.wrapping_add({});",
                function * statements_per_function + statement
            )
            .expect("writing to a String cannot fail");
        }
        if edited_functions.contains(&function) {
            writeln!(&mut source, "    value = value.wrapping_mul(3);")
                .expect("writing to a String cannot fail");
        }
        writeln!(&mut source, "    value\n}}").expect("writing to a String cannot fail");
    }
    source
}

fn shared_dependency_source(offset: i64) -> String {
    let mut source = format!("const OFFSET: i64 = {offset};\n");
    for function in 0..FUNCTION_COUNT {
        writeln!(
            &mut source,
            "fn shared_{function}(seed: i64) -> i64 {{ seed + OFFSET + {function} }}"
        )
        .expect("writing to a String cannot fail");
    }
    source
}

fn builder(config: CpgBuilderConfig) -> TreeSitterCpgBuilder {
    TreeSitterCpgBuilder::with_config(config)
}

fn seeded_cache(builder: &TreeSitterCpgBuilder, source: &str) -> FunctionAnalysisCache {
    let mut cache = FunctionAnalysisCache::new();
    let graph = builder
        .build_with_analysis_cache(source, Language::Rust, &mut cache)
        .expect("cache seed parses");
    assert_eq!(graph.functions().count(), FUNCTION_COUNT);
    cache.reset_stats();
    cache
}

fn benchmark_reanalysis(c: &mut Criterion) {
    let original = many_function_source(FUNCTION_COUNT, STATEMENTS_PER_FUNCTION, &[]);
    let edited = many_function_source(
        FUNCTION_COUNT,
        STATEMENTS_PER_FUNCTION,
        &[FUNCTION_COUNT / 2],
    );
    let churned = many_function_source(
        FUNCTION_COUNT,
        STATEMENTS_PER_FUNCTION,
        &(0..FUNCTION_COUNT).step_by(2).collect::<Vec<_>>(),
    );
    let shared_before = shared_dependency_source(1);
    let shared_after = shared_dependency_source(2);
    let full_config = CpgBuilderConfig::default().with_import_resolution(true);
    let full_builder = builder(full_config.clone());
    let edited_oracle = full_builder
        .build(&edited, Language::Rust)
        .expect("edited benchmark corpus parses");
    assert_eq!(edited_oracle.functions().count(), FUNCTION_COUNT);

    let mut decomposition = c.benchmark_group("reanalyze_decomposition");
    decomposition.sample_size(10);
    decomposition.warm_up_time(Duration::from_secs(1));
    decomposition.measurement_time(Duration::from_secs(5));
    decomposition.throughput(Throughput::Elements(edited_oracle.node_count() as u64));
    for (name, config) in [
        (
            "parse_ast",
            CpgBuilderConfig::default().with_cfg(false).with_dfg(false),
        ),
        (
            "parse_ast_cfg",
            CpgBuilderConfig::default().with_cfg(true).with_dfg(false),
        ),
        (
            "parse_ast_cfg_dfg",
            CpgBuilderConfig::default().with_cfg(true).with_dfg(true),
        ),
        ("parse_ast_cfg_resolve_dfg", full_config),
    ] {
        let stage_builder = builder(config);
        decomposition.bench_with_input(
            BenchmarkId::new("edit_one_of_64", name),
            &stage_builder,
            |benchmark, stage_builder| {
                benchmark.iter(|| {
                    black_box(
                        stage_builder
                            .build(black_box(&edited), Language::Rust)
                            .expect("stage corpus remains valid"),
                    )
                });
            },
        );
    }
    decomposition.finish();

    let mut workloads = c.benchmark_group("reanalyze_production_cache");
    workloads.sample_size(10);
    workloads.warm_up_time(Duration::from_secs(1));
    workloads.measurement_time(Duration::from_secs(5));
    workloads.throughput(Throughput::Elements(edited_oracle.node_count() as u64));

    workloads.bench_function("fresh_edit_one_of_64", |benchmark| {
        benchmark.iter(|| {
            black_box(
                full_builder
                    .build(black_box(&edited), Language::Rust)
                    .expect("fresh corpus remains valid"),
            )
        });
    });

    workloads.bench_function("cold_empty_cache", |benchmark| {
        benchmark.iter_batched(
            FunctionAnalysisCache::new,
            |mut cache| {
                let graph = full_builder
                    .build_with_analysis_cache(black_box(&edited), Language::Rust, &mut cache)
                    .expect("cold cached corpus remains valid");
                black_box((graph, cache))
            },
            BatchSize::SmallInput,
        );
    });

    let mut all_hit_cache = seeded_cache(&full_builder, &original);
    workloads.bench_function("all_hit_unchanged_64", |benchmark| {
        benchmark.iter(|| {
            black_box(
                full_builder
                    .build_with_analysis_cache(
                        black_box(&original),
                        Language::Rust,
                        &mut all_hit_cache,
                    )
                    .expect("all-hit corpus remains valid"),
            )
        });
    });

    workloads.bench_function("edit_one_of_64", |benchmark| {
        benchmark.iter_batched(
            || seeded_cache(&full_builder, &original),
            |mut cache| {
                let graph = full_builder
                    .build_with_analysis_cache(black_box(&edited), Language::Rust, &mut cache)
                    .expect("edit-one corpus remains valid");
                let stats = cache.stats();
                assert_eq!(stats.cfg_hits, (FUNCTION_COUNT - 1) as u64);
                assert_eq!(stats.dfg_hits, (FUNCTION_COUNT - 1) as u64);
                black_box((graph, cache))
            },
            BatchSize::SmallInput,
        );
    });

    workloads.bench_function("edit_shared_dependency", |benchmark| {
        benchmark.iter_batched(
            || seeded_cache(&full_builder, &shared_before),
            |mut cache| {
                let graph = full_builder
                    .build_with_analysis_cache(black_box(&shared_after), Language::Rust, &mut cache)
                    .expect("shared-dependency corpus remains valid");
                assert!(cache.stats().dfg_dependency_bypasses >= FUNCTION_COUNT as u64);
                black_box((graph, cache))
            },
            BatchSize::SmallInput,
        );
    });

    workloads.bench_function("fresh_shared_dependency", |benchmark| {
        benchmark.iter(|| {
            black_box(
                full_builder
                    .build(black_box(&shared_after), Language::Rust)
                    .expect("fresh shared-dependency corpus remains valid"),
            )
        });
    });

    workloads.bench_function("low_hit_half_churn", |benchmark| {
        benchmark.iter_batched(
            || seeded_cache(&full_builder, &original),
            |mut cache| {
                let graph = full_builder
                    .build_with_analysis_cache(black_box(&churned), Language::Rust, &mut cache)
                    .expect("half-churn corpus remains valid");
                black_box((graph, cache))
            },
            BatchSize::SmallInput,
        );
    });

    workloads.bench_function("fresh_half_churn", |benchmark| {
        benchmark.iter(|| {
            black_box(
                full_builder
                    .build(black_box(&churned), Language::Rust)
                    .expect("fresh half-churn corpus remains valid"),
            )
        });
    });

    workloads.bench_function("bounded_cache_eviction", |benchmark| {
        benchmark.iter_batched(
            || {
                let mut cache = FunctionAnalysisCache::with_limits(
                    FunctionAnalysisCacheLimits::new(16, 2 * 1024 * 1024),
                );
                let _ = full_builder
                    .build_with_analysis_cache(&original, Language::Rust, &mut cache)
                    .expect("bounded cache seed parses");
                cache.reset_stats();
                cache
            },
            |mut cache| {
                let graph = full_builder
                    .build_with_analysis_cache(black_box(&edited), Language::Rust, &mut cache)
                    .expect("bounded-cache corpus remains valid");
                assert!(cache.stats().evictions > 0);
                black_box((graph, cache))
            },
            BatchSize::SmallInput,
        );
    });

    let mut disabled_cache =
        FunctionAnalysisCache::with_limits(FunctionAnalysisCacheLimits::new(0, 0));
    workloads.bench_function("disabled_cache_key_overhead", |benchmark| {
        benchmark.iter(|| {
            black_box(
                full_builder
                    .build_with_analysis_cache(
                        black_box(&edited),
                        Language::Rust,
                        &mut disabled_cache,
                    )
                    .expect("disabled-cache corpus remains valid"),
            )
        });
    });
    workloads.finish();
}

criterion_group!(benches, benchmark_reanalysis);
criterion_main!(benches);
