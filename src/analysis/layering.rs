//! Deterministic architectural layering over an SCC condensation.
//!
//! [`CondensationMetrics`] numbers longest-path levels from sources toward
//! sinks. This module reverses that numbering for architectural presentation:
//! layer zero is the lowest layer, larger indices are higher layers, and a
//! healthy dependency moves down exactly one layer. Cyclic SCCs are reported
//! as component-level witnesses because condensation deliberately discards the
//! identities and multiplicities of edges internal to a component.

use super::{condensation_metrics, CondensationMetrics, CondensationOptions, SccDecomposition};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Why a dependency violates the inferred architectural layering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum LayeringViolationKind {
    /// One or more dependency edges remain inside a cyclic SCC.
    CyclicComponent,
    /// A dependency moves down more than one layer.
    SkipLayer,
    /// A dependency moves upward or remains on the same layer.
    Upward,
}

/// One deterministic witness that violates the inferred layering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LayeringViolation {
    /// Violation classification.
    pub kind: LayeringViolationKind,
    /// Source SCC component id.
    ///
    /// For [`LayeringViolationKind::CyclicComponent`], this equals
    /// [`Self::target_component`] and identifies the contracted component.
    pub source_component: usize,
    /// Target SCC component id.
    pub target_component: usize,
    /// Bottom-first source layer index.
    pub source_layer: u32,
    /// Bottom-first target layer index.
    pub target_layer: u32,
    /// Absolute distance between the source and target layers.
    ///
    /// A cyclic-component witness has distance zero.
    pub layers_crossed: u32,
}

/// Longest-path architectural layers and their dependency violations.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LayeringReport {
    /// Bottom-first layers containing sorted SCC component ids.
    ///
    /// The highest-indexed layer contains source components. Empty input has
    /// no layers.
    pub layers: Vec<Vec<usize>>,
    /// Bottom-first layer index for every SCC component id.
    pub layer_of: Vec<u32>,
    /// Sorted cyclic, skip-layer, and upward dependency witnesses.
    pub violations: Vec<LayeringViolation>,
    /// Whether the decomposition and level vector were structurally valid.
    ///
    /// A complete report may contain violations. `false` means the public
    /// input structs were malformed or an upstream computation was incomplete.
    pub complete: bool,
    /// Whether the upstream condensation computation exceeded its cap.
    pub capped: bool,
}

impl LayeringReport {
    fn unavailable(capped: bool) -> Self {
        Self {
            layers: Vec::new(),
            layer_of: Vec::new(),
            violations: Vec::new(),
            complete: false,
            capped,
        }
    }
}

