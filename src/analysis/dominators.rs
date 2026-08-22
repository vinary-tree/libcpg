//! Deterministic dominator and post-dominator analysis.
//!
//! The implementation uses the Cooper–Harvey–Kennedy iterative algorithm over
//! sorted dense adjacency. Post-dominators reuse the same engine on a reversed
//! function CFG with a synthetic exit root.

use std::collections::BTreeMap;

use crate::{CodePropertyGraph, NodeId};

use super::{cfg_scope::function_cfg_view, scc::heap_sorted, AnalysisError, GraphProjection};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// A rooted dominator tree for the nodes reachable from one projection node.
///
/// A node `a` dominates `b` when every directed path from [`Self::root`] to
/// `b` contains `a`. The root has no entry in `idom`; every other reachable
/// node maps to its unique immediate dominator.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DominatorTree {
    /// Stable root node identifier.
    pub root: NodeId,
    /// Immediate dominator of every reachable non-root node.
    pub idom: BTreeMap<NodeId, NodeId>,
    /// Dominator-tree children for every reachable node, sorted by `NodeId`.
    pub children: BTreeMap<NodeId, Vec<NodeId>>,
    /// Dominance frontier for every reachable node, sorted by `NodeId`.
    pub frontier: BTreeMap<NodeId, Vec<NodeId>>,
    entry_time: BTreeMap<NodeId, u64>,
    exit_time: BTreeMap<NodeId, u64>,
}

impl DominatorTree {
    /// Returns the unique immediate dominator of `node`.
    ///
    /// The root and nodes unreachable from the root return `None`.
    pub fn immediate_dominator(&self, node: NodeId) -> Option<NodeId> {
        self.idom.get(&node).copied()
    }

    /// Returns whether `a` dominates `b` in constant time.
    pub fn dominates(&self, a: NodeId, b: NodeId) -> bool {
        interval_contains(&self.entry_time, &self.exit_time, a, b)
    }

    /// Iterates `node`, its immediate dominator, and so on through the root.
    ///
    /// Nodes unreachable from the root produce an empty iterator.
    /// Externally deserialized parent cycles are bounded by the represented
    /// tree size rather than producing an infinite iterator.
    pub fn dominators_of(&self, node: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        let initial = (node == self.root || self.idom.contains_key(&node)).then_some(node);
        std::iter::successors(initial, move |current| {
            (*current != self.root)
                .then(|| self.idom.get(current).copied())
                .flatten()
        })
        // A valid tree reaches `root` in at most this many steps. The cap also
        // makes externally deserialized, cyclic parent maps safe to inspect.
        .take(self.idom.len().saturating_add(1))
    }
}

/// A post-dominator tree for one intraprocedural control-flow graph.
///
/// A node `a` post-dominates `b` when every path from `b` to a modeled exit
/// contains `a`. `ipdom` excludes nodes whose immediate post-dominator is the
/// synthetic exit; those nodes are listed in `virtual_children` instead.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PostDominatorTree {
    /// Explicit and structural CFG exits, sorted by `NodeId`.
    pub exits: Vec<NodeId>,
    /// Immediate real post-dominator of each node that has one.
    pub ipdom: BTreeMap<NodeId, NodeId>,
    /// Real children of the synthetic exit root, sorted by `NodeId`.
    pub virtual_children: Vec<NodeId>,
    /// Reverse dominance frontier, keyed by dependent node.
    ///
    /// Each value is the sorted set of controllers for the key.
    pub frontier: BTreeMap<NodeId, Vec<NodeId>>,
    entry_time: BTreeMap<NodeId, u64>,
    exit_time: BTreeMap<NodeId, u64>,
}

impl PostDominatorTree {
    /// Returns whether `a` post-dominates `b` in constant time.
    pub fn post_dominates(&self, a: NodeId, b: NodeId) -> bool {
        interval_contains(&self.entry_time, &self.exit_time, a, b)
    }

    /// Returns sorted `(controller, dependent)` control-dependence edges.
    pub fn control_dependence_edges(&self) -> Vec<(NodeId, NodeId)> {
        let mut edges = heap_sorted(self.frontier.iter().flat_map(|(&dependent, controllers)| {
            controllers
                .iter()
                .map(move |&controller| (controller, dependent))
        }));
        edges.dedup();
        edges
    }

    /// Returns true when post-dominance is undefined for the selected CFG.
    ///
    /// This occurs for a CFG with fewer than two nodes or with no modeled exit.
    pub fn is_empty(&self) -> bool {
        self.entry_time.is_empty()
    }
}

/// Computes the dominator tree rooted at `root`.
///
/// Only nodes reachable from `root` participate. Iteration is capped at
/// `g.node_count() + 2` passes; exceeding that bound returns
/// [`AnalysisError::IterationLimitExceeded`].
pub fn dominator_tree(
    graph: &GraphProjection,
    root: NodeId,
) -> Result<DominatorTree, AnalysisError> {
    let root_dense = graph
        .dense_index(root)
        .ok_or(AnalysisError::UnknownNode(root))?;
    let adjacency = DenseAdjacency::projection(graph);
    let computed = compute_dominators(
        &adjacency,
        root_dense,
        graph.node_count() as u64 + 2,
        "dominators",
    )?;

    let mut idom = BTreeMap::new();
    let mut children = BTreeMap::new();
    let mut frontier = BTreeMap::new();
    let mut entry_time = BTreeMap::new();
    let mut exit_time = BTreeMap::new();
    // Dense identifiers follow ascending NodeId order. Converting in that
    // order gives BTreeMap its append path while retaining only reachable
    // nodes, rather than repeatedly searching for RPO-ordered insertions.
    for dense in 0..graph.node_count() as u32 {
        let Some((entry, exit)) = computed.intervals[dense as usize] else {
            continue;
        };
        let node = graph.node_id(dense);
        children.insert(
            node,
            computed.children[dense as usize]
                .iter()
                .map(|&child| graph.node_id(child))
                .collect(),
        );
        frontier.insert(
            node,
            computed.frontier[dense as usize]
                .iter()
                .map(|&member| graph.node_id(member))
                .collect(),
        );
        if dense != root_dense {
            let immediate = computed.idom[dense as usize]
                .expect("every reachable non-root node has an immediate dominator");
            idom.insert(node, graph.node_id(immediate));
        }
        entry_time.insert(node, entry);
        exit_time.insert(node, exit);
    }

    Ok(DominatorTree {
        root,
        idom,
        children,
        frontier,
        entry_time,
        exit_time,
    })
}

