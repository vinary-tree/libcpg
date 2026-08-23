//! Advisory information-flow and constant-time evidence over represented PDGs.
//!
//! Dependence edges point from an influence source to an influenced node.
//! Therefore non-interference and timing checks are deterministic forward
//! reachability problems. Declassification is modeled as a cut: traversal does
//! not enter a sanctioned declassifier, so no path can silently resume beyond
//! it. All graph state, queues, parents, and witnesses are heap resident.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::{CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNodeKind, NodeId};

use super::super::sdg::SUMMARY_EDGE_LABEL;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Caller-owned node labels for a two-level confidentiality policy.
///
/// A node may be a high source, a low sink, both, or neither. Declassifiers are
/// sanctioned cut vertices: a represented dependence path that reaches one is
/// not followed further. Labels that name nodes outside `cpg` are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SecurityLabels {
    /// Confidential influence sources.
    pub high_sources: BTreeSet<NodeId>,
    /// Public observations that must not depend on a high source.
    pub low_sinks: BTreeSet<NodeId>,
    /// Sanctioned dependence-graph cut vertices.
    pub declassifiers: BTreeSet<NodeId>,
}

impl SecurityLabels {
    /// Creates a policy with no declassification cuts.
    pub fn new(
        high_sources: impl IntoIterator<Item = NodeId>,
        low_sinks: impl IntoIterator<Item = NodeId>,
    ) -> Self {
        Self {
            high_sources: high_sources.into_iter().collect(),
            low_sinks: low_sinks.into_iter().collect(),
            declassifiers: BTreeSet::new(),
        }
    }

    /// Adds one sanctioned declassification cut vertex.
    pub fn with_declassifier(mut self, node: NodeId) -> Self {
        self.declassifiers.insert(node);
        self
    }
}

/// Dependence-edge class retained in a replayable witness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum InformationFlowEdgeKind {
    /// Ordinary PDG data dependence.
    DataDependence,
    /// Caller-local SDG summary dependence.
    Summary,
    /// PDG control dependence, representing an implicit flow.
    ControlDependence,
}

/// One directed step in a canonical dependence witness.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct InformationFlowStep {
    /// Influence source for this step.
    pub source: NodeId,
    /// Influenced target for this step.
    pub target: NodeId,
    /// Represented dependence class.
    pub kind: InformationFlowEdgeKind,
}

/// A shortest, stable-id-tiebroken dependence path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct InformationFlowWitness {
    /// Ordered source-to-target steps. An empty witness denotes the same node.
    pub steps: Vec<InformationFlowStep>,
}

/// Which kinds of represented paths connect one high source to one low sink.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum FlowViolationKind {
    /// At least one path uses data/summary dependences only.
    Explicit,
    /// At least one path contains a control dependence.
    Implicit,
    /// Both an explicit path and an implicit path are represented.
    ExplicitAndImplicit,
}

/// Advisory evidence that a low observation depends on a high source.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FlowViolation {
    /// Confidential source.
    pub source: NodeId,
    /// Public observation.
    pub sink: NodeId,
    /// Classes of represented paths between `source` and `sink`.
    pub kind: FlowViolationKind,
    /// Canonical shortest path with no control-dependence step, when one exists.
    pub explicit_witness: Option<InformationFlowWitness>,
    /// Canonical shortest path containing a control-dependence step, when one
    /// exists.
    pub implicit_witness: Option<InformationFlowWitness>,
}

/// Timing-observable operation reached from one secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum LeakSiteKind {
    /// A secret-dependent true successor.
    ConditionalTrue {
        /// CFG target selected by the conditional edge.
        target: NodeId,
    },
    /// A secret-dependent false successor.
    ConditionalFalse {
        /// CFG target selected by the conditional edge.
        target: NodeId,
    },
    /// A secret-dependent array or collection index expression.
    IndexAccess,
}

/// Advisory constant-time violation evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LeakSite {
    /// Secret whose represented influence reaches the site.
    pub secret: NodeId,
    /// Branch controller or index-access node.
    pub site: NodeId,
    /// Timing-observable mechanism.
    pub kind: LeakSiteKind,
    /// Canonical shortest represented dependence path.
    pub witness: InformationFlowWitness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ProjectionEdge {
    target: usize,
    kind: InformationFlowEdgeKind,
}

