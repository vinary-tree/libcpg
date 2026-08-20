//! Weisfeiler-Lehman label refinement and structural clone classes.
//!
//! Labels start from directed in/out degree, then refine against the sorted
//! multiset of incident-neighbor labels. A fixed SplitMix-style combiner and
//! sorted per-round canonicalization make results reproducible across Rust
//! releases, storage orders, and isomorphic stable-identifier relabelings.

use std::collections::BTreeMap;

use super::GraphProjection;
use crate::NodeId;

/// Maximum number of Weisfeiler-Lehman refinement rounds.
pub const MAX_WL_ITERATIONS: usize = 16;

/// Deterministic 64-bit combiner shared by WL-based analyses.
///
/// The constants and wrapping sequence are fixed API-independent algorithm
/// details; unlike `DefaultHasher`, they do not vary across Rust releases.
pub(crate) const fn stable_wl_mix(hash: u64, value: u64) -> u64 {
    let mut mixed = hash
        .wrapping_mul(0x100000001b3)
        .wrapping_add(value)
        .wrapping_add(0x9e3779b97f4a7c15);
    mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94d049bb133111eb);
    mixed ^ (mixed >> 31)
}

/// Computes deterministic Weisfeiler-Lehman labels aligned with projection nodes.
///
/// The initial label is a stable hash of `(in_degree, out_degree)`. Each round
/// hashes the current label together with the sorted multiset of labels on
/// incoming and outgoing neighbors. Edge direction therefore contributes to
/// the seed, while refinement treats incident neighborhoods as undirected,
/// matching the structural-clone kernel ported from pgmcp. Edge weights are
/// intentionally ignored.
///
/// `iterations` is clamped to [`MAX_WL_ITERATIONS`]. A malformed public-field
/// projection returns an empty vector before indexed traversal.
pub fn wl_labels(graph: &GraphProjection, iterations: usize) -> Vec<u64> {
    comparable_wl_labels(&[graph], iterations)
        .and_then(|mut labels| labels.pop())
        .unwrap_or_default()
}

