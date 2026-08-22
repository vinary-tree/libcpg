//! Deterministic, id-preserving basic-block CFG views.
//!
//! A block CFG groups reachable CPG control-flow nodes into maximal sequential
//! chains. It retains stable `NodeId` values and typed inter-block edges while
//! leaving the source `CodePropertyGraph` unchanged.

use std::collections::{BTreeMap, BTreeSet};

use petgraph::Direction;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{CfgEdgeKind, CodePropertyGraph, NodeId};

use super::{
    cfg_scope::{cfg_kind_rank, function_cfg_edges, function_cfg_view},
    AnalysisError, GraphProjection,
};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Dense, stable identifier of one block within a [`BlockCfg`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct BlockId(pub u32);

impl BlockId {
    /// Creates a block identifier from its dense index.
    #[inline]
    pub const fn new(id: u32) -> Self {
        Self(id)
    }

    /// Returns the dense block index.
    #[inline]
    pub const fn as_u32(self) -> u32 {
        self.0
    }
}

impl From<u32> for BlockId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}

impl From<BlockId> for u32 {
    fn from(id: BlockId) -> Self {
        id.0
    }
}

/// One maximal sequential run in a function's reachable CFG.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct BasicBlock {
    /// Dense block identifier; always equal to this block's vector index.
    pub id: BlockId,
    /// First node and sole control-flow entry of the block.
    pub leader: NodeId,
    /// CPG nodes in execution order; the first node is always `leader`.
    pub nodes: Vec<NodeId>,
}

/// One typed edge in the block-level CFG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct BlockEdge {
    /// Source block.
    pub source: BlockId,
    /// Target block.
    pub target: BlockId,
    /// Original CPG control-flow edge classification.
    pub kind: CfgEdgeKind,
}

/// Complete basic-block view for one function's reachable CFG.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct BlockCfg {
    /// Function whose control flow was grouped.
    pub function: NodeId,
    /// Blocks sorted by leader `NodeId`; `blocks[id.0].id == id`.
    pub blocks: Vec<BasicBlock>,
    /// Stable node-to-block membership for every projected CFG node.
    pub block_of_node: BTreeMap<NodeId, BlockId>,
    /// Deduplicated block edges sorted by source, target, and edge kind.
    pub edges: Vec<BlockEdge>,
    /// Block containing the function entry.
    pub entry: BlockId,
    /// Blocks containing structural or explicit function exits, sorted by id.
    pub exit_blocks: Vec<BlockId>,
    successor_edges: Vec<Vec<usize>>,
    predecessor_edges: Vec<Vec<usize>>,
    reverse_post_order: Vec<BlockId>,
    post_order: Vec<BlockId>,
}

impl BlockCfg {
    /// Returns a block by dense identifier.
    pub fn block(&self, id: BlockId) -> Option<&BasicBlock> {
        self.blocks.get(id.0 as usize)
    }

    /// Iterates outgoing block edges in canonical order.
    pub fn successors(&self, id: BlockId) -> impl Iterator<Item = &BlockEdge> {
        self.successor_edges
            .get(id.0 as usize)
            .into_iter()
            .flatten()
            .filter_map(|&edge| self.edges.get(edge))
    }

    /// Iterates incoming block edges in canonical order.
    pub fn predecessors(&self, id: BlockId) -> impl Iterator<Item = &BlockEdge> {
        self.predecessor_edges
            .get(id.0 as usize)
            .into_iter()
            .flatten()
            .filter_map(|&edge| self.edges.get(edge))
    }

    /// Returns the precomputed block order for a traversal direction.
    ///
    /// `Outgoing` selects reverse post-order for forward analyses; `Incoming`
    /// selects post-order for backward analyses.
    pub fn nodes_in(&self, direction: Direction) -> &[BlockId] {
        match direction {
            Direction::Outgoing => &self.reverse_post_order,
            Direction::Incoming => &self.post_order,
        }
    }

