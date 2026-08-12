//! Deterministic spectral analysis over graph projections.
//!
//! The analysis forgets direction and numeric weights, deduplicates reciprocal
//! topology, and ignores self-loops. It computes the second-smallest eigenpair
//! of the combinatorial Laplacian with a bounded, deflated power iteration over
//! sparse CSR adjacency. Disconnected graphs are recognized exactly.

use std::collections::VecDeque;

use crate::NodeId;

use super::{AnalysisError, GraphProjection};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Hard ceiling on deflated power iterations.
pub const MAX_SPECTRAL_ITERATIONS: usize = 1_000;

/// Hard ceiling on deterministic linear starts for connected graphs.
pub const MAX_SPECTRAL_STARTS: usize = 8;

/// Controls deterministic Fiedler iteration.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SpectralOptions {
    /// Requested iterations, clamped to [`MAX_SPECTRAL_ITERATIONS`].
    pub max_iterations: usize,
    /// Nonnegative finite eigenvalue and vector convergence tolerance.
    pub tolerance: f64,
}

impl Default for SpectralOptions {
    fn default() -> Self {
        Self {
            max_iterations: 300,
            tolerance: 1.0e-9,
        }
    }
}

/// Algebraic connectivity, a canonical Fiedler vector, and its sign split.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Spectral {
    /// Stable identifiers in projection order.
    pub nodes: Vec<NodeId>,
    /// Second-smallest eigenvalue of the combinatorial Laplacian.
    pub algebraic_connectivity: f64,
    /// Unit Fiedler vector aligned one-for-one with [`Self::nodes`].
    ///
    /// Its first nonzero component is positive, removing the eigenvector's
    /// arbitrary global sign.
    pub fiedler: Vec<f64>,
    /// Sign bipartition aligned with [`Self::nodes`]; `true` means nonnegative.
    pub partition: Vec<bool>,
    /// Number of sparse power iterations completed.
    ///
    /// This is the iteration count of the selected start, not the sum across
    /// [`Self::starts`] independently bounded starts.
    pub iterations: usize,
    /// Number of deterministic cyclic linear starts evaluated.
    pub starts: usize,
    /// Whether the configured tolerance was reached.
    pub converged: bool,
    /// Euclidean norm of `L fiedler - algebraic_connectivity * fiedler`.
    pub residual_norm: f64,
}

/// Computes algebraic connectivity and a canonical Fiedler bipartition.
///
/// This total convenience wrapper returns `None` for fewer than two vertices
/// or for malformed input/options. Use [`try_algebraic_connectivity`] when the
/// rejection reason matters.
pub fn algebraic_connectivity(
    graph: &GraphProjection,
    options: &SpectralOptions,
) -> Option<Spectral> {
    try_algebraic_connectivity(graph, options).ok().flatten()
}

/// Checked form of [`algebraic_connectivity`].
///
/// Direction and weights are ignored. Reciprocal and parallel topology is
/// deduplicated and self-loops are inert. Graphs with fewer than two vertices
/// return `Ok(None)`. A disconnected graph returns the exact value zero and a
/// canonical component-indicator eigenvector without iteration.
pub fn try_algebraic_connectivity(
    graph: &GraphProjection,
    options: &SpectralOptions,
) -> Result<Option<Spectral>, AnalysisError> {
    validate_inputs(graph, options)?;
    if graph.node_count() < 2 {
        return Ok(None);
    }

    let topology = SimpleUndirected::from_projection(graph);
    let components = topology.components();
    if components.len() > 1 {
        return Ok(Some(disconnected_result(graph, &topology, &components[0])));
    }

    let starts = topology.node_count().min(MAX_SPECTRAL_STARTS);
    let mut outcome = iterate_fiedler(&topology, options, 0);
    for offset in 1..starts {
        let candidate = iterate_fiedler(&topology, options, offset);
        if compare_outcomes(&candidate, &outcome).is_lt() {
            outcome = candidate;
        }
    }
    let partition = outcome.fiedler.iter().map(|&value| value >= 0.0).collect();
    Ok(Some(Spectral {
        nodes: graph.nodes.clone(),
        algebraic_connectivity: outcome.eigenvalue,
        fiedler: outcome.fiedler,
        partition,
        iterations: outcome.iterations,
        starts,
        converged: outcome.converged,
        residual_norm: outcome.residual_norm,
    }))
}

