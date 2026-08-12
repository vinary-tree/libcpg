//! Deterministic, bounded community detection over graph projections.
//!
//! Louvain local moving uses dense vertex order, ascending community candidates,
//! and graph aggregation between levels. The companion connected refinement
//! splits every assigned community into connected components of its induced
//! undirected subgraph.

use std::collections::{BTreeMap, VecDeque};

use crate::NodeId;

use super::{AnalysisError, GraphProjection};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Hard ceiling on local-moving sweeps at one Louvain level.
pub const MAX_LOUVAIN_PASSES: usize = 50;

/// Hard ceiling on Louvain graph-aggregation levels.
pub const MAX_LOUVAIN_LEVELS: usize = 10;

/// Identifies how a [`CommunityAssignment`] was produced.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CommunityKind {
    /// Multi-level Louvain at the stored modularity resolution.
    Louvain {
        /// Resolution parameter used in generalized modularity.
        resolution: f64,
    },
    /// Connected-component refinement at the stored modularity resolution.
    ConnectedRefinement {
        /// Resolution parameter inherited from the input assignment.
        resolution: f64,
    },
}

impl CommunityKind {
    fn resolution(self) -> f64 {
        match self {
            Self::Louvain { resolution } | Self::ConnectedRefinement { resolution } => resolution,
        }
    }
}

/// Node-aligned deterministic community partition.
///
/// Community identifiers are contiguous from zero and ordered by the smallest
/// member's position in `nodes`. Because projection nodes are sorted stable
/// identifiers, community zero therefore contains the globally smallest member.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CommunityAssignment {
    /// Algorithm and generalized-modularity resolution represented by the result.
    pub kind: CommunityKind,
    /// Stable identifiers in projection order.
    pub nodes: Vec<NodeId>,
    /// Community identifiers aligned one-for-one with `nodes`.
    pub community: Vec<u32>,
    /// Number of distinct communities.
    pub num_communities: usize,
    /// Generalized weighted-undirected modularity of this exact partition.
    pub modularity: f64,
}

/// Controls deterministic multi-level Louvain.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LouvainOptions {
    /// Generalized-modularity resolution; must be finite and nonnegative.
    pub resolution: f64,
    /// Requested local-moving sweeps per level, clamped to [`MAX_LOUVAIN_PASSES`].
    pub max_passes: usize,
    /// Requested aggregation levels, clamped to [`MAX_LOUVAIN_LEVELS`].
    pub max_levels: usize,
}

impl Default for LouvainOptions {
    fn default() -> Self {
        Self {
            resolution: 1.0,
            max_passes: MAX_LOUVAIN_PASSES,
            max_levels: MAX_LOUVAIN_LEVELS,
        }
    }
}

/// Computes a deterministic multi-level Louvain partition.
///
/// Edge direction is forgotten. Reciprocal directed weights are summed, zero
/// weights are inert, and self-loops contribute twice to weighted degree.
/// Negative or non-finite weights are rejected because generalized modularity
/// assumes a finite nonnegative measure. An empty graph returns an empty
/// partition. With zero levels or zero passes, the canonical singleton
/// partition is returned without local moving.
pub fn louvain(
    graph: &GraphProjection,
    options: LouvainOptions,
) -> Result<CommunityAssignment, AnalysisError> {
    validate_projection(graph, "louvain")?;
    validate_resolution(options.resolution, "louvain")?;
    let base = LevelGraph::from_projection(graph, "louvain")?;
    let n = base.node_count();
    let mut original_to_level = (0..n as u32).collect::<Vec<_>>();
    let max_passes = options.max_passes.min(MAX_LOUVAIN_PASSES);
    let max_levels = options.max_levels.min(MAX_LOUVAIN_LEVELS);

    if n > 0 && base.total_edge_weight > 0.0 && max_passes > 0 && max_levels > 0 {
        let mut level_graph = base.clone();
        for _ in 0..max_levels {
            let moved = local_move(&level_graph, options.resolution, max_passes);
            for assigned in &mut original_to_level {
                *assigned = moved.community[*assigned as usize];
            }
            if moved.num_communities == level_graph.node_count() || moved.num_communities <= 1 {
                break;
            }
            level_graph = level_graph.aggregate(&moved.community, moved.num_communities)?;
        }
    }

    let (community, num_communities) = canonicalize(&original_to_level);
    let modularity = base.modularity(&community, num_communities, options.resolution);
    Ok(CommunityAssignment {
        kind: CommunityKind::Louvain {
            resolution: options.resolution,
        },
        nodes: graph.nodes.clone(),
        community,
        num_communities,
        modularity,
    })
}

