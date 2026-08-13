//! Deterministic, advisory architectural-smell detection.
//!
//! The detectors combine a directed dependency projection with its exact SCC
//! decomposition, caller-selected module boundaries, and Robert C. Martin's
//! instability metric. They report evidence; they do not mutate the graph or
//! claim that a reported structure is necessarily a defect.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::NodeId;

use super::{projection_sccs, GraphProjection, MartinMetrics, ModuleAssignment, SccDecomposition};

#[cfg(feature = "serde")]
use serde::{Deserialize, Deserializer, Serialize};

const COMPONENT_SIZE: &str = "component_size";
const INTERNAL_DEPENDENCIES: &str = "internal_dependencies";
const FAN_IN: &str = "fan_in";
const FAN_OUT: &str = "fan_out";
const MEAN_FAN_IN: &str = "mean_fan_in";
const MEAN_FAN_OUT: &str = "mean_fan_out";
const STDDEV_FAN_IN: &str = "stddev_fan_in";
const STDDEV_FAN_OUT: &str = "stddev_fan_out";
const FAN_IN_Z_SCORE: &str = "fan_in_z_score";
const FAN_OUT_Z_SCORE: &str = "fan_out_z_score";
const EFFERENT_DEPENDENCIES: &str = "efferent_dependencies";
const LESS_STABLE_DEPENDENCIES: &str = "less_stable_dependencies";
const LESS_STABLE_SHARE: &str = "less_stable_share";
const SOURCE_INSTABILITY: &str = "source_instability";
const SHARE_THRESHOLD: &str = "share_threshold";
const NODE_COUNT: &str = "node_count";
const MEAN_NODE_COUNT: &str = "mean_node_count";
const STDDEV_NODE_COUNT: &str = "stddev_node_count";
const NODE_COUNT_Z_SCORE: &str = "node_count_z_score";

/// Architectural-smell classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ArchSmellKind {
    /// A multi-node strongly connected dependency component.
    CyclicDependency {
        /// Stable index into [`SccDecomposition::components`].
        component: usize,
    },
    /// A node whose incoming and outgoing fan exceed configured sigma bounds.
    HubLikeDependency,
    /// A module whose efferent dependencies disproportionately target less-stable modules.
    UnstableDependency,
    /// A module whose projected node count is a configured high-side outlier.
    GodComponent,
}

/// One advisory architectural-smell finding with numerical evidence.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct ArchSmell {
    /// Smell classification.
    pub kind: ArchSmellKind,
    /// Sorted projection nodes involved in the finding.
    pub nodes: Vec<NodeId>,
    /// Detector-specific finite severity; larger values rank first within a kind.
    pub severity: f64,
    /// Deterministically ordered numerical evidence.
    pub evidence: BTreeMap<&'static str, f64>,
}

/// Thresholds for advisory architectural-smell detection.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SmellOptions {
    /// Standard deviations above mean fan-in required for a hub finding.
    pub hub_fan_in_sigma: f64,
    /// Standard deviations above mean fan-out required for a hub finding.
    pub hub_fan_out_sigma: f64,
    /// Exclusive lower bound on the share of dependencies toward less-stable modules.
    pub unstable_dependency_threshold: f64,
    /// Standard deviations above mean module node count required for a god component.
    pub god_component_sigma: f64,
}

struct ModuleEvidence {
    nodes_by_module: BTreeMap<Arc<str>, Vec<NodeId>>,
    instability: BTreeMap<Arc<str>, f64>,
}

impl Default for SmellOptions {
    fn default() -> Self {
        Self {
            hub_fan_in_sigma: 1.0,
            hub_fan_out_sigma: 1.0,
            unstable_dependency_threshold: 0.5,
            god_component_sigma: 1.0,
        }
    }
}

