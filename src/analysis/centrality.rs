//! Deterministic centrality measures over graph projections.
//!
//! Iterative kernels traverse dense vertices and sorted CSR neighbors in a
//! fixed order. Traversal-heavy kernels enforce caller-visible work budgets.

use std::collections::{BTreeMap, VecDeque};

use crate::NodeId;

use super::{AnalysisError, GraphProjection};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Hard ceiling applied to every iterative centrality request.
pub const MAX_CENTRALITY_ITERATIONS: usize = 10_000;

/// Default edge-visit budget for all-sources centrality kernels.
pub const DEFAULT_CENTRALITY_WORK_LIMIT: u64 = 2_000_000_000;

/// Identifies the mathematical measure stored in [`CentralityScores`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CentralityKind {
    /// PageRank on the directed projection.
    PageRank,
    /// PageRank on the transposed projection.
    ReversePageRank,
    /// Random-walk-with-restart PageRank.
    PersonalizedPageRank,
    /// Directed vertex betweenness.
    Betweenness,
    /// Principal eigenvector of the nonnegative undirected adjacency matrix.
    Eigenvector,
    /// Katz walk centrality over the nonnegative undirected adjacency matrix.
    Katz,
    /// Wasserman–Faust normalized undirected closeness.
    Closeness,
    /// Normalized undirected harmonic centrality.
    Harmonic,
    /// HITS hub score.
    HitsHub,
    /// HITS authority score.
    HitsAuthority,
    /// Burt's structural-holes constraint.
    BurtConstraint,
}

/// Node-aligned output shared by scalar centrality measures.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CentralityScores {
    /// Measure represented by `scores`.
    pub kind: CentralityKind,
    /// Stable identifiers in projection order.
    pub nodes: Vec<NodeId>,
    /// Scores aligned one-for-one with `nodes`.
    pub scores: Vec<f64>,
    /// Completed fixed-point iterations, or source traversals for betweenness.
    pub iterations: usize,
    /// Whether the requested numerical fixed point was reached.
    pub converged: bool,
}

/// Deterministically ordered undirected edge-betweenness output.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct EdgeCentralityScores {
    /// Canonical `(minimum NodeId, maximum NodeId)` edges.
    pub edges: Vec<(NodeId, NodeId)>,
    /// Scores aligned one-for-one with `edges`.
    pub scores: Vec<f64>,
    /// Number of source traversals completed.
    pub iterations: usize,
    /// Always true for a successful, budget-complete traversal.
    pub converged: bool,
}

/// Coupled HITS hub and authority vectors.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct HitsScores {
    /// Hub scores aligned with projection nodes.
    pub hubs: CentralityScores,
    /// Authority scores aligned with projection nodes.
    pub authorities: CentralityScores,
}

/// Shared convergence controls for power-iteration kernels.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct IterationOptions {
    /// Requested iteration count, clamped to [`MAX_CENTRALITY_ITERATIONS`].
    pub max_iterations: usize,
    /// Maximum componentwise change required for convergence.
    pub tolerance: f64,
}

impl Default for IterationOptions {
    fn default() -> Self {
        Self {
            max_iterations: 100,
            tolerance: 1.0e-8,
        }
    }
}

/// PageRank convergence and damping controls.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PageRankOptions {
    /// Probability of following an edge rather than teleporting.
    pub damping: f64,
    /// Requested iteration count, clamped to [`MAX_CENTRALITY_ITERATIONS`].
    pub max_iterations: usize,
    /// Maximum componentwise change required for convergence.
    pub tolerance: f64,
}

impl Default for PageRankOptions {
    fn default() -> Self {
        Self {
            damping: 0.85,
            max_iterations: 100,
            tolerance: 1.0e-8,
        }
    }
}

/// Katz attenuation, baseline, and convergence controls.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct KatzOptions {
    /// Walk attenuation. It must satisfy `alpha * max_weighted_degree < 1`.
    pub alpha: f64,
    /// Exogenous score assigned on every iteration.
    pub beta: f64,
    /// Requested iteration count, clamped to [`MAX_CENTRALITY_ITERATIONS`].
    pub max_iterations: usize,
    /// Maximum componentwise change required for convergence.
    pub tolerance: f64,
}

impl Default for KatzOptions {
    fn default() -> Self {
        Self {
            alpha: 0.1,
            beta: 1.0,
            max_iterations: 100,
            tolerance: 1.0e-8,
        }
    }
}

/// Work budget for all-sources traversal and neighborhood kernels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CentralityBudget {
    /// Maximum modeled edge visits.
    pub max_work: u64,
}

impl Default for CentralityBudget {
    fn default() -> Self {
        Self {
            max_work: DEFAULT_CENTRALITY_WORK_LIMIT,
        }
    }
}

/// Controls directed vertex-betweenness normalization and work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct BetweennessOptions {
    /// Divide by `(n - 1)(n - 2)` for a directed graph.
    pub normalized: bool,
    /// Maximum modeled edge visits.
    pub max_work: u64,
}

impl Default for BetweennessOptions {
    fn default() -> Self {
        Self {
            normalized: true,
            max_work: DEFAULT_CENTRALITY_WORK_LIMIT,
        }
    }
}

/// Computes PageRank with uniform teleportation and dangling redistribution.
pub fn pagerank(graph: &GraphProjection, options: PageRankOptions) -> CentralityScores {
    pagerank_impl(graph, options, false, None)
}

/// Computes PageRank after reversing every edge.
pub fn reverse_pagerank(graph: &GraphProjection, options: PageRankOptions) -> CentralityScores {
    pagerank_impl(graph, options, true, None)
}

/// Computes personalized PageRank with an L1-normalized restart vector.
///
/// Seed identifiers must belong to the projection and weights must be finite
/// and nonnegative. Repeated identifiers are deterministically summed, and the
/// combined mass must remain finite. When all supplied weights are zero,
/// restart mass is uniform over distinct seeds.
pub fn personalized_pagerank(
    graph: &GraphProjection,
    seeds: &[(NodeId, f64)],
    options: PageRankOptions,
) -> Result<CentralityScores, AnalysisError> {
    require_well_formed(graph, "personalized_pagerank")?;
    if seeds.is_empty() {
        return Err(AnalysisError::InvalidInput {
            analysis: "personalized_pagerank",
            input: "seeds",
            reason: "must contain at least one projection node",
        });
    }

    let mut grouped: BTreeMap<u32, Vec<f64>> = BTreeMap::new();
    for &(node, weight) in seeds {
        if !weight.is_finite() || weight < 0.0 {
            return Err(AnalysisError::InvalidInput {
                analysis: "personalized_pagerank",
                input: "seed weight",
                reason: "must be finite and nonnegative",
            });
        }
        let dense = graph
            .dense_index(node)
            .ok_or(AnalysisError::UnknownNode(node))?;
        grouped.entry(dense).or_default().push(weight);
    }
    let mut restart = vec![0.0; graph.node_count()];
    for (&dense, weights) in &mut grouped {
        let weight = stable_sum(std::mem::take(weights));
        if !weight.is_finite() {
            return Err(AnalysisError::InvalidInput {
                analysis: "personalized_pagerank",
                input: "seed weights",
                reason: "must have a finite sum",
            });
        }
        restart[dense as usize] = weight;
    }
    let total = stable_sum(restart.clone());
    if !total.is_finite() {
        return Err(AnalysisError::InvalidInput {
            analysis: "personalized_pagerank",
            input: "seed weights",
            reason: "must have a finite sum",
        });
    }
    if total > 0.0 {
        for value in &mut restart {
            *value /= total;
        }
    } else {
        let uniform = 1.0 / grouped.len() as f64;
        for dense in grouped.keys() {
            restart[*dense as usize] = uniform;
        }
    }
    Ok(pagerank_impl(graph, options, false, Some(&restart)))
}

