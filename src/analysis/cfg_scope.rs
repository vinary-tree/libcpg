//! Shared, deterministic per-function control-flow projection.
//!
//! The view preserves the CFG scope and exit rules historically used by PDG
//! construction: start at the function node, follow intraprocedural CFG edges,
//! retain connective nodes such as blocks, and model recorded exits, structural
//! sinks, and explicit `Return`/`Throw` nodes as exit-like.

use rustc_hash::FxHashSet;

use crate::{CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNodeKind, NodeId};

use super::{cfg_projection, AnalysisError, GraphProjection, ProjectionError};

/// Canonical function-scoped CFG consumed by exact intraprocedural analyses.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FunctionCfgView {
    /// Intraprocedural control-flow projection, including connective nodes.
    pub(crate) projection: GraphProjection,
    /// Dense identifier of the function entry node.
    pub(crate) entry: u32,
    /// Sorted dense identifiers of structural or explicit exit-like nodes.
    pub(crate) exit_like: Vec<u32>,
}

/// Builds the canonical, read-only CFG view for one function.
///
/// The node universe is exactly the set reachable from `function` through CFG
/// successors other than [`CfgEdgeKind::Call`]. The edge relation is the
/// induced intraprocedural CFG. Exit-like nodes are the sorted union of CFG
/// exits recorded by the builder, structural sinks, and `Return`/`Throw`
/// nodes. No CPG node or edge is added, removed, or changed.
pub(crate) fn function_cfg_view(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<FunctionCfgView, AnalysisError> {
    let base = cfg_projection(cpg, function).map_err(|error| match error {
        ProjectionError::UnknownNode(node) => AnalysisError::UnknownNode(node),
        ProjectionError::NotAFunction(node) => AnalysisError::NotAFunction(node),
    })?;

    let mut reachable = FxHashSet::default();
    let mut stack = vec![function];
    while let Some(node) = stack.pop() {
        if !reachable.insert(node) {
            continue;
        }
        let mut successors: Vec<_> = cpg
            .cfg_successors(node)
            .into_iter()
            .filter(|(_, kind)| !matches!(kind, CfgEdgeKind::Call))
            .map(|(successor, _)| successor)
            .collect();
        successors.sort_unstable();
        successors.dedup();
        successors.reverse();
        stack.extend(successors);
    }

    let edges = cpg.edges().filter_map(|edge| {
        (edge.kind.is_cfg()
            && !matches!(edge.kind, CpgEdgeKind::ControlFlow(CfgEdgeKind::Call))
            && reachable.contains(&edge.source)
            && reachable.contains(&edge.target))
        .then_some((edge.source, edge.target, 1.0))
    });
    let projection = GraphProjection::from_edges(base.kind, reachable.iter().copied(), edges);
    let entry = projection
        .dense_index(function)
        .expect("the function entry is retained in its CFG projection");

    let recorded_exits: FxHashSet<NodeId> = cpg
        .cfg_exits()
        .iter()
        .copied()
        .filter(|node| projection.dense_index(*node).is_some())
        .collect();
    let mut exit_like = Vec::new();
    for dense in 0..projection.node_count() as u32 {
        let node_id = projection.node_id(dense);
        let explicit_exit = matches!(
            cpg.node(node_id).map(|node| &node.kind),
            Some(CpgNodeKind::Return) | Some(CpgNodeKind::Throw)
        );
        if projection.successors(dense).is_empty()
            || explicit_exit
            || recorded_exits.contains(&node_id)
        {
            exit_like.push(dense);
        }
    }

    Ok(FunctionCfgView {
        projection,
        entry,
        exit_like,
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use proptest::prelude::*;
    #[cfg(feature = "lang-rust")]
    use rustc_hash::FxHashSet;

    use super::*;
    use crate::{
        CpgEdgeKind, CpgNode, Language, MethodSignature, ScopeId, SourceRange, Visibility,
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

    fn connect_cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId, kind: CfgEdgeKind) {
        cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind));
    }

    fn stable_edges(projection: &GraphProjection) -> BTreeSet<(NodeId, NodeId)> {
        projection
            .edges()
            .map(|(source, target, _)| (projection.node_id(source), projection.node_id(target)))
            .collect()
    }

    fn stable_exits(view: &FunctionCfgView) -> Vec<NodeId> {
        view.exit_like
            .iter()
            .map(|&dense| view.projection.node_id(dense))
            .collect()
    }

    #[test]
    fn view_retains_reachable_connective_nodes_and_all_exit_classes() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function_id = cpg.add_node_with_id(node_with_id(10, function("f")));
        let branch = cpg.add_node_with_id(node_with_id(20, CpgNodeKind::If));
        let recorded = cpg.add_node_with_id(node_with_id(30, block()));
        let returned = cpg.add_node_with_id(node_with_id(40, CpgNodeKind::Return));
        let thrown = cpg.add_node_with_id(node_with_id(50, CpgNodeKind::Throw));
        let sink = cpg.add_node_with_id(node_with_id(60, block()));
        let call = cpg.add_node_with_id(node_with_id(
            70,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        ));
        let callee = cpg.add_node_with_id(node_with_id(80, function("callee")));

        connect_cfg(&mut cpg, function_id, branch, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, branch, recorded, CfgEdgeKind::ConditionalTrue);
        connect_cfg(&mut cpg, branch, thrown, CfgEdgeKind::ConditionalFalse);
        connect_cfg(&mut cpg, recorded, returned, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, returned, sink, CfgEdgeKind::Return);
        connect_cfg(&mut cpg, sink, call, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, call, callee, CfgEdgeKind::Call);
        cpg.add_cfg_exit(recorded);

        let before = (cpg.node_count(), cpg.edge_count());
        let view = function_cfg_view(&cpg, function_id).expect("valid function CFG");

        assert_eq!(view.projection.node_id(view.entry), function_id);
        assert_eq!(
            view.projection.nodes,
            vec![function_id, branch, recorded, returned, thrown, sink, call]
        );
        assert_eq!(
            stable_edges(&view.projection),
            BTreeSet::from([
                (function_id, branch),
                (branch, recorded),
                (branch, thrown),
                (recorded, returned),
                (returned, sink),
                (sink, call),
            ])
        );
        assert_eq!(stable_exits(&view), vec![recorded, returned, thrown, call]);
        assert!(!view.projection.nodes.contains(&callee));
        assert_eq!((cpg.node_count(), cpg.edge_count()), before);
    }

    #[test]
    fn invalid_function_selectors_return_shared_analysis_errors() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let block = cpg.add_node_with_id(node_with_id(7, block()));
        assert_eq!(
            function_cfg_view(&cpg, NodeId::new(99)),
            Err(AnalysisError::UnknownNode(NodeId::new(99)))
        );
        assert_eq!(
            function_cfg_view(&cpg, block),
            Err(AnalysisError::NotAFunction(block))
        );
    }

    fn permutation_fixture(
        node_count: usize,
        edges: &[(usize, usize, bool)],
        exits: &[usize],
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

        let mut edges = edges.to_vec();
        if reverse {
            edges.reverse();
        }
        for (source, target, is_call) in edges {
            connect_cfg(
                &mut cpg,
                NodeId::new(source as u32),
                NodeId::new(target as u32),
                if is_call {
                    CfgEdgeKind::Call
                } else {
                    CfgEdgeKind::Sequential
                },
            );
        }

        let mut exits = exits.to_vec();
        if reverse {
            exits.reverse();
        }
        for exit in exits {
            cpg.add_cfg_exit(NodeId::new(exit as u32));
        }
        cpg
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn function_cfg_view_is_deterministic_under_node_edge_and_exit_permutations(
            node_count in 1usize..24,
            raw_edges in prop::collection::vec((0usize..24, 0usize..24, any::<bool>()), 0..96),
            raw_exits in prop::collection::vec(0usize..24, 0..24),
        ) {
            let edges: Vec<_> = raw_edges
                .into_iter()
                .filter(|(source, target, _)| *source < node_count && *target < node_count)
                .collect();
            let exits: Vec<_> = raw_exits
                .into_iter()
                .filter(|exit| *exit < node_count)
                .collect();
            let forward = permutation_fixture(node_count, &edges, &exits, false);
            let reverse = permutation_fixture(node_count, &edges, &exits, true);

            let forward_view = function_cfg_view(&forward, NodeId::new(0)).expect("forward view");
            let reverse_view = function_cfg_view(&reverse, NodeId::new(0)).expect("reverse view");
            prop_assert_eq!(forward_view, reverse_view);
        }
    }

    #[cfg(feature = "lang-rust")]
    fn legacy_pdg_scope(
        cpg: &CodePropertyGraph,
        function: NodeId,
    ) -> (Vec<NodeId>, BTreeSet<(NodeId, NodeId)>, Vec<NodeId>) {
        let mut reachable = FxHashSet::default();
        let mut stack = vec![function];
        while let Some(node) = stack.pop() {
            if !reachable.insert(node) {
                continue;
            }
            for (successor, kind) in cpg.cfg_successors(node) {
                if !matches!(kind, CfgEdgeKind::Call) {
                    stack.push(successor);
                }
            }
        }

        let mut nodes: Vec<_> = reachable.iter().copied().collect();
        nodes.sort_unstable();
        let mut has_successor = FxHashSet::default();
        let mut edges = BTreeSet::new();
        for &node in &reachable {
            for (successor, kind) in cpg.cfg_successors(node) {
                if !matches!(kind, CfgEdgeKind::Call) && reachable.contains(&successor) {
                    edges.insert((node, successor));
                    has_successor.insert(node);
                }
            }
        }
        let recorded: FxHashSet<_> = cpg
            .cfg_exits()
            .iter()
            .copied()
            .filter(|exit| reachable.contains(exit))
            .collect();
        let mut exits: Vec<_> = reachable
            .iter()
            .copied()
            .filter(|node| {
                !has_successor.contains(node)
                    || recorded.contains(node)
                    || matches!(
                        cpg.node(*node).map(|node| &node.kind),
                        Some(CpgNodeKind::Return) | Some(CpgNodeKind::Throw)
                    )
            })
            .collect();
        exits.sort_unstable();
        (nodes, edges, exits)
    }

    #[cfg(feature = "lang-rust")]
    #[test]
    fn parsed_rust_views_match_the_pre_extraction_pdg_scope_rules() {
        use crate::{CpgBuilder, TreeSitterCpgBuilder};

        let corpus = [
            "fn choose(x: bool) -> i32 { if x { return 1; } 2 }",
            "fn count(mut n: i32) { while n > 0 { n -= 1; } }",
            "fn nested(x: bool) { if x { loop { break; } } else { work(); } }",
        ];
        for source in corpus {
            let cpg = TreeSitterCpgBuilder::new()
                .build(source, Language::Rust)
                .expect("parse corpus member");
            for function in cpg.functions().map(|node| node.id) {
                let view = function_cfg_view(&cpg, function).expect("parsed function view");
                let (nodes, edges, exits) = legacy_pdg_scope(&cpg, function);
                assert_eq!(view.projection.nodes, nodes, "source: {source}");
                assert_eq!(stable_edges(&view.projection), edges, "source: {source}");
                assert_eq!(stable_exits(&view), exits, "source: {source}");
            }
        }
    }
}
