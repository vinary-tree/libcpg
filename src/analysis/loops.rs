//! Deterministic dominator-based natural-loop and nesting analysis.
//!
//! Reducible loops follow the Aho–Sethi–Ullman construction: a CFG edge
//! `tail -> header` is a back edge exactly when `header` dominates `tail`, and
//! the loop body is the reverse predecessor closure from every latch up to the
//! shared header. Cyclic SCCs with a non-dominating retreating edge are exposed
//! as irreducible whole-SCC regions rather than being silently discarded.

use std::collections::{BTreeMap, BTreeSet};

use crate::{CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, NodeId};

use super::{
    cfg_scope::function_cfg_view,
    dominator_tree,
    scc::{decompose, SccProjection},
    AnalysisError, DominatorTree, GraphProjection,
};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// One intraprocedural CFG edge that leaves a loop body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LoopExitEdge {
    /// Node inside the loop body.
    pub source: NodeId,
    /// Intraprocedural successor outside the loop body.
    pub target: NodeId,
    /// Original CFG edge classification.
    pub kind: CfgEdgeKind,
}

/// One natural loop or conservative irreducible cyclic region.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NaturalLoop {
    /// Stable index into [`LoopForest::loops`].
    pub id: usize,
    /// Unique natural-loop header, or deterministic representative of an
    /// irreducible region.
    pub header: NodeId,
    /// Sorted nodes receiving CFG edges from outside the body.
    pub entries: Vec<NodeId>,
    /// Sorted back-edge tails, or non-dominating retreating-edge tails for an
    /// irreducible region.
    pub latches: Vec<NodeId>,
    /// Sorted loop-body nodes, including `header`.
    pub body: Vec<NodeId>,
    /// Sorted, deduplicated CFG edges from the body to outside nodes.
    pub exits: Vec<LoopExitEdge>,
    /// Smallest strictly enclosing loop, when one exists.
    pub parent: Option<usize>,
    /// Strictly nested immediate child loops, sorted by loop id.
    pub children: Vec<usize>,
    /// Nesting depth, with roots at depth one.
    pub depth: usize,
    /// Whether this is a whole-SCC over-approximation of an irreducible region.
    pub irreducible: bool,
}

/// Deterministic nesting forest for one function's cyclic CFG regions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LoopForest {
    /// Natural and irreducible loops in stable id order.
    pub loops: Vec<NaturalLoop>,
    /// Innermost containing loop id for every represented loop-body node.
    pub loop_of_node: BTreeMap<NodeId, usize>,
    /// Top-level loop ids, sorted ascending.
    pub roots: Vec<usize>,
}

impl LoopForest {
    /// Returns the innermost represented loop containing `node`.
    pub fn innermost(&self, node: NodeId) -> Option<&NaturalLoop> {
        self.loop_of_node
            .get(&node)
            .and_then(|&id| self.loops.get(id))
    }

    /// Returns the greatest nesting depth, or zero when the forest is empty.
    pub fn max_depth(&self) -> usize {
        self.loops
            .iter()
            .map(|natural_loop| natural_loop.depth)
            .max()
            .unwrap_or(0)
    }