/// Splits assigned communities until every induced undirected subgraph is connected.
///
/// This total convenience wrapper returns the canonical singleton partition if
/// the projection or supplied assignment is malformed. Call
/// [`try_refine_connected`] when the caller needs the typed rejection reason.
pub fn refine_connected(
    graph: &GraphProjection,
    assignment: &CommunityAssignment,
) -> CommunityAssignment {
    match try_refine_connected(graph, assignment) {
        Ok(refined) => refined,
        Err(_) => fallback_singletons(graph, assignment.kind.resolution()),
    }
}

/// Checked form of [`refine_connected`].
///
/// The supplied assignment must align exactly with the projection and already
/// obey canonical contiguous community numbering. Its stored resolution is
/// reused when the refined partition's modularity is recomputed.
pub fn try_refine_connected(
    graph: &GraphProjection,
    assignment: &CommunityAssignment,
) -> Result<CommunityAssignment, AnalysisError> {
    validate_projection(graph, "community refinement")?;
    let resolution = assignment.kind.resolution();
    validate_resolution(resolution, "community refinement")?;
    validate_assignment(graph, assignment)?;
    let base = LevelGraph::from_projection(graph, "community refinement")?;
    let undirected = graph.undirected();
    let n = graph.node_count();

    let mut by_community: BTreeMap<u32, Vec<usize>> = BTreeMap::new();
    for (node, &community) in assignment.community.iter().enumerate() {
        by_community.entry(community).or_default().push(node);
    }

    let mut components = Vec::<Vec<usize>>::new();
    let mut in_community = vec![false; n];
    let mut visited = vec![false; n];
    let mut queue = VecDeque::with_capacity(n);
    for members in by_community.into_values() {
        for &member in &members {
            in_community[member] = true;
        }
        for &start in &members {
            if visited[start] {
                continue;
            }
            let mut component = Vec::new();
            visited[start] = true;
            queue.push_back(start as u32);
            while let Some(node) = queue.pop_front() {
                component.push(node as usize);
                for &neighbor in undirected.neighbors(node) {
                    let neighbor = neighbor as usize;
                    if in_community[neighbor] && !visited[neighbor] {
                        visited[neighbor] = true;
                        queue.push_back(neighbor as u32);
                    }
                }
            }
            component.sort_unstable();
            components.push(component);
        }
        for &member in &members {
            in_community[member] = false;
        }
    }

    components.sort_unstable_by_key(|component| component[0]);
    let mut community = vec![0u32; n];
    for (id, component) in components.iter().enumerate() {
        let id = u32::try_from(id).expect("community count fits the projection's u32 domain");
        for &node in component {
            community[node] = id;
        }
    }
    let num_communities = components.len();
    let modularity = base.modularity(&community, num_communities, resolution);
    Ok(CommunityAssignment {
        kind: CommunityKind::ConnectedRefinement { resolution },
        nodes: graph.nodes.clone(),
        community,
        num_communities,
        modularity,
    })
}

#[derive(Debug, Clone)]
struct LevelGraph {
    offsets: Vec<usize>,
    targets: Vec<u32>,
    weights: Vec<f64>,
    degree: Vec<f64>,
    edges: Vec<(u32, u32, f64)>,
    total_edge_weight: f64,
}

