//! Deterministic analytics over an SCC condensation [`super::GraphProjection`].
//!
//! The input is an [`SccDecomposition`]; its public condensation projection is
//! the canonical payload-free CSR substrate, and vertices are strongly
//! connected components (SCCs) rather than source-program nodes. Longest-path
//! levels grow from sources toward sinks. [`CgLayering`] uses the opposite,
//! scheduling-oriented convention: layer zero contains sinks and every
//! precedence edge points from a strictly higher layer to a lower layer.

use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap};

use super::{AnalysisError, SccDecomposition};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Hard upper bound for condensation-DAG analyses.
///
/// A caller may select a smaller bound through [`CondensationOptions`], but
/// cannot raise the public algorithms above one million components.
pub const MAX_CONDENSATION_COMPONENTS: usize = 1_000_000;

const ANALYSIS_NAME: &str = "condensation_metrics";
const COFFMAN_GRAHAM_NAME: &str = "coffman_graham";

/// Options for [`condensation_metrics`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CondensationOptions {
    /// Maximum number of components to analyze.
    ///
    /// Values above [`MAX_CONDENSATION_COMPONENTS`] are clamped to that hard
    /// ceiling.  A lower value lets latency-sensitive callers impose a tighter
    /// budget.  Exceeding the effective cap produces an incomplete, capped
    /// report without traversing the DAG.
    pub max_components: usize,
    /// Optional Coffman–Graham width limit.
    ///
    /// `None` skips the optional layering.  `Some(0)` is invalid and leaves
    /// [`CondensationMetrics::complete`] false; call [`coffman_graham`]
    /// directly when the diagnostic [`AnalysisError`] is required.
    pub coffman_graham_width: Option<u32>,
}

impl Default for CondensationOptions {
    fn default() -> Self {
        Self {
            max_components: MAX_CONDENSATION_COMPONENTS,
            coffman_graham_width: None,
        }
    }
}

/// A width-bounded Coffman–Graham layering of condensation components.
///
/// `layers[0]` is the sink layer.  For every condensation edge `u -> v`,
/// `layer_of[u] > layer_of[v]`.  Component ids inside each layer are sorted.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CgLayering {
    /// Requested maximum number of components in any layer.
    pub width_limit: u32,
    /// Sink-first layers containing sorted SCC component ids.
    pub layers: Vec<Vec<usize>>,
    /// Sink-first layer index for every component id.
    pub layer_of: Vec<u32>,
}

impl CgLayering {
    /// Number of layers in the schedule.
    pub fn height(&self) -> u32 {
        u32::try_from(self.layers.len())
            .expect("the component cap keeps the layer count inside u32")
    }

    /// Largest number of components assigned to one layer.
    pub fn width(&self) -> u32 {
        self.layers.iter().map(Vec::len).max().map_or(0, |width| {
            u32::try_from(width).expect("the component cap keeps the layer width inside u32")
        })
    }
}

/// Longest-path and optional scheduling metrics for a condensation DAG.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CondensationMetrics {
    /// Source-oriented longest-path layer for every component id.
    ///
    /// Sources are at level zero.  Each value is the maximum number of edges
    /// on a path from any source to that component.
    pub levels: Vec<u32>,
    /// Deterministic maximum-weight source-to-sink path of component ids.
    pub critical_path: Vec<usize>,
    /// Sum of SCC member counts along [`Self::critical_path`].
    pub critical_path_weight: u64,
    /// Number of source-oriented longest-path levels.
    pub height: u32,
    /// Maximum number of components sharing one source-oriented level.
    pub width: u32,
    /// Optional width-bounded Coffman–Graham layering.
    pub coffman_graham: Option<CgLayering>,
    /// Whether every requested metric was computed from a valid condensation.
    ///
    /// This is false for a malformed decomposition, an invalid optional width,
    /// or a resource-cap breach.
    pub complete: bool,
    /// Whether computation stopped because the effective component cap was
    /// exceeded.
    pub capped: bool,
}

