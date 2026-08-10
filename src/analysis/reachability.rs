//! Deterministic, read-only reachability and dead-branch reporting.
//!
//! Reachability starts at a function entry and follows only intraprocedural
//! control-flow edges. The result partitions every function-scoped CFG node
//! into reachable and unreachable sets without changing the source CPG.

use rustc_hash::FxHashSet;

use crate::{CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, NodeId};

use super::{
    cfg_projection,
    cfg_scope::{cfg_kind_rank, function_cfg_view},
    AnalysisError,
};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// One conditional CFG edge whose target cannot be reached from the function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DeadBranch {
    /// Conditional guard or match dispatch node.
    pub guard: NodeId,
    /// Original conditional CFG edge classification.
    pub edge_kind: CfgEdgeKind,
    /// Branch target that is unreachable from the function entry.
    pub target: NodeId,
}

/// Complete reachable/unreachable partition for one function-scoped CFG.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ReachabilityReport {
    /// Function whose intraprocedural CFG was analyzed.
    pub function: NodeId,
    /// Nodes reachable from `function`, sorted by stable node id.
    pub reachable: Vec<NodeId>,
    /// Function-scoped CFG nodes not reachable from `function`, sorted by id.
    pub unreachable: Vec<NodeId>,
    /// Conditional edges in unreachable CFG islands, sorted canonically.
    ///
    /// Constant-proven dead branches are added by SCCP refinement.
    pub dead_branches: Vec<DeadBranch>,
}

impl ReachabilityReport {
    /// Returns whether `node` belongs to the reachable partition.
    pub fn is_reachable(&self, node: NodeId) -> bool {
        self.reachable.binary_search(&node).is_ok()
    }
}

