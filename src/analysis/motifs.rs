//! Deterministic directed-triad and bounded four-node graphlet census.
//!
//! The triad classifier implements the complete Davis–Leinhardt 16-class
//! table. Directed topology is represented as bounded dense bit rows so every
//! edge query is constant-time, independent of hash order, and payload-free.

use super::{AnalysisError, GraphProjection};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Hard ceiling on projection vertices admitted to the triad census.
pub const MAX_TRIAD_NODES: usize = 512;

/// Hard ceiling on projection vertices admitted to the four-node census.
pub const MAX_GRAPHLET_NODES: usize = 50;

/// Davis–Leinhardt triad names in [`MotifCensus::triads`] index order.
pub const TRIAD_NAMES: [&str; 16] = [
    "003", "012", "102", "021D", "021U", "021C", "111D", "111U", "030T", "030C", "201", "120D",
    "120U", "120C", "210", "300",
];

/// Resource controls for [`motif_census`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MotifOptions {
    /// Maximum projection size admitted to the three-node census.
    ///
    /// Values above [`MAX_TRIAD_NODES`] are clamped to the hard ceiling.
    pub max_nodes_triads: usize,
    /// Maximum projection size admitted to the four-node census.
    ///
    /// Values above [`MAX_GRAPHLET_NODES`] are clamped to the hard ceiling.
    pub max_nodes_graphlets: usize,
}

impl Default for MotifOptions {
    fn default() -> Self {
        Self {
            max_nodes_triads: MAX_TRIAD_NODES,
            max_nodes_graphlets: MAX_GRAPHLET_NODES,
        }
    }
}

/// Complete three-node triads and selected four-node graphlets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MotifCensus {
    /// Directed triads in [`TRIAD_NAMES`] order.
    ///
    /// Every unordered three-vertex subset contributes exactly once. Self-loops
    /// and numeric edge weights do not affect the classification.
    pub triads: [u64; 16],
    /// Selected four-node graphlet counts.
    ///
    /// Index zero counts complete bidirected four-cliques (all twelve arcs).
    /// Index one counts out-star embeddings: one center reaches three leaves
    /// having no arcs among themselves. Reverse leaf-to-center arcs are inert,
    /// matching the source algorithm's topology contract.
    pub graphlets_4: [u64; 2],
}

/// Computes the full directed triad census and selected four-node graphlets.
///
/// Projection CSR is validated before traversal. Parallel-edge multiplicity,
/// weights, and self-loops are ignored; all other directions are preserved.
/// Enumeration follows ascending dense vertex order, and the returned fixed
/// arrays therefore have no order-sensitive surface.
///
/// The projection size must fit both effective option limits. Exceeding either
/// limit returns [`AnalysisError::BudgetExceeded`] rather than silently
/// omitting part of the requested census.
pub fn motif_census(
    graph: &GraphProjection,
    options: &MotifOptions,
) -> Result<MotifCensus, AnalysisError> {
    validate_projection(graph)?;
    let node_count = graph.node_count();
    let triad_cap = options.max_nodes_triads.min(MAX_TRIAD_NODES);
    if node_count > triad_cap {
        return Err(AnalysisError::BudgetExceeded {
            analysis: "motif_census",
            unit: "triad nodes",
            needed: node_count as u64,
            cap: triad_cap as u64,
        });
    }
    let graphlet_cap = options.max_nodes_graphlets.min(MAX_GRAPHLET_NODES);
    if node_count > graphlet_cap {
        return Err(AnalysisError::BudgetExceeded {
            analysis: "motif_census",
            unit: "graphlet nodes",
            needed: node_count as u64,
            cap: graphlet_cap as u64,
        });
    }

    let adjacency = AdjacencyBits::from_projection(graph);
    let mut census = MotifCensus::default();
    for first in 0..node_count {
        for second in first + 1..node_count {
            for third in second + 1..node_count {
                let code = triad_code(&adjacency, first, second, third);
                census.triads[TRIAD_CLASS_BY_CODE[code]] += 1;
            }
        }
    }

    for first in 0..node_count {
        for second in first + 1..node_count {
            for third in second + 1..node_count {
                for fourth in third + 1..node_count {
                    let nodes = [first, second, third, fourth];
                    if is_bidirected_clique(&adjacency, nodes) {
                        census.graphlets_4[0] += 1;
                    }
                    census.graphlets_4[1] += out_star_count(&adjacency, nodes);
                }
            }
        }
    }
    Ok(census)
}