/// Computes post-dominators and control dependence for one function CFG.
///
/// The modeled exit relation is identical to [`crate::PdgBuilder`]: recorded
/// CFG exits, structural sinks, and `Return`/`Throw` nodes connect to a
/// synthetic exit. Nodes in an infinite region that cannot reach any exit are
/// also connected directly to the synthetic root so the tree is total. A CFG
/// with no exit returns an empty tree.
pub fn post_dominator_tree(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<PostDominatorTree, AnalysisError> {
    let view = function_cfg_view(cpg, function)?;
    if view.projection.node_count() < 2 || view.exit_like.is_empty() {
        return Ok(empty_post_dominator_tree(
            view.exit_like
                .iter()
                .map(|&dense| view.projection.node_id(dense))
                .collect(),
        ));
    }

    let node_count = view.projection.node_count();
    let virtual_exit = node_count as u32;
    let adjacency = DenseAdjacency::post_dominators(&view.projection, &view.exit_like);
    let computed = compute_dominators(
        &adjacency,
        virtual_exit,
        node_count as u64 + 3,
        "post-dominators",
    )?;

    let mut ipdom = BTreeMap::new();
    let mut virtual_children = Vec::new();
    let mut frontier = BTreeMap::new();
    let mut entry_time = BTreeMap::new();
    let mut exit_time = BTreeMap::new();
    for dense in 0..node_count as u32 {
        let node = view.projection.node_id(dense);
        let immediate = computed.idom[dense as usize]
            .expect("every real post-dominator node is reachable from the synthetic exit");
        if immediate == virtual_exit {
            virtual_children.push(node);
        } else {
            ipdom.insert(node, view.projection.node_id(immediate));
        }
        frontier.insert(
            node,
            computed.frontier[dense as usize]
                .iter()
                .copied()
                .filter(|&member| member != virtual_exit)
                .map(|member| view.projection.node_id(member))
                .collect(),
        );
        if let Some((entry, exit)) = computed.intervals[dense as usize] {
            entry_time.insert(node, entry);
            exit_time.insert(node, exit);
        }
    }
    Ok(PostDominatorTree {
        exits: view
            .exit_like
            .iter()
            .map(|&dense| view.projection.node_id(dense))
            .collect(),
        ipdom,
        virtual_children,
        frontier,
        entry_time,
        exit_time,
    })
}

fn empty_post_dominator_tree(exits: Vec<NodeId>) -> PostDominatorTree {
    PostDominatorTree {
        exits,
        ipdom: BTreeMap::new(),
        virtual_children: Vec::new(),
        frontier: BTreeMap::new(),
        entry_time: BTreeMap::new(),
        exit_time: BTreeMap::new(),
    }
}

#[derive(Debug)]
struct ComputedDominators {
    idom: Vec<Option<u32>>,
    children: Vec<Vec<u32>>,
    frontier: Vec<Vec<u32>>,
    intervals: Vec<Option<(u64, u64)>>,
}

/// Borrowed CSR adjacency with an optional synthetic root overlay.
///
/// Ordinary dominators borrow both CSR directions without copying a single
/// edge. Post-dominators swap the directions and represent `virtual EXIT`
/// with one sorted successor row plus a bit-packed marker for its matching
/// predecessor edges. The overlay is exact but never materializes a second
/// graph or a `Vec<Vec<_>>` adjacency.
#[derive(Debug)]
struct DenseAdjacency<'a> {
    successor_offsets: &'a [u32],
    successor_targets: &'a [u32],
    predecessor_offsets: &'a [u32],
    predecessor_targets: &'a [u32],
    real_node_count: usize,
    synthetic_root: Option<u32>,
    synthetic_successors: Vec<u32>,
    has_synthetic_predecessor: Vec<bool>,
}

impl<'a> DenseAdjacency<'a> {
    fn projection(graph: &'a GraphProjection) -> Self {
        Self {
            successor_offsets: &graph.offsets,
            successor_targets: &graph.targets,
            predecessor_offsets: &graph.rev_offsets,
            predecessor_targets: &graph.rev_targets,
            real_node_count: graph.node_count(),
            synthetic_root: None,
            synthetic_successors: Vec::new(),
            has_synthetic_predecessor: Vec::new(),
        }
    }