struct InfluenceProjection {
    nodes: Vec<NodeId>,
    index: BTreeMap<NodeId, usize>,
    outgoing: Vec<Vec<ProjectionEdge>>,
}

impl InfluenceProjection {
    fn new(cpg: &CodePropertyGraph) -> Self {
        let mut nodes: Vec<_> = cpg.node_ids().collect();
        nodes.sort_unstable();
        let index: BTreeMap<_, _> = nodes
            .iter()
            .copied()
            .enumerate()
            .map(|(dense, node)| (node, dense))
            .collect();
        let mut outgoing = vec![Vec::new(); nodes.len()];

        for edge in cpg.edges() {
            let kind = match edge.kind {
                CpgEdgeKind::ControlDependence => InformationFlowEdgeKind::ControlDependence,
                CpgEdgeKind::DataDependence
                    if edge.label.as_deref() == Some(SUMMARY_EDGE_LABEL) =>
                {
                    InformationFlowEdgeKind::Summary
                }
                CpgEdgeKind::DataDependence => InformationFlowEdgeKind::DataDependence,
                _ => continue,
            };
            let (Some(&source), Some(&target)) = (index.get(&edge.source), index.get(&edge.target))
            else {
                continue;
            };
            outgoing[source].push(ProjectionEdge { target, kind });
        }
        for row in &mut outgoing {
            row.sort_unstable_by_key(|edge| (nodes[edge.target], edge.kind));
            row.dedup();
        }

        Self {
            nodes,
            index,
            outgoing,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FlowState {
    node: usize,
    implicit: bool,
}

#[derive(Debug, Clone, Copy)]
struct FlowParent {
    previous: FlowState,
    edge: InformationFlowEdgeKind,
}

struct FlowSearch {
    visited: Vec<bool>,
    parents: Vec<Option<FlowParent>>,
}

impl FlowSearch {
    fn offset(state: FlowState) -> usize {
        state.node * 2 + usize::from(state.implicit)
    }

    fn contains(&self, state: FlowState) -> bool {
        self.visited[Self::offset(state)]
    }

    fn parent(&self, state: FlowState) -> Option<FlowParent> {
        self.parents[Self::offset(state)]
    }
}

/// Finds every represented high-to-low PDG/summary dependence.
///
/// The function is feature-free, deterministic, non-mutating, and advisory.
/// It reports at most one explicit and one implicit shortest witness for each
/// source/sink pair. No semantic result cap is applied; runtime is
/// $`O(|H|(|V|+|E|)+|W|)`$, where `H` is the represented high-source set
/// and `W` is the total emitted witness length.
pub fn noninterference(cpg: &CodePropertyGraph, labels: &SecurityLabels) -> Vec<FlowViolation> {
    let projection = InfluenceProjection::new(cpg);
    let blocked: BTreeSet<usize> = labels
        .declassifiers
        .iter()
        .filter_map(|node| projection.index.get(node).copied())
        .collect();
    let sinks: Vec<(NodeId, usize)> = labels
        .low_sinks
        .iter()
        .filter_map(|&node| {
            projection
                .index
                .get(&node)
                .copied()
                .filter(|dense| !blocked.contains(dense))
                .map(|dense| (node, dense))
        })
        .collect();
    let mut violations = Vec::new();

    for &source in &labels.high_sources {
        let Some(&source_dense) = projection.index.get(&source) else {
            continue;
        };
        if blocked.contains(&source_dense) {
            continue;
        }
        let search = flow_search(&projection, source_dense, &blocked);
        for &(sink, sink_dense) in &sinks {
            let explicit_state = FlowState {
                node: sink_dense,
                implicit: false,
            };
            let implicit_state = FlowState {
                node: sink_dense,
                implicit: true,
            };
            let has_explicit = search.contains(explicit_state);
            let has_implicit = search.contains(implicit_state);
            if !has_explicit && !has_implicit {
                continue;
            }
            violations.push(FlowViolation {
                source,
                sink,
                kind: match (has_explicit, has_implicit) {
                    (true, true) => FlowViolationKind::ExplicitAndImplicit,
                    (true, false) => FlowViolationKind::Explicit,
                    (false, true) => FlowViolationKind::Implicit,
                    (false, false) => continue,
                },
                explicit_witness: has_explicit
                    .then(|| {
                        reconstruct_flow_witness(&projection, &search, source_dense, explicit_state)
                    })
                    .flatten(),
                implicit_witness: has_implicit
                    .then(|| {
                        reconstruct_flow_witness(&projection, &search, source_dense, implicit_state)
                    })
                    .flatten(),
            });
        }
    }
    violations
}

fn flow_search(
    projection: &InfluenceProjection,
    source: usize,
    blocked: &BTreeSet<usize>,
) -> FlowSearch {
    let start = FlowState {
        node: source,
        implicit: false,
    };
    let mut search = FlowSearch {
        visited: vec![false; projection.nodes.len().saturating_mul(2)],
        parents: vec![None; projection.nodes.len().saturating_mul(2)],
    };
    search.visited[FlowSearch::offset(start)] = true;
    let mut queue = VecDeque::from([start]);

    while let Some(state) = queue.pop_front() {
        for edge in &projection.outgoing[state.node] {
            if blocked.contains(&edge.target) {
                continue;
            }
            let next = FlowState {
                node: edge.target,
                implicit: state.implicit || edge.kind == InformationFlowEdgeKind::ControlDependence,
            };
            let offset = FlowSearch::offset(next);
            if search.visited[offset] {
                continue;
            }
            search.visited[offset] = true;
            search.parents[offset] = Some(FlowParent {
                previous: state,
                edge: edge.kind,
            });
            queue.push_back(next);
        }
    }
    search
}

fn reconstruct_flow_witness(
    projection: &InfluenceProjection,
    search: &FlowSearch,
    source: usize,
    mut current: FlowState,
) -> Option<InformationFlowWitness> {
    let start = FlowState {
        node: source,
        implicit: false,
    };
    let mut reversed = Vec::new();
    while current != start {
        let parent = search.parent(current)?;
        reversed.push(InformationFlowStep {
            source: projection.nodes[parent.previous.node],
            target: projection.nodes[current.node],
            kind: parent.edge,
        });
        current = parent.previous;
    }
    reversed.reverse();
    Some(InformationFlowWitness { steps: reversed })
}

#[derive(Debug, Clone, Copy)]
struct SimpleParent {
    previous: usize,
    edge: InformationFlowEdgeKind,
}

/// Finds represented secret-dependent branches and index accesses.
///
/// Each conditional CFG edge is reported separately, so a conventional binary
/// branch yields true- and false-edge findings with the same controller. The
/// function does not treat declassification as a constant-time defense:
/// intentionally releasing a value does not make secret-dependent timing safe.
pub fn constant_time(cpg: &CodePropertyGraph, secrets: &BTreeSet<NodeId>) -> Vec<LeakSite> {
    let projection = InfluenceProjection::new(cpg);
    let sites = timing_sites(cpg, &projection);
    let mut findings = Vec::new();

    for &secret in secrets {
        let Some(&source) = projection.index.get(&secret) else {
            continue;
        };
        let parents = simple_search(&projection, source);
        for (&site, kinds) in &sites {
            let Some(&site_dense) = projection.index.get(&site) else {
                continue;
            };
            if site_dense != source && parents[site_dense].is_none() {
                continue;
            }
            let Some(witness) =
                reconstruct_simple_witness(&projection, &parents, source, site_dense)
            else {
                continue;
            };
            for &kind in kinds {
                findings.push(LeakSite {
                    secret,
                    site,
                    kind,
                    witness: witness.clone(),
                });
            }
        }
    }
    findings
}

fn timing_sites(
    cpg: &CodePropertyGraph,
    projection: &InfluenceProjection,
) -> BTreeMap<NodeId, Vec<LeakSiteKind>> {
    let mut sites: BTreeMap<NodeId, Vec<LeakSiteKind>> = BTreeMap::new();
    for &node in &projection.nodes {
        if cpg
            .node(node)
            .is_some_and(|value| matches!(value.kind, CpgNodeKind::IndexAccess))
        {
            sites
                .entry(node)
                .or_default()
                .push(LeakSiteKind::IndexAccess);
        }
    }
    for edge in cpg.edges() {
        let kind = match edge.kind {
            CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue) => {
                LeakSiteKind::ConditionalTrue {
                    target: edge.target,
                }
            }
            CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalFalse) => {
                LeakSiteKind::ConditionalFalse {
                    target: edge.target,
                }
            }
            _ => continue,
        };
        if projection.index.contains_key(&edge.source)
            && projection.index.contains_key(&edge.target)
        {
            sites.entry(edge.source).or_default().push(kind);
        }
    }
    for kinds in sites.values_mut() {
        kinds.sort_unstable();
        kinds.dedup();
    }
    sites
}

fn simple_search(projection: &InfluenceProjection, source: usize) -> Vec<Option<SimpleParent>> {
    let mut parents = vec![None; projection.nodes.len()];
    let mut visited = vec![false; projection.nodes.len()];
    visited[source] = true;
    let mut queue = VecDeque::from([source]);
    while let Some(node) = queue.pop_front() {
        for edge in &projection.outgoing[node] {
            if visited[edge.target] {
                continue;
            }
            visited[edge.target] = true;
            parents[edge.target] = Some(SimpleParent {
                previous: node,
                edge: edge.kind,
            });
            queue.push_back(edge.target);
        }
    }
    parents
}

fn reconstruct_simple_witness(
    projection: &InfluenceProjection,
    parents: &[Option<SimpleParent>],
    source: usize,
    mut current: usize,
) -> Option<InformationFlowWitness> {
    let mut reversed = Vec::new();
    while current != source {
        let parent = parents.get(current).copied().flatten()?;
        reversed.push(InformationFlowStep {
            source: projection.nodes[parent.previous],
            target: projection.nodes[current],
            kind: parent.edge,
        });
        current = parent.previous;
    }
    reversed.reverse();
    Some(InformationFlowWitness { steps: reversed })
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::{CpgEdge, CpgNode, EdgeId, Language, SourceRange};

    fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(id), kind, SourceRange::from_bytes(id, id + 1))
    }

    fn opaque(name: &str) -> CpgNodeKind {
        CpgNodeKind::Unknown { kind: name.into() }
    }

    fn graph(kinds: impl IntoIterator<Item = CpgNodeKind>) -> CodePropertyGraph {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        for (index, kind) in kinds.into_iter().enumerate() {
            cpg.add_node_with_id(node(index as u32, kind));
        }
        cpg
    }

    #[test]
    fn explicit_and_implicit_paths_have_distinct_replayable_witnesses() {
        let mut cpg = graph((0..7).map(|index| opaque(&format!("n{index}"))));
        cpg.connect(NodeId::new(0), NodeId::new(1), CpgEdgeKind::DataDependence);
        cpg.connect(NodeId::new(1), NodeId::new(2), CpgEdgeKind::DataDependence);
        cpg.connect(NodeId::new(3), NodeId::new(4), CpgEdgeKind::DataDependence);
        cpg.connect(
            NodeId::new(4),
            NodeId::new(5),
            CpgEdgeKind::ControlDependence,
        );
        cpg.connect(NodeId::new(3), NodeId::new(6), CpgEdgeKind::DataDependence);
        cpg.connect(NodeId::new(6), NodeId::new(5), CpgEdgeKind::DataDependence);
        let labels = SecurityLabels::new(
            [NodeId::new(0), NodeId::new(3)],
            [NodeId::new(2), NodeId::new(5)],
        );

        let violations = noninterference(&cpg, &labels);
        assert_eq!(violations.len(), 2);
        assert_eq!(violations[0].kind, FlowViolationKind::Explicit);
        assert_eq!(
            violations[0]
                .explicit_witness
                .as_ref()
                .map(|witness| witness.steps.len()),
            Some(2)
        );
        assert_eq!(violations[1].kind, FlowViolationKind::ExplicitAndImplicit);
        assert!(violations[1]
            .implicit_witness
            .as_ref()
            .is_some_and(|witness| witness
                .steps
                .iter()
                .any(|step| step.kind == InformationFlowEdgeKind::ControlDependence)));
    }

    #[test]
    fn declassifier_is_a_cut_and_missing_labels_are_total() {
        let mut cpg = graph((0..3).map(|index| opaque(&format!("n{index}"))));
        cpg.connect(NodeId::new(0), NodeId::new(1), CpgEdgeKind::DataDependence);
        cpg.connect(NodeId::new(1), NodeId::new(2), CpgEdgeKind::DataDependence);
        let labels = SecurityLabels::new(
            [NodeId::new(0), NodeId::new(99)],
            [NodeId::new(2), NodeId::new(100)],
        )
        .with_declassifier(NodeId::new(1));
        assert!(noninterference(&cpg, &labels).is_empty());
        assert!(constant_time(&cpg, &BTreeSet::from([NodeId::new(99)])).is_empty());
    }

    #[test]
    fn summary_edges_are_explicit_interprocedural_evidence() {
        let mut cpg = graph((0..2).map(|index| opaque(&format!("n{index}"))));
        cpg.connect_unique_labeled(
            NodeId::new(0),
            NodeId::new(1),
            CpgEdgeKind::DataDependence,
            SUMMARY_EDGE_LABEL,
        );
        let violations = noninterference(
            &cpg,
            &SecurityLabels::new([NodeId::new(0)], [NodeId::new(1)]),
        );
        assert_eq!(violations.len(), 1);
        assert_eq!(
            violations[0].explicit_witness.as_ref().unwrap().steps[0].kind,
            InformationFlowEdgeKind::Summary
        );
    }

    #[test]
    fn constant_time_reports_each_conditional_edge_and_index_site() {
        let mut cpg = graph([
            opaque("secret"),
            opaque("guard"),
            opaque("true"),
            opaque("false"),
            CpgNodeKind::IndexAccess,
        ]);
        cpg.connect(NodeId::new(0), NodeId::new(1), CpgEdgeKind::DataDependence);
        cpg.connect(NodeId::new(0), NodeId::new(4), CpgEdgeKind::DataDependence);
        cpg.connect(
            NodeId::new(1),
            NodeId::new(2),
            CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue),
        );
        cpg.connect(
            NodeId::new(1),
            NodeId::new(3),
            CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalFalse),
        );

        let findings = constant_time(&cpg, &BTreeSet::from([NodeId::new(0)]));
        assert_eq!(
            findings
                .iter()
                .map(|finding| (finding.site, finding.kind))
                .collect::<Vec<_>>(),
            vec![
                (
                    NodeId::new(1),
                    LeakSiteKind::ConditionalTrue {
                        target: NodeId::new(2),
                    },
                ),
                (
                    NodeId::new(1),
                    LeakSiteKind::ConditionalFalse {
                        target: NodeId::new(3),
                    },
                ),
                (NodeId::new(4), LeakSiteKind::IndexAccess),
            ]
        );
        assert!(findings
            .iter()
            .all(|finding| finding.witness.steps.len() == 1));
    }

    fn add_generated_edges(cpg: &mut CodePropertyGraph, edges: &[(u32, u32, bool)], reverse: bool) {
        if reverse {
            for (id, &edge) in edges.iter().enumerate().rev() {
                add_generated_edge(cpg, id, edge);
            }
        } else {
            for (id, &edge) in edges.iter().enumerate() {
                add_generated_edge(cpg, id, edge);
            }
        }
    }

    fn add_generated_edge(
        cpg: &mut CodePropertyGraph,
        id: usize,
        (source, target, control): (u32, u32, bool),
    ) {
        cpg.add_edge_with_id(CpgEdge::new(
            EdgeId::new(id as u32),
            NodeId::new(source),
            NodeId::new(target),
            if control {
                CpgEdgeKind::ControlDependence
            } else {
                CpgEdgeKind::DataDependence
            },
        ));
    }

    fn oracle_pairs(
        count: usize,
        edges: &[(u32, u32, bool)],
        labels: &SecurityLabels,
    ) -> BTreeMap<(NodeId, NodeId), (bool, bool)> {
        let mut result = BTreeMap::new();
        for &source in &labels.high_sources {
            if source.as_u32() as usize >= count || labels.declassifiers.contains(&source) {
                continue;
            }
            let mut reached = vec![[false; 2]; count];
            reached[source.as_u32() as usize][0] = true;
            loop {
                let mut changed = false;
                for &(from, to, control) in edges {
                    if labels.declassifiers.contains(&NodeId::new(to)) {
                        continue;
                    }
                    for implicit in [false, true] {
                        if reached[from as usize][usize::from(implicit)] {
                            let next = implicit || control;
                            if !reached[to as usize][usize::from(next)] {
                                reached[to as usize][usize::from(next)] = true;
                                changed = true;
                            }
                        }
                    }
                }
                if !changed {
                    break;
                }
            }
            for &sink in &labels.low_sinks {
                if sink.as_u32() as usize >= count || labels.declassifiers.contains(&sink) {
                    continue;
                }
                let state = reached[sink.as_u32() as usize];
                if state[0] || state[1] {
                    result.insert((source, sink), (state[0], state[1]));
                }
            }
        }
        result
    }

    fn violation_pairs(violations: &[FlowViolation]) -> BTreeMap<(NodeId, NodeId), (bool, bool)> {
        violations
            .iter()
            .map(|finding| {
                (
                    (finding.source, finding.sink),
                    (
                        finding.explicit_witness.is_some(),
                        finding.implicit_witness.is_some(),
                    ),
                )
            })
            .collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn generated_noninterference_matches_fixed_point_and_permutations(
            count in 1usize..8,
            raw_edges in prop::collection::vec((0u8..8, 0u8..8, any::<bool>()), 0..40),
            high_mask in any::<u8>(),
            low_mask in any::<u8>(),
            declass_mask in any::<u8>(),
        ) {
            let edges: Vec<_> = raw_edges
                .into_iter()
                .map(|(source, target, control)| {
                    (
                        u32::from(source) % count as u32,
                        u32::from(target) % count as u32,
                        control,
                    )
                })
                .collect();
            let mut forward = graph((0..count).map(|index| opaque(&format!("n{index}"))));
            let mut reverse = graph((0..count).map(|index| opaque(&format!("n{index}"))));
            add_generated_edges(&mut forward, &edges, false);
            add_generated_edges(&mut reverse, &edges, true);
            let labels = SecurityLabels {
                high_sources: (0..count)
                    .filter(|index| high_mask & (1 << index) != 0)
                    .map(|index| NodeId::new(index as u32))
                    .collect(),
                low_sinks: (0..count)
                    .filter(|index| low_mask & (1 << index) != 0)
                    .map(|index| NodeId::new(index as u32))
                    .collect(),
                declassifiers: (0..count)
                    .filter(|index| declass_mask & (1 << index) != 0)
                    .map(|index| NodeId::new(index as u32))
                    .collect(),
            };
            let expected = oracle_pairs(count, &edges, &labels);
            let left = noninterference(&forward, &labels);
            let right = noninterference(&reverse, &labels);
            prop_assert_eq!(violation_pairs(&left), expected);
            prop_assert_eq!(&left, &right);
            for finding in left {
                for witness in [finding.explicit_witness, finding.implicit_witness]
                    .into_iter()
                    .flatten()
                {
                    let mut current = finding.source;
                    for step in witness.steps {
                        prop_assert_eq!(step.source, current);
                        prop_assert!(!labels.declassifiers.contains(&step.target));
                        current = step.target;
                    }
                    prop_assert_eq!(current, finding.sink);
                }
            }
        }
    }

    #[test]
    fn deep_lifecycle_is_stack_safe() {
        std::thread::Builder::new()
            .name("ifc-small-stack".into())
            .stack_size(256 * 1024)
            .spawn(|| {
                let count = 100_001u32;
                let mut cpg = CodePropertyGraph::with_capacity(
                    Language::Rust,
                    count as usize,
                    count as usize - 1,
                );
                for id in 0..count {
                    cpg.add_node_with_id(node(
                        id,
                        if id + 1 == count {
                            CpgNodeKind::IndexAccess
                        } else {
                            opaque("deep")
                        },
                    ));
                    if id > 0 {
                        cpg.add_edge_with_id(CpgEdge::new(
                            EdgeId::new(id - 1),
                            NodeId::new(id - 1),
                            NodeId::new(id),
                            CpgEdgeKind::DataDependence,
                        ));
                    }
                }
                let source = NodeId::new(0);
                let sink = NodeId::new(count - 1);
                let violations = noninterference(&cpg, &SecurityLabels::new([source], [sink]));
                assert_eq!(
                    violations[0]
                        .explicit_witness
                        .as_ref()
                        .map(|witness| witness.steps.len()),
                    Some(count as usize - 1)
                );
                let leaks = constant_time(&cpg, &BTreeSet::from([source]));
                assert_eq!(leaks.len(), 1);
                assert_eq!(leaks[0].site, sink);
                assert_eq!(leaks[0].witness.steps.len(), count as usize - 1);
            })
            .expect("spawn IFC small-stack lifecycle")
            .join()
            .expect("IFC small-stack lifecycle");
    }
}
