//! Deterministic bounded simple-cycle enumeration over graph projections.
//!
//! Enumeration is restricted to cyclic strongly connected components (SCCs)
//! discovered by the crate's iterative Tarjan implementation. Within those
//! components, an iterative depth-first search visits cycle lengths, starting
//! vertices, and successor vertices in canonical order. Every returned cycle
//! is rotation-normalized to its smallest stable node identifier.

use std::collections::BTreeSet;

use crate::NodeId;

use super::{projection_sccs, AnalysisError, GraphProjection};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Hard ceiling on the number of vertices in an enumerated simple cycle.
pub const MAX_CYCLE_LENGTH: usize = 32;

/// Hard ceiling on the number of simple cycles retained in one result.
pub const MAX_SIMPLE_CYCLES: usize = 100_000;

/// Resource controls for [`simple_cycles`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CycleOptions {
    /// Requested maximum number of vertices per cycle.
    ///
    /// Values above [`MAX_CYCLE_LENGTH`] are clamped. Zero disables cycle
    /// enumeration and returns a complete empty result.
    pub max_length: usize,
    /// Requested maximum number of cycles retained in the result.
    ///
    /// Values above [`MAX_SIMPLE_CYCLES`] are clamped. Zero still probes for
    /// the first cycle so truncation cannot be silent.
    pub max_cycles: usize,
}

impl Default for CycleOptions {
    fn default() -> Self {
        Self {
            max_length: 10,
            max_cycles: 10_000,
        }
    }
}

/// Canonical bounded simple cycles and any count-cap truncation evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CycleSet {
    /// Projection nodes in the dense order used during enumeration.
    pub nodes: Vec<NodeId>,
    /// Simple directed cycles sorted by `(length, lexicographic node order)`.
    ///
    /// The closing start vertex is implicit: for `[a, b, c]`, the required
    /// directed edges are `a -> b`, `b -> c`, and `c -> a`.
    pub cycles: Vec<Vec<NodeId>>,
    /// Effective length limit after applying [`MAX_CYCLE_LENGTH`].
    pub max_length: usize,
    /// Effective result-count limit after applying [`MAX_SIMPLE_CYCLES`].
    pub max_cycles: usize,
    /// Whether at least one unique cycle was omitted by the count limit.
    pub truncated: bool,
    /// Proven lower bound on the number of omitted unique cycles.
    ///
    /// Enumeration stops at the first over-cap cycle so the count cap also
    /// bounds work on cycle-dense graphs. Consequently this is zero for a
    /// complete result and one for a truncated result, rather than a claim
    /// about the unknown suffix of the search space.
    pub dropped_cycles: usize,
    /// Canonical witness for the first cycle omitted by the count limit.
    pub first_dropped_cycle: Option<Vec<NodeId>>,
}

