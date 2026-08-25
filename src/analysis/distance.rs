//! Directed-fuzzing distance over control-flow and call-graph edges.

use std::{
    cmp::Reverse,
    collections::{BTreeMap, BinaryHeap, VecDeque},
};

use crate::{CodePropertyGraph, CpgEdgeKind, NodeId};

/// Computes deterministic harmonic-mean shortest-path distance to targets.
///
/// Paths may traverse control-flow edges and static, dynamic, or call-site
/// edges in their stored direction. Every valid target has distance zero. For
/// a non-target node that reaches one or more targets, the result is the
/// harmonic mean of its positive integral shortest-path lengths. A node that
/// cannot reach a valid target maps to [`f64::INFINITY`]. Duplicate targets are
/// counted once, and identifiers absent from `cpg` are ignored.
///
/// The implementation performs a reverse breadth-first traversal per valid
/// target using explicit heap worklists. It does not recurse with graph depth.
pub fn distance_to_targets(cpg: &CodePropertyGraph, targets: &[NodeId]) -> BTreeMap<NodeId, f64> {
    let mut node_heap = BinaryHeap::with_capacity(cpg.node_count());
    node_heap.extend(cpg.node_ids().map(Reverse));
    let mut node_ids = Vec::with_capacity(node_heap.len());
    while let Some(Reverse(node)) = node_heap.pop() {
        node_ids.push(node);
    }
    let dense: BTreeMap<_, _> = node_ids
        .iter()
        .copied()
        .enumerate()
        .map(|(index, node)| (node, index))
        .collect();

    let mut reverse_edge_heap = BinaryHeap::with_capacity(cpg.edge_count());
    for edge in cpg.edges() {
        if !is_distance_edge(&edge.kind) {
            continue;
        }
        let (Some(&source), Some(&target)) = (dense.get(&edge.source), dense.get(&edge.target))
        else {
            continue;
        };
        reverse_edge_heap.push(Reverse((target, source)));
    }
    let mut predecessors = vec![Vec::new(); node_ids.len()];
    let mut previous = None;
    while let Some(Reverse(edge @ (target, source))) = reverse_edge_heap.pop() {
        if previous != Some(edge) {
            predecessors[target].push(source);
            previous = Some(edge);
        }
    }

    let mut target_heap = BinaryHeap::with_capacity(targets.len());
    for &target in targets {
        if dense.contains_key(&target) {
            target_heap.push(Reverse(target));
        }
    }
    let mut canonical_targets = Vec::with_capacity(target_heap.len());
    while let Some(Reverse(target)) = target_heap.pop() {
        if canonical_targets.last().copied() != Some(target) {
            canonical_targets.push(target);
        }
    }

    let mut is_target = vec![false; node_ids.len()];
    let mut reachable_targets = vec![0_u32; node_ids.len()];
    let mut reciprocal_sum = vec![0.0_f64; node_ids.len()];
    let mut shortest = vec![u32::MAX; node_ids.len()];
    let mut queue = VecDeque::with_capacity(node_ids.len());
    for target in canonical_targets {
        shortest.fill(u32::MAX);
        queue.clear();
        let target_index = dense[&target];
        is_target[target_index] = true;
        shortest[target_index] = 0;
        queue.push_back(target_index);

        while let Some(node) = queue.pop_front() {
            let successor_distance = shortest[node].saturating_add(1);
            for &predecessor in &predecessors[node] {
                if shortest[predecessor] != u32::MAX {
                    continue;
                }
                shortest[predecessor] = successor_distance;
                queue.push_back(predecessor);
            }
        }

        for (index, &distance) in shortest.iter().enumerate() {
            if distance == 0 || distance == u32::MAX {
                continue;
            }
            reachable_targets[index] = reachable_targets[index].saturating_add(1);
            reciprocal_sum[index] += 1.0 / f64::from(distance);
        }
    }

    node_ids
        .into_iter()
        .enumerate()
        .map(|(index, node)| {
            let distance = if is_target[index] {
                0.0
            } else if reachable_targets[index] == 0 {
                f64::INFINITY
            } else {
                f64::from(reachable_targets[index]) / reciprocal_sum[index]
            };
            (node, distance)
        })
        .collect()
}