impl LevelGraph {
    fn from_projection(
        graph: &GraphProjection,
        analysis: &'static str,
    ) -> Result<Self, AnalysisError> {
        if graph
            .edges()
            .any(|(_, _, weight)| !weight.is_finite() || weight < 0.0)
        {
            return Err(invalid_weights(analysis));
        }
        let undirected = graph.undirected();
        let mut edges = Vec::with_capacity(undirected.edge_count());
        for source in 0..undirected.node_count() {
            let neighbors = undirected.neighbors(source as u32);
            let weights = undirected.neighbor_weights(source as u32);
            for (offset, &target) in neighbors.iter().enumerate() {
                if target < source as u32 {
                    continue;
                }
                let weight = weights.map_or(1.0, |weights| weights[offset]);
                if !weight.is_finite() || weight < 0.0 {
                    return Err(invalid_weights(analysis));
                }
                if weight > 0.0 {
                    edges.push((source as u32, target, weight));
                }
            }
        }
        Self::from_canonical_edges(undirected.node_count(), edges, analysis)
    }

    fn from_canonical_edges(
        node_count: usize,
        edges: Vec<(u32, u32, f64)>,
        analysis: &'static str,
    ) -> Result<Self, AnalysisError> {
        let total_edge_weight = stable_sum(edges.iter().map(|edge| edge.2));
        if !total_edge_weight.is_finite() {
            return Err(invalid_weights(analysis));
        }

        let mut adjacency = Vec::with_capacity(edges.len().saturating_mul(2));
        let mut degree = vec![0.0; node_count];
        for &(source, target, weight) in &edges {
            if source == target {
                let doubled = weight * 2.0;
                if !doubled.is_finite() {
                    return Err(invalid_weights(analysis));
                }
                adjacency.push((source, target, doubled));
                degree[source as usize] += doubled;
            } else {
                adjacency.push((source, target, weight));
                adjacency.push((target, source, weight));
                degree[source as usize] += weight;
                degree[target as usize] += weight;
            }
        }
        if degree.iter().any(|degree| !degree.is_finite()) {
            return Err(invalid_weights(analysis));
        }
        adjacency.sort_unstable_by_key(|&(source, target, _)| (source, target));
        let mut offsets = Vec::with_capacity(node_count + 1);
        let mut targets = Vec::with_capacity(adjacency.len());
        let mut weights = Vec::with_capacity(adjacency.len());
        let mut cursor = 0usize;
        for source in 0..node_count {
            offsets.push(cursor);
            while cursor < adjacency.len() && adjacency[cursor].0 == source as u32 {
                targets.push(adjacency[cursor].1);
                weights.push(adjacency[cursor].2);
                cursor += 1;
            }
        }
        offsets.push(cursor);
        let total_strength = stable_sum(degree.iter().copied());
        if !total_strength.is_finite() {
            return Err(invalid_weights(analysis));
        }
        Ok(Self {
            offsets,
            targets,
            weights,
            degree,
            edges,
            total_edge_weight,
        })
    }

    fn node_count(&self) -> usize {
        self.degree.len()
    }

    fn neighbors(&self, node: usize) -> impl Iterator<Item = (u32, f64)> + '_ {
        let start = self.offsets[node];
        let end = self.offsets[node + 1];
        self.targets[start..end]
            .iter()
            .copied()
            .zip(self.weights[start..end].iter().copied())
    }

    fn aggregate(&self, community: &[u32], num_communities: usize) -> Result<Self, AnalysisError> {
        let mut grouped: BTreeMap<(u32, u32), Vec<f64>> = BTreeMap::new();
        for &(source, target, weight) in &self.edges {
            let left = community[source as usize];
            let right = community[target as usize];
            let edge = if left <= right {
                (left, right)
            } else {
                (right, left)
            };
            grouped.entry(edge).or_default().push(weight);
        }
        let mut edges = Vec::with_capacity(grouped.len());
        for ((source, target), mut weights) in grouped {
            weights.sort_by(f64::total_cmp);
            let weight = stable_sum(weights);
            if !weight.is_finite() {
                return Err(invalid_weights("louvain aggregation"));
            }
            if weight > 0.0 {
                edges.push((source, target, weight));
            }
        }
        Self::from_canonical_edges(num_communities, edges, "louvain aggregation")
    }

    fn modularity(&self, community: &[u32], count: usize, resolution: f64) -> f64 {
        if self.total_edge_weight == 0.0 || count == 0 {
            return 0.0;
        }
        let total_strength = self.total_edge_weight * 2.0;
        let mut internal = vec![0.0; count];
        let mut strength = vec![0.0; count];
        for (node, &degree) in self.degree.iter().enumerate() {
            strength[community[node] as usize] += degree;
        }
        for &(source, target, weight) in &self.edges {
            if community[source as usize] == community[target as usize] {
                internal[community[source as usize] as usize] += weight;
            }
        }
        stable_sum((0..count).map(|community| {
            let fraction = strength[community] / total_strength;
            internal[community] / self.total_edge_weight - (resolution * fraction) * fraction
        }))
    }
}

