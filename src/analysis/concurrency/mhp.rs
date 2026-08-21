//! May-happen-in-parallel relation over typed parallel-composition blocks.
//!
//! A frontend marks a parallel-composition [`CpgNodeKind::Block`] with
//! [`MsgKind::Spawn`] and retains its operands as sibling AST subtrees. This
//! module derives a deterministic, symmetric relation between executable
//! statement sites in different operands. The source CPG is never mutated.

use std::collections::BTreeSet;

use rustc_hash::FxHashSet;

use crate::{CodePropertyGraph, CpgNode, CpgNodeKind, MsgKind, NodeId};

/// Maximum number of canonical statement pairs returned by one analysis.
///
/// Parallel operands induce a Cartesian product, so a finite cap prevents an
/// adversarial AST from forcing unbounded quadratic output. A capped result is
/// an explicit under-approximation: every returned pair is valid, while later
/// valid pairs may be absent.
pub const MAX_MHP_PAIRS: usize = 1_000_000;

/// One canonical unordered pair in the may-happen-in-parallel relation.
///
/// `first < second` always holds. Logical membership is symmetric; use
/// [`MhpRelation::contains`] when operand order should be ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MhpPair {
    /// Lower stable statement id.
    pub first: NodeId,
    /// Higher stable statement id.
    pub second: NodeId,
}

impl MhpPair {
    pub(crate) fn canonical(left: NodeId, right: NodeId) -> Option<Self> {
        match left.cmp(&right) {
            std::cmp::Ordering::Less => Some(Self {
                first: left,
                second: right,
            }),
            std::cmp::Ordering::Greater => Some(Self {
                first: right,
                second: left,
            }),
            std::cmp::Ordering::Equal => None,
        }
    }
}

/// Deterministic out-of-graph may-happen-in-parallel relation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MhpRelation {
    /// Canonical pairs sorted by `(first, second)` and deduplicated.
    pub pairs: Vec<MhpPair>,
    /// Whether the fixed public pair cap truncated the relation.
    pub capped: bool,
}

impl MhpRelation {
    /// Returns the number of canonical unordered pairs.
    pub fn len(&self) -> usize {
        self.pairs.len()
    }

    /// Returns true when no pair was retained.
    pub fn is_empty(&self) -> bool {
        self.pairs.is_empty()
    }

    /// Tests symmetric membership. Self-pairs are always false.
    pub fn contains(&self, left: NodeId, right: NodeId) -> bool {
        MhpPair::canonical(left, right).is_some_and(|pair| self.pairs.binary_search(&pair).is_ok())
    }

    /// Iterates every statement paired with `node` in stable order.
    pub fn partners(&self, node: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        self.pairs.iter().filter_map(move |pair| {
            if pair.first == node {
                Some(pair.second)
            } else if pair.second == node {
                Some(pair.first)
            } else {
                None
            }
        })
    }
}

/// Computes the symmetric statement-level may-happen-in-parallel relation.
///
/// A pair is present exactly when its statements occur in different direct
/// operand subtrees of a frontend-normalized parallel-composition block. In a
/// well-formed AST, that block is their nearest common ancestor. Sequential
/// siblings and statements within the same operand are not related. Nested
/// functions are independent definitions and their bodies are not attributed
/// to the enclosing operand.
///
/// The returned relation is read-only, sorted, deduplicated, and capped at
/// [`MAX_MHP_PAIRS`]. No language grammar or source text is inspected.
pub fn may_happen_in_parallel(cpg: &CodePropertyGraph) -> MhpRelation {
    may_happen_in_parallel_with_limit(cpg, MAX_MHP_PAIRS)
}

