//! System Dependence Graph summary edges and context-sensitive slicing.
//!
//! A System Dependence Graph (SDG) connects per-function Program Dependence
//! Graphs (PDGs) with parameter and return-value flow. Summary edges let a
//! slice step over a call without entering and then returning through an
//! unrelated call site. The two traversal phases follow Horwitz, Reps, and
//! Binkley (<https://doi.org/10.1145/77606.77608>).

use std::collections::{BTreeMap, BTreeSet};

use petgraph::Direction;

use crate::builder::{call_arguments, slice_phase, SliceEdgeMask};
use crate::{CodePropertyGraph, CpgEdgeKind, CpgNodeKind, NodeId};

use super::EffectSummary;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Stable label used to distinguish SDG summary edges from ordinary PDG data
/// dependences.
pub const SUMMARY_EDGE_LABEL: &str = "summary";

/// Explicit resource limits for an interprocedural slice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SliceCaps {
    /// Maximum number of distinct nodes returned by the slice.
    pub max_nodes: usize,
    /// Maximum number of interprocedural parameter/return boundaries crossed
    /// along any represented dependence path.
    pub max_call_depth: usize,
}

impl SliceCaps {
    /// Creates explicit node-count and call-depth limits.
    pub const fn new(max_nodes: usize, max_call_depth: usize) -> Self {
        Self {
            max_nodes,
            max_call_depth,
        }
    }
}

/// Materializes caller-local SDG summary edges from procedure effect summaries.
///
/// For every resolved call, argument `i` is connected to the call node when
/// bit `i` of the callee's [`EffectSummary::param_to_return`] mask is set. The
/// edge kind is [`CpgEdgeKind::DataDependence`] and its label is
/// [`SUMMARY_EDGE_LABEL`]. Capped or truncated fixed-width summaries
/// conservatively connect arguments not represented by the mask.
///
/// The operation is append-only and idempotent. It returns the number of edges
/// added by this invocation.
pub fn build_summary_edges(
    cpg: &mut CodePropertyGraph,
    effects: &BTreeMap<NodeId, EffectSummary>,
) -> usize {
    let mut calls: Vec<NodeId> = cpg
        .node_ids()
        .filter(|&id| {
            cpg.node(id)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Call { .. }))
        })
        .collect();
    calls.sort_unstable();

    let mut additions = Vec::new();
    for call in calls {
        let arguments = call_arguments(cpg, call);
        for callee in cpg.callees(call) {
            let Some(summary) = effects.get(&callee) else {
                continue;
            };
            for (index, &argument) in arguments.iter().enumerate() {
                let represented = u32::try_from(index)
                    .ok()
                    .filter(|&bit| bit < u64::BITS)
                    .is_some_and(|bit| summary.param_to_return & (1_u64 << bit) != 0);
                let conservatively_unknown =
                    summary.capped || (index >= u64::BITS as usize && summary.param_mask_truncated);
                if represented || conservatively_unknown {
                    additions.push((argument, call));
                }
            }
        }
    }
    additions.sort_unstable();
    additions.dedup();

    let mut added = 0;
    for (argument, call) in additions {
        let before = cpg.edge_count();
        cpg.connect_unique_labeled(
            argument,
            call,
            CpgEdgeKind::DataDependence,
            SUMMARY_EDGE_LABEL,
        );
        added += usize::from(cpg.edge_count() > before);
    }
    added
}

/// Computes a context-sensitive backward slice over the SDG.
///
/// Phase one follows PDG and summary edges and may ascend through `Parameter`
/// edges, but excludes `ReturnValue`. Phase two starts from every phase-one
/// node and may descend through `ReturnValue`, but excludes `Parameter`. This
/// prevents a dependence path from returning through an unrelated call site.
/// An absent criterion or a zero node cap yields the empty set.
pub fn interprocedural_backward_slice(
    cpg: &CodePropertyGraph,
    criterion: NodeId,
    caps: &SliceCaps,
) -> BTreeSet<NodeId> {
    two_pass_slice(cpg, criterion, caps, Direction::Incoming)
}