struct LocalMove {
    community: Vec<u32>,
    num_communities: usize,
}

fn local_move(graph: &LevelGraph, resolution: f64, max_passes: usize) -> LocalMove {
    let n = graph.node_count();
    let mut community = (0..n as u32).collect::<Vec<_>>();
    if graph.total_edge_weight == 0.0 || max_passes == 0 {
        return LocalMove {
            community,
            num_communities: n,
        };
    }

    let total_strength = graph.total_edge_weight * 2.0;
    let mut community_strength = graph.degree.clone();
    let mut weights_to_community = vec![0.0; n];
    let mut candidates = Vec::<u32>::new();

    for _ in 0..max_passes {
        let mut changed = false;
        for node in 0..n {
            let degree = graph.degree[node];
            if degree == 0.0 {
                continue;
            }
            let old = community[node];
            community_strength[old as usize] = (community_strength[old as usize] - degree).max(0.0);

            for &candidate in &candidates {
                weights_to_community[candidate as usize] = 0.0;
            }
            candidates.clear();
            for (neighbor, weight) in graph.neighbors(node) {
                if neighbor == node as u32 {
                    continue;
                }
                let candidate = community[neighbor as usize];
                if weights_to_community[candidate as usize] == 0.0 {
                    candidates.push(candidate);
                }
                weights_to_community[candidate as usize] += weight;
            }
            candidates.push(old);
            candidates.sort_unstable();
            candidates.dedup();

            let old_score = candidate_score(
                weights_to_community[old as usize],
                degree,
                community_strength[old as usize],
                total_strength,
                resolution,
            );
            let mut best = old;
            let mut best_score = old_score;
            for &candidate in &candidates {
                let score = candidate_score(
                    weights_to_community[candidate as usize],
                    degree,
                    community_strength[candidate as usize],
                    total_strength,
                    resolution,
                );
                if score > best_score || (score == best_score && candidate < best) {
                    best = candidate;
                    best_score = score;
                }
            }

            community[node] = best;
            community_strength[best as usize] += degree;
            changed |= best != old;
        }
        if !changed {
            break;
        }
    }

    let (community, num_communities) = canonicalize(&community);
    LocalMove {
        community,
        num_communities,
    }
}

fn candidate_score(
    weight_to_community: f64,
    degree: f64,
    community_strength: f64,
    total_strength: f64,
    resolution: f64,
) -> f64 {
    weight_to_community / total_strength
        - (resolution * (degree / total_strength)) * (community_strength / total_strength)
}