fn pagerank_impl(
    graph: &GraphProjection,
    options: PageRankOptions,
    reverse: bool,
    restart: Option<&[f64]>,
) -> CentralityScores {
    let kind = if restart.is_some() {
        CentralityKind::PersonalizedPageRank
    } else if reverse {
        CentralityKind::ReversePageRank
    } else {
        CentralityKind::PageRank
    };
    if !graph.is_well_formed() {
        return failed_scores(kind);
    }
    let n = graph.node_count();
    if n == 0 {
        return scores(graph, kind, Vec::new(), 0, true);
    }
    let teleport = restart
        .map(Vec::from)
        .unwrap_or_else(|| vec![1.0 / n as f64; n]);
    let mut current = teleport.clone();
    if !valid_probability(options.damping) || !valid_tolerance(options.tolerance) {
        return scores(graph, kind, current, 0, false);
    }

    let limit = options.max_iterations.min(MAX_CENTRALITY_ITERATIONS);
    for iteration in 1..=limit {
        let mut dangling_values = Vec::new();
        for dense in 0..n as u32 {
            let outgoing = if reverse {
                graph.predecessors(dense)
            } else {
                graph.successors(dense)
            };
            if outgoing.is_empty() {
                dangling_values.push(current[dense as usize]);
            }
        }
        dangling_values.sort_by(f64::total_cmp);
        let dangling: f64 = dangling_values.iter().sum();

        let mut next = vec![0.0; n];
        for dense in 0..n as u32 {
            let incoming = if reverse {
                graph.successors(dense)
            } else {
                graph.predecessors(dense)
            };
            let mut contributions = Vec::with_capacity(incoming.len());
            for &source in incoming {
                let degree = if reverse {
                    graph.predecessors(source).len()
                } else {
                    graph.successors(source).len()
                };
                if degree != 0 {
                    contributions.push(current[source as usize] / degree as f64);
                }
            }
            contributions.sort_by(f64::total_cmp);
            let received: f64 = contributions.iter().sum();
            next[dense as usize] = (1.0 - options.damping) * teleport[dense as usize]
                + options.damping * (received + dangling * teleport[dense as usize]);
        }
        let diff = max_difference(&current, &next);
        current = next;
        if diff < options.tolerance {
            return scores(graph, kind, current, iteration, true);
        }
    }
    scores(graph, kind, current, limit, false)
}

/// Computes normalized directed vertex betweenness with Brandes' algorithm.
///
/// Shortest-path weights are rescaled independently at each breadth-first
/// layer. This preserves every predecessor ratio while preventing path-count
/// overflow on deep, exponentially branching graphs.
pub fn betweenness_centrality(
    graph: &GraphProjection,
    options: BetweennessOptions,
) -> Result<CentralityScores, AnalysisError> {
    let sources = graph.nodes.clone();
    accumulate_for_sources(graph, &sources, options)
}

/// Accumulates directed Brandes contributions for a deterministic source set.
///
/// Sources are validated, sorted, and deduplicated. Separate source ranges can
/// be evaluated by ordered callers and their aligned score vectors summed.
pub fn accumulate_for_sources(
    graph: &GraphProjection,
    sources: &[NodeId],
    options: BetweennessOptions,
) -> Result<CentralityScores, AnalysisError> {
    require_well_formed(graph, "betweenness")?;
    let mut dense_sources = sources
        .iter()
        .map(|&node| {
            graph
                .dense_index(node)
                .ok_or(AnalysisError::UnknownNode(node))
        })
        .collect::<Result<Vec<_>, _>>()?;
    dense_sources.sort_unstable();
    dense_sources.dedup();
    enforce_work(
        "betweenness",
        dense_sources.len(),
        graph.edge_count(),
        options.max_work,
    )?;

    let n = graph.node_count();
    let mut output = vec![0.0; n];
    let mut predecessors = vec![Vec::<u32>::new(); n];
    let mut path_weight = vec![0.0; n];
    let mut distance = vec![usize::MAX; n];
    let mut dependency = vec![0.0; n];
    let mut stack = Vec::with_capacity(n);
    let mut queue = VecDeque::with_capacity(n);

    for &source in &dense_sources {
        predecessors.iter_mut().for_each(Vec::clear);
        distance.fill(usize::MAX);
        dependency.fill(0.0);
        stack.clear();
        queue.clear();
        distance[source as usize] = 0;
        queue.push_back(source);

        while let Some(vertex) = queue.pop_front() {
            stack.push(vertex);
            for &neighbor in graph.successors(vertex) {
                if distance[neighbor as usize] == usize::MAX {
                    distance[neighbor as usize] = distance[vertex as usize] + 1;
                    queue.push_back(neighbor);
                }
                if distance[neighbor as usize] == distance[vertex as usize] + 1 {
                    predecessors[neighbor as usize].push(vertex);
                }
            }
        }

        rescale_shortest_path_weights(&predecessors, &distance, &stack, source, &mut path_weight);

        while let Some(vertex) = stack.pop() {
            let denominator = stable_sum(
                predecessors[vertex as usize]
                    .iter()
                    .map(|&predecessor| path_weight[predecessor as usize])
                    .collect(),
            );
            if denominator > 0.0 {
                for &predecessor in &predecessors[vertex as usize] {
                    dependency[predecessor as usize] += path_weight[predecessor as usize]
                        / denominator
                        * (1.0 + dependency[vertex as usize]);
                }
            }
            if vertex != source {
                output[vertex as usize] += dependency[vertex as usize];
            }
        }
    }

    if options.normalized && n > 2 {
        let denominator = ((n - 1) * (n - 2)) as f64;
        output.iter_mut().for_each(|score| *score /= denominator);
    }
    Ok(scores(
        graph,
        CentralityKind::Betweenness,
        output,
        dense_sources.len(),
        true,
    ))
}

