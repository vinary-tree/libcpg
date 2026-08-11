//! Design Structure Matrix visibility and core-periphery analysis.
//!
//! The transitive-closure visibility matrix is summarized without
//! materializing its quadratic storage: one forward breadth-first search per
//! vertex computes visibility fan-out, visibility fan-in, and propagation
//! cost. Core-periphery classification uses the two visibility medians and the
//! existing deterministic SCC engine.

use std::collections::VecDeque;

use super::{scc::projection_sccs, GraphProjection};
use crate::NodeId;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Maximum projection size accepted by [`analyze_dsm`].
///
/// DSM closure analysis is `O(n(n + m))` time for `n` vertices and `m` edges.
/// Inputs above this bound produce an explicitly capped result.
pub const MAX_DSM_NODES: usize = 50_000;

/// Visibility summary of a directed dependency graph.
///
/// Vector positions are dense identifiers in the input [`GraphProjection`].
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DsmAnalysis {
    /// Stable node universe corresponding one-to-one with both visibility vectors.
    pub nodes: Vec<NodeId>,
    /// Number of distinct vertices reachable from each vertex, excluding self.
    pub visibility_fan_out: Vec<usize>,
    /// Number of distinct vertices that reach each vertex, excluding self.
    pub visibility_fan_in: Vec<usize>,
    /// Density of reachable ordered pairs in the `n` by `n` visibility matrix.
    pub propagation_cost: f64,
    /// Number of vertices in the input projection.
    pub n: usize,
    /// Whether all visibility values were computed.
    pub complete: bool,
    /// Whether computation was refused because [`MAX_DSM_NODES`] was exceeded.
    pub capped: bool,
}

impl DsmAnalysis {
    fn unavailable(graph: &GraphProjection, capped: bool) -> Self {
        Self {
            nodes: graph.nodes.clone(),
            visibility_fan_out: Vec::new(),
            visibility_fan_in: Vec::new(),
            propagation_cost: 0.0,
            n: graph.node_count(),
            complete: false,
            capped,
        }
    }
}

/// Computes visibility fan-in, visibility fan-out, and propagation cost.
///
/// Self-reachability is excluded, including when a cycle leads back to the
/// source. The calculation performs one forward breadth-first search per
/// vertex and uses `O(n)` temporary storage. Inputs above
/// [`MAX_DSM_NODES`] return a result with `complete == false` and
/// `capped == true`; no partial vectors are reported.
pub fn analyze_dsm(graph: &GraphProjection) -> DsmAnalysis {
    analyze_dsm_with_cap(graph, MAX_DSM_NODES)
}

fn analyze_dsm_with_cap(graph: &GraphProjection, max_nodes: usize) -> DsmAnalysis {
    let n = graph.node_count();
    if n > max_nodes {
        return DsmAnalysis::unavailable(graph, true);
    }
    if !projection_is_well_formed(graph) {
        return DsmAnalysis::unavailable(graph, false);
    }

    let mut visibility_fan_out = vec![0usize; n];
    let mut visibility_fan_in = vec![0usize; n];
    if n == 0 {
        return DsmAnalysis {
            nodes: graph.nodes.clone(),
            visibility_fan_out,
            visibility_fan_in,
            propagation_cost: 0.0,
            n,
            complete: true,
            capped: false,
        };
    }

    let mut total_pairs = 0u64;
    let mut visited = vec![false; n];
    let mut queue = VecDeque::with_capacity(n);

    for start in 0..n {
        visited.fill(false);
        visited[start] = true;
        queue.clear();
        queue.push_back(start as u32);
        let mut reached = 0usize;

        while let Some(node) = queue.pop_front() {
            for &successor in graph.successors(node) {
                let successor = successor as usize;
                if !visited[successor] {
                    visited[successor] = true;
                    reached += 1;
                    visibility_fan_in[successor] += 1;
                    queue.push_back(successor as u32);
                }
            }
        }

        visibility_fan_out[start] = reached;
        total_pairs += reached as u64;
    }

    let propagation_cost = total_pairs as f64 / (n as f64 * n as f64);
    DsmAnalysis {
        nodes: graph.nodes.clone(),
        visibility_fan_out,
        visibility_fan_in,
        propagation_cost,
        n,
        complete: true,
        capped: false,
    }
}