    /// Returns deterministic reverse post-order from the entry block.
    pub fn reverse_post_order(&self) -> &[BlockId] {
        &self.reverse_post_order
    }

    /// Returns deterministic post-order from the entry block.
    pub fn post_order(&self) -> &[BlockId] {
        &self.post_order
    }
}

/// Builds a deterministic basic-block CFG view for one function.
///
/// Leaders are the entry, every target of a non-sequential edge, every
/// successor of a multi-successor node, and every node with two or more
/// predecessors. Blocks then grow through unique sequential successor chains.
/// The result borrows no CPG payload and never adds, removes, or changes CPG
/// nodes or edges.
pub fn block_cfg(cpg: &CodePropertyGraph, function: NodeId) -> Result<BlockCfg, AnalysisError> {
    let view = function_cfg_view(cpg, function)?;
    let projection = &view.projection;
    let typed_edges = function_cfg_edges(cpg, projection);
    let entry_node = projection.node_id(view.entry);
    let leaders = find_leaders(projection, entry_node, &typed_edges);
    let pair_kinds = pair_edge_kinds(&typed_edges);
    let mut drafts = build_block_drafts(projection, &leaders, &pair_kinds);
    drafts.sort_unstable_by_key(|nodes| nodes[0]);

    let mut blocks = Vec::with_capacity(drafts.len());
    let mut block_of_node = BTreeMap::new();
    let mut position_in_block = FxHashMap::default();
    for (index, nodes) in drafts.into_iter().enumerate() {
        let id = BlockId(
            u32::try_from(index).expect("basic-block count exceeds the u32 identifier domain"),
        );
        let leader = nodes[0];
        for (position, &node) in nodes.iter().enumerate() {
            block_of_node.insert(node, id);
            position_in_block.insert(node, position);
        }
        blocks.push(BasicBlock { id, leader, nodes });
    }

    let entry = block_of_node[&entry_node];
    let mut edges = Vec::new();
    for &(source_node, target_node, kind) in &typed_edges {
        let (Some(&source), Some(&target)) = (
            block_of_node.get(&source_node),
            block_of_node.get(&target_node),
        ) else {
            continue;
        };
        let is_internal_step = source == target
            && kind == CfgEdgeKind::Sequential
            && position_in_block
                .get(&source_node)
                .zip(position_in_block.get(&target_node))
                .is_some_and(|(source, target)| *target == source.saturating_add(1));
        if !is_internal_step {
            edges.push(BlockEdge {
                source,
                target,
                kind,
            });
        }
    }
    edges.sort_unstable_by_key(|edge| (edge.source, edge.target, cfg_kind_rank(edge.kind)));
    edges.dedup();

    let (successor_edges, predecessor_edges) = edge_indices(blocks.len(), &edges);
    let adjacency = block_adjacency(blocks.len(), &edges);
    let (post_order, reverse_post_order) = traversal_orders(entry, &adjacency);
    let exit_blocks: Vec<_> = view
        .exit_like
        .iter()
        .filter_map(|&dense| block_of_node.get(&projection.node_id(dense)).copied())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    Ok(BlockCfg {
        function,
        blocks,
        block_of_node,
        edges,
        entry,
        exit_blocks,
        successor_edges,
        predecessor_edges,
        reverse_post_order,
        post_order,
    })
}

fn find_leaders(
    projection: &GraphProjection,
    entry: NodeId,
    typed_edges: &[(NodeId, NodeId, CfgEdgeKind)],
) -> BTreeSet<NodeId> {
    let mut leaders = BTreeSet::from([entry]);
    for dense in 0..projection.node_count() as u32 {
        if projection.predecessors(dense).len() >= 2 {
            leaders.insert(projection.node_id(dense));
        }
        let successors = projection.successors(dense);
        if successors.len() >= 2 {
            leaders.extend(
                successors
                    .iter()
                    .map(|&successor| projection.node_id(successor)),
            );
        }
    }
    leaders.extend(
        typed_edges
            .iter()
            .filter(|(_, _, kind)| *kind != CfgEdgeKind::Sequential)
            .map(|(_, target, _)| *target),
    );
    leaders
}