impl CondensationMetrics {
    fn unavailable(capped: bool) -> Self {
        Self {
            levels: Vec::new(),
            critical_path: Vec::new(),
            critical_path_weight: 0,
            height: 0,
            width: 0,
            coffman_graham: None,
            complete: false,
            capped,
        }
    }
}

/// Computes deterministic height, width, and weighted-critical-path metrics.
///
/// The canonical topological order is Kahn's algorithm with the smallest
/// available component id selected first.  A critical-path tie selects the
/// smallest immediate predecessor, then the smallest terminal component.
/// Passing `coffman_graham_width` additionally computes the scheduling layers.
/// The function is total: malformed public struct values and budget breaches
/// return an incomplete report instead of panicking.
pub fn condensation_metrics(
    decomposition: &SccDecomposition,
    options: &CondensationOptions,
) -> CondensationMetrics {
    let effective_cap = options.max_components.min(MAX_CONDENSATION_COMPONENTS);
    let dag = match CondensationDag::from_decomposition(decomposition, effective_cap, ANALYSIS_NAME)
    {
        Ok(dag) => dag,
        Err(AnalysisError::BudgetExceeded { .. }) => return CondensationMetrics::unavailable(true),
        Err(_) => return CondensationMetrics::unavailable(false),
    };
    let topological_order = match dag.canonical_topological_order(ANALYSIS_NAME) {
        Ok(order) => order,
        Err(_) => return CondensationMetrics::unavailable(false),
    };

    let mut metrics = longest_path_metrics(&dag, &topological_order);
    if let Some(width) = options.coffman_graham_width {
        match coffman_graham_from_dag(&dag, width) {
            Ok(layering) => metrics.coffman_graham = Some(layering),
            Err(_) => metrics.complete = false,
        }
    }
    metrics
}

/// Computes a deterministic Coffman–Graham layering with width `width`.
///
/// The algorithm first labels source-ready components.  Eligible components
/// are ordered by their immediate-predecessor labels sorted descending and
/// compared lexicographically; exact ties select the smaller component id.
/// Components are then visited in reverse label order and placed in the lowest
/// non-full layer strictly above every successor.
///
/// # Errors
///
/// Returns [`AnalysisError::InvalidInput`] when `width` is zero or the supplied
/// public decomposition is malformed/cyclic, and
/// [`AnalysisError::BudgetExceeded`] above
/// [`MAX_CONDENSATION_COMPONENTS`].
pub fn coffman_graham(
    decomposition: &SccDecomposition,
    width: u32,
) -> Result<CgLayering, AnalysisError> {
    let dag = CondensationDag::from_decomposition(
        decomposition,
        MAX_CONDENSATION_COMPONENTS,
        COFFMAN_GRAHAM_NAME,
    )?;
    coffman_graham_from_dag(&dag, width)
}

#[derive(Debug)]
struct CondensationDag {
    successors: Vec<Vec<usize>>,
    predecessors: Vec<Vec<usize>>,
    component_weights: Vec<u64>,
}