    fn post_dominators(graph: &'a GraphProjection, exit_like: &[u32]) -> Self {
        let mut can_reach_exit = vec![false; graph.node_count()];
        let mut stack: Vec<_> = exit_like.iter().rev().copied().collect();
        while let Some(node) = stack.pop() {
            if can_reach_exit[node as usize] {
                continue;
            }
            can_reach_exit[node as usize] = true;
            // Push in reverse so the explicit heap traversal observes the
            // projection's canonical ascending order.
            stack.extend(graph.predecessors(node).iter().rev().copied());
        }

        let mut synthetic_successors = exit_like.to_vec();
        synthetic_successors.extend(
            can_reach_exit
                .iter()
                .enumerate()
                .filter_map(|(node, &reachable)| (!reachable).then_some(node as u32)),
        );
        synthetic_successors = heap_sorted(synthetic_successors);
        synthetic_successors.dedup();
        let mut has_synthetic_predecessor = vec![false; graph.node_count()];
        for &node in &synthetic_successors {
            has_synthetic_predecessor[node as usize] = true;
        }

        Self {
            // Dominators of the reverse CFG are post-dominators of the CFG.
            successor_offsets: &graph.rev_offsets,
            successor_targets: &graph.rev_targets,
            predecessor_offsets: &graph.offsets,
            predecessor_targets: &graph.targets,
            real_node_count: graph.node_count(),
            synthetic_root: Some(graph.node_count() as u32),
            synthetic_successors,
            has_synthetic_predecessor,
        }
    }

    fn node_count(&self) -> usize {
        self.real_node_count + usize::from(self.synthetic_root.is_some())
    }

    fn successors(&self, node: u32) -> &[u32] {
        if self.synthetic_root == Some(node) {
            return &self.synthetic_successors;
        }
        csr_row(self.successor_offsets, self.successor_targets, node)
    }

    fn predecessors(
        &self,
        node: u32,
    ) -> std::iter::Chain<std::iter::Copied<std::slice::Iter<'_, u32>>, std::option::IntoIter<u32>>
    {
        if self.synthetic_root == Some(node) {
            return [].iter().copied().chain(None);
        }
        let synthetic = self.synthetic_root.filter(|_| {
            self.has_synthetic_predecessor
                .get(node as usize)
                .copied()
                .unwrap_or(false)
        });
        csr_row(self.predecessor_offsets, self.predecessor_targets, node)
            .iter()
            .copied()
            .chain(synthetic)
    }
}

fn csr_row<'a>(offsets: &[u32], targets: &'a [u32], node: u32) -> &'a [u32] {
    let start = offsets[node as usize] as usize;
    let end = offsets[node as usize + 1] as usize;
    &targets[start..end]
}

fn compute_dominators(
    adjacency: &DenseAdjacency<'_>,
    root: u32,
    pass_limit: u64,
    analysis: &'static str,
) -> Result<ComputedDominators, AnalysisError> {
    let reverse_postorder = reverse_postorder(adjacency, root);
    let mut rpo_position = vec![u32::MAX; adjacency.node_count()];
    for (position, &node) in reverse_postorder.iter().enumerate() {
        rpo_position[node as usize] = position as u32;
    }

    let mut idom = vec![None; adjacency.node_count()];
    idom[root as usize] = Some(root);
    let mut converged = false;
    for _ in 0..pass_limit {
        let mut changed = false;
        for &node in reverse_postorder.iter().skip(1) {
            let mut processed_predecessors = adjacency
                .predecessors(node)
                .filter(|&predecessor| idom[predecessor as usize].is_some());
            let Some(mut candidate) = processed_predecessors.next() else {
                continue;
            };
            for predecessor in processed_predecessors {
                candidate = intersect(&idom, &rpo_position, predecessor, candidate);
            }
            if idom[node as usize] != Some(candidate) {
                idom[node as usize] = Some(candidate);
                changed = true;
            }
        }
        if !changed {
            converged = true;
            break;
        }
    }
    if !converged {
        return Err(AnalysisError::IterationLimitExceeded {
            analysis,
            limit: pass_limit,
        });
    }

    let mut children = vec![Vec::new(); adjacency.node_count()];
    for &node in reverse_postorder.iter().skip(1) {
        let parent =
            idom[node as usize].expect("every reachable non-root node has an immediate dominator");
        children[parent as usize].push(node);
    }
    normalize_rows(&mut children);
    let frontier = dominance_frontier(adjacency, &idom, &reverse_postorder, root);
    let intervals = euler_intervals(&children, root);

    Ok(ComputedDominators {
        idom,
        children,
        frontier,
        intervals,
    })
}

fn intersect(idom: &[Option<u32>], rpo_position: &[u32], mut left: u32, mut right: u32) -> u32 {
    while left != right {
        while rpo_position[left as usize] > rpo_position[right as usize] {
            left = idom[left as usize]
                .expect("intersect visits only nodes with known immediate dominators");
        }
        while rpo_position[right as usize] > rpo_position[left as usize] {
            right = idom[right as usize]
                .expect("intersect visits only nodes with known immediate dominators");
        }
    }
    left
}

fn reverse_postorder(adjacency: &DenseAdjacency<'_>, root: u32) -> Vec<u32> {
    let mut visited = vec![false; adjacency.node_count()];
    let mut postorder = Vec::new();
    let mut stack = vec![(root, 0usize)];
    visited[root as usize] = true;
    while let Some((node, next_successor)) = stack.last_mut() {
        let successors = adjacency.successors(*node);
        if *next_successor < successors.len() {
            let successor = successors[*next_successor];
            *next_successor += 1;
            if !visited[successor as usize] {
                visited[successor as usize] = true;
                stack.push((successor, 0));
            }
        } else {
            postorder.push(*node);
            stack.pop();
        }
    }
    postorder.reverse();
    postorder
}