fn canonicalize(raw: &[u32]) -> (Vec<u32>, usize) {
    let mut first_member = BTreeMap::<u32, usize>::new();
    for (node, &community) in raw.iter().enumerate() {
        first_member.entry(community).or_insert(node);
    }
    let mut ordered = first_member.into_iter().collect::<Vec<_>>();
    ordered.sort_unstable_by_key(|&(_, first)| first);
    let remap = ordered
        .iter()
        .enumerate()
        .map(|(new, &(old, _))| {
            (
                old,
                u32::try_from(new).expect("community count fits the projection's u32 domain"),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let canonical = raw
        .iter()
        .map(|community| remap[community])
        .collect::<Vec<_>>();
    (canonical, ordered.len())
}

fn validate_projection(
    graph: &GraphProjection,
    analysis: &'static str,
) -> Result<(), AnalysisError> {
    if graph.is_well_formed() {
        Ok(())
    } else {
        Err(AnalysisError::InvalidInput {
            analysis,
            input: "projection",
            reason: "CSR fields must be structurally well formed",
        })
    }
}

fn validate_resolution(resolution: f64, analysis: &'static str) -> Result<(), AnalysisError> {
    if resolution.is_finite() && resolution >= 0.0 {
        Ok(())
    } else {
        Err(AnalysisError::InvalidInput {
            analysis,
            input: "resolution",
            reason: "resolution must be finite and nonnegative",
        })
    }
}

fn validate_assignment(
    graph: &GraphProjection,
    assignment: &CommunityAssignment,
) -> Result<(), AnalysisError> {
    if assignment.nodes != graph.nodes || assignment.community.len() != graph.node_count() {
        return Err(AnalysisError::InvalidInput {
            analysis: "community refinement",
            input: "assignment alignment",
            reason: "nodes and community vectors must align with the projection",
        });
    }
    let (canonical, count) = canonicalize(&assignment.community);
    if canonical != assignment.community || count != assignment.num_communities {
        return Err(AnalysisError::InvalidInput {
            analysis: "community refinement",
            input: "community identifiers",
            reason: "identifiers must be contiguous and ordered by smallest member",
        });
    }
    Ok(())
}

fn invalid_weights(analysis: &'static str) -> AnalysisError {
    AnalysisError::InvalidInput {
        analysis,
        input: "edge weights",
        reason: "weights and their undirected sums must be finite and nonnegative",
    }
}

fn fallback_singletons(graph: &GraphProjection, resolution: f64) -> CommunityAssignment {
    let resolution = if resolution.is_finite() && resolution >= 0.0 {
        resolution
    } else {
        LouvainOptions::default().resolution
    };
    let community = (0..graph.node_count() as u32).collect::<Vec<_>>();
    let modularity = LevelGraph::from_projection(graph, "community refinement")
        .map(|level| level.modularity(&community, community.len(), resolution))
        .unwrap_or(0.0);
    CommunityAssignment {
        kind: CommunityKind::ConnectedRefinement { resolution },
        nodes: graph.nodes.clone(),
        num_communities: community.len(),
        community,
        modularity,
    }
}

fn stable_sum(values: impl IntoIterator<Item = f64>) -> f64 {
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
    sum + correction
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use proptest::prelude::*;

    use super::*;
    use crate::ProjectionKind;

    fn graph(node_count: usize, edges: &[(usize, usize, f64)]) -> GraphProjection {
        GraphProjection::from_edges(
            ProjectionKind::External,
            (0..node_count as u32).map(NodeId::new),
            edges.iter().map(|&(source, target, weight)| {
                (
                    NodeId::new(source as u32),
                    NodeId::new(target as u32),
                    weight,
                )
            }),
        )
    }

    fn close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= 1.0e-10,
            "expected {expected}, got {actual}"
        );
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
                internal[community] / total
                    - resolution * (degree[community] / (2.0 * total)).powi(2)
            })
            .sum()
    }

    fn canonical(raw: &[u32]) -> (Vec<u32>, usize) {
        canonicalize(raw)
    }

    fn internally_connected(graph: &GraphProjection, assignment: &[u32]) -> bool {
        let undirected = graph.undirected();
        let mut members = BTreeMap::<u32, Vec<usize>>::new();
        for (node, &community) in assignment.iter().enumerate() {
            members.entry(community).or_default().push(node);
        }
        for group in members.into_values() {
            let mut allowed = vec![false; graph.node_count()];
            for &node in &group {
                allowed[node] = true;
            }
            let mut visited = vec![false; graph.node_count()];
            let mut queue = VecDeque::new();
            visited[group[0]] = true;
            queue.push_back(group[0] as u32);
            while let Some(node) = queue.pop_front() {
                for &neighbor in undirected.neighbors(node) {
                    let neighbor = neighbor as usize;
                    if allowed[neighbor] && !visited[neighbor] {
                        visited[neighbor] = true;
                        queue.push_back(neighbor as u32);
                    }
                }
            }
            if group.iter().any(|&node| !visited[node]) {
                return false;
            }
        }
        true
    }

    #[test]
    fn planted_weighted_cliques_are_recovered_with_positive_modularity() {
        let mut edges = Vec::new();
        for start in [0usize, 4] {
            for left in start..start + 4 {
                for right in left + 1..start + 4 {
                    edges.push((left, right, 1.0));
                }
            }
        }
        edges.push((3, 4, 0.1));
        let graph = graph(8, &edges);
        let assignment =
            louvain(&graph, LouvainOptions::default()).expect("finite planted partition is valid");
        assert_eq!(assignment.community, vec![0, 0, 0, 0, 1, 1, 1, 1]);
        assert_eq!(assignment.num_communities, 2);
        assert!(assignment.modularity > 0.4);
        close(
            assignment.modularity,
            independent_modularity(&graph, &assignment.community, 1.0),
        );
    }

    #[test]
    fn complete_graph_and_self_loops_have_expected_partitions_and_modularity() {
        let complete = graph(
            4,
            &[
                (0, 1, 1.0),
                (0, 2, 1.0),
                (0, 3, 1.0),
                (1, 2, 1.0),
                (1, 3, 1.0),
                (2, 3, 1.0),
            ],
        );
        let assignment = louvain(&complete, LouvainOptions::default())
            .expect("complete graph has finite weights");
        assert_eq!(assignment.community, vec![0, 0, 0, 0]);
        close(assignment.modularity, 0.0);

        let loops = graph(2, &[(0, 0, 3.0), (1, 1, 2.0)]);
        let assignment =
            louvain(&loops, LouvainOptions::default()).expect("self-loop graph has finite weights");
        assert_eq!(assignment.community, vec![0, 1]);
        close(
            assignment.modularity,
            independent_modularity(&loops, &assignment.community, 1.0),
        );
    }

    #[test]
    fn connected_refinement_splits_disconnected_groups_and_recomputes_q() {
        let graph = graph(5, &[(0, 1, 1.0), (2, 3, 1.0), (3, 4, 1.0)]);
        let input = CommunityAssignment {
            kind: CommunityKind::Louvain { resolution: 0.75 },
            nodes: graph.nodes.clone(),
            community: vec![0, 0, 0, 0, 0],
            num_communities: 1,
            modularity: 0.0,
        };
        let refined = try_refine_connected(&graph, &input).expect("aligned assignment");
        assert_eq!(refined.community, vec![0, 0, 1, 1, 1]);
        assert_eq!(refined.num_communities, 2);
        assert!(matches!(
            refined.kind,
            CommunityKind::ConnectedRefinement { resolution: 0.75 }
        ));
        assert!(internally_connected(&graph, &refined.community));
        close(
            refined.modularity,
            independent_modularity(&graph, &refined.community, 0.75),
        );
    }

    #[test]
    fn aggregation_conserves_weight_and_enables_multiple_levels() {
        let graph = graph(
            6,
            &[
                (0, 1, 3.0),
                (1, 2, 3.0),
                (0, 2, 3.0),
                (3, 4, 3.0),
                (4, 5, 3.0),
                (3, 5, 3.0),
                (2, 3, 0.5),
            ],
        );
        let level = LevelGraph::from_projection(&graph, "test").expect("finite weights");
        let moved = local_move(&level, 1.0, MAX_LOUVAIN_PASSES);
        assert!(moved.num_communities < level.node_count());
        let aggregated = level
            .aggregate(&moved.community, moved.num_communities)
            .expect("finite aggregate");
        close(aggregated.total_edge_weight, level.total_edge_weight);
        let assignment = louvain(
            &graph,
            LouvainOptions {
                max_levels: 2,
                ..LouvainOptions::default()
            },
        )
        .expect("two-level fixture");
        close(
            assignment.modularity,
            independent_modularity(&graph, &assignment.community, 1.0),
        );
    }

    #[test]
    fn ring_of_cliques_uses_aggregation_beyond_the_first_level() {
        let clique_count = 8usize;
        let clique_size = 3usize;
        let mut edges = Vec::new();
        for clique in 0..clique_count {
            let start = clique * clique_size;
            for left in start..start + clique_size {
                for right in left + 1..start + clique_size {
                    edges.push((left, right, 1.0));
                }
            }
            let next = ((clique + 1) % clique_count) * clique_size;
            edges.push((start + clique_size - 1, next, 1.0));
        }
        let graph = graph(clique_count * clique_size, &edges);
        let one_level = louvain(
            &graph,
            LouvainOptions {
                max_levels: 1,
                ..LouvainOptions::default()
            },
        )
        .expect("one-level ring of cliques");
        let multi_level =
            louvain(&graph, LouvainOptions::default()).expect("multi-level ring of cliques");
        assert!(
            multi_level.num_communities < one_level.num_communities,
            "aggregation must merge first-level groups: one={one_level:?}, multi={multi_level:?}"
        );
        assert!(multi_level.modularity >= one_level.modularity);
    }

    #[test]
    fn empty_edgeless_zero_work_and_hard_caps_are_explicit() {
        let empty = graph(0, &[]);
        let result = louvain(&empty, LouvainOptions::default()).expect("empty graph is valid");
        assert!(result.nodes.is_empty());
        assert!(result.community.is_empty());
        assert_eq!(result.num_communities, 0);
        assert_eq!(result.modularity, 0.0);

        let edgeless = graph(3, &[]);
        let singleton =
            louvain(&edgeless, LouvainOptions::default()).expect("edgeless graph is valid");
        assert_eq!(singleton.community, vec![0, 1, 2]);
        let no_levels = louvain(
            &graph(3, &[(0, 1, 1.0), (1, 2, 1.0)]),
            LouvainOptions {
                max_levels: 0,
                ..LouvainOptions::default()
            },
        )
        .expect("zero levels is a valid no-work request");
        assert_eq!(no_levels.community, vec![0, 1, 2]);

        let fixture = graph(5, &[(0, 1, 1.0), (1, 2, 1.0), (2, 3, 1.0)]);
        assert_eq!(
            louvain(
                &fixture,
                LouvainOptions {
                    max_passes: usize::MAX,
                    max_levels: usize::MAX,
                    ..LouvainOptions::default()
                }
            ),
            louvain(&fixture, LouvainOptions::default())
        );
    }

    #[test]
    fn malformed_inputs_and_checked_refinement_return_typed_errors() {
        let valid = graph(3, &[(0, 1, 1.0)]);
        assert!(matches!(
            louvain(
                &valid,
                LouvainOptions {
                    resolution: f64::NAN,
                    ..LouvainOptions::default()
                }
            ),
            Err(AnalysisError::InvalidInput {
                input: "resolution",
                ..
            })
        ));
        assert!(louvain(
            &valid,
            LouvainOptions {
                resolution: -1.0,
                ..LouvainOptions::default()
            }
        )
        .is_err());
        assert!(louvain(
            &graph(2, &[(0, 1, f64::INFINITY)]),
            LouvainOptions::default()
        )
        .is_err());
        assert!(louvain(&graph(2, &[(0, 1, f64::NAN)]), LouvainOptions::default()).is_err());
        assert!(louvain(&graph(2, &[(0, 1, -1.0)]), LouvainOptions::default()).is_err());
        assert!(louvain(
            &graph(2, &[(0, 1, f64::MAX), (1, 0, f64::MAX)]),
            LouvainOptions::default()
        )
        .is_err());

        let mut malformed = valid.clone();
        malformed.offsets = vec![0, 2, 1, 1];
        assert!(louvain(&malformed, LouvainOptions::default()).is_err());

        let assignment = CommunityAssignment {
            kind: CommunityKind::Louvain { resolution: 1.0 },
            nodes: valid.nodes.clone(),
            community: vec![7, 7, 9],
            num_communities: 2,
            modularity: 0.0,
        };
        assert!(try_refine_connected(&valid, &assignment).is_err());
        let fallback = refine_connected(&valid, &assignment);
        assert_eq!(fallback.community, vec![0, 1, 2]);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn repeated_assignments_are_byte_identical_and_public_types_round_trip() {
        let graph = graph(
            6,
            &[
                (0, 1, 1.0),
                (1, 2, 1.0),
                (2, 0, 1.0),
                (3, 4, 1.0),
                (4, 5, 1.0),
                (5, 3, 1.0),
                (2, 3, 0.1),
            ],
        );
        let options = LouvainOptions {
            resolution: 0.75,
            max_passes: 17,
            max_levels: 4,
        };
        let first = louvain(&graph, options).expect("finite weighted projection");
        let second = louvain(&graph, options).expect("repeat weighted projection");
        let first_bytes = serde_json::to_vec(&first).expect("serialize first assignment");
        let second_bytes = serde_json::to_vec(&second).expect("serialize repeated assignment");
        assert_eq!(first_bytes, second_bytes);
        assert_eq!(
            serde_json::from_slice::<CommunityAssignment>(&first_bytes)
                .expect("deserialize assignment"),
            first
        );

        let option_bytes = serde_json::to_vec(&options).expect("serialize options");
        assert_eq!(
            serde_json::from_slice::<LouvainOptions>(&option_bytes).expect("deserialize options"),
            options
        );
    }

    proptest! {
        #[test]
        fn louvain_is_repeatable_permutation_invariant_and_reports_independent_q(
            node_count in 1usize..9,
            raw_edges in prop::collection::vec((0usize..8, 0usize..8, 1u8..5), 0..32),
            resolution in 0.25f64..2.5,
        ) {
            let mut edges = raw_edges
                .into_iter()
                .filter(|&(source, target, _)| source < node_count && target < node_count)
                .map(|(source, target, weight)| (source, target, f64::from(weight)))
                .collect::<Vec<_>>();
            let forward = graph(node_count, &edges);
            edges.reverse();
            let reversed = graph(node_count, &edges);
            let options = LouvainOptions { resolution, ..LouvainOptions::default() };
            let first = louvain(&forward, options).expect("generated weights are finite");
            let second = louvain(&forward, options).expect("repeat generated run");
            let permuted = louvain(&reversed, options).expect("permuted storage run");
            prop_assert_eq!(&first, &second);
            prop_assert_eq!(&first, &permuted);
            prop_assert_eq!(first.nodes.len(), first.community.len());
            let (canonical, count) = canonical(&first.community);
            prop_assert_eq!(&canonical, &first.community);
            prop_assert_eq!(count, first.num_communities);
            prop_assert!((first.modularity
                - independent_modularity(&forward, &first.community, resolution)).abs() <= 1.0e-9);
            let singleton = (0..node_count as u32).collect::<Vec<_>>();
            prop_assert!(first.modularity + 1.0e-12
                >= independent_modularity(&forward, &singleton, resolution));
        }

        #[test]
        fn refinement_makes_every_generated_community_connected(
            node_count in 1usize..10,
            raw_edges in prop::collection::vec((0usize..9, 0usize..9), 0..36),
            raw_assignment in prop::collection::vec(0u32..5, 1..10),
        ) {
            let edges = raw_edges
                .into_iter()
                .filter(|&(source, target)| source < node_count && target < node_count)
                .map(|(source, target)| (source, target, 1.0))
                .collect::<Vec<_>>();
            let graph = graph(node_count, &edges);
            let mut raw_assignment = raw_assignment;
            raw_assignment.resize(node_count, 0);
            raw_assignment.truncate(node_count);
            let (community, num_communities) = canonical(&raw_assignment);
            let assignment = CommunityAssignment {
                kind: CommunityKind::Louvain { resolution: 1.0 },
                nodes: graph.nodes.clone(),
                community,
                num_communities,
                modularity: 0.0,
            };
            let refined = try_refine_connected(&graph, &assignment)
                .expect("generated assignment is aligned and canonical");
            prop_assert!(internally_connected(&graph, &refined.community));
            let (canonical, count) = canonical(&refined.community);
            prop_assert_eq!(&canonical, &refined.community);
            prop_assert_eq!(count, refined.num_communities);
        }
    }
}