impl CondensationDag {
    fn from_decomposition(
        decomposition: &SccDecomposition,
        component_cap: usize,
        analysis: &'static str,
    ) -> Result<Self, AnalysisError> {
        let component_count = decomposition.components.len();
        if component_count > component_cap {
            return Err(AnalysisError::BudgetExceeded {
                analysis,
                unit: "components",
                needed: usize_to_u64(component_count),
                cap: usize_to_u64(component_cap),
            });
        }

        let mut component_weights = Vec::with_capacity(component_count);
        let mut member_count = 0usize;
        for (expected_id, component) in decomposition.components.iter().enumerate() {
            if component.id != expected_id {
                return Err(invalid_decomposition(
                    analysis,
                    "component ids must equal their positions",
                ));
            }
            if component.nodes.is_empty() {
                return Err(invalid_decomposition(
                    analysis,
                    "every component must contain at least one node",
                ));
            }
            for node in &component.nodes {
                if decomposition.component_by_node.get(node) != Some(&expected_id) {
                    return Err(invalid_decomposition(
                        analysis,
                        "component membership and component_by_node must agree",
                    ));
                }
            }
            member_count = member_count
                .checked_add(component.nodes.len())
                .ok_or_else(|| {
                    invalid_decomposition(analysis, "component membership count overflowed usize")
                })?;
            component_weights.push(usize_to_u64(component.nodes.len()));
        }
        if member_count != decomposition.component_by_node.len() {
            return Err(invalid_decomposition(
                analysis,
                "component membership and component_by_node must have the same domain",
            ));
        }

        for edge in &decomposition.condensation_edges {
            if edge.source >= component_count || edge.target >= component_count {
                return Err(invalid_decomposition(
                    analysis,
                    "an edge endpoint is outside the component domain",
                ));
            }
            if edge.source == edge.target {
                return Err(invalid_decomposition(
                    analysis,
                    "a condensation edge cannot be a self-loop",
                ));
            }
        }

        let projection = decomposition.condensation_projection();
        if projection.node_count() != component_count {
            return Err(invalid_decomposition(
                analysis,
                "every component must have one distinct projection representative",
            ));
        }
        let dense_to_component: Vec<usize> = projection
            .nodes
            .iter()
            .map(|representative| decomposition.component_by_node[representative])
            .collect();
        let mut successors = vec![Vec::new(); component_count];
        let mut predecessors = vec![Vec::new(); component_count];
        for (dense, &component) in dense_to_component.iter().enumerate() {
            successors[component].extend(
                projection
                    .successors(dense as u32)
                    .iter()
                    .map(|&successor| dense_to_component[successor as usize]),
            );
            predecessors[component].extend(
                projection
                    .predecessors(dense as u32)
                    .iter()
                    .map(|&predecessor| dense_to_component[predecessor as usize]),
            );
        }
        for adjacency in successors.iter_mut().chain(predecessors.iter_mut()) {
            adjacency.sort_unstable();
        }

        Ok(Self {
            successors,
            predecessors,
            component_weights,
        })
    }

    fn canonical_topological_order(
        &self,
        analysis: &'static str,
    ) -> Result<Vec<usize>, AnalysisError> {
        let mut remaining_predecessors: Vec<usize> =
            self.predecessors.iter().map(Vec::len).collect();
        let mut ready = BinaryHeap::new();
        for (component, &count) in remaining_predecessors.iter().enumerate() {
            if count == 0 {
                ready.push(Reverse(component));
            }
        }

        let mut order = Vec::with_capacity(self.successors.len());
        while let Some(Reverse(component)) = ready.pop() {
            order.push(component);
            for &successor in &self.successors[component] {
                remaining_predecessors[successor] -= 1;
                if remaining_predecessors[successor] == 0 {
                    ready.push(Reverse(successor));
                }
            }
        }
        if order.len() != self.successors.len() {
            return Err(invalid_decomposition(
                analysis,
                "the condensation edges contain a directed cycle",
            ));
        }
        Ok(order)
    }
}