/// Partitions one function's CFG into reachable and unreachable nodes.
///
/// The universe contains the entry-reachable projection plus every endpoint of
/// a non-`Call` CFG edge whose endpoints both belong to the function's AST
/// scope. The union retains CFG-only hand-built graphs that have no AST scope;
/// nested functions remain separate whenever scope evidence exists. Traversal
/// is iterative, so long CFG paths cannot overflow the native stack. The CPG is
/// borrowed immutably and the report is entirely out-of-graph.
pub fn unreachable_code(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<ReachabilityReport, AnalysisError> {
    let view = function_cfg_view(cpg, function)?;
    let scoped_cfg = cfg_projection(cpg, function)?;
    let mut universe_nodes = scoped_cfg.nodes;
    universe_nodes.extend(view.projection.nodes.iter().copied());
    universe_nodes.sort_unstable();
    universe_nodes.dedup();

    let mut reachable_dense = FxHashSet::default();
    let mut stack = vec![view.entry];
    while let Some(node) = stack.pop() {
        if !reachable_dense.insert(node) {
            continue;
        }
        stack.extend(view.projection.successors(node).iter().rev().copied());
    }

    let reachable_set: FxHashSet<NodeId> = reachable_dense
        .into_iter()
        .map(|dense| view.projection.node_id(dense))
        .collect();
    let (reachable, unreachable): (Vec<_>, Vec<_>) = universe_nodes
        .iter()
        .copied()
        .partition(|node| reachable_set.contains(node));

    let universe_set: FxHashSet<NodeId> = universe_nodes.iter().copied().collect();
    let mut dead_branches: Vec<_> = cpg
        .edges()
        .filter_map(|edge| {
            let CpgEdgeKind::ControlFlow(edge_kind) = edge.kind else {
                return None;
            };
            (edge_kind.is_conditional()
                && universe_set.contains(&edge.source)
                && universe_set.contains(&edge.target)
                && !reachable_set.contains(&edge.target))
            .then_some(DeadBranch {
                guard: edge.source,
                edge_kind,
                target: edge.target,
            })
        })
        .collect();
    dead_branches.sort_unstable_by_key(|branch| {
        (branch.guard, cfg_kind_rank(branch.edge_kind), branch.target)
    });
    dead_branches.dedup();

    Ok(ReachabilityReport {
        function,
        reachable,
        unreachable,
        dead_branches,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use petgraph::{graph::DiGraph, visit::Bfs};
    use proptest::prelude::*;

    use super::*;
    use crate::{
        builder::CfgExtractor, CpgNode, CpgNodeKind, Language, MethodSignature, ScopeId,
        SourceRange, Visibility,
    };

    fn node_with_id(id: u32, kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(id), kind, SourceRange::default())
    }

    fn function(name: &str) -> CpgNodeKind {
        CpgNodeKind::Function {
            signature: MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            },
        }
    }

    fn block() -> CpgNodeKind {
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        }
    }

    fn connect_ast(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId) {
        cpg.connect(source, target, CpgEdgeKind::AstChild);
    }

    fn connect_cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId, kind: CfgEdgeKind) {
        cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind));
    }

    #[test]
    fn partitions_disconnected_cfg_islands_and_reports_their_branches() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function_id = cpg.add_node_with_id(node_with_id(10, function("f")));
        let body = cpg.add_node_with_id(node_with_id(20, block()));
        let returned = cpg.add_node_with_id(node_with_id(30, CpgNodeKind::Return));
        let guard = cpg.add_node_with_id(node_with_id(40, CpgNodeKind::If));
        let dead_true = cpg.add_node_with_id(node_with_id(50, block()));
        let dead_false = cpg.add_node_with_id(node_with_id(60, block()));

        for child in [body, returned, guard, dead_true, dead_false] {
            connect_ast(&mut cpg, function_id, child);
        }
        connect_cfg(&mut cpg, function_id, body, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, body, returned, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, guard, dead_true, CfgEdgeKind::ConditionalTrue);
        connect_cfg(&mut cpg, guard, dead_false, CfgEdgeKind::ConditionalFalse);

        let before = (cpg.node_count(), cpg.edge_count());
        let report = unreachable_code(&cpg, function_id).expect("valid function CFG");

        assert_eq!(report.reachable, vec![function_id, body, returned]);
        assert_eq!(report.unreachable, vec![guard, dead_true, dead_false]);
        assert_eq!(
            report.dead_branches,
            vec![
                DeadBranch {
                    guard,
                    edge_kind: CfgEdgeKind::ConditionalTrue,
                    target: dead_true,
                },
                DeadBranch {
                    guard,
                    edge_kind: CfgEdgeKind::ConditionalFalse,
                    target: dead_false,
                },
            ]
        );
        assert!(report.is_reachable(body));
        assert!(!report.is_reachable(guard));
        assert_eq!((cpg.node_count(), cpg.edge_count()), before);
    }

    #[test]
    fn excludes_call_edges_nested_functions_and_cross_scope_cfg_edges() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let outer = cpg.add_node_with_id(node_with_id(1, function("outer")));
        let outer_body = cpg.add_node_with_id(node_with_id(2, block()));
        let call = cpg.add_node_with_id(node_with_id(
            3,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        ));
        let nested = cpg.add_node_with_id(node_with_id(4, function("nested")));
        let nested_body = cpg.add_node_with_id(node_with_id(5, block()));
        connect_ast(&mut cpg, outer, outer_body);
        connect_ast(&mut cpg, outer_body, call);
        connect_ast(&mut cpg, outer, nested);
        connect_ast(&mut cpg, nested, nested_body);
        connect_cfg(&mut cpg, outer, outer_body, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, outer_body, call, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, call, nested, CfgEdgeKind::Call);
        connect_cfg(&mut cpg, call, nested_body, CfgEdgeKind::Sequential);

        let report = unreachable_code(&cpg, outer).expect("outer function");
        assert_eq!(report.reachable, vec![outer, outer_body, call]);
        assert!(report.unreachable.is_empty());
        assert!(!report.reachable.contains(&nested));
        assert!(!report.reachable.contains(&nested_body));
    }

    #[test]
    fn rejects_unknown_and_non_function_selectors() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let block_id = cpg.add_node_with_id(node_with_id(3, block()));
        assert_eq!(
            unreachable_code(&cpg, NodeId::new(99)),
            Err(AnalysisError::UnknownNode(NodeId::new(99)))
        );
        assert_eq!(
            unreachable_code(&cpg, block_id),
            Err(AnalysisError::NotAFunction(block_id))
        );
    }

    #[test]
    fn cfg_only_graphs_use_the_reachable_projection_as_their_universe() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function_id = cpg.add_node_with_id(node_with_id(10, function("cfg_only")));
        let middle = cpg.add_node_with_id(node_with_id(20, block()));
        let returned = cpg.add_node_with_id(node_with_id(30, CpgNodeKind::Return));
        connect_cfg(&mut cpg, function_id, middle, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, middle, returned, CfgEdgeKind::Sequential);

        let report = unreachable_code(&cpg, function_id).expect("CFG-only function");
        assert_eq!(report.reachable, vec![function_id, middle, returned]);
        assert!(report.unreachable.is_empty());
    }

    fn graph_fixture(
        node_count: usize,
        edges: &[(usize, usize, bool)],
        reverse: bool,
    ) -> CodePropertyGraph {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let mut nodes: Vec<_> = (0..node_count)
            .map(|index| {
                node_with_id(
                    index as u32,
                    if index == 0 { function("f") } else { block() },
                )
            })
            .collect();
        if reverse {
            nodes.reverse();
        }
        for node in nodes {
            cpg.add_node_with_id(node);
        }

        let mut ast_children: Vec<_> = (1..node_count).collect();
        if reverse {
            ast_children.reverse();
        }
        for child in ast_children {
            connect_ast(&mut cpg, NodeId::new(0), NodeId::new(child as u32));
        }

        let mut edges = edges.to_vec();
        if reverse {
            edges.reverse();
        }
        for (source, target, conditional) in edges {
            let source = NodeId::new((source % node_count) as u32);
            let target = NodeId::new((target % node_count) as u32);
            connect_cfg(
                &mut cpg,
                source,
                target,
                if conditional {
                    CfgEdgeKind::ConditionalTrue
                } else {
                    CfgEdgeKind::Sequential
                },
            );
        }
        cpg
    }

    fn petgraph_reachable(node_count: usize, edges: &[(usize, usize, bool)]) -> BTreeSet<NodeId> {
        let universe: BTreeSet<usize> = std::iter::once(0)
            .chain(
                edges
                    .iter()
                    .flat_map(|(source, target, _)| [source % node_count, target % node_count]),
            )
            .collect();
        let mut graph = DiGraph::<NodeId, ()>::new();
        let indices: BTreeMap<_, _> = universe
            .iter()
            .map(|&node| (node, graph.add_node(NodeId::new(node as u32))))
            .collect();
        for &(source, target, _) in edges {
            graph.add_edge(
                indices[&(source % node_count)],
                indices[&(target % node_count)],
                (),
            );
        }
        let mut bfs = Bfs::new(&graph, indices[&0]);
        let mut reached = BTreeSet::new();
        while let Some(index) = bfs.next(&graph) {
            reached.insert(graph[index]);
        }
        reached
    }

    proptest! {
        #[test]
        fn agrees_with_petgraph_and_is_deterministic(
            node_count in 1usize..24,
            raw_edges in prop::collection::vec((0usize..24, 0usize..24, any::<bool>()), 0..80),
        ) {
            let forward = graph_fixture(node_count, &raw_edges, false);
            let reverse = graph_fixture(node_count, &raw_edges, true);
            let actual = unreachable_code(&forward, NodeId::new(0)).expect("valid fixture");
            let permuted = unreachable_code(&reverse, NodeId::new(0)).expect("permuted fixture");
            let expected = petgraph_reachable(node_count, &raw_edges);

            prop_assert_eq!(actual.clone(), permuted);
            prop_assert_eq!(actual.reachable.iter().copied().collect::<BTreeSet<_>>(), expected);

            let universe: BTreeSet<NodeId> = std::iter::once(NodeId::new(0))
                .chain(raw_edges.iter().flat_map(|(source, target, _)| [
                    NodeId::new((source % node_count) as u32),
                    NodeId::new((target % node_count) as u32),
                ]))
                .collect();
            let reachable: BTreeSet<_> = actual.reachable.iter().copied().collect();
            let unreachable: BTreeSet<_> = actual.unreachable.iter().copied().collect();
            prop_assert!(reachable.is_disjoint(&unreachable));
            prop_assert_eq!(reachable.union(&unreachable).copied().collect::<BTreeSet<_>>(), universe);
        }

        #[test]
        fn adding_an_edge_never_shrinks_reachability(
            node_count in 1usize..24,
            raw_edges in prop::collection::vec((0usize..24, 0usize..24, any::<bool>()), 0..80),
            extra in (0usize..24, 0usize..24),
        ) {
            let base = graph_fixture(node_count, &raw_edges, false);
            let before = unreachable_code(&base, NodeId::new(0)).expect("base fixture");
            let mut augmented_edges = raw_edges.clone();
            augmented_edges.push((extra.0, extra.1, false));
            let augmented = graph_fixture(node_count, &augmented_edges, false);
            let after = unreachable_code(&augmented, NodeId::new(0)).expect("augmented fixture");
            prop_assert!(before.reachable.iter().all(|node| after.is_reachable(*node)));
        }

        #[test]
        fn builder_reports_every_cfg_endpoint_after_a_terminator(
            use_break in any::<bool>(),
            tail_len in 2usize..20,
        ) {
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            let function_id = cpg.add_node_with_id(node_with_id(0, function("f")));
            let body = cpg.add_node_with_id(node_with_id(1, block()));
            connect_ast(&mut cpg, function_id, body);
            let terminator = cpg.add_node_with_id(node_with_id(
                2,
                if use_break { CpgNodeKind::Break } else { CpgNodeKind::Return },
            ));
            connect_ast(&mut cpg, body, terminator);

            let mut tail = Vec::new();
            for offset in 0..tail_len {
                let node = cpg.add_node_with_id(node_with_id(
                    (offset + 3) as u32,
                    CpgNodeKind::Variable {
                        name: format!("dead_{offset}").into(),
                        var_type: None,
                        scope: ScopeId::GLOBAL,
                        is_mutable: false,
                    },
                ));
                connect_ast(&mut cpg, body, node);
                tail.push(node);
            }
            CfgExtractor::new().extract_function_cfg(&mut cpg, function_id);

            let report = unreachable_code(&cpg, function_id).expect("extracted CFG");
            prop_assert!(tail.iter().all(|node| report.unreachable.binary_search(node).is_ok()));
        }
    }

    #[test]
    fn iterative_walk_handles_twenty_five_thousand_nodes() {
        const REACHABLE: u32 = 20_000;
        const UNREACHABLE: u32 = 5_000;
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function_id = cpg.add_node_with_id(node_with_id(0, function("large")));
        for id in 1..=REACHABLE + UNREACHABLE {
            let node = cpg.add_node_with_id(node_with_id(id, block()));
            connect_ast(&mut cpg, function_id, node);
        }
        for id in 0..REACHABLE {
            connect_cfg(
                &mut cpg,
                NodeId::new(id),
                NodeId::new(id + 1),
                CfgEdgeKind::Sequential,
            );
        }
        for id in REACHABLE + 1..REACHABLE + UNREACHABLE {
            connect_cfg(
                &mut cpg,
                NodeId::new(id),
                NodeId::new(id + 1),
                CfgEdgeKind::Sequential,
            );
        }

        let report = unreachable_code(&cpg, function_id).expect("large iterative CFG");
        assert_eq!(report.reachable.len(), REACHABLE as usize + 1);
        assert_eq!(report.unreachable.len(), UNREACHABLE as usize);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn report_round_trips_through_serde() {
        let report = ReachabilityReport {
            function: NodeId::new(1),
            reachable: vec![NodeId::new(1), NodeId::new(2)],
            unreachable: vec![NodeId::new(3)],
            dead_branches: vec![DeadBranch {
                guard: NodeId::new(3),
                edge_kind: CfgEdgeKind::ConditionalFalse,
                target: NodeId::new(4),
            }],
        };
        let json = serde_json::to_string(&report).expect("serialize report");
        let decoded: ReachabilityReport = serde_json::from_str(&json).expect("deserialize report");
        assert_eq!(decoded, report);
    }
}
