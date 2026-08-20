use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::{
    analyze_api_usage_mining, ApiUsageMiningConfig, ApiUsageMisuseKind, CfgEdgeKind,
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, DfgEdgeKind, Language, MethodSignature,
    NamedTemporalProperty, NodeId, PopulationThreshold, PropertyFsm, SourceRange,
    TemporalPropertyFindingKind, Visibility,
};

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn function(name: String) -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: MethodSignature {
            name: name.into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        },
    }
}

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let id = cpg.add_node(node(kind).with_parent(parent));
    cpg.connect(parent, id, CpgEdgeKind::AstChild)
        .expect("benchmark AST endpoints exist");
    cpg.node_mut(parent)
        .expect("benchmark parent exists")
        .children
        .push(id);
    id
}

fn call(cpg: &mut CodePropertyGraph, owner: NodeId, name: &str) -> NodeId {
    let site = child(
        cpg,
        owner,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    child(
        cpg,
        site,
        CpgNodeKind::Identifier {
            name: name.into(),
            definition: None,
        },
    );
    site
}

fn cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId) {
    cpg.connect(
        source,
        target,
        CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
    )
    .expect("benchmark CFG endpoints exist");
}

fn fixture(population: usize) -> CodePropertyGraph {
    assert_eq!(population % 20, 0);
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    for index in 0..population {
        let owner = cpg.add_node(node(function(format!("peer_{index}"))));
        let lock = call(&mut cpg, owner, "lock");
        let exit = child(&mut cpg, owner, CpgNodeKind::Return);
        cfg(&mut cpg, owner, lock);
        if index % 20 != 19 {
            let unlock = call(&mut cpg, owner, "unlock");
            let argument = child(
                &mut cpg,
                unlock,
                CpgNodeKind::Identifier {
                    name: "guard".into(),
                    definition: Some(lock),
                },
            );
            cpg.connect(lock, argument, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
                .expect("benchmark DFG endpoints exist");
            cfg(&mut cpg, lock, unlock);
            cfg(&mut cpg, unlock, exit);
        } else {
            cfg(&mut cpg, lock, exit);
        }
    }
    cpg
}

fn benchmark_api_usage_mining(c: &mut Criterion) {
    let property = NamedTemporalProperty::new(
        "release-after-acquire",
        PropertyFsm::new("error")
            .with_allocation("lock", "acquired")
            .with_event("unlock")
            .with_transition("acquired", "unlock", "released")
            .with_accepting("released"),
    );
    let mut group = c.benchmark_group("api_usage_mining");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    for population in [20usize, 100] {
        let cpg = fixture(population);
        let config = ApiUsageMiningConfig {
            minimum_support: PopulationThreshold::new(19, 20),
            minimum_population: population,
            ..ApiUsageMiningConfig::default()
        };
        let oracle = analyze_api_usage_mining(&cpg, std::slice::from_ref(&property), &config)
            .expect("valid benchmark fixture");
        assert!(oracle.is_complete());
        assert_eq!(
            oracle
                .findings
                .iter()
                .filter(|finding| matches!(
                    &finding.kind,
                    ApiUsageMisuseKind::MissingFollowingCall { expected }
                        if expected.name() == "unlock"
                ))
                .count(),
            population / 20
        );
        assert_eq!(
            oracle
                .temporal_findings
                .iter()
                .filter(|finding| matches!(
                    &finding.kind,
                    TemporalPropertyFindingKind::NonAcceptingExit { state }
                        if state.as_ref() == "acquired"
                ))
                .count(),
            population / 20
        );

        group.throughput(Throughput::Elements(population as u64));
        group.bench_with_input(
            BenchmarkId::new("usage_and_temporal", population),
            &cpg,
            |benchmark, graph| {
                benchmark.iter(|| {
                    black_box(
                        analyze_api_usage_mining(
                            black_box(graph),
                            black_box(std::slice::from_ref(&property)),
                            black_box(&config),
                        )
                        .expect("valid benchmark fixture"),
                    )
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, benchmark_api_usage_mining);
criterion_main!(benches);