fn pair_edge_kinds(
    edges: &[(NodeId, NodeId, CfgEdgeKind)],
) -> FxHashMap<(NodeId, NodeId), Vec<CfgEdgeKind>> {
    let mut pairs: FxHashMap<_, Vec<_>> = FxHashMap::default();
    for &(source, target, kind) in edges {
        pairs.entry((source, target)).or_default().push(kind);
    }
    pairs
}

fn build_block_drafts(
    projection: &GraphProjection,
    leaders: &BTreeSet<NodeId>,
    pair_kinds: &FxHashMap<(NodeId, NodeId), Vec<CfgEdgeKind>>,
) -> Vec<Vec<NodeId>> {
    let mut assigned = FxHashSet::default();
    let mut drafts = Vec::new();
    for &leader in leaders {
        if !assigned.contains(&leader) {
            drafts.push(grow_block(
                projection,
                leader,
                leaders,
                pair_kinds,
                &mut assigned,
            ));
        }
    }

    // The leader rules cover every well-formed reachable projection. Retaining
    // a deterministic fallback makes the view total on malformed hand-built
    // CFGs without assigning a node twice.
    for &node in &projection.nodes {
        if !assigned.contains(&node) {
            drafts.push(grow_block(
                projection,
                node,
                leaders,
                pair_kinds,
                &mut assigned,
            ));
        }
    }
    drafts
}

fn grow_block(
    projection: &GraphProjection,
    leader: NodeId,
    leaders: &BTreeSet<NodeId>,
    pair_kinds: &FxHashMap<(NodeId, NodeId), Vec<CfgEdgeKind>>,
    assigned: &mut FxHashSet<NodeId>,
) -> Vec<NodeId> {
    let mut nodes = vec![leader];
    assigned.insert(leader);
    let mut current = leader;
    while let Some(current_dense) = projection.dense_index(current) {
        let [successor_dense] = projection.successors(current_dense) else {
            break;
        };
        let successor = projection.node_id(*successor_dense);
        let is_unique_sequential = pair_kinds.get(&(current, successor)).is_some_and(|kinds| {
            !kinds.is_empty() && kinds.iter().all(|kind| *kind == CfgEdgeKind::Sequential)
        });
        if !is_unique_sequential
            || leaders.contains(&successor)
            || assigned.contains(&successor)
            || projection
                .dense_index(successor)
                .is_none_or(|dense| projection.predecessors(dense).len() != 1)
        {
            break;
        }
        nodes.push(successor);
        assigned.insert(successor);
        current = successor;
    }
    nodes
}

fn edge_indices(block_count: usize, edges: &[BlockEdge]) -> (Vec<Vec<usize>>, Vec<Vec<usize>>) {
    let mut successors = vec![Vec::new(); block_count];
    let mut predecessors = vec![Vec::new(); block_count];
    for (index, edge) in edges.iter().enumerate() {
        if let Some(outgoing) = successors.get_mut(edge.source.0 as usize) {
            outgoing.push(index);
        }
        if let Some(incoming) = predecessors.get_mut(edge.target.0 as usize) {
            incoming.push(index);
        }
    }
    (successors, predecessors)
}

fn block_adjacency(block_count: usize, edges: &[BlockEdge]) -> Vec<Vec<BlockId>> {
    let mut adjacency = vec![Vec::new(); block_count];
    for edge in edges {
        if let Some(successors) = adjacency.get_mut(edge.source.0 as usize) {
            successors.push(edge.target);
        }
    }
    for successors in &mut adjacency {
        successors.sort_unstable();
        successors.dedup();
    }
    adjacency
}