fn dominance_frontier(
    adjacency: &DenseAdjacency<'_>,
    idom: &[Option<u32>],
    reachable: &[u32],
    root: u32,
) -> Vec<Vec<u32>> {
    let mut frontier = vec![Vec::new(); adjacency.node_count()];
    for &join in reachable {
        let reachable_incoming = adjacency
            .predecessors(join)
            .filter(|&predecessor| idom[predecessor as usize].is_some())
            .take(2)
            .count();
        // The distinguished entry has an implicit external predecessor. One
        // explicit self-loop or backedge is therefore enough to place entry
        // in a dominance frontier; unlike every other reachable node, its
        // immediate dominator is itself rather than a strict dominator.
        let minimum_incoming = if join == root { 1 } else { 2 };
        if reachable_incoming < minimum_incoming {
            continue;
        }
        let join_idom = idom[join as usize]
            .expect("every reachable dominance-frontier join has an immediate dominator");
        for predecessor in adjacency
            .predecessors(join)
            .filter(|&predecessor| idom[predecessor as usize].is_some())
        {
            let mut runner = predecessor;
            while runner != join_idom {
                let members = &mut frontier[runner as usize];
                if members.last() != Some(&join) {
                    members.push(join);
                }
                let Some(parent) = idom[runner as usize] else {
                    break;
                };
                if parent == runner {
                    break;
                }
                runner = parent;
            }
        }
        if join == root {
            frontier[root as usize].push(root);
        }
    }
    normalize_rows(&mut frontier);
    frontier
}

fn euler_intervals(children: &[Vec<u32>], root: u32) -> Vec<Option<(u64, u64)>> {
    let mut intervals: Vec<Option<(u64, u64)>> = vec![None; children.len()];
    let mut clock = 0u64;
    let mut stack = vec![(root, false)];
    while let Some((node, exiting)) = stack.pop() {
        if exiting {
            let entry = intervals[node as usize]
                .expect("Euler exit follows its entry")
                .0;
            intervals[node as usize] = Some((entry, clock));
            clock += 1;
            continue;
        }
        intervals[node as usize] = Some((clock, clock));
        clock += 1;
        stack.push((node, true));
        for &child in children[node as usize].iter().rev() {
            stack.push((child, false));
        }
    }
    intervals
}

fn interval_contains(
    entry_time: &BTreeMap<NodeId, u64>,
    exit_time: &BTreeMap<NodeId, u64>,
    ancestor: NodeId,
    descendant: NodeId,
) -> bool {
    let (Some(&ancestor_entry), Some(&ancestor_exit)) =
        (entry_time.get(&ancestor), exit_time.get(&ancestor))
    else {
        return false;
    };
    let (Some(&descendant_entry), Some(&descendant_exit)) =
        (entry_time.get(&descendant), exit_time.get(&descendant))
    else {
        return false;
    };
    ancestor_entry <= descendant_entry && descendant_exit <= ancestor_exit
}

