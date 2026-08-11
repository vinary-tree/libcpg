use std::collections::BTreeMap;
use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::{
    condensation_metrics, CondensationOptions, NodeId, SccComponent, SccCondensationEdge,
    SccDecomposition, SccProjection,
};

#[derive(Clone, Copy)]
enum DagShape {
    Path,
    EightChains,
}

impl DagShape {
    fn name(self) -> &'static str {
        match self {
            Self::Path => "path",
            Self::EightChains => "eight_chains",
        }
    }
}

fn decomposition(component_count: usize, shape: DagShape) -> SccDecomposition {
    const CHAIN_COUNT: usize = 8;

    let components: Vec<_> = (0..component_count)
        .map(|id| SccComponent {
            id,
            nodes: vec![NodeId::new(id as u32)],
            cyclic: false,
        })
        .collect();
    let component_by_node = components
        .iter()
        .map(|component| (component.nodes[0], component.id))
        .collect::<BTreeMap<_, _>>();
    let condensation_edges = match shape {
        DagShape::Path => (0..component_count.saturating_sub(1))
            .map(|source| SccCondensationEdge {
                source,
                target: source + 1,
            })
            .collect(),
        DagShape::EightChains => (CHAIN_COUNT..component_count)
            .map(|target| SccCondensationEdge {
                source: target - CHAIN_COUNT,
                target,
            })
            .collect(),
    };
    SccDecomposition {
        projection: SccProjection::CallGraph,
        components,
        component_by_node,
        condensation_edges,
    }
}

fn assert_oracle(decomposition: &SccDecomposition, shape: DagShape, include_cg: bool) {
    const CHAIN_COUNT: usize = 8;

    let options = CondensationOptions {
        coffman_graham_width: include_cg.then_some(CHAIN_COUNT as u32),
        ..CondensationOptions::default()
    };
    let metrics = condensation_metrics(decomposition, &options);
    let component_count = decomposition.components.len();
    let expected_height = match shape {
        DagShape::Path => component_count,
        DagShape::EightChains => component_count / CHAIN_COUNT,
    };
    let expected_width = match shape {
        DagShape::Path => 1,
        DagShape::EightChains => CHAIN_COUNT,
    };

    assert!(metrics.complete, "benchmark report completeness oracle");
    assert!(!metrics.capped, "benchmark cap oracle");
    assert_eq!(metrics.height as usize, expected_height, "height oracle");
    assert_eq!(metrics.width as usize, expected_width, "width oracle");
    assert_eq!(
        metrics.critical_path_weight as usize, expected_height,
        "critical-path weight oracle"
    );
    assert_eq!(
        metrics.critical_path.len(),
        expected_height,
        "critical-path length oracle"
    );
    match (include_cg, metrics.coffman_graham) {
        (false, None) => {}
        (true, Some(layering)) => {
            assert_eq!(
                layering.height() as usize,
                expected_height,
                "CG height oracle"
            );
            assert_eq!(layering.width() as usize, expected_width, "CG width oracle");
            assert!(layering
                .layers
                .iter()
                .all(|layer| layer.len() <= CHAIN_COUNT));
            assert!(decomposition
                .condensation_edges
                .iter()
                .all(|edge| { layering.layer_of[edge.source] > layering.layer_of[edge.target] }));
        }
        _ => panic!("benchmark optional-layering oracle"),
    }
}

fn benchmark_condensation(c: &mut Criterion) {
    const CHAIN_COUNT: usize = 8;

    for include_cg in [false, true] {
        let group_name = if include_cg {
            "condensation_metrics_with_cg"
        } else {
            "condensation_metrics"
        };
        let mut group = c.benchmark_group(group_name);
        group.sample_size(20);
        group.warm_up_time(Duration::from_secs(1));
        group.measurement_time(Duration::from_secs(3));

        for component_count in [1_024usize, 8_192] {
            for shape in [DagShape::Path, DagShape::EightChains] {
                let decomposition = decomposition(component_count, shape);
                assert_oracle(&decomposition, shape, include_cg);
                let options = CondensationOptions {
                    coffman_graham_width: include_cg.then_some(CHAIN_COUNT as u32),
                    ..CondensationOptions::default()
                };
                group.throughput(Throughput::Elements(component_count as u64));
                group.bench_with_input(
                    BenchmarkId::new(shape.name(), component_count),
                    &decomposition,
                    |benchmark, decomposition| {
                        benchmark.iter(|| {
                            black_box(condensation_metrics(
                                black_box(decomposition),
                                black_box(&options),
                            ))
                        });
                    },
                );
            }
        }
        group.finish();
    }
}

criterion_group!(benches, benchmark_condensation);
criterion_main!(benches);
