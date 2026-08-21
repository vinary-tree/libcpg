mod support;

use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::{
    call_graph_projection, call_graph_sccs, wl_labels, CpgBuilder, CpgBuilderConfig,
    TreeSitterCpgBuilder,
};

use support::{CorpusFile, CROSS_LANGUAGE, RUST_LARGE, RUST_MEDIUM, RUST_SMALL};

const SMALL: [CorpusFile; 1] = [RUST_SMALL];
const MEDIUM: [CorpusFile; 1] = [RUST_MEDIUM];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CorpusSummary {
    files: usize,
    lines: usize,
    nodes: usize,
    edges: usize,
    functions: usize,
    call_components: usize,
    wl_labels: usize,
}

fn analyze(files: &[CorpusFile]) -> CorpusSummary {
    let builder =
        TreeSitterCpgBuilder::with_config(CpgBuilderConfig::default().with_import_resolution(true));
    let mut summary = CorpusSummary {
        files: files.len(),
        lines: 0,
        nodes: 0,
        edges: 0,
        functions: 0,
        call_components: 0,
        wl_labels: 0,
    };

    for file in files {
        let graph = builder
            .build(file.source, file.language)
            .expect("committed benchmark corpus parses");
        let calls = call_graph_projection(&graph);
        let sccs = call_graph_sccs(&graph);
        let labels = wl_labels(&calls, 4);

        summary.lines += file.source.lines().count();
        summary.nodes += graph.node_count();
        summary.edges += graph.edge_count();
        summary.functions += graph.functions().count();
        summary.call_components += sccs.components.len();
        summary.wl_labels += labels.len();
    }
    summary
}

fn benchmark_analyze_corpus(c: &mut Criterion) {
    let cases: [(&str, &[CorpusFile]); 4] = [
        ("rust_approximately_50_loc", &SMALL),
        ("rust_approximately_500_loc", &MEDIUM),
        ("rust_approximately_5000_loc", &RUST_LARGE),
        ("python_javascript_go_c", &CROSS_LANGUAGE),
    ];
    let mut group = c.benchmark_group("analyze_corpus");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(5));

    for (name, files) in cases {
        let oracle = analyze(files);
        assert!(oracle.nodes > 0, "corpus node oracle");
        assert!(oracle.edges > 0, "corpus edge oracle");
        assert!(oracle.functions > 0, "corpus function oracle");
        assert_eq!(
            oracle,
            analyze(files),
            "deterministic corpus-analysis oracle"
        );
        group.throughput(Throughput::Elements(oracle.nodes as u64));
        group.bench_with_input(
            BenchmarkId::new("end_to_end", name),
            &files,
            |benchmark, files| {
                benchmark.iter(|| black_box(analyze(black_box(files))));
            },
        );
    }

    group.finish();
}

criterion_group!(benches, benchmark_analyze_corpus);
criterion_main!(benches);