#[derive(Debug)]
struct AdjacencyBits {
    words_per_row: usize,
    bits: Vec<u64>,
}

impl AdjacencyBits {
    fn from_projection(graph: &GraphProjection) -> Self {
        let node_count = graph.node_count();
        let words_per_row = node_count.div_ceil(u64::BITS as usize);
        let mut bits = vec![0; node_count * words_per_row];
        for source in 0..node_count {
            for &target in graph.successors(source as u32) {
                let target = target as usize;
                if source != target {
                    let offset = source * words_per_row + target / u64::BITS as usize;
                    bits[offset] |= 1 << (target % u64::BITS as usize);
                }
            }
        }
        Self {
            words_per_row,
            bits,
        }
    }

    fn has(&self, source: usize, target: usize) -> bool {
        let offset = source * self.words_per_row + target / u64::BITS as usize;
        self.bits[offset] & (1 << (target % u64::BITS as usize)) != 0
    }
}

fn triad_code(adjacency: &AdjacencyBits, first: usize, second: usize, third: usize) -> usize {
    usize::from(adjacency.has(first, second))
        | (usize::from(adjacency.has(second, first)) << 1)
        | (usize::from(adjacency.has(first, third)) << 2)
        | (usize::from(adjacency.has(third, first)) << 3)
        | (usize::from(adjacency.has(second, third)) << 4)
        | (usize::from(adjacency.has(third, second)) << 5)
}

// Complete Batagelj–Mrvar tricode lookup in Davis–Leinhardt index order. The
// table fixes the source implementation's former 111D/111U and 120D/U/C
// collapses. Index bits are a→b, b→a, a→c, c→a, b→c, c→b.
const TRIAD_CLASS_BY_CODE: [usize; 64] = [
    0, 1, 1, 2, 1, 3, 5, 7, 1, 5, 4, 6, 2, 7, 6, 10, 1, 5, 3, 7, 4, 8, 8, 12, 5, 9, 8, 13, 6, 13,
    11, 14, 1, 4, 5, 6, 5, 8, 9, 13, 3, 8, 8, 11, 7, 12, 13, 14, 2, 6, 7, 10, 6, 11, 13, 14, 7, 13,
    12, 14, 10, 14, 14, 15,
];

fn is_bidirected_clique(adjacency: &AdjacencyBits, nodes: [usize; 4]) -> bool {
    for left in 0..nodes.len() {
        for right in left + 1..nodes.len() {
            if !adjacency.has(nodes[left], nodes[right])
                || !adjacency.has(nodes[right], nodes[left])
            {
                return false;
            }
        }
    }
    true
}

fn out_star_count(adjacency: &AdjacencyBits, nodes: [usize; 4]) -> u64 {
    let mut count = 0;
    for center_index in 0..nodes.len() {
        let center = nodes[center_index];
        let mut leaves = [0; 3];
        let mut next_leaf = 0;
        for &node in &nodes {
            if node != center {
                leaves[next_leaf] = node;
                next_leaf += 1;
            }
        }
        if !leaves.iter().all(|&leaf| adjacency.has(center, leaf)) {
            continue;
        }
        let leaves_are_independent = (0..leaves.len()).all(|left| {
            (left + 1..leaves.len()).all(|right| {
                !adjacency.has(leaves[left], leaves[right])
                    && !adjacency.has(leaves[right], leaves[left])
            })
        });
        count += u64::from(leaves_are_independent);
    }
    count
}

