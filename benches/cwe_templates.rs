use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::{
    analyze_builtin_cwes, CallGraphResolution, CodePropertyGraph, CpgEdgeKind, CpgNode,
    CpgNodeKind, CweAnalysisConfig, DfgEdgeKind, Language, MethodSignature, NodeId, SourceRange,
    Visibility,
};

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let id = cpg.add_node(node(kind).with_parent(parent));
    cpg.connect(parent, id, CpgEdgeKind::AstChild)
        .expect("benchmark AST endpoints exist");
    id
}

fn call(cpg: &mut CodePropertyGraph, parent: NodeId, path: &str) -> NodeId {
    let site = child(
        cpg,
        parent,
        CpgNodeKind::Call {
            target: None,
            is_method: path.contains('.') || path.contains("::"),
        },
    );
    child(
        cpg,
        site,
        CpgNodeKind::Identifier {
            name: path.into(),
            definition: None,
        },
    );
    site
}

fn fixture(population: usize) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    for index in 0..population {
        let owner = cpg.add_node(node(CpgNodeKind::Function {
            signature: Box::new(MethodSignature {
                name: format!("endpoint_{index}").into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            }),
        }));
        let source = call(&mut cpg, owner, "request.body");
        let sink = call(&mut cpg, owner, "sqlx::query");
        let argument = child(
            &mut cpg,
            sink,
            CpgNodeKind::Identifier {
                name: "body".into(),
                definition: Some(source),
            },
        );
        cpg.connect(source, argument, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
            .expect("benchmark DFG endpoints exist");
    }
    cpg
}

fn benchmark_cwe_templates(c: &mut Criterion) {
    let resolution = CallGraphResolution::default();
    let config = CweAnalysisConfig::default();
    let mut group = c.benchmark_group("cwe_templates");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    for population in [20usize, 100] {
        let cpg = fixture(population);
        let oracle = analyze_builtin_cwes(&cpg, &resolution, &config)
            .expect("valid benchmark fixture and built-in templates");
        assert!(oracle.is_complete(), "{oracle:#?}");
        assert_eq!(
            oracle
                .candidates
                .iter()
                .filter(|candidate| candidate.cwe == 89)
                .count(),
            population
        );
        assert_eq!(oracle.candidates.len(), population);

        group.throughput(Throughput::Elements(cpg.node_count() as u64));
        group.bench_with_input(
            BenchmarkId::new("ten_template_registry", population),
            &cpg,
            |benchmark, graph| {
                benchmark.iter(|| {
                    black_box(
                        analyze_builtin_cwes(
                            black_box(graph),
                            black_box(&resolution),
                            black_box(&config),
                        )
                        .expect("valid benchmark fixture and built-in templates"),
                    )
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, benchmark_cwe_templates);
criterion_main!(benches);