#[derive(Debug)]
struct IterationOutcome {
    fiedler: Vec<f64>,
    eigenvalue: f64,
    iterations: usize,
    converged: bool,
    residual_norm: f64,
}

fn iterate_fiedler(
    topology: &SimpleUndirected,
    options: &SpectralOptions,
    offset: usize,
) -> IterationOutcome {
    let node_count = topology.node_count();
    let center = (node_count as f64 - 1.0) / 2.0;
    let mut fiedler: Vec<f64> = (0..node_count)
        .map(|node| (node + offset) % node_count)
        .map(|rank| rank as f64 - center)
        .collect();
    normalize_orthogonal(&mut fiedler);
    pin_sign(&mut fiedler);

    let maximum_degree = topology.degree.iter().copied().max().unwrap_or(0) as f64;
    let shift = 2.0 * maximum_degree + 1.0;
    let limit = options.max_iterations.min(MAX_SPECTRAL_ITERATIONS);
    let mut candidate = vec![0.0; node_count];
    let mut eigenvalue = topology.rayleigh(&fiedler);
    let mut iterations = 0;
    let mut converged = false;

    for iteration in 1..=limit {
        for (node, output) in candidate.iter_mut().enumerate() {
            let neighbor_sum = topology
                .neighbors(node)
                .iter()
                .map(|&neighbor| fiedler[neighbor as usize])
                .sum::<f64>();
            *output = (shift - topology.degree[node] as f64) * fiedler[node] + neighbor_sum;
        }
        normalize_orthogonal(&mut candidate);
        pin_sign(&mut candidate);
        let next_eigenvalue = topology.rayleigh(&candidate);
        let vector_delta = candidate
            .iter()
            .zip(&fiedler)
            .map(|(next, previous)| {
                let difference = next - previous;
                difference * difference
            })
            .sum::<f64>()
            .sqrt();

        std::mem::swap(&mut fiedler, &mut candidate);
        iterations = iteration;
        let eigenvalue_delta = (next_eigenvalue - eigenvalue).abs();
        eigenvalue = next_eigenvalue;
        if eigenvalue_delta <= options.tolerance && vector_delta <= options.tolerance {
            converged = true;
            break;
        }
    }

    IterationOutcome {
        residual_norm: topology.residual_norm(&fiedler, eigenvalue),
        fiedler,
        eigenvalue,
        iterations,
        converged,
    }
}

fn compare_outcomes(left: &IterationOutcome, right: &IterationOutcome) -> std::cmp::Ordering {
    left.eigenvalue
        .total_cmp(&right.eigenvalue)
        .then_with(|| left.residual_norm.total_cmp(&right.residual_norm))
        .then_with(|| {
            left.fiedler
                .iter()
                .zip(&right.fiedler)
                .map(|(left, right)| left.total_cmp(right))
                .find(|ordering| !ordering.is_eq())
                .unwrap_or(std::cmp::Ordering::Equal)
        })
}

fn validate_inputs(
    graph: &GraphProjection,
    options: &SpectralOptions,
) -> Result<(), AnalysisError> {
    if !graph.is_well_formed() {
        return Err(AnalysisError::InvalidInput {
            analysis: "algebraic_connectivity",
            input: "projection",
            reason: "must satisfy sorted-node and forward/reverse CSR invariants",
        });
    }
    if !options.tolerance.is_finite() || options.tolerance < 0.0 {
        return Err(AnalysisError::InvalidInput {
            analysis: "algebraic_connectivity",
            input: "tolerance",
            reason: "must be finite and nonnegative",
        });
    }
    Ok(())
}