/// Enumerates bounded simple directed cycles in canonical order.
///
/// The projection must satisfy its public CSR invariants. Edge weights are
/// ignored because cycle membership depends only on directed topology.
/// Self-loops are length-one cycles, reciprocal edges form length-two cycles,
/// and opposite orientations are distinct when both exist. Rotation-equivalent
/// representations of the same directed cycle are emitted once.
///
/// The search first reuses [`projection_sccs`] to discard acyclic regions. It
/// then processes cycle lengths in ascending order, followed by stable dense
/// starting-node and successor order. Paths use an explicit stack and cannot
/// overflow the native call stack. The result is therefore identical across
/// runs and edge insertion orders for the same projection topology.
///
/// If `max_cycles` is reached, the first additional unique cycle is recorded
/// in [`CycleSet::first_dropped_cycle`], `truncated` is set, and enumeration
/// stops. This makes truncation explicit without defeating the resource cap by
/// traversing an unbounded remainder merely to count it.
pub fn simple_cycles(
    graph: &GraphProjection,
    options: &CycleOptions,
) -> Result<CycleSet, AnalysisError> {
    validate_projection(graph)?;

    let max_length = options.max_length.min(MAX_CYCLE_LENGTH);
    let max_cycles = options.max_cycles.min(MAX_SIMPLE_CYCLES);
    let mut result = CycleSet {
        nodes: graph.nodes.clone(),
        cycles: Vec::new(),
        max_length,
        max_cycles,
        truncated: false,
        dropped_cycles: 0,
        first_dropped_cycle: None,
    };
    if max_length == 0 || graph.is_empty() {
        return Ok(result);
    }

    let decomposition = projection_sccs(graph);
    let component_by_dense: Vec<usize> = graph
        .nodes
        .iter()
        .map(|node| decomposition.component_by_node[node])
        .collect();
    let cyclic: Vec<bool> = decomposition
        .components
        .iter()
        .map(|component| component.cyclic)
        .collect();
    let component_size: Vec<usize> = decomposition
        .components
        .iter()
        .map(|component| component.nodes.len())
        .collect();

    let greatest_length = max_length.min(graph.node_count());
    let mut seen = BTreeSet::<Vec<u32>>::new();
    let mut search = CycleSearch::new(graph.node_count(), greatest_length);
    'lengths: for length in 1..=greatest_length {
        for start in 0..graph.node_count() {
            let component = component_by_dense[start];
            if !cyclic[component] || component_size[component] < length {
                continue;
            }

            enumerate_from_start(
                graph,
                &component_by_dense,
                component,
                start as u32,
                length,
                max_cycles.saturating_sub(seen.len()).saturating_add(1),
                &mut search,
            );
            for cycle in search.discovered.drain(..) {
                let cycle = canonical_rotation(cycle);
                if seen.len() == max_cycles {
                    result.truncated = true;
                    result.dropped_cycles = 1;
                    result.first_dropped_cycle =
                        Some(cycle.iter().map(|&dense| graph.node_id(dense)).collect());
                    break 'lengths;
                }
                // `start` is the cycle's unique minimum dense index, so this
                // insertion is unique by construction. The ordered set also
                // keeps that invariant defensive at the representation level.
                let _ = seen.insert(cycle);
            }
        }
    }

    result.cycles = seen
        .into_iter()
        .map(|cycle| {
            cycle
                .into_iter()
                .map(|dense| graph.node_id(dense))
                .collect()
        })
        .collect();
    result
        .cycles
        .sort_unstable_by(|left, right| left.len().cmp(&right.len()).then_with(|| left.cmp(right)));
    Ok(result)
}

#[derive(Debug, Clone, Copy)]
struct DfsFrame {
    node: u32,
    next_successor: usize,
}

#[derive(Debug)]
struct CycleSearch {
    path: Vec<u32>,
    on_path: Vec<bool>,
    stack: Vec<DfsFrame>,
    discovered: Vec<Vec<u32>>,
}

impl CycleSearch {
    fn new(node_count: usize, max_length: usize) -> Self {
        Self {
            path: Vec::with_capacity(max_length),
            on_path: vec![false; node_count],
            stack: Vec::with_capacity(max_length),
            discovered: Vec::new(),
        }
    }
}

fn enumerate_from_start(
    graph: &GraphProjection,
    component_by_dense: &[usize],
    component: usize,
    start: u32,
    target_length: usize,
    discovery_limit: usize,
    search: &mut CycleSearch,
) {
    debug_assert!(search.path.is_empty());
    debug_assert!(search.stack.is_empty());
    debug_assert!(search.discovered.is_empty());
    if target_length == 1 {
        if graph.successors(start).binary_search(&start).is_ok() {
            search.discovered.push(vec![start]);
        }
        return;
    }

    search.path.push(start);
    search.on_path[start as usize] = true;
    search.stack.push(DfsFrame {
        node: start,
        next_successor: 0,
    });

    while let Some(frame) = search.stack.last_mut() {
        if search.path.len() == target_length {
            if graph.successors(frame.node).binary_search(&start).is_ok() {
                search.discovered.push(search.path.clone());
                if search.discovered.len() == discovery_limit {
                    for &node in &search.path {
                        search.on_path[node as usize] = false;
                    }
                    search.path.clear();
                    search.stack.clear();
                    return;
                }
            }
            let departed = search
                .stack
                .pop()
                .expect("the terminal DFS frame exists")
                .node;
            search.on_path[departed as usize] = false;
            search.path.pop();
            continue;
        }

        let successors = graph.successors(frame.node);
        if frame.next_successor == successors.len() {
            let departed = search
                .stack
                .pop()
                .expect("the exhausted DFS frame exists")
                .node;
            search.on_path[departed as usize] = false;
            search.path.pop();
            continue;
        }

        let successor = successors[frame.next_successor];
        frame.next_successor += 1;
        let successor_index = successor as usize;
        if successor > start
            && component_by_dense[successor_index] == component
            && !search.on_path[successor_index]
        {
            search.on_path[successor_index] = true;
            search.path.push(successor);
            search.stack.push(DfsFrame {
                node: successor,
                next_successor: 0,
            });
        }
    }

    debug_assert!(search.path.is_empty());
    debug_assert!(search.stack.is_empty());
    debug_assert!(!search.on_path[start as usize]);
}