/// Median-based visibility class for one projection vertex.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CorePeriphery {
    /// High visibility fan-in and high visibility fan-out.
    Core,
    /// High visibility fan-in and low visibility fan-out.
    Shared,
    /// Low visibility fan-in and high visibility fan-out.
    Control,
    /// Low visibility fan-in and low visibility fan-out.
    Peripheral,
}

impl CorePeriphery {
    /// Returns the stable lowercase label for this class.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Core => "core",
            Self::Shared => "shared",
            Self::Control => "control",
            Self::Peripheral => "peripheral",
        }
    }
}

/// Core-periphery classes and the largest multi-vertex cyclic group.
///
/// Class and cyclic-core positions are dense identifiers in the input
/// [`GraphProjection`].
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CorePeripheryResult {
    /// Stable node universe corresponding one-to-one with `classes`.
    pub nodes: Vec<NodeId>,
    /// Per-vertex class, aligned with the input projection's dense identifiers.
    pub classes: Vec<CorePeriphery>,
    /// Median visibility fan-in threshold used for the high/low split.
    pub vfi_threshold: f64,
    /// Median visibility fan-out threshold used for the high/low split.
    pub vfo_threshold: f64,
    /// Dense identifiers in the largest SCC containing at least two vertices.
    pub cyclic_core: Vec<usize>,
    /// Whether the classification and cyclic-core search both completed.
    pub complete: bool,
    /// Whether the result is unavailable because the DSM node cap was exceeded.
    pub capped: bool,
}

impl CorePeripheryResult {
    fn unavailable(graph: &GraphProjection, capped: bool) -> Self {
        Self {
            nodes: graph.nodes.clone(),
            classes: Vec::new(),
            vfi_threshold: 0.0,
            vfo_threshold: 0.0,
            cyclic_core: Vec::new(),
            complete: false,
            capped,
        }
    }

    /// Returns the class of a stable node, or `None` if absent or incomplete.
    pub fn class_of(&self, node: NodeId) -> Option<CorePeriphery> {
        let dense = self.nodes.binary_search(&node).ok()?;
        self.classes.get(dense).copied()
    }

    /// Iterates stable node identifiers in the largest multi-vertex cyclic core.
    pub fn cyclic_core_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.cyclic_core
            .iter()
            .filter_map(|&dense| self.nodes.get(dense).copied())
    }
}

/// Classifies vertices by visibility medians and finds the largest cyclic core.
///
/// A vertex is high on an axis when its visibility count is greater than or
/// equal to that axis's median. Ties between largest multi-vertex SCCs select
/// the component with the smallest stable [`crate::NodeId`]. If `dsm` is
/// incomplete or does not align exactly with `graph`, the function returns an
/// explicitly incomplete result instead of indexing malformed input.
pub fn classify_core_periphery(graph: &GraphProjection, dsm: &DsmAnalysis) -> CorePeripheryResult {
    let capped = dsm.capped || graph.node_count() > MAX_DSM_NODES;
    if !projection_is_well_formed(graph)
        || !dsm.complete
        || capped
        || !dsm_is_well_formed_for(graph, dsm)
    {
        return CorePeripheryResult::unavailable(graph, capped);
    }

    let vfi_threshold = median(&dsm.visibility_fan_in);
    let vfo_threshold = median(&dsm.visibility_fan_out);
    let classes = dsm
        .visibility_fan_in
        .iter()
        .zip(&dsm.visibility_fan_out)
        .map(|(&fan_in, &fan_out)| {
            match (
                fan_in as f64 >= vfi_threshold,
                fan_out as f64 >= vfo_threshold,
            ) {
                (true, true) => CorePeriphery::Core,
                (true, false) => CorePeriphery::Shared,
                (false, true) => CorePeriphery::Control,
                (false, false) => CorePeriphery::Peripheral,
            }
        })
        .collect();

    let decomposition = projection_sccs(graph);
    let cyclic_core = decomposition
        .components
        .iter()
        .filter(|component| component.nodes.len() >= 2)
        .max_by(|left, right| {
            left.nodes
                .len()
                .cmp(&right.nodes.len())
                .then_with(|| right.nodes[0].cmp(&left.nodes[0]))
        })
        .map(|component| {
            component
                .nodes
                .iter()
                .map(|&node| {
                    graph
                        .dense_index(node)
                        .expect("SCC members originate from the analyzed projection")
                        as usize
                })
                .collect()
        })
        .unwrap_or_default();

    CorePeripheryResult {
        nodes: graph.nodes.clone(),
        classes,
        vfi_threshold,
        vfo_threshold,
        cyclic_core,
        complete: true,
        capped: false,
    }
}