fn traversal_orders(entry: BlockId, adjacency: &[Vec<BlockId>]) -> (Vec<BlockId>, Vec<BlockId>) {
    let mut visited = FxHashSet::default();
    let mut post_order = Vec::with_capacity(adjacency.len());
    let roots = std::iter::once(entry).chain(
        (0..adjacency.len())
            .map(|index| BlockId(index as u32))
            .filter(|&block| block != entry),
    );
    for root in roots {
        if !visited.insert(root) {
            continue;
        }
        let mut stack = vec![(root, 0usize)];
        while let Some((node, next)) = stack.last_mut() {
            let successors = adjacency
                .get(node.0 as usize)
                .map_or(&[][..], Vec::as_slice);
            if let Some(&successor) = successors.get(*next) {
                *next += 1;
                if visited.insert(successor) {
                    stack.push((successor, 0));
                }
            } else {
                post_order.push(*node);
                stack.pop();
            }
        }
    }
    let reverse_post_order = post_order.iter().rev().copied().collect();
    (post_order, reverse_post_order)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use proptest::prelude::*;

    use super::*;
    use crate::{
        CpgEdgeKind, CpgNode, CpgNodeKind, Language, MethodSignature, ScopeId, SourceRange,
        Visibility,
    };

    fn node_with_id(id: u32, kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(id), kind, SourceRange::default())
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

    fn block() -> CpgNodeKind {
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        }
    }

    fn connect_ast(cpg: &mut CodePropertyGraph, function: NodeId, node: NodeId) {
        cpg.connect(function, node, CpgEdgeKind::AstChild);
    }

    fn connect_cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId, kind: CfgEdgeKind) {
        cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind));
    }

    #[test]
    fn groups_a_diamond_into_canonical_blocks_and_edges() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function_id = cpg.add_node_with_id(node_with_id(10, function("diamond")));
        let guard = cpg.add_node_with_id(node_with_id(20, CpgNodeKind::If));
        let left = cpg.add_node_with_id(node_with_id(30, block()));
        let right = cpg.add_node_with_id(node_with_id(40, block()));
        let join = cpg.add_node_with_id(node_with_id(50, block()));
        let returned = cpg.add_node_with_id(node_with_id(60, CpgNodeKind::Return));
        for node in [guard, left, right, join, returned] {
            connect_ast(&mut cpg, function_id, node);
        }
        connect_cfg(&mut cpg, function_id, guard, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, guard, left, CfgEdgeKind::ConditionalTrue);
        connect_cfg(&mut cpg, guard, right, CfgEdgeKind::ConditionalFalse);
        connect_cfg(&mut cpg, left, join, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, right, join, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, join, returned, CfgEdgeKind::Sequential);

        let before = (cpg.node_count(), cpg.edge_count());
        let cfg = block_cfg(&cpg, function_id).expect("diamond block CFG");

        assert_eq!(cfg.blocks.len(), 4);
        assert_eq!(cfg.blocks[0].nodes, vec![function_id, guard]);
        assert_eq!(cfg.blocks[1].nodes, vec![left]);
        assert_eq!(cfg.blocks[2].nodes, vec![right]);
        assert_eq!(cfg.blocks[3].nodes, vec![join, returned]);
        assert_eq!(cfg.entry, BlockId(0));
        assert_eq!(cfg.exit_blocks, vec![BlockId(3)]);
        assert_eq!(
            cfg.edges,
            vec![
                BlockEdge {
                    source: BlockId(0),
                    target: BlockId(1),
                    kind: CfgEdgeKind::ConditionalTrue,
                },
                BlockEdge {
                    source: BlockId(0),
                    target: BlockId(2),
                    kind: CfgEdgeKind::ConditionalFalse,
                },
                BlockEdge {
                    source: BlockId(1),
                    target: BlockId(3),
                    kind: CfgEdgeKind::Sequential,
                },
                BlockEdge {
                    source: BlockId(2),
                    target: BlockId(3),
                    kind: CfgEdgeKind::Sequential,
                },
            ]
        );
        assert_eq!(
            cfg.successors(BlockId(0)).copied().collect::<Vec<_>>(),
            cfg.edges[..2]
        );
        assert_eq!(cfg.predecessors(BlockId(3)).count(), 2);
        assert!(cfg.block(BlockId(99)).is_none());
        assert_eq!(cfg.nodes_in(Direction::Outgoing), cfg.reverse_post_order());
        assert_eq!(cfg.nodes_in(Direction::Incoming), cfg.post_order());
        assert_eq!((cpg.node_count(), cpg.edge_count()), before);
    }

    #[test]
    fn loop_exit_and_non_sequential_targets_are_leaders() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function_id = cpg.add_node_with_id(node_with_id(1, function("loop")));
        let head = cpg.add_node_with_id(node_with_id(2, CpgNodeKind::While));
        let body = cpg.add_node_with_id(node_with_id(3, block()));
        let after = cpg.add_node_with_id(node_with_id(4, block()));
        for node in [head, body, after] {
            connect_ast(&mut cpg, function_id, node);
        }
        connect_cfg(&mut cpg, function_id, head, CfgEdgeKind::Sequential);
        connect_cfg(&mut cpg, head, body, CfgEdgeKind::ConditionalTrue);
        connect_cfg(&mut cpg, body, head, CfgEdgeKind::LoopBack);
        connect_cfg(&mut cpg, head, after, CfgEdgeKind::LoopExit);

        let cfg = block_cfg(&cpg, function_id).expect("loop block CFG");
        assert_eq!(cfg.block_of_node[&after], BlockId(3));
        assert_eq!(cfg.blocks[3].leader, after);
        assert!(cfg
            .edges
            .iter()
            .any(|edge| edge.kind == CfgEdgeKind::LoopExit));
    }

    #[test]
    fn cfg_only_graphs_and_invalid_selectors_are_total() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function_id = cpg.add_node_with_id(node_with_id(1, function("cfg_only")));
        let tail = cpg.add_node_with_id(node_with_id(2, block()));
        connect_cfg(&mut cpg, function_id, tail, CfgEdgeKind::Sequential);
        let cfg = block_cfg(&cpg, function_id).expect("CFG-only function");
        assert_eq!(cfg.blocks[0].nodes, vec![function_id, tail]);
        assert_eq!(
            block_cfg(&cpg, NodeId::new(99)),
            Err(AnalysisError::UnknownNode(NodeId::new(99)))
        );
        assert_eq!(
            block_cfg(&cpg, tail),
            Err(AnalysisError::NotAFunction(tail))
        );
    }

    fn fixture(
        node_count: usize,
        raw_edges: &[(usize, usize, u8)],
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
        let mut children: Vec<_> = (1..node_count).collect();
        if reverse {
            children.reverse();
        }
        for child in children {
            connect_ast(&mut cpg, NodeId::new(0), NodeId::new(child as u32));
        }

        let mut edges: Vec<_> = (0..node_count.saturating_sub(1))
            .map(|source| (source, source + 1, 0u8))
            .chain(raw_edges.iter().copied())
            .collect();
        if reverse {
            edges.reverse();
        }
        for (source, target, kind) in edges {
            connect_cfg(
                &mut cpg,
                NodeId::new((source % node_count) as u32),
                NodeId::new((target % node_count) as u32),
                match kind % 4 {
                    0 => CfgEdgeKind::Sequential,
                    1 => CfgEdgeKind::ConditionalTrue,
                    2 => CfgEdgeKind::LoopBack,
                    _ => CfgEdgeKind::LoopExit,
                },
            );
        }
        cpg
    }

    fn assert_partition_and_edge_oracle(cpg: &CodePropertyGraph, cfg: &BlockCfg) {
        let projection = super::super::cfg_projection(cpg, cfg.function).expect("projection");
        let members: Vec<_> = cfg
            .blocks
            .iter()
            .flat_map(|block| block.nodes.iter().copied())
            .collect();
        assert_eq!(
            members.iter().copied().collect::<BTreeSet<_>>(),
            projection.nodes.iter().copied().collect()
        );
        assert_eq!(members.len(), projection.nodes.len());

        for (index, block) in cfg.blocks.iter().enumerate() {
            assert_eq!(block.id.0 as usize, index);
            assert_eq!(block.nodes.first(), Some(&block.leader));
            for pair in block.nodes.windows(2) {
                let predecessor_dense = projection
                    .dense_index(pair[1])
                    .expect("block member belongs to projection");
                let predecessors: BTreeSet<_> = projection
                    .predecessors(predecessor_dense)
                    .iter()
                    .map(|&dense| projection.node_id(dense))
                    .collect();
                assert_eq!(predecessors, BTreeSet::from([pair[0]]));
                assert!(cpg
                    .cfg_predecessors(pair[1])
                    .iter()
                    .filter(|(source, _)| *source == pair[0])
                    .all(|(_, kind)| kind == CfgEdgeKind::Sequential));
            }
        }

        let mut expected_edges = Vec::new();
        for (source, target, kind) in function_cfg_edges(cpg, &projection) {
            let source_block = cfg.block_of_node[&source];
            let target_block = cfg.block_of_node[&target];
            let internal = source_block == target_block
                && kind == CfgEdgeKind::Sequential
                && cfg.blocks[source_block.0 as usize]
                    .nodes
                    .windows(2)
                    .any(|pair| pair == [source, target]);
            if !internal {
                expected_edges.push(BlockEdge {
                    source: source_block,
                    target: target_block,
                    kind,
                });
            }
        }
        expected_edges
            .sort_unstable_by_key(|edge| (edge.source, edge.target, cfg_kind_rank(edge.kind)));
        expected_edges.dedup();
        assert_eq!(cfg.edges, expected_edges);

        assert_eq!(
            cfg.post_order.iter().rev().copied().collect::<Vec<_>>(),
            cfg.reverse_post_order
        );
        assert_eq!(
            cfg.post_order.iter().copied().collect::<BTreeSet<_>>(),
            cfg.blocks.iter().map(|block| block.id).collect()
        );
    }

    proptest! {
        #[test]
        fn deterministic_partition_and_edge_oracles(
            node_count in 1usize..24,
            raw_edges in prop::collection::vec((0usize..24, 0usize..24, any::<u8>()), 0..80),
        ) {
            let forward = fixture(node_count, &raw_edges, false);
            let reverse = fixture(node_count, &raw_edges, true);
            let cfg = block_cfg(&forward, NodeId::new(0)).expect("forward block CFG");
            let permuted = block_cfg(&reverse, NodeId::new(0)).expect("permuted block CFG");
            prop_assert_eq!(cfg.clone(), permuted);
            assert_partition_and_edge_oracle(&forward, &cfg);
        }
    }

    #[test]
    fn traversal_is_stack_safe_for_twenty_five_thousand_blocks() {
        const NODE_COUNT: u32 = 25_000;
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let function_id = cpg.add_node_with_id(node_with_id(0, function("large")));
        for id in 1..NODE_COUNT {
            let node = cpg.add_node_with_id(node_with_id(id, block()));
            connect_ast(&mut cpg, function_id, node);
        }
        for id in 0..NODE_COUNT - 1 {
            connect_cfg(
                &mut cpg,
                NodeId::new(id),
                NodeId::new(id + 1),
                CfgEdgeKind::ConditionalTrue,
            );
        }

        let cfg = block_cfg(&cpg, function_id).expect("large block CFG");
        assert_eq!(cfg.blocks.len(), NODE_COUNT as usize);
        assert_eq!(cfg.reverse_post_order().len(), NODE_COUNT as usize);
        assert_eq!(cfg.post_order().len(), NODE_COUNT as usize);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn block_cfg_round_trips_through_serde() {
        let cpg = fixture(8, &[(2, 6, 1), (6, 3, 2)], false);
        let cfg = block_cfg(&cpg, NodeId::new(0)).expect("block CFG");
        let json = serde_json::to_string(&cfg).expect("serialize block CFG");
        let decoded: BlockCfg = serde_json::from_str(&json).expect("deserialize block CFG");
        assert_eq!(decoded, cfg);
    }
}
