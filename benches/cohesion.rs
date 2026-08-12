use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::{
    attack_simulation, cut_vertices_and_bridges, degree_assortativity, global_min_cut,
    k_core_decomposition, k_truss_decomposition, two_edge_connected_components, GraphProjection,
    NodeId, ProjectionKind,
};

const CLIQUE_SIZE: usize = 8;

fn projection(
    node_count: usize,
    edges: impl IntoIterator<Item = (usize, usize, f64)>,
) -> GraphProjection {
    GraphProjection::from_edges(
        ProjectionKind::External,
        (0..node_count as u32).map(NodeId::new),
        edges.into_iter().map(|(source, target, weight)| {
            (
                NodeId::new(source as u32),
                NodeId::new(target as u32),
                weight,
            )
        }),
    )
}

fn ring_lattice(node_count: usize) -> GraphProjection {
    projection(
        node_count,
        (0..node_count).flat_map(|node| {
            [1usize, 2]
                .into_iter()
                .map(move |distance| (node, (node + distance) % node_count, 1.0))
        }),
    )
}

fn bridged_cliques(node_count: usize) -> GraphProjection {
    assert_eq!(node_count % CLIQUE_SIZE, 0);
    let groups = node_count / CLIQUE_SIZE;
    let mut edges = Vec::new();
    for group in 0..groups {
        let start = group * CLIQUE_SIZE;
        for left in start..start + CLIQUE_SIZE {
            for right in left + 1..start + CLIQUE_SIZE {
                edges.push((left, right, 1.0));
            }
        }
        if group + 1 < groups {
            edges.push((start + CLIQUE_SIZE - 1, start + CLIQUE_SIZE, 1.0));
        }
    }
    projection(node_count, edges)
}

fn weighted_seam(node_count: usize) -> GraphProjection {
    assert_eq!(node_count % 2, 0);
    let half = node_count / 2;
    let mut edges = Vec::new();
    for start in [0, half] {
        for left in start..start + half {
            for right in left + 1..start + half {
                edges.push((left, right, 5.0));
            }
        }
    }
    edges.push((half - 1, half, 1.0));
    projection(node_count, edges)
}

fn star(node_count: usize) -> GraphProjection {
    projection(node_count, (1..node_count).map(|leaf| (0, leaf, 1.0)))
}

fn assert_core_oracle(graph: &GraphProjection) {
    let first = k_core_decomposition(graph).expect("benchmark projection is valid");
    let second = k_core_decomposition(graph).expect("repeat benchmark projection");
    assert_eq!(first, second, "repeatability oracle");
    assert_eq!(first.nodes, graph.nodes, "alignment oracle");
    assert_eq!(first.coreness, vec![4; graph.node_count()]);
    assert_eq!(first.max_core, 4);
}

fn assert_truss_oracle(graph: &GraphProjection) {
    let first = k_truss_decomposition(graph).expect("benchmark edge count is bounded");
    let second = k_truss_decomposition(graph).expect("repeat benchmark edge count");
    assert_eq!(first, second, "repeatability oracle");
    assert_eq!(first.max_truss, CLIQUE_SIZE as u32);
    let bridge_count = graph.node_count() / CLIQUE_SIZE - 1;
    assert_eq!(
        first
            .edges
            .iter()
            .filter(|edge| edge.trussness == 2)
            .count(),
        bridge_count,
        "bridge trussness oracle"
    );
    assert!(first
        .edges
        .iter()
        .filter(|edge| edge.trussness != 2)
        .all(|edge| edge.trussness == CLIQUE_SIZE as u32));
}

fn assert_star_truss_oracle(graph: &GraphProjection) {
    let first = k_truss_decomposition(graph).expect("benchmark edge count is bounded");
    let second = k_truss_decomposition(graph).expect("repeat benchmark edge count");
    assert_eq!(first, second, "repeatability oracle");
    assert_eq!(first.edges.len(), graph.node_count() - 1);
    assert_eq!(first.max_truss, 2);
    assert!(first.edges.iter().all(|edge| edge.trussness == 2));
}

fn assert_connectivity_oracle(graph: &GraphProjection) {
    let cuts = cut_vertices_and_bridges(graph).expect("benchmark projection is valid");
    let components = two_edge_connected_components(graph).expect("benchmark projection is valid");
    let group_count = graph.node_count() / CLIQUE_SIZE;
    assert_eq!(cuts.bridges.len(), group_count - 1, "bridge-count oracle");
    assert_eq!(
        cuts.articulation_points.len(),
        2 * (group_count - 1),
        "articulation-count oracle"
    );
    assert_eq!(components.components.len(), group_count);
    assert!(components
        .components
        .iter()
        .all(|component| component.len() == CLIQUE_SIZE));
}

fn assert_min_cut_oracle(graph: &GraphProjection) {
    let first = global_min_cut(graph)
        .expect("finite benchmark weights")
        .expect("at least two benchmark nodes");
    let second = global_min_cut(graph)
        .expect("repeat benchmark weights")
        .expect("repeat benchmark cut");
    assert_eq!(first, second, "repeatability oracle");
    assert_eq!(first.weight, 1.0, "weighted-seam oracle");
    assert_eq!(first.partition.len(), graph.node_count() / 2);
}