fn projection_is_well_formed(graph: &GraphProjection) -> bool {
    let n = graph.node_count();
    if graph.nodes.windows(2).any(|pair| pair[0] >= pair[1])
        || !csr_is_well_formed(&graph.offsets, &graph.targets, n)
        || !csr_is_well_formed(&graph.rev_offsets, &graph.rev_targets, n)
        || graph.targets.len() != graph.rev_targets.len()
        || graph
            .weights
            .as_ref()
            .is_some_and(|weights| weights.len() != graph.targets.len())
    {
        return false;
    }

    for source in 0..n {
        let start = graph.offsets[source] as usize;
        let end = graph.offsets[source + 1] as usize;
        for &target in &graph.targets[start..end] {
            let target = target as usize;
            let reverse_start = graph.rev_offsets[target] as usize;
            let reverse_end = graph.rev_offsets[target + 1] as usize;
            if graph.rev_targets[reverse_start..reverse_end]
                .binary_search(&(source as u32))
                .is_err()
            {
                return false;
            }
        }
    }
    true
}

fn csr_is_well_formed(offsets: &[u32], targets: &[u32], node_count: usize) -> bool {
    if offsets.len() != node_count + 1
        || offsets.first() != Some(&0)
        || offsets.last().copied().map(|end| end as usize) != Some(targets.len())
        || offsets.windows(2).any(|pair| pair[0] > pair[1])
        || targets.iter().any(|&target| target as usize >= node_count)
    {
        return false;
    }

    offsets.windows(2).all(|range| {
        targets[range[0] as usize..range[1] as usize]
            .windows(2)
            .all(|pair| pair[0] < pair[1])
    })
}

fn dsm_is_well_formed_for(graph: &GraphProjection, dsm: &DsmAnalysis) -> bool {
    let n = graph.node_count();
    if dsm.n != n
        || dsm.nodes != graph.nodes
        || dsm.visibility_fan_in.len() != n
        || dsm.visibility_fan_out.len() != n
        || !dsm.propagation_cost.is_finite()
    {
        return false;
    }

    let largest_visibility = n.saturating_sub(1);
    if dsm
        .visibility_fan_in
        .iter()
        .chain(&dsm.visibility_fan_out)
        .any(|&count| count > largest_visibility)
    {
        return false;
    }

    let total_in: u128 = dsm
        .visibility_fan_in
        .iter()
        .map(|&count| count as u128)
        .sum();
    let total_out: u128 = dsm
        .visibility_fan_out
        .iter()
        .map(|&count| count as u128)
        .sum();
    if total_in != total_out {
        return false;
    }

    let expected_cost = if n == 0 {
        0.0
    } else {
        total_out as f64 / (n as f64 * n as f64)
    };
    (dsm.propagation_cost - expected_cost).abs() <= f64::EPSILON * expected_cost.abs().max(1.0)
}