#[derive(Debug)]
struct SimpleUndirected {
    offsets: Vec<u32>,
    targets: Vec<u32>,
    degree: Vec<usize>,
}

impl SimpleUndirected {
    fn from_projection(graph: &GraphProjection) -> Self {
        let undirected = graph.undirected();
        let mut offsets = Vec::with_capacity(graph.node_count() + 1);
        let mut targets = Vec::with_capacity(undirected.targets.len());
        let mut degree = Vec::with_capacity(graph.node_count());
        offsets.push(0);
        for node in 0..graph.node_count() {
            targets.extend(
                undirected
                    .neighbors(node as u32)
                    .iter()
                    .copied()
                    .filter(|&neighbor| neighbor as usize != node),
            );
            degree.push(targets.len() - offsets[node] as usize);
            offsets.push(
                u32::try_from(targets.len())
                    .expect("simple projection adjacency fits the u32 CSR domain"),
            );
        }
        Self {
            offsets,
            targets,
            degree,
        }
    }

    fn node_count(&self) -> usize {
        self.degree.len()
    }

    fn neighbors(&self, node: usize) -> &[u32] {
        &self.targets[self.offsets[node] as usize..self.offsets[node + 1] as usize]
    }

    fn components(&self) -> Vec<Vec<usize>> {
        let mut visited = vec![false; self.node_count()];
        let mut queue = VecDeque::new();
        let mut components = Vec::new();
        for start in 0..self.node_count() {
            if visited[start] {
                continue;
            }
            visited[start] = true;
            queue.push_back(start);
            let mut members = Vec::new();
            while let Some(node) = queue.pop_front() {
                members.push(node);
                for &neighbor in self.neighbors(node) {
                    let neighbor = neighbor as usize;
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

    fn rayleigh(&self, vector: &[f64]) -> f64 {
        let mut quadratic = 0.0;
        for source in 0..self.node_count() {
            for &target in self.neighbors(source) {
                let target = target as usize;
                if source < target {
                    let difference = vector[source] - vector[target];
                    quadratic += difference * difference;
                }
            }
        }
        quadratic
    }

    fn residual_norm(&self, vector: &[f64], eigenvalue: f64) -> f64 {
        (0..self.node_count())
            .map(|node| {
                let neighbor_sum = self
                    .neighbors(node)
                    .iter()
                    .map(|&neighbor| vector[neighbor as usize])
                    .sum::<f64>();
                let residual = self.degree[node] as f64 * vector[node]
                    - neighbor_sum
                    - eigenvalue * vector[node];
                residual * residual
            })
            .sum::<f64>()
            .sqrt()
    }
}

fn disconnected_result(
    graph: &GraphProjection,
    topology: &SimpleUndirected,
    first_component: &[usize],
) -> Spectral {
    let node_count = topology.node_count();
    let first_size = first_component.len();
    let other_size = node_count - first_size;
    let positive = (other_size as f64 / (first_size * node_count) as f64).sqrt();
    let negative = -(first_size as f64 / (other_size * node_count) as f64).sqrt();
    let mut in_first = vec![false; node_count];
    for &node in first_component {
        in_first[node] = true;
    }
    let fiedler: Vec<f64> = in_first
        .iter()
        .map(|&member| if member { positive } else { negative })
        .collect();
    let partition = fiedler.iter().map(|&value| value >= 0.0).collect();
    Spectral {
        nodes: graph.nodes.clone(),
        algebraic_connectivity: 0.0,
        residual_norm: topology.residual_norm(&fiedler, 0.0),
        fiedler,
        partition,
        iterations: 0,
        starts: 0,
        converged: true,
    }
}

fn normalize_orthogonal(vector: &mut [f64]) {
    let mean = vector.iter().sum::<f64>() / vector.len() as f64;
    for value in vector.iter_mut() {
        *value -= mean;
    }
    let norm = vector.iter().map(|value| value * value).sum::<f64>().sqrt();
    debug_assert!(norm.is_finite() && norm > 0.0);
    for value in vector {
        *value /= norm;
    }
}

fn pin_sign(vector: &mut [f64]) {
    if vector
        .iter()
        .copied()
        .find(|&value| value != 0.0)
        .is_some_and(f64::is_sign_negative)
    {
        for value in vector {
            *value = -*value;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeSet, VecDeque};

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

    fn path_graph(node_count: usize) -> GraphProjection {
        projection(
            node_count,
            (0..node_count.saturating_sub(1)).map(|node| (node, node + 1, 1.0)),
        )
    }

    fn cycle_graph(node_count: usize) -> GraphProjection {
        projection(
            node_count,
            (0..node_count).map(|node| (node, (node + 1) % node_count, 1.0)),
        )
    }

    fn assert_eigenpair(graph: &GraphProjection, result: &Spectral, tolerance: f64) {
        assert_eq!(result.nodes, graph.nodes);
        assert_eq!(result.fiedler.len(), graph.node_count());
        assert_eq!(result.partition.len(), graph.node_count());
        assert!(result
            .partition
            .iter()
            .zip(&result.fiedler)
            .all(|(&side, &value)| side == (value >= 0.0)));
        assert!(result
            .fiedler
            .iter()
            .copied()
            .find(|&value| value != 0.0)
            .is_some_and(|value| value > 0.0));
        assert!(result.fiedler.iter().sum::<f64>().abs() <= tolerance);
        assert!(
            (result
                .fiedler
                .iter()
                .map(|value| value * value)
                .sum::<f64>()
                - 1.0)
                .abs()
                <= tolerance
        );
        assert!(result.residual_norm <= tolerance);
    }

    #[test]
    fn complete_graphs_have_connectivity_equal_to_order() {
        for node_count in 2..=8 {
            let graph = complete_graph(node_count);
            let result = try_algebraic_connectivity(&graph, &SpectralOptions::default())
                .expect("valid complete graph")
                .expect("at least two nodes");
            assert!(result.converged);
            assert!((result.algebraic_connectivity - node_count as f64).abs() <= 1.0e-10);
            assert_eigenpair(&graph, &result, 1.0e-8);
        }
    }

    #[test]
    fn path_four_has_closed_form_and_middle_sign_cut() {
        let graph = path_graph(4);
        let result = try_algebraic_connectivity(&graph, &SpectralOptions::default())
            .expect("valid path")
            .expect("four nodes");
        let expected = 2.0 - 2.0 * (std::f64::consts::PI / 4.0).cos();
        assert!(result.converged);
        assert!((result.algebraic_connectivity - expected).abs() <= 1.0e-8);
        assert_eq!(result.partition[0], result.partition[1]);
        assert_eq!(result.partition[2], result.partition[3]);
        assert_ne!(result.partition[0], result.partition[3]);
        assert_eigenpair(&graph, &result, 1.0e-7);
    }

    #[test]
    fn cycles_match_the_laplacian_closed_form() {
        for node_count in 3..=12 {
            let graph = cycle_graph(node_count);
            let result = try_algebraic_connectivity(
                &graph,
                &SpectralOptions {
                    max_iterations: MAX_SPECTRAL_ITERATIONS,
                    tolerance: 1.0e-11,
                },
            )
            .expect("valid cycle")
            .expect("at least three nodes");
            let expected = 2.0 * (1.0 - (2.0 * std::f64::consts::PI / node_count as f64).cos());
            assert!(result.converged, "cycle order {node_count}");
            assert!((result.algebraic_connectivity - expected).abs() <= 1.0e-8);
            assert_eigenpair(&graph, &result, 1.0e-6);
        }
    }

    #[test]
    fn disconnected_graph_is_exact_and_component_canonical() {
        let graph = projection(6, [(0, 1, 1.0), (1, 2, 1.0), (3, 4, 1.0)]);
        let result = try_algebraic_connectivity(&graph, &SpectralOptions::default())
            .expect("valid disconnected graph")
            .expect("six nodes");
        assert_eq!(result.algebraic_connectivity, 0.0);
        assert_eq!(result.iterations, 0);
        assert_eq!(result.starts, 0);
        assert!(result.converged);
        assert_eq!(
            result.partition,
            vec![true, true, true, false, false, false]
        );
        assert_eigenpair(&graph, &result, 1.0e-12);
    }

    #[test]
    fn cyclic_linear_starts_escape_a_higher_eigenvector_symmetry() {
        let graph = projection(
            5,
            [
                (0, 2, 1.0),
                (2, 3, 1.0),
                (2, 4, 1.0),
                (1, 3, 1.0),
                (0, 4, 1.0),
                (1, 2, 1.0),
            ],
        );
        let topology = SimpleUndirected::from_projection(&graph);
        let primary = iterate_fiedler(&topology, &SpectralOptions::default(), 0);
        assert!((primary.eigenvalue - 3.0).abs() <= 1.0e-12);

        let result = try_algebraic_connectivity(&graph, &SpectralOptions::default())
            .expect("valid symmetric graph")
            .expect("five nodes");
        assert_eq!(result.starts, 5);
        assert!(result.converged);
        assert!((result.algebraic_connectivity - 1.0).abs() <= 1.0e-10);
        assert_eigenpair(&graph, &result, 1.0e-8);
    }

    #[test]
    fn topology_ignores_direction_duplicates_loops_and_numeric_weights() {
        let graph = projection(
            4,
            [
                (0, 1, -7.0),
                (1, 0, f64::NAN),
                (0, 1, 9.0),
                (1, 2, f64::INFINITY),
                (2, 3, 1.0),
                (2, 2, f64::MAX),
            ],
        );
        let expected = path_graph(4);
        assert_eq!(
            algebraic_connectivity(&graph, &SpectralOptions::default()),
            algebraic_connectivity(&expected, &SpectralOptions::default())
        );
    }

    #[test]
    fn small_empty_malformed_and_invalid_options_are_explicit() {
        assert_eq!(
            try_algebraic_connectivity(&projection(0, []), &SpectralOptions::default())
                .expect("empty projection is valid"),
            None
        );
        assert_eq!(
            try_algebraic_connectivity(&projection(1, []), &SpectralOptions::default())
                .expect("singleton projection is valid"),
            None
        );

        let mut malformed = path_graph(3);
        malformed.rev_targets.clear();
        assert!(matches!(
            try_algebraic_connectivity(&malformed, &SpectralOptions::default()),
            Err(AnalysisError::InvalidInput {
                input: "projection",
                ..
            })
        ));
        assert_eq!(
            algebraic_connectivity(&malformed, &SpectralOptions::default()),
            None
        );
        for tolerance in [f64::NAN, f64::INFINITY, -1.0] {
            assert!(matches!(
                try_algebraic_connectivity(
                    &path_graph(3),
                    &SpectralOptions {
                        max_iterations: 10,
                        tolerance,
                    },
                ),
                Err(AnalysisError::InvalidInput {
                    input: "tolerance",
                    ..
                })
            ));
        }
    }

    #[test]
    fn iteration_requests_are_clamped_and_zero_is_observable() {
        let graph = path_graph(5);
        let zero = try_algebraic_connectivity(
            &graph,
            &SpectralOptions {
                max_iterations: 0,
                tolerance: 0.0,
            },
        )
        .expect("valid zero-iteration request")
        .expect("five nodes");
        assert_eq!(zero.iterations, 0);
        assert_eq!(zero.starts, 5);
        assert!(!zero.converged);

        let capped = try_algebraic_connectivity(
            &graph,
            &SpectralOptions {
                max_iterations: usize::MAX,
                tolerance: 0.0,
            },
        )
        .expect("valid clamped request")
        .expect("five nodes");
        let exact_cap = try_algebraic_connectivity(
            &graph,
            &SpectralOptions {
                max_iterations: MAX_SPECTRAL_ITERATIONS,
                tolerance: 0.0,
            },
        )
        .expect("valid exact-cap request")
        .expect("five nodes");
        assert_eq!(capped, exact_cap);
        assert!(capped.iterations <= MAX_SPECTRAL_ITERATIONS);
    }

    #[test]
    fn sparse_iteration_is_stack_safe_on_a_deep_path() {
        let graph = path_graph(25_000);
        let result = try_algebraic_connectivity(
            &graph,
            &SpectralOptions {
                max_iterations: 1,
                tolerance: 0.0,
            },
        )
        .expect("deep path is structurally valid")
        .expect("deep path has nodes");
        assert_eq!(result.nodes.len(), 25_000);
        assert_eq!(result.fiedler.len(), 25_000);
        assert_eq!(result.iterations, 1);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn result_and_options_round_trip_with_stable_json() {
        let graph = path_graph(4);
        let options = SpectralOptions::default();
        let first = try_algebraic_connectivity(&graph, &options)
            .expect("valid path")
            .expect("path result");
        let second = try_algebraic_connectivity(&graph, &options)
            .expect("repeat path")
            .expect("repeat result");
        let bytes = serde_json::to_vec(&first).expect("serialize spectral result");
        assert_eq!(bytes, serde_json::to_vec(&second).expect("repeat bytes"));
        assert_eq!(
            serde_json::from_slice::<Spectral>(&bytes).expect("deserialize spectral result"),
            first
        );
        let option_bytes = serde_json::to_vec(&options).expect("serialize options");
        assert_eq!(
            serde_json::from_slice::<SpectralOptions>(&option_bytes).expect("deserialize options"),
            options
        );
    }

    fn expected_edges(node_count: usize, raw: &[(u8, u8)]) -> Vec<(usize, usize)> {
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
                (left as usize % node_count, right as usize % node_count, 1.0)
            }),
        )
    }

    fn jacobi_lambda2(node_count: usize, edges: &[(usize, usize)]) -> f64 {
        let mut matrix: Vec<Vec<f64>> = vec![vec![0.0; node_count]; node_count];
        for &(left, right) in edges {
            matrix[left][left] += 1.0;
            matrix[right][right] += 1.0;
            matrix[left][right] -= 1.0;
            matrix[right][left] -= 1.0;
        }
        for _ in 0..20_000 {
            let mut pivot = (0usize, 1usize);
            let mut largest = 0.0_f64;
            for (row, values) in matrix.iter().enumerate() {
                for (column, &value) in values.iter().enumerate().skip(row + 1) {
                    if value.abs() > largest {
                        largest = value.abs();
                        pivot = (row, column);
                    }
                }
            }
            if largest <= 1.0e-13 {
                break;
            }
            let (left, right) = pivot;
            let angle: f64 =
                0.5 * (2.0 * matrix[left][right]).atan2(matrix[right][right] - matrix[left][left]);
            let cosine = angle.cos();
            let sine = angle.sin();
            let left_diagonal = matrix[left][left];
            let right_diagonal = matrix[right][right];
            let cross = matrix[left][right];
            let rotations: Vec<_> = matrix
                .iter()
                .enumerate()
                .filter(|(node, _)| *node != left && *node != right)
                .map(|(node, row)| {
                    let rotated_left = cosine * row[left] - sine * row[right];
                    let rotated_right = sine * row[left] + cosine * row[right];
                    (node, rotated_left, rotated_right)
                })
                .collect();
            for (node, rotated_left, rotated_right) in rotations {
                matrix[node][left] = rotated_left;
                matrix[left][node] = rotated_left;
                matrix[node][right] = rotated_right;
                matrix[right][node] = rotated_right;
            }
            matrix[left][left] = cosine * cosine * left_diagonal - 2.0 * sine * cosine * cross
                + sine * sine * right_diagonal;
            matrix[right][right] = sine * sine * left_diagonal
                + 2.0 * sine * cosine * cross
                + cosine * cosine * right_diagonal;
            matrix[left][right] = 0.0;
            matrix[right][left] = 0.0;
        }
        let mut eigenvalues: Vec<f64> = (0..node_count).map(|node| matrix[node][node]).collect();
        eigenvalues.sort_by(f64::total_cmp);
        eigenvalues[1].max(0.0)
    }

    fn connected(node_count: usize, edges: &[(usize, usize)]) -> bool {
        let mut adjacency = vec![Vec::new(); node_count];
        for &(left, right) in edges {
            adjacency[left].push(right);
            adjacency[right].push(left);
        }
        let mut visited = vec![false; node_count];
        let mut queue = VecDeque::from([0usize]);
        visited[0] = true;
        while let Some(node) = queue.pop_front() {
            for &neighbor in &adjacency[node] {
                if !visited[neighbor] {
                    visited[neighbor] = true;
                    queue.push_back(neighbor);
                }
            }
        }
        visited.into_iter().all(|value| value)
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(96))]

        #[test]
        fn generated_spectra_match_dense_jacobi_and_ignore_insertion_order(
            node_count in 2usize..10,
            raw in prop::collection::vec((any::<u8>(), any::<u8>()), 0..64),
        ) {
            let graph = projection_from_raw(node_count, &raw);
            let edges = expected_edges(node_count, &raw);
            let options = SpectralOptions {
                max_iterations: MAX_SPECTRAL_ITERATIONS,
                tolerance: 1.0e-10,
            };
            let result = try_algebraic_connectivity(&graph, &options)
                .expect("generated projection is valid")
                .expect("at least two generated nodes");
            let expected = jacobi_lambda2(node_count, &edges);
            let is_connected = connected(node_count, &edges);
            prop_assert!((result.algebraic_connectivity - expected).abs() <= 1.0e-3,
                "expected {expected}, observed {}", result.algebraic_connectivity);
            prop_assert!(result.iterations <= MAX_SPECTRAL_ITERATIONS);
            prop_assert_eq!(result.starts, if is_connected { node_count.min(MAX_SPECTRAL_STARTS) } else { 0 });
            prop_assert!(result.residual_norm.is_finite());
            if result.converged {
                prop_assert!(result.residual_norm <= 1.0e-5);
            } else {
                prop_assert_eq!(result.iterations, MAX_SPECTRAL_ITERATIONS);
            }
            prop_assert!(result.fiedler.iter().sum::<f64>().abs() <= 1.0e-8);
            prop_assert!((result.fiedler.iter().map(|value| value * value).sum::<f64>() - 1.0).abs() <= 1.0e-8);
            prop_assert!(result.fiedler.iter().copied().find(|&value| value != 0.0).is_some_and(|value| value > 0.0));
            prop_assert_eq!(&result, &try_algebraic_connectivity(&graph, &options)
                .expect("repeat generated projection")
                .expect("repeat result"));

            let reversed = projection_from_raw(node_count, &raw.iter().rev().copied().collect::<Vec<_>>());
            prop_assert_eq!(&result, &try_algebraic_connectivity(&reversed, &options)
                .expect("reversed projection")
                .expect("reversed result"));

            if is_connected {
                prop_assert!(expected > 0.0);
            } else {
                prop_assert_eq!(result.algebraic_connectivity, 0.0);
                prop_assert_eq!(result.iterations, 0);
            }
        }
    }
}