fn longest_path_metrics(dag: &CondensationDag, topological_order: &[usize]) -> CondensationMetrics {
    let component_count = dag.successors.len();
    if component_count == 0 {
        return CondensationMetrics {
            levels: Vec::new(),
            critical_path: Vec::new(),
            critical_path_weight: 0,
            height: 0,
            width: 0,
            coffman_graham: None,
            complete: true,
            capped: false,
        };
    }

    let mut levels = vec![0u32; component_count];
    let mut path_weights = dag.component_weights.clone();
    let mut path_predecessor = vec![None; component_count];

    for &component in topological_order {
        for &successor in &dag.successors[component] {
            levels[successor] = levels[successor].max(levels[component] + 1);
            let candidate_weight = path_weights[component]
                .checked_add(dag.component_weights[successor])
                .expect("component membership cannot overflow u64");
            let replace = candidate_weight > path_weights[successor]
                || (candidate_weight == path_weights[successor]
                    && path_predecessor[successor].is_none_or(|prior| component < prior));
            if replace {
                path_weights[successor] = candidate_weight;
                path_predecessor[successor] = Some(component);
            }
        }
    }

    let mut terminal = 0usize;
    for component in 1..component_count {
        if path_weights[component] > path_weights[terminal]
            || (path_weights[component] == path_weights[terminal] && component < terminal)
        {
            terminal = component;
        }
    }
    let critical_path_weight = path_weights[terminal];
    let mut critical_path = Vec::new();
    let mut cursor = Some(terminal);
    while let Some(component) = cursor {
        critical_path.push(component);
        cursor = path_predecessor[component];
    }
    critical_path.reverse();

    let height = levels.iter().copied().max().map_or(0, |level| level + 1);
    let mut level_occupancy = vec![0u32; height as usize];
    for &level in &levels {
        level_occupancy[level as usize] += 1;
    }
    let width = level_occupancy.into_iter().max().unwrap_or(0);

    CondensationMetrics {
        levels,
        critical_path,
        critical_path_weight,
        height,
        width,
        coffman_graham: None,
        complete: true,
        capped: false,
    }
}

fn coffman_graham_from_dag(dag: &CondensationDag, width: u32) -> Result<CgLayering, AnalysisError> {
    if width == 0 {
        return Err(AnalysisError::InvalidInput {
            analysis: COFFMAN_GRAHAM_NAME,
            input: "width",
            reason: "must be at least one",
        });
    }

    let component_count = dag.successors.len();
    let mut remaining_predecessors: Vec<usize> = dag.predecessors.iter().map(Vec::len).collect();
    let mut labels = vec![0u32; component_count];
    let mut eligible: BinaryHeap<Reverse<(Vec<u32>, usize)>> = BinaryHeap::new();
    for (component, &count) in remaining_predecessors.iter().enumerate() {
        if count == 0 {
            eligible.push(Reverse((Vec::new(), component)));
        }
    }

    let mut label_order = Vec::with_capacity(component_count);
    while let Some(Reverse((_predecessor_labels, component))) = eligible.pop() {
        let label = u32::try_from(label_order.len() + 1)
            .expect("the component cap keeps Coffman-Graham labels inside u32");
        labels[component] = label;
        label_order.push(component);

        for &successor in &dag.successors[component] {
            remaining_predecessors[successor] -= 1;
            if remaining_predecessors[successor] == 0 {
                let mut predecessor_labels: Vec<u32> = dag.predecessors[successor]
                    .iter()
                    .map(|&predecessor| labels[predecessor])
                    .collect();
                predecessor_labels.sort_unstable_by(|left, right| right.cmp(left));
                eligible.push(Reverse((predecessor_labels, successor)));
            }
        }
    }
    if label_order.len() != component_count {
        return Err(invalid_decomposition(
            COFFMAN_GRAHAM_NAME,
            "the condensation edges contain a directed cycle",
        ));
    }

    let width_as_usize = width as usize;
    let mut layers: Vec<Vec<usize>> = Vec::new();
    let mut layer_of = vec![u32::MAX; component_count];
    let mut available_layers = BTreeSet::new();

    for &component in label_order.iter().rev() {
        let mut minimum_layer = 0usize;
        for &successor in &dag.successors[component] {
            let successor_layer = layer_of[successor];
            if successor_layer == u32::MAX {
                return Err(invalid_decomposition(
                    COFFMAN_GRAHAM_NAME,
                    "a successor was not labeled before its predecessor",
                ));
            }
            minimum_layer = minimum_layer.max(successor_layer as usize + 1);
        }

        let layer = match available_layers.range(minimum_layer..).next().copied() {
            Some(layer) => layer,
            None => {
                if minimum_layer > layers.len() {
                    return Err(invalid_decomposition(
                        COFFMAN_GRAHAM_NAME,
                        "layer construction encountered a non-contiguous level",
                    ));
                }
                layers.push(Vec::new());
                layers.len() - 1
            }
        };
        layers[layer].push(component);
        layer_of[component] =
            u32::try_from(layer).expect("the component cap keeps Coffman-Graham layers inside u32");
        if layers[layer].len() < width_as_usize {
            available_layers.insert(layer);
        } else {
            available_layers.remove(&layer);
        }
    }

    for layer in &mut layers {
        layer.sort_unstable();
    }
    Ok(CgLayering {
        width_limit: width,
        layers,
        layer_of,
    })
}

