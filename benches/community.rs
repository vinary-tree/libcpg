use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use libcpg::{
    louvain, try_refine_connected, CommunityAssignment, CommunityKind, GraphProjection,
    LouvainOptions, NodeId, ProjectionKind,
};

const GROUP_SIZE: usize = 8;

fn ring_of_cliques(node_count: usize) -> GraphProjection {
    assert_eq!(node_count % GROUP_SIZE, 0);
    let group_count = node_count / GROUP_SIZE;
    let mut edges = Vec::with_capacity(group_count * (GROUP_SIZE * (GROUP_SIZE - 1) / 2 + 1));
    for group in 0..group_count {
        let start = group * GROUP_SIZE;
        for left in start..start + GROUP_SIZE {
            for right in left + 1..start + GROUP_SIZE {
                edges.push((left, right, 1.0));
            }
        }
        let next = ((group + 1) % group_count) * GROUP_SIZE;
        edges.push((start + GROUP_SIZE - 1, next, 0.05));
    }
    external_projection(node_count, edges)
}

fn disconnected_paths(node_count: usize) -> GraphProjection {
    assert_eq!(node_count % GROUP_SIZE, 0);
    let mut edges = Vec::with_capacity(node_count - node_count / GROUP_SIZE);
    for start in (0..node_count).step_by(GROUP_SIZE) {
        for source in start..start + GROUP_SIZE - 1 {
            edges.push((source, source + 1, 1.0));
        }
    }
    external_projection(node_count, edges)
}

fn external_projection(
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

fn independent_modularity(graph: &GraphProjection, assignment: &[u32], resolution: f64) -> f64 {
    let undirected = graph.undirected();
    let count = assignment
        .iter()
        .copied()
        .max()
        .map_or(0, |maximum| maximum as usize + 1);
    let mut internal = vec![0.0; count];
    let mut degree = vec![0.0; count];
    let mut total = 0.0;
    for source in 0..undirected.node_count() {
        let neighbors = undirected.neighbors(source as u32);
        let weights = undirected.neighbor_weights(source as u32);
        for (offset, &target) in neighbors.iter().enumerate() {
            if target < source as u32 {
                continue;
            }
            let weight = weights.map_or(1.0, |weights| weights[offset]);
            if weight == 0.0 {
                continue;
            }
            total += weight;
            degree[assignment[source] as usize] += weight;
            degree[assignment[target as usize] as usize] += weight;
            if assignment[source] == assignment[target as usize] {
                internal[assignment[source] as usize] += weight;
            }
        }
    }
    if total == 0.0 {
        return 0.0;
    }
    (0..count)
        .map(|community| {
            internal[community] / total - resolution * (degree[community] / (2.0 * total)).powi(2)
        })
        .sum()
}

fn assert_canonical(assignment: &CommunityAssignment) {
    assert_eq!(assignment.nodes.len(), assignment.community.len());
    let mut first_member = vec![usize::MAX; assignment.num_communities];
    for (node, &community) in assignment.community.iter().enumerate() {
        assert!((community as usize) < assignment.num_communities);
        first_member[community as usize] = first_member[community as usize].min(node);
    }
    assert!(first_member.iter().all(|&member| member != usize::MAX));
    assert!(first_member.windows(2).all(|pair| pair[0] < pair[1]));
}

fn assert_connected(graph: &GraphProjection, assignment: &CommunityAssignment) {
    let undirected = graph.undirected();
    let mut groups = BTreeMap::<u32, Vec<usize>>::new();
    for (node, &community) in assignment.community.iter().enumerate() {
        groups.entry(community).or_default().push(node);
    }
    for group in groups.into_values() {
        let mut allowed = vec![false; graph.node_count()];
        for &node in &group {
            allowed[node] = true;
        }
        let mut reached = vec![false; graph.node_count()];
        let mut queue = VecDeque::from([group[0] as u32]);
        reached[group[0]] = true;
        while let Some(node) = queue.pop_front() {
            for &neighbor in undirected.neighbors(node) {
                let neighbor = neighbor as usize;
                if allowed[neighbor] && !reached[neighbor] {
                    reached[neighbor] = true;
                    queue.push_back(neighbor as u32);
                }
            }
        }
        assert!(group.iter().all(|&node| reached[node]));
    }
}

fn assert_louvain_oracle(graph: &GraphProjection) {
    let first = louvain(graph, LouvainOptions::default()).expect("finite benchmark projection");
    let second = louvain(graph, LouvainOptions::default()).expect("repeat benchmark projection");
    assert_eq!(first, second, "repeatability oracle");
    assert_eq!(first.nodes, graph.nodes, "alignment oracle");
    assert_canonical(&first);
    assert!(
        first.num_communities > 1,
        "ring-of-cliques separation oracle"
    );
    assert!(first.modularity > 0.0, "positive modularity oracle");
    assert!(
        (first.modularity - independent_modularity(graph, &first.community, 1.0)).abs() <= 1.0e-10,
        "independent modularity oracle"
    );
}

fn disconnected_assignment(graph: &GraphProjection) -> CommunityAssignment {
    CommunityAssignment {
        kind: CommunityKind::Louvain { resolution: 1.0 },
        nodes: graph.nodes.clone(),
        community: vec![0; graph.node_count()],
        num_communities: usize::from(graph.node_count() > 0),
        modularity: 0.0,
    }
}

fn assert_refinement_oracle(graph: &GraphProjection, input: &CommunityAssignment) {
    let refined = try_refine_connected(graph, input).expect("aligned benchmark assignment");
    assert_eq!(refined.nodes, graph.nodes, "alignment oracle");
    assert_eq!(
        refined.num_communities,
        graph.node_count() / GROUP_SIZE,
        "component-count oracle"
    );
    assert_canonical(&refined);
    assert_connected(graph, &refined);
    assert!(
        (refined.modularity - independent_modularity(graph, &refined.community, 1.0)).abs()
            <= 1.0e-10,
        "refinement modularity oracle"
    );
}

fn benchmark_louvain(c: &mut Criterion) {
    let mut group = c.benchmark_group("community_louvain");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(3));
    for node_count in [256usize, 1_024] {
        let graph = ring_of_cliques(node_count);
        assert_louvain_oracle(&graph);
        group.throughput(Throughput::Elements(node_count as u64));
        group.bench_with_input(
            BenchmarkId::new("ring_of_cliques", node_count),
            &graph,
            |b, graph| {
                b.iter(|| {
                    black_box(
                        louvain(black_box(graph), black_box(LouvainOptions::default()))
                            .expect("timed projection remains valid"),
                    )
                });
            },
        );
    }
    group.finish();
}

fn benchmark_refinement(c: &mut Criterion) {
    let mut group = c.benchmark_group("community_refinement");
    group.sample_size(10);
    group.warm_up_time(Duration::from_secs(1));
    group.measurement_time(Duration::from_secs(2));
    for node_count in [512usize, 2_048] {
        let graph = disconnected_paths(node_count);
        let assignment = disconnected_assignment(&graph);
        assert_refinement_oracle(&graph, &assignment);
        group.throughput(Throughput::Elements(node_count as u64));
        group.bench_with_input(
            BenchmarkId::new("disconnected_paths", node_count),
            &(graph, assignment),
            |b, (graph, assignment)| {
                b.iter(|| {
                    black_box(
                        try_refine_connected(black_box(graph), black_box(assignment))
                            .expect("timed assignment remains aligned"),
                    )
                });
            },
        );
    }
    group.finish();
}

criterion_group!(benches, benchmark_louvain, benchmark_refinement);
criterion_main!(benches);