fn assert_attack_oracle(graph: &GraphProjection, order: &[NodeId], steps: usize) {
    let first = attack_simulation(graph, order, steps).expect("valid benchmark order");
    let second = attack_simulation(graph, order, steps).expect("repeat benchmark order");
    assert_eq!(first, second, "repeatability oracle");
    assert_eq!(first.trace.len(), steps);
    assert_eq!(first.trace[0].largest_component, 1, "center-removal oracle");
    assert!(first.trace.iter().all(|step| step.largest_component == 1));
    assert!((degree_assortativity(graph).expect("valid star") + 1.0).abs() <= 1.0e-12);
}

fn benchmark_peeling(c: &mut Criterion) {
    let mut group = c.benchmark_group("cohesion_peeling");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
    for node_count in [1_024usize, 8_192] {
        let graph = ring_lattice(node_count);
        assert_core_oracle(&graph);
        group.throughput(Throughput::Elements(node_count as u64));
        group.bench_with_input(
            BenchmarkId::new("k_core_ring_lattice", node_count),
            &graph,
            |bencher, graph| {
                bencher.iter(|| {
                    black_box(
                        k_core_decomposition(black_box(graph))
                            .expect("timed projection remains valid"),
                    )
                });
            },
        );
    }
    for node_count in [512usize, 2_048] {
        let graph = bridged_cliques(node_count);
        assert_truss_oracle(&graph);
        group.throughput(Throughput::Elements(graph.edge_count() as u64));
        group.bench_with_input(
            BenchmarkId::new("k_truss_bridged_cliques", node_count),
            &graph,
            |bencher, graph| {
                bencher.iter(|| {
                    black_box(
                        k_truss_decomposition(black_box(graph))
                            .expect("timed edge count remains bounded"),
                    )
                });
            },
        );
    }
    for node_count in [1_024usize, 8_192] {
        let graph = star(node_count);
        assert_star_truss_oracle(&graph);
        group.throughput(Throughput::Elements(graph.edge_count() as u64));
        group.bench_with_input(
            BenchmarkId::new("k_truss_high_degree_star", node_count),
            &graph,
            |bencher, graph| {
                bencher.iter(|| {
                    black_box(
                        k_truss_decomposition(black_box(graph))
                            .expect("timed edge count remains bounded"),
                    )
                });
            },
        );
    }
    group.finish();
}

fn benchmark_connectivity(c: &mut Criterion) {
    let mut group = c.benchmark_group("cohesion_connectivity");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
    for node_count in [512usize, 2_048] {
        let graph = bridged_cliques(node_count);
        assert_connectivity_oracle(&graph);
        group.throughput(Throughput::Elements(graph.edge_count() as u64));
        group.bench_with_input(
            BenchmarkId::new("cut_vertices_and_bridges", node_count),
            &graph,
            |bencher, graph| {
                bencher.iter(|| {
                    black_box(
                        cut_vertices_and_bridges(black_box(graph))
                            .expect("timed projection remains valid"),
                    )
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("two_edge_components", node_count),
            &graph,
            |bencher, graph| {
                bencher.iter(|| {
                    black_box(
                        two_edge_connected_components(black_box(graph))
                            .expect("timed projection remains valid"),
                    )
                });
            },
        );
    }
    group.finish();
}

fn benchmark_weighted_and_attack(c: &mut Criterion) {
    let mut group = c.benchmark_group("cohesion_weighted_and_attack");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
    for node_count in [64usize, 128] {
        let graph = weighted_seam(node_count);
        assert_min_cut_oracle(&graph);
        group.throughput(Throughput::Elements(node_count as u64));
        group.bench_with_input(
            BenchmarkId::new("global_min_cut", node_count),
            &graph,
            |bencher, graph| {
                bencher.iter(|| {
                    black_box(global_min_cut(black_box(graph)).expect("timed weights remain valid"))
                });
            },
        );
    }
    for node_count in [1_024usize, 8_192] {
        let graph = star(node_count);
        let order: Vec<_> = (0..node_count as u32).map(NodeId::new).collect();
        let steps = 64;
        assert_attack_oracle(&graph, &order, steps);
        group.throughput(Throughput::Elements(node_count as u64));
        group.bench_with_input(
            BenchmarkId::new("center_first_attack", node_count),
            &(graph.clone(), order),
            |bencher, (graph, order)| {
                bencher.iter(|| {
                    black_box(
                        attack_simulation(black_box(graph), black_box(order), black_box(steps))
                            .expect("timed order remains valid"),
                    )
                });
            },
        );
        group.bench_with_input(
            BenchmarkId::new("degree_assortativity", node_count),
            &graph,
            |bencher, graph| {
                bencher.iter(|| {
                    black_box(
                        degree_assortativity(black_box(graph))
                            .expect("timed projection remains valid"),
                    )
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    benches,
    benchmark_peeling,
    benchmark_connectivity,
    benchmark_weighted_and_attack
);
criterion_main!(benches);