/// Detects four advisory architectural-smell families.
///
/// Cyclic dependencies are multi-node SCCs. Hub-like dependencies exceed the
/// independently configured population-mean-plus-sigma bounds for both fan-in
/// and fan-out. An unstable-dependency finding is emitted per source module
/// when the share of its cross-module edges targeting a module with greater
/// Martin instability exceeds the configured threshold. God components are
/// module-size high outliers measured over assigned projection-node counts.
/// Projection weights and self-loops are ignored by the fan and module-edge
/// detectors.
///
/// Findings are sorted by smell-family order, descending severity, then the
/// smallest involved node. Inputs are treated as one evidence bundle: an
/// invalid option, malformed projection, stale SCC decomposition, incomplete
/// projection assignment, duplicate/invalid Martin metric, or missing metric
/// yields no advisory verdict.
pub fn architecture_smells(
    view: &GraphProjection,
    scc: &SccDecomposition,
    martin: &[MartinMetrics],
    assignment: &ModuleAssignment,
    options: &SmellOptions,
) -> Vec<ArchSmell> {
    if !valid_options(options) || !view.is_well_formed() || projection_sccs(view) != *scc {
        return Vec::new();
    }

    let Some(module_evidence) = validate_module_evidence(view, martin, assignment) else {
        return Vec::new();
    };

    let mut smells = Vec::new();
    detect_cycles(view, scc, &mut smells);
    detect_hubs(view, options, &mut smells);
    detect_unstable_dependencies(
        view,
        assignment,
        &module_evidence.instability,
        options,
        &mut smells,
    );
    detect_god_components(&module_evidence.nodes_by_module, options, &mut smells);
    smells.sort_by(compare_smells);
    smells
}

fn valid_options(options: &SmellOptions) -> bool {
    options.hub_fan_in_sigma.is_finite()
        && options.hub_fan_in_sigma >= 0.0
        && options.hub_fan_out_sigma.is_finite()
        && options.hub_fan_out_sigma >= 0.0
        && options.unstable_dependency_threshold.is_finite()
        && (0.0..=1.0).contains(&options.unstable_dependency_threshold)
        && options.god_component_sigma.is_finite()
        && options.god_component_sigma >= 0.0
}

fn validate_module_evidence(
    view: &GraphProjection,
    martin: &[MartinMetrics],
    assignment: &ModuleAssignment,
) -> Option<ModuleEvidence> {
    let mut nodes_by_module = BTreeMap::<Arc<str>, Vec<NodeId>>::new();
    for &node in &view.nodes {
        let module = assignment.module_of(node)?.clone();
        nodes_by_module.entry(module).or_default().push(node);
    }

    let mut instability = BTreeMap::<Arc<str>, f64>::new();
    for metric in martin {
        if !metric.instability.is_finite()
            || !(0.0..=1.0).contains(&metric.instability)
            || !metric.abstractness.is_finite()
            || !(0.0..=1.0).contains(&metric.abstractness)
            || !metric.distance.is_finite()
            || !(0.0..=1.0).contains(&metric.distance)
            || instability
                .insert(metric.module.clone(), metric.instability)
                .is_some()
        {
            return None;
        }
    }
    if nodes_by_module
        .keys()
        .any(|module| !instability.contains_key(module))
    {
        return None;
    }
    Some(ModuleEvidence {
        nodes_by_module,
        instability,
    })
}

fn detect_cycles(view: &GraphProjection, scc: &SccDecomposition, smells: &mut Vec<ArchSmell>) {
    let mut internal_dependencies = vec![0usize; scc.components.len()];
    for (source, target, _) in view.edges() {
        let source_component = scc.component_by_node[&view.node_id(source)];
        if source_component == scc.component_by_node[&view.node_id(target)] {
            internal_dependencies[source_component] += 1;
        }
    }

    for component in scc
        .components
        .iter()
        .filter(|component| component.nodes.len() >= 2)
    {
        smells.push(ArchSmell {
            kind: ArchSmellKind::CyclicDependency {
                component: component.id,
            },
            nodes: component.nodes.clone(),
            severity: component.nodes.len() as f64,
            evidence: evidence([
                (COMPONENT_SIZE, component.nodes.len() as f64),
                (
                    INTERNAL_DEPENDENCIES,
                    internal_dependencies[component.id] as f64,
                ),
            ]),
        });
    }
}

