//! Robert C. Martin package metrics over caller-selected module boundaries.
//!
//! Coupling is measured on a [`GraphProjection`], while abstractness is read
//! from the CPG type declarations assigned to each module. The implementation
//! is feature-free, deterministic, and read-only.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::{CodePropertyGraph, CpgNode, CpgNodeKind, NodeId};

use super::GraphProjection;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

const ROOT_MODULE: &str = "<root>";
const UNKNOWN_SOURCE: &str = "<unknown-source>";

/// A deterministic assignment of CPG nodes to named module buckets.
///
/// `of_node[id]` indexes `buckets`. Public analyses ignore an entry whose
/// index is outside `buckets`, so deserialized or caller-mutated assignments
/// remain total. Duplicate bucket names denote the same logical module.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ModuleAssignment {
    /// Module names indexed by `of_node` values.
    pub buckets: Vec<Arc<str>>,
    /// Stable node-to-bucket mapping.
    pub of_node: BTreeMap<NodeId, u32>,
}

impl ModuleAssignment {
    /// Builds a canonical assignment from `(node, module-name)` pairs.
    ///
    /// Bucket names are deduplicated and sorted. If a node occurs more than
    /// once with different names, the lexicographically smallest name wins;
    /// consequently, input order cannot change the result.
    pub fn from_pairs<I, S>(pairs: I) -> Self
    where
        I: IntoIterator<Item = (NodeId, S)>,
        S: Into<Arc<str>>,
    {
        let mut module_of_node = BTreeMap::<NodeId, Arc<str>>::new();
        for (node, module) in pairs {
            let module = module.into();
            module_of_node
                .entry(node)
                .and_modify(|current| {
                    if module < *current {
                        *current = module.clone();
                    }
                })
                .or_insert(module);
        }

        let buckets: Vec<_> = module_of_node
            .values()
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let index_by_module: BTreeMap<_, _> = buckets
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, module)| {
                (
                    module,
                    u32::try_from(index).expect("module bucket count fits the u32 node domain"),
                )
            })
            .collect();
        let of_node = module_of_node
            .into_iter()
            .map(|(node, module)| (node, index_by_module[&module]))
            .collect();

        Self { buckets, of_node }
    }

    /// Assigns every CPG node to its nearest enclosing `Module` declaration.
    ///
    /// Nested module names are qualified with `::`. Nodes outside a module are
    /// assigned to `<root>`. Anonymous module names include their stable node
    /// identifier so unrelated declarations are never merged accidentally.
    pub fn by_module_nodes(cpg: &CodePropertyGraph) -> Self {
        Self::from_pairs(
            cpg.nodes()
                .map(|node| (node.id, Arc::<str>::from(qualified_module_name(cpg, node)))),
        )
    }

    /// Assigns every CPG node to the graph's source path.
    ///
    /// A graph without a recorded path uses `<unknown-source>`. An empty graph
    /// produces an empty assignment rather than a zero-member bucket.
    pub fn by_source_path(cpg: &CodePropertyGraph) -> Self {
        let source: Arc<str> = cpg.source_path().unwrap_or(UNKNOWN_SOURCE).into();
        Self::from_pairs(cpg.node_ids().map(|node| (node, source.clone())))
    }

    /// Returns the logical module name assigned to `node`.
    ///
    /// Returns `None` for unassigned nodes and out-of-range bucket indices.
    pub fn module_of(&self, node: NodeId) -> Option<&Arc<str>> {
        let bucket = *self.of_node.get(&node)? as usize;
        self.buckets.get(bucket)
    }
}

/// Robert C. Martin's coupling, instability, abstractness, and distance values.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MartinMetrics {
    /// Logical module name.
    pub module: Arc<str>,
    /// Afferent coupling: distinct external projection nodes depending on this module.
    pub ca: u32,
    /// Efferent coupling: distinct external projection nodes this module depends on.
    pub ce: u32,
    /// Instability `I = Ce / (Ca + Ce)`, with isolated modules defined as zero.
    pub instability: f64,
    /// Abstractness `A`: abstract classes plus traits divided by all type declarations.
    pub abstractness: f64,
    /// Distance from the main sequence, `D = |A + I - 1|`.
    pub distance: f64,
}

