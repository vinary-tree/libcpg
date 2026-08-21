mod support;

use std::time::Duration;

use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput,
};
use libcpg::{CpgBuilder, PdgBuilder, TreeSitterCpgBuilder};

fn build_all_pdgs(graph: &mut libcpg::CodePropertyGraph, functions: &[libcpg::NodeId]) {
    let builder = PdgBuilder::new();
    for &function in functions {
        builder.build(graph, function);
    }
}

fn benchmark_pdg_construction(c: &mut Criterion) {
    let mut group = c.benchmark_group("pdg_construction");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    for corpus in [support::RUST_SMALL, support::RUST_MEDIUM] {
        let template = TreeSitterCpgBuilder::new()
            .build(corpus.source, corpus.language)
            .expect("PDG corpus parses");
        let functions: Vec<_> = template.functions().map(|node| node.id).collect();
        assert!(!functions.is_empty(), "PDG function oracle");
        assert_eq!(
            template.edges().filter(|edge| edge.kind.is_pdg()).count(),
            0,
            "PDG-free fixture oracle"
        );
        let mut oracle = template.clone();
        build_all_pdgs(&mut oracle, &functions);
        assert!(
            oracle.edges().any(|edge| edge.kind.is_pdg()),
            "PDG construction must materialize dependence evidence"
        );

        group.throughput(Throughput::Elements(template.node_count() as u64));
        group.bench_with_input(
            BenchmarkId::new("build_all_functions", corpus.name),
            &(&template, &functions),
            |benchmark, (template, functions)| {
                benchmark.iter_batched(
                    || (*template).clone(),
                    |mut graph| {
                        build_all_pdgs(&mut graph, functions);
                        black_box(graph)
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

criterion_group!(benches, benchmark_pdg_construction);
criterion_main!(benches);