fn is_distance_edge(kind: &CpgEdgeKind) -> bool {
    matches!(
        kind,
        CpgEdgeKind::ControlFlow(_)
            | CpgEdgeKind::StaticCall
            | CpgEdgeKind::DynamicCall
            | CpgEdgeKind::CallSite
    )
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::{CfgEdgeKind, CpgNode, CpgNodeKind, Language, SourceRange};

    fn add_nodes(cpg: &mut CodePropertyGraph, ids: impl IntoIterator<Item = u32>) {
        for id in ids {
            cpg.add_node_with_id(CpgNode::new(
                NodeId::new(id),
                CpgNodeKind::Return,
                SourceRange::from_bytes(id, id.saturating_add(1)),
            ));
        }
    }

    fn connect(cpg: &mut CodePropertyGraph, source: u32, target: u32, kind: CpgEdgeKind) {
        cpg.connect(NodeId::new(source), NodeId::new(target), kind)
            .expect("distance fixture endpoints exist");
    }

    fn exact_fixture() -> CodePropertyGraph {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        add_nodes(&mut cpg, [0, 1, 2, 3, 10, 20, 30, 40, 99]);
        connect(
            &mut cpg,
            0,
            1,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        );
        connect(
            &mut cpg,
            0,
            1,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        );
        connect(
            &mut cpg,
            1,
            10,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Return),
        );
        connect(
            &mut cpg,
            0,
            2,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue),
        );
        connect(
            &mut cpg,
            2,
            3,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        );
        connect(
            &mut cpg,
            3,
            20,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Return),
        );
        connect(&mut cpg, 30, 0, CpgEdgeKind::CallSite);
        connect(&mut cpg, 40, 30, CpgEdgeKind::DynamicCall);
        connect(&mut cpg, 99, 0, CpgEdgeKind::AstChild);
        cpg
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() <= f64::EPSILON * expected.abs().max(1.0) * 4.0,
            "actual {actual}, expected {expected}"
        );
    }

    #[test]
    fn harmonic_distance_matches_hand_calculated_cfg_and_call_paths() {
        let cpg = exact_fixture();
        let before = (cpg.node_count(), cpg.edge_count());
        let distance = distance_to_targets(&cpg, &[NodeId::new(20), NodeId::new(10)]);

        assert_eq!(distance[&NodeId::new(10)], 0.0);
        assert_eq!(distance[&NodeId::new(20)], 0.0);
        assert_close(distance[&NodeId::new(0)], 12.0 / 5.0);
        assert_close(distance[&NodeId::new(30)], 24.0 / 7.0);
        assert_close(distance[&NodeId::new(40)], 40.0 / 9.0);
        assert!(distance[&NodeId::new(99)].is_infinite());
        assert_eq!((cpg.node_count(), cpg.edge_count()), before);
    }

    #[test]
    fn empty_duplicate_and_unknown_targets_have_explicit_semantics() {
        let cpg = exact_fixture();
        let empty = distance_to_targets(&cpg, &[]);
        assert!(empty.values().all(|distance| distance.is_infinite()));

        let unknown = distance_to_targets(&cpg, &[NodeId::new(1_000_000)]);
        assert_eq!(empty, unknown);

        let unique = distance_to_targets(&cpg, &[NodeId::new(10), NodeId::new(20)]);
        let duplicate_unknown = distance_to_targets(
            &cpg,
            &[
                NodeId::new(20),
                NodeId::new(1_000_000),
                NodeId::new(10),
                NodeId::new(20),
                NodeId::new(10),
            ],
        );
        assert_eq!(unique, duplicate_unknown);
    }

    fn permutation_fixture(enabled: &[bool], reverse: bool) -> CodePropertyGraph {
        let mut node_ids: Vec<_> = (0..10).collect();
        if reverse {
            node_ids.reverse();
        }
        let candidates = [
            (0, 1, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential)),
            (1, 8, CpgEdgeKind::ControlFlow(CfgEdgeKind::Return)),
            (0, 2, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential)),
            (2, 3, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential)),
            (3, 9, CpgEdgeKind::ControlFlow(CfgEdgeKind::Return)),
            (4, 0, CpgEdgeKind::StaticCall),
            (5, 4, CpgEdgeKind::DynamicCall),
            (6, 5, CpgEdgeKind::CallSite),
            (7, 0, CpgEdgeKind::AstChild),
            (0, 1, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential)),
        ];
        let mut edges: Vec<_> = candidates
            .into_iter()
            .enumerate()
            .filter(|(index, _)| enabled.get(*index).copied().unwrap_or(false))
            .map(|(_, edge)| edge)
            .collect();
        if reverse {
            edges.reverse();
        }

        let mut cpg = CodePropertyGraph::new(Language::Rust);
        add_nodes(&mut cpg, node_ids);
        for (source, target, kind) in edges {
            connect(&mut cpg, source, target, kind);
        }
        cpg
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn distances_are_exactly_insertion_and_target_order_invariant(
            enabled in prop::collection::vec(any::<bool>(), 0..14),
            repeat_first in any::<bool>(),
        ) {
            let forward = permutation_fixture(&enabled, false);
            let reverse = permutation_fixture(&enabled, true);
            let mut targets = vec![NodeId::new(8), NodeId::new(9), NodeId::new(999)];
            if repeat_first {
                targets.push(NodeId::new(8));
            }
            let mut reversed_targets = targets.clone();
            reversed_targets.reverse();
            prop_assert_eq!(
                distance_to_targets(&forward, &targets),
                distance_to_targets(&reverse, &reversed_targets),
            );
        }
    }
}