/// One module dependency that violates the Stable Dependencies Principle.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SdpViolation {
    /// Depending module, which is more stable than its target.
    pub source_module: Arc<str>,
    /// Depended-on module, which is less stable than its source.
    pub target_module: Arc<str>,
    /// Lexicographically smallest projection-edge source witnessing this module edge.
    pub source: NodeId,
    /// Lexicographically smallest projection-edge target witnessing this module edge.
    pub target: NodeId,
    /// Number of distinct projection edges represented by this module dependency.
    pub dependency_count: u32,
    /// Instability of `source_module`.
    pub source_instability: f64,
    /// Instability of `target_module`.
    pub target_instability: f64,
}

#[derive(Default)]
struct ModuleCounts {
    incoming: BTreeSet<NodeId>,
    outgoing: BTreeSet<NodeId>,
    abstract_types: u32,
    total_types: u32,
}

/// Computes Martin package metrics for every named module in `assignment`.
///
/// Cross-module coupling is structural: projection weights are ignored. `Ca`
/// and `Ce` count distinct external projection nodes, matching Martin's
/// original class-level definition when projection vertices are classes and
/// pgmcp's file-level interpretation when they are files. Abstractness counts
/// abstract `Class` declarations and all `Trait` declarations in the numerator;
/// `Class`, `Struct`, `Enum`, and `Trait` declarations form the denominator.
///
/// Results are sorted by module name. Duplicate names in `buckets` are merged;
/// unassigned nodes, invalid bucket indices, and assignment entries outside
/// the projection/CPG are inert. A malformed projection returns no metrics.
pub fn martin_metrics(
    cpg: &CodePropertyGraph,
    assignment: &ModuleAssignment,
    view: &GraphProjection,
) -> Vec<MartinMetrics> {
    if !view.is_well_formed() {
        return Vec::new();
    }

    let mut counts: BTreeMap<Arc<str>, ModuleCounts> = assignment
        .buckets
        .iter()
        .cloned()
        .map(|module| (module, ModuleCounts::default()))
        .collect();

    for (source, target, _) in view.edges() {
        let source = view.node_id(source);
        let target = view.node_id(target);
        let (Some(source_module), Some(target_module)) =
            (assignment.module_of(source), assignment.module_of(target))
        else {
            continue;
        };
        if source_module == target_module {
            continue;
        }
        counts
            .get_mut(source_module)
            .expect("module_of names are initialized from assignment buckets")
            .outgoing
            .insert(target);
        counts
            .get_mut(target_module)
            .expect("module_of names are initialized from assignment buckets")
            .incoming
            .insert(source);
    }

    for node in cpg.nodes() {
        let Some(module) = assignment.module_of(node.id) else {
            continue;
        };
        let module_counts = counts
            .get_mut(module)
            .expect("module_of names are initialized from assignment buckets");
        match node.kind {
            CpgNodeKind::Class { is_abstract, .. } => {
                module_counts.total_types += 1;
                module_counts.abstract_types += u32::from(is_abstract);
            }
            CpgNodeKind::Trait { .. } => {
                module_counts.total_types += 1;
                module_counts.abstract_types += 1;
            }
            CpgNodeKind::Struct { .. } | CpgNodeKind::Enum { .. } => {
                module_counts.total_types += 1;
            }
            _ => {}
        }
    }

    counts
        .into_iter()
        .map(|(module, counts)| {
            let ca = u32::try_from(counts.incoming.len())
                .expect("afferent coupling fits the u32 projection domain");
            let ce = u32::try_from(counts.outgoing.len())
                .expect("efferent coupling fits the u32 projection domain");
            let total_coupling = f64::from(ca) + f64::from(ce);
            let instability = if total_coupling == 0.0 {
                0.0
            } else {
                f64::from(ce) / total_coupling
            };
            let abstractness = if counts.total_types == 0 {
                0.0
            } else {
                f64::from(counts.abstract_types) / f64::from(counts.total_types)
            };
            MartinMetrics {
                module,
                ca,
                ce,
                instability,
                abstractness,
                distance: (abstractness + instability - 1.0).abs(),
            }
        })
        .collect()
}

