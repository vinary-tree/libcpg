//! Deterministic cohesion and structural-robustness analyses.
//!
//! Topological analyses use one simple undirected view: direction is forgotten,
//! reciprocal and parallel edges are deduplicated, and self-loops are ignored.
//! The weighted global minimum cut instead sums reciprocal weights and rejects
//! negative or non-finite input.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use rustc_hash::FxHashMap;

use crate::NodeId;

use super::{AnalysisError, GraphProjection};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Maximum number of simple undirected edges accepted by k-truss analysis.
pub const MAX_K_TRUSS_EDGES: usize = 200_000;

/// Maximum number of vertices accepted by the cubic Stoer-Wagner kernel.
pub const MAX_MIN_CUT_NODES: usize = 1_024;

/// Maximum number of sequential removals accepted by attack simulation.
pub const MAX_ATTACK_STEPS: usize = 10_000;

/// Node-aligned k-core decomposition.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct KCoreDecomposition {
    /// Stable identifiers in projection order.
    pub nodes: Vec<NodeId>,
    /// Core number aligned one-for-one with `nodes`.
    pub coreness: Vec<u32>,
    /// Largest core number present, or zero for an empty graph.
    pub max_core: u32,
}

/// One canonical simple edge and its trussness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TrussEdge {
    /// Smaller stable endpoint.
    pub source: NodeId,
    /// Larger stable endpoint.
    pub target: NodeId,
    /// Largest `k` for which this edge belongs to a `k`-truss.
    pub trussness: u32,
}

/// Edge-aligned k-truss decomposition.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct KTrussDecomposition {
    /// Canonical edges ordered lexicographically by endpoint.
    pub edges: Vec<TrussEdge>,
    /// Largest edge trussness present, or zero when there are no edges.
    pub max_truss: u32,
}

/// Articulation vertices and bridges of the simple undirected projection.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CutStructure {
    /// Stable articulation identifiers in ascending order.
    pub articulation_points: Vec<NodeId>,
    /// Canonical bridge endpoints in lexicographic order.
    pub bridges: Vec<(NodeId, NodeId)>,
}

/// Canonical two-edge-connected partition.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TwoEdgeConnectedComponents {
    /// Stable identifiers in projection order.
    pub nodes: Vec<NodeId>,
    /// Component identifier aligned one-for-one with `nodes`.
    pub component: Vec<u32>,
    /// Components ordered by their smallest member; members are ascending.
    pub components: Vec<Vec<NodeId>>,
}

/// A deterministic global minimum cut.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct GlobalMinCut {
    /// Total weight crossing the bipartition.
    pub weight: f64,
    /// Canonical side of the bipartition, sorted by stable identifier.
    ///
    /// The smaller side is selected. Equal-size sides are resolved
    /// lexicographically.
    pub partition: Vec<NodeId>,
}

/// One observation in a sequential node-removal simulation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AttackStep {
    /// One-based removal number.
    pub step: u32,
    /// Stable identifier removed at this step.
    pub removed: NodeId,
    /// Size of the largest remaining connected component.
    pub largest_component: usize,
    /// Number of vertices still present.
    pub remaining_nodes: usize,
}

/// Deterministic attack-simulation trace and normalized robustness area.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AttackResult {
    /// Removal observations in caller-supplied order.
    pub trace: Vec<AttackStep>,
    /// Trapezoidal area under `largest_component / original_nodes` against
    /// normalized processed steps. Lower values indicate greater fragility.
    pub auc: f64,
    /// Whether `max_steps` intentionally left a suffix of `order` unprocessed.
    pub truncated: bool,
}

#[derive(Debug, Clone)]
struct SimpleUndirected {
    adjacency: Vec<Vec<(u32, usize)>>,
    edges: Vec<(u32, u32)>,
}

/// Computes core numbers with the Batagelj-Zaversnik bin-sort algorithm.
///
/// The operation is `O(n + m)` over the simple undirected projection. Results
/// are aligned with the projection's sorted stable identifiers.
pub fn k_core_decomposition(graph: &GraphProjection) -> Result<KCoreDecomposition, AnalysisError> {
    let simple = simple_undirected(graph, "k_core_decomposition")?;
    let mut degree: Vec<usize> = simple.adjacency.iter().map(Vec::len).collect();
    let (mut bins, mut positions, mut vertices) = bin_sort(&degree);

    for index in 0..vertices.len() {
        let vertex = vertices[index];
        for &(neighbor, _) in &simple.adjacency[vertex] {
            let neighbor = neighbor as usize;
            if degree[neighbor] > degree[vertex] {
                decrease_bin_value(
                    neighbor,
                    &mut degree,
                    &mut bins,
                    &mut positions,
                    &mut vertices,
                );
            }
        }
    }

    let coreness: Vec<u32> = degree
        .into_iter()
        .map(|value| u32::try_from(value).expect("simple degree fits the u32 node domain"))
        .collect();
    let max_core = coreness.iter().copied().max().unwrap_or(0);
    Ok(KCoreDecomposition {
        nodes: graph.nodes.clone(),
        coreness,
        max_core,
    })
}