fn may_happen_in_parallel_with_limit(cpg: &CodePropertyGraph, max_pairs: usize) -> MhpRelation {
    let mut parallel_blocks: Vec<_> = cpg
        .nodes()
        .filter(|node| {
            node.message_kind == Some(MsgKind::Spawn)
                && matches!(node.kind, CpgNodeKind::Block { .. })
        })
        .map(|node| node.id)
        .collect();
    parallel_blocks.sort_unstable();
    parallel_blocks.dedup();

    let mut pairs = BTreeSet::new();
    for parallel in parallel_blocks {
        let mut operands: Vec<_> = cpg
            .ast_children(parallel)
            .iter()
            .copied()
            .filter(|&child| cpg.contains_node(child))
            .collect();
        operands.sort_unstable();
        operands.dedup();

        let sites: Vec<_> = operands
            .iter()
            .map(|&operand| execution_sites(cpg, operand))
            .collect();
        for left_index in 0..sites.len() {
            for right_index in left_index + 1..sites.len() {
                for &left in &sites[left_index] {
                    for &right in &sites[right_index] {
                        let Some(pair) = MhpPair::canonical(left, right) else {
                            continue;
                        };
                        if pairs.contains(&pair) {
                            continue;
                        }
                        if pairs.len() == max_pairs {
                            return MhpRelation {
                                pairs: pairs.into_iter().collect(),
                                capped: true,
                            };
                        }
                        pairs.insert(pair);
                    }
                }
            }
        }
    }

    MhpRelation {
        pairs: pairs.into_iter().collect(),
        capped: false,
    }
}

fn execution_sites(cpg: &CodePropertyGraph, operand: NodeId) -> Vec<NodeId> {
    let mut result = BTreeSet::new();
    let mut visited = FxHashSet::default();
    let mut stack = vec![operand];

    while let Some(site) = stack.pop() {
        if !visited.insert(site) {
            continue;
        }
        let Some(node) = cpg.node(site) else {
            continue;
        };
        if matches!(node.kind, CpgNodeKind::Function { .. }) {
            continue;
        }
        if is_execution_site(node) {
            result.insert(site);
        }

        let mut children = cpg.ast_children(site).to_vec();
        children.sort_unstable_by(|left, right| right.cmp(left));
        stack.extend(children);
    }

    result.into_iter().collect()
}