/// Computes undirected edge betweenness with canonical stable edge keys.
///
/// Per-layer path-weight rescaling prevents overflow without changing Brandes
/// dependency ratios.
pub fn edge_betweenness(
    graph: &GraphProjection,
    budget: CentralityBudget,
) -> Result<EdgeCentralityScores, AnalysisError> {
    require_well_formed(graph, "edge_betweenness")?;
    let adjacency = undirected_neighbors(graph);
    let adjacency_entries = adjacency.iter().map(Vec::len).sum();
    enforce_work(
        "edge_betweenness",
        graph.node_count(),
        adjacency_entries,
        budget.max_work,
    )?;
    let n = graph.node_count();
    let mut values = BTreeMap::<(u32, u32), f64>::new();
    let mut predecessors = vec![Vec::<u32>::new(); n];
    let mut path_weight = vec![0.0; n];
    let mut distance = vec![usize::MAX; n];
    let mut dependency = vec![0.0; n];
    let mut stack = Vec::with_capacity(n);
    let mut queue = VecDeque::with_capacity(n);
    for source in 0..n as u32 {
        predecessors.iter_mut().for_each(Vec::clear);
        distance.fill(usize::MAX);
        dependency.fill(0.0);
        stack.clear();
        queue.clear();
        distance[source as usize] = 0;
        queue.push_back(source);
        while let Some(vertex) = queue.pop_front() {
            stack.push(vertex);
            for &neighbor in &adjacency[vertex as usize] {
                if distance[neighbor as usize] == usize::MAX {
                    distance[neighbor as usize] = distance[vertex as usize] + 1;
                    queue.push_back(neighbor);
                }
                if distance[neighbor as usize] == distance[vertex as usize] + 1 {
                    predecessors[neighbor as usize].push(vertex);
                }
            }
        }
        rescale_shortest_path_weights(&predecessors, &distance, &stack, source, &mut path_weight);
        while let Some(vertex) = stack.pop() {
            let denominator = stable_sum(
                predecessors[vertex as usize]
                    .iter()
                    .map(|&predecessor| path_weight[predecessor as usize])
                    .collect(),
            );
            if denominator > 0.0 {
                for &predecessor in &predecessors[vertex as usize] {
                    let contribution = path_weight[predecessor as usize] / denominator
                        * (1.0 + dependency[vertex as usize]);
                    dependency[predecessor as usize] += contribution;
                    let edge = canonical_dense(predecessor, vertex);
                    *values.entry(edge).or_default() += contribution;
                }
            }
        }
    }
    let (edges, scores) = values
        .into_iter()
        .map(|((left, right), value)| ((graph.node_id(left), graph.node_id(right)), value / 2.0))
        .unzip();
    Ok(EdgeCentralityScores {
        edges,
        scores,
        iterations: n,
        converged: true,
    })
}

/// Computes weighted undirected eigenvector centrality by shifted power iteration.
///
/// A non-finite projection weight produces an aligned non-converged result.
pub fn eigenvector_centrality(
    graph: &GraphProjection,
    options: IterationOptions,
) -> CentralityScores {
    if !graph.is_well_formed() {
        return failed_scores(CentralityKind::Eigenvector);
    }
    let n = graph.node_count();
    if n == 0 {
        return scores(graph, CentralityKind::Eigenvector, Vec::new(), 0, true);
    }
    let mut current = vec![1.0 / (n as f64).sqrt(); n];
    if !valid_tolerance(options.tolerance) || !has_finite_weights(graph) {
        return scores(graph, CentralityKind::Eigenvector, current, 0, false);
    }
    let adjacency = undirected_weighted(graph);
    let limit = options.max_iterations.min(MAX_CENTRALITY_ITERATIONS);
    for iteration in 1..=limit {
        let mut next = multiply(&adjacency, &current);
        for (next, current) in next.iter_mut().zip(&current) {
            *next += *current;
        }
        normalize_l2(&mut next);
        let diff = max_difference(&current, &next);
        current = next;
        if diff < options.tolerance {
            return scores(graph, CentralityKind::Eigenvector, current, iteration, true);
        }
    }
    scores(graph, CentralityKind::Eigenvector, current, limit, false)
}

/// Computes normalized weighted undirected Katz centrality.
///
/// An attenuation outside the documented sufficient convergence region or a
/// non-finite projection weight returns the zero initial vector with
/// `converged == false`.
pub fn katz_centrality(graph: &GraphProjection, options: KatzOptions) -> CentralityScores {
    if !graph.is_well_formed() {
        return failed_scores(CentralityKind::Katz);
    }
    let n = graph.node_count();
    if n == 0 {
        return scores(graph, CentralityKind::Katz, Vec::new(), 0, true);
    }
    let mut current = vec![0.0; n];
    if !valid_tolerance(options.tolerance)
        || !has_finite_weights(graph)
        || !options.alpha.is_finite()
        || options.alpha < 0.0
        || !options.beta.is_finite()
    {
        return scores(graph, CentralityKind::Katz, current, 0, false);
    }
    let adjacency = undirected_weighted(graph);
    let max_degree = adjacency
        .iter()
        .map(|neighbors| stable_sum(neighbors.iter().map(|edge| edge.1).collect()))
        .fold(0.0_f64, f64::max);
    if options.alpha * max_degree >= 1.0 {
        return scores(graph, CentralityKind::Katz, current, 0, false);
    }
    let limit = options.max_iterations.min(MAX_CENTRALITY_ITERATIONS);
    for iteration in 1..=limit {
        let mut next = multiply(&adjacency, &current);
        next.iter_mut()
            .for_each(|value| *value = options.alpha * *value + options.beta);
        let diff = max_difference(&current, &next);
        current = next;
        if diff < options.tolerance {
            normalize_l2(&mut current);
            return scores(graph, CentralityKind::Katz, current, iteration, true);
        }
    }
    normalize_l2(&mut current);
    scores(graph, CentralityKind::Katz, current, limit, false)
}

/// Computes Wasserman–Faust normalized closeness on the undirected projection.
pub fn closeness_centrality(
    graph: &GraphProjection,
    budget: CentralityBudget,
) -> Result<CentralityScores, AnalysisError> {
    distance_centrality(graph, budget, false)
}

/// Computes normalized harmonic centrality on the undirected projection.
pub fn harmonic_centrality(
    graph: &GraphProjection,
    budget: CentralityBudget,
) -> Result<CentralityScores, AnalysisError> {
    distance_centrality(graph, budget, true)
}