/// Computes canonical edge trussness with support-bin peeling.
///
/// Triangle support is discovered from the smaller endpoint neighborhood when
/// an edge is peeled, avoiding an unbounded materialized triangle list. The
/// simple-edge cap bounds memory, while the classical degree bound makes the
/// neighborhood work `O(m^(3/2))` in a simple graph.
pub fn k_truss_decomposition(
    graph: &GraphProjection,
) -> Result<KTrussDecomposition, AnalysisError> {
    let simple = simple_undirected(graph, "k_truss_decomposition")?;
    if simple.edges.len() > MAX_K_TRUSS_EDGES {
        return Err(AnalysisError::BudgetExceeded {
            analysis: "k_truss_decomposition",
            unit: "undirected edges",
            needed: simple.edges.len() as u64,
            cap: MAX_K_TRUSS_EDGES as u64,
        });
    }
    if simple.edges.is_empty() {
        return Ok(KTrussDecomposition {
            edges: Vec::new(),
            max_truss: 0,
        });
    }

    let edge_by_pair: FxHashMap<(u32, u32), usize> = simple
        .edges
        .iter()
        .copied()
        .enumerate()
        .map(|(edge, pair)| (pair, edge))
        .collect();
    let mut support = Vec::with_capacity(simple.edges.len());
    for &(left, right) in &simple.edges {
        support.push(common_neighbor_count(
            left,
            right,
            &simple.adjacency,
            &edge_by_pair,
        ));
    }

    let (mut bins, mut positions, mut edges_by_support) = bin_sort(&support);
    let mut removed = vec![false; simple.edges.len()];
    let mut trussness = vec![0u32; simple.edges.len()];

    for index in 0..edges_by_support.len() {
        let edge = edges_by_support[index];
        removed[edge] = true;
        let current_support = support[edge];
        trussness[edge] =
            u32::try_from(current_support + 2).expect("triangle support fits the u32 node domain");
        let (left, right) = simple.edges[edge];
        let (scan, other) =
            if simple.adjacency[left as usize].len() <= simple.adjacency[right as usize].len() {
                (left, right)
            } else {
                (right, left)
            };

        for &(third, adjacent_edge) in &simple.adjacency[scan as usize] {
            if third == other || removed[adjacent_edge] {
                continue;
            }
            let other_pair = canonical_pair(other, third);
            let Some(&other_edge) = edge_by_pair.get(&other_pair) else {
                continue;
            };
            if removed[other_edge] {
                continue;
            }
            for candidate in [adjacent_edge, other_edge] {
                if support[candidate] > current_support {
                    decrease_bin_value(
                        candidate,
                        &mut support,
                        &mut bins,
                        &mut positions,
                        &mut edges_by_support,
                    );
                }
            }
        }
    }

    let edges: Vec<TrussEdge> = simple
        .edges
        .iter()
        .copied()
        .enumerate()
        .map(|(edge, (source, target))| TrussEdge {
            source: graph.nodes[source as usize],
            target: graph.nodes[target as usize],
            trussness: trussness[edge],
        })
        .collect();
    let max_truss = trussness.into_iter().max().unwrap_or(0);
    Ok(KTrussDecomposition { edges, max_truss })
}

/// Finds articulation vertices and bridges with iterative low-link DFS.
pub fn cut_vertices_and_bridges(graph: &GraphProjection) -> Result<CutStructure, AnalysisError> {
    let simple = simple_undirected(graph, "cut_vertices_and_bridges")?;
    let dense = dense_cut_structure(&simple);
    Ok(CutStructure {
        articulation_points: dense
            .articulation_points
            .into_iter()
            .map(|node| graph.nodes[node as usize])
            .collect(),
        bridges: dense
            .bridges
            .into_iter()
            .map(|(left, right)| (graph.nodes[left as usize], graph.nodes[right as usize]))
            .collect(),
    })
}

/// Partitions vertices after deleting every bridge.
///
/// Component identifiers are contiguous and ordered by the smallest stable
/// member. Isolated vertices are represented by singleton components.
pub fn two_edge_connected_components(
    graph: &GraphProjection,
) -> Result<TwoEdgeConnectedComponents, AnalysisError> {
    let simple = simple_undirected(graph, "two_edge_connected_components")?;
    let bridges: BTreeSet<(u32, u32)> = dense_cut_structure(&simple).bridges.into_iter().collect();
    let mut component = vec![u32::MAX; graph.node_count()];
    let mut components = Vec::new();
    let mut queue = VecDeque::new();

    for start in 0..graph.node_count() {
        if component[start] != u32::MAX {
            continue;
        }
        let id = u32::try_from(components.len())
            .expect("component count fits the projection's u32 node domain");
        component[start] = id;
        queue.push_back(start as u32);
        let mut members = Vec::new();
        while let Some(vertex) = queue.pop_front() {
            members.push(graph.nodes[vertex as usize]);
            for &(neighbor, _) in &simple.adjacency[vertex as usize] {
                if bridges.contains(&canonical_pair(vertex, neighbor)) {
                    continue;
                }
                if component[neighbor as usize] == u32::MAX {
                    component[neighbor as usize] = id;
                    queue.push_back(neighbor);
                }
            }
        }
        members.sort_unstable();
        components.push(members);
    }

    Ok(TwoEdgeConnectedComponents {
        nodes: graph.nodes.clone(),
        component,
        components,
    })
}

/// Computes the Stoer-Wagner global minimum cut of a weighted projection.
///
/// Direction is forgotten and reciprocal weights are deterministically summed.
/// Self-loops do not cross a cut and are ignored. Weights must be finite and
/// nonnegative, and every intermediate sum must remain finite.
pub fn global_min_cut(graph: &GraphProjection) -> Result<Option<GlobalMinCut>, AnalysisError> {
    require_well_formed(graph, "global_min_cut")?;
    let node_count = graph.node_count();
    if node_count > MAX_MIN_CUT_NODES {
        return Err(AnalysisError::BudgetExceeded {
            analysis: "global_min_cut",
            unit: "nodes",
            needed: node_count as u64,
            cap: MAX_MIN_CUT_NODES as u64,
        });
    }
    if node_count < 2 {
        return Ok(None);
    }

    let mut grouped = BTreeMap::<(usize, usize), Vec<f64>>::new();
    for (source, target, weight) in graph.edges() {
        if !weight.is_finite() || weight < 0.0 {
            return Err(AnalysisError::InvalidInput {
                analysis: "global_min_cut",
                input: "edge weight",
                reason: "must be finite and nonnegative",
            });
        }
        if source == target {
            continue;
        }
        let (left, right) = canonical_pair(source, target);
        grouped
            .entry((left as usize, right as usize))
            .or_default()
            .push(weight);
    }

    let mut weights = vec![vec![0.0; node_count]; node_count];
    for ((left, right), values) in grouped {
        let weight = checked_stable_sum(values, "global_min_cut")?;
        weights[left][right] = weight;
        weights[right][left] = weight;
    }

    let mut members: Vec<Vec<usize>> = (0..node_count).map(|node| vec![node]).collect();
    let mut active: Vec<usize> = (0..node_count).collect();
    let mut best_weight: f64 = f64::INFINITY;
    let mut best_partition = Vec::new();

    while active.len() > 1 {
        let mut added = vec![false; node_count];
        let mut connection: Vec<f64> = vec![0.0; node_count];
        let mut previous = active[0];
        let mut last = active[0];

        for phase_index in 0..active.len() {
            let selected = active
                .iter()
                .copied()
                .filter(|&vertex| !added[vertex])
                .max_by(|&left, &right| {
                    connection[left]
                        .total_cmp(&connection[right])
                        .then_with(|| right.cmp(&left))
                })
                .expect("a phase always has an unselected active vertex");
            added[selected] = true;
            if phase_index + 1 == active.len() {
                last = selected;
                let partition = canonical_partition(&members[selected], node_count);
                let cut_weight = if connection[selected] == 0.0 {
                    0.0
                } else {
                    connection[selected]
                };
                if cut_weight.total_cmp(&best_weight).is_lt()
                    || (cut_weight.total_cmp(&best_weight).is_eq()
                        && (best_partition.is_empty() || partition < best_partition))
                {
                    best_weight = cut_weight;
                    best_partition = partition;
                }
            } else {
                previous = selected;
                for &vertex in &active {
                    if !added[vertex] {
                        connection[vertex] = checked_add(
                            connection[vertex],
                            weights[selected][vertex],
                            "global_min_cut",
                        )?;
                    }
                }
            }
        }

        let last_members = std::mem::take(&mut members[last]);
        members[previous].extend(last_members);
        members[previous].sort_unstable();
        for &vertex in &active {
            if vertex != previous && vertex != last {
                let merged = checked_add(
                    weights[previous][vertex],
                    weights[last][vertex],
                    "global_min_cut",
                )?;
                weights[previous][vertex] = merged;
                weights[vertex][previous] = merged;
            }
        }
        active.retain(|&vertex| vertex != last);
    }

    Ok(Some(GlobalMinCut {
        weight: best_weight,
        partition: best_partition
            .into_iter()
            .map(|node| graph.nodes[node])
            .collect(),
    }))
}