fn validate_projection(graph: &GraphProjection) -> Result<(), AnalysisError> {
    if graph.is_well_formed() {
        Ok(())
    } else {
        Err(AnalysisError::InvalidInput {
            analysis: "motif_census",
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
    use crate::{NodeId, ProjectionKind};

    const TRIAD_FIXTURES: [(&str, &[(usize, usize)]); 16] = [
        ("003", &[]),
        ("012", &[(0, 1)]),
        ("102", &[(0, 1), (1, 0)]),
        ("021D", &[(0, 1), (0, 2)]),
        ("021U", &[(1, 0), (2, 0)]),
        ("021C", &[(1, 0), (0, 2)]),
        ("111D", &[(0, 1), (1, 0), (2, 0)]),
        ("111U", &[(0, 1), (1, 0), (0, 2)]),
        ("030T", &[(0, 1), (0, 2), (1, 2)]),
        ("030C", &[(0, 1), (1, 2), (2, 0)]),
        ("201", &[(0, 1), (1, 0), (0, 2), (2, 0)]),
        ("120D", &[(1, 0), (0, 2), (2, 0), (1, 2)]),
        ("120U", &[(0, 1), (1, 0), (0, 2), (1, 2)]),
        ("120C", &[(0, 1), (1, 0), (2, 0), (1, 2)]),
        ("210", &[(0, 1), (1, 0), (0, 2), (2, 0), (1, 2)]),
        ("300", &[(0, 1), (1, 0), (0, 2), (2, 0), (1, 2), (2, 1)]),
    ];

    fn id(dense: usize) -> NodeId {
        NodeId::new(11 + dense as u32 * 13)
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

    #[test]
    fn defaults_and_names_match_the_public_contract() {
        assert_eq!(
            MotifOptions::default(),
            MotifOptions {
                max_nodes_triads: 512,
                max_nodes_graphlets: 50,
            }
        );
        assert_eq!(
            TRIAD_NAMES.map(str::to_owned),
            TRIAD_FIXTURES.map(|item| item.0.to_owned())
        );
    }

    #[test]
    fn hand_fixtures_cover_every_directed_triad_class() {
        for (expected_class, (name, edges)) in TRIAD_FIXTURES.iter().enumerate() {
            let result = motif_census(
                &projection(3, edges.iter().copied()),
                &MotifOptions::default(),
            )
            .expect("three-node fixture is within every cap");
            let mut expected = [0; 16];
            expected[expected_class] = 1;
            assert_eq!(result.triads, expected, "fixture {name}");
            assert_eq!(result.graphlets_4, [0, 0], "fixture {name}");
        }
    }

    #[test]
    fn clique_and_out_star_graphlets_have_exact_counts() {
        let clique = projection(
            4,
            (0..4).flat_map(|source| {
                (0..4)
                    .filter(move |&target| target != source)
                    .map(move |target| (source, target))
            }),
        );
        let clique_census = motif_census(&clique, &MotifOptions::default()).expect("clique");
        assert_eq!(clique_census.triads[15], 4);
        assert_eq!(clique_census.triads.iter().sum::<u64>(), 4);
        assert_eq!(clique_census.graphlets_4, [1, 0]);

        let star = projection(4, [(0, 1), (0, 2), (0, 3)]);
        let star_census = motif_census(&star, &MotifOptions::default()).expect("out-star");
        assert_eq!(star_census.triads[0], 1);
        assert_eq!(star_census.triads[3], 3);
        assert_eq!(star_census.graphlets_4, [0, 1]);

        let reciprocal_spokes = projection(4, [(0, 1), (1, 0), (0, 2), (2, 0), (0, 3), (3, 0)]);
        assert_eq!(
            motif_census(&reciprocal_spokes, &MotifOptions::default())
                .expect("reciprocal spokes")
                .graphlets_4,
            [0, 1],
            "reverse spoke arcs do not change the source out-star contract"
        );
    }

    #[test]
    fn empty_small_weighted_and_self_loop_inputs_are_total() {
        for node_count in 0..3 {
            let result = motif_census(
                &projection(node_count, (0..node_count).map(|node| (node, node))),
                &MotifOptions::default(),
            )
            .expect("small input");
            assert_eq!(result, MotifCensus::default());
        }

        let weighted = GraphProjection::from_edges(
            ProjectionKind::External,
            (0..3).map(id),
            [
                (id(0), id(0), -7.0),
                (id(0), id(1), 2.5),
                (id(0), id(2), f64::INFINITY),
                (id(1), id(2), f64::NAN),
            ],
        );
        let result = motif_census(&weighted, &MotifOptions::default()).expect("weighted topology");
        assert_eq!(result.triads[8], 1);
        assert_eq!(result.triads.iter().sum::<u64>(), 1);
    }

    #[test]
    fn configured_and_hard_caps_return_typed_errors() {
        let triad_limited = projection(4, []);
        assert!(matches!(
            motif_census(
                &triad_limited,
                &MotifOptions {
                    max_nodes_triads: 3,
                    max_nodes_graphlets: 50,
                }
            ),
            Err(AnalysisError::BudgetExceeded {
                analysis: "motif_census",
                unit: "triad nodes",
                needed: 4,
                cap: 3,
            })
        ));

        let graphlet_limited = projection(MAX_GRAPHLET_NODES + 1, []);
        assert!(matches!(
            motif_census(&graphlet_limited, &MotifOptions::default()),
            Err(AnalysisError::BudgetExceeded {
                analysis: "motif_census",
                unit: "graphlet nodes",
                needed: 51,
                cap: 50,
            })
        ));

        let beyond_both_hard_caps = projection(MAX_TRIAD_NODES + 1, []);
        assert!(matches!(
            motif_census(
                &beyond_both_hard_caps,
                &MotifOptions {
                    max_nodes_triads: usize::MAX,
                    max_nodes_graphlets: usize::MAX,
                }
            ),
            Err(AnalysisError::BudgetExceeded {
                unit: "triad nodes",
                needed: 513,
                cap: 512,
                ..
            })
        ));
    }

    #[test]
    fn malformed_projection_is_rejected_before_indexing() {
        let mut graph = projection(3, [(0, 1)]);
        graph.targets[0] = 9;
        assert!(matches!(
            motif_census(&graph, &MotifOptions::default()),
            Err(AnalysisError::InvalidInput {
                analysis: "motif_census",
                input: "projection",
                ..
            })
        ));
    }

    fn graph_from_mask(node_count: usize, mask: u64, reverse: bool) -> GraphProjection {
        let mut edges = Vec::new();
        for source in 0..node_count {
            for target in 0..node_count {
                if mask & (1 << (source * 8 + target)) != 0 {
                    edges.push((source, target));
                }
            }
        }
        if reverse {
            edges.reverse();
        }
        projection(node_count, edges)
    }

    fn code_has(code: u8, source: usize, target: usize) -> bool {
        let bit = match (source, target) {
            (0, 1) => 0,
            (1, 0) => 1,
            (0, 2) => 2,
            (2, 0) => 3,
            (1, 2) => 4,
            (2, 1) => 5,
            _ => return false,
        };
        code & (1 << bit) != 0
    }

    fn isomorphic(actual: u8, representative: u8) -> bool {
        const PERMUTATIONS: [[usize; 3]; 6] = [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ];
        PERMUTATIONS.iter().any(|permutation| {
            (0..3).all(|source| {
                (0..3).all(|target| {
                    source == target
                        || code_has(representative, source, target)
                            == code_has(actual, permutation[source], permutation[target])
                })
            })
        })
    }

    fn independent_class(code: u8) -> usize {
        const REPRESENTATIVES: [u8; 16] =
            [0, 1, 3, 5, 10, 6, 11, 7, 21, 25, 15, 30, 23, 27, 31, 63];
        REPRESENTATIVES
            .iter()
            .position(|&representative| isomorphic(code, representative))
            .expect("the representatives partition all directed triads")
    }

    fn independent_census(node_count: usize, mask: u64) -> MotifCensus {
        let edge = |source: usize, target: usize| mask & (1 << (source * 8 + target)) != 0;
        let mut result = MotifCensus::default();
        for first in 0..node_count {
            for second in first + 1..node_count {
                for third in second + 1..node_count {
                    let code = u8::from(edge(first, second))
                        | (u8::from(edge(second, first)) << 1)
                        | (u8::from(edge(first, third)) << 2)
                        | (u8::from(edge(third, first)) << 3)
                        | (u8::from(edge(second, third)) << 4)
                        | (u8::from(edge(third, second)) << 5);
                    result.triads[independent_class(code)] += 1;
                }
            }
        }

        for first in 0..node_count {
            for second in first + 1..node_count {
                for third in second + 1..node_count {
                    for fourth in third + 1..node_count {
                        let nodes = [first, second, third, fourth];
                        let clique = (0..4).all(|left| {
                            (left + 1..4).all(|right| {
                                edge(nodes[left], nodes[right]) && edge(nodes[right], nodes[left])
                            })
                        });
                        result.graphlets_4[0] += u64::from(clique);
                        for &center in &nodes {
                            let leaves: Vec<_> = nodes
                                .iter()
                                .copied()
                                .filter(|&node| node != center)
                                .collect();
                            let spokes = leaves.iter().all(|&leaf| edge(center, leaf));
                            let independent = (0..3).all(|left| {
                                (left + 1..3).all(|right| {
                                    !edge(leaves[left], leaves[right])
                                        && !edge(leaves[right], leaves[left])
                                })
                            });
                            result.graphlets_4[1] += u64::from(spokes && independent);
                        }
                    }
                }
            }
        }
        result
    }

    #[test]
    fn lookup_table_agrees_with_explicit_isomorphism_partition() {
        assert!(!code_has(63, 0, 0), "self-loops are outside the tricode");
        for code in 0..64u8 {
            assert_eq!(
                TRIAD_CLASS_BY_CODE[code as usize],
                independent_class(code),
                "tricode {code:06b}"
            );
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn generated_census_matches_independent_oracle_and_is_deterministic(
            node_count in 0usize..=8,
            mask in any::<u64>(),
        ) {
            let graph = graph_from_mask(node_count, mask, false);
            let result = motif_census(&graph, &MotifOptions::default())
                .expect("generated graph is below both hard caps");
            let expected = independent_census(node_count, mask);
            prop_assert_eq!(&result, &expected);
            prop_assert_eq!(
                result.triads.iter().sum::<u64>(),
                u64::try_from(node_count.saturating_sub(2) * node_count.saturating_sub(1) * node_count / 6)
                    .expect("small binomial coefficient"),
            );
            prop_assert_eq!(
                &result,
                &motif_census(&graph, &MotifOptions::default()).expect("repeat census"),
            );
            let reversed = graph_from_mask(node_count, mask, true);
            prop_assert_eq!(
                &result,
                &motif_census(&reversed, &MotifOptions::default())
                    .expect("reversed insertion census"),
            );
        }
    }

    #[test]
    fn every_reported_triad_is_accounted_for_once() {
        let graph = projection(
            8,
            [
                (0, 1),
                (1, 0),
                (0, 2),
                (2, 3),
                (3, 0),
                (4, 5),
                (4, 6),
                (4, 7),
            ],
        );
        let result = motif_census(&graph, &MotifOptions::default()).expect("mixed fixture");
        let nonzero: BTreeSet<_> = result
            .triads
            .iter()
            .enumerate()
            .filter_map(|(class, &count)| (count > 0).then_some(class))
            .collect();
        assert!(!nonzero.is_empty());
        assert_eq!(result.triads.iter().sum::<u64>(), 56);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn options_and_census_round_trip_with_serde() {
        let options = MotifOptions::default();
        let result = motif_census(&projection(3, [(0, 1)]), &options).expect("one-edge triad");
        let options_json = serde_json::to_string(&options).expect("serialize options");
        let result_json = serde_json::to_string(&result).expect("serialize census");
        assert_eq!(
            serde_json::from_str::<MotifOptions>(&options_json).expect("deserialize options"),
            options
        );
        assert_eq!(
            serde_json::from_str::<MotifCensus>(&result_json).expect("deserialize census"),
            result
        );
    }
}