    /// Returns whether `source -> target` is a dominator-qualified back edge.
    ///
    /// Non-dominating retreating edges used to identify irreducible regions
    /// are deliberately not reported as natural back edges.
    pub fn is_back_edge(&self, source: NodeId, target: NodeId) -> bool {
        self.loops.iter().any(|natural_loop| {
            !natural_loop.irreducible
                && natural_loop.header == target
                && natural_loop.latches.binary_search(&source).is_ok()
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LoopDraft {
    header: NodeId,
    entries: BTreeSet<NodeId>,
    latches: BTreeSet<NodeId>,
    body: BTreeSet<NodeId>,
    irreducible: bool,
}

/// Computes natural loops, irreducible cyclic regions, and their nesting forest.
///
/// The analysis is feature-free and read-only. Shared-header back edges are
/// merged into one natural loop. Irreducible regions are reported as
/// whole-SCC over-approximations, so consumers never mistake a partial cycle
/// for a complete loop body.
pub fn loop_forest(cpg: &CodePropertyGraph, function: NodeId) -> Result<LoopForest, AnalysisError> {
    let view = function_cfg_view(cpg, function)?;
    let projection = &view.projection;
    let entry = projection.node_id(view.entry);
    let dominators = dominator_tree(projection, entry)?;
    let cfg_edges = intraprocedural_cfg_edges(cpg, projection);

    let mut latches_by_header: BTreeMap<NodeId, BTreeSet<NodeId>> = BTreeMap::new();
    for (tail, header, _) in &cfg_edges {
        if dominators.dominates(*header, *tail) {
            latches_by_header.entry(*header).or_default().insert(*tail);
        }
    }

    let mut drafts = Vec::new();
    for (header, latches) in latches_by_header {
        let body = natural_body(projection, header, &latches);
        let mut entries = region_entries(&cfg_edges, &body);
        if entries.is_empty() {
            entries.insert(header);
        }
        drafts.push(LoopDraft {
            header,
            entries,
            latches,
            body,
            irreducible: false,
        });
    }

    append_irreducible_regions(projection, function, &dominators, &cfg_edges, &mut drafts);
    Ok(build_forest(drafts, &cfg_edges))
}

fn natural_body(
    projection: &GraphProjection,
    header: NodeId,
    latches: &BTreeSet<NodeId>,
) -> BTreeSet<NodeId> {
    let mut body = BTreeSet::from([header]);
    let mut stack: Vec<NodeId> = latches.iter().copied().collect();
    stack.reverse();
    while let Some(node) = stack.pop() {
        if !body.insert(node) || node == header {
            continue;
        }
        let Some(dense) = projection.dense_index(node) else {
            continue;
        };
        for &predecessor in projection.predecessors(dense).iter().rev() {
            let predecessor = projection.node_id(predecessor);
            if predecessor != header && !body.contains(&predecessor) {
                stack.push(predecessor);
            }
        }
    }
    body
}

fn append_irreducible_regions(
    projection: &GraphProjection,
    function: NodeId,
    dominators: &DominatorTree,
    cfg_edges: &[(NodeId, NodeId, CfgEdgeKind)],
    drafts: &mut Vec<LoopDraft>,
) {
    let adjacency: Vec<Vec<u32>> = (0..projection.node_count() as u32)
        .map(|dense| projection.successors(dense).to_vec())
        .collect();
    let decomposition = decompose(
        SccProjection::ControlFlow { function },
        projection.nodes.clone(),
        adjacency,
    );
    let (entry_time, exit_time) = dfs_intervals(projection, projection.dense_index(function));

    for component in decomposition
        .components
        .iter()
        .filter(|component| component.cyclic)
    {
        let body: BTreeSet<NodeId> = component.nodes.iter().copied().collect();
        let mut retreating = BTreeSet::new();
        for &(tail, head, _) in cfg_edges {
            if !body.contains(&tail) || !body.contains(&head) {
                continue;
            }
            let Some(tail_dense) = projection.dense_index(tail) else {
                continue;
            };
            let Some(head_dense) = projection.dense_index(head) else {
                continue;
            };
            if is_dfs_ancestor(head_dense, tail_dense, &entry_time, &exit_time)
                && !dominators.dominates(head, tail)
            {
                retreating.insert((tail, head));
            }
        }
        if retreating.is_empty() {
            continue;
        }

        let mut entries = region_entries(cfg_edges, &body);
        let header = entries
            .iter()
            .next()
            .copied()
            .or_else(|| body.iter().next().copied())
            .expect("a cyclic SCC contains at least one node");
        if entries.is_empty() {
            entries.insert(header);
        }
        drafts.push(LoopDraft {
            header,
            entries,
            latches: retreating.into_iter().map(|(tail, _)| tail).collect(),
            body,
            irreducible: true,
        });
    }
}

fn dfs_intervals(
    projection: &GraphProjection,
    root: Option<u32>,
) -> (Vec<Option<u64>>, Vec<Option<u64>>) {
    let mut entry = vec![None; projection.node_count()];
    let mut exit = vec![None; projection.node_count()];
    let Some(root) = root else {
        return (entry, exit);
    };
    let mut clock = 0u64;
    entry[root as usize] = Some(clock);
    clock = clock.saturating_add(1);
    let mut stack = vec![(root, 0usize)];
    while let Some((node, next_successor)) = stack.last_mut() {
        if *next_successor < projection.successors(*node).len() {
            let successor = projection.successors(*node)[*next_successor];
            *next_successor += 1;
            if entry[successor as usize].is_none() {
                entry[successor as usize] = Some(clock);
                clock = clock.saturating_add(1);
                stack.push((successor, 0));
            }
        } else {
            exit[*node as usize] = Some(clock);
            clock = clock.saturating_add(1);
            stack.pop();
        }
    }
    (entry, exit)
}

fn is_dfs_ancestor(ancestor: u32, node: u32, entry: &[Option<u64>], exit: &[Option<u64>]) -> bool {
    match (
        entry[ancestor as usize],
        exit[ancestor as usize],
        entry[node as usize],
        exit[node as usize],
    ) {
        (Some(ancestor_entry), Some(ancestor_exit), Some(node_entry), Some(node_exit)) => {
            ancestor_entry <= node_entry && node_exit <= ancestor_exit
        }
        _ => false,
    }
}

fn intraprocedural_cfg_edges(
    cpg: &CodePropertyGraph,
    projection: &GraphProjection,
) -> Vec<(NodeId, NodeId, CfgEdgeKind)> {
    let mut edges = Vec::new();
    for edge in cpg.edges() {
        let CpgEdgeKind::ControlFlow(kind) = &edge.kind else {
            continue;
        };
        if matches!(kind, CfgEdgeKind::Call)
            || projection.dense_index(edge.source).is_none()
            || projection.dense_index(edge.target).is_none()
        {
            continue;
        }
        edges.push((edge.source, edge.target, *kind));
    }
    edges.sort_unstable_by_key(|(source, target, kind)| (*source, *target, cfg_kind_rank(*kind)));
    edges.dedup();
    edges
}

fn region_entries(
    cfg_edges: &[(NodeId, NodeId, CfgEdgeKind)],
    body: &BTreeSet<NodeId>,
) -> BTreeSet<NodeId> {
    cfg_edges
        .iter()
        .filter(|(source, target, _)| !body.contains(source) && body.contains(target))
        .map(|(_, target, _)| *target)
        .collect()
}

fn region_exits(
    cfg_edges: &[(NodeId, NodeId, CfgEdgeKind)],
    body: &BTreeSet<NodeId>,
) -> Vec<LoopExitEdge> {
    cfg_edges
        .iter()
        .filter(|(source, target, _)| body.contains(source) && !body.contains(target))
        .map(|&(source, target, kind)| LoopExitEdge {
            source,
            target,
            kind,
        })
        .collect()
}

fn build_forest(
    mut drafts: Vec<LoopDraft>,
    cfg_edges: &[(NodeId, NodeId, CfgEdgeKind)],
) -> LoopForest {
    drafts.sort_by(|left, right| {
        (
            left.header,
            left.irreducible,
            left.body.len(),
            &left.body,
            &left.latches,
        )
            .cmp(&(
                right.header,
                right.irreducible,
                right.body.len(),
                &right.body,
                &right.latches,
            ))
    });

    let bodies: Vec<_> = drafts.iter().map(|draft| draft.body.clone()).collect();
    let parents: Vec<Option<usize>> = (0..drafts.len())
        .map(|child| {
            (0..drafts.len())
                .filter(|&candidate| {
                    bodies[candidate].len() > bodies[child].len()
                        && bodies[candidate].is_superset(&bodies[child])
                })
                .min_by_key(|&candidate| (bodies[candidate].len(), candidate))
        })
        .collect();

    let mut loops: Vec<NaturalLoop> = drafts
        .into_iter()
        .enumerate()
        .map(|(id, draft)| NaturalLoop {
            id,
            header: draft.header,
            entries: draft.entries.into_iter().collect(),
            latches: draft.latches.into_iter().collect(),
            exits: region_exits(cfg_edges, &draft.body),
            body: draft.body.into_iter().collect(),
            parent: parents[id],
            children: Vec::new(),
            depth: 0,
            irreducible: draft.irreducible,
        })
        .collect();

    for (child, parent) in parents.iter().copied().enumerate() {
        if let Some(parent) = parent {
            loops[parent].children.push(child);
        }
    }
    let roots: Vec<_> = parents
        .iter()
        .enumerate()
        .filter_map(|(id, parent)| parent.is_none().then_some(id))
        .collect();
    let mut stack: Vec<_> = roots.iter().rev().map(|&id| (id, 1usize)).collect();
    while let Some((id, depth)) = stack.pop() {
        loops[id].depth = depth;
        for &child in loops[id].children.iter().rev() {
            stack.push((child, depth.saturating_add(1)));
        }
    }

    let mut loop_order: Vec<_> = (0..loops.len()).collect();
    loop_order.sort_by_key(|&id| (loops[id].body.len(), id));
    let mut loop_of_node = BTreeMap::new();
    for id in loop_order {
        for &node in &loops[id].body {
            loop_of_node.entry(node).or_insert(id);
        }
    }

    LoopForest {
        loops,
        loop_of_node,
        roots,
    }
}

const fn cfg_kind_rank(kind: CfgEdgeKind) -> u8 {
    match kind {
        CfgEdgeKind::Sequential => 0,
        CfgEdgeKind::ConditionalTrue => 1,
        CfgEdgeKind::ConditionalFalse => 2,
        CfgEdgeKind::LoopBack => 3,
        CfgEdgeKind::LoopExit => 4,
        CfgEdgeKind::Break => 5,
        CfgEdgeKind::Continue => 6,
        CfgEdgeKind::Return => 7,
        CfgEdgeKind::Throw => 8,
        CfgEdgeKind::Catch => 9,
        CfgEdgeKind::Call => 10,
        CfgEdgeKind::CallReturn => 11,
        CfgEdgeKind::Case => 12,
        CfgEdgeKind::DefaultCase => 13,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use proptest::prelude::*;

    use super::*;
    use crate::{
        control_flow_sccs, CpgNode, CpgNodeKind, Language, MethodSignature, ScopeId, SourceRange,
        Visibility,
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

    fn add_cfg(cpg: &mut CodePropertyGraph, source: u32, target: u32, kind: CfgEdgeKind) {
        cpg.connect(
            NodeId::new(source),
            NodeId::new(target),
            CpgEdgeKind::ControlFlow(kind),
        );
    }

    fn graph(node_count: u32, edges: &[(u32, u32, CfgEdgeKind)]) -> CodePropertyGraph {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        cpg.add_node_with_id(node_with_id(0, function("f")));
        for id in 1..node_count {
            cpg.add_node_with_id(node_with_id(id, block()));
            cpg.connect(NodeId::new(0), NodeId::new(id), CpgEdgeKind::AstChild);
            cpg.node_mut(NodeId::new(id)).expect("node").parent = Some(NodeId::new(0));
        }
        for &(source, target, kind) in edges {
            add_cfg(&mut cpg, source, target, kind);
        }
        cpg
    }

    #[test]
    fn nested_natural_loops_form_a_strict_forest_with_stable_exit_edges() {
        let cpg = graph(
            6,
            &[
                (0, 1, CfgEdgeKind::Sequential),
                (1, 2, CfgEdgeKind::ConditionalTrue),
                (1, 5, CfgEdgeKind::LoopExit),
                (2, 3, CfgEdgeKind::ConditionalTrue),
                (2, 4, CfgEdgeKind::LoopExit),
                (3, 2, CfgEdgeKind::LoopBack),
                (4, 1, CfgEdgeKind::LoopBack),
            ],
        );
        let forest = loop_forest(&cpg, NodeId::new(0)).expect("loop forest");

        assert_eq!(forest.loops.len(), 2);
        let outer = forest
            .loops
            .iter()
            .find(|natural_loop| natural_loop.header == NodeId::new(1))
            .expect("outer loop");
        let inner = forest
            .loops
            .iter()
            .find(|natural_loop| natural_loop.header == NodeId::new(2))
            .expect("inner loop");
        assert_eq!(
            outer.body,
            vec![
                NodeId::new(1),
                NodeId::new(2),
                NodeId::new(3),
                NodeId::new(4)
            ]
        );
        assert_eq!(outer.latches, vec![NodeId::new(4)]);
        assert_eq!(outer.children, vec![inner.id]);
        assert_eq!(outer.depth, 1);
        assert_eq!(inner.body, vec![NodeId::new(2), NodeId::new(3)]);
        assert_eq!(inner.parent, Some(outer.id));
        assert_eq!(inner.depth, 2);
        assert_eq!(forest.roots, vec![outer.id]);
        assert_eq!(forest.max_depth(), 2);
        assert_eq!(forest.innermost(NodeId::new(2)), Some(inner));
        assert!(forest.is_back_edge(NodeId::new(3), NodeId::new(2)));
        assert!(forest.is_back_edge(NodeId::new(4), NodeId::new(1)));
        assert!(!forest.is_back_edge(NodeId::new(2), NodeId::new(3)));
        assert_eq!(
            outer.exits,
            vec![LoopExitEdge {
                source: NodeId::new(1),
                target: NodeId::new(5),
                kind: CfgEdgeKind::LoopExit,
            }]
        );
        assert_eq!(
            inner.exits,
            vec![LoopExitEdge {
                source: NodeId::new(2),
                target: NodeId::new(4),
                kind: CfgEdgeKind::LoopExit,
            }]
        );
    }

    #[test]
    fn shared_header_latches_merge_into_one_loop() {
        let cpg = graph(
            5,
            &[
                (0, 1, CfgEdgeKind::Sequential),
                (1, 2, CfgEdgeKind::ConditionalTrue),
                (1, 3, CfgEdgeKind::ConditionalFalse),
                (2, 1, CfgEdgeKind::LoopBack),
                (3, 1, CfgEdgeKind::Continue),
                (1, 4, CfgEdgeKind::LoopExit),
            ],
        );
        let forest = loop_forest(&cpg, NodeId::new(0)).expect("loop forest");
        assert_eq!(forest.loops.len(), 1);
        assert_eq!(forest.loops[0].header, NodeId::new(1));
        assert_eq!(
            forest.loops[0].latches,
            vec![NodeId::new(2), NodeId::new(3)]
        );
        assert_eq!(
            forest.loops[0].body,
            vec![NodeId::new(1), NodeId::new(2), NodeId::new(3)]
        );
    }

    #[test]
    fn non_dominating_retreating_edge_becomes_one_irreducible_scc_region() {
        let cpg = graph(
            5,
            &[
                (0, 1, CfgEdgeKind::ConditionalTrue),
                (0, 2, CfgEdgeKind::ConditionalFalse),
                (1, 3, CfgEdgeKind::Sequential),
                (2, 3, CfgEdgeKind::Sequential),
                (3, 1, CfgEdgeKind::LoopBack),
                (3, 2, CfgEdgeKind::LoopBack),
                (3, 4, CfgEdgeKind::LoopExit),
            ],
        );
        let forest = loop_forest(&cpg, NodeId::new(0)).expect("loop forest");
        let irreducible: Vec<_> = forest
            .loops
            .iter()
            .filter(|natural_loop| natural_loop.irreducible)
            .collect();
        assert_eq!(irreducible.len(), 1);
        assert_eq!(irreducible[0].header, NodeId::new(1));
        assert_eq!(irreducible[0].entries, vec![NodeId::new(1), NodeId::new(2)]);
        assert_eq!(
            irreducible[0].body,
            vec![NodeId::new(1), NodeId::new(2), NodeId::new(3)]
        );
        assert_eq!(irreducible[0].latches, vec![NodeId::new(2), NodeId::new(3)]);
        assert!(!forest.is_back_edge(NodeId::new(3), NodeId::new(1)));
    }

    #[test]
    fn acyclic_and_invalid_functions_are_precise() {
        let cpg = graph(
            3,
            &[
                (0, 1, CfgEdgeKind::Sequential),
                (1, 2, CfgEdgeKind::Sequential),
            ],
        );
        assert_eq!(loop_forest(&cpg, NodeId::new(0)), Ok(LoopForest::default()));
        assert_eq!(
            loop_forest(&cpg, NodeId::new(99)),
            Err(AnalysisError::UnknownNode(NodeId::new(99)))
        );
        assert_eq!(
            loop_forest(&cpg, NodeId::new(1)),
            Err(AnalysisError::NotAFunction(NodeId::new(1)))
        );
    }

    #[test]
    fn nested_functions_and_call_edges_do_not_leak_into_the_selected_forest() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        for (id, kind) in [
            (0, function("outer")),
            (1, block()),
            (2, block()),
            (3, function("inner")),
            (4, block()),
            (5, block()),
        ] {
            cpg.add_node_with_id(node_with_id(id, kind));
        }
        for (parent, child) in [(0, 1), (0, 2), (0, 3), (3, 4), (3, 5)] {
            cpg.connect(
                NodeId::new(parent),
                NodeId::new(child),
                CpgEdgeKind::AstChild,
            );
            cpg.node_mut(NodeId::new(child)).expect("child").parent = Some(NodeId::new(parent));
        }

        for (source, target, kind) in [
            (0, 1, CfgEdgeKind::Sequential),
            (1, 2, CfgEdgeKind::ConditionalTrue),
            (2, 1, CfgEdgeKind::LoopBack),
            (2, 3, CfgEdgeKind::Call),
            (3, 4, CfgEdgeKind::Sequential),
            (4, 5, CfgEdgeKind::ConditionalTrue),
            (5, 4, CfgEdgeKind::LoopBack),
        ] {
            add_cfg(&mut cpg, source, target, kind);
        }

        let outer = loop_forest(&cpg, NodeId::new(0)).expect("outer forest");
        assert_eq!(outer.loops.len(), 1);
        assert_eq!(outer.loops[0].body, vec![NodeId::new(1), NodeId::new(2)]);

        let inner = loop_forest(&cpg, NodeId::new(3)).expect("inner forest");
        assert_eq!(inner.loops.len(), 1);
        assert_eq!(inner.loops[0].body, vec![NodeId::new(4), NodeId::new(5)]);
    }

    #[test]
    fn twenty_five_thousand_node_loop_is_stack_safe() {
        const LAST: u32 = 25_000;
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        cpg.add_node_with_id(node_with_id(0, function("deep")));
        for id in 1..=LAST {
            cpg.add_node_with_id(node_with_id(id, block()));
            cpg.connect(NodeId::new(0), NodeId::new(id), CpgEdgeKind::AstChild);
            cpg.node_mut(NodeId::new(id)).expect("node").parent = Some(NodeId::new(0));
            add_cfg(
                &mut cpg,
                id - 1,
                id,
                if id == 1 {
                    CfgEdgeKind::ConditionalTrue
                } else {
                    CfgEdgeKind::Sequential
                },
            );
        }
        add_cfg(&mut cpg, LAST, 1, CfgEdgeKind::LoopBack);

        let forest = loop_forest(&cpg, NodeId::new(0)).expect("deep loop forest");
        assert_eq!(forest.loops.len(), 1);
        assert_eq!(forest.loops[0].header, NodeId::new(1));
        assert_eq!(forest.loops[0].body.len(), LAST as usize);
        assert_eq!(forest.loops[0].latches, vec![NodeId::new(LAST)]);
        assert_eq!(forest.max_depth(), 1);
    }

    fn fixture_from_parts(
        node_count: usize,
        edges: &[(usize, usize, u8)],
        reverse: bool,
    ) -> CodePropertyGraph {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let mut nodes: Vec<_> = (0..node_count)
            .map(|id| node_with_id(id as u32, if id == 0 { function("f") } else { block() }))
            .collect();
        if reverse {
            nodes.reverse();
        }
        for node in nodes {
            cpg.add_node_with_id(node);
        }

        let mut ast_nodes: Vec<_> = (1..node_count).collect();
        if reverse {
            ast_nodes.reverse();
        }
        for id in ast_nodes {
            cpg.connect(
                NodeId::new(0),
                NodeId::new(id as u32),
                CpgEdgeKind::AstChild,
            );
            cpg.node_mut(NodeId::new(id as u32)).expect("node").parent = Some(NodeId::new(0));
        }

        let mut edges = edges.to_vec();
        if reverse {
            edges.reverse();
        }
        for (source, target, tag) in edges {
            let kind = match tag % 5 {
                0 => CfgEdgeKind::Sequential,
                1 => CfgEdgeKind::ConditionalTrue,
                2 => CfgEdgeKind::LoopBack,
                3 => CfgEdgeKind::Continue,
                _ => CfgEdgeKind::Call,
            };
            add_cfg(&mut cpg, source as u32, target as u32, kind);
        }
        cpg
    }

    fn brute_dominates(
        projection: &GraphProjection,
        entry: NodeId,
        dominator: NodeId,
        node: NodeId,
    ) -> bool {
        if dominator == node {
            return true;
        }
        if dominator == entry {
            return true;
        }
        let mut seen = BTreeSet::new();
        let mut stack = vec![entry];
        while let Some(current) = stack.pop() {
            if current == dominator || !seen.insert(current) {
                continue;
            }
            if current == node {
                return false;
            }
            if let Some(dense) = projection.dense_index(current) {
                stack.extend(
                    projection
                        .successors(dense)
                        .iter()
                        .map(|&successor| projection.node_id(successor)),
                );
            }
        }
        true
    }

    fn brute_natural_loops(
        projection: &GraphProjection,
        entry: NodeId,
    ) -> BTreeMap<NodeId, (BTreeSet<NodeId>, BTreeSet<NodeId>)> {
        let mut latches: BTreeMap<NodeId, BTreeSet<NodeId>> = BTreeMap::new();
        for (tail, head, _) in projection.edges() {
            let tail = projection.node_id(tail);
            let head = projection.node_id(head);
            if brute_dominates(projection, entry, head, tail) {
                latches.entry(head).or_default().insert(tail);
            }
        }
        latches
            .into_iter()
            .map(|(header, latches)| {
                // Independent fixed-point oracle: repeatedly add every
                // predecessor of an already admitted non-header node.
                let mut body = BTreeSet::from([header]);
                body.extend(latches.iter().copied());
                loop {
                    let previous_len = body.len();
                    let frontier: Vec<_> = body
                        .iter()
                        .copied()
                        .filter(|node| *node != header)
                        .collect();
                    for node in frontier {
                        let dense = projection.dense_index(node).expect("projected loop node");
                        body.extend(
                            projection
                                .predecessors(dense)
                                .iter()
                                .map(|&predecessor| projection.node_id(predecessor)),
                        );
                    }
                    if body.len() == previous_len {
                        break;
                    }
                }
                (header, (latches, body))
            })
            .collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn forests_are_deterministic_and_match_brute_force_back_edges_and_bodies(
            node_count in 1usize..24,
            raw_edges in prop::collection::vec((0usize..24, 0usize..24, 0u8..10), 0..96),
        ) {
            let edges: Vec<_> = raw_edges
                .into_iter()
                .filter(|(source, target, _)| *source < node_count && *target < node_count)
                .collect();
            let forward = fixture_from_parts(node_count, &edges, false);
            let reverse = fixture_from_parts(node_count, &edges, true);
            let forward_forest = loop_forest(&forward, NodeId::new(0)).expect("forward forest");
            let reverse_forest = loop_forest(&reverse, NodeId::new(0)).expect("reverse forest");
            prop_assert_eq!(&forward_forest, &reverse_forest);

            let view = function_cfg_view(&forward, NodeId::new(0)).expect("CFG view");
            let expected = brute_natural_loops(&view.projection, NodeId::new(0));
            let actual: BTreeMap<_, _> = forward_forest
                .loops
                .iter()
                .filter(|natural_loop| !natural_loop.irreducible)
                .map(|natural_loop| {
                    (
                        natural_loop.header,
                        (
                            natural_loop.latches.iter().copied().collect(),
                            natural_loop.body.iter().copied().collect(),
                        ),
                    )
                })
                .collect();
            prop_assert_eq!(actual, expected);

            let sccs = control_flow_sccs(&forward, NodeId::new(0)).expect("CFG SCCs");
            for natural_loop in &forward_forest.loops {
                let contained_in_cyclic_scc = sccs.components.iter().any(|component| {
                    component.cyclic
                        && natural_loop.body.iter().all(|node| component.nodes.contains(node))
                });
                prop_assert!(contained_in_cyclic_scc);
                if let Some(parent) = natural_loop.parent {
                    let child_body: BTreeSet<_> = natural_loop.body.iter().copied().collect();
                    let parent_body: BTreeSet<_> = forward_forest.loops[parent]
                        .body
                        .iter()
                        .copied()
                        .collect();
                    prop_assert!(child_body.len() < parent_body.len());
                    prop_assert!(parent_body.is_superset(&child_body));
                }
            }
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn loop_forest_round_trips_through_json() {
        let cpg = graph(
            3,
            &[
                (0, 1, CfgEdgeKind::Sequential),
                (1, 2, CfgEdgeKind::ConditionalTrue),
                (2, 1, CfgEdgeKind::LoopBack),
            ],
        );
        let forest = loop_forest(&cpg, NodeId::new(0)).expect("loop forest");
        let encoded = serde_json::to_string(&forest).expect("serialize forest");
        let decoded: LoopForest = serde_json::from_str(&encoded).expect("deserialize forest");
        assert_eq!(decoded, forest);
    }
}