/// Computes Newman's undirected degree assortativity coefficient.
///
/// A zero value is returned when the graph has no edges or endpoint-degree
/// variance is zero. Otherwise the result is clamped to `[-1, 1]` to absorb
/// roundoff at the mathematical boundary.
pub fn degree_assortativity(graph: &GraphProjection) -> Result<f64, AnalysisError> {
    let simple = simple_undirected(graph, "degree_assortativity")?;
    if simple.edges.is_empty() {
        return Ok(0.0);
    }
    let edge_count = simple.edges.len() as f64;
    let mut product_sum = 0.0;
    let mut mean_sum = 0.0;
    let mut square_sum = 0.0;
    for &(left, right) in &simple.edges {
        let left_degree = simple.adjacency[left as usize].len() as f64;
        let right_degree = simple.adjacency[right as usize].len() as f64;
        product_sum += left_degree * right_degree;
        mean_sum += 0.5 * (left_degree + right_degree);
        square_sum += 0.5 * (left_degree * left_degree + right_degree * right_degree);
    }
    let mean = mean_sum / edge_count;
    let numerator = product_sum / edge_count - mean * mean;
    let denominator = square_sum / edge_count - mean * mean;
    if denominator.abs() <= f64::EPSILON {
        Ok(0.0)
    } else {
        Ok((numerator / denominator).clamp(-1.0, 1.0))
    }
}

/// Simulates sequential removal in caller-supplied order.
///
/// `max_steps` is an intentional prefix bound and may not exceed
/// [`MAX_ATTACK_STEPS`]. Every identifier in `order` must be a distinct member
/// of the projection, including identifiers after the processed prefix.
pub fn attack_simulation(
    graph: &GraphProjection,
    order: &[NodeId],
    max_steps: usize,
) -> Result<AttackResult, AnalysisError> {
    let simple = simple_undirected(graph, "attack_simulation")?;
    if max_steps > MAX_ATTACK_STEPS {
        return Err(AnalysisError::BudgetExceeded {
            analysis: "attack_simulation",
            unit: "steps",
            needed: max_steps as u64,
            cap: MAX_ATTACK_STEPS as u64,
        });
    }
    let mut dense_order = Vec::with_capacity(order.len());
    let mut seen = BTreeSet::new();
    for &node in order {
        let dense = graph
            .dense_index(node)
            .ok_or(AnalysisError::UnknownNode(node))?;
        if !seen.insert(dense) {
            return Err(AnalysisError::InvalidInput {
                analysis: "attack_simulation",
                input: "removal order",
                reason: "must not contain duplicate nodes",
            });
        }
        dense_order.push(dense);
    }

    let steps = dense_order.len().min(max_steps);
    let truncated = steps < dense_order.len();
    if steps == 0 {
        return Ok(AttackResult {
            trace: Vec::new(),
            auc: 0.0,
            truncated,
        });
    }

    let mut alive = vec![true; graph.node_count()];
    let initial_largest = largest_component_size(&simple.adjacency, &alive);
    let mut trace = Vec::with_capacity(steps);
    for (index, &removed) in dense_order.iter().take(steps).enumerate() {
        alive[removed as usize] = false;
        trace.push(AttackStep {
            step: u32::try_from(index + 1).expect("attack step is capped below u32::MAX"),
            removed: graph.nodes[removed as usize],
            largest_component: largest_component_size(&simple.adjacency, &alive),
            remaining_nodes: graph.node_count() - index - 1,
        });
    }

    let denominator = graph.node_count().max(1) as f64;
    let width = 1.0 / steps as f64;
    let mut previous = initial_largest as f64 / denominator;
    let mut auc = 0.0;
    for step in &trace {
        let current = step.largest_component as f64 / denominator;
        auc += 0.5 * (previous + current) * width;
        previous = current;
    }
    Ok(AttackResult {
        trace,
        auc,
        truncated,
    })
}

fn simple_undirected(
    graph: &GraphProjection,
    analysis: &'static str,
) -> Result<SimpleUndirected, AnalysisError> {
    require_well_formed(graph, analysis)?;
    let mut edges = Vec::with_capacity(graph.edge_count());
    for source in 0..graph.node_count() {
        let start = graph.offsets[source] as usize;
        let end = graph.offsets[source + 1] as usize;
        for &target in &graph.targets[start..end] {
            if target as usize != source {
                edges.push(canonical_pair(source as u32, target));
            }
        }
    }
    edges.sort_unstable();
    edges.dedup();

    let mut adjacency = vec![Vec::new(); graph.node_count()];
    for (edge, &(left, right)) in edges.iter().enumerate() {
        adjacency[left as usize].push((right, edge));
        adjacency[right as usize].push((left, edge));
    }
    for neighbors in &mut adjacency {
        neighbors.sort_unstable_by_key(|&(neighbor, _)| neighbor);
    }
    Ok(SimpleUndirected { adjacency, edges })
}

fn require_well_formed(
    graph: &GraphProjection,
    analysis: &'static str,
) -> Result<(), AnalysisError> {
    if graph.is_well_formed() {
        Ok(())
    } else {
        Err(AnalysisError::InvalidInput {
            analysis,
            input: "projection",
            reason: "must satisfy sorted-node and forward/reverse CSR invariants",
        })
    }
}