/// Finds Stable Dependencies Principle violations in a module dependency view.
///
/// A module edge `X -> Y` violates the principle exactly when
/// `I(X) < I(Y)`: a more stable source depends on a less stable target. One
/// result is returned per module pair, ordered by `(source_module,
/// target_module)`, with the smallest stable node-edge witness. Projection
/// weights are ignored.
///
/// A malformed projection, duplicate metric name, non-finite/out-of-range
/// instability, missing metric, invalid assignment entry, or unassigned node
/// is handled conservatively: malformed metric collections yield no verdict;
/// individual unmapped edges are ignored.
pub fn sdp_violations(
    metrics: &[MartinMetrics],
    view: &GraphProjection,
    assignment: &ModuleAssignment,
) -> Vec<SdpViolation> {
    if !view.is_well_formed() {
        return Vec::new();
    }

    let mut instability = BTreeMap::<Arc<str>, f64>::new();
    for metric in metrics {
        if !metric.instability.is_finite()
            || !(0.0..=1.0).contains(&metric.instability)
            || instability
                .insert(metric.module.clone(), metric.instability)
                .is_some()
        {
            return Vec::new();
        }
    }

    let mut violations = BTreeMap::<(Arc<str>, Arc<str>), (NodeId, NodeId, u32, f64, f64)>::new();
    for (source, target, _) in view.edges() {
        let source = view.node_id(source);
        let target = view.node_id(target);
        let (Some(source_module), Some(target_module)) =
            (assignment.module_of(source), assignment.module_of(target))
        else {
            continue;
        };
        if source_module == target_module {
            continue;
        }
        let (Some(&source_instability), Some(&target_instability)) = (
            instability.get(source_module),
            instability.get(target_module),
        ) else {
            continue;
        };
        if source_instability >= target_instability {
            continue;
        }

        violations
            .entry((source_module.clone(), target_module.clone()))
            .and_modify(|entry| entry.2 += 1)
            .or_insert((source, target, 1, source_instability, target_instability));
    }

    violations
        .into_iter()
        .map(
            |(
                (source_module, target_module),
                (source, target, dependency_count, source_instability, target_instability),
            )| SdpViolation {
                source_module,
                target_module,
                source,
                target,
                dependency_count,
                source_instability,
                target_instability,
            },
        )
        .collect()
}