pub(crate) fn is_execution_site(node: &CpgNode) -> bool {
    node.is_statement()
        || matches!(
            node.kind,
            CpgNodeKind::Variable { .. }
                | CpgNodeKind::Assignment { .. }
                | CpgNodeKind::Call { .. }
                | CpgNodeKind::Await
                | CpgNodeKind::Yield
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CpgEdgeKind, Language, LiteralKind, ScopeId, SourceRange};
    use proptest::prelude::*;

    fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(id), kind, SourceRange::from_bytes(id, id + 1))
    }

    fn block(id: u32, parallel: bool) -> CpgNode {
        let node = node(
            id,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        if parallel {
            node.with_message_kind(MsgKind::Spawn)
        } else {
            node
        }
    }

    fn call(id: u32) -> CpgNode {
        node(
            id,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        )
    }

    fn add_ast(cpg: &mut CodePropertyGraph, parent: u32, child: u32) {
        cpg.connect(
            NodeId::new(parent),
            NodeId::new(child),
            CpgEdgeKind::AstChild,
        );
    }

    fn basic_fixture(reverse: bool) -> CodePropertyGraph {
        let mut nodes = vec![
            block(10, true),
            block(20, false),
            block(30, false),
            call(21),
            node(22, CpgNodeKind::Return),
            call(31),
            block(40, false),
            block(50, false),
            block(60, false),
            call(51),
            call(61),
        ];
        let mut edges = vec![
            (10, 20),
            (10, 30),
            (20, 21),
            (20, 22),
            (30, 31),
            (40, 50),
            (40, 60),
            (50, 51),
            (60, 61),
        ];
        if reverse {
            nodes.reverse();
            edges.reverse();
        }
        let mut cpg = CodePropertyGraph::new(Language::Rholang);
        for node in nodes {
            cpg.add_node_with_id(node);
        }
        for (parent, child) in edges {
            add_ast(&mut cpg, parent, child);
        }
        cpg
    }

    fn generated_fixture(left_count: u32, right_count: u32, reverse: bool) -> CodePropertyGraph {
        let mut nodes = vec![block(10, true), block(20, false), block(30, false)];
        let mut edges = vec![(10, 20), (10, 30)];
        for offset in 0..left_count {
            nodes.push(call(100 + offset));
            edges.push((20, 100 + offset));
        }
        for offset in 0..right_count {
            nodes.push(call(200 + offset));
            edges.push((30, 200 + offset));
        }
        if reverse {
            nodes.reverse();
            edges.reverse();
        }
        let mut cpg = CodePropertyGraph::new(Language::Rholang);
        for node in nodes {
            cpg.add_node_with_id(node);
        }
        for (parent, child) in edges {
            add_ast(&mut cpg, parent, child);
        }
        cpg
    }

    #[test]
    fn different_parallel_operands_are_related_but_same_and_sequential_subtrees_are_not() {
        let cpg = basic_fixture(false);
        let relation = may_happen_in_parallel(&cpg);
        assert_eq!(
            relation.pairs,
            vec![
                MhpPair {
                    first: NodeId::new(21),
                    second: NodeId::new(31),
                },
                MhpPair {
                    first: NodeId::new(22),
                    second: NodeId::new(31),
                },
            ]
        );
        assert!(relation.contains(NodeId::new(21), NodeId::new(31)));
        assert!(relation.contains(NodeId::new(31), NodeId::new(21)));
        assert!(!relation.contains(NodeId::new(21), NodeId::new(22)));
        assert!(!relation.contains(NodeId::new(51), NodeId::new(61)));
        assert!(!relation.contains(NodeId::new(21), NodeId::new(21)));
        assert!(!relation.capped);
    }

    #[test]
    fn direct_process_roots_are_valid_parallel_operands() {
        let mut cpg = CodePropertyGraph::new(Language::Rholang);
        for node in [block(10, true), call(20), call(30)] {
            cpg.add_node_with_id(node);
        }
        add_ast(&mut cpg, 10, 20);
        add_ast(&mut cpg, 10, 30);

        let relation = may_happen_in_parallel(&cpg);
        assert_eq!(relation.len(), 1);
        assert!(relation.contains(NodeId::new(20), NodeId::new(30)));
    }

    #[test]
    fn execution_site_vocabulary_includes_effectful_nodes_and_excludes_value_syntax() {
        let mut cpg = CodePropertyGraph::new(Language::Rholang);
        for node in [
            block(10, true),
            block(20, false),
            block(30, false),
            node(
                21,
                CpgNodeKind::Variable {
                    name: "x".into(),
                    var_type: None,
                    scope: ScopeId::GLOBAL,
                    is_mutable: false,
                },
            ),
            node(
                22,
                CpgNodeKind::Assignment {
                    operator: "=".into(),
                },
            ),
            call(23),
            node(24, CpgNodeKind::Await),
            node(25, CpgNodeKind::Yield),
            node(
                26,
                CpgNodeKind::Identifier {
                    name: "x".into(),
                    definition: Some(NodeId::new(21)),
                },
            ),
            node(
                27,
                CpgNodeKind::Literal {
                    kind: LiteralKind::Integer(1),
                },
            ),
            node(
                28,
                CpgNodeKind::BinaryOp {
                    operator: "+".into(),
                },
            ),
            call(31),
        ] {
            cpg.add_node_with_id(node);
        }
        for child in 21..=28 {
            add_ast(&mut cpg, 20, child);
        }
        add_ast(&mut cpg, 10, 20);
        add_ast(&mut cpg, 10, 30);
        add_ast(&mut cpg, 30, 31);

        let relation = may_happen_in_parallel(&cpg);
        assert_eq!(relation.len(), 5);
        for site in 21..=25 {
            assert!(relation.contains(NodeId::new(site), NodeId::new(31)));
        }
        for syntax in 26..=28 {
            assert!(!relation.contains(NodeId::new(syntax), NodeId::new(31)));
        }
    }

    #[test]
    fn nested_parallel_regions_compose_without_relating_same_operand_statements() {
        let mut cpg = CodePropertyGraph::new(Language::Rholang);
        for node in [
            block(10, true),
            block(20, false),
            block(30, false),
            block(40, true),
            block(50, false),
            block(60, false),
            call(31),
            call(51),
            call(61),
        ] {
            cpg.add_node_with_id(node);
        }
        for (parent, child) in [
            (10, 20),
            (10, 30),
            (20, 40),
            (30, 31),
            (40, 50),
            (40, 60),
            (50, 51),
            (60, 61),
        ] {
            add_ast(&mut cpg, parent, child);
        }

        let relation = may_happen_in_parallel(&cpg);
        for pair in [(51, 61), (51, 31), (61, 31)] {
            assert!(relation.contains(NodeId::new(pair.0), NodeId::new(pair.1)));
        }
        assert_eq!(relation.len(), 3);
    }

    #[test]
    fn nested_function_body_is_not_attributed_to_the_enclosing_operand() {
        let mut cpg = basic_fixture(false);
        cpg.add_node_with_id(node(
            23,
            CpgNodeKind::Function {
                signature: Box::new(crate::MethodSignature {
                    name: "later".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: crate::Visibility::Private,
                }),
            },
        ));
        cpg.add_node_with_id(call(24));
        add_ast(&mut cpg, 20, 23);
        add_ast(&mut cpg, 23, 24);

        let relation = may_happen_in_parallel(&cpg);
        assert!(!relation.contains(NodeId::new(24), NodeId::new(31)));
    }

    #[test]
    fn explicit_cap_is_a_deterministic_under_approximation() {
        let cpg = basic_fixture(false);
        let empty = may_happen_in_parallel_with_limit(&cpg, 0);
        assert!(empty.is_empty());
        assert!(empty.capped);

        let one = may_happen_in_parallel_with_limit(&cpg, 1);
        assert_eq!(one.len(), 1);
        assert!(one.capped);
        assert_eq!(one.pairs[0].first, NodeId::new(21));
        assert_eq!(one.pairs[0].second, NodeId::new(31));

        let complete = may_happen_in_parallel_with_limit(&cpg, 2);
        assert_eq!(complete.len(), 2);
        assert!(!complete.capped);
    }

    #[test]
    fn analysis_is_read_only_and_queries_unknown_nodes_totally() {
        let cpg = basic_fixture(false);
        let before = (cpg.node_count(), cpg.edge_count());
        let relation = may_happen_in_parallel(&cpg);
        assert_eq!(before, (cpg.node_count(), cpg.edge_count()));
        assert!(!relation.contains(NodeId::new(u32::MAX), NodeId::new(21)));
        assert!(relation
            .partners(NodeId::new(31))
            .eq([NodeId::new(21), NodeId::new(22)]));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn relation_round_trips_through_serde() {
        let relation = may_happen_in_parallel(&basic_fixture(false));
        let json = serde_json::to_string(&relation).expect("serialize MHP relation");
        let round_trip: MhpRelation =
            serde_json::from_str(&json).expect("deserialize MHP relation");
        assert_eq!(round_trip, relation);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn generated_parallel_products_are_symmetric_sorted_and_insertion_order_independent(
            left_count in 1u32..7,
            right_count in 1u32..7,
        ) {
            let cpg = generated_fixture(left_count, right_count, false);
            let relation = may_happen_in_parallel(&cpg);
            let reversed = may_happen_in_parallel(&generated_fixture(left_count, right_count, true));
            let expected: Vec<_> = (0..left_count)
                .flat_map(|left| {
                    (0..right_count).map(move |right| MhpPair {
                        first: NodeId::new(100 + left),
                        second: NodeId::new(200 + right),
                    })
                })
                .collect();

            prop_assert_eq!(&relation.pairs, &expected);
            prop_assert_eq!(&reversed, &relation);
            prop_assert!(!relation.capped);
            prop_assert!(relation.pairs.windows(2).all(|window| window[0] < window[1]));
            for left in 0..left_count {
                for right in 0..right_count {
                    prop_assert!(relation.contains(NodeId::new(100 + left), NodeId::new(200 + right)));
                    prop_assert!(relation.contains(NodeId::new(200 + right), NodeId::new(100 + left)));
                }
                for other_left in 0..left_count {
                    prop_assert!(!relation.contains(NodeId::new(100 + left), NodeId::new(100 + other_left)));
                }
            }
            for right in 0..right_count {
                for other_right in 0..right_count {
                    prop_assert!(!relation.contains(NodeId::new(200 + right), NodeId::new(200 + other_right)));
                }
            }
        }
    }
}