fn median(values: &[usize]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let midpoint = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[midpoint - 1] as f64 + sorted[midpoint] as f64) / 2.0
    } else {
        sorted[midpoint] as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NodeId, ProjectionKind};
    use proptest::prelude::*;

    fn graph_from(n: usize, edges: &[(usize, usize)]) -> GraphProjection {
        GraphProjection::from_edges(
            ProjectionKind::External,
            (0..n as u32).map(NodeId::new),
            edges.iter().map(|&(source, target)| {
                (NodeId::new(source as u32), NodeId::new(target as u32), 1.0)
            }),
        )
    }

    #[test]
    fn linear_chain_propagation_and_visibility() {
        let graph = graph_from(4, &[(0, 1), (1, 2), (2, 3)]);
        let dsm = analyze_dsm(&graph);
        assert_eq!(dsm.visibility_fan_out, vec![3, 2, 1, 0]);
        assert_eq!(dsm.visibility_fan_in, vec![0, 1, 2, 3]);
        assert!((dsm.propagation_cost - 0.375).abs() < 1e-9);
        assert!(dsm.complete);
        assert!(!dsm.capped);
    }

    #[test]
    fn disconnected_graph_has_zero_propagation() {
        let graph = graph_from(3, &[]);
        let dsm = analyze_dsm(&graph);
        assert_eq!(dsm.propagation_cost, 0.0);
        assert_eq!(dsm.visibility_fan_out, vec![0, 0, 0]);
    }

    #[test]
    fn fully_connected_cycle_has_max_propagation() {
        let graph = graph_from(3, &[(0, 1), (1, 2), (2, 0)]);
        let dsm = analyze_dsm(&graph);
        assert_eq!(dsm.visibility_fan_out, vec![2, 2, 2]);
        assert_eq!(dsm.visibility_fan_in, vec![2, 2, 2]);
        assert!((dsm.propagation_cost - (6.0 / 9.0)).abs() < 1e-9);

        let classification = classify_core_periphery(&graph, &dsm);
        assert_eq!(classification.cyclic_core, vec![0, 1, 2]);
        assert!(classification
            .classes
            .iter()
            .all(|class| *class == CorePeriphery::Core));
    }

    #[test]
    fn classifies_shared_and_control_roles() {
        let graph = graph_from(5, &[(0, 2), (1, 2), (2, 3), (2, 4)]);
        let dsm = analyze_dsm(&graph);
        let classification = classify_core_periphery(&graph, &dsm);
        assert!(classification.cyclic_core.is_empty());
        assert_eq!(classification.classes[0], CorePeriphery::Control);
        assert_eq!(classification.classes[1], CorePeriphery::Control);
        assert_eq!(classification.classes[2], CorePeriphery::Core);
        assert_eq!(classification.classes[3], CorePeriphery::Shared);
        assert_eq!(classification.classes[4], CorePeriphery::Shared);
    }

    #[test]
    fn empty_graph_is_complete_and_uses_zero_thresholds() {
        let graph = graph_from(0, &[]);
        let dsm = analyze_dsm(&graph);
        let classification = classify_core_periphery(&graph, &dsm);
        assert_eq!(dsm.n, 0);
        assert_eq!(dsm.propagation_cost, 0.0);
        assert!(dsm.complete);
        assert!(classification.complete);
        assert_eq!(classification.vfi_threshold, 0.0);
        assert_eq!(classification.vfo_threshold, 0.0);
        assert!(classification.classes.is_empty());
    }

    #[test]
    fn self_loop_does_not_add_self_visibility_or_form_multi_vertex_core() {
        let graph = graph_from(1, &[(0, 0)]);
        let dsm = analyze_dsm(&graph);
        let classification = classify_core_periphery(&graph, &dsm);
        assert_eq!(dsm.visibility_fan_out, vec![0]);
        assert_eq!(dsm.visibility_fan_in, vec![0]);
        assert_eq!(dsm.propagation_cost, 0.0);
        assert!(classification.cyclic_core.is_empty());
    }

    #[test]
    fn equal_sized_cyclic_cores_tie_break_by_smallest_stable_node() {
        let graph = GraphProjection::from_edges(
            ProjectionKind::External,
            [
                NodeId::new(50),
                NodeId::new(60),
                NodeId::new(10),
                NodeId::new(20),
                NodeId::new(90),
            ],
            [
                (NodeId::new(50), NodeId::new(60), 1.0),
                (NodeId::new(60), NodeId::new(50), 1.0),
                (NodeId::new(10), NodeId::new(20), 1.0),
                (NodeId::new(20), NodeId::new(10), 1.0),
            ],
        );
        let dsm = analyze_dsm(&graph);
        let classification = classify_core_periphery(&graph, &dsm);
        assert_eq!(
            graph.nodes,
            vec![
                NodeId::new(10),
                NodeId::new(20),
                NodeId::new(50),
                NodeId::new(60),
                NodeId::new(90),
            ]
        );
        assert_eq!(classification.cyclic_core, vec![0, 1]);
        assert_eq!(classification.classes[4], CorePeriphery::Peripheral);
    }

    #[test]
    fn cap_and_misalignment_return_explicitly_incomplete_results() {
        let graph = graph_from(3, &[(0, 1)]);
        let capped = analyze_dsm_with_cap(&graph, 2);
        assert_eq!(capped.n, 3);
        assert!(!capped.complete);
        assert!(capped.capped);
        assert!(capped.visibility_fan_in.is_empty());
        assert!(classify_core_periphery(&graph, &capped).capped);

        let at_cap = analyze_dsm_with_cap(&graph, 3);
        assert!(at_cap.complete);
        assert!(!at_cap.capped);

        let mut malformed = analyze_dsm(&graph);
        malformed.visibility_fan_out.pop();
        let unavailable = classify_core_periphery(&graph, &malformed);
        assert!(!unavailable.complete);
        assert!(!unavailable.capped);
        assert!(unavailable.classes.is_empty());

        let same_size_different_nodes = GraphProjection::from_edges(
            ProjectionKind::External,
            [NodeId::new(10), NodeId::new(20), NodeId::new(30)],
            [],
        );
        let valid = analyze_dsm(&graph);
        let unavailable = classify_core_periphery(&same_size_different_nodes, &valid);
        assert!(!unavailable.complete);
        assert!(unavailable.classes.is_empty());

        let mut impossible_counts = analyze_dsm(&graph);
        impossible_counts.visibility_fan_in[0] = usize::MAX;
        impossible_counts.visibility_fan_out[0] = usize::MAX;
        let unavailable = classify_core_periphery(&graph, &impossible_counts);
        assert!(!unavailable.complete);
        assert!(unavailable.classes.is_empty());

        let mut inconsistent_totals = analyze_dsm(&graph);
        inconsistent_totals.visibility_fan_in[0] = 1;
        let unavailable = classify_core_periphery(&graph, &inconsistent_totals);
        assert!(!unavailable.complete);

        let mut non_finite_cost = analyze_dsm(&graph);
        non_finite_cost.propagation_cost = f64::NAN;
        let unavailable = classify_core_periphery(&graph, &non_finite_cost);
        assert!(!unavailable.complete);
    }

    #[test]
    fn malformed_public_projection_is_rejected_without_traversal() {
        let mut malformed = graph_from(2, &[(0, 1)]);
        malformed.offsets = vec![0, 2, 1];
        let dsm = analyze_dsm(&malformed);
        assert!(!dsm.complete);
        assert!(!dsm.capped);
        assert!(dsm.visibility_fan_in.is_empty());
        assert!(!classify_core_periphery(&malformed, &dsm).complete);

        let mut inconsistent_reverse = graph_from(2, &[(0, 1)]);
        inconsistent_reverse.rev_targets[0] = 1;
        assert!(!analyze_dsm(&inconsistent_reverse).complete);
    }

    #[test]
    fn public_node_cap_refuses_oversized_projection_without_partial_output() {
        let graph = graph_from(MAX_DSM_NODES + 1, &[]);
        let dsm = analyze_dsm(&graph);
        assert_eq!(dsm.n, MAX_DSM_NODES + 1);
        assert!(!dsm.complete);
        assert!(dsm.capped);
        assert!(dsm.visibility_fan_in.is_empty());
        assert!(dsm.visibility_fan_out.is_empty());
        assert_eq!(dsm.propagation_cost, 0.0);
    }

    #[test]
    fn edges_with_endpoints_outside_the_projection_are_inert() {
        let graph = GraphProjection::from_edges(
            ProjectionKind::External,
            [NodeId::new(10), NodeId::new(20)],
            [
                (NodeId::new(10), NodeId::new(20), 1.0),
                (NodeId::new(10), NodeId::new(999), 1.0),
                (NodeId::new(999), NodeId::new(20), 1.0),
            ],
        );
        let dsm = analyze_dsm(&graph);
        assert_eq!(dsm.visibility_fan_out, vec![1, 0]);
        assert_eq!(dsm.visibility_fan_in, vec![0, 1]);
        assert_eq!(dsm.propagation_cost, 0.25);
    }

    #[test]
    fn labels_are_stable_and_complete() {
        assert_eq!(CorePeriphery::Core.as_str(), "core");
        assert_eq!(CorePeriphery::Shared.as_str(), "shared");
        assert_eq!(CorePeriphery::Control.as_str(), "control");
        assert_eq!(CorePeriphery::Peripheral.as_str(), "peripheral");

        let graph = graph_from(2, &[(0, 1)]);
        let dsm = analyze_dsm(&graph);
        let classification = classify_core_periphery(&graph, &dsm);
        assert_eq!(
            classification.class_of(NodeId::new(0)),
            Some(CorePeriphery::Control)
        );
        assert_eq!(classification.class_of(NodeId::new(99)), None);
        assert!(classification.cyclic_core_nodes().next().is_none());
    }

    fn closure_oracle(n: usize, edges: &[(usize, usize)]) -> Vec<Vec<bool>> {
        let mut reachable = vec![vec![false; n]; n];
        for &(source, target) in edges {
            if source < n && target < n {
                reachable[source][target] = true;
            }
        }
        for intermediate in 0..n {
            for source in 0..n {
                for target in 0..n {
                    reachable[source][target] |=
                        reachable[source][intermediate] && reachable[intermediate][target];
                }
            }
        }
        for (node, row) in reachable.iter_mut().enumerate() {
            row[node] = false;
        }
        reachable
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn dsm_matches_transitive_closure_and_is_storage_order_invariant(
            node_count in 0usize..10,
            raw_edges in prop::collection::vec((0usize..10, 0usize..10), 0..80),
        ) {
            let edges: Vec<_> = raw_edges
                .into_iter()
                .filter(|&(source, target)| source < node_count && target < node_count)
                .collect();
            let graph = graph_from(node_count, &edges);
            let mut repeated_reversed = edges.clone();
            repeated_reversed.extend(edges.iter().rev().copied());
            repeated_reversed.reverse();
            let reordered = graph_from(node_count, &repeated_reversed);

            let actual = analyze_dsm(&graph);
            let reordered_actual = analyze_dsm(&reordered);
            let closure = closure_oracle(node_count, &edges);
            let expected_out: Vec<usize> = closure
                .iter()
                .map(|row| row.iter().filter(|&&reachable| reachable).count())
                .collect();
            let expected_in: Vec<usize> = (0..node_count)
                .map(|target| closure.iter().filter(|row| row[target]).count())
                .collect();
            let pairs: usize = expected_out.iter().sum();
            let expected_cost = if node_count == 0 {
                0.0
            } else {
                pairs as f64 / (node_count * node_count) as f64
            };

            prop_assert_eq!(&actual, &reordered_actual);
            prop_assert_eq!(&actual.visibility_fan_out, &expected_out);
            prop_assert_eq!(&actual.visibility_fan_in, &expected_in);
            prop_assert_eq!(
                actual.visibility_fan_out.iter().sum::<usize>(),
                actual.visibility_fan_in.iter().sum::<usize>(),
            );
            prop_assert!((actual.propagation_cost - expected_cost).abs() <= f64::EPSILON);

            let classes = classify_core_periphery(&graph, &actual);
            let reordered_classes = classify_core_periphery(&reordered, &reordered_actual);
            prop_assert_eq!(&classes, &reordered_classes);

            let mut assigned = vec![false; node_count];
            let mut expected_core = Vec::new();
            for first in 0..node_count {
                if assigned[first] {
                    continue;
                }
                let component: Vec<usize> = (0..node_count)
                    .filter(|&node| {
                        node == first || (closure[first][node] && closure[node][first])
                    })
                    .collect();
                for &node in &component {
                    assigned[node] = true;
                }
                if component.len() >= 2 && component.len() > expected_core.len() {
                    expected_core = component;
                }
            }
            prop_assert_eq!(classes.cyclic_core, expected_core);
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn results_round_trip_through_json() {
        let graph = graph_from(4, &[(0, 1), (1, 2), (2, 0), (2, 3)]);
        let dsm = analyze_dsm(&graph);
        let classification = classify_core_periphery(&graph, &dsm);

        let encoded = serde_json::to_string(&dsm).expect("serialize DSM result");
        let decoded: DsmAnalysis = serde_json::from_str(&encoded).expect("deserialize DSM result");
        assert_eq!(decoded, dsm);

        let encoded = serde_json::to_string(&classification).expect("serialize classes");
        let decoded: CorePeripheryResult =
            serde_json::from_str(&encoded).expect("deserialize classes");
        assert_eq!(decoded, classification);
    }
}
