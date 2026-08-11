use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::{
    analyze_dsm, classify_core_periphery, CorePeriphery, DsmAnalysis, GraphProjection, NodeId,
    ProjectionKind,
};

#[derive(Clone, Copy)]
enum GraphShape {
    Path,
    Ring,
}

impl GraphShape {
    fn name(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::Ring => "ring",
        }
    }
}

fn projection(node_count: usize, shape: GraphShape) -> GraphProjection {
    let nodes = (0..node_count as u32).map(NodeId::new);
    let mut edges = (0..node_count.saturating_sub(1))
        .map(|source| {
            (
                NodeId::new(source as u32),
                NodeId::new(source as u32 + 1),
                1.0,
            )
        })
        .collect::<Vec<_>>();
    if matches!(shape, GraphShape::Ring) && node_count > 1 {
        edges.push((NodeId::new(node_count as u32 - 1), NodeId::new(0), 1.0));
    }
    GraphProjection::from_edges(ProjectionKind::External, nodes, edges)
}

fn assert_oracle(graph: &GraphProjection, shape: GraphShape) -> DsmAnalysis {
    let n = graph.node_count();
    let analysis = analyze_dsm(graph);
    assert!(analysis.complete, "DSM benchmark completion oracle");
    assert!(!analysis.capped, "DSM benchmark cap oracle");

    match shape {
        GraphShape::Path => {
            assert_eq!(
                analysis.visibility_fan_out,
                (0..n).map(|node| n - node - 1).collect::<Vec<_>>(),
                "path fan-out oracle"
            );
            assert_eq!(
                analysis.visibility_fan_in,
                (0..n).collect::<Vec<_>>(),
                "path fan-in oracle"
            );
            let expected = (n.saturating_sub(1)) as f64 / (2.0 * n as f64);
            assert!(
                (analysis.propagation_cost - expected).abs() <= f64::EPSILON,
                "path propagation-cost oracle"
            );
        }
        GraphShape::Ring => {
            assert_eq!(analysis.visibility_fan_out, vec![n - 1; n]);
            assert_eq!(analysis.visibility_fan_in, vec![n - 1; n]);
            let expected = (n - 1) as f64 / n as f64;
            assert!(
                (analysis.propagation_cost - expected).abs() <= f64::EPSILON,
                "ring propagation-cost oracle"
            );
        }
    }

    let classification = classify_core_periphery(graph, &analysis);
    assert!(classification.complete);
    assert_eq!(classification.classes.len(), n);
    match shape {
        GraphShape::Path => {
            let mut expected_classes = vec![CorePeriphery::Control; n / 2];
            expected_classes.extend(vec![CorePeriphery::Shared; n - n / 2]);
            assert_eq!(classification.classes, expected_classes);
            assert!(classification.cyclic_core.is_empty());
        }
        GraphShape::Ring => {
            assert!(classification
                .classes
                .iter()
                .all(|class| *class == CorePeriphery::Core));
            assert_eq!(classification.cyclic_core, (0..n).collect::<Vec<_>>());
        }
    }
    analysis
}

fn benchmark_dsm(c: &mut Criterion) {
    let mut visibility = c.benchmark_group("dsm_visibility");
    visibility.sample_size(20);
    visibility.warm_up_time(Duration::from_secs(1));
    visibility.measurement_time(Duration::from_secs(3));
    for node_count in [256usize, 1_024] {
        for shape in [GraphShape::Path, GraphShape::Ring] {
            let graph = projection(node_count, shape);
            assert_oracle(&graph, shape);
            visibility.throughput(Throughput::Elements(node_count as u64));
            visibility.bench_with_input(
                BenchmarkId::new(shape.name(), node_count),
                &graph,
                |benchmark, graph| {
                    benchmark.iter(|| black_box(analyze_dsm(black_box(graph))));
                },
            );
        }
    }
    visibility.finish();

    let mut classification = c.benchmark_group("dsm_classification");
    classification.sample_size(20);
    classification.warm_up_time(Duration::from_secs(1));
    classification.measurement_time(Duration::from_secs(3));
    for node_count in [256usize, 1_024] {
        for shape in [GraphShape::Path, GraphShape::Ring] {
            let graph = projection(node_count, shape);
            let analysis = assert_oracle(&graph, shape);
            classification.throughput(Throughput::Elements(node_count as u64));
            classification.bench_with_input(
                BenchmarkId::new(shape.name(), node_count),
                &graph,
                |benchmark, graph| {
                    benchmark.iter(|| {
                        black_box(classify_core_periphery(
                            black_box(graph),
                            black_box(&analysis),
                        ))
                    });
                },
            );
        }
    }
    classification.finish();
}

criterion_group!(benches, benchmark_dsm);
criterion_main!(benches);