fn distance_centrality(
    graph: &GraphProjection,
    budget: CentralityBudget,
    harmonic: bool,
) -> Result<CentralityScores, AnalysisError> {
    let name = if harmonic { "harmonic" } else { "closeness" };
    require_well_formed(graph, name)?;
    let adjacency = undirected_neighbors(graph);
    let adjacency_entries = adjacency.iter().map(Vec::len).sum();
    enforce_work(name, graph.node_count(), adjacency_entries, budget.max_work)?;
    let n = graph.node_count();
    let mut output = vec![0.0; n];
    let mut distance = vec![usize::MAX; n];
    let mut queue = VecDeque::with_capacity(n);
    for source in 0..n {
        distance.fill(usize::MAX);
        queue.clear();
        distance[source] = 0;
        queue.push_back(source as u32);
        while let Some(vertex) = queue.pop_front() {
            for &neighbor in &adjacency[vertex as usize] {
                if distance[neighbor as usize] == usize::MAX {
                    distance[neighbor as usize] = distance[vertex as usize] + 1;
                    queue.push_back(neighbor);
                }
            }
        }
        let reachable = distance
            .iter()
            .enumerate()
            .filter(|&(target, value)| target != source && *value != usize::MAX);
        if harmonic {
            output[source] = reachable.map(|(_, value)| 1.0 / *value as f64).sum::<f64>()
                / (n.saturating_sub(1).max(1)) as f64;
        } else {
            let (count, sum) = reachable.fold((0usize, 0usize), |(count, sum), (_, value)| {
                (count + 1, sum + *value)
            });
            if sum != 0 {
                output[source] =
                    count as f64 / n.saturating_sub(1).max(1) as f64 * count as f64 / sum as f64;
            }
        }
    }
    Ok(scores(
        graph,
        if harmonic {
            CentralityKind::Harmonic
        } else {
            CentralityKind::Closeness
        },
        output,
        n,
        true,
    ))
}

/// Computes mutually reinforcing HITS hub and authority scores.
pub fn hits(graph: &GraphProjection, options: IterationOptions) -> HitsScores {
    if !graph.is_well_formed() {
        return HitsScores {
            hubs: failed_scores(CentralityKind::HitsHub),
            authorities: failed_scores(CentralityKind::HitsAuthority),
        };
    }
    let n = graph.node_count();
    if n == 0 {
        return hits_result(graph, Vec::new(), Vec::new(), 0, true);
    }
    let mut hubs = vec![1.0; n];
    let mut authorities = vec![1.0; n];
    if !valid_tolerance(options.tolerance) {
        return hits_result(graph, hubs, authorities, 0, false);
    }
    let limit = options.max_iterations.min(MAX_CENTRALITY_ITERATIONS);
    for iteration in 1..=limit {
        let mut next_authorities = vec![0.0; n];
        for vertex in 0..n as u32 {
            let values = graph
                .predecessors(vertex)
                .iter()
                .map(|&source| hubs[source as usize])
                .collect::<Vec<_>>();
            next_authorities[vertex as usize] = stable_sum(values);
        }
        let mut next_hubs = vec![0.0; n];
        for vertex in 0..n as u32 {
            let values = graph
                .successors(vertex)
                .iter()
                .map(|&target| next_authorities[target as usize])
                .collect::<Vec<_>>();
            next_hubs[vertex as usize] = stable_sum(values);
        }
        normalize_l2(&mut next_authorities);
        normalize_l2(&mut next_hubs);
        let diff =
            max_difference(&hubs, &next_hubs).max(max_difference(&authorities, &next_authorities));
        hubs = next_hubs;
        authorities = next_authorities;
        if diff < options.tolerance {
            return hits_result(graph, hubs, authorities, iteration, true);
        }
    }
    hits_result(graph, hubs, authorities, limit, false)
}

/// Computes Burt's weighted structural-holes constraint.
///
/// Non-finite projection weights are rejected as invalid input.
pub fn burt_constraint(
    graph: &GraphProjection,
    budget: CentralityBudget,
) -> Result<CentralityScores, AnalysisError> {
    require_well_formed(graph, "burt_constraint")?;
    if !has_finite_weights(graph) {
        return Err(AnalysisError::InvalidInput {
            analysis: "burt_constraint",
            input: "edge weights",
            reason: "must be finite",
        });
    }
    let adjacency = undirected_weighted_without_loops(graph);
    let needed = adjacency.iter().try_fold(0u64, |total, neighbors| {
        let degree = neighbors.len() as u64;
        total.checked_add(degree.saturating_mul(degree))
    });
    let needed = needed.unwrap_or(u64::MAX);
    if needed > budget.max_work {
        return Err(AnalysisError::BudgetExceeded {
            analysis: "burt_constraint",
            unit: "neighbor comparisons",
            needed,
            cap: budget.max_work,
        });
    }
    let totals = adjacency
        .iter()
        .map(|neighbors| stable_sum(neighbors.iter().map(|edge| edge.1).collect()))
        .collect::<Vec<_>>();
    let mut output = vec![0.0; graph.node_count()];
    for vertex in 0..graph.node_count() {
        if totals[vertex] <= 0.0 {
            continue;
        }
        let mut constraint_terms = Vec::with_capacity(adjacency[vertex].len());
        for &(neighbor, direct_weight) in &adjacency[vertex] {
            let direct = direct_weight / totals[vertex];
            let mut indirect_terms = Vec::new();
            for &(middle, middle_weight) in &adjacency[vertex] {
                if middle == neighbor || totals[middle as usize] <= 0.0 {
                    continue;
                }
                if let Ok(position) =
                    adjacency[middle as usize].binary_search_by_key(&neighbor, |edge| edge.0)
                {
                    indirect_terms.push(
                        middle_weight / totals[vertex] * adjacency[middle as usize][position].1
                            / totals[middle as usize],
                    );
                }
            }
            let indirect = stable_sum(indirect_terms);
            constraint_terms.push((direct + indirect).powi(2));
        }
        output[vertex] = stable_sum(constraint_terms);
    }
    Ok(scores(
        graph,
        CentralityKind::BurtConstraint,
        output,
        graph.node_count(),
        true,
    ))
}

fn scores(
    graph: &GraphProjection,
    kind: CentralityKind,
    values: Vec<f64>,
    iterations: usize,
    converged: bool,
) -> CentralityScores {
    CentralityScores {
        kind,
        nodes: graph.nodes.clone(),
        scores: values,
        iterations,
        converged,
    }
}

fn failed_scores(kind: CentralityKind) -> CentralityScores {
    CentralityScores {
        kind,
        nodes: Vec::new(),
        scores: Vec::new(),
        iterations: 0,
        converged: false,
    }
}

fn hits_result(
    graph: &GraphProjection,
    hubs: Vec<f64>,
    authorities: Vec<f64>,
    iterations: usize,
    converged: bool,
) -> HitsScores {
    HitsScores {
        hubs: scores(graph, CentralityKind::HitsHub, hubs, iterations, converged),
        authorities: scores(
            graph,
            CentralityKind::HitsAuthority,
            authorities,
            iterations,
            converged,
        ),
    }
}

fn valid_probability(value: f64) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn valid_tolerance(value: f64) -> bool {
    value.is_finite() && value >= 0.0
}

fn has_finite_weights(graph: &GraphProjection) -> bool {
    graph
        .weights
        .as_ref()
        .is_none_or(|weights| weights.iter().all(|weight| weight.is_finite()))
}