fn invalid_decomposition(analysis: &'static str, reason: &'static str) -> AnalysisError {
    AnalysisError::InvalidInput {
        analysis,
        input: "SccDecomposition",
        reason,
    }
}

fn usize_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use proptest::prelude::*;

    use super::*;
    use crate::{NodeId, SccComponent, SccCondensationEdge, SccProjection};

    fn synthetic_decomposition(
        component_sizes: &[usize],
        edges: &[(usize, usize)],
    ) -> SccDecomposition {
        let mut next_node = 0u32;
        let mut components = Vec::with_capacity(component_sizes.len());
        let mut component_by_node = BTreeMap::new();
        for (id, &size) in component_sizes.iter().enumerate() {
            let mut nodes = Vec::with_capacity(size);
            for _ in 0..size {
                let node = NodeId::new(next_node);
                next_node += 1;
                component_by_node.insert(node, id);
                nodes.push(node);
            }
            components.push(SccComponent {
                id,
                nodes,
                cyclic: size > 1,
            });
        }
        SccDecomposition {
            projection: SccProjection::CallGraph,
            components,
            component_by_node,
            condensation_edges: edges
                .iter()
                .map(|&(source, target)| SccCondensationEdge { source, target })
                .collect(),
        }
    }

    fn edge_set(decomposition: &SccDecomposition) -> BTreeSet<(usize, usize)> {
        decomposition
            .condensation_edges
            .iter()
            .map(|edge| (edge.source, edge.target))
            .collect()
    }

    fn assert_critical_path(decomposition: &SccDecomposition, metrics: &CondensationMetrics) {
        let edges = edge_set(decomposition);
        assert!(metrics
            .critical_path
            .windows(2)
            .all(|edge| edges.contains(&(edge[0], edge[1]))));
        let weight: u64 = metrics
            .critical_path
            .iter()
            .map(|&component| decomposition.components[component].nodes.len() as u64)
            .sum();
        assert_eq!(weight, metrics.critical_path_weight);
    }

    fn assert_layering(decomposition: &SccDecomposition, layering: &CgLayering) {
        assert_eq!(layering.layer_of.len(), decomposition.components.len());
        let mut found = Vec::new();
        for (layer_index, layer) in layering.layers.iter().enumerate() {
            assert!(layer.len() <= layering.width_limit as usize);
            let mut sorted = layer.clone();
            sorted.sort_unstable();
            assert_eq!(&sorted, layer);
            for &component in layer {
                assert_eq!(layering.layer_of[component], layer_index as u32);
                found.push(component);
            }
        }
        found.sort_unstable();
        assert_eq!(
            found,
            (0..decomposition.components.len()).collect::<Vec<_>>()
        );
        for edge in &decomposition.condensation_edges {
            assert!(layering.layer_of[edge.source] > layering.layer_of[edge.target]);
        }
    }

    fn brute_longest_path_weight(decomposition: &SccDecomposition) -> u64 {
        let component_count = decomposition.components.len();
        let mut successors = vec![Vec::new(); component_count];
        for edge in &decomposition.condensation_edges {
            if !successors[edge.source].contains(&edge.target) {
                successors[edge.source].push(edge.target);
            }
        }
        let weights: Vec<u64> = decomposition
            .components
            .iter()
            .map(|component| component.nodes.len() as u64)
            .collect();
        let mut best = 0u64;
        let mut stack = Vec::new();
        for start in 0..component_count {
            stack.push((start, weights[start]));
            while let Some((component, weight)) = stack.pop() {
                best = best.max(weight);
                for &successor in &successors[component] {
                    stack.push((successor, weight + weights[successor]));
                }
            }
        }
        best
    }

    fn optimal_unit_height(decomposition: &SccDecomposition, width: usize) -> usize {
        let component_count = decomposition.components.len();
        assert!(component_count < usize::BITS as usize);
        let all = (1usize << component_count) - 1;
        let mut predecessor_masks = vec![0usize; component_count];
        for edge in &decomposition.condensation_edges {
            predecessor_masks[edge.target] |= 1usize << edge.source;
        }
        let mut distance = vec![usize::MAX; all + 1];
        distance[0] = 0;
        for state in 0..=all {
            if distance[state] == usize::MAX || state == all {
                continue;
            }
            let mut ready = 0usize;
            for (component, &predecessors) in predecessor_masks.iter().enumerate() {
                let bit = 1usize << component;
                if state & bit == 0 && predecessors & !state == 0 {
                    ready |= bit;
                }
            }
            let mut batch = ready;
            while batch != 0 {
                if batch.count_ones() as usize <= width {
                    let next = state | batch;
                    distance[next] = distance[next].min(distance[state] + 1);
                }
                batch = (batch - 1) & ready;
            }
        }
        distance[all]
    }

    #[test]
    fn empty_decomposition_has_zero_metrics_and_empty_layering() {
        let decomposition = synthetic_decomposition(&[], &[]);
        let metrics = condensation_metrics(&decomposition, &CondensationOptions::default());
        assert_eq!(
            metrics,
            CondensationMetrics {
                levels: Vec::new(),
                critical_path: Vec::new(),
                critical_path_weight: 0,
                height: 0,
                width: 0,
                coffman_graham: None,
                complete: true,
                capped: false,
            }
        );
        let layering = coffman_graham(&decomposition, 3).expect("empty DAG is valid");
        assert_eq!(layering.height(), 0);
        assert_eq!(layering.width(), 0);
    }

    #[test]
    fn weighted_critical_path_and_longest_levels_are_exact() {
        let decomposition =
            synthetic_decomposition(&[2, 1, 3, 1, 2], &[(0, 1), (1, 2), (1, 3), (2, 4), (3, 4)]);
        let metrics = condensation_metrics(&decomposition, &CondensationOptions::default());
        assert!(metrics.complete);
        assert!(!metrics.capped);
        assert_eq!(metrics.levels, vec![0, 1, 2, 2, 3]);
        assert_eq!(metrics.height, 4);
        assert_eq!(metrics.width, 2);
        assert_eq!(metrics.critical_path, vec![0, 1, 2, 4]);
        assert_eq!(metrics.critical_path_weight, 8);
        assert_critical_path(&decomposition, &metrics);

        let mut representative_reordered = decomposition.clone();
        representative_reordered.component_by_node.clear();
        let component_count = representative_reordered.components.len();
        for component in &mut representative_reordered.components {
            let base = (component_count - component.id) as u32 * 10;
            for (offset, node) in component.nodes.iter_mut().enumerate() {
                *node = NodeId::new(base + offset as u32);
                representative_reordered
                    .component_by_node
                    .insert(*node, component.id);
            }
        }
        assert_eq!(
            condensation_metrics(&representative_reordered, &CondensationOptions::default()),
            metrics,
            "projection dense order must not replace component identity",
        );
    }

    #[test]
    fn coffman_graham_is_width_bounded_and_strictly_downward() {
        let decomposition = synthetic_decomposition(
            &[1; 8],
            &[
                (0, 3),
                (0, 4),
                (1, 4),
                (1, 5),
                (2, 5),
                (3, 6),
                (4, 6),
                (5, 7),
            ],
        );
        let layering = coffman_graham(&decomposition, 2).expect("valid DAG and width");
        assert_layering(&decomposition, &layering);
        assert_eq!(layering.width_limit, 2);
        assert!(layering.width() <= 2);
        assert_eq!(
            layering.height(),
            optimal_unit_height(&decomposition, 2) as u32
        );

        let metrics = condensation_metrics(
            &decomposition,
            &CondensationOptions {
                coffman_graham_width: Some(2),
                ..CondensationOptions::default()
            },
        );
        assert_eq!(metrics.coffman_graham, Some(layering));
        assert!(metrics.complete);
    }

    #[test]
    fn caps_and_invalid_inputs_are_reported_without_panics() {
        let decomposition = synthetic_decomposition(&[1, 1, 1], &[(0, 1), (1, 2)]);
        let capped = condensation_metrics(
            &decomposition,
            &CondensationOptions {
                max_components: 2,
                coffman_graham_width: Some(2),
            },
        );
        assert!(!capped.complete);
        assert!(capped.capped);
        assert!(capped.levels.is_empty());
        assert_eq!(MAX_CONDENSATION_COMPONENTS, 1_000_000);
        assert!(matches!(
            CondensationDag::from_decomposition(&decomposition, 2, ANALYSIS_NAME),
            Err(AnalysisError::BudgetExceeded {
                analysis: ANALYSIS_NAME,
                unit: "components",
                needed: 3,
                cap: 2,
            })
        ));

        let unlimited_option = condensation_metrics(
            &decomposition,
            &CondensationOptions {
                max_components: usize::MAX,
                coffman_graham_width: None,
            },
        );
        assert!(unlimited_option.complete);

        let invalid_optional_width = condensation_metrics(
            &decomposition,
            &CondensationOptions {
                max_components: MAX_CONDENSATION_COMPONENTS,
                coffman_graham_width: Some(0),
            },
        );
        assert!(!invalid_optional_width.complete);
        assert!(!invalid_optional_width.capped);
        assert_eq!(invalid_optional_width.levels, vec![0, 1, 2]);

        assert_eq!(
            coffman_graham(&decomposition, 0),
            Err(AnalysisError::InvalidInput {
                analysis: COFFMAN_GRAHAM_NAME,
                input: "width",
                reason: "must be at least one",
            })
        );

        let cyclic = synthetic_decomposition(&[1, 1], &[(0, 1), (1, 0)]);
        assert!(matches!(
            coffman_graham(&cyclic, 2),
            Err(AnalysisError::InvalidInput { .. })
        ));
        assert!(!condensation_metrics(&cyclic, &CondensationOptions::default()).complete);

        let mut bad_id = synthetic_decomposition(&[1], &[]);
        bad_id.components[0].id = 9;
        assert!(matches!(
            coffman_graham(&bad_id, 1),
            Err(AnalysisError::InvalidInput { .. })
        ));

        let mut bad_edge = synthetic_decomposition(&[1], &[]);
        bad_edge.condensation_edges.push(SccCondensationEdge {
            source: 0,
            target: 7,
        });
        assert!(matches!(
            coffman_graham(&bad_edge, 1),
            Err(AnalysisError::InvalidInput { .. })
        ));

        let mut empty_component = synthetic_decomposition(&[1], &[]);
        empty_component.components[0].nodes.clear();
        empty_component.component_by_node.clear();
        assert!(matches!(
            coffman_graham(&empty_component, 1),
            Err(AnalysisError::InvalidInput { .. })
        ));

        let self_edge = synthetic_decomposition(&[1], &[(0, 0)]);
        assert!(matches!(
            coffman_graham(&self_edge, 1),
            Err(AnalysisError::InvalidInput { .. })
        ));

        let mut mismatched_membership = synthetic_decomposition(&[1, 1], &[]);
        let first_node = mismatched_membership.components[0].nodes[0];
        mismatched_membership
            .component_by_node
            .insert(first_node, 1);
        assert!(matches!(
            coffman_graham(&mismatched_membership, 1),
            Err(AnalysisError::InvalidInput { .. })
        ));

        let mut extra_membership = synthetic_decomposition(&[1], &[]);
        extra_membership
            .component_by_node
            .insert(NodeId::new(99), 0);
        assert!(matches!(
            coffman_graham(&extra_membership, 1),
            Err(AnalysisError::InvalidInput { .. })
        ));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        /// Brute-force path enumeration and exact unit-task scheduling are
        /// independent oracles for arbitrary small DAGs.
        #[test]
        fn metrics_and_layering_match_small_graph_oracles(
            node_count in 1usize..9,
            raw_edges in prop::collection::vec((0usize..9, 0usize..9), 0..40),
            raw_sizes in prop::collection::vec(1usize..5, 9),
            width in 1u32..5,
        ) {
            let mut edges: Vec<_> = raw_edges
                .into_iter()
                .filter(|&(source, target)| source < node_count && target < node_count && source < target)
                .collect();
            edges.sort_unstable();
            edges.dedup();
            let decomposition = synthetic_decomposition(&raw_sizes[..node_count], &edges);
            let options = CondensationOptions {
                coffman_graham_width: Some(width),
                ..CondensationOptions::default()
            };
            let metrics = condensation_metrics(&decomposition, &options);
            prop_assert!(metrics.complete);
            prop_assert!(!metrics.capped);
            prop_assert_eq!(
                metrics.critical_path_weight,
                brute_longest_path_weight(&decomposition),
            );
            assert_critical_path(&decomposition, &metrics);
            for &(source, target) in &edges {
                prop_assert!(metrics.levels[source] < metrics.levels[target]);
            }

            let layering = metrics.coffman_graham.as_ref().expect("requested layering");
            assert_layering(&decomposition, layering);
            let optimum = optimal_unit_height(&decomposition, width as usize);
            if width == 1 {
                prop_assert_eq!(layering.height() as usize, optimum);
            } else {
                prop_assert!(
                    width as usize * layering.height() as usize
                        <= (2 * width as usize - 2) * optimum,
                    "CG approximation bound: width={}, height={}, optimum={}",
                    width,
                    layering.height(),
                    optimum,
                );
            }

            let mut permuted_edges: Vec<_> = edges.iter().rev().copied().collect();
            permuted_edges.extend(edges.iter().copied());
            let permuted = synthetic_decomposition(&raw_sizes[..node_count], &permuted_edges);
            prop_assert_eq!(
                condensation_metrics(&permuted, &options),
                metrics,
            );
            prop_assert_eq!(
                coffman_graham(&permuted, width).expect("permuted DAG"),
                coffman_graham(&decomposition, width).expect("canonical DAG"),
            );
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn reports_round_trip_through_json() {
        let decomposition = synthetic_decomposition(&[2, 1, 1], &[(0, 1), (0, 2)]);
        let metrics = condensation_metrics(
            &decomposition,
            &CondensationOptions {
                coffman_graham_width: Some(2),
                ..CondensationOptions::default()
            },
        );
        let encoded = serde_json::to_string(&metrics).expect("serialize metrics");
        let decoded: CondensationMetrics =
            serde_json::from_str(&encoded).expect("deserialize metrics");
        assert_eq!(decoded, metrics);

        let options = CondensationOptions {
            max_components: 73,
            coffman_graham_width: Some(4),
        };
        let encoded = serde_json::to_string(&options).expect("serialize options");
        let decoded: CondensationOptions =
            serde_json::from_str(&encoded).expect("deserialize options");
        assert_eq!(decoded, options);
    }
}