/// Infers bottom-first architectural layers and classifies violations.
///
/// The supplied [`CondensationMetrics::levels`] are interpreted as
/// source-oriented longest-path levels and reversed. For every condensation
/// edge `u -> v`, a one-layer descent is healthy, a longer descent is a
/// [`LayeringViolationKind::SkipLayer`], and a non-descent is an
/// [`LayeringViolationKind::Upward`]. The latter can only arise when callers
/// construct or deserialize metrics that are stale with respect to the
/// decomposition; metrics returned directly by [`condensation_metrics`] obey
/// the longest-path invariant.
///
/// The function is total over public struct values. It returns an incomplete
/// empty report for malformed decompositions, incomplete metrics, mismatched
/// level domains, impossible level values, or inconsistent height and width.
pub fn infer_layering(
    decomposition: &SccDecomposition,
    metrics: &CondensationMetrics,
) -> LayeringReport {
    if !metrics.complete {
        return LayeringReport::unavailable(metrics.capped);
    }

    // Reuse the condensation engine's complete structural validator instead
    // of maintaining a second, subtly different definition of a valid SCC DAG.
    let validated = condensation_metrics(decomposition, &CondensationOptions::default());
    if !validated.complete {
        return LayeringReport::unavailable(validated.capped);
    }
    if decomposition
        .components
        .iter()
        .any(|component| component.nodes.len() > 1 && !component.cyclic)
        || decomposition
            .condensation_edges
            .windows(2)
            .any(|pair| (pair[0].source, pair[0].target) >= (pair[1].source, pair[1].target))
    {
        return LayeringReport::unavailable(false);
    }

    let component_count = decomposition.components.len();
    if metrics.capped || metrics.levels.len() != component_count {
        return LayeringReport::unavailable(metrics.capped);
    }
    if component_count == 0 {
        return if metrics.height == 0 && metrics.width == 0 {
            LayeringReport {
                layers: Vec::new(),
                layer_of: Vec::new(),
                violations: Vec::new(),
                complete: true,
                capped: false,
            }
        } else {
            LayeringReport::unavailable(false)
        };
    }

    let max_level = *metrics
        .levels
        .iter()
        .max()
        .expect("a non-empty component domain has a level");
    let max_level_index = max_level as usize;
    if max_level_index >= component_count || metrics.height != max_level.saturating_add(1) {
        return LayeringReport::unavailable(false);
    }

    let mut source_level_widths = vec![0usize; max_level_index + 1];
    for &level in &metrics.levels {
        source_level_widths[level as usize] += 1;
    }
    let observed_width = source_level_widths.iter().copied().max().unwrap_or(0);
    if source_level_widths.contains(&0) || metrics.width as usize != observed_width {
        return LayeringReport::unavailable(false);
    }

    let mut layers = vec![Vec::new(); max_level_index + 1];
    let mut layer_of = Vec::with_capacity(component_count);
    for (component, &source_level) in metrics.levels.iter().enumerate() {
        let layer = max_level - source_level;
        let layer_index = layer as usize;
        layers[layer_index].push(component);
        layer_of.push(layer);
    }

    let mut violations = Vec::new();
    for component in decomposition
        .components
        .iter()
        .filter(|component| component.cyclic)
    {
        let layer = layer_of[component.id];
        violations.push(LayeringViolation {
            kind: LayeringViolationKind::CyclicComponent,
            source_component: component.id,
            target_component: component.id,
            source_layer: layer,
            target_layer: layer,
            layers_crossed: 0,
        });
    }

    for edge in &decomposition.condensation_edges {
        let source_layer = layer_of[edge.source];
        let target_layer = layer_of[edge.target];
        let kind = if source_layer > target_layer.saturating_add(1) {
            Some(LayeringViolationKind::SkipLayer)
        } else if source_layer <= target_layer {
            Some(LayeringViolationKind::Upward)
        } else {
            None
        };
        if let Some(kind) = kind {
            violations.push(LayeringViolation {
                kind,
                source_component: edge.source,
                target_component: edge.target,
                source_layer,
                target_layer,
                layers_crossed: source_layer.abs_diff(target_layer),
            });
        }
    }
    violations.sort_unstable();

    LayeringReport {
        layers,
        layer_of,
        violations,
        complete: true,
        capped: false,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use proptest::prelude::*;

    use super::*;
    use crate::{NodeId, ProjectionKind};

    use super::super::{
        condensation_metrics, CondensationOptions, SccComponent, SccCondensationEdge, SccProjection,
    };

    fn decomposition(
        cyclic: &[usize],
        component_count: usize,
        edges: &[(usize, usize)],
    ) -> SccDecomposition {
        let components: Vec<_> = (0..component_count)
            .map(|id| SccComponent {
                id,
                nodes: vec![NodeId::new(id as u32)],
                cyclic: cyclic.contains(&id),
            })
            .collect();
        let component_by_node = components
            .iter()
            .map(|component| (component.nodes[0], component.id))
            .collect::<BTreeMap<_, _>>();
        let mut condensation_edges = edges
            .iter()
            .map(|&(source, target)| SccCondensationEdge { source, target })
            .collect::<Vec<_>>();
        condensation_edges.sort_unstable_by_key(|edge| (edge.source, edge.target));
        condensation_edges.dedup();
        SccDecomposition {
            projection: SccProjection::Projection(ProjectionKind::ModuleGraph),
            components,
            component_by_node,
            condensation_edges,
        }
    }

    #[test]
    fn exact_layers_flag_cycles_and_downward_skips() {
        let decomposition = decomposition(&[1], 4, &[(0, 1), (0, 2), (1, 2), (2, 3)]);
        let metrics = condensation_metrics(&decomposition, &CondensationOptions::default());

        let report = infer_layering(&decomposition, &metrics);

        assert!(report.complete);
        assert!(!report.capped);
        assert_eq!(metrics.levels, vec![0, 1, 2, 3]);
        assert_eq!(report.layers, vec![vec![3], vec![2], vec![1], vec![0]]);
        assert_eq!(report.layer_of, vec![3, 2, 1, 0]);
        assert_eq!(
            report.violations,
            vec![
                LayeringViolation {
                    kind: LayeringViolationKind::CyclicComponent,
                    source_component: 1,
                    target_component: 1,
                    source_layer: 2,
                    target_layer: 2,
                    layers_crossed: 0,
                },
                LayeringViolation {
                    kind: LayeringViolationKind::SkipLayer,
                    source_component: 0,
                    target_component: 2,
                    source_layer: 3,
                    target_layer: 1,
                    layers_crossed: 2,
                },
            ]
        );

        for edge in &decomposition.condensation_edges {
            let is_violation = report.violations.iter().any(|violation| {
                violation.source_component == edge.source
                    && violation.target_component == edge.target
            });
            if !is_violation {
                assert_eq!(
                    report.layer_of[edge.source],
                    report.layer_of[edge.target] + 1
                );
            }
        }
    }

    #[test]
    fn stale_but_bounded_levels_expose_upward_and_lateral_edges() {
        let decomposition = decomposition(&[], 3, &[(0, 1), (1, 2)]);
        let metrics = CondensationMetrics {
            levels: vec![1, 1, 0],
            critical_path: vec![0, 1],
            critical_path_weight: 2,
            height: 2,
            width: 2,
            coffman_graham: None,
            complete: true,
            capped: false,
        };

        let report = infer_layering(&decomposition, &metrics);

        assert!(report.complete);
        assert_eq!(report.layers, vec![vec![0, 1], vec![2]]);
        assert_eq!(
            report.violations,
            vec![
                LayeringViolation {
                    kind: LayeringViolationKind::Upward,
                    source_component: 0,
                    target_component: 1,
                    source_layer: 0,
                    target_layer: 0,
                    layers_crossed: 0,
                },
                LayeringViolation {
                    kind: LayeringViolationKind::Upward,
                    source_component: 1,
                    target_component: 2,
                    source_layer: 0,
                    target_layer: 1,
                    layers_crossed: 1,
                },
            ]
        );
    }

    #[test]
    fn empty_decomposition_has_an_empty_complete_report() {
        let decomposition = decomposition(&[], 0, &[]);
        let metrics = condensation_metrics(&decomposition, &CondensationOptions::default());

        assert_eq!(
            infer_layering(&decomposition, &metrics),
            LayeringReport {
                layers: Vec::new(),
                layer_of: Vec::new(),
                violations: Vec::new(),
                complete: true,
                capped: false,
            }
        );
    }

    #[test]
    fn malformed_and_incomplete_public_inputs_are_total() {
        let mut outside = decomposition(&[], 2, &[(0, 2)]);
        let apparently_complete = CondensationMetrics {
            levels: vec![0, 1],
            critical_path: vec![0, 1],
            critical_path_weight: 2,
            height: 2,
            width: 1,
            coffman_graham: None,
            complete: true,
            capped: false,
        };
        assert!(!infer_layering(&outside, &apparently_complete).complete);

        outside.condensation_edges = vec![SccCondensationEdge {
            source: 0,
            target: 1,
        }];
        let incomplete = CondensationMetrics {
            complete: false,
            capped: true,
            ..apparently_complete.clone()
        };
        let report = infer_layering(&outside, &incomplete);
        assert!(!report.complete);
        assert!(report.capped);

        let complete_but_capped = CondensationMetrics {
            complete: true,
            ..incomplete
        };
        let report = infer_layering(&outside, &complete_but_capped);
        assert!(!report.complete);
        assert!(report.capped);

        let wrong_domain = CondensationMetrics {
            levels: vec![0],
            height: 1,
            width: 1,
            ..apparently_complete.clone()
        };
        assert!(!infer_layering(&outside, &wrong_domain).complete);

        let wrong_height = CondensationMetrics {
            height: 1,
            ..apparently_complete.clone()
        };
        assert!(!infer_layering(&outside, &wrong_height).complete);

        let impossible_level = CondensationMetrics {
            levels: vec![0, u32::MAX],
            height: u32::MAX,
            ..apparently_complete.clone()
        };
        assert!(!infer_layering(&outside, &impossible_level).complete);

        let mut duplicate_edges = decomposition(&[], 2, &[(0, 1)]);
        duplicate_edges
            .condensation_edges
            .push(SccCondensationEdge {
                source: 0,
                target: 1,
            });
        assert!(!infer_layering(&duplicate_edges, &apparently_complete).complete);

        let mut impossible_component = decomposition(&[], 2, &[(0, 1)]);
        impossible_component.components[0]
            .nodes
            .push(NodeId::new(20));
        impossible_component
            .component_by_node
            .insert(NodeId::new(20), 0);
        assert!(!infer_layering(&impossible_component, &apparently_complete).complete);

        let three = decomposition(&[], 3, &[(0, 2)]);
        let gap = CondensationMetrics {
            levels: vec![0, 2, 2],
            critical_path: vec![0, 2],
            critical_path_weight: 2,
            height: 3,
            width: 2,
            coffman_graham: None,
            complete: true,
            capped: false,
        };
        assert!(!infer_layering(&three, &gap).complete);

        let wrong_width = CondensationMetrics {
            levels: vec![0, 1, 1],
            height: 2,
            width: 1,
            ..gap
        };
        assert!(!infer_layering(&three, &wrong_width).complete);

        let empty = decomposition(&[], 0, &[]);
        let invalid_empty = CondensationMetrics {
            levels: Vec::new(),
            critical_path: Vec::new(),
            critical_path_weight: 0,
            height: 1,
            width: 0,
            coffman_graham: None,
            complete: true,
            capped: false,
        };
        assert!(!infer_layering(&empty, &invalid_empty).complete);

        let invalid_empty_width = CondensationMetrics {
            height: 0,
            width: 1,
            ..invalid_empty
        };
        assert!(!infer_layering(&empty, &invalid_empty_width).complete);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn valid_dag_reports_are_deterministic_and_healthy_edges_descend(
            node_count in 0usize..20,
            edge_bits in prop::collection::vec(any::<bool>(), 0..400),
            cyclic_bits in prop::collection::vec(any::<bool>(), 0..20),
        ) {
            let mut bit = edge_bits.into_iter();
            let mut edges = Vec::new();
            for source in 0..node_count {
                for target in (source + 1)..node_count {
                    if bit.next().unwrap_or(false) {
                        edges.push((source, target));
                    }
                }
            }
            let cyclic = cyclic_bits
                .into_iter()
                .enumerate()
                .filter_map(|(id, yes)| (yes && id < node_count).then_some(id))
                .collect::<Vec<_>>();
            let graph = decomposition(&cyclic, node_count, &edges);
            let metrics = condensation_metrics(&graph, &CondensationOptions::default());

            let mut reordered_edges = edges.clone();
            reordered_edges.reverse();
            reordered_edges.extend(edges.iter().take(3).copied());
            let reordered = decomposition(&cyclic, node_count, &reordered_edges);
            let reordered_metrics =
                condensation_metrics(&reordered, &CondensationOptions::default());

            let first = infer_layering(&graph, &metrics);
            let second = infer_layering(&graph, &metrics);
            let from_reordered = infer_layering(&reordered, &reordered_metrics);
            prop_assert_eq!(&first, &second);
            prop_assert_eq!(&first, &from_reordered);
            prop_assert!(first.complete);

            let mut assigned = first.layers.iter().flatten().copied().collect::<Vec<_>>();
            assigned.sort_unstable();
            prop_assert_eq!(assigned, (0..node_count).collect::<Vec<_>>());

            for &component in &cyclic {
                prop_assert_eq!(
                    first
                        .violations
                        .iter()
                        .filter(|violation| {
                            violation.kind == LayeringViolationKind::CyclicComponent
                                && violation.source_component == component
                                && violation.target_component == component
                        })
                        .count(),
                    1
                );
            }

            for edge in &graph.condensation_edges {
                let source_layer = first.layer_of[edge.source];
                let target_layer = first.layer_of[edge.target];
                let expected_kind = if source_layer > target_layer + 1 {
                    Some(LayeringViolationKind::SkipLayer)
                } else if source_layer <= target_layer {
                    Some(LayeringViolationKind::Upward)
                } else {
                    None
                };
                let witnesses = first
                    .violations
                    .iter()
                    .filter(|violation| {
                        violation.source_component == edge.source
                            && violation.target_component == edge.target
                            && violation.kind != LayeringViolationKind::CyclicComponent
                    })
                    .collect::<Vec<_>>();
                match expected_kind {
                    Some(kind) => {
                        prop_assert_eq!(witnesses.len(), 1);
                        prop_assert_eq!(witnesses[0].kind, kind);
                        prop_assert_eq!(
                            witnesses[0].layers_crossed,
                            source_layer.abs_diff(target_layer)
                        );
                    }
                    None => {
                        prop_assert!(witnesses.is_empty());
                        prop_assert_eq!(source_layer, target_layer + 1);
                    }
                }
            }
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn report_round_trips_through_json() {
        let decomposition = decomposition(&[1], 3, &[(0, 1), (0, 2), (1, 2)]);
        let metrics = condensation_metrics(&decomposition, &CondensationOptions::default());
        let report = infer_layering(&decomposition, &metrics);

        let json = serde_json::to_string(&report).expect("serialize report");
        let decoded: LayeringReport = serde_json::from_str(&json).expect("deserialize report");
        assert_eq!(decoded, report);
    }
}