fn normalize_rows(rows: &mut [Vec<u32>]) {
    for neighbors in rows {
        *neighbors = heap_sorted(std::mem::take(neighbors));
        neighbors.dedup();
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use petgraph::algo::dominators::simple_fast;
    use petgraph::graphmap::DiGraphMap;
    use petgraph::Direction;
    use proptest::prelude::*;
    use rustc_hash::FxHashSet;

    use super::*;
    use crate::{
        CfgEdgeKind, CpgEdgeKind, CpgNode, CpgNodeKind, Language, MethodSignature, PdgBuilder,
        ProjectionKind, ScopeId, SourceRange, Visibility,
    };

    const LEGACY_VIRTUAL_EXIT: u32 = u32::MAX;

    fn projection(node_count: usize, edges: &[(usize, usize)]) -> GraphProjection {
        let nodes: Vec<NodeId> = (0..node_count as u32).map(NodeId::new).collect();
        GraphProjection::from_edges(
            ProjectionKind::External,
            nodes.iter().copied(),
            edges
                .iter()
                .map(|&(source, target)| (nodes[source], nodes[target], 1.0)),
        )
    }

    fn node(kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(0), kind, SourceRange::default())
    }

    fn function(name: &str) -> CpgNodeKind {
        CpgNodeKind::Function {
            signature: Box::new(MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            }),
        }
    }

    fn connect_cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId) {
        cpg.connect(
            source,
            target,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        );
    }

    fn control_edges(cpg: &CodePropertyGraph) -> BTreeSet<(NodeId, NodeId)> {
        cpg.edges()
            .filter(|edge| matches!(edge.kind, CpgEdgeKind::ControlDependence))
            .map(|edge| (edge.source, edge.target))
            .collect()
    }

    #[test]
    fn diamond_tree_frontier_intervals_and_chain_iterator_are_exact() {
        // 0 branches to 1/2, reconverges at 3, then reaches 4. Node 5 is
        // intentionally unreachable from the selected root.
        let graph = projection(6, &[(0, 1), (0, 2), (1, 3), (2, 3), (3, 4)]);
        let tree = dominator_tree(&graph, NodeId::new(0)).expect("diamond dominators");

        assert_eq!(
            tree.idom,
            BTreeMap::from([
                (NodeId::new(1), NodeId::new(0)),
                (NodeId::new(2), NodeId::new(0)),
                (NodeId::new(3), NodeId::new(0)),
                (NodeId::new(4), NodeId::new(3)),
            ])
        );
        assert_eq!(
            tree.children[&NodeId::new(0)],
            vec![NodeId::new(1), NodeId::new(2), NodeId::new(3)]
        );
        assert_eq!(tree.frontier[&NodeId::new(1)], vec![NodeId::new(3)]);
        assert_eq!(tree.frontier[&NodeId::new(2)], vec![NodeId::new(3)]);
        assert!(tree.dominates(NodeId::new(0), NodeId::new(4)));
        assert!(tree.dominates(NodeId::new(3), NodeId::new(4)));
        assert!(!tree.dominates(NodeId::new(1), NodeId::new(3)));
        assert!(!tree.dominates(NodeId::new(0), NodeId::new(5)));
        assert_eq!(
            tree.dominators_of(NodeId::new(4)).collect::<Vec<_>>(),
            vec![NodeId::new(4), NodeId::new(3), NodeId::new(0)]
        );
        assert!(tree.dominators_of(NodeId::new(5)).next().is_none());
        assert_eq!(tree.immediate_dominator(NodeId::new(0)), None);
    }

    #[test]
    fn dominator_tree_rejects_a_root_outside_the_projection() {
        let graph = projection(2, &[(0, 1)]);
        assert_eq!(
            dominator_tree(&graph, NodeId::new(99)),
            Err(AnalysisError::UnknownNode(NodeId::new(99)))
        );
    }

    #[test]
    fn dominator_iteration_is_bounded_for_a_malformed_parent_cycle() {
        let graph = projection(3, &[(0, 1), (1, 2)]);
        let mut tree = dominator_tree(&graph, NodeId::new(0)).expect("path dominators");
        tree.idom.insert(NodeId::new(1), NodeId::new(2));

        assert_eq!(
            tree.dominators_of(NodeId::new(2)).collect::<Vec<_>>(),
            vec![NodeId::new(2), NodeId::new(1), NodeId::new(2)]
        );
    }

    #[test]
    fn iterative_walks_handle_a_deep_dominator_tree() {
        const NODE_COUNT: usize = 25_000;
        let edges: Vec<_> = (0..NODE_COUNT - 1).map(|node| (node, node + 1)).collect();
        let graph = projection(NODE_COUNT, &edges);
        let tree = dominator_tree(&graph, NodeId::new(0)).expect("deep path dominators");
        assert_eq!(tree.idom.len(), NODE_COUNT - 1);
        assert_eq!(
            tree.immediate_dominator(NodeId::new((NODE_COUNT - 1) as u32)),
            Some(NodeId::new((NODE_COUNT - 2) as u32))
        );
        assert!(tree.dominates(NodeId::new(0), NodeId::new((NODE_COUNT - 1) as u32)));
    }

    #[test]
    fn post_dominators_and_control_dependence_match_a_hand_computed_if() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function_id = cpg.add_node(node(function("f")));
        let branch = cpg.add_node(node(CpgNodeKind::If));
        let then_node = cpg.add_node(node(CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        }));
        let else_node = cpg.add_node(node(CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        }));
        let join = cpg.add_node(node(CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        }));
        let exit = cpg.add_node(node(CpgNodeKind::Return));
        for (source, target) in [
            (function_id, branch),
            (branch, then_node),
            (branch, else_node),
            (then_node, join),
            (else_node, join),
            (join, exit),
        ] {
            connect_cfg(&mut cpg, source, target);
        }

        let tree = post_dominator_tree(&cpg, function_id).expect("if post-dominators");
        assert!(!tree.is_empty());
        assert_eq!(tree.exits, vec![exit]);
        assert_eq!(tree.ipdom[&function_id], branch);
        assert_eq!(tree.ipdom[&branch], join);
        assert_eq!(tree.ipdom[&then_node], join);
        assert_eq!(tree.ipdom[&else_node], join);
        assert_eq!(tree.ipdom[&join], exit);
        assert_eq!(tree.virtual_children, vec![exit]);
        assert!(tree.post_dominates(join, branch));
        assert!(tree.post_dominates(exit, function_id));
        assert!(!tree.post_dominates(branch, then_node));
        assert_eq!(
            tree.control_dependence_edges(),
            vec![(branch, then_node), (branch, else_node)]
        );

        PdgBuilder::new().build(&mut cpg, function_id);
        assert_eq!(
            control_edges(&cpg),
            tree.control_dependence_edges().into_iter().collect()
        );
    }

    #[test]
    fn post_dominators_model_infinite_regions_and_empty_cfgs() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function_id = cpg.add_node(node(function("f")));
        let branch = cpg.add_node(node(CpgNodeKind::If));
        let exit = cpg.add_node(node(CpgNodeKind::Return));
        let infinite = cpg.add_node(node(CpgNodeKind::Loop));
        for (source, target) in [
            (function_id, branch),
            (branch, exit),
            (branch, infinite),
            (infinite, infinite),
        ] {
            connect_cfg(&mut cpg, source, target);
        }
        let tree = post_dominator_tree(&cpg, function_id).expect("mixed finite/infinite CFG");
        assert_eq!(tree.exits, vec![exit]);
        assert_eq!(tree.virtual_children, vec![branch, exit, infinite]);
        assert!(!tree.is_empty());

        let mut no_exit = CodePropertyGraph::new(Language::Rust);
        let spin = no_exit.add_node(node(function("spin")));
        let loop_node = no_exit.add_node(node(CpgNodeKind::Loop));
        connect_cfg(&mut no_exit, spin, loop_node);
        connect_cfg(&mut no_exit, loop_node, spin);
        assert!(post_dominator_tree(&no_exit, spin)
            .expect("exit-free CFG degrades gracefully")
            .is_empty());

        let mut singleton = CodePropertyGraph::new(Language::Rust);
        let empty = singleton.add_node(node(function("empty")));
        assert!(post_dominator_tree(&singleton, empty)
            .expect("singleton CFG")
            .is_empty());
    }

    #[test]
    fn control_dependence_edges_sort_and_deduplicate_public_frontier_rows() {
        let mut tree = empty_post_dominator_tree(Vec::new());
        tree.frontier = BTreeMap::from([
            (
                NodeId::new(2),
                vec![NodeId::new(3), NodeId::new(1), NodeId::new(3)],
            ),
            (NodeId::new(1), vec![NodeId::new(2)]),
        ]);

        assert_eq!(
            tree.control_dependence_edges(),
            vec![
                (NodeId::new(1), NodeId::new(2)),
                (NodeId::new(2), NodeId::new(1)),
                (NodeId::new(3), NodeId::new(2)),
            ]
        );
    }

    #[test]
    fn post_dominator_scope_errors_remain_typed() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let block = cpg.add_node(node(CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        }));
        assert_eq!(
            post_dominator_tree(&cpg, NodeId::new(99)),
            Err(AnalysisError::UnknownNode(NodeId::new(99)))
        );
        assert_eq!(
            post_dominator_tree(&cpg, block),
            Err(AnalysisError::NotAFunction(block))
        );
    }

    fn legacy_control_dependence(
        cpg: &CodePropertyGraph,
        function: NodeId,
    ) -> BTreeSet<(NodeId, NodeId)> {
        let mut cfg_nodes = FxHashSet::default();
        cfg_nodes.insert(function);
        let mut stack = vec![function];
        while let Some(node) = stack.pop() {
            for (successor, kind) in cpg.cfg_successors(node) {
                if !matches!(kind, CfgEdgeKind::Call) && cfg_nodes.insert(successor) {
                    stack.push(successor);
                }
            }
        }
        if cfg_nodes.len() < 2 {
            return BTreeSet::new();
        }

        let mut reverse = DiGraphMap::<u32, ()>::new();
        reverse.add_node(LEGACY_VIRTUAL_EXIT);
        for &node in &cfg_nodes {
            reverse.add_node(node.as_u32());
        }
        let mut has_successor = FxHashSet::default();
        for &node in &cfg_nodes {
            for (successor, kind) in cpg.cfg_successors(node) {
                if !matches!(kind, CfgEdgeKind::Call) && cfg_nodes.contains(&successor) {
                    reverse.add_edge(successor.as_u32(), node.as_u32(), ());
                    has_successor.insert(node);
                }
            }
        }
        let recorded_exits: FxHashSet<NodeId> = cpg
            .cfg_exits()
            .iter()
            .copied()
            .filter(|node| cfg_nodes.contains(node))
            .collect();
        let mut has_exit = false;
        for &node in &cfg_nodes {
            let explicit_exit = matches!(
                cpg.node(node).map(|node| &node.kind),
                Some(CpgNodeKind::Return) | Some(CpgNodeKind::Throw)
            );
            if !has_successor.contains(&node) || explicit_exit || recorded_exits.contains(&node) {
                reverse.add_edge(LEGACY_VIRTUAL_EXIT, node.as_u32(), ());
                has_exit = true;
            }
        }
        if !has_exit {
            return BTreeSet::new();
        }

        let mut can_reach_exit = FxHashSet::default();
        let mut stack = vec![LEGACY_VIRTUAL_EXIT];
        while let Some(node) = stack.pop() {
            if !can_reach_exit.insert(node) {
                continue;
            }
            stack.extend(reverse.neighbors_directed(node, Direction::Outgoing));
        }
        for &node in &cfg_nodes {
            if !can_reach_exit.contains(&node.as_u32()) {
                reverse.add_edge(LEGACY_VIRTUAL_EXIT, node.as_u32(), ());
            }
        }

        let dominators = simple_fast(&reverse, LEGACY_VIRTUAL_EXIT);
        let mut edges = BTreeSet::new();
        for join in reverse.nodes().filter(|&node| node != LEGACY_VIRTUAL_EXIT) {
            let predecessors: Vec<_> = reverse
                .neighbors_directed(join, Direction::Incoming)
                .collect();
            if predecessors.len() < 2 {
                continue;
            }
            let Some(join_idom) = dominators.immediate_dominator(join) else {
                continue;
            };
            for predecessor in predecessors {
                let mut runner = predecessor;
                while runner != join_idom {
                    if runner != LEGACY_VIRTUAL_EXIT {
                        edges.insert((NodeId::new(join), NodeId::new(runner)));
                    }
                    let Some(parent) = dominators.immediate_dominator(runner) else {
                        break;
                    };
                    runner = parent;
                }
            }
        }
        edges
    }

    #[derive(Debug)]
    struct RemovalDominanceOracle {
        reachable: Vec<bool>,
        dominates: Vec<Vec<bool>>,
        idom: BTreeMap<NodeId, NodeId>,
        frontier: BTreeMap<NodeId, Vec<NodeId>>,
    }

    fn reachable_without(
        node_count: usize,
        edges: &[(usize, usize)],
        root: usize,
        excluded: Option<usize>,
    ) -> Vec<bool> {
        let mut reachable = vec![false; node_count];
        if excluded == Some(root) {
            return reachable;
        }
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if excluded == Some(node) || reachable[node] {
                continue;
            }
            reachable[node] = true;
            for &(source, target) in edges.iter().rev() {
                if source == node && excluded != Some(target) {
                    stack.push(target);
                }
            }
        }
        reachable
    }

    /// Independent exact oracle: `candidate` dominates `node` precisely when
    /// deleting `candidate` makes a previously reachable `node` unreachable.
    /// Immediate dominators and frontiers are then derived from that complete
    /// dominance relation rather than by another CHK-style fixed point.
    fn removal_dominance_oracle(
        node_count: usize,
        raw_edges: &[(usize, usize)],
    ) -> RemovalDominanceOracle {
        let mut edges: Vec<_> = raw_edges
            .iter()
            .copied()
            .filter(|&(source, target)| source < node_count && target < node_count)
            .collect();
        edges.sort_unstable();
        edges.dedup();

        let reachable = reachable_without(node_count, &edges, 0, None);
        let mut dominates = vec![vec![false; node_count]; node_count];
        for (candidate, dominance_row) in dominates.iter_mut().enumerate() {
            let without_candidate = reachable_without(node_count, &edges, 0, Some(candidate));
            for node in 0..node_count {
                dominance_row[node] =
                    reachable[node] && (candidate == node || !without_candidate[node]);
            }
        }

        let mut idom = BTreeMap::new();
        for node in 1..node_count {
            if !reachable[node] {
                continue;
            }
            let strict: Vec<_> = (0..node_count)
                .filter(|&candidate| candidate != node && dominates[candidate][node])
                .collect();
            let immediate: Vec<_> = strict
                .iter()
                .copied()
                .filter(|&candidate| {
                    strict
                        .iter()
                        .all(|&other| other == candidate || dominates[other][candidate])
                })
                .collect();
            assert_eq!(immediate.len(), 1, "unique exact idom for node {node}");
            idom.insert(NodeId::new(node as u32), NodeId::new(immediate[0] as u32));
        }

        let mut frontier = BTreeMap::new();
        for candidate in 0..node_count {
            if !reachable[candidate] {
                continue;
            }
            let members = (0..node_count)
                .filter(|&join| reachable[join])
                .filter(|&join| {
                    edges.iter().any(|&(predecessor, target)| {
                        target == join
                            && reachable[predecessor]
                            && dominates[candidate][predecessor]
                    }) && (candidate == join || !dominates[candidate][join])
                })
                .map(|join| NodeId::new(join as u32))
                .collect();
            frontier.insert(NodeId::new(candidate as u32), members);
        }

        RemovalDominanceOracle {
            reachable,
            dominates,
            idom,
            frontier,
        }
    }

    fn assert_matches_removal_oracle(node_count: usize, edges: &[(usize, usize)]) {
        let graph = projection(node_count, edges);
        let actual = dominator_tree(&graph, NodeId::new(0)).expect("valid oracle root");
        let oracle = removal_dominance_oracle(node_count, edges);
        assert_eq!(actual.idom, oracle.idom);
        assert_eq!(actual.frontier, oracle.frontier);
        for candidate in 0..node_count {
            for node in 0..node_count {
                assert_eq!(
                    actual.dominates(NodeId::new(candidate as u32), NodeId::new(node as u32)),
                    oracle.dominates[candidate][node],
                    "dominance mismatch for {candidate} -> {node}; reachable={:?}; edges={edges:?}",
                    oracle.reachable,
                );
            }
        }
    }

    #[test]
    fn all_directed_graphs_through_four_nodes_match_vertex_removal_oracle() {
        for node_count in 1..=4 {
            let possible_edges: Vec<_> = (0..node_count)
                .flat_map(|source| {
                    (0..node_count)
                        .filter(move |&target| source != target)
                        .map(move |target| (source, target))
                })
                .collect();
            for mask in 0u64..(1u64 << possible_edges.len()) {
                let edges: Vec<_> = possible_edges
                    .iter()
                    .enumerate()
                    .filter_map(|(bit, &edge)| ((mask >> bit) & 1 == 1).then_some(edge))
                    .collect();
                assert_matches_removal_oracle(node_count, &edges);
            }
        }
    }

    #[test]
    fn loops_multiple_backedges_irreducible_region_and_unreachable_entry_are_exact() {
        let edges = [
            (0, 1),
            (0, 2),
            (1, 3),
            (2, 3),
            (3, 1),
            (3, 4),
            (4, 2),
            (4, 5),
            (2, 5),
            (6, 3),
            (6, 7),
            (7, 6),
        ];
        assert_matches_removal_oracle(8, &edges);
        let tree = dominator_tree(&projection(8, &edges), NodeId::new(0)).expect("loop graph");
        assert!(!tree.idom.contains_key(&NodeId::new(6)));
        assert!(!tree.idom.contains_key(&NodeId::new(7)));
    }

    #[test]
    fn entry_self_loop_and_backedge_produce_the_exact_root_frontier() {
        let edges = [(0, 0), (0, 1), (1, 0)];
        assert_matches_removal_oracle(2, &edges);
        let tree = dominator_tree(&projection(2, &edges), NodeId::new(0)).expect("root cycle");
        assert_eq!(tree.frontier[&NodeId::new(0)], vec![NodeId::new(0)]);
        assert_eq!(tree.frontier[&NodeId::new(1)], vec![NodeId::new(0)]);
    }

    fn generated_postdom_cpg(
        node_count: usize,
        raw_edges: &[(usize, usize)],
        reverse_insertion: bool,
    ) -> (CodePropertyGraph, NodeId) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let mut insertion_order: Vec<_> = (0..node_count).collect();
        if reverse_insertion {
            insertion_order.reverse();
        }
        for index in insertion_order {
            let kind = if index == 0 {
                function("f")
            } else if index + 1 == node_count {
                CpgNodeKind::Return
            } else {
                CpgNodeKind::Block {
                    scope: ScopeId::GLOBAL,
                }
            };
            cpg.add_node_with_id(CpgNode::new(
                NodeId::new(index as u32),
                kind,
                SourceRange::default(),
            ));
        }

        let mut edges: Vec<_> = (0..node_count - 1)
            .map(|source| (source, source + 1))
            .chain(raw_edges.iter().copied())
            .filter(|&(source, target)| source < node_count && target < node_count)
            .collect();
        if reverse_insertion {
            edges.reverse();
        }
        for (source, target) in edges {
            connect_cfg(
                &mut cpg,
                NodeId::new(source as u32),
                NodeId::new(target as u32),
            );
        }
        (cpg, NodeId::new(0))
    }

    #[test]
    fn all_exit_reachable_cfgs_through_four_nodes_match_postdominator_oracle() {
        for node_count in 2..=4 {
            let possible_edges: Vec<_> = (0..node_count)
                .flat_map(|source| {
                    (0..node_count)
                        .filter(move |&target| source != target)
                        .map(move |target| (source, target))
                })
                .collect();
            for mask in 0u64..(1u64 << possible_edges.len()) {
                let edges: Vec<_> = possible_edges
                    .iter()
                    .enumerate()
                    .filter_map(|(bit, &edge)| ((mask >> bit) & 1 == 1).then_some(edge))
                    .collect();
                let (cpg, function) = generated_postdom_cpg(node_count, &edges, false);
                let expected = legacy_control_dependence(&cpg, function);
                let tree = post_dominator_tree(&cpg, function).expect("exhaustive postdominator");
                assert_eq!(
                    tree.control_dependence_edges()
                        .into_iter()
                        .collect::<BTreeSet<_>>(),
                    expected,
                    "node_count={node_count}; edges={edges:?}",
                );
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn chk_idom_matches_petgraph_and_is_permutation_deterministic(
            node_count in 1usize..24,
            raw_edges in prop::collection::vec((0usize..24, 0usize..24), 0..96),
        ) {
            let edges: Vec<_> = raw_edges
                .into_iter()
                .filter(|(source, target)| *source < node_count && *target < node_count)
                .collect();
            let graph = projection(node_count, &edges);
            let actual = dominator_tree(&graph, NodeId::new(0)).expect("valid root");
            let reversed = GraphProjection::from_edges(
                ProjectionKind::External,
                (0..node_count as u32).rev().map(NodeId::new),
                edges.iter().rev().map(|&(source, target)| {
                    (NodeId::new(source as u32), NodeId::new(target as u32), 1.0)
                }),
            );
            let repeated = dominator_tree(&reversed, NodeId::new(0)).expect("valid root");
            prop_assert_eq!(&actual, &repeated);

            let mut oracle = DiGraphMap::<u32, ()>::new();
            for node in 0..node_count as u32 {
                oracle.add_node(node);
            }
            for &(source, target) in &edges {
                oracle.add_edge(source as u32, target as u32, ());
            }
            let oracle = simple_fast(&oracle, 0);
            let expected: BTreeMap<NodeId, NodeId> = (1..node_count as u32)
                .filter_map(|node| {
                    oracle
                        .immediate_dominator(node)
                        .map(|idom| (NodeId::new(node), NodeId::new(idom)))
                })
                .collect();
            prop_assert_eq!(&actual.idom, &expected);
            for (&node, &idom) in &actual.idom {
                prop_assert!(actual.dominates(idom, node));
                prop_assert!(actual.dominates(actual.root, node));
            }
        }

        #[test]
        fn chk_tree_and_frontier_match_vertex_removal_oracle(
            node_count in 1usize..11,
            raw_edges in prop::collection::vec((0usize..11, 0usize..11), 0..72),
        ) {
            let edges: Vec<_> = raw_edges
                .into_iter()
                .filter(|(source, target)| *source < node_count && *target < node_count)
                .collect();
            assert_matches_removal_oracle(node_count, &edges);
        }

        #[test]
        fn post_dominator_results_are_node_and_edge_insertion_deterministic(
            node_count in 2usize..14,
            raw_edges in prop::collection::vec((0usize..14, 0usize..14), 0..64),
        ) {
            let edges: Vec<_> = raw_edges
                .into_iter()
                .filter(|(source, target)| *source < node_count && *target < node_count)
                .collect();
            let (forward, function) = generated_postdom_cpg(node_count, &edges, false);
            let (reverse, _) = generated_postdom_cpg(node_count, &edges, true);
            let forward = post_dominator_tree(&forward, function).expect("forward postdominator");
            let reverse = post_dominator_tree(&reverse, function).expect("reverse postdominator");
            prop_assert_eq!(forward, reverse);
        }

        #[test]
        fn post_dominance_and_pdg_match_the_legacy_petgraph_oracle(
            node_count in 2usize..14,
            raw_edges in prop::collection::vec((0usize..14, 0usize..14), 0..64),
        ) {
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            let function = cpg.add_node(node(function("f")));
            let mut nodes = vec![function];
            for index in 1..node_count {
                let kind = if index + 1 == node_count {
                    CpgNodeKind::Return
                } else {
                    CpgNodeKind::Block { scope: ScopeId::GLOBAL }
                };
                nodes.push(cpg.add_node(node(kind)));
            }
            // The path makes every generated node reachable and guarantees an
            // exit; arbitrary edges add loops, branches, and joins.
            for index in 0..node_count - 1 {
                connect_cfg(&mut cpg, nodes[index], nodes[index + 1]);
            }
            for (source, target) in raw_edges {
                if source < node_count && target < node_count {
                    connect_cfg(&mut cpg, nodes[source], nodes[target]);
                }
            }

            let expected = legacy_control_dependence(&cpg, function);
            let tree = post_dominator_tree(&cpg, function).expect("generated post-dominators");
            let actual: BTreeSet<_> = tree.control_dependence_edges().into_iter().collect();
            prop_assert_eq!(&actual, &expected);

            let counts_before = (cpg.node_count(), cpg.edge_count());
            PdgBuilder::new().build(&mut cpg, function);
            prop_assert_eq!(control_edges(&cpg), expected);
            prop_assert_eq!(cpg.node_count(), counts_before.0);
            prop_assert_eq!(cpg.edge_count(), counts_before.1 + actual.len());
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn dominator_results_round_trip_through_json() {
        let graph = projection(4, &[(0, 1), (0, 2), (1, 3), (2, 3)]);
        let tree = dominator_tree(&graph, NodeId::new(0)).expect("dominators");
        let encoded = serde_json::to_string(&tree).expect("serialize dominator tree");
        let decoded: DominatorTree =
            serde_json::from_str(&encoded).expect("deserialize dominator tree");
        assert_eq!(decoded, tree);
    }
}