/// Computes WL labels in one shared vocabulary across all input projections.
///
/// Per-graph canonical ranks are not comparable across independently refined
/// graphs: rank zero can denote different raw neighborhood signatures in each
/// graph. Joint canonicalization retains the compact public labels while
/// ensuring that equal ranks mean equal refinement signatures throughout one
/// comparison. `None` reports a malformed public-field projection.
pub(crate) fn comparable_wl_labels(
    graphs: &[&GraphProjection],
    iterations: usize,
) -> Option<Vec<Vec<u64>>> {
    if graphs.iter().any(|graph| !graph.is_well_formed()) {
        return None;
    }

    let mut labels = graphs
        .iter()
        .map(|graph| {
            (0..graph.node_count())
                .map(|dense| {
                    let incoming = graph.predecessors(dense as u32).len() as u64;
                    let outgoing = graph.successors(dense as u32).len() as u64;
                    stable_wl_mix(stable_wl_mix(0xABCD, incoming), outgoing)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let mut neighbor_labels = Vec::new();
    for _ in 0..iterations.min(MAX_WL_ITERATIONS) {
        let mut next = Vec::with_capacity(graphs.len());
        for (graph, graph_labels) in graphs.iter().zip(&labels) {
            let mut graph_next = Vec::with_capacity(graph.node_count());
            for dense in 0..graph.node_count() {
                neighbor_labels.clear();
                neighbor_labels.extend(
                    graph
                        .successors(dense as u32)
                        .iter()
                        .map(|&neighbor| graph_labels[neighbor as usize]),
                );
                neighbor_labels.extend(
                    graph
                        .predecessors(dense as u32)
                        .iter()
                        .map(|&neighbor| graph_labels[neighbor as usize]),
                );
                neighbor_labels.sort_unstable();

                let mut hash = stable_wl_mix(0x1234, graph_labels[dense]);
                for &neighbor in &neighbor_labels {
                    hash = stable_wl_mix(hash, neighbor);
                }
                graph_next.push(hash);
            }
            next.push(graph_next);
        }

        let mut canonical = next.iter().flatten().copied().collect::<Vec<_>>();
        canonical.sort_unstable();
        canonical.dedup();
        for label in next.iter_mut().flatten() {
            *label = canonical
                .binary_search(label)
                .expect("a raw WL label is present in its canonical set")
                as u64;
        }
        labels = next;
    }

    Some(labels)
}

/// Cosine similarity between jointly canonicalized WL-label multisets.
pub(crate) fn wl_multiset_cosine(
    left: &GraphProjection,
    right: &GraphProjection,
    iterations: usize,
) -> Option<f64> {
    let labels = comparable_wl_labels(&[left, right], iterations)?;
    Some(multiset_cosine(&labels[0], &labels[1]))
}

pub(crate) fn multiset_cosine(left: &[u64], right: &[u64]) -> f64 {
    let mut left_counts = BTreeMap::<u64, usize>::new();
    let mut right_counts = BTreeMap::<u64, usize>::new();
    for &label in left {
        *left_counts.entry(label).or_default() += 1;
    }
    for &label in right {
        *right_counts.entry(label).or_default() += 1;
    }
    if left_counts == right_counts {
        return 1.0;
    }
    let dot: f64 = left_counts
        .iter()
        .map(|(label, &count)| count as f64 * right_counts.get(label).copied().unwrap_or(0) as f64)
        .sum();
    let left_norm: f64 = left_counts
        .values()
        .map(|&count| (count * count) as f64)
        .sum();
    let right_norm: f64 = right_counts
        .values()
        .map(|&count| (count * count) as f64)
        .sum();
    if left_norm == 0.0 || right_norm == 0.0 {
        f64::from(left.is_empty() && right.is_empty())
    } else {
        (dot / (left_norm.sqrt() * right_norm.sqrt())).clamp(0.0, 1.0)
    }
}

/// Groups structurally indistinguishable projection nodes by final WL label.
///
/// Members are sorted stable [`NodeId`] values. Classes are ordered by
/// descending size, then by their smallest member. Because a clone requires at
/// least two occurrences, `min_class` values below two are treated as two.
/// Values above the node count simply produce an empty result. Iteration and
/// malformed-input behavior is inherited from [`wl_labels`].
pub fn structural_clone_classes(
    graph: &GraphProjection,
    iterations: usize,
    min_class: usize,
) -> Vec<Vec<NodeId>> {
    let labels = wl_labels(graph, iterations);
    if labels.len() != graph.node_count() {
        return Vec::new();
    }

    let mut by_label: BTreeMap<u64, Vec<NodeId>> = BTreeMap::new();
    for (&node, label) in graph.nodes.iter().zip(labels) {
        by_label.entry(label).or_default().push(node);
    }

    let minimum = min_class.max(2);
    let mut classes: Vec<_> = by_label
        .into_values()
        .filter(|class| class.len() >= minimum)
        .collect();
    classes.sort_by(|left, right| {
        right
            .len()
            .cmp(&left.len())
            .then_with(|| left[0].cmp(&right[0]))
    });
    classes
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use proptest::prelude::*;

    use super::*;
    use crate::ProjectionKind;

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
    fn two_isomorphic_triangles_share_one_label_and_clone_class() {
        let graph = graph_from(6, &[(0, 1), (1, 2), (2, 0), (3, 4), (4, 5), (5, 3)]);
        let labels = wl_labels(&graph, 2);
        assert!(labels.iter().all(|&label| label == labels[0]));
        assert_eq!(
            structural_clone_classes(&graph, 2, 2),
            vec![(0..6).map(NodeId::new).collect::<Vec<_>>()]
        );
    }

    #[test]
    fn star_center_differs_from_its_structurally_identical_leaves() {
        let graph = graph_from(4, &[(0, 1), (0, 2), (0, 3)]);
        let labels = wl_labels(&graph, 1);
        assert_ne!(labels[0], labels[1]);
        assert_eq!(labels[1], labels[2]);
        assert_eq!(labels[2], labels[3]);
        assert_eq!(
            structural_clone_classes(&graph, 1, 2),
            vec![vec![NodeId::new(1), NodeId::new(2), NodeId::new(3)]]
        );
    }

    #[test]
    fn clone_classes_use_size_then_stable_identifier_order_and_minimum_two() {
        let graph = graph_from(8, &[(0, 1), (0, 2), (0, 3), (6, 7), (7, 6)]);
        assert_eq!(
            structural_clone_classes(&graph, 2, 0),
            vec![
                vec![NodeId::new(1), NodeId::new(2), NodeId::new(3)],
                vec![NodeId::new(4), NodeId::new(5)],
                vec![NodeId::new(6), NodeId::new(7)],
            ]
        );
        assert_eq!(
            structural_clone_classes(&graph, 2, 3),
            vec![vec![NodeId::new(1), NodeId::new(2), NodeId::new(3)]]
        );
        assert!(structural_clone_classes(&graph, 2, 9).is_empty());
    }

    #[test]
    fn empty_and_malformed_public_projections_are_total() {
        let empty = graph_from(0, &[]);
        assert!(wl_labels(&empty, 3).is_empty());
        assert!(structural_clone_classes(&empty, 3, 2).is_empty());

        let mut malformed = graph_from(2, &[(0, 1)]);
        malformed.offsets = vec![0, 2, 1];
        assert!(wl_labels(&malformed, 3).is_empty());
        assert!(structural_clone_classes(&malformed, 3, 2).is_empty());
    }

    #[test]
    fn iteration_cap_is_inclusive_and_explicit() {
        let graph = graph_from(5, &[(0, 1), (1, 2), (2, 3), (3, 4)]);
        assert_eq!(
            wl_labels(&graph, MAX_WL_ITERATIONS),
            wl_labels(&graph, MAX_WL_ITERATIONS + 1)
        );
        assert_eq!(
            structural_clone_classes(&graph, usize::MAX, 2),
            structural_clone_classes(&graph, MAX_WL_ITERATIONS, 2)
        );
    }

    #[test]
    fn weights_and_out_of_universe_edges_do_not_change_topology_labels() {
        let nodes = [NodeId::new(10), NodeId::new(20), NodeId::new(30)];
        let plain = GraphProjection::from_edges(
            ProjectionKind::External,
            nodes,
            [
                (NodeId::new(10), NodeId::new(20), 1.0),
                (NodeId::new(20), NodeId::new(30), 1.0),
            ],
        );
        let weighted = GraphProjection::from_edges(
            ProjectionKind::External,
            nodes,
            [
                (NodeId::new(10), NodeId::new(20), 7.5),
                (NodeId::new(20), NodeId::new(30), -2.0),
                (NodeId::new(10), NodeId::new(999), 3.0),
                (NodeId::new(999), NodeId::new(20), 4.0),
            ],
        );
        assert_eq!(wl_labels(&plain, 4), wl_labels(&weighted, 4));
        assert_eq!(
            structural_clone_classes(&plain, 4, 2),
            structural_clone_classes(&weighted, 4, 2)
        );
    }

    #[test]
    fn directed_degree_seed_distinguishes_source_and_sink_without_refinement() {
        let graph = graph_from(2, &[(0, 1)]);
        let labels = wl_labels(&graph, 0);
        assert_ne!(labels[0], labels[1]);
        assert!(structural_clone_classes(&graph, 0, 2).is_empty());
    }

    #[test]
    fn splitmix_combiner_matches_the_ported_stable_regression_values() {
        assert_eq!(stable_wl_mix(0, 0), 0xe220_a839_7b1d_cdaf);
        assert_eq!(stable_wl_mix(0xABCD, 1), 0x1e4a_a880_194a_7520);
        assert_eq!(stable_wl_mix(0x1234, 0x5678), 0xd1d1_0597_aec7_16b6);
    }

    #[test]
    fn joint_vocabulary_compares_isomorphic_graphs_without_local_rank_aliasing() {
        let original = graph_from(5, &[(0, 1), (1, 2), (1, 3), (3, 4)]);
        let relabeled = GraphProjection::from_edges(
            ProjectionKind::External,
            (0..5).map(|index| NodeId::new(100 + index * 7)),
            [(100, 107), (107, 114), (107, 121), (121, 128)]
                .map(|(source, target)| (NodeId::new(source), NodeId::new(target), 1.0)),
        );
        assert_eq!(wl_multiset_cosine(&original, &relabeled, 4), Some(1.0));

        let unrelated = graph_from(5, &[(0, 1), (0, 2), (0, 3), (0, 4)]);
        assert!(wl_multiset_cosine(&original, &unrelated, 4).unwrap() < 1.0);
    }

    #[test]
    fn joint_vocabulary_rejects_malformed_inputs_and_handles_empty_graphs() {
        let empty = graph_from(0, &[]);
        assert_eq!(wl_multiset_cosine(&empty, &empty, 3), Some(1.0));

        let mut malformed = graph_from(2, &[(0, 1)]);
        malformed.targets.push(9);
        assert_eq!(wl_multiset_cosine(&empty, &malformed, 3), None);
    }

    fn normalized_logical_classes(
        classes: Vec<Vec<NodeId>>,
        logical_by_node: &BTreeMap<NodeId, usize>,
    ) -> Vec<Vec<usize>> {
        let mut normalized = classes
            .into_iter()
            .map(|class| {
                let mut logical = class
                    .into_iter()
                    .map(|node| logical_by_node[&node])
                    .collect::<Vec<_>>();
                logical.sort_unstable();
                logical
            })
            .collect::<Vec<_>>();
        normalized.sort();
        normalized
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn isomorphic_relabeling_preserves_label_multisets_and_clone_partitions(
            ordering_keys in prop::collection::vec(any::<u16>(), 0..10),
            raw_edges in prop::collection::vec((0usize..10, 0usize..10), 0..80),
            iterations in 0usize..24,
            min_class in 0usize..12,
        ) {
            let n = ordering_keys.len();
            let edges = raw_edges
                .into_iter()
                .filter(|&(source, target)| source < n && target < n)
                .collect::<Vec<_>>();
            let original = graph_from(n, &edges);

            let mut logical_order = (0..n).collect::<Vec<_>>();
            logical_order.sort_unstable_by_key(|&logical| (ordering_keys[logical], logical));
            let mut relabeled_node = vec![NodeId::new(0); n];
            for (rank, logical) in logical_order.into_iter().enumerate() {
                relabeled_node[logical] = NodeId::new(100 + rank as u32 * 3);
            }
            let mut relabeled_edges = edges
                .iter()
                .rev()
                .map(|&(source, target)| {
                    (relabeled_node[source], relabeled_node[target], 1.0)
                })
                .collect::<Vec<_>>();
            relabeled_edges.extend(relabeled_edges.clone());
            let relabeled = GraphProjection::from_edges(
                ProjectionKind::External,
                relabeled_node.iter().copied(),
                relabeled_edges,
            );

            let original_labels = wl_labels(&original, iterations);
            let mut original_multiset = original_labels.clone();
            original_multiset.sort_unstable();
            let mut relabeled_multiset = wl_labels(&relabeled, iterations);
            relabeled_multiset.sort_unstable();
            prop_assert_eq!(original_multiset, relabeled_multiset);
            prop_assert_eq!(&original_labels, &wl_labels(&original, iterations));

            let original_lookup = (0..n)
                .map(|logical| (NodeId::new(logical as u32), logical))
                .collect::<BTreeMap<_, _>>();
            let relabeled_lookup = relabeled_node
                .iter()
                .copied()
                .enumerate()
                .map(|(logical, node)| (node, logical))
                .collect::<BTreeMap<_, _>>();
            let original_classes = normalized_logical_classes(
                structural_clone_classes(&original, iterations, min_class),
                &original_lookup,
            );
            let relabeled_classes = normalized_logical_classes(
                structural_clone_classes(&relabeled, iterations, min_class),
                &relabeled_lookup,
            );
            prop_assert_eq!(original_classes, relabeled_classes);
            prop_assert_eq!(
                wl_multiset_cosine(&original, &relabeled, iterations),
                Some(1.0),
            );
            prop_assert_eq!(
                wl_multiset_cosine(&original, &relabeled, iterations),
                wl_multiset_cosine(&relabeled, &original, iterations),
            );
        }
    }
}