fn qualified_module_name(cpg: &CodePropertyGraph, node: &CpgNode) -> String {
    let mut modules = Vec::<(NodeId, Arc<str>)>::new();
    if let CpgNodeKind::Module { name } = &node.kind {
        modules.push((node.id, name.clone()));
    }
    for ancestor in cpg.ast_ancestors(node.id) {
        let ancestor_node = cpg
            .node(ancestor)
            .expect("ast_ancestors yields only existing CPG nodes");
        if let CpgNodeKind::Module { name } = &ancestor_node.kind {
            modules.push((ancestor, name.clone()));
        }
    }
    modules.reverse();
    if modules.is_empty() {
        return ROOT_MODULE.to_owned();
    }
    modules
        .into_iter()
        .map(|(id, name)| {
            if name.is_empty() {
                format!("<anonymous@{}>", id.as_u32())
            } else {
                name.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("::")
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::{CpgNode, Language, ProjectionKind, SourceRange};

    fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(id), kind, SourceRange::default())
    }

    fn type_fixture() -> (CodePropertyGraph, ModuleAssignment, GraphProjection) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        for (id, kind) in [
            (10, CpgNodeKind::Module { name: "A".into() }),
            (20, CpgNodeKind::Module { name: "B".into() }),
            (30, CpgNodeKind::Module { name: "C".into() }),
            (11, CpgNodeKind::Trait { name: "Api".into() }),
            (
                21,
                CpgNodeKind::Class {
                    name: "Concrete".into(),
                    is_abstract: false,
                },
            ),
            (
                31,
                CpgNodeKind::Class {
                    name: "Base".into(),
                    is_abstract: true,
                },
            ),
            (
                32,
                CpgNodeKind::Struct {
                    name: "Data".into(),
                },
            ),
        ] {
            cpg.add_node_with_id(node(id, kind));
        }
        let assignment = ModuleAssignment::from_pairs([
            (NodeId::new(10), "A"),
            (NodeId::new(11), "A"),
            (NodeId::new(20), "B"),
            (NodeId::new(21), "B"),
            (NodeId::new(30), "C"),
            (NodeId::new(31), "C"),
            (NodeId::new(32), "C"),
        ]);
        let view = GraphProjection::from_edges(
            ProjectionKind::External,
            [NodeId::new(10), NodeId::new(20), NodeId::new(30)],
            [
                (NodeId::new(10), NodeId::new(20), 1.0),
                (NodeId::new(10), NodeId::new(30), 1.0),
                (NodeId::new(20), NodeId::new(30), 1.0),
                (NodeId::new(30), NodeId::new(10), 1.0),
            ],
        );
        (cpg, assignment, view)
    }

    #[test]
    fn three_module_oracle_has_exact_coupling_abstraction_distance_and_sdp() {
        let (cpg, assignment, view) = type_fixture();
        let metrics = martin_metrics(&cpg, &assignment, &view);
        assert_eq!(metrics.len(), 3);
        assert_eq!(metrics[0].module.as_ref(), "A");
        assert_eq!((metrics[0].ca, metrics[0].ce), (1, 2));
        assert_eq!(metrics[0].instability, 2.0 / 3.0);
        assert_eq!(metrics[0].abstractness, 1.0);
        assert!((metrics[0].distance - 2.0 / 3.0).abs() < 1e-12);

        assert_eq!(metrics[1].module.as_ref(), "B");
        assert_eq!((metrics[1].ca, metrics[1].ce), (1, 1));
        assert_eq!(metrics[1].instability, 0.5);
        assert_eq!(metrics[1].abstractness, 0.0);
        assert_eq!(metrics[1].distance, 0.5);

        assert_eq!(metrics[2].module.as_ref(), "C");
        assert_eq!((metrics[2].ca, metrics[2].ce), (2, 1));
        assert_eq!(metrics[2].instability, 1.0 / 3.0);
        assert_eq!(metrics[2].abstractness, 0.5);
        assert!((metrics[2].distance - 1.0 / 6.0).abs() < 1e-12);

        assert_eq!(
            sdp_violations(&metrics, &view, &assignment),
            vec![SdpViolation {
                source_module: "C".into(),
                target_module: "A".into(),
                source: NodeId::new(30),
                target: NodeId::new(10),
                dependency_count: 1,
                source_instability: 1.0 / 3.0,
                target_instability: 2.0 / 3.0,
            }]
        );
    }

    #[test]
    fn module_and_source_assigners_are_complete_qualified_and_canonical() {
        let mut cpg = CodePropertyGraph::new(Language::Rust).with_source_path("src/lib.rs");
        cpg.add_node_with_id(node(1, CpgNodeKind::Root));
        cpg.add_node_with_id(node(4, CpgNodeKind::Struct { name: "Top".into() }));
        cpg.add_node_with_id(node(
            7,
            CpgNodeKind::Module {
                name: "outer".into(),
            },
        ));
        cpg.add_node_with_id(
            node(9, CpgNodeKind::Module { name: "".into() }).with_parent(NodeId::new(7)),
        );
        cpg.add_node_with_id(
            node(12, CpgNodeKind::Trait { name: "T".into() }).with_parent(NodeId::new(9)),
        );

        let modules = ModuleAssignment::by_module_nodes(&cpg);
        assert_eq!(
            modules.module_of(NodeId::new(1)).map(AsRef::as_ref),
            Some("<root>")
        );
        assert_eq!(
            modules.module_of(NodeId::new(4)).map(AsRef::as_ref),
            Some("<root>")
        );
        assert_eq!(
            modules.module_of(NodeId::new(7)).map(AsRef::as_ref),
            Some("outer")
        );
        assert_eq!(
            modules.module_of(NodeId::new(9)).map(AsRef::as_ref),
            Some("outer::<anonymous@9>")
        );
        assert_eq!(
            modules.module_of(NodeId::new(12)).map(AsRef::as_ref),
            Some("outer::<anonymous@9>")
        );

        let source = ModuleAssignment::by_source_path(&cpg);
        assert_eq!(source.buckets, vec![Arc::<str>::from("src/lib.rs")]);
        assert_eq!(source.of_node.len(), cpg.node_count());
        assert!(source
            .of_node
            .keys()
            .all(|id| source.module_of(*id).is_some()));
    }

    #[test]
    fn duplicates_invalid_entries_intra_edges_and_malformed_inputs_are_total() {
        let assignment = ModuleAssignment::from_pairs([
            (NodeId::new(3), "z"),
            (NodeId::new(3), "a"),
            (NodeId::new(2), "z"),
        ]);
        assert_eq!(
            assignment.buckets,
            vec![Arc::<str>::from("a"), Arc::from("z")]
        );
        assert_eq!(
            assignment.module_of(NodeId::new(3)).map(AsRef::as_ref),
            Some("a")
        );

        let mut invalid_assignment = assignment.clone();
        invalid_assignment.of_node.insert(NodeId::new(99), u32::MAX);
        assert!(invalid_assignment.module_of(NodeId::new(99)).is_none());

        let mut cpg = CodePropertyGraph::new(Language::Rust);
        cpg.add_node_with_id(node(
            70,
            CpgNodeKind::Struct {
                name: "Unassigned".into(),
            },
        ));
        let view = GraphProjection::from_edges(
            ProjectionKind::External,
            [NodeId::new(2), NodeId::new(3), NodeId::new(99)],
            [
                (NodeId::new(2), NodeId::new(2), 4.0),
                (NodeId::new(2), NodeId::new(3), 2.0),
                (NodeId::new(99), NodeId::new(2), 1.0),
            ],
        );
        let metrics = martin_metrics(&cpg, &invalid_assignment, &view);
        assert_eq!(metrics.len(), 2);
        assert_eq!((metrics[0].ca, metrics[0].ce), (1, 0));
        assert_eq!((metrics[1].ca, metrics[1].ce), (0, 1));

        let mut malformed = view.clone();
        malformed.offsets.clear();
        assert!(martin_metrics(&cpg, &assignment, &malformed).is_empty());
        assert!(sdp_violations(&metrics, &malformed, &assignment).is_empty());

        let duplicate_metrics = [metrics[0].clone(), metrics[0].clone()];
        assert!(sdp_violations(&duplicate_metrics, &view, &assignment).is_empty());
        let mut nonfinite = metrics.clone();
        nonfinite[0].instability = f64::NAN;
        assert!(sdp_violations(&nonfinite, &view, &assignment).is_empty());

        assert!(sdp_violations(&metrics, &view, &invalid_assignment).is_empty());
        assert!(sdp_violations(&metrics[..1], &view, &assignment).is_empty());

        let grouped_assignment = ModuleAssignment::from_pairs([
            (NodeId::new(1), "stable"),
            (NodeId::new(2), "stable"),
            (NodeId::new(3), "stable"),
            (NodeId::new(4), "stable"),
            (NodeId::new(5), "stable"),
            (NodeId::new(6), "stable"),
            (NodeId::new(10), "unstable"),
            (NodeId::new(11), "unstable"),
            (NodeId::new(12), "unstable"),
            (NodeId::new(13), "unstable"),
            (NodeId::new(14), "unstable"),
            (NodeId::new(15), "unstable"),
        ]);
        let grouped_view = GraphProjection::from_edges(
            ProjectionKind::External,
            [
                NodeId::new(1),
                NodeId::new(2),
                NodeId::new(3),
                NodeId::new(4),
                NodeId::new(5),
                NodeId::new(6),
                NodeId::new(10),
                NodeId::new(11),
                NodeId::new(12),
                NodeId::new(13),
                NodeId::new(14),
                NodeId::new(15),
            ],
            [
                (NodeId::new(2), NodeId::new(11), 2.0),
                (NodeId::new(1), NodeId::new(10), 1.0),
                (NodeId::new(12), NodeId::new(3), 1.0),
                (NodeId::new(13), NodeId::new(4), 1.0),
                (NodeId::new(14), NodeId::new(5), 1.0),
                (NodeId::new(15), NodeId::new(6), 1.0),
            ],
        );
        let grouped_metrics = [
            MartinMetrics {
                module: "stable".into(),
                ca: 4,
                ce: 2,
                instability: 1.0 / 3.0,
                abstractness: 1.0,
                distance: 1.0 / 3.0,
            },
            MartinMetrics {
                module: "unstable".into(),
                ca: 2,
                ce: 4,
                instability: 2.0 / 3.0,
                abstractness: 0.0,
                distance: 1.0 / 3.0,
            },
        ];
        let grouped = sdp_violations(&grouped_metrics, &grouped_view, &grouped_assignment);
        assert_eq!(grouped.len(), 1);
        assert_eq!(grouped[0].dependency_count, 2);
        assert_eq!(
            (grouped[0].source, grouped[0].target),
            (NodeId::new(1), NodeId::new(10))
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn generated_metrics_are_deterministic_bounded_and_edge_order_invariant(
            node_count in 0usize..=16,
            modules in prop::collection::vec(0u8..4, 16),
            abstract_flags in prop::collection::vec(any::<bool>(), 16),
            raw_edges in prop::collection::vec((0usize..16, 0usize..16, 1u8..=5), 0..96),
        ) {
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            let mut pairs = Vec::new();
            for dense in 0..node_count {
                let id = NodeId::new((dense * 13 + 5) as u32);
                let kind = if dense % 2 == 0 {
                    CpgNodeKind::Class {
                        name: format!("C{dense}").into(),
                        is_abstract: abstract_flags[dense],
                    }
                } else {
                    CpgNodeKind::Struct { name: format!("S{dense}").into() }
                };
                cpg.add_node_with_id(node(id.as_u32(), kind));
                pairs.push((id, Arc::<str>::from(format!("m{}", modules[dense]))));
            }
            let assignment = ModuleAssignment::from_pairs(pairs.clone());
            pairs.reverse();
            prop_assert_eq!(assignment.clone(), ModuleAssignment::from_pairs(pairs));

            let nodes: Vec<_> = cpg.node_ids().collect();
            let edges: Vec<_> = raw_edges
                .iter()
                .filter_map(|&(source, target, weight)| {
                    (source < node_count && target < node_count).then_some((
                        NodeId::new((source * 13 + 5) as u32),
                        NodeId::new((target * 13 + 5) as u32),
                        f64::from(weight),
                    ))
                })
                .collect();
            let mut reversed_edges = edges.clone();
            reversed_edges.reverse();
            let first = GraphProjection::from_edges(ProjectionKind::External, nodes.clone(), edges);
            let second = GraphProjection::from_edges(ProjectionKind::External, nodes, reversed_edges);
            let first_metrics = martin_metrics(&cpg, &assignment, &first);
            let second_metrics = martin_metrics(&cpg, &assignment, &second);

            prop_assert_eq!(&first_metrics, &second_metrics);
            prop_assert_eq!(sdp_violations(&first_metrics, &first, &assignment), sdp_violations(&second_metrics, &second, &assignment));
            for metric in &first_metrics {
                prop_assert!((0.0..=1.0).contains(&metric.instability));
                prop_assert!((0.0..=1.0).contains(&metric.abstractness));
                prop_assert!((0.0..=1.0).contains(&metric.distance));
                if metric.ca == 0 && metric.ce == 0 {
                    prop_assert_eq!(metric.instability, 0.0);
                }
            }
            for violation in sdp_violations(&first_metrics, &first, &assignment) {
                prop_assert!(violation.source_instability < violation.target_instability);
                prop_assert!(violation.dependency_count > 0);
            }
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn public_values_round_trip_through_json() {
        let (cpg, assignment, view) = type_fixture();
        let metrics = martin_metrics(&cpg, &assignment, &view);
        let violations = sdp_violations(&metrics, &view, &assignment);
        assert_eq!(
            serde_json::from_str::<ModuleAssignment>(
                &serde_json::to_string(&assignment).expect("serialize assignment")
            )
            .expect("deserialize assignment"),
            assignment
        );
        assert_eq!(
            serde_json::from_str::<Vec<MartinMetrics>>(
                &serde_json::to_string(&metrics).expect("serialize metrics")
            )
            .expect("deserialize metrics"),
            metrics
        );
        assert_eq!(
            serde_json::from_str::<Vec<SdpViolation>>(
                &serde_json::to_string(&violations).expect("serialize violations")
            )
            .expect("deserialize violations"),
            violations
        );
    }
}
