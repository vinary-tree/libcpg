use std::sync::Arc;
use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::pattern::{detect_clones, CloneOptions, CloneType};
use libcpg::{
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, Language, MethodSignature, NodeId,
    SourceRange, Visibility,
};

#[derive(Clone, Copy)]
enum Case {
    Type1Group,
    Type3Insertion,
    Type4Reordering,
}

impl Case {
    fn name(self) -> &'static str {
        match self {
            Self::Type1Group => "type1_group",
            Self::Type3Insertion => "type3_insertion",
            Self::Type4Reordering => "type4_reordering",
        }
    }
}

fn function() -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: Box::new(MethodSignature {
            name: Arc::from("clone_fixture"),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        }),
    }
}

fn identifier(name: &'static str) -> CpgNodeKind {
    CpgNodeKind::Identifier {
        name: Arc::from(name),
        definition: None,
    }
}

fn add_function(
    graph: &mut CodePropertyGraph,
    body: impl IntoIterator<Item = CpgNodeKind>,
    add_pdg_star: bool,
) -> NodeId {
    let root = graph.add_node(CpgNode::new(
        NodeId::new(0),
        function(),
        SourceRange::default(),
    ));
    for kind in body {
        let child = graph.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()));
        graph.connect(root, child, CpgEdgeKind::AstChild);
        if add_pdg_star {
            graph.connect(root, child, CpgEdgeKind::ControlDependence);
        }
    }
    root
}

fn fixture(function_count: usize, case: Case) -> CodePropertyGraph {
    let mut graph = CodePropertyGraph::new(Language::Rust);
    for index in 0..function_count {
        match case {
            Case::Type1Group => {
                add_function(
                    &mut graph,
                    [identifier("value"), CpgNodeKind::Return],
                    false,
                );
            }
            Case::Type3Insertion => {
                let mut body = vec![
                    identifier("a"),
                    identifier("b"),
                    identifier("c"),
                    identifier("d"),
                ];
                if index % 2 == 1 {
                    body.push(CpgNodeKind::Return);
                }
                add_function(&mut graph, body, false);
            }
            Case::Type4Reordering => {
                let body = if index % 2 == 0 {
                    vec![identifier("value"), CpgNodeKind::Return]
                } else {
                    vec![CpgNodeKind::Return, identifier("value")]
                };
                add_function(&mut graph, body, true);
            }
        }
    }
    graph
}

fn assert_oracle(graph: &CodePropertyGraph, function_count: usize, case: Case) {
    let classes = detect_clones(graph, &CloneOptions::default()).expect("valid clone fixture");
    assert_eq!(graph.functions().count(), function_count);
    match case {
        Case::Type1Group => {
            assert_eq!(classes.len(), 1);
            assert_eq!(classes[0].clone_type, CloneType::Type1);
            assert_eq!(classes[0].members.len(), function_count);
            assert_eq!(classes[0].similarity, 1.0);
        }
        Case::Type3Insertion => {
            assert_eq!(
                classes
                    .iter()
                    .filter(|class| class.clone_type == CloneType::Type1)
                    .count(),
                2
            );
            let near = classes
                .iter()
                .find(|class| class.clone_type == CloneType::Type3)
                .expect("cross-shape Type-3 class");
            assert_eq!(near.members.len(), function_count);
            assert!((near.similarity - 5.0 / 6.0).abs() < 1.0e-12);
        }
        Case::Type4Reordering => {
            assert_eq!(
                classes
                    .iter()
                    .filter(|class| class.clone_type == CloneType::Type1)
                    .count(),
                2
            );
            let reordered = classes
                .iter()
                .find(|class| class.clone_type == CloneType::Type4)
                .expect("cross-order Type-4 class");
            assert_eq!(reordered.members.len(), function_count);
            assert_eq!(reordered.similarity, 1.0);
        }
    }
}

fn benchmark_clones(c: &mut Criterion) {
    let options = CloneOptions::default();
    let mut group = c.benchmark_group("detect_clones");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    for function_count in [16usize, 64] {
        for case in [
            Case::Type1Group,
            Case::Type3Insertion,
            Case::Type4Reordering,
        ] {
            let graph = fixture(function_count, case);
            assert_oracle(&graph, function_count, case);
            group.throughput(Throughput::Elements(function_count as u64));
            group.bench_with_input(
                BenchmarkId::new(case.name(), function_count),
                &graph,
                |benchmark, graph| {
                    benchmark.iter(|| {
                        black_box(
                            detect_clones(black_box(graph), black_box(&options))
                                .expect("valid clone fixture"),
                        )
                    });
                },
            );
        }
    }
    group.finish();
}

criterion_group!(benches, benchmark_clones);
criterion_main!(benches);