fn canonical_pair(left: u32, right: u32) -> (u32, u32) {
    if left <= right {
        (left, right)
    } else {
        (right, left)
    }
}

fn bin_sort(values: &[usize]) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
    if values.is_empty() {
        return (vec![0], Vec::new(), Vec::new());
    }
    let maximum = values.iter().copied().max().unwrap_or(0);
    let mut bins = vec![0usize; maximum + 1];
    for &value in values {
        bins[value] += 1;
    }
    let mut start = 0;
    for count in &mut bins {
        let size = *count;
        *count = start;
        start += size;
    }
    let mut positions = vec![0usize; values.len()];
    let mut elements = vec![0usize; values.len()];
    for (element, &value) in values.iter().enumerate() {
        positions[element] = bins[value];
        elements[positions[element]] = element;
        bins[value] += 1;
    }
    for value in (1..bins.len()).rev() {
        bins[value] = bins[value - 1];
    }
    bins[0] = 0;
    (bins, positions, elements)
}

fn decrease_bin_value(
    element: usize,
    values: &mut [usize],
    bins: &mut [usize],
    positions: &mut [usize],
    elements: &mut [usize],
) {
    let value = values[element];
    let position = positions[element];
    let first = bins[value];
    let displaced = elements[first];
    if element != displaced {
        elements.swap(position, first);
        positions[element] = first;
        positions[displaced] = position;
    }
    bins[value] += 1;
    values[element] -= 1;
}

fn common_neighbor_count(
    left: u32,
    right: u32,
    adjacency: &[Vec<(u32, usize)>],
    edge_by_pair: &FxHashMap<(u32, u32), usize>,
) -> usize {
    let (scan, other) = if adjacency[left as usize].len() <= adjacency[right as usize].len() {
        (left, right)
    } else {
        (right, left)
    };
    adjacency[scan as usize]
        .iter()
        .filter(|&&(third, _)| edge_by_pair.contains_key(&canonical_pair(other, third)))
        .count()
}

#[derive(Debug)]
struct DenseCutStructure {
    articulation_points: Vec<u32>,
    bridges: Vec<(u32, u32)>,
}

fn dense_cut_structure(simple: &SimpleUndirected) -> DenseCutStructure {
    let node_count = simple.adjacency.len();
    let undiscovered = usize::MAX;
    let mut discovery = vec![undiscovered; node_count];
    let mut low = vec![0usize; node_count];
    let mut parent = vec![undiscovered; node_count];
    let mut children = vec![0usize; node_count];
    let mut articulation = vec![false; node_count];
    let mut bridges = Vec::new();
    let mut clock = 0usize;

    for root in 0..node_count {
        if discovery[root] != undiscovered {
            continue;
        }
        discovery[root] = clock;
        low[root] = clock;
        clock += 1;
        let mut stack = vec![(root, 0usize)];
        while let Some((vertex, next_neighbor)) = stack.last_mut() {
            if *next_neighbor < simple.adjacency[*vertex].len() {
                let neighbor = simple.adjacency[*vertex][*next_neighbor].0 as usize;
                *next_neighbor += 1;
                if discovery[neighbor] == undiscovered {
                    parent[neighbor] = *vertex;
                    children[*vertex] += 1;
                    discovery[neighbor] = clock;
                    low[neighbor] = clock;
                    clock += 1;
                    stack.push((neighbor, 0));
                } else if parent[*vertex] != neighbor {
                    low[*vertex] = low[*vertex].min(discovery[neighbor]);
                }
            } else {
                let (finished, _) = stack.pop().expect("finished DFS frame exists");
                if parent[finished] == undiscovered {
                    articulation[finished] = children[finished] > 1;
                } else {
                    let ancestor = parent[finished];
                    low[ancestor] = low[ancestor].min(low[finished]);
                    if parent[ancestor] != undiscovered && low[finished] >= discovery[ancestor] {
                        articulation[ancestor] = true;
                    }
                    if low[finished] > discovery[ancestor] {
                        bridges.push(canonical_pair(ancestor as u32, finished as u32));
                    }
                }
            }
        }
    }
    bridges.sort_unstable();
    DenseCutStructure {
        articulation_points: articulation
            .into_iter()
            .enumerate()
            .filter_map(|(node, is_cut)| is_cut.then_some(node as u32))
            .collect(),
        bridges,
    }
}

fn canonical_partition(side: &[usize], node_count: usize) -> Vec<usize> {
    let mut side = side.to_vec();
    side.sort_unstable();
    let membership: BTreeSet<usize> = side.iter().copied().collect();
    let complement: Vec<usize> = (0..node_count)
        .filter(|node| !membership.contains(node))
        .collect();
    if side.len() < complement.len() || (side.len() == complement.len() && side <= complement) {
        side
    } else {
        complement
    }
}

fn checked_stable_sum(mut values: Vec<f64>, analysis: &'static str) -> Result<f64, AnalysisError> {
    values.sort_by(f64::total_cmp);
    let mut sum = 0.0;
    let mut correction = 0.0;
    for value in values {
        let next = sum + value;
        if sum.abs() >= value.abs() {
            correction += (sum - next) + value;
        } else {
            correction += (value - next) + sum;
        }
        sum = next;
    }
    checked_add(sum, correction, analysis)
}

fn checked_add(left: f64, right: f64, analysis: &'static str) -> Result<f64, AnalysisError> {
    let result = left + right;
    if result.is_finite() {
        Ok(result)
    } else {
        Err(AnalysisError::InvalidInput {
            analysis,
            input: "edge weights",
            reason: "all accumulated weights must remain finite",
        })
    }
}

