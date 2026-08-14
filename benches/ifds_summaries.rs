use std::collections::BTreeSet;
use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::{
    call_graph_sccs, solve_summaries, CalleeRef, CodePropertyGraph, CpgEdgeKind, CpgNode,
    CpgNodeKind, IfdsCaps, IfdsLattice, Language, MethodSignature, NodeId, SourceRange, SummaryCtx,
    SummaryDomain, Visibility,
};

#[derive(Clone, Default)]
struct ReachSummary(BTreeSet<NodeId>);

impl IfdsLattice for ReachSummary {
    fn join(&mut self, other: &Self) -> bool {
        let before = self.0.len();
        self.0.extend(other.0.iter().copied());
        self.0.len() != before
    }

    fn subsumes(&self, other: &Self) -> bool {
        self.0.is_superset(&other.0)
    }
}

struct Reachability;

impl SummaryDomain for Reachability {
    type Fact = NodeId;
    type Summary = ReachSummary;

    fn seed_facts(&self, _cpg: &CodePropertyGraph, function: NodeId) -> Vec<(NodeId, Self::Fact)> {
        vec![(function, function)]
    }

    fn summarize(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
        ctx: &SummaryCtx<'_, Self>,
    ) -> Self::Summary {
        let mut summary = ReachSummary(BTreeSet::from([function]));
        for call in cpg.ast_descendants(function).into_iter().filter(|&node| {
            cpg.node(node)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Call { .. }))
        }) {
            if let CalleeRef::Resolved(callee) = (ctx.resolve)(call) {
                summary.0.insert(callee);
                if let Some(callee_summary) = ctx.summaries.get(&callee) {
                    summary.0.extend(callee_summary.0.iter().copied());
                }
            }
        }
        summary
    }
}

#[derive(Clone, Copy)]
enum Shape {
    Path,
    Clustered,
}

impl Shape {
    const fn name(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Clustered => "clustered-recursion",
        }
    }
}

fn function_node(name: String) -> CpgNode {
    CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Function {
            signature: MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            },
        },
        SourceRange::default(),
    )
}

fn fixture(function_count: usize, shape: Shape) -> (CodePropertyGraph, Vec<NodeId>) {
    const CLUSTER_SIZE: usize = 8;

    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let functions: Vec<_> = (0..function_count)
        .map(|index| cpg.add_node(function_node(format!("f{index}"))))
        .collect();
    let mut edges = Vec::new();
    match shape {
        Shape::Path => edges.extend((0..function_count - 1).map(|source| (source, source + 1))),
        Shape::Clustered => {
            for start in (0..function_count).step_by(CLUSTER_SIZE) {
                let end = (start + CLUSTER_SIZE).min(function_count);
                if end - start == 1 {
                    edges.push((start, start));
                } else {
                    edges.extend((start..end).map(|source| {
                        let target = if source + 1 == end { start } else { source + 1 };
                        (source, target)
                    }));
                }
                if end < function_count {
                    edges.push((end - 1, end));
                }
            }
        }
    }

    for (source, target) in edges {
        let call = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Call {
                target: Some(functions[target]),
                is_method: false,
            },
            SourceRange::default(),
        ));
        cpg.connect(functions[source], call, CpgEdgeKind::AstChild)
            .expect("benchmark AST endpoints exist");
        cpg.node_mut(call).expect("benchmark call exists").parent = Some(functions[source]);
    }
    (cpg, functions)
}

fn benchmark_ifds_summaries(c: &mut Criterion) {
    let mut group = c.benchmark_group("ifds_summaries");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));

    for function_count in [256usize, 1_024] {
        for shape in [Shape::Path, Shape::Clustered] {
            let (cpg, functions) = fixture(function_count, shape);
            let call_graph = call_graph_sccs(&cpg);
            let caps = IfdsCaps::default();

            // Deterministic oracle is asserted before the timed region.
            let oracle = solve_summaries(&cpg, &Reachability, &call_graph, &caps);
            assert!(oracle.is_complete());
            assert_eq!(oracle.summaries[&functions[0]].0.len(), function_count);
            assert!(oracle
                .summaries
                .values()
                .all(|summary| !summary.0.is_empty()));

            group.throughput(Throughput::Elements(function_count as u64));
            group.bench_with_input(
                BenchmarkId::new(shape.name(), function_count),
                &function_count,
                |benchmark, _| {
                    benchmark.iter(|| {
                        black_box(solve_summaries(
                            black_box(&cpg),
                            black_box(&Reachability),
                            black_box(&call_graph),
                            black_box(&caps),
                        ))
                    });
                },
            );
        }
    }
    group.finish();
}

criterion_group!(benches, benchmark_ifds_summaries);
criterion_main!(benches);