fn detect_hubs(view: &GraphProjection, options: &SmellOptions, smells: &mut Vec<ArchSmell>) {
    if view.is_empty() {
        return;
    }
    let fan_in = (0..view.node_count())
        .map(|dense| {
            view.predecessors(dense as u32)
                .iter()
                .filter(|&&source| source != dense as u32)
                .count() as f64
        })
        .collect::<Vec<_>>();
    let fan_out = (0..view.node_count())
        .map(|dense| {
            view.successors(dense as u32)
                .iter()
                .filter(|&&target| target != dense as u32)
                .count() as f64
        })
        .collect::<Vec<_>>();
    let (mean_in, stddev_in) = mean_and_population_stddev(&fan_in);
    let (mean_out, stddev_out) = mean_and_population_stddev(&fan_out);
    let in_threshold = mean_in + options.hub_fan_in_sigma * stddev_in;
    let out_threshold = mean_out + options.hub_fan_out_sigma * stddev_out;

    for dense in 0..view.node_count() {
        if fan_in[dense] <= in_threshold || fan_out[dense] <= out_threshold {
            continue;
        }
        let in_z = positive_z_score(fan_in[dense], mean_in, stddev_in);
        let out_z = positive_z_score(fan_out[dense], mean_out, stddev_out);
        smells.push(ArchSmell {
            kind: ArchSmellKind::HubLikeDependency,
            nodes: vec![view.node_id(dense as u32)],
            severity: in_z.min(out_z),
            evidence: evidence([
                (FAN_IN, fan_in[dense]),
                (FAN_OUT, fan_out[dense]),
                (MEAN_FAN_IN, mean_in),
                (MEAN_FAN_OUT, mean_out),
                (STDDEV_FAN_IN, stddev_in),
                (STDDEV_FAN_OUT, stddev_out),
                (FAN_IN_Z_SCORE, in_z),
                (FAN_OUT_Z_SCORE, out_z),
            ]),
        });
    }
}

fn detect_unstable_dependencies(
    view: &GraphProjection,
    assignment: &ModuleAssignment,
    instability: &BTreeMap<Arc<str>, f64>,
    options: &SmellOptions,
    smells: &mut Vec<ArchSmell>,
) {
    #[derive(Default)]
    struct DependencyEvidence {
        total: usize,
        less_stable: usize,
        nodes: BTreeSet<NodeId>,
    }

    let mut by_source = BTreeMap::<Arc<str>, DependencyEvidence>::new();
    for (source, target, _) in view.edges() {
        if source == target {
            continue;
        }
        let source = view.node_id(source);
        let target = view.node_id(target);
        let source_module = assignment
            .module_of(source)
            .expect("validated projection assignment");
        let target_module = assignment
            .module_of(target)
            .expect("validated projection assignment");
        if source_module == target_module {
            continue;
        }
        let entry = by_source.entry(source_module.clone()).or_default();
        entry.total += 1;
        if instability[source_module] < instability[target_module] {
            entry.less_stable += 1;
            entry.nodes.insert(source);
            entry.nodes.insert(target);
        }
    }

    for (module, dependency) in by_source {
        let share = dependency.less_stable as f64 / dependency.total as f64;
        if share <= options.unstable_dependency_threshold {
            continue;
        }
        smells.push(ArchSmell {
            kind: ArchSmellKind::UnstableDependency,
            nodes: dependency.nodes.into_iter().collect(),
            severity: share,
            evidence: evidence([
                (EFFERENT_DEPENDENCIES, dependency.total as f64),
                (LESS_STABLE_DEPENDENCIES, dependency.less_stable as f64),
                (LESS_STABLE_SHARE, share),
                (SOURCE_INSTABILITY, instability[&module]),
                (SHARE_THRESHOLD, options.unstable_dependency_threshold),
            ]),
        });
    }
}

fn detect_god_components(
    nodes_by_module: &BTreeMap<Arc<str>, Vec<NodeId>>,
    options: &SmellOptions,
    smells: &mut Vec<ArchSmell>,
) {
    if nodes_by_module.is_empty() {
        return;
    }
    let counts = nodes_by_module
        .values()
        .map(|nodes| nodes.len() as f64)
        .collect::<Vec<_>>();
    let (mean, stddev) = mean_and_population_stddev(&counts);
    let threshold = mean + options.god_component_sigma * stddev;
    for nodes in nodes_by_module.values() {
        let count = nodes.len() as f64;
        if count <= threshold {
            continue;
        }
        let z_score = positive_z_score(count, mean, stddev);
        smells.push(ArchSmell {
            kind: ArchSmellKind::GodComponent,
            nodes: nodes.clone(),
            severity: z_score,
            evidence: evidence([
                (NODE_COUNT, count),
                (MEAN_NODE_COUNT, mean),
                (STDDEV_NODE_COUNT, stddev),
                (NODE_COUNT_Z_SCORE, z_score),
            ]),
        });
    }
}