fn largest_component_size(adjacency: &[Vec<(u32, usize)>], alive: &[bool]) -> usize {
    let mut visited = vec![false; adjacency.len()];
    let mut queue = VecDeque::new();
    let mut largest = 0;
    for start in 0..adjacency.len() {
        if !alive[start] || visited[start] {
            continue;
        }
        visited[start] = true;
        queue.push_back(start as u32);
        let mut size = 0;
        while let Some(vertex) = queue.pop_front() {
            size += 1;
            for &(neighbor, _) in &adjacency[vertex as usize] {
                let neighbor = neighbor as usize;
                if alive[neighbor] && !visited[neighbor] {
                    visited[neighbor] = true;
                    queue.push_back(neighbor as u32);
                }
            }
        }
        largest = largest.max(size);
    }
    largest
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet, VecDeque};

    use proptest::prelude::*;

    use super::*;
    use crate::analysis::ProjectionKind;

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

    fn complete_graph(node_count: usize) -> GraphProjection {
        projection(
            node_count,
            (0..node_count)
                .flat_map(|left| (left + 1..node_count).map(move |right| (left, right, 1.0))),
        )
    }

    fn expected_edges(node_count: usize, raw: &[(u8, u8)]) -> Vec<(usize, usize)> {
        if node_count == 0 {
            return Vec::new();
        }
        raw.iter()
            .map(|&(left, right)| (left as usize % node_count, right as usize % node_count))
            .filter(|(left, right)| left != right)
            .map(|(left, right)| {
                if left < right {
                    (left, right)
                } else {
                    (right, left)
                }
            })
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    fn projection_from_raw(node_count: usize, raw: &[(u8, u8)]) -> GraphProjection {
        projection(
            node_count,
            raw.iter().map(|&(left, right)| {
                if node_count == 0 {
                    (0, 0, 1.0)
                } else {
                    (left as usize % node_count, right as usize % node_count, 1.0)
                }
            }),
        )
    }

    #[test]
    fn clique_and_path_have_exact_core_numbers() {
        let clique = complete_graph(6);
        let result = k_core_decomposition(&clique).expect("well-formed clique");
        assert_eq!(result.nodes, clique.nodes);
        assert_eq!(result.coreness, vec![5; 6]);
        assert_eq!(result.max_core, 5);

        let path = projection(5, (0..4).map(|node| (node, node + 1, 1.0)));
        let result = k_core_decomposition(&path).expect("well-formed path");
        assert_eq!(result.coreness, vec![1; 5]);
        assert_eq!(result.max_core, 1);

        let empty = projection(0, []);
        assert_eq!(
            k_core_decomposition(&empty).expect("empty projection"),
            KCoreDecomposition {
                nodes: Vec::new(),
                coreness: Vec::new(),
                max_core: 0,
            }
        );
    }

    #[test]
    fn k_truss_distinguishes_clique_triangle_and_bridge_edges() {
        let graph = projection(
            7,
            [
                (0, 1, 1.0),
                (0, 2, 1.0),
                (0, 3, 1.0),
                (1, 2, 1.0),
                (1, 3, 1.0),
                (2, 3, 1.0),
                (3, 4, 1.0),
                (4, 5, 1.0),
                (4, 6, 1.0),
                (5, 6, 1.0),
            ],
        );
        let result = k_truss_decomposition(&graph).expect("bounded graph");
        let trussness: BTreeMap<_, _> = result
            .edges
            .iter()
            .map(|edge| ((edge.source.as_u32(), edge.target.as_u32()), edge.trussness))
            .collect();
        for left in 0..4 {
            for right in left + 1..4 {
                assert_eq!(trussness[&(left, right)], 4);
            }
        }
        assert_eq!(trussness[&(3, 4)], 2);
        assert_eq!(trussness[&(4, 5)], 3);
        assert_eq!(trussness[&(4, 6)], 3);
        assert_eq!(trussness[&(5, 6)], 3);
        assert_eq!(result.max_truss, 4);
    }

    #[test]
    fn lowlink_and_bridge_deletion_find_two_triangle_blocks() {
        let graph = projection(
            7,
            [
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 0, 1.0),
                (2, 3, 1.0),
                (3, 4, 1.0),
                (4, 5, 1.0),
                (5, 3, 1.0),
            ],
        );
        let cut = cut_vertices_and_bridges(&graph).expect("well-formed graph");
        assert_eq!(
            cut.articulation_points,
            vec![NodeId::new(2), NodeId::new(3)]
        );
        assert_eq!(cut.bridges, vec![(NodeId::new(2), NodeId::new(3))]);

        let components =
            two_edge_connected_components(&graph).expect("well-formed graph partition");
        assert_eq!(components.nodes, graph.nodes);
        assert_eq!(components.component, vec![0, 0, 0, 1, 1, 1, 2]);
        assert_eq!(
            components.components,
            vec![
                vec![NodeId::new(0), NodeId::new(1), NodeId::new(2)],
                vec![NodeId::new(3), NodeId::new(4), NodeId::new(5)],
                vec![NodeId::new(6)],
            ]
        );
    }

    #[test]
    fn stoer_wagner_finds_weighted_seam_and_canonical_side() {
        let graph = projection(
            6,
            [
                (0, 1, 5.0),
                (1, 2, 5.0),
                (2, 0, 5.0),
                (2, 3, 0.4),
                (3, 2, 0.6),
                (3, 4, 5.0),
                (4, 5, 5.0),
                (5, 3, 5.0),
                (2, 2, 1000.0),
            ],
        );
        let cut = global_min_cut(&graph)
            .expect("finite nonnegative weights")
            .expect("six nodes have a cut");
        assert!((cut.weight - 1.0).abs() <= 1.0e-12);
        assert_eq!(
            cut.partition,
            vec![NodeId::new(0), NodeId::new(1), NodeId::new(2)]
        );

        assert_eq!(global_min_cut(&projection(0, [])).expect("empty"), None);
        assert_eq!(global_min_cut(&projection(1, [])).expect("singleton"), None);
    }

    #[test]
    fn star_is_disassortative_and_center_attack_fragments_immediately() {
        let star = projection(5, (1..5).map(|leaf| (0, leaf, 1.0)));
        let truss = k_truss_decomposition(&star).expect("small star is below the edge cap");
        assert_eq!(truss.max_truss, 2);
        assert!(truss.edges.iter().all(|edge| edge.trussness == 2));
        let assortativity = degree_assortativity(&star).expect("well-formed star");
        assert!((assortativity + 1.0).abs() <= 1.0e-12);

        let order = [NodeId::new(0), NodeId::new(1), NodeId::new(2)];
        let result = attack_simulation(&star, &order, 2).expect("distinct projected order");
        assert_eq!(
            result.trace,
            vec![
                AttackStep {
                    step: 1,
                    removed: NodeId::new(0),
                    largest_component: 1,
                    remaining_nodes: 4,
                },
                AttackStep {
                    step: 2,
                    removed: NodeId::new(1),
                    largest_component: 1,
                    remaining_nodes: 3,
                },
            ]
        );
        assert!((result.auc - 0.4).abs() <= 1.0e-12);
        assert!(result.truncated);
    }

    #[test]
    fn topology_is_simple_undirected_and_ignores_weights() {
        let graph = projection(
            3,
            [
                (0, 1, -7.0),
                (1, 0, f64::NAN),
                (0, 1, 9.0),
                (1, 2, f64::INFINITY),
                (2, 2, 1.0),
            ],
        );
        assert_eq!(
            k_core_decomposition(&graph)
                .expect("topology does not interpret weights")
                .coreness,
            vec![1, 1, 1]
        );
        assert_eq!(
            k_truss_decomposition(&graph)
                .expect("two simple edges")
                .edges
                .iter()
                .map(|edge| edge.trussness)
                .collect::<Vec<_>>(),
            vec![2, 2]
        );
        assert_eq!(
            cut_vertices_and_bridges(&graph)
                .expect("simple topology")
                .articulation_points,
            vec![NodeId::new(1)]
        );
    }

    #[test]
    fn malformed_capped_and_invalid_inputs_are_explicit_errors() {
        let mut malformed = projection(2, [(0, 1, 1.0)]);
        malformed.rev_targets.clear();
        for result in [
            k_core_decomposition(&malformed).map(|_| ()),
            k_truss_decomposition(&malformed).map(|_| ()),
            cut_vertices_and_bridges(&malformed).map(|_| ()),
            two_edge_connected_components(&malformed).map(|_| ()),
            degree_assortativity(&malformed).map(|_| ()),
            attack_simulation(&malformed, &[], 0).map(|_| ()),
        ] {
            assert!(matches!(result, Err(AnalysisError::InvalidInput { .. })));
        }
        assert!(matches!(
            global_min_cut(&malformed),
            Err(AnalysisError::InvalidInput { .. })
        ));

        assert!(matches!(
            global_min_cut(&projection(MAX_MIN_CUT_NODES + 1, [])),
            Err(AnalysisError::BudgetExceeded { .. })
        ));
        assert!(matches!(
            global_min_cut(&projection(2, [(0, 1, -1.0)])),
            Err(AnalysisError::InvalidInput { .. })
        ));
        assert!(matches!(
            global_min_cut(&projection(2, [(0, 1, f64::NAN)])),
            Err(AnalysisError::InvalidInput { .. })
        ));
        assert!(matches!(
            global_min_cut(&projection(2, [(0, 1, f64::MAX), (1, 0, f64::MAX)])),
            Err(AnalysisError::InvalidInput { .. })
        ));

        let graph = projection(2, [(0, 1, 1.0)]);
        assert!(matches!(
            attack_simulation(&graph, &[], MAX_ATTACK_STEPS + 1),
            Err(AnalysisError::BudgetExceeded { .. })
        ));
        assert_eq!(
            attack_simulation(&graph, &[NodeId::new(99)], 1),
            Err(AnalysisError::UnknownNode(NodeId::new(99)))
        );
        assert!(matches!(
            attack_simulation(&graph, &[NodeId::new(0), NodeId::new(0)], 1),
            Err(AnalysisError::InvalidInput { .. })
        ));
    }

    #[test]
    fn truss_edge_cap_is_enforced_before_triangle_work() {
        let node_count = 634usize;
        let mut edges = Vec::with_capacity(MAX_K_TRUSS_EDGES + 1);
        'outer: for left in 0..node_count {
            for right in left + 1..node_count {
                edges.push((left, right, 1.0));
                if edges.len() == MAX_K_TRUSS_EDGES + 1 {
                    break 'outer;
                }
            }
        }
        let graph = projection(node_count, edges);
        assert!(matches!(
            k_truss_decomposition(&graph),
            Err(AnalysisError::BudgetExceeded {
                analysis: "k_truss_decomposition",
                needed,
                cap,
                ..
            }) if needed == (MAX_K_TRUSS_EDGES + 1) as u64 && cap == MAX_K_TRUSS_EDGES as u64
        ));
    }

    #[test]
    fn iterative_lowlink_is_stack_safe_on_a_deep_path() {
        let node_count = 25_000usize;
        let graph = projection(
            node_count,
            (0..node_count - 1).map(|node| (node, node + 1, 1.0)),
        );
        let cut = cut_vertices_and_bridges(&graph).expect("deep well-formed path");
        assert_eq!(cut.articulation_points.len(), node_count - 2);
        assert_eq!(cut.bridges.len(), node_count - 1);
        assert_eq!(cut.articulation_points[0], NodeId::new(1));
        assert_eq!(
            cut.articulation_points.last(),
            Some(&NodeId::new((node_count - 2) as u32))
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn public_results_round_trip_with_byte_stable_json() {
        let graph = complete_graph(4);
        let first = k_truss_decomposition(&graph).expect("bounded clique");
        let second = k_truss_decomposition(&graph).expect("repeat bounded clique");
        let first_bytes = serde_json::to_vec(&first).expect("serialize truss result");
        let second_bytes = serde_json::to_vec(&second).expect("serialize repeat result");
        assert_eq!(first_bytes, second_bytes);
        assert_eq!(
            serde_json::from_slice::<KTrussDecomposition>(&first_bytes)
                .expect("deserialize truss result"),
            first
        );

        let cut = global_min_cut(&graph)
            .expect("valid weights")
            .expect("clique has a cut");
        let bytes = serde_json::to_vec(&cut).expect("serialize cut");
        assert_eq!(
            serde_json::from_slice::<GlobalMinCut>(&bytes).expect("deserialize cut"),
            cut
        );
    }

    fn naive_core_numbers(node_count: usize, edges: &[(usize, usize)]) -> Vec<u32> {
        let mut core = vec![0u32; node_count];
        for k in 0..=node_count {
            let mut alive = vec![true; node_count];
            loop {
                let remove: Vec<usize> = (0..node_count)
                    .filter(|&node| {
                        alive[node]
                            && edges
                                .iter()
                                .filter(|&&(left, right)| {
                                    (left == node && alive[right]) || (right == node && alive[left])
                                })
                                .count()
                                < k
                    })
                    .collect();
                if remove.is_empty() {
                    break;
                }
                for node in remove {
                    alive[node] = false;
                }
            }
            for node in 0..node_count {
                if alive[node] {
                    core[node] = k as u32;
                }
            }
        }
        core
    }

    fn naive_trussness(
        node_count: usize,
        edges: &[(usize, usize)],
    ) -> BTreeMap<(usize, usize), u32> {
        let edge_index: BTreeMap<_, _> = edges
            .iter()
            .copied()
            .enumerate()
            .map(|(index, edge)| (edge, index))
            .collect();
        let mut truss = vec![0u32; edges.len()];
        for k in 2..=node_count.max(2) {
            let mut alive = vec![true; edges.len()];
            loop {
                let remove: Vec<usize> = edges
                    .iter()
                    .copied()
                    .enumerate()
                    .filter_map(|(edge, (left, right))| {
                        if !alive[edge] {
                            return None;
                        }
                        let support = (0..node_count)
                            .filter(|&third| third != left && third != right)
                            .filter(|&third| {
                                let left_third = if left < third {
                                    (left, third)
                                } else {
                                    (third, left)
                                };
                                let right_third = if right < third {
                                    (right, third)
                                } else {
                                    (third, right)
                                };
                                edge_index
                                    .get(&left_third)
                                    .is_some_and(|&candidate| alive[candidate])
                                    && edge_index
                                        .get(&right_third)
                                        .is_some_and(|&candidate| alive[candidate])
                            })
                            .count();
                        (support < k - 2).then_some(edge)
                    })
                    .collect();
                if remove.is_empty() {
                    break;
                }
                for edge in remove {
                    alive[edge] = false;
                }
            }
            for edge in 0..edges.len() {
                if alive[edge] {
                    truss[edge] = k as u32;
                }
            }
        }
        edges
            .iter()
            .copied()
            .enumerate()
            .map(|(edge, pair)| (pair, truss[edge]))
            .collect()
    }

    fn component_count(
        node_count: usize,
        edges: &[(usize, usize)],
        removed_node: Option<usize>,
        removed_edge: Option<(usize, usize)>,
    ) -> usize {
        let mut adjacency = vec![Vec::new(); node_count];
        for &(left, right) in edges {
            if Some(left) == removed_node
                || Some(right) == removed_node
                || Some((left, right)) == removed_edge
            {
                continue;
            }
            adjacency[left].push(right);
            adjacency[right].push(left);
        }
        let mut visited = vec![false; node_count];
        let mut queue = VecDeque::new();
        let mut count = 0;
        for start in 0..node_count {
            if Some(start) == removed_node || visited[start] {
                continue;
            }
            count += 1;
            visited[start] = true;
            queue.push_back(start);
            while let Some(node) = queue.pop_front() {
                for &neighbor in &adjacency[node] {
                    if !visited[neighbor] {
                        visited[neighbor] = true;
                        queue.push_back(neighbor);
                    }
                }
            }
        }
        count
    }

    fn naive_cuts(
        node_count: usize,
        edges: &[(usize, usize)],
    ) -> (Vec<usize>, Vec<(usize, usize)>) {
        let baseline = component_count(node_count, edges, None, None);
        let articulation = (0..node_count)
            .filter(|&node| component_count(node_count, edges, Some(node), None) > baseline)
            .collect();
        let bridges = edges
            .iter()
            .copied()
            .filter(|&edge| component_count(node_count, edges, None, Some(edge)) > baseline)
            .collect();
        (articulation, bridges)
    }

    fn components_without_bridges(
        node_count: usize,
        edges: &[(usize, usize)],
        bridges: &[(usize, usize)],
    ) -> Vec<Vec<usize>> {
        let bridges: BTreeSet<_> = bridges.iter().copied().collect();
        let mut adjacency = vec![Vec::new(); node_count];
        for &(left, right) in edges {
            if !bridges.contains(&(left, right)) {
                adjacency[left].push(right);
                adjacency[right].push(left);
            }
        }
        let mut visited = vec![false; node_count];
        let mut queue = VecDeque::new();
        let mut components = Vec::new();
        for start in 0..node_count {
            if visited[start] {
                continue;
            }
            visited[start] = true;
            queue.push_back(start);
            let mut members = Vec::new();
            while let Some(node) = queue.pop_front() {
                members.push(node);
                for &neighbor in &adjacency[node] {
                    if !visited[neighbor] {
                        visited[neighbor] = true;
                        queue.push_back(neighbor);
                    }
                }
            }
            members.sort_unstable();
            components.push(members);
        }
        components
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(96))]

        #[test]
        fn k_core_matches_naive_peeling_through_twenty_vertices(
            node_count in 0usize..21,
            raw in prop::collection::vec((any::<u8>(), any::<u8>()), 0..80),
        ) {
            let graph = projection_from_raw(node_count, &raw);
            let expected = expected_edges(node_count, &raw);
            let core = k_core_decomposition(&graph).expect("generated projection is valid");
            prop_assert_eq!(&core.coreness, &naive_core_numbers(node_count, &expected));
            prop_assert_eq!(core.clone(), k_core_decomposition(&graph).expect("repeat core"));

            let reversed = projection_from_raw(node_count, &raw.iter().rev().copied().collect::<Vec<_>>());
            prop_assert_eq!(core, k_core_decomposition(&reversed).expect("permuted core"));
        }

        #[test]
        fn truss_and_connectivity_match_independent_peeling_and_removal_oracles(
            node_count in 0usize..13,
            raw in prop::collection::vec((any::<u8>(), any::<u8>()), 0..48),
        ) {
            let graph = projection_from_raw(node_count, &raw);
            let expected = expected_edges(node_count, &raw);

            let truss = k_truss_decomposition(&graph).expect("generated graph is below cap");
            let observed_truss: BTreeMap<_, _> = truss.edges.iter().map(|edge| {
                ((edge.source.as_u32() as usize, edge.target.as_u32() as usize), edge.trussness)
            }).collect();
            prop_assert_eq!(observed_truss, naive_trussness(node_count, &expected));
            prop_assert_eq!(truss.clone(), k_truss_decomposition(&graph).expect("repeat truss"));

            let (expected_articulation, expected_bridges) = naive_cuts(node_count, &expected);
            let cuts = cut_vertices_and_bridges(&graph).expect("generated projection is valid");
            prop_assert_eq!(cuts.articulation_points.iter().map(|node| node.as_u32() as usize).collect::<Vec<_>>(), expected_articulation);
            prop_assert_eq!(cuts.bridges.iter().map(|(left, right)| (left.as_u32() as usize, right.as_u32() as usize)).collect::<Vec<_>>(), expected_bridges.clone());

            let components = two_edge_connected_components(&graph).expect("generated projection is valid");
            let observed_components = components.components.iter().map(|component| component.iter().map(|node| node.as_u32() as usize).collect::<Vec<_>>()).collect::<Vec<_>>();
            prop_assert_eq!(observed_components, components_without_bridges(node_count, &expected, &expected_bridges));
            prop_assert_eq!(components.clone(), two_edge_connected_components(&graph).expect("repeat components"));

            let reversed = projection_from_raw(node_count, &raw.iter().rev().copied().collect::<Vec<_>>());
            prop_assert_eq!(truss, k_truss_decomposition(&reversed).expect("permuted truss"));
            prop_assert_eq!(cuts, cut_vertices_and_bridges(&reversed).expect("permuted cuts"));
            prop_assert_eq!(components, two_edge_connected_components(&reversed).expect("permuted components"));
        }
    }

    fn weighted_projection_from_raw(
        node_count: usize,
        raw: &[(u8, u8, u8)],
    ) -> (GraphProjection, BTreeMap<(usize, usize), f64>) {
        let mut expected = BTreeMap::<(usize, usize), f64>::new();
        let edges = raw
            .iter()
            .map(|&(left, right, weight)| {
                let left = left as usize % node_count;
                let right = right as usize % node_count;
                let weight = weight as f64;
                if left != right {
                    let pair = if left < right {
                        (left, right)
                    } else {
                        (right, left)
                    };
                    *expected.entry(pair).or_default() += weight;
                }
                (left, right, weight)
            })
            .collect::<Vec<_>>();
        (projection(node_count, edges), expected)
    }

    fn brute_min_cut(node_count: usize, weights: &BTreeMap<(usize, usize), f64>) -> f64 {
        let mut best = f64::INFINITY;
        for mask in 1usize..(1usize << node_count) - 1 {
            let mut weight = 0.0;
            for (&(left, right), &edge_weight) in weights {
                if ((mask >> left) & 1) != ((mask >> right) & 1) {
                    weight += edge_weight;
                }
            }
            if weight < best {
                best = weight;
            }
        }
        best
    }

    fn partition_weight(partition: &[NodeId], weights: &BTreeMap<(usize, usize), f64>) -> f64 {
        let side: BTreeSet<_> = partition
            .iter()
            .map(|node| node.as_u32() as usize)
            .collect();
        weights
            .iter()
            .filter_map(|(&(left, right), &weight)| {
                (side.contains(&left) != side.contains(&right)).then_some(weight)
            })
            .sum()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(96))]

        #[test]
        fn stoer_wagner_matches_brute_force_and_returns_a_real_canonical_cut(
            node_count in 2usize..11,
            raw in prop::collection::vec((any::<u8>(), any::<u8>(), 0u8..8), 0..48),
        ) {
            let (graph, weights) = weighted_projection_from_raw(node_count, &raw);
            let first = global_min_cut(&graph).expect("generated weights are finite").expect("at least two nodes");
            let second = global_min_cut(&graph).expect("repeat weights").expect("repeat cut");
            let expected = brute_min_cut(node_count, &weights);
            prop_assert!((first.weight - expected).abs() <= 1.0e-10);
            prop_assert!((partition_weight(&first.partition, &weights) - expected).abs() <= 1.0e-10);
            prop_assert!(!first.partition.is_empty());
            prop_assert!(first.partition.len() <= node_count / 2 + node_count % 2);
            prop_assert!(first.partition.windows(2).all(|pair| pair[0] < pair[1]));
            prop_assert_eq!(&first, &second);

            let reversed_raw = raw.iter().rev().copied().collect::<Vec<_>>();
            let (reversed, _) = weighted_projection_from_raw(node_count, &reversed_raw);
            prop_assert_eq!(first, global_min_cut(&reversed).expect("permuted weights").expect("permuted cut"));
        }
    }

    fn naive_largest(node_count: usize, edges: &[(usize, usize)], alive: &[bool]) -> usize {
        let mut adjacency = vec![Vec::new(); node_count];
        for &(left, right) in edges {
            adjacency[left].push(right);
            adjacency[right].push(left);
        }
        let mut visited = vec![false; node_count];
        let mut queue = VecDeque::new();
        let mut largest = 0;
        for start in 0..node_count {
            if !alive[start] || visited[start] {
                continue;
            }
            visited[start] = true;
            queue.push_back(start);
            let mut size = 0;
            while let Some(node) = queue.pop_front() {
                size += 1;
                for &neighbor in &adjacency[node] {
                    if alive[neighbor] && !visited[neighbor] {
                        visited[neighbor] = true;
                        queue.push_back(neighbor);
                    }
                }
            }
            largest = largest.max(size);
        }
        largest
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        #[test]
        fn attack_trace_matches_independent_bfs_and_assortativity_is_bounded(
            node_count in 1usize..10,
            raw in prop::collection::vec((any::<u8>(), any::<u8>()), 0..48),
            priorities in prop::collection::vec(any::<u16>(), 1..10),
            requested_steps in 0usize..10,
        ) {
            let graph = projection_from_raw(node_count, &raw);
            let edges = expected_edges(node_count, &raw);
            let mut ranked = (0..node_count).map(|node| {
                (priorities[node % priorities.len()], node)
            }).collect::<Vec<_>>();
            ranked.sort_unstable();
            let order = ranked.iter().map(|&(_, node)| NodeId::new(node as u32)).collect::<Vec<_>>();
            let max_steps = requested_steps.min(node_count);
            let result = attack_simulation(&graph, &order, max_steps).expect("permutation order");
            let repeat = attack_simulation(&graph, &order, max_steps).expect("repeat order");
            prop_assert_eq!(&result, &repeat);

            let mut alive = vec![true; node_count];
            let mut previous = naive_largest(node_count, &edges, &alive) as f64 / node_count as f64;
            let mut expected_auc = 0.0;
            for (index, &(_, node)) in ranked.iter().take(max_steps).enumerate() {
                alive[node] = false;
                let largest = naive_largest(node_count, &edges, &alive);
                prop_assert_eq!(result.trace[index].largest_component, largest);
                prop_assert_eq!(result.trace[index].remaining_nodes, node_count - index - 1);
                let current = largest as f64 / node_count as f64;
                expected_auc += 0.5 * (previous + current) / max_steps.max(1) as f64;
                previous = current;
            }
            prop_assert!((result.auc - expected_auc).abs() <= 1.0e-12);
            prop_assert_eq!(result.truncated, max_steps < node_count);

            let assortativity = degree_assortativity(&graph).expect("generated projection");
            prop_assert!((-1.0..=1.0).contains(&assortativity));
            prop_assert_eq!(assortativity, degree_assortativity(&graph).expect("repeat coefficient"));

            let reversed = projection_from_raw(node_count, &raw.iter().rev().copied().collect::<Vec<_>>());
            prop_assert_eq!(result, attack_simulation(&reversed, &order, max_steps).expect("permuted attack graph"));
            prop_assert_eq!(assortativity, degree_assortativity(&reversed).expect("permuted coefficient"));
        }
    }
}