fn rescale_shortest_path_weights(
    predecessors: &[Vec<u32>],
    distance: &[usize],
    breadth_first_order: &[u32],
    source: u32,
    weights: &mut [f64],
) {
    weights.fill(0.0);
    weights[source as usize] = 1.0;

    let mut start = 0;
    while start < breadth_first_order.len() {
        let depth = distance[breadth_first_order[start] as usize];
        let mut end = start + 1;
        while end < breadth_first_order.len()
            && distance[breadth_first_order[end] as usize] == depth
        {
            end += 1;
        }

        if depth != 0 {
            let mut maximum = 0.0_f64;
            for &vertex in &breadth_first_order[start..end] {
                let weight = stable_sum(
                    predecessors[vertex as usize]
                        .iter()
                        .map(|&predecessor| weights[predecessor as usize])
                        .collect(),
                );
                weights[vertex as usize] = weight;
                maximum = maximum.max(weight);
            }
            if maximum > 0.0 {
                for &vertex in &breadth_first_order[start..end] {
                    weights[vertex as usize] /= maximum;
                }
            }
        }
        start = end;
    }
}

fn max_difference(left: &[f64], right: &[f64]) -> f64 {
    left.iter()
        .zip(right)
        .map(|(left, right)| (left - right).abs())
        .fold(0.0, f64::max)
}

fn normalize_l2(values: &mut [f64]) {
    let norm = stable_sum(values.iter().map(|value| value * value).collect()).sqrt();
    if norm > 0.0 {
        values.iter_mut().for_each(|value| *value /= norm);
    }
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
            reason: "must satisfy the GraphProjection CSR invariants",
        })
    }
}

fn enforce_work(
    analysis: &'static str,
    sources: usize,
    edges: usize,
    cap: u64,
) -> Result<(), AnalysisError> {
    let needed = (sources as u64).saturating_mul(edges as u64);
    if needed <= cap {
        Ok(())
    } else {
        Err(AnalysisError::BudgetExceeded {
            analysis,
            unit: "edge visits",
            needed,
            cap,
        })
    }
}

fn undirected_neighbors(graph: &GraphProjection) -> Vec<Vec<u32>> {
    let undirected = graph.undirected();
    (0..undirected.node_count() as u32)
        .map(|vertex| {
            undirected
                .neighbors(vertex)
                .iter()
                .copied()
                .filter(|&neighbor| neighbor != vertex)
                .collect()
        })
        .collect()
}

fn undirected_weighted(graph: &GraphProjection) -> Vec<Vec<(u32, f64)>> {
    weighted_adjacency(graph, true)
}

fn undirected_weighted_without_loops(graph: &GraphProjection) -> Vec<Vec<(u32, f64)>> {
    weighted_adjacency(graph, false)
}

fn weighted_adjacency(graph: &GraphProjection, double_self_loops: bool) -> Vec<Vec<(u32, f64)>> {
    let mut rows = vec![BTreeMap::<u32, Vec<f64>>::new(); graph.node_count()];
    for (source, target, weight) in graph.edges() {
        let weight = weight.max(0.0);
        if source == target {
            if double_self_loops {
                rows[source as usize]
                    .entry(target)
                    .or_default()
                    .extend([weight, weight]);
            }
        } else {
            rows[source as usize]
                .entry(target)
                .or_default()
                .push(weight);
            rows[target as usize]
                .entry(source)
                .or_default()
                .push(weight);
        }
    }
    rows.into_iter()
        .map(|row| {
            row.into_iter()
                .map(|(neighbor, mut weights)| {
                    weights.sort_by(f64::total_cmp);
                    (neighbor, weights.into_iter().sum())
                })
                .collect()
        })
        .collect()
}

fn multiply(adjacency: &[Vec<(u32, f64)>], vector: &[f64]) -> Vec<f64> {
    adjacency
        .iter()
        .map(|neighbors| {
            stable_sum(
                neighbors
                    .iter()
                    .map(|&(neighbor, weight)| weight * vector[neighbor as usize])
                    .collect(),
            )
        })
        .collect()
}

fn stable_sum(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    values.into_iter().sum()
}