fn canonical_rotation(mut cycle: Vec<u32>) -> Vec<u32> {
    if let Some((offset, _)) = cycle.iter().enumerate().min_by_key(|(_, node)| *node) {
        cycle.rotate_left(offset);
    }
    cycle
}

fn validate_projection(graph: &GraphProjection) -> Result<(), AnalysisError> {
    if graph.is_well_formed() {
        Ok(())
    } else {
        Err(AnalysisError::InvalidInput {
            analysis: "simple_cycles",
            input: "projection",
            reason: "must satisfy sorted-node and forward/reverse CSR invariants",
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use proptest::prelude::*;

    use super::*;
    use crate::ProjectionKind;

    fn id(dense: usize) -> NodeId {
        NodeId::new(10 + dense as u32 * 7)
    }

    fn projection(
        node_count: usize,
        edges: impl IntoIterator<Item = (usize, usize)>,
    ) -> GraphProjection {
        GraphProjection::from_edges(
            ProjectionKind::External,
            (0..node_count).map(id),
            edges
                .into_iter()
                .map(|(source, target)| (id(source), id(target), 1.0)),
        )
    }

    fn fixture() -> GraphProjection {
        projection(5, [(0, 0), (0, 1), (1, 0), (1, 2), (2, 0), (3, 4)])
    }

    #[test]
    fn defaults_match_the_public_contract() {
        assert_eq!(
            CycleOptions::default(),
            CycleOptions {
                max_length: 10,
                max_cycles: 10_000,
            }
        );
    }

    #[test]
    fn reports_self_reciprocal_and_three_node_cycles_in_canonical_order() {
        let graph = fixture();
        let before = graph.clone();
        let result = simple_cycles(&graph, &CycleOptions::default()).expect("valid fixture");

        assert_eq!(
            result.cycles,
            vec![vec![id(0)], vec![id(0), id(1)], vec![id(0), id(1), id(2)]]
        );
        assert_eq!(result.nodes, graph.nodes);
        assert!(!result.truncated);
        assert_eq!(result.dropped_cycles, 0);
        assert_eq!(result.first_dropped_cycle, None);
        assert_eq!(graph, before, "cycle analysis is read-only");
    }

    #[test]
    fn rotation_is_canonical_but_direction_is_preserved() {
        assert!(canonical_rotation(Vec::new()).is_empty());
        assert_eq!(canonical_rotation(vec![4, 7, 2, 3]), vec![2, 3, 4, 7]);
        let graph = projection(3, [(0, 1), (1, 2), (2, 0), (0, 2), (2, 1), (1, 0)]);
        let result = simple_cycles(
            &graph,
            &CycleOptions {
                max_length: 3,
                max_cycles: 100,
            },
        )
        .expect("bidirectional triangle");

        assert!(result.cycles.contains(&vec![id(0), id(1), id(2)]));
        assert!(result.cycles.contains(&vec![id(0), id(2), id(1)]));
        assert_eq!(
            result
                .cycles
                .iter()
                .filter(|cycle| cycle.len() == 3)
                .count(),
            2
        );
        assert!(result
            .cycles
            .iter()
            .all(|cycle| cycle[0] == *cycle.iter().min().expect("nonempty cycle")));
    }

    #[test]
    fn length_and_count_limits_are_explicit() {
        let graph = fixture();
        let length_limited = simple_cycles(
            &graph,
            &CycleOptions {
                max_length: 2,
                max_cycles: 10,
            },
        )
        .expect("length-limited fixture");
        assert_eq!(length_limited.cycles, vec![vec![id(0)], vec![id(0), id(1)]]);
        assert!(!length_limited.truncated);

        let count_limited = simple_cycles(
            &graph,
            &CycleOptions {
                max_length: 3,
                max_cycles: 2,
            },
        )
        .expect("count-limited fixture");
        assert_eq!(count_limited.cycles, vec![vec![id(0)], vec![id(0), id(1)]]);
        assert!(count_limited.truncated);
        assert_eq!(count_limited.dropped_cycles, 1);
        assert_eq!(
            count_limited.first_dropped_cycle,
            Some(vec![id(0), id(1), id(2)])
        );
    }

    #[test]
    fn zero_limits_and_empty_graphs_are_total() {
        let cyclic = projection(1, [(0, 0)]);
        let no_length = simple_cycles(
            &cyclic,
            &CycleOptions {
                max_length: 0,
                max_cycles: 10,
            },
        )
        .expect("zero length");
        assert!(no_length.cycles.is_empty());
        assert!(!no_length.truncated);

        let no_capacity = simple_cycles(
            &cyclic,
            &CycleOptions {
                max_length: 1,
                max_cycles: 0,
            },
        )
        .expect("zero capacity");
        assert!(no_capacity.cycles.is_empty());
        assert!(no_capacity.truncated);
        assert_eq!(no_capacity.first_dropped_cycle, Some(vec![id(0)]));

        let empty = projection(0, []);
        assert!(simple_cycles(&empty, &CycleOptions::default())
            .expect("empty projection")
            .cycles
            .is_empty());
    }

    #[test]
    fn public_limits_are_clamped() {
        let graph = projection(
            MAX_CYCLE_LENGTH + 1,
            (0..MAX_CYCLE_LENGTH + 1).map(|node| (node, (node + 1) % (MAX_CYCLE_LENGTH + 1))),
        );
        let result = simple_cycles(
            &graph,
            &CycleOptions {
                max_length: usize::MAX,
                max_cycles: usize::MAX,
            },
        )
        .expect("long ring");
        assert_eq!(result.max_length, MAX_CYCLE_LENGTH);
        assert_eq!(result.max_cycles, MAX_SIMPLE_CYCLES);
        assert!(
            result.cycles.is_empty(),
            "the 33-cycle is beyond the hard cap"
        );
        assert!(!result.truncated);
    }

    #[test]
    fn malformed_public_projection_is_rejected_without_indexing() {
        let mut graph = projection(2, [(0, 1)]);
        graph.targets[0] = 7;
        assert!(matches!(
            simple_cycles(&graph, &CycleOptions::default()),
            Err(AnalysisError::InvalidInput {
                analysis: "simple_cycles",
                input: "projection",
                ..
            })
        ));
    }

    #[test]
    fn each_cycle_is_simple_unique_and_edge_complete() {
        let graph = projection(
            5,
            [
                (0, 0),
                (0, 1),
                (1, 2),
                (2, 0),
                (0, 2),
                (2, 3),
                (3, 0),
                (1, 3),
                (3, 4),
                (4, 1),
            ],
        );
        let result = simple_cycles(
            &graph,
            &CycleOptions {
                max_length: 5,
                max_cycles: 100,
            },
        )
        .expect("multi-cycle fixture");
        let unique: BTreeSet<_> = result.cycles.iter().cloned().collect();
        assert_eq!(unique.len(), result.cycles.len());
        for cycle in &result.cycles {
            let members: BTreeSet<_> = cycle.iter().copied().collect();
            assert_eq!(members.len(), cycle.len(), "cycle must be simple");
            assert_eq!(cycle[0], *members.first().expect("nonempty cycle"));
            for edge in 0..cycle.len() {
                let source = graph.dense_index(cycle[edge]).expect("cycle source");
                let target = graph
                    .dense_index(cycle[(edge + 1) % cycle.len()])
                    .expect("cycle target");
                assert!(graph.successors(source).binary_search(&target).is_ok());
            }
        }
        let mut ordered = result.cycles.clone();
        ordered.sort_unstable_by(|left, right| {
            left.len().cmp(&right.len()).then_with(|| left.cmp(right))
        });
        assert_eq!(result.cycles, ordered);
    }

    #[test]
    fn long_ring_is_stack_safe_and_respects_the_length_ceiling() {
        let node_count = 25_000;
        let graph = projection(
            node_count,
            (0..node_count).map(|node| (node, (node + 1) % node_count)),
        );
        let result = simple_cycles(
            &graph,
            &CycleOptions {
                max_length: usize::MAX,
                max_cycles: usize::MAX,
            },
        )
        .expect("long ring projection");

        assert!(result.cycles.is_empty());
        assert_eq!(result.max_length, MAX_CYCLE_LENGTH);
        assert!(!result.truncated);
    }

    fn graph_from_mask(node_count: usize, mask: u64, reverse: bool) -> GraphProjection {
        let mut edges = Vec::new();
        for source in 0..node_count {
            for target in 0..node_count {
                let bit = source * 8 + target;
                if mask & (1u64 << bit) != 0 {
                    edges.push((source, target));
                }
            }
        }
        if reverse {
            edges.reverse();
        }
        projection(node_count, edges)
    }

    fn brute_force_cycles(graph: &GraphProjection, max_length: usize) -> Vec<Vec<NodeId>> {
        fn permutations(
            graph: &GraphProjection,
            start: u32,
            target_length: usize,
            path: &mut Vec<u32>,
            used: &mut [bool],
            output: &mut BTreeSet<Vec<NodeId>>,
        ) {
            if path.len() == target_length {
                let last = *path.last().expect("nonempty permutation");
                let internal_edges_exist = path
                    .windows(2)
                    .all(|edge| graph.successors(edge[0]).binary_search(&edge[1]).is_ok());
                if internal_edges_exist && graph.successors(last).binary_search(&start).is_ok() {
                    output.insert(path.iter().map(|&dense| graph.node_id(dense)).collect());
                }
                return;
            }

            for candidate in start as usize + 1..graph.node_count() {
                if !used[candidate] {
                    used[candidate] = true;
                    path.push(candidate as u32);
                    permutations(graph, start, target_length, path, used, output);
                    path.pop();
                    used[candidate] = false;
                }
            }
        }

        let mut output = BTreeSet::new();
        for length in 1..=max_length.min(graph.node_count()) {
            for start in 0..graph.node_count() as u32 {
                if length == 1 {
                    if graph.successors(start).binary_search(&start).is_ok() {
                        output.insert(vec![graph.node_id(start)]);
                    }
                    continue;
                }
                let mut path = vec![start];
                let mut used = vec![false; graph.node_count()];
                used[start as usize] = true;
                permutations(graph, start, length, &mut path, &mut used, &mut output);
            }
        }
        let mut output: Vec<_> = output.into_iter().collect();
        output.sort_unstable_by(|left, right| {
            left.len().cmp(&right.len()).then_with(|| left.cmp(right))
        });
        output
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(96))]

        #[test]
        fn generated_cycles_match_exhaustive_permutations_and_insertion_order(
            node_count in 0usize..=8,
            mask in any::<u64>(),
        ) {
            let graph = graph_from_mask(node_count, mask, false);
            let options = CycleOptions {
                max_length: 8,
                max_cycles: MAX_SIMPLE_CYCLES,
            };
            let result = simple_cycles(&graph, &options).expect("generated projection");
            let expected = brute_force_cycles(&graph, options.max_length);
            prop_assert_eq!(&result.cycles, &expected);
            prop_assert!(!result.truncated);
            prop_assert_eq!(result.dropped_cycles, 0);
            prop_assert_eq!(&result, &simple_cycles(&graph, &options).expect("repeat result"));

            let reversed = graph_from_mask(node_count, mask, true);
            prop_assert_eq!(&result, &simple_cycles(&reversed, &options).expect("reversed insertion"));

            for cycle in &result.cycles {
                let unique: BTreeSet<_> = cycle.iter().collect();
                prop_assert_eq!(unique.len(), cycle.len());
                prop_assert_eq!(cycle.first(), cycle.iter().min());
                for edge in 0..cycle.len() {
                    let source = graph.dense_index(cycle[edge]).expect("source in graph");
                    let target = graph.dense_index(cycle[(edge + 1) % cycle.len()]).expect("target in graph");
                    prop_assert!(graph.successors(source).binary_search(&target).is_ok());
                }
            }
        }

        #[test]
        fn count_cap_returns_the_sorted_prefix_and_first_dropped_witness(
            node_count in 0usize..=7,
            mask in any::<u64>(),
            requested_cap in 0usize..32,
        ) {
            let graph = graph_from_mask(node_count, mask, false);
            let expected = brute_force_cycles(&graph, 7);
            let result = simple_cycles(
                &graph,
                &CycleOptions {
                    max_length: 7,
                    max_cycles: requested_cap,
                },
            ).expect("capped generated projection");
            let retained = expected.len().min(requested_cap);
            prop_assert_eq!(&result.cycles, &expected[..retained]);
            prop_assert_eq!(result.truncated, expected.len() > requested_cap);
            prop_assert_eq!(result.dropped_cycles, usize::from(expected.len() > requested_cap));
            prop_assert_eq!(
                result.first_dropped_cycle.as_ref(),
                expected.get(requested_cap),
            );
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn options_and_results_round_trip_with_serde() {
        let graph = fixture();
        let options = CycleOptions {
            max_length: 3,
            max_cycles: 2,
        };
        let result = simple_cycles(&graph, &options).expect("fixture result");
        let options_json = serde_json::to_string(&options).expect("serialize options");
        let result_json = serde_json::to_string(&result).expect("serialize result");
        assert_eq!(
            serde_json::from_str::<CycleOptions>(&options_json).expect("deserialize options"),
            options
        );
        assert_eq!(
            serde_json::from_str::<CycleSet>(&result_json).expect("deserialize result"),
            result
        );
    }
}
