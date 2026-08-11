use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::{
    betweenness_centrality, burt_constraint, closeness_centrality, edge_betweenness,
    eigenvector_centrality, harmonic_centrality, hits, pagerank, BetweennessOptions,
    CentralityBudget, GraphProjection, IterationOptions, NodeId, PageRankOptions, ProjectionKind,
};

#[derive(Clone, Copy)]
enum GraphShape {
    Path,
    Ring,
    Clustered,
}

impl GraphShape {
    fn name(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Ring => "ring",
            Self::Clustered => "clustered",
        }
    }
}

fn projection(node_count: usize, shape: GraphShape) -> GraphProjection {
    let mut edges = (0..node_count.saturating_sub(1))
        .map(|source| (source, source + 1))
        .collect::<Vec<_>>();
    match shape {
        GraphShape::Path => {}
        GraphShape::Ring => {
            if node_count > 1 {
                edges.push((node_count - 1, 0));
            }
        }
        GraphShape::Clustered => {
            let split = node_count / 2;
            edges.retain(|&(source, target)| target == source + 1 && target != split);
            for (start, end) in [(0, split), (split, node_count)] {
                if end.saturating_sub(start) > 1 {
                    edges.push((end - 1, start));
                    for node in start..end - 1 {
                        edges.push((node + 1, node));
                    }
                    edges.push((start, end - 1));
                }
            }
            if split > 0 && split < node_count {
                edges.push((split - 1, split));
            }
        }
    }
    GraphProjection::from_edges(
        ProjectionKind::External,
        (0..node_count as u32).map(NodeId::new),
        edges
            .into_iter()
            .map(|(source, target)| (NodeId::new(source as u32), NodeId::new(target as u32), 1.0)),
    )
}

fn assert_iterative_oracles(graph: &GraphProjection, shape: GraphShape) {
    let page = pagerank(
        graph,
        PageRankOptions {
            max_iterations: 1_000,
            tolerance: 1.0e-10,
            ..PageRankOptions::default()
        },
    );
    assert!(page.converged, "PageRank benchmark convergence oracle");
    assert_eq!(page.nodes, graph.nodes, "PageRank alignment oracle");
    assert!(
        (page.scores.iter().sum::<f64>() - 1.0).abs() <= 1.0e-8,
        "PageRank mass oracle"
    );
    if matches!(shape, GraphShape::Ring) {
        assert!(page
            .scores
            .iter()
            .all(|score| (*score - 1.0 / graph.node_count() as f64).abs() <= 1.0e-12));
    }

    let options = IterationOptions {
        max_iterations: 1_000,
        tolerance: 1.0e-10,
    };
    let eigen = eigenvector_centrality(graph, options);
    assert!(eigen.iterations <= options.max_iterations);
    assert!(eigen.scores.iter().all(|score| score.is_finite()));
    assert!(
        (eigen.scores.iter().map(|score| score * score).sum::<f64>() - 1.0).abs() <= 1.0e-8,
        "eigenvector L2-normalization oracle"
    );
    let hits = hits(graph, options);
    assert_eq!(hits.hubs.nodes, graph.nodes, "HITS alignment oracle");
    assert!(hits.hubs.scores.iter().all(|score| score.is_finite()));
    assert!(hits
        .authorities
        .scores
        .iter()
        .all(|score| score.is_finite()));
}