fn canonical_dense(left: u32, right: u32) -> (u32, u32) {
    if left <= right {
        (left, right)
    } else {
        (right, left)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use petgraph::{algo::page_rank, graph::DiGraph};
    use proptest::prelude::*;

    use super::*;
    use crate::ProjectionKind;

    fn graph(node_count: usize, edges: &[(usize, usize)]) -> GraphProjection {
        GraphProjection::from_edges(
            ProjectionKind::External,
            (0..node_count as u32).map(NodeId::new),
            edges.iter().map(|&(source, target)| {
                (NodeId::new(source as u32), NodeId::new(target as u32), 1.0)
            }),
        )
    }

    fn close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= 1.0e-8,
            "expected {expected}, got {actual}"
        );
    }

    fn convergent_iterations() -> IterationOptions {
        IterationOptions {
            max_iterations: 1_000,
            tolerance: 1.0e-12,
        }
    }

    fn convergent_pagerank() -> PageRankOptions {
        PageRankOptions {
            max_iterations: 1_000,
            tolerance: 1.0e-12,
            ..PageRankOptions::default()
        }
    }

    #[test]
    fn pagerank_ring_is_uniform_normalized_and_matches_petgraph() {
        let projection = graph(4, &[(0, 1), (1, 2), (2, 3), (3, 0)]);
        let result = pagerank(&projection, convergent_pagerank());
        assert!(result.converged);
        assert_eq!(result.nodes, projection.nodes);
        assert_eq!(result.scores, vec![0.25; 4]);
        assert_eq!(result.scores.iter().sum::<f64>(), 1.0);

        let mut oracle = DiGraph::<(), ()>::new();
        let nodes = (0..4).map(|_| oracle.add_node(())).collect::<Vec<_>>();
        for source in 0..4 {
            oracle.add_edge(nodes[source], nodes[(source + 1) % 4], ());
        }
        for (actual, expected) in result.scores.iter().zip(page_rank(&oracle, 0.85, 20)) {
            close(*actual, expected);
        }
    }

    #[test]
    fn reverse_pagerank_is_pagerank_of_transpose() {
        let projection = graph(4, &[(0, 1), (0, 2), (2, 3)]);
        assert_eq!(
            reverse_pagerank(&projection, convergent_pagerank()).scores,
            pagerank(&projection.transpose(), convergent_pagerank()).scores
        );
    }

    #[test]
    fn personalized_restart_validates_and_normalizes_seed_mass() {
        let projection = graph(3, &[(0, 1), (1, 2)]);
        let result = personalized_pagerank(
            &projection,
            &[
                (NodeId::new(0), 1.0),
                (NodeId::new(0), 2.0),
                (NodeId::new(2), 1.0),
            ],
            convergent_pagerank(),
        )
        .expect("valid personalized PageRank fixture");
        assert!(result.converged);
        close(result.scores.iter().sum(), 1.0);
        assert!(result.scores[0] > result.scores[1]);
        assert!(matches!(
            personalized_pagerank(
                &projection,
                &[(NodeId::new(99), 1.0)],
                PageRankOptions::default()
            ),
            Err(AnalysisError::UnknownNode(NodeId(99)))
        ));
        assert!(personalized_pagerank(&projection, &[], PageRankOptions::default()).is_err());
        assert!(personalized_pagerank(
            &projection,
            &[(NodeId::new(0), -1.0)],
            PageRankOptions::default()
        )
        .is_err());
        assert!(personalized_pagerank(
            &projection,
            &[(NodeId::new(0), f64::MAX), (NodeId::new(1), f64::MAX)],
            PageRankOptions::default()
        )
        .is_err());

        let zero = personalized_pagerank(
            &projection,
            &[(NodeId::new(0), 0.0), (NodeId::new(2), 0.0)],
            PageRankOptions {
                damping: 0.0,
                ..PageRankOptions::default()
            },
        )
        .expect("zero-weight seeds are valid");
        assert_eq!(zero.scores, vec![0.5, 0.0, 0.5]);
    }

    #[test]
    fn directed_brandes_matches_naive_shortest_path_enumeration() {
        let projection = graph(6, &[(0, 1), (0, 2), (1, 3), (2, 3), (1, 4), (4, 3), (3, 5)]);
        let actual = betweenness_centrality(
            &projection,
            BetweennessOptions {
                normalized: false,
                ..BetweennessOptions::default()
            },
        )
        .expect("valid Brandes fixture is within the default budget");
        let expected = naive_betweenness(&projection);
        for (actual, expected) in actual.scores.iter().zip(expected) {
            close(*actual, expected);
        }
    }

    #[test]
    fn ordered_source_partitions_sum_to_full_brandes_result() {
        let projection = graph(5, &[(0, 1), (0, 2), (1, 3), (2, 3), (3, 4)]);
        let options = BetweennessOptions::default();
        let full = betweenness_centrality(&projection, options)
            .expect("full Brandes fixture is within the default budget");
        let left = accumulate_for_sources(&projection, &projection.nodes[..2], options)
            .expect("left source partition is valid");
        let right = accumulate_for_sources(&projection, &projection.nodes[2..], options)
            .expect("right source partition is valid");
        for ((full, left), right) in full.scores.iter().zip(left.scores).zip(right.scores) {
            close(*full, left + right);
        }
        assert!(matches!(
            accumulate_for_sources(&projection, &[NodeId::new(99)], options),
            Err(AnalysisError::UnknownNode(NodeId(99)))
        ));
    }

    #[test]
    fn edge_betweenness_on_path_has_canonical_keys_and_closed_form() {
        let projection = graph(3, &[(0, 1), (1, 2)]);
        let result = edge_betweenness(&projection, CentralityBudget::default())
            .expect("path edge-betweenness fixture is within the default budget");
        assert_eq!(
            result.edges,
            vec![
                (NodeId::new(0), NodeId::new(1)),
                (NodeId::new(1), NodeId::new(2))
            ]
        );
        assert_eq!(result.scores, vec![2.0, 2.0]);
        assert!(result.converged);
    }

    #[test]
    fn brandes_path_ratios_remain_finite_beyond_the_f64_path_count_range() {
        let layers = 1_100usize;
        let sink = 1 + layers * 2;
        let mut edges = vec![(0, 1), (0, 2)];
        for layer in 0..layers - 1 {
            let left = 1 + layer * 2;
            let right = left + 1;
            let next_left = left + 2;
            let next_right = left + 3;
            edges.extend([
                (left, next_left),
                (left, next_right),
                (right, next_left),
                (right, next_right),
            ]);
        }
        edges.extend([(sink - 2, sink), (sink - 1, sink)]);
        let projection = graph(sink + 1, &edges);

        let vertex = accumulate_for_sources(
            &projection,
            &[NodeId::new(0)],
            BetweennessOptions {
                normalized: false,
                ..BetweennessOptions::default()
            },
        )
        .expect("deep layered projection is within the Brandes budget");
        assert!(vertex.scores.iter().all(|score| score.is_finite()));
        for layer in 0..layers {
            assert_eq!(vertex.scores[1 + layer * 2], vertex.scores[2 + layer * 2]);
        }

        let edge = edge_betweenness(&projection, CentralityBudget::default())
            .expect("deep layered projection is within the edge-Brandes budget");
        assert!(edge.scores.iter().all(|score| score.is_finite()));
    }

    #[test]
    fn eigenvector_and_katz_match_star_symmetry_and_normalization() {
        let projection = graph(4, &[(0, 1), (0, 2), (0, 3)]);
        let eigen = eigenvector_centrality(&projection, convergent_iterations());
        assert!(eigen.converged);
        close(eigen.scores[0], 1.0 / 2.0_f64.sqrt());
        for leaf in &eigen.scores[1..] {
            close(*leaf, 1.0 / 6.0_f64.sqrt());
        }
        close(eigen.scores.iter().map(|value| value * value).sum(), 1.0);

        let katz = katz_centrality(
            &projection,
            KatzOptions {
                max_iterations: 1_000,
                tolerance: 1.0e-12,
                ..KatzOptions::default()
            },
        );
        assert!(katz.converged);
        assert!(katz.scores[0] > katz.scores[1]);
        assert_eq!(katz.scores[1], katz.scores[2]);
        assert_eq!(katz.scores[2], katz.scores[3]);
        close(katz.scores.iter().map(|value| value * value).sum(), 1.0);

        let invalid = katz_centrality(
            &projection,
            KatzOptions {
                alpha: 1.0,
                ..KatzOptions::default()
            },
        );
        assert!(!invalid.converged);
        assert_eq!(invalid.iterations, 0);
        assert_eq!(invalid.scores, vec![0.0; 4]);
    }

    #[test]
    fn path_distance_scores_have_known_closed_forms() {
        let projection = graph(3, &[(0, 1), (1, 2)]);
        let closeness = closeness_centrality(&projection, CentralityBudget::default())
            .expect("path closeness fixture is within the default budget");
        let harmonic = harmonic_centrality(&projection, CentralityBudget::default())
            .expect("path harmonic fixture is within the default budget");
        for (actual, expected) in closeness.scores.iter().zip([2.0 / 3.0, 1.0, 2.0 / 3.0]) {
            close(*actual, expected);
        }
        assert_eq!(harmonic.scores, vec![0.75, 1.0, 0.75]);

        let disconnected = graph(3, &[(0, 1)]);
        assert_eq!(
            closeness_centrality(&disconnected, CentralityBudget::default())
                .expect("disconnected closeness fixture is within the default budget")
                .scores,
            vec![0.5, 0.5, 0.0]
        );
    }

    #[test]
    fn hits_separates_one_hub_from_three_authorities() {
        let projection = graph(4, &[(0, 1), (0, 2), (0, 3)]);
        let result = hits(&projection, convergent_iterations());
        assert!(result.hubs.converged);
        assert_eq!(result.hubs.scores, vec![1.0, 0.0, 0.0, 0.0]);
        assert_eq!(result.authorities.scores[0], 0.0);
        for authority in &result.authorities.scores[1..] {
            close(*authority, 1.0 / 3.0_f64.sqrt());
        }
    }

    #[test]
    fn burt_constraint_distinguishes_path_center_and_triangle_redundancy() {
        let path = graph(3, &[(0, 1), (1, 2)]);
        assert_eq!(
            burt_constraint(&path, CentralityBudget::default())
                .expect("path Burt fixture is within the default budget")
                .scores,
            vec![1.0, 0.5, 1.0]
        );
        let triangle = graph(3, &[(0, 1), (1, 2), (2, 0)]);
        for score in burt_constraint(&triangle, CentralityBudget::default())
            .expect("triangle Burt fixture is within the default budget")
            .scores
        {
            close(score, 1.125);
        }
    }

    #[test]
    fn empty_invalid_options_malformed_inputs_and_budgets_are_total() {
        let empty = graph(0, &[]);
        assert!(pagerank(&empty, PageRankOptions::default()).converged);
        assert!(eigenvector_centrality(&empty, IterationOptions::default()).converged);
        assert!(hits(&empty, IterationOptions::default()).hubs.converged);

        let projection = graph(2, &[(0, 1)]);
        let invalid = pagerank(
            &projection,
            PageRankOptions {
                damping: 2.0,
                ..PageRankOptions::default()
            },
        );
        assert!(!invalid.converged);
        assert_eq!(invalid.iterations, 0);
        assert!(matches!(
            betweenness_centrality(
                &projection,
                BetweennessOptions {
                    max_work: 1,
                    ..BetweennessOptions::default()
                }
            ),
            Err(AnalysisError::BudgetExceeded { .. })
        ));
        assert!(matches!(
            burt_constraint(&projection, CentralityBudget { max_work: 0 }),
            Err(AnalysisError::BudgetExceeded { .. })
        ));

        let mut malformed = projection.clone();
        malformed.targets[0] = 9;
        assert!(!pagerank(&malformed, PageRankOptions::default()).converged);
        assert!(!eigenvector_centrality(&malformed, IterationOptions::default()).converged);
        assert!(!hits(&malformed, IterationOptions::default()).hubs.converged);
        assert!(closeness_centrality(&malformed, CentralityBudget::default()).is_err());
        assert!(edge_betweenness(&malformed, CentralityBudget::default()).is_err());

        let nonfinite = GraphProjection::from_edges(
            ProjectionKind::External,
            [NodeId::new(0), NodeId::new(1)],
            [(NodeId::new(0), NodeId::new(1), f64::INFINITY)],
        );
        let eigen = eigenvector_centrality(&nonfinite, IterationOptions::default());
        assert!(!eigen.converged);
        assert!(eigen.scores.iter().all(|score| score.is_finite()));
        let katz = katz_centrality(&nonfinite, KatzOptions::default());
        assert!(!katz.converged);
        assert!(katz.scores.iter().all(|score| score.is_finite()));
        assert!(burt_constraint(&nonfinite, CentralityBudget::default()).is_err());

        let negative = GraphProjection::from_edges(
            ProjectionKind::External,
            [NodeId::new(0), NodeId::new(1)],
            [(NodeId::new(0), NodeId::new(1), -2.0)],
        );
        assert_eq!(
            burt_constraint(&negative, CentralityBudget::default())
                .expect("negative finite weights are clamped")
                .scores,
            vec![0.0, 0.0]
        );
    }

    #[test]
    fn iteration_requests_are_clamped_and_zero_requests_do_no_work() {
        let projection = graph(1, &[]);
        let capped = pagerank(
            &projection,
            PageRankOptions {
                max_iterations: usize::MAX,
                tolerance: 0.1,
                ..PageRankOptions::default()
            },
        );
        assert!(capped.converged);
        assert_eq!(capped.iterations, 1);
        let zero = eigenvector_centrality(
            &projection,
            IterationOptions {
                max_iterations: 0,
                ..IterationOptions::default()
            },
        );
        assert!(!zero.converged);
        assert_eq!(zero.iterations, 0);
    }

    fn naive_betweenness(graph: &GraphProjection) -> Vec<f64> {
        let n = graph.node_count();
        let mut scores = vec![0.0; n];
        for source in 0..n as u32 {
            for target in 0..n as u32 {
                if source == target {
                    continue;
                }
                let mut paths = Vec::new();
                let mut current = vec![source];
                let mut visited = vec![false; n];
                visited[source as usize] = true;
                enumerate_simple_paths(
                    graph,
                    source,
                    target,
                    &mut visited,
                    &mut current,
                    &mut paths,
                );
                let Some(shortest) = paths.iter().map(Vec::len).min() else {
                    continue;
                };
                let shortest_paths = paths
                    .iter()
                    .filter(|path| path.len() == shortest)
                    .collect::<Vec<_>>();
                for vertex in 0..n as u32 {
                    if vertex != source && vertex != target {
                        let through = shortest_paths
                            .iter()
                            .filter(|path| path.contains(&vertex))
                            .count();
                        scores[vertex as usize] += through as f64 / shortest_paths.len() as f64;
                    }
                }
            }
        }
        scores
    }

    fn enumerate_simple_paths(
        graph: &GraphProjection,
        vertex: u32,
        target: u32,
        visited: &mut [bool],
        current: &mut Vec<u32>,
        paths: &mut Vec<Vec<u32>>,
    ) {
        if vertex == target {
            paths.push(current.clone());
            return;
        }
        for &neighbor in graph.successors(vertex) {
            if !visited[neighbor as usize] {
                visited[neighbor as usize] = true;
                current.push(neighbor);
                enumerate_simple_paths(graph, neighbor, target, visited, current, paths);
                current.pop();
                visited[neighbor as usize] = false;
            }
        }
    }

    fn mapped_scores(result: &CentralityScores, reverse_ids: bool) -> BTreeMap<usize, f64> {
        let n = result.nodes.len();
        result
            .nodes
            .iter()
            .copied()
            .zip(result.scores.iter().copied())
            .map(|(node, score)| {
                let raw = node.as_u32() as usize;
                let original = if reverse_ids { 100 + n - 1 - raw } else { raw };
                (original, score)
            })
            .collect()
    }

    fn mapped_edge_scores(
        result: &EdgeCentralityScores,
        node_count: usize,
        reverse_ids: bool,
    ) -> BTreeMap<(usize, usize), f64> {
        result
            .edges
            .iter()
            .copied()
            .zip(result.scores.iter().copied())
            .map(|((source, target), score)| {
                let map = |node: NodeId| {
                    let raw = node.as_u32() as usize;
                    if reverse_ids {
                        100 + node_count - 1 - raw
                    } else {
                        raw
                    }
                };
                let source = map(source);
                let target = map(target);
                ((source.min(target), source.max(target)), score)
            })
            .collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn centralities_are_repeatable_and_invariant_under_stable_id_relabeling(
            n in 1usize..7,
            raw in prop::collection::vec(any::<bool>(), 36),
        ) {
            let edges = (0..n)
                .flat_map(|source| (0..n).map(move |target| (source, target)))
                .zip(raw)
                .filter_map(|((source, target), present)| (present && source != target).then_some((source, target)))
                .collect::<Vec<_>>();
            let original = graph(n, &edges);
            let permuted = GraphProjection::from_edges(
                ProjectionKind::External,
                (0..n).rev().map(|node| NodeId::new(node as u32)),
                edges.iter().rev().map(|&(source, target)| {
                    (NodeId::new(source as u32), NodeId::new(target as u32), 1.0)
                }),
            );
            let relabel = |node: usize| 100 + n - 1 - node;
            let relabeled = GraphProjection::from_edges(
                ProjectionKind::External,
                (0..n).map(|node| NodeId::new(relabel(node) as u32)),
                edges.iter().map(|&(source, target)| {
                    (NodeId::new(relabel(source) as u32), NodeId::new(relabel(target) as u32), 1.0)
                }),
            );
            let page_options = PageRankOptions { max_iterations: 200, tolerance: 1.0e-10, ..PageRankOptions::default() };
            let iteration_options = IterationOptions { max_iterations: 200, tolerance: 1.0e-10 };
            let katz_options = KatzOptions { alpha: 0.01, max_iterations: 200, tolerance: 1.0e-10, ..KatzOptions::default() };
            let budget = CentralityBudget::default();
            let seeds = (0..n)
                .map(|node| (NodeId::new(node as u32), if node == 0 { 1.0e16 } else { 1.0 }))
                .collect::<Vec<_>>();
            let relabeled_seeds = (0..n)
                .map(|node| {
                    (
                        NodeId::new(relabel(node) as u32),
                        if node == 0 { 1.0e16 } else { 1.0 },
                    )
                })
                .collect::<Vec<_>>();

            let original_results = vec![
                pagerank(&original, page_options),
                reverse_pagerank(&original, page_options),
                personalized_pagerank(&original, &seeds, page_options)
                    .expect("generated seed set belongs to the original projection"),
                eigenvector_centrality(&original, iteration_options),
                katz_centrality(&original, katz_options),
                closeness_centrality(&original, budget)
                    .expect("generated projection is within the closeness budget"),
                harmonic_centrality(&original, budget)
                    .expect("generated projection is within the harmonic budget"),
                hits(&original, iteration_options).hubs,
                hits(&original, iteration_options).authorities,
                burt_constraint(&original, budget)
                    .expect("generated projection is within the Burt budget"),
                betweenness_centrality(&original, BetweennessOptions::default())
                    .expect("generated projection is within the Brandes budget"),
            ];
            let repeated_results = vec![
                pagerank(&original, page_options),
                reverse_pagerank(&original, page_options),
                personalized_pagerank(&original, &seeds, page_options)
                    .expect("generated seed set belongs to the original projection"),
                eigenvector_centrality(&original, iteration_options),
                katz_centrality(&original, katz_options),
                closeness_centrality(&original, budget)
                    .expect("generated projection is within the closeness budget"),
                harmonic_centrality(&original, budget)
                    .expect("generated projection is within the harmonic budget"),
                hits(&original, iteration_options).hubs,
                hits(&original, iteration_options).authorities,
                burt_constraint(&original, budget)
                    .expect("generated projection is within the Burt budget"),
                betweenness_centrality(&original, BetweennessOptions::default())
                    .expect("generated projection is within the Brandes budget"),
            ];
            prop_assert_eq!(&original_results, &repeated_results);

            let raw_brandes = betweenness_centrality(
                &original,
                BetweennessOptions {
                    normalized: false,
                    ..BetweennessOptions::default()
                },
            )
            .expect("generated projection is valid and within the Brandes budget");
            let naive_brandes = naive_betweenness(&original);
            for (actual, expected) in raw_brandes.scores.iter().zip(naive_brandes) {
                prop_assert!((actual - expected).abs() <= 1.0e-9, "Brandes differential: {actual} != {expected}");
            }

            let permuted_results = vec![
                pagerank(&permuted, page_options),
                reverse_pagerank(&permuted, page_options),
                personalized_pagerank(&permuted, &seeds, page_options)
                    .expect("generated seed set belongs to the permuted projection"),
                eigenvector_centrality(&permuted, iteration_options),
                katz_centrality(&permuted, katz_options),
                closeness_centrality(&permuted, budget)
                    .expect("permuted projection is within the closeness budget"),
                harmonic_centrality(&permuted, budget)
                    .expect("permuted projection is within the harmonic budget"),
                hits(&permuted, iteration_options).hubs,
                hits(&permuted, iteration_options).authorities,
                burt_constraint(&permuted, budget)
                    .expect("permuted projection is within the Burt budget"),
                betweenness_centrality(&permuted, BetweennessOptions::default())
                    .expect("permuted projection is within the Brandes budget"),
            ];
            prop_assert_eq!(&original_results, &permuted_results);

            let relabeled_results = vec![
                pagerank(&relabeled, page_options),
                reverse_pagerank(&relabeled, page_options),
                personalized_pagerank(&relabeled, &relabeled_seeds, page_options)
                    .expect("generated relabeled seeds belong to the relabeled projection"),
                eigenvector_centrality(&relabeled, iteration_options),
                katz_centrality(&relabeled, katz_options),
                closeness_centrality(&relabeled, budget)
                    .expect("relabeled projection is within the closeness budget"),
                harmonic_centrality(&relabeled, budget)
                    .expect("relabeled projection is within the harmonic budget"),
                hits(&relabeled, iteration_options).hubs,
                hits(&relabeled, iteration_options).authorities,
                burt_constraint(&relabeled, budget)
                    .expect("relabeled projection is within the Burt budget"),
                betweenness_centrality(&relabeled, BetweennessOptions::default())
                    .expect("relabeled projection is within the Brandes budget"),
            ];
            for (left, right) in original_results.iter().zip(&relabeled_results) {
                let kind = left.kind;
                let left = mapped_scores(left, false);
                let right = mapped_scores(right, true);
                prop_assert_eq!(left.keys().collect::<Vec<_>>(), right.keys().collect::<Vec<_>>());
                for (node, left) in left {
                    let right = right[&node];
                    prop_assert!((left - right).abs() <= 1.0e-9, "kind {kind:?}, node {node}: {left} != {right}");
                }
            }

            let original_edges = edge_betweenness(&original, budget)
                .expect("generated projection is within the edge-Brandes budget");
            let repeated_edges = edge_betweenness(&original, budget)
                .expect("repeated projection is within the edge-Brandes budget");
            let permuted_edges = edge_betweenness(&permuted, budget)
                .expect("permuted projection is within the edge-Brandes budget");
            prop_assert_eq!(&original_edges, &repeated_edges);
            prop_assert_eq!(&original_edges, &permuted_edges);

            let relabeled_edges = edge_betweenness(&relabeled, budget)
                .expect("relabeled projection is within the edge-Brandes budget");
            let original_edges = mapped_edge_scores(&original_edges, n, false);
            let relabeled_edges = mapped_edge_scores(&relabeled_edges, n, true);
            prop_assert_eq!(original_edges.keys().collect::<Vec<_>>(), relabeled_edges.keys().collect::<Vec<_>>());
            for (edge, left) in original_edges {
                let right = relabeled_edges[&edge];
                prop_assert!((left - right).abs() <= 1.0e-9, "edge {edge:?}: {left} != {right}");
            }
        }
    }
}