fn mean_and_population_stddev(values: &[f64]) -> (f64, f64) {
    let count = values.len() as f64;
    let mean = values.iter().sum::<f64>() / count;
    let variance = values
        .iter()
        .map(|value| {
            let difference = value - mean;
            difference * difference
        })
        .sum::<f64>()
        / count;
    (mean, variance.sqrt())
}

fn positive_z_score(value: f64, mean: f64, stddev: f64) -> f64 {
    debug_assert!(stddev > 0.0 && value > mean);
    (value - mean) / stddev
}

fn evidence<const N: usize>(pairs: [(&'static str, f64); N]) -> BTreeMap<&'static str, f64> {
    pairs.into_iter().collect()
}

fn kind_rank(kind: ArchSmellKind) -> u8 {
    match kind {
        ArchSmellKind::CyclicDependency { .. } => 0,
        ArchSmellKind::HubLikeDependency => 1,
        ArchSmellKind::UnstableDependency => 2,
        ArchSmellKind::GodComponent => 3,
    }
}

fn compare_smells(left: &ArchSmell, right: &ArchSmell) -> std::cmp::Ordering {
    kind_rank(left.kind)
        .cmp(&kind_rank(right.kind))
        .then_with(|| right.severity.total_cmp(&left.severity))
        .then_with(|| left.nodes.first().cmp(&right.nodes.first()))
        .then_with(|| left.nodes.cmp(&right.nodes))
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for ArchSmell {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct WireArchSmell {
            kind: ArchSmellKind,
            nodes: Vec<NodeId>,
            severity: f64,
            evidence: BTreeMap<String, f64>,
        }

        let wire = WireArchSmell::deserialize(deserializer)?;
        let evidence = wire
            .evidence
            .into_iter()
            .map(|(key, value)| {
                evidence_key(&key)
                    .map(|key| (key, value))
                    .ok_or_else(|| serde::de::Error::custom(format!("unknown evidence key: {key}")))
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            kind: wire.kind,
            nodes: wire.nodes,
            severity: wire.severity,
            evidence,
        })
    }
}

#[cfg(feature = "serde")]
fn evidence_key(key: &str) -> Option<&'static str> {
    Some(match key {
        COMPONENT_SIZE => COMPONENT_SIZE,
        INTERNAL_DEPENDENCIES => INTERNAL_DEPENDENCIES,
        FAN_IN => FAN_IN,
        FAN_OUT => FAN_OUT,
        MEAN_FAN_IN => MEAN_FAN_IN,
        MEAN_FAN_OUT => MEAN_FAN_OUT,
        STDDEV_FAN_IN => STDDEV_FAN_IN,
        STDDEV_FAN_OUT => STDDEV_FAN_OUT,
        FAN_IN_Z_SCORE => FAN_IN_Z_SCORE,
        FAN_OUT_Z_SCORE => FAN_OUT_Z_SCORE,
        EFFERENT_DEPENDENCIES => EFFERENT_DEPENDENCIES,
        LESS_STABLE_DEPENDENCIES => LESS_STABLE_DEPENDENCIES,
        LESS_STABLE_SHARE => LESS_STABLE_SHARE,
        SOURCE_INSTABILITY => SOURCE_INSTABILITY,
        SHARE_THRESHOLD => SHARE_THRESHOLD,
        NODE_COUNT => NODE_COUNT,
        MEAN_NODE_COUNT => MEAN_NODE_COUNT,
        STDDEV_NODE_COUNT => STDDEV_NODE_COUNT,
        NODE_COUNT_Z_SCORE => NODE_COUNT_Z_SCORE,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::analysis::martin_metrics;
    use crate::{CodePropertyGraph, Language, ProjectionKind};

    fn graph(node_count: usize, edges: &[(usize, usize)]) -> GraphProjection {
        GraphProjection::from_edges(
            ProjectionKind::External,
            (0..node_count).map(|node| NodeId::new((node * 7 + 3) as u32)),
            edges.iter().map(|&(source, target)| {
                (
                    NodeId::new((source * 7 + 3) as u32),
                    NodeId::new((target * 7 + 3) as u32),
                    1.0,
                )
            }),
        )
    }

    fn one_module_per_node(view: &GraphProjection) -> ModuleAssignment {
        ModuleAssignment::from_pairs(
            view.nodes
                .iter()
                .enumerate()
                .map(|(dense, &node)| (node, Arc::<str>::from(format!("m{dense:03}")))),
        )
    }

    fn metrics(view: &GraphProjection, assignment: &ModuleAssignment) -> Vec<MartinMetrics> {
        martin_metrics(&CodePropertyGraph::new(Language::Rust), assignment, view)
    }

    fn smells(view: &GraphProjection, assignment: &ModuleAssignment) -> Vec<ArchSmell> {
        architecture_smells(
            view,
            &projection_sccs(view),
            &metrics(view, assignment),
            assignment,
            &SmellOptions::default(),
        )
    }

    #[test]
    fn multi_node_cycle_has_exact_component_evidence() {
        let view = graph(4, &[(0, 1), (1, 0), (1, 2), (3, 0)]);
        let assignment = ModuleAssignment::from_pairs(
            view.nodes
                .iter()
                .copied()
                .map(|node| (node, Arc::<str>::from("one-module"))),
        );
        let found = smells(&view, &assignment);

        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].kind,
            ArchSmellKind::CyclicDependency { component: 0 }
        );
        assert_eq!(found[0].nodes, vec![NodeId::new(3), NodeId::new(10)]);
        assert_eq!(found[0].severity, 2.0);
        assert_eq!(found[0].evidence[COMPONENT_SIZE], 2.0);
        assert_eq!(found[0].evidence[INTERNAL_DEPENDENCIES], 2.0);
    }

    #[test]
    fn total_dependence_oracle_finds_only_the_two_sided_hub() {
        let view = graph(7, &[(1, 0), (2, 0), (3, 0), (0, 4), (0, 5), (0, 6)]);
        let assignment = one_module_per_node(&view);
        let found = smells(&view, &assignment);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, ArchSmellKind::HubLikeDependency);
        assert_eq!(found[0].nodes, vec![NodeId::new(3)]);
        assert_eq!(found[0].evidence[FAN_IN], 3.0);
        assert_eq!(found[0].evidence[FAN_OUT], 3.0);
        assert!(found[0].evidence[FAN_IN_Z_SCORE] > 2.0);
        assert!(found[0].evidence[FAN_OUT_Z_SCORE] > 2.0);
    }

    #[test]
    fn martin_oracle_finds_majority_share_toward_less_stable_module() {
        let view = graph(8, &[(0, 3), (1, 3), (2, 3), (3, 4), (4, 5), (4, 6), (4, 7)]);
        let assignment = one_module_per_node(&view);
        let found = smells(&view, &assignment);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, ArchSmellKind::UnstableDependency);
        assert_eq!(found[0].nodes, vec![NodeId::new(24), NodeId::new(31)]);
        assert_eq!(found[0].severity, 1.0);
        assert_eq!(found[0].evidence[EFFERENT_DEPENDENCIES], 1.0);
        assert_eq!(found[0].evidence[LESS_STABLE_DEPENDENCIES], 1.0);
        assert_eq!(found[0].evidence[SOURCE_INSTABILITY], 0.25);
    }

    #[test]
    fn module_size_z_score_finds_only_the_god_component() {
        let view = graph(8, &[]);
        let assignment =
            ModuleAssignment::from_pairs(view.nodes.iter().enumerate().map(|(dense, &node)| {
                let module = if dense < 4 {
                    Arc::<str>::from("large")
                } else {
                    Arc::<str>::from(format!("small-{dense}"))
                };
                (node, module)
            }));
        let found = smells(&view, &assignment);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, ArchSmellKind::GodComponent);
        assert_eq!(
            found[0].nodes,
            vec![
                NodeId::new(3),
                NodeId::new(10),
                NodeId::new(17),
                NodeId::new(24)
            ]
        );
        assert_eq!(found[0].evidence[NODE_COUNT], 4.0);
        assert!(found[0].evidence[NODE_COUNT_Z_SCORE] > 1.0);
    }

    #[test]
    fn clean_chain_and_singleton_self_cycle_have_no_smells() {
        let chain = graph(3, &[(0, 1), (1, 2)]);
        assert!(smells(&chain, &one_module_per_node(&chain)).is_empty());

        let self_cycle = graph(1, &[(0, 0)]);
        assert!(smells(&self_cycle, &one_module_per_node(&self_cycle)).is_empty());
    }

    #[test]
    fn invalid_bundle_fails_closed_and_outside_assignment_is_inert() {
        let view = graph(3, &[(0, 1), (1, 2)]);
        let mut assignment = one_module_per_node(&view);
        assignment.of_node.insert(NodeId::new(999_999), u32::MAX);
        assert!(smells(&view, &assignment).is_empty());

        let mut partial = assignment.clone();
        partial.of_node.remove(&view.nodes[0]);
        assert!(architecture_smells(
            &view,
            &projection_sccs(&view),
            &metrics(&view, &assignment),
            &partial,
            &SmellOptions::default()
        )
        .is_empty());

        let stale_scc = projection_sccs(&graph(3, &[(0, 2)]));
        assert!(architecture_smells(
            &view,
            &stale_scc,
            &metrics(&view, &assignment),
            &assignment,
            &SmellOptions::default()
        )
        .is_empty());

        let mut malformed = view.clone();
        malformed.offsets.clear();
        assert!(architecture_smells(
            &malformed,
            &projection_sccs(&view),
            &metrics(&view, &assignment),
            &assignment,
            &SmellOptions::default()
        )
        .is_empty());

        let mut invalid_metrics = metrics(&view, &assignment);
        invalid_metrics[0].instability = f64::NAN;
        assert!(architecture_smells(
            &view,
            &projection_sccs(&view),
            &invalid_metrics,
            &assignment,
            &SmellOptions::default()
        )
        .is_empty());

        let valid_metrics = metrics(&view, &assignment);
        let mut invalid_metric_sets = Vec::new();
        enum MetricField {
            Instability,
            Abstractness,
            Distance,
        }
        for (field, value) in [
            (MetricField::Instability, -0.1),
            (MetricField::Abstractness, f64::NAN),
            (MetricField::Abstractness, 1.1),
            (MetricField::Distance, f64::INFINITY),
            (MetricField::Distance, -0.1),
        ] {
            let mut invalid = valid_metrics.clone();
            match field {
                MetricField::Instability => invalid[0].instability = value,
                MetricField::Abstractness => invalid[0].abstractness = value,
                MetricField::Distance => invalid[0].distance = value,
            }
            invalid_metric_sets.push(invalid);
        }
        for invalid in invalid_metric_sets {
            assert!(architecture_smells(
                &view,
                &projection_sccs(&view),
                &invalid,
                &assignment,
                &SmellOptions::default()
            )
            .is_empty());
        }

        let duplicate = [invalid_metrics[1].clone(), invalid_metrics[1].clone()];
        assert!(architecture_smells(
            &view,
            &projection_sccs(&view),
            &duplicate,
            &assignment,
            &SmellOptions::default()
        )
        .is_empty());

        let missing_metric = metrics(&view, &assignment);
        assert!(architecture_smells(
            &view,
            &projection_sccs(&view),
            &missing_metric[..2],
            &assignment,
            &SmellOptions::default()
        )
        .is_empty());

        for invalid in [
            SmellOptions {
                hub_fan_in_sigma: f64::NAN,
                ..SmellOptions::default()
            },
            SmellOptions {
                hub_fan_in_sigma: -1.0,
                ..SmellOptions::default()
            },
            SmellOptions {
                hub_fan_out_sigma: f64::INFINITY,
                ..SmellOptions::default()
            },
            SmellOptions {
                hub_fan_out_sigma: -1.0,
                ..SmellOptions::default()
            },
            SmellOptions {
                unstable_dependency_threshold: f64::NAN,
                ..SmellOptions::default()
            },
            SmellOptions {
                unstable_dependency_threshold: -0.1,
                ..SmellOptions::default()
            },
            SmellOptions {
                unstable_dependency_threshold: 1.1,
                ..SmellOptions::default()
            },
            SmellOptions {
                god_component_sigma: f64::INFINITY,
                ..SmellOptions::default()
            },
            SmellOptions {
                god_component_sigma: -1.0,
                ..SmellOptions::default()
            },
        ] {
            assert!(architecture_smells(
                &view,
                &projection_sccs(&view),
                &metrics(&view, &assignment),
                &assignment,
                &invalid
            )
            .is_empty());
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn findings_are_deterministic_order_invariant_and_inside_the_projection(
            node_count in 0usize..=18,
            module_ids in prop::collection::vec(0u8..5, 18),
            raw_edges in prop::collection::vec((0usize..18, 0usize..18, 1u8..=4), 0..160),
        ) {
            let nodes = (0..node_count)
                .map(|dense| NodeId::new((dense * 7 + 3) as u32))
                .collect::<Vec<_>>();
            let edges = raw_edges
                .into_iter()
                .filter(|&(source, target, _)| source < node_count && target < node_count)
                .map(|(source, target, weight)| {
                    (nodes[source], nodes[target], f64::from(weight))
                })
                .collect::<Vec<_>>();
            let mut reversed_edges = edges.clone();
            reversed_edges.reverse();
            let first = GraphProjection::from_edges(
                ProjectionKind::External,
                nodes.iter().copied(),
                edges,
            );
            let second = GraphProjection::from_edges(
                ProjectionKind::External,
                nodes.iter().rev().copied(),
                reversed_edges,
            );

            let pairs = nodes
                .iter()
                .enumerate()
                .map(|(dense, &node)| {
                    (node, Arc::<str>::from(format!("m{}", module_ids[dense])))
                })
                .collect::<Vec<_>>();
            let mut reversed_pairs = pairs.clone();
            reversed_pairs.reverse();
            let first_assignment = ModuleAssignment::from_pairs(pairs);
            let second_assignment = ModuleAssignment::from_pairs(reversed_pairs);
            prop_assert_eq!(&first, &second);
            prop_assert_eq!(&first_assignment, &second_assignment);

            let first_metrics = metrics(&first, &first_assignment);
            let second_metrics = metrics(&second, &second_assignment);
            let first_smells = architecture_smells(
                &first,
                &projection_sccs(&first),
                &first_metrics,
                &first_assignment,
                &SmellOptions::default(),
            );
            let second_smells = architecture_smells(
                &second,
                &projection_sccs(&second),
                &second_metrics,
                &second_assignment,
                &SmellOptions::default(),
            );
            prop_assert_eq!(&first_smells, &second_smells);

            for smell in &first_smells {
                prop_assert!(!smell.nodes.is_empty());
                prop_assert!(smell.severity.is_finite());
                prop_assert!(smell.nodes.windows(2).all(|pair| pair[0] < pair[1]));
                prop_assert!(smell
                    .nodes
                    .iter()
                    .all(|node| first.dense_index(*node).is_some()));
                prop_assert!(smell.evidence.values().all(|value| value.is_finite()));
            }
            let is_canonically_sorted = first_smells.windows(2).all(|pair| {
                compare_smells(&pair[0], &pair[1]) != std::cmp::Ordering::Greater
            });
            prop_assert!(is_canonically_sorted);
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn public_findings_and_options_round_trip_through_json() {
        let view = graph(7, &[(1, 0), (2, 0), (3, 0), (0, 4), (0, 5), (0, 6)]);
        let assignment = one_module_per_node(&view);
        let found = smells(&view, &assignment);
        let encoded = serde_json::to_string(&found).expect("serialize smells");
        let decoded: Vec<ArchSmell> = serde_json::from_str(&encoded).expect("deserialize smells");
        assert_eq!(decoded, found);

        let options = SmellOptions::default();
        let encoded = serde_json::to_string(&options).expect("serialize options");
        let decoded: SmellOptions = serde_json::from_str(&encoded).expect("deserialize options");
        assert_eq!(decoded, options);

        for key in [
            COMPONENT_SIZE,
            INTERNAL_DEPENDENCIES,
            FAN_IN,
            FAN_OUT,
            MEAN_FAN_IN,
            MEAN_FAN_OUT,
            STDDEV_FAN_IN,
            STDDEV_FAN_OUT,
            FAN_IN_Z_SCORE,
            FAN_OUT_Z_SCORE,
            EFFERENT_DEPENDENCIES,
            LESS_STABLE_DEPENDENCIES,
            LESS_STABLE_SHARE,
            SOURCE_INSTABILITY,
            SHARE_THRESHOLD,
            NODE_COUNT,
            MEAN_NODE_COUNT,
            STDDEV_NODE_COUNT,
            NODE_COUNT_Z_SCORE,
        ] {
            assert_eq!(evidence_key(key), Some(key));
        }
        assert_eq!(evidence_key("unknown"), None);
        assert!(serde_json::from_str::<ArchSmell>(
            r#"{"kind":"GodComponent","nodes":[1],"severity":1.0,"evidence":{"unknown":1.0}}"#
        )
        .is_err());
        assert!(serde_json::from_str::<ArchSmell>("{").is_err());
    }
}