fn assert_traversal_oracles(graph: &GraphProjection, shape: GraphShape) {
    let betweenness = betweenness_centrality(graph, BetweennessOptions::default())
        .expect("benchmark projection is within the Brandes budget");
    let closeness = closeness_centrality(graph, CentralityBudget::default())
        .expect("benchmark projection is within the closeness budget");
    let harmonic = harmonic_centrality(graph, CentralityBudget::default())
        .expect("benchmark projection is within the harmonic budget");
    let edges = edge_betweenness(graph, CentralityBudget::default())
        .expect("benchmark projection is within the edge-Brandes budget");
    assert!(betweenness.converged && closeness.converged && harmonic.converged);
    assert_eq!(edges.edges.len(), edges.scores.len());
    if graph.node_count() > 2 && matches!(shape, GraphShape::Path) {
        let middle = graph.node_count() / 2;
        assert!(betweenness.scores[middle] > betweenness.scores[0]);
        assert!(closeness.scores[middle] > closeness.scores[0]);
        assert!(harmonic.scores[middle] > harmonic.scores[0]);
    }
    if matches!(shape, GraphShape::Ring) {
        assert!(closeness
            .scores
            .windows(2)
            .all(|pair| (pair[0] - pair[1]).abs() <= 1.0e-12));
    }
    if matches!(shape, GraphShape::Clustered) && graph.node_count() > 3 {
        let bridge = (
            NodeId::new((graph.node_count() / 2 - 1) as u32),
            NodeId::new((graph.node_count() / 2) as u32),
        );
        let position = edges
            .edges
            .binary_search(&bridge)
            .expect("cluster benchmark bridge edge");
        let maximum = edges.scores.iter().copied().fold(0.0_f64, f64::max);
        assert_eq!(edges.scores[position], maximum, "bridge edge oracle");
    }
    let constraint = burt_constraint(graph, CentralityBudget::default())
        .expect("benchmark projection is within the Burt budget");
    assert!(constraint.scores.iter().all(|score| score.is_finite()));
}

fn benchmark_iterative(c: &mut Criterion) {
    let mut group = c.benchmark_group("centrality_iterative");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
    for node_count in [256usize, 1_024] {
        for shape in [GraphShape::Path, GraphShape::Ring, GraphShape::Clustered] {
            let graph = projection(node_count, shape);
            assert_iterative_oracles(&graph, shape);
            group.throughput(Throughput::Elements(node_count as u64));
            group.bench_with_input(
                BenchmarkId::new(format!("pagerank_{}", shape.name()), node_count),
                &graph,
                |benchmark, graph| {
                    benchmark.iter(|| {
                        black_box(pagerank(
                            black_box(graph),
                            black_box(PageRankOptions::default()),
                        ))
                    });
                },
            );
            group.bench_with_input(
                BenchmarkId::new(format!("eigenvector_{}", shape.name()), node_count),
                &graph,
                |benchmark, graph| {
                    benchmark.iter(|| {
                        black_box(eigenvector_centrality(
                            black_box(graph),
                            black_box(IterationOptions::default()),
                        ))
                    });
                },
            );
        }
    }
    group.finish();
}

fn benchmark_traversals(c: &mut Criterion) {
    let mut group = c.benchmark_group("centrality_all_sources");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    for node_count in [128usize, 256] {
        for shape in [GraphShape::Path, GraphShape::Ring, GraphShape::Clustered] {
            let graph = projection(node_count, shape);
            assert_traversal_oracles(&graph, shape);
            group.throughput(Throughput::Elements(node_count as u64));
            group.bench_with_input(
                BenchmarkId::new(format!("brandes_{}", shape.name()), node_count),
                &graph,
                |benchmark, graph| {
                    benchmark.iter(|| {
                        black_box(
                            betweenness_centrality(
                                black_box(graph),
                                black_box(BetweennessOptions::default()),
                            )
                            .expect("timed projection is within the Brandes budget"),
                        )
                    });
                },
            );
            group.bench_with_input(
                BenchmarkId::new(format!("closeness_{}", shape.name()), node_count),
                &graph,
                |benchmark, graph| {
                    benchmark.iter(|| {
                        black_box(
                            closeness_centrality(
                                black_box(graph),
                                black_box(CentralityBudget::default()),
                            )
                            .expect("timed projection is within the closeness budget"),
                        )
                    });
                },
            );
        }
    }
    group.finish();
}

criterion_group!(benches, benchmark_iterative, benchmark_traversals);
criterion_main!(benches);