/// Computes the direction-dual context-sensitive forward slice over the SDG.
///
/// In the forward direction `ReturnValue` ascends into callers, whereas
/// `Parameter` descends into callees. Consequently phase one excludes
/// `Parameter` and phase two excludes `ReturnValue`. Summary edges remain
/// available in both phases. An absent criterion or a zero node cap yields the
/// empty set.
pub fn interprocedural_forward_slice(
    cpg: &CodePropertyGraph,
    criterion: NodeId,
    caps: &SliceCaps,
) -> BTreeSet<NodeId> {
    two_pass_slice(cpg, criterion, caps, Direction::Outgoing)
}

fn two_pass_slice(
    cpg: &CodePropertyGraph,
    criterion: NodeId,
    caps: &SliceCaps,
    direction: Direction,
) -> BTreeSet<NodeId> {
    if caps.max_nodes == 0 || cpg.node(criterion).is_none() {
        return BTreeSet::new();
    }

    let (ascend, descend) = match direction {
        Direction::Incoming => (
            SliceEdgeMask {
                parameter: true,
                return_value: false,
            },
            SliceEdgeMask {
                parameter: false,
                return_value: true,
            },
        ),
        Direction::Outgoing => (
            SliceEdgeMask {
                parameter: false,
                return_value: true,
            },
            SliceEdgeMask {
                parameter: true,
                return_value: false,
            },
        ),
    };

    let mut reached = BTreeSet::new();
    let phase_one_seeds = BTreeMap::from([(criterion, 0)]);
    let phase_one = slice_phase(
        cpg,
        &phase_one_seeds,
        caps.max_nodes,
        caps.max_call_depth,
        direction,
        ascend,
        &mut reached,
    );
    slice_phase(
        cpg,
        &phase_one,
        caps.max_nodes,
        caps.max_call_depth,
        direction,
        descend,
        &mut reached,
    );
    reached
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CpgEdge, CpgNode, DfgEdgeKind, EdgeId, Language, MethodSignature, SourceRange, Visibility,
    };

    fn node(cpg: &mut CodePropertyGraph, kind: CpgNodeKind) -> NodeId {
        cpg.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()))
    }

    fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
        let id = node(cpg, kind);
        cpg.connect(parent, id, CpgEdgeKind::AstChild);
        id
    }

    fn opaque(name: &str) -> CpgNodeKind {
        CpgNodeKind::Unknown { kind: name.into() }
    }

    fn function(name: &str) -> CpgNodeKind {
        CpgNodeKind::Function {
            signature: MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Public,
            },
        }
    }

    fn call() -> CpgNodeKind {
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        }
    }

    fn summary_fixture() -> (CodePropertyGraph, NodeId, NodeId, NodeId, NodeId) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let caller = node(&mut cpg, function("caller"));
        let callee = node(&mut cpg, function("callee"));
        let call = child(&mut cpg, caller, call());
        let _callee_syntax = child(&mut cpg, call, opaque("callee_syntax"));
        let first = child(&mut cpg, call, opaque("first_argument"));
        let second = child(&mut cpg, call, opaque("second_argument"));
        cpg.connect(call, callee, CpgEdgeKind::StaticCall);
        (cpg, callee, call, first, second)
    }

    #[test]
    fn summary_edges_are_labeled_selective_append_only_and_idempotent() {
        let (mut cpg, callee, call, first, second) = summary_fixture();
        cpg.connect(first, call, CpgEdgeKind::DataDependence);
        let nodes_before = cpg.node_count();
        let edges_before = cpg.edge_count();
        let effects = BTreeMap::from([(
            callee,
            EffectSummary {
                param_to_return: 1,
                ..EffectSummary::default()
            },
        )]);

        assert_eq!(build_summary_edges(&mut cpg, &effects), 1);
        assert_eq!(cpg.node_count(), nodes_before);
        assert_eq!(cpg.edge_count(), edges_before + 1);
        assert_eq!(build_summary_edges(&mut cpg, &effects), 0);
        assert_eq!(cpg.edge_count(), edges_before + 1);

        assert!(cpg.edges_between(first, call).iter().any(|edge| {
            edge.kind == CpgEdgeKind::DataDependence
                && edge.label.as_deref() == Some(SUMMARY_EDGE_LABEL)
        }));
        assert!(cpg.edges_between(second, call).iter().all(|edge| {
            edge.kind != CpgEdgeKind::DataDependence
                || edge.label.as_deref() != Some(SUMMARY_EDGE_LABEL)
        }));
    }

    #[test]
    fn absent_effect_summary_leaves_graph_unchanged() {
        let (mut cpg, _callee, _call, _first, _second) = summary_fixture();
        let shape = (cpg.node_count(), cpg.edge_count());
        assert_eq!(build_summary_edges(&mut cpg, &BTreeMap::new()), 0);
        assert_eq!((cpg.node_count(), cpg.edge_count()), shape);
    }

    #[test]
    fn multiple_resolved_callees_union_their_parameter_dependencies() {
        let (mut cpg, first_callee, call, first, second) = summary_fixture();
        let second_callee = node(&mut cpg, function("alternate"));
        cpg.connect(call, second_callee, CpgEdgeKind::DynamicCall);
        let effects = BTreeMap::from([
            (
                first_callee,
                EffectSummary {
                    param_to_return: 1,
                    ..EffectSummary::default()
                },
            ),
            (
                second_callee,
                EffectSummary {
                    param_to_return: 2,
                    ..EffectSummary::default()
                },
            ),
        ]);

        assert_eq!(build_summary_edges(&mut cpg, &effects), 2);
        for argument in [first, second] {
            assert!(cpg
                .edges_between(argument, call)
                .iter()
                .any(|edge| { edge.label.as_deref() == Some(SUMMARY_EDGE_LABEL) }));
        }
    }

    #[test]
    fn capped_summary_conservatively_connects_every_argument() {
        let (mut cpg, callee, call, first, second) = summary_fixture();
        let effects = BTreeMap::from([(
            callee,
            EffectSummary {
                capped: true,
                ..EffectSummary::default()
            },
        )]);

        assert_eq!(build_summary_edges(&mut cpg, &effects), 2);
        for argument in [first, second] {
            assert!(cpg
                .edges_between(argument, call)
                .iter()
                .any(|edge| { edge.label.as_deref() == Some(SUMMARY_EDGE_LABEL) }));
        }
    }

    #[test]
    fn truncated_summary_conservatively_connects_unrepresentable_arguments() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let caller = node(&mut cpg, function("caller"));
        let callee = node(&mut cpg, function("callee"));
        let call = child(&mut cpg, caller, call());
        let _callee_syntax = child(&mut cpg, call, opaque("callee_syntax"));
        let arguments: Vec<_> = (0..65)
            .map(|index| child(&mut cpg, call, opaque(&format!("arg_{index}"))))
            .collect();
        cpg.connect(call, callee, CpgEdgeKind::CallSite);
        let effects = BTreeMap::from([(
            callee,
            EffectSummary {
                param_mask_truncated: true,
                ..EffectSummary::default()
            },
        )]);

        assert_eq!(build_summary_edges(&mut cpg, &effects), 1);
        assert!(cpg
            .edges_between(arguments[64], call)
            .iter()
            .any(|edge| { edge.label.as_deref() == Some(SUMMARY_EDGE_LABEL) }));
    }

    struct SliceFixture {
        cpg: CodePropertyGraph,
        argument_one: NodeId,
        argument_two: NodeId,
        formal: NodeId,
        body: NodeId,
        returned: NodeId,
        call_one: NodeId,
        call_two: NodeId,
    }

    fn slice_fixture() -> SliceFixture {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let argument_one = node(&mut cpg, opaque("argument_one"));
        let argument_two = node(&mut cpg, opaque("argument_two"));
        let formal = node(&mut cpg, opaque("formal"));
        let body = node(&mut cpg, opaque("callee_body"));
        let returned = node(&mut cpg, opaque("returned"));
        let call_one = node(&mut cpg, call());
        let call_two = node(&mut cpg, call());

        cpg.connect(
            argument_one,
            formal,
            CpgEdgeKind::DataFlow(DfgEdgeKind::Parameter),
        );
        cpg.connect(
            argument_two,
            formal,
            CpgEdgeKind::DataFlow(DfgEdgeKind::Parameter),
        );
        cpg.connect(formal, body, CpgEdgeKind::DataDependence);
        cpg.connect(body, returned, CpgEdgeKind::DataDependence);
        cpg.connect(
            returned,
            call_one,
            CpgEdgeKind::DataFlow(DfgEdgeKind::ReturnValue),
        );
        cpg.connect(
            returned,
            call_two,
            CpgEdgeKind::DataFlow(DfgEdgeKind::ReturnValue),
        );
        cpg.add_edge(
            CpgEdge::new(
                EdgeId::new(0),
                argument_one,
                call_one,
                CpgEdgeKind::DataDependence,
            )
            .with_label(SUMMARY_EDGE_LABEL),
        );
        cpg.add_edge(
            CpgEdge::new(
                EdgeId::new(0),
                argument_two,
                call_two,
                CpgEdgeKind::DataDependence,
            )
            .with_label(SUMMARY_EDGE_LABEL),
        );

        SliceFixture {
            cpg,
            argument_one,
            argument_two,
            formal,
            body,
            returned,
            call_one,
            call_two,
        }
    }

    #[test]
    fn backward_slice_crosses_call_but_not_an_unrealizable_sibling_return() {
        let fixture = slice_fixture();
        let slice =
            interprocedural_backward_slice(&fixture.cpg, fixture.call_one, &SliceCaps::new(32, 4));

        assert!(slice.contains(&fixture.call_one));
        assert!(slice.contains(&fixture.argument_one));
        assert!(slice.contains(&fixture.returned));
        assert!(slice.contains(&fixture.body));
        assert!(slice.contains(&fixture.formal));
        assert!(!slice.contains(&fixture.argument_two));
        assert!(!slice.contains(&fixture.call_two));
    }

    #[test]
    fn forward_slice_descends_call_but_not_an_unrealizable_sibling_return() {
        let fixture = slice_fixture();
        let slice = interprocedural_forward_slice(
            &fixture.cpg,
            fixture.argument_one,
            &SliceCaps::new(32, 4),
        );

        assert!(slice.contains(&fixture.argument_one));
        assert!(slice.contains(&fixture.call_one));
        assert!(slice.contains(&fixture.formal));
        assert!(slice.contains(&fixture.body));
        assert!(slice.contains(&fixture.returned));
        assert!(!slice.contains(&fixture.argument_two));
        assert!(!slice.contains(&fixture.call_two));
    }

    #[test]
    fn slice_caps_and_unknown_criteria_are_total() {
        let fixture = slice_fixture();
        let depth_zero =
            interprocedural_backward_slice(&fixture.cpg, fixture.call_one, &SliceCaps::new(32, 0));
        assert_eq!(
            depth_zero,
            BTreeSet::from([fixture.argument_one, fixture.call_one])
        );

        let one_node =
            interprocedural_backward_slice(&fixture.cpg, fixture.call_one, &SliceCaps::new(1, 4));
        assert_eq!(one_node, BTreeSet::from([fixture.call_one]));
        assert!(interprocedural_backward_slice(
            &fixture.cpg,
            NodeId::new(u32::MAX),
            &SliceCaps::new(32, 4),
        )
        .is_empty());
        assert!(interprocedural_forward_slice(
            &fixture.cpg,
            fixture.call_one,
            &SliceCaps::new(0, 4),
        )
        .is_empty());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn slice_caps_serde_round_trip() {
        let caps = SliceCaps::new(4_096, 12);
        let json = serde_json::to_string(&caps).expect("serialize slice caps");
        let decoded: SliceCaps = serde_json::from_str(&json).expect("deserialize slice caps");
        assert_eq!(decoded, caps);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::testutil::{arb_pdg_graph, node_ids};
    use crate::{backward_slice, CpgNode, DfgEdgeKind, Language, SourceRange};
    use proptest::prelude::*;

    fn graph_with_edges(node_count: usize, edges: &[(usize, usize, u8)]) -> CodePropertyGraph {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        for index in 0..node_count {
            cpg.add_node(CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::Unknown {
                    kind: format!("n{index}").into(),
                },
                SourceRange::default(),
            ));
        }
        for &(source, target, kind) in edges {
            let source = NodeId::new((source % node_count) as u32);
            let target = NodeId::new((target % node_count) as u32);
            let kind = match kind % 4 {
                0 => CpgEdgeKind::DataDependence,
                1 => CpgEdgeKind::ControlDependence,
                2 => CpgEdgeKind::DataFlow(DfgEdgeKind::Parameter),
                _ => CpgEdgeKind::DataFlow(DfgEdgeKind::ReturnValue),
            };
            cpg.connect(source, target, kind);
        }
        cpg
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn prop_slices_are_insertion_order_deterministic_and_bounded(
            node_count in 1usize..12,
            edges in prop::collection::vec((any::<usize>(), any::<usize>(), any::<u8>()), 0..36),
            criterion in any::<usize>(),
            max_nodes in 0usize..16,
            max_call_depth in 0usize..6,
        ) {
            let left = graph_with_edges(node_count, &edges);
            let mut reversed = edges.clone();
            reversed.reverse();
            let right = graph_with_edges(node_count, &reversed);
            let criterion = NodeId::new((criterion % node_count) as u32);
            let caps = SliceCaps::new(max_nodes, max_call_depth);

            let left_backward = interprocedural_backward_slice(&left, criterion, &caps);
            let right_backward = interprocedural_backward_slice(&right, criterion, &caps);
            prop_assert_eq!(&left_backward, &right_backward);
            prop_assert!(left_backward.len() <= max_nodes);

            let left_forward = interprocedural_forward_slice(&left, criterion, &caps);
            let right_forward = interprocedural_forward_slice(&right, criterion, &caps);
            prop_assert_eq!(&left_forward, &right_forward);
            prop_assert!(left_forward.len() <= max_nodes);
        }

        #[test]
        fn prop_node_and_depth_caps_are_monotone(
            node_count in 1usize..12,
            edges in prop::collection::vec((any::<usize>(), any::<usize>(), any::<u8>()), 0..36),
            criterion in any::<usize>(),
            a in 0usize..16,
            b in 0usize..16,
            depth in 0usize..6,
        ) {
            let cpg = graph_with_edges(node_count, &edges);
            let criterion = NodeId::new((criterion % node_count) as u32);
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            let smaller = interprocedural_backward_slice(
                &cpg,
                criterion,
                &SliceCaps::new(lo, depth),
            );
            let larger = interprocedural_backward_slice(
                &cpg,
                criterion,
                &SliceCaps::new(hi, depth),
            );
            prop_assert!(smaller.is_subset(&larger));

            let shallow = interprocedural_backward_slice(
                &cpg,
                criterion,
                &SliceCaps::new(node_count, 0),
            );
            let deep = interprocedural_backward_slice(
                &cpg,
                criterion,
                &SliceCaps::new(node_count, depth),
            );
            prop_assert!(shallow.is_subset(&deep));
        }

        #[test]
        fn prop_intraprocedural_backward_slice_is_preserved(
            cpg in arb_pdg_graph(),
            index in any::<usize>(),
        ) {
            let ids = node_ids(&cpg);
            prop_assume!(!ids.is_empty());
            let criterion = ids[index % ids.len()];
            let intra = backward_slice(&cpg, criterion, cpg.node_count());
            let inter = interprocedural_backward_slice(
                &cpg,
                criterion,
                &SliceCaps::new(cpg.node_count(), cpg.node_count()),
            );
            prop_assert!(intra.iter().all(|node| inter.contains(node)));
        }
    }
}
