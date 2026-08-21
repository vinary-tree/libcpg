mod support;

use std::time::Duration;

use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput,
};
use libcpg::{CpgBuilder, CpgBuilderConfig, DfgExtractor, TreeSitterCpgBuilder};

fn benchmark_dfg_extraction(c: &mut Criterion) {
    let builder = TreeSitterCpgBuilder::with_config(CpgBuilderConfig::default().with_dfg(false));
    let mut group = c.benchmark_group("dfg_extraction");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    for corpus in [support::RUST_SMALL, support::RUST_MEDIUM] {
        let template = builder
            .build(corpus.source, corpus.language)
            .expect("DFG corpus parses");
        assert_eq!(
            template.edges().filter(|edge| edge.kind.is_dfg()).count(),
            0,
            "DFG-disabled fixture oracle"
        );
        let mut oracle = template.clone();
        DfgExtractor::new().extract(&mut oracle);
        assert!(
            oracle.edges().any(|edge| edge.kind.is_dfg()),
            "DFG extraction must materialize data-flow evidence"
        );

        group.throughput(Throughput::Elements(template.node_count() as u64));
        group.bench_with_input(
            BenchmarkId::new("extract", corpus.name),
            &template,
            |benchmark, template| {
                benchmark.iter_batched(
                    || template.clone(),
                    |mut graph| {
                        DfgExtractor::new().extract(&mut graph);
                        black_box(graph)
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

criterion_group!(benches, benchmark_dfg_extraction);
criterion_main!(benches);
