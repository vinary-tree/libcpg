//! Code Property Graph implementation.
//!
//! The main graph data structure that combines AST, CFG, and DFG into a unified representation.

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::Direction;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use std::{
    fmt,
    sync::{Arc, OnceLock},
};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "serde")]
use super::serde_util::option_arc_str;

use super::projection_cache::{
    CfgNeighbors, CpgEdges, CpgProjectionCache, DenseNodeView, DfgNeighbors, EdgeLayer,
};
#[cfg(test)]
use super::CfgEdgeKind;
use super::{
    CpgEdge, CpgEdgeKind, CpgNode, CpgNodeKind, CpgNodeKindTag, DfgEdgeKind, EdgeId, Language,
    NodeId, SourceRange,
};

/// A Code Property Graph combining AST, CFG, and DFG.
///
/// This is the main data structure for program analysis, providing:
/// - Abstract Syntax Tree (AST) structure via parent/child relationships
/// - Control Flow Graph (CFG) via control flow edges
/// - Data Flow Graph (DFG) via def-use and use-def chains
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CodePropertyGraph {
    /// The underlying directed graph.
    graph: DiGraph<CpgNode, CpgEdge>,
    /// Map from NodeId to petgraph NodeIndex.
    node_index_map: FxHashMap<NodeId, NodeIndex>,
    /// The programming language of the source code.
    language: Language,
    /// The source file path (if available).
    #[cfg_attr(feature = "serde", serde(with = "option_arc_str"))]
    source_path: Option<Arc<str>>,
    /// The source code (if retained).
    #[cfg_attr(feature = "serde", serde(with = "option_arc_str"))]
    source_code: Option<Arc<str>>,
    /// Next available node ID.
    next_node_id: u32,
    /// Next available edge ID.
    next_edge_id: u32,
    /// The root node of the AST.
    root: Option<NodeId>,
    /// Entry points for the CFG (function entry nodes).
    cfg_entries: Vec<NodeId>,
    /// Exit points for the CFG (function exit/return nodes).
    cfg_exits: Vec<NodeId>,
    /// Lazily built CSR read index; every topology mutation invalidates it.
    #[cfg_attr(feature = "serde", serde(skip, default))]
    projection_cache: OnceLock<Box<CpgProjectionCache>>,
}

impl fmt::Debug for CodePropertyGraph {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CodePropertyGraph")
            .field("graph", &self.graph)
            .field("node_index_map", &self.node_index_map)
            .field("language", &self.language)
            .field("source_path", &self.source_path)
            .field("source_code", &self.source_code)
            .field("next_node_id", &self.next_node_id)
            .field("next_edge_id", &self.next_edge_id)
            .field("root", &self.root)
            .field("cfg_entries", &self.cfg_entries)
            .field("cfg_exits", &self.cfg_exits)
            .finish()
    }
}

impl CodePropertyGraph {
    /// Creates a new empty Code Property Graph.
    pub fn new(language: Language) -> Self {
        Self::with_capacity(language, 0, 0)
    }

    /// Creates an empty graph with storage reserved for the expected shape.
    ///
    /// This constructor is crate-private because capacities are an ingestion
    /// concern rather than graph semantics. Tree-sitter exposes its concrete
    /// syntax-node count in constant time, which gives the builder a safe upper
    /// bound for included CPG nodes and their AST edges without adding another
    /// syntax-tree traversal.
    pub(crate) fn with_capacity(
        language: Language,
        node_capacity: usize,
        edge_capacity: usize,
    ) -> Self {
        Self {
            graph: DiGraph::with_capacity(node_capacity, edge_capacity),
            node_index_map: FxHashMap::with_capacity_and_hasher(node_capacity, Default::default()),
            language,
            source_path: None,
            source_code: None,
            next_node_id: 0,
            next_edge_id: 0,
            root: None,
            cfg_entries: Vec::new(),
            cfg_exits: Vec::new(),
            projection_cache: OnceLock::new(),
        }
    }

    /// Sets the source file path.
    pub fn with_source_path(mut self, path: impl Into<Arc<str>>) -> Self {
        self.source_path = Some(path.into());
        self
    }

    /// Sets the source code (for reference during analysis).
    pub fn with_source_code(mut self, code: impl Into<Arc<str>>) -> Self {
        self.source_code = Some(code.into());
        self
    }

    /// Returns the programming language.
    pub fn language(&self) -> Language {
        self.language
    }

    /// Returns the source file path if set.
    pub fn source_path(&self) -> Option<&str> {
        self.source_path.as_deref()
    }

    /// Returns the source code if retained.
    pub fn source_code(&self) -> Option<&str> {
        self.source_code.as_deref()
    }

    /// Returns the root node ID.
    pub fn root(&self) -> Option<NodeId> {
        self.root
    }

    /// Returns the number of nodes in the graph.
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Returns the number of edges in the graph.
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    // ========== Node Operations ==========

    /// Adds a node to the graph and returns its ID.
    ///
    /// # Panics
    ///
    /// Panics after all `u32` node identifiers have been assigned. A graph can
    /// contain [`NodeId::new`] with `u32::MAX`, but no later automatic insertion can
    /// produce another unique identifier.
    pub fn add_node(&mut self, mut node: CpgNode) -> NodeId {
        let id = self.take_next_node_id();
        self.invalidate_projection_cache();
        node.id = id;

        let index = self.graph.add_node(node);
        self.node_index_map.insert(id, index);

        // Set root if this is the first node
        if self.root.is_none() {
            self.root = Some(id);
        }

        id
    }

    /// Adds a node with a specific ID (for reconstruction from serialization).
    pub fn add_node_with_id(&mut self, node: CpgNode) -> NodeId {
        self.invalidate_projection_cache();
        let id = node.id;
        self.next_node_id = self.next_node_id.max(id.0.saturating_add(1));

        let index = self.graph.add_node(node);
        self.node_index_map.insert(id, index);

        if self.root.is_none() {
            self.root = Some(id);
        }

        id
    }

    fn take_next_node_id(&mut self) -> NodeId {
        let id = NodeId::new(self.next_node_id);
        if self.next_node_id == u32::MAX {
            assert!(
                !self.node_index_map.contains_key(&id),
                "the CPG NodeId domain is exhausted"
            );
        } else {
            self.next_node_id += 1;
        }
        id
    }

    /// Returns a reference to a node by ID.
    pub fn node(&self, id: NodeId) -> Option<&CpgNode> {
        self.node_index_map
            .get(&id)
            .and_then(|&idx| self.graph.node_weight(idx))
    }

    /// Returns a node's complete copy-sized kind discriminant.
    ///
    /// This is the allocation-free choice when code only needs to branch on
    /// the [`CpgNodeKind`] variant and must subsequently mutate the graph.
    pub fn node_tag(&self, id: NodeId) -> Option<CpgNodeKindTag> {
        self.node(id).map(|node| node.kind.tag())
    }

    /// Returns a mutable reference to a node by ID.
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut CpgNode> {
        self.invalidate_projection_cache();
        self.node_index_map
            .get(&id)
            .copied()
            .and_then(|idx| self.graph.node_weight_mut(idx))
    }

    /// Returns true if the node exists.
    pub fn contains_node(&self, id: NodeId) -> bool {
        self.node_index_map.contains_key(&id)
    }

    /// Returns an iterator over all nodes.
    pub fn nodes(&self) -> impl Iterator<Item = &CpgNode> {
        self.graph.node_weights()
    }

    /// Returns an iterator over all node IDs.
    pub fn node_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.node_index_map.keys().copied()
    }

    /// Appends every node and edge from `other` using fresh identifiers.
    ///
    /// Source nodes are visited in ascending [`NodeId`] order, so an identical
    /// destination and source produce an identical mapping regardless of the
    /// source graph's insertion order. Every embedded node reference is
    /// translated through that mapping: AST parents and children, resolved
    /// call targets, identifier definitions, and lambda captures. A reference
    /// that does not name a node in `other` is discarded instead of becoming a
    /// dangling reference or accidentally binding to a colliding destination
    /// identifier.
    ///
    /// Edges are appended in ascending source [`EdgeId`] order with freshly
    /// allocated edge identifiers. CFG entry and exit metadata is translated
    /// as well. Existing destination nodes, edges, and metadata are never
    /// removed.
    ///
    /// The returned map translates each source identifier to its fresh
    /// destination identifier.
    pub fn merge_from(&mut self, other: &CodePropertyGraph) -> FxHashMap<NodeId, NodeId> {
        let destination_was_empty = self.node_count() == 0;
        let mut source_nodes: Vec<_> = other.nodes().cloned().collect();
        source_nodes.sort_by_key(|node| node.id);

        let mut remap = FxHashMap::default();
        for node in source_nodes {
            let source = node.id;
            let destination = self.add_node(node);
            remap.insert(source, destination);
        }

        let mut translated: Vec<_> = remap.iter().map(|(&old, &new)| (old, new)).collect();
        translated.sort_unstable_by_key(|(old, _)| *old);
        for (source, destination) in translated {
            let Some(source_node) = other.node(source) else {
                continue;
            };
            let Some(node) = self.node_mut(destination) else {
                continue;
            };

            node.parent = source_node
                .parent
                .and_then(|parent| remap.get(&parent).copied());
            // AST edges are appended below in ascending source EdgeId order.
            // Rebuilding this cache from those edges avoids duplicating the
            // child references copied with the node.
            node.children.clear();
            match &mut node.kind {
                CpgNodeKind::Call { target, .. } => {
                    *target = target.and_then(|target| remap.get(&target).copied());
                }
                CpgNodeKind::Identifier { definition, .. } => {
                    *definition = definition.and_then(|definition| remap.get(&definition).copied());
                }
                CpgNodeKind::Lambda { captures } => {
                    *captures = captures
                        .iter()
                        .filter_map(|capture| remap.get(capture).copied())
                        .collect();
                }
                _ => {}
            }
        }

        let mut source_edges: Vec<_> = other.edges().cloned().collect();
        source_edges.sort_by_key(|edge| edge.id.0);
        for mut edge in source_edges {
            let (Some(source), Some(target)) = (
                remap.get(&edge.source).copied(),
                remap.get(&edge.target).copied(),
            ) else {
                continue;
            };
            edge.source = source;
            edge.target = target;
            let _ = self.add_edge(edge);
        }

        let mut entries: Vec<_> = other
            .cfg_entries
            .iter()
            .filter_map(|entry| remap.get(entry).copied())
            .collect();
        entries.sort_unstable();
        for entry in entries {
            self.add_cfg_entry(entry);
        }

        let mut exits: Vec<_> = other
            .cfg_exits
            .iter()
            .filter_map(|exit| remap.get(exit).copied())
            .collect();
        exits.sort_unstable();
        for exit in exits {
            self.add_cfg_exit(exit);
        }

        if destination_was_empty {
            if let Some(root) = other.root.and_then(|root| remap.get(&root).copied()) {
                self.root = Some(root);
            }
        }

        remap
    }

    // ========== Edge Operations ==========

    /// Adds an edge to the graph and returns its ID.
    ///
    /// An [`CpgEdgeKind::AstChild`] edge also appends its target to the
    /// source node's child index and derives the target's parent from the
    /// lowest-ID incoming AST-child edge. Callers must not repeat that metadata
    /// update manually.
    ///
    /// # Panics
    ///
    /// Panics after all `u32` edge identifiers have been assigned. A graph can
    /// contain [`EdgeId::new`] with `u32::MAX`, but no later automatic insertion can
    /// produce another unique identifier.
    pub fn add_edge(&mut self, mut edge: CpgEdge) -> Option<EdgeId> {
        let source_idx = *self.node_index_map.get(&edge.source)?;
        let target_idx = *self.node_index_map.get(&edge.target)?;

        let id = self.take_next_edge_id();
        self.invalidate_projection_cache();
        edge.id = id;

        let is_ast_child = edge.kind == CpgEdgeKind::AstChild;
        let source = edge.source;
        let target = edge.target;
        let replace_ast_parent =
            is_ast_child && self.appended_ast_parent_needs_update(target_idx, source);
        self.graph.add_edge(source_idx, target_idx, edge);

        if is_ast_child {
            self.index_appended_ast_child(source, target, replace_ast_parent);
        }

        Some(id)
    }

    /// Adds an edge with a specific ID (for reconstruction from serialization).
    ///
    /// Explicit identifiers may arrive out of order, so AST-child insertion
    /// rebuilds the source node's child index in ascending [`EdgeId`] order.
    pub fn add_edge_with_id(&mut self, edge: CpgEdge) -> Option<EdgeId> {
        let source_idx = *self.node_index_map.get(&edge.source)?;
        let target_idx = *self.node_index_map.get(&edge.target)?;

        self.invalidate_projection_cache();
        let id = edge.id;
        let is_ast_child = edge.kind == CpgEdgeKind::AstChild;
        let source = edge.source;
        let target = edge.target;
        self.next_edge_id = self.next_edge_id.max(id.0.saturating_add(1));

        self.graph.add_edge(source_idx, target_idx, edge);

        if is_ast_child {
            self.rebuild_ast_children(source);
            self.rebuild_ast_parent(target);
        }

        Some(id)
    }

    fn take_next_edge_id(&mut self) -> EdgeId {
        let id = EdgeId::new(self.next_edge_id);
        if self.next_edge_id == u32::MAX {
            assert!(
                !self.graph.edge_weights().any(|edge| edge.id == id),
                "the CPG EdgeId domain is exhausted"
            );
        } else {
            self.next_edge_id += 1;
        }
        id
    }

    /// Appends a monotonically numbered AST edge to the stored source order.
    fn index_appended_ast_child(&mut self, source: NodeId, target: NodeId, replace_parent: bool) {
        if let Some(parent) = self.node_mut(source) {
            parent.children.push(target);
        }
        if replace_parent {
            if let Some(child) = self.node_mut(target) {
                child.parent = Some(source);
            }
        }
    }

    /// Decides whether a monotonic AST edge supplies the canonical parent.
    ///
    /// The common tree case needs no incoming-edge scan: an absent pointer is
    /// necessarily initialized, and an already matching pointer remains
    /// correct. Only conflicting pre-populated metadata needs the graph check.
    fn appended_ast_parent_needs_update(&self, target_idx: NodeIndex, source: NodeId) -> bool {
        match self
            .graph
            .node_weight(target_idx)
            .and_then(|node| node.parent)
        {
            None => true,
            Some(parent) if parent == source => false,
            Some(_) => !self
                .graph
                .edges_directed(target_idx, Direction::Incoming)
                .any(|edge| edge.weight().kind == CpgEdgeKind::AstChild),
        }
    }

    /// Restores ascending-edge-ID child order after explicit-ID insertion.
    ///
    /// Reconstruction may present edges in any order. The transient worklist
    /// is heap-backed (and inline for small sibling sets), so represented AST
    /// width cannot consume the native call stack.
    fn rebuild_ast_children(&mut self, source: NodeId) {
        let Some(&source_idx) = self.node_index_map.get(&source) else {
            return;
        };
        let mut ordered: SmallVec<[(EdgeId, NodeId); 8]> = self
            .graph
            .edges_directed(source_idx, Direction::Outgoing)
            .filter(|edge| edge.weight().kind == CpgEdgeKind::AstChild)
            .map(|edge| (edge.weight().id, edge.weight().target))
            .collect();
        ordered.sort_unstable_by_key(|(edge_id, _)| edge_id.0);

        if let Some(parent) = self.graph.node_weight_mut(source_idx) {
            parent.children.clear();
            parent
                .children
                .extend(ordered.into_iter().map(|(_, target)| target));
        }
    }

    /// Restores the deterministic parent selected by the lowest AST edge ID.
    fn rebuild_ast_parent(&mut self, target: NodeId) {
        let Some(&target_idx) = self.node_index_map.get(&target) else {
            return;
        };
        let parent = self
            .graph
            .edges_directed(target_idx, Direction::Incoming)
            .filter(|edge| edge.weight().kind == CpgEdgeKind::AstChild)
            .min_by_key(|edge| edge.weight().id.0)
            .map(|edge| edge.weight().source);

        if let Some(child) = self.graph.node_weight_mut(target_idx) {
            child.parent = parent;
        }
    }

    /// Creates and adds an edge between two nodes.
    pub fn connect(&mut self, source: NodeId, target: NodeId, kind: CpgEdgeKind) -> Option<EdgeId> {
        let edge = CpgEdge::new(EdgeId::new(0), source, target, kind);
        self.add_edge(edge)
    }

    /// Connects `source` to `target` with `kind`, but only if no identical edge
    /// (same endpoints and same kind) already exists.
    ///
    /// This makes repeated calls idempotent: re-running a builder that uses it
    /// adds no duplicate edges. Returns the id of the existing or newly-created
    /// edge, or `None` if either endpoint is absent.
    pub fn connect_unique(
        &mut self,
        source: NodeId,
        target: NodeId,
        kind: CpgEdgeKind,
    ) -> Option<EdgeId> {
        let source_index = *self.node_index_map.get(&source)?;
        let target_index = *self.node_index_map.get(&target)?;
        let existing = self
            .graph
            .edges_connecting(source_index, target_index)
            .find(|edge| edge.weight().kind == kind)
            .map(|edge| edge.weight().id);
        match existing {
            Some(id) => Some(id),
            None => self.connect(source, target, kind),
        }
    }

    /// Connects an edge whose label participates in its identity.
    ///
    /// This crate-internal variant keeps labeled overlays idempotent without
    /// conflating them with an unlabeled edge of the same kind and endpoints.
    pub(crate) fn connect_unique_labeled(
        &mut self,
        source: NodeId,
        target: NodeId,
        kind: CpgEdgeKind,
        label: &str,
    ) -> Option<EdgeId> {
        let source_index = *self.node_index_map.get(&source)?;
        let target_index = *self.node_index_map.get(&target)?;
        let existing = self
            .graph
            .edges_connecting(source_index, target_index)
            .find(|edge| {
                edge.weight().kind == kind && edge.weight().label.as_deref() == Some(label)
            })
            .map(|edge| edge.weight().id);
        match existing {
            Some(id) => Some(id),
            None => {
                self.add_edge(CpgEdge::new(EdgeId::new(0), source, target, kind).with_label(label))
            }
        }
    }

    /// Returns edges between two nodes.
    pub fn edges_between(&self, source: NodeId, target: NodeId) -> CpgEdges<'_> {
        self.projection_cache()
            .between(&self.graph, self.node_index(source), target)
    }

    /// Returns an iterator over all edges.
    pub fn edges(&self) -> impl Iterator<Item = &CpgEdge> {
        self.graph.edge_weights()
    }

    // ========== AST Traversal ==========

    /// Borrows the AST children of a node in source order.
    ///
    /// AST edge insertion maintains this contiguous child index in ascending
    /// [`EdgeId`] order. Reads therefore require neither a graph-edge scan nor
    /// a temporary allocation or sort. Missing node identifiers produce an
    /// empty slice.
    pub fn ast_children(&self, id: NodeId) -> &[NodeId] {
        self.node(id).map_or(&[], |node| node.children.as_slice())
    }

    /// Returns the AST parent of a node.
    pub fn ast_parent(&self, id: NodeId) -> Option<NodeId> {
        self.node(id)?.parent
    }

    /// Returns AST descendants of a node (depth-first).
    ///
    /// A `visited` set guards against cycles in `AstChild` edges (which a
    /// well-formed, parser-built AST never has, but a hand-constructed graph
    /// might): each node is emitted at most once and the traversal always
    /// terminates.
    pub fn ast_descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut result = Vec::new();
        let mut visited: rustc_hash::FxHashSet<NodeId> = rustc_hash::FxHashSet::default();
        visited.insert(id);
        let mut stack = vec![id];

        while let Some(current) = stack.pop() {
            for &child in self.ast_children(current) {
                if visited.insert(child) {
                    result.push(child);
                    stack.push(child);
                }
            }
        }

        result
    }

    /// Returns AST ancestors of a node (towards root).
    ///
    /// A `visited` set guards against cycles in the parent-pointer chain, so the
    /// walk always terminates even on a malformed graph.
    pub fn ast_ancestors(&self, id: NodeId) -> Vec<NodeId> {
        let mut result = Vec::new();
        let mut visited: rustc_hash::FxHashSet<NodeId> = rustc_hash::FxHashSet::default();
        visited.insert(id);
        let mut current = self.ast_parent(id);

        while let Some(parent) = current {
            // A hand-built graph may hold a parent pointer to a node that was
            // never added. Such an id is not an ancestor — it is not in the
            // graph at all — so the chain ends here rather than handing the
            // caller an id that `node()` will not resolve.
            if self.node(parent).is_none() {
                break;
            }
            if !visited.insert(parent) {
                break; // cycle detected
            }
            result.push(parent);
            current = self.ast_parent(parent);
        }

        result
    }

    // ========== CFG Traversal ==========

    /// Returns CFG successors of a node.
    pub fn cfg_successors(&self, id: NodeId) -> CfgNeighbors<'_> {
        CfgNeighbors::new(self.outgoing_layer(id, EdgeLayer::Cfg), true)
    }

    /// Returns CFG predecessors of a node.
    pub fn cfg_predecessors(&self, id: NodeId) -> CfgNeighbors<'_> {
        CfgNeighbors::new(self.incoming_layer(id, EdgeLayer::Cfg), false)
    }

    /// Returns all CFG entry nodes (function entries).
    pub fn cfg_entries(&self) -> &[NodeId] {
        &self.cfg_entries
    }

    /// Returns all CFG exit nodes.
    pub fn cfg_exits(&self) -> &[NodeId] {
        &self.cfg_exits
    }

    /// Adds a CFG entry point.
    pub fn add_cfg_entry(&mut self, id: NodeId) {
        if !self.cfg_entries.contains(&id) {
            self.cfg_entries.push(id);
        }
    }

    /// Adds a CFG exit point.
    pub fn add_cfg_exit(&mut self, id: NodeId) {
        if !self.cfg_exits.contains(&id) {
            self.cfg_exits.push(id);
        }
    }

    /// Returns all nodes in the CFG (nodes with control flow edges).
    pub fn cfg_nodes(&self) -> impl Iterator<Item = &CpgNode> {
        self.nodes().filter(|n| {
            let id = n.id;
            self.outgoing_edges(id).any(|e| e.kind.is_cfg())
                || self.incoming_edges(id).any(|e| e.kind.is_cfg())
        })
    }

    // ========== DFG Traversal ==========

    /// Returns definitions that reach this use site (def-use chain).
    pub fn reaching_definitions(&self, use_site: NodeId) -> Vec<NodeId> {
        self.incoming_edges(use_site)
            .filter_map(|e| {
                if let CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse | DfgEdgeKind::ReachingDef) =
                    &e.kind
                {
                    Some(e.source)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Returns uses of this definition (use-def chain).
    pub fn uses_of_definition(&self, def: NodeId) -> Vec<NodeId> {
        self.outgoing_edges(def)
            .filter_map(|e| {
                if let CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse) = &e.kind {
                    Some(e.target)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Returns data flow successors of a node.
    pub fn dfg_successors(&self, id: NodeId) -> DfgNeighbors<'_> {
        DfgNeighbors::new(self.outgoing_layer(id, EdgeLayer::Dfg), true)
    }

    /// Returns data flow predecessors of a node.
    pub fn dfg_predecessors(&self, id: NodeId) -> DfgNeighbors<'_> {
        DfgNeighbors::new(self.incoming_layer(id, EdgeLayer::Dfg), false)
    }

    /// Iterates outgoing abstract-heap relations in deterministic order.
    pub fn heap_successors(&self, id: NodeId) -> CpgEdges<'_> {
        self.outgoing_layer(id, EdgeLayer::Heap)
    }

    /// Iterates incoming abstract-heap relations in deterministic order.
    pub fn heap_predecessors(&self, id: NodeId) -> CpgEdges<'_> {
        self.incoming_layer(id, EdgeLayer::Heap)
    }

    /// Iterates outgoing reflective-name provenance in deterministic order.
    pub fn name_flow_successors(&self, id: NodeId) -> CpgEdges<'_> {
        self.outgoing_layer(id, EdgeLayer::NameFlow)
    }

    /// Iterates incoming reflective-name provenance in deterministic order.
    pub fn name_flow_predecessors(&self, id: NodeId) -> CpgEdges<'_> {
        self.incoming_layer(id, EdgeLayer::NameFlow)
    }

    /// Iterates outgoing rewrite dependencies and conflicts in deterministic order.
    pub fn rewrite_dep_successors(&self, id: NodeId) -> CpgEdges<'_> {
        self.outgoing_layer(id, EdgeLayer::RewriteDep)
    }

    /// Iterates incoming rewrite dependencies and conflicts in deterministic order.
    pub fn rewrite_dep_predecessors(&self, id: NodeId) -> CpgEdges<'_> {
        self.incoming_layer(id, EdgeLayer::RewriteDep)
    }

    /// Iterates outgoing ownership and borrowing relations deterministically.
    pub fn place_capability_successors(&self, id: NodeId) -> CpgEdges<'_> {
        self.outgoing_layer(id, EdgeLayer::PlaceCapability)
    }

    /// Iterates incoming ownership and borrowing relations deterministically.
    pub fn place_capability_predecessors(&self, id: NodeId) -> CpgEdges<'_> {
        self.incoming_layer(id, EdgeLayer::PlaceCapability)
    }

    // ========== Call Graph ==========

    /// Returns call sites in this function.
    pub fn call_sites(&self, function: NodeId) -> Vec<NodeId> {
        self.ast_descendants(function)
            .into_iter()
            .filter(|&id| {
                self.node(id)
                    .map(|n| matches!(n.kind, CpgNodeKind::Call { .. }))
                    .unwrap_or(false)
            })
            .collect()
    }

    /// Returns callees of a call site.
    pub fn callees(&self, call_site: NodeId) -> Vec<NodeId> {
        let mut callees: Vec<_> = self
            .native_outgoing_edges(call_site)
            .filter_map(|e| {
                if matches!(
                    e.kind,
                    CpgEdgeKind::CallSite | CpgEdgeKind::StaticCall | CpgEdgeKind::DynamicCall
                ) {
                    Some(e.target)
                } else {
                    None
                }
            })
            .collect();
        callees.sort_unstable();
        callees.dedup();
        callees
    }

    /// Returns callers of a function.
    pub fn callers(&self, function: NodeId) -> Vec<NodeId> {
        let mut callers: Vec<_> = self
            .native_incoming_edges(function)
            .filter_map(|e| {
                if matches!(
                    e.kind,
                    CpgEdgeKind::CallSite | CpgEdgeKind::StaticCall | CpgEdgeKind::DynamicCall
                ) {
                    Some(e.source)
                } else {
                    None
                }
            })
            .collect();
        callers.sort_unstable();
        callers.dedup();
        callers
    }

    // ========== Query by Kind ==========

    /// Returns all function nodes.
    pub fn functions(&self) -> impl Iterator<Item = &CpgNode> {
        self.nodes()
            .filter(|n| matches!(n.kind, CpgNodeKind::Function { .. }))
    }

    /// Returns all class nodes.
    pub fn classes(&self) -> impl Iterator<Item = &CpgNode> {
        self.nodes()
            .filter(|n| matches!(n.kind, CpgNodeKind::Class { .. }))
    }

    /// Returns all variable declaration nodes.
    pub fn variables(&self) -> impl Iterator<Item = &CpgNode> {
        self.nodes()
            .filter(|n| matches!(n.kind, CpgNodeKind::Variable { .. }))
    }

    /// Returns all call expression nodes.
    pub fn calls(&self) -> impl Iterator<Item = &CpgNode> {
        self.nodes()
            .filter(|n| matches!(n.kind, CpgNodeKind::Call { .. }))
    }

    /// Returns all nodes of a specific kind.
    pub fn nodes_by_kind<F>(&self, predicate: F) -> impl Iterator<Item = &CpgNode>
    where
        F: Fn(&CpgNodeKind) -> bool,
    {
        self.nodes().filter(move |n| predicate(&n.kind))
    }

    // ========== Query by Position ==========

    /// Returns the node at a specific byte offset.
    pub fn node_at_offset(&self, offset: u32) -> Option<&CpgNode> {
        self.nodes()
            .filter(|n| n.range.start <= offset && offset < n.range.end)
            .min_by_key(|n| n.range.len())
    }

    /// Returns nodes overlapping a range.
    pub fn nodes_in_range(&self, range: SourceRange) -> Vec<&CpgNode> {
        self.nodes()
            .filter(|n| n.range.start < range.end && n.range.end > range.start)
            .collect()
    }

    /// Returns the innermost scope at an offset.
    pub fn scope_at_offset(&self, offset: u32) -> Option<&CpgNode> {
        self.nodes()
            .filter(|n| {
                n.range.start <= offset
                    && offset < n.range.end
                    && matches!(
                        n.kind,
                        CpgNodeKind::Block { .. } | CpgNodeKind::Function { .. }
                    )
            })
            .min_by_key(|n| n.range.len())
    }

    // ========== Edge Helpers ==========

    /// Returns outgoing edges from a node.
    pub fn outgoing_edges(&self, id: NodeId) -> CpgEdges<'_> {
        self.projection_cache()
            .outgoing(&self.graph, self.node_index(id))
    }

    /// Returns incoming edges to a node.
    pub fn incoming_edges(&self, id: NodeId) -> CpgEdges<'_> {
        self.projection_cache()
            .incoming(&self.graph, self.node_index(id))
    }

    fn overlay_edges(&self, layer: EdgeLayer) -> impl Iterator<Item = &CpgEdge> {
        self.projection_cache().layer(&self.graph, layer)
    }

    /// Borrows dense node metadata aligned by ascending stable identifier.
    ///
    /// Search algorithms use this crate-internal view to keep their hot state
    /// vector-indexed without copying or re-sorting graph identifiers.
    pub(crate) fn dense_node_view(&self) -> DenseNodeView<'_> {
        self.projection_cache().dense_nodes(&self.graph)
    }

    /// Iterates the complete AST overlay in canonical source/target/id order.
    pub(crate) fn ast_overlay_edges(&self) -> impl Iterator<Item = &CpgEdge> {
        self.overlay_edges(EdgeLayer::Ast)
    }

    /// Iterates the complete CFG overlay in canonical source/target/id order.
    pub(crate) fn cfg_overlay_edges(&self) -> impl Iterator<Item = &CpgEdge> {
        self.overlay_edges(EdgeLayer::Cfg)
    }

    /// Iterates the complete DFG overlay in canonical source/target/id order.
    pub(crate) fn dfg_overlay_edges(&self) -> impl Iterator<Item = &CpgEdge> {
        self.overlay_edges(EdgeLayer::Dfg)
    }

    /// Iterates the complete PDG overlay in canonical source/target/id order.
    pub(crate) fn pdg_overlay_edges(&self) -> impl Iterator<Item = &CpgEdge> {
        self.overlay_edges(EdgeLayer::Pdg)
    }

    /// Iterates the complete heap overlay in canonical source/target/id order.
    pub fn heap_edges(&self) -> impl Iterator<Item = &CpgEdge> {
        self.overlay_edges(EdgeLayer::Heap)
    }

    /// Iterates the complete NameFlow overlay in canonical order.
    pub fn name_flow_edges(&self) -> impl Iterator<Item = &CpgEdge> {
        self.overlay_edges(EdgeLayer::NameFlow)
    }

    /// Iterates the complete RewriteDep overlay in canonical order.
    pub fn rewrite_dep_edges(&self) -> impl Iterator<Item = &CpgEdge> {
        self.overlay_edges(EdgeLayer::RewriteDep)
    }

    /// Iterates the complete PlaceCapability overlay in canonical order.
    pub fn place_capability_edges(&self) -> impl Iterator<Item = &CpgEdge> {
        self.overlay_edges(EdgeLayer::PlaceCapability)
    }

    /// Iterates the explicit call overlay in canonical source/target/id order.
    pub(crate) fn call_overlay_edges(&self) -> impl Iterator<Item = &CpgEdge> {
        self.overlay_edges(EdgeLayer::Call)
    }

    /// Walks petgraph's native outgoing row without constructing the read cache.
    ///
    /// This is reserved for crate-internal algorithms that make one complete
    /// pass and canonicalize their own result. Public repeated/random access
    /// goes through the deterministic CSR cache.
    #[inline]
    pub(crate) fn native_outgoing_edges(&self, id: NodeId) -> impl Iterator<Item = &CpgEdge> {
        self.node_index_map
            .get(&id)
            .map(|&index| {
                self.graph
                    .edges_directed(index, Direction::Outgoing)
                    .map(|edge| edge.weight())
            })
            .into_iter()
            .flatten()
    }

    /// Walks petgraph's native incoming row without constructing the read cache.
    #[inline]
    pub(crate) fn native_incoming_edges(&self, id: NodeId) -> impl Iterator<Item = &CpgEdge> {
        self.node_index_map
            .get(&id)
            .map(|&index| {
                self.graph
                    .edges_directed(index, Direction::Incoming)
                    .map(|edge| edge.weight())
            })
            .into_iter()
            .flatten()
    }

    /// Walks petgraph's native endpoint relation without constructing the cache.
    ///
    /// Mutation-oriented passes use this only for a local check immediately
    /// before an insertion, which would otherwise invalidate a freshly built
    /// read cache.
    #[inline]
    pub(crate) fn native_edges_between(
        &self,
        source: NodeId,
        target: NodeId,
    ) -> impl Iterator<Item = &CpgEdge> {
        self.node_index_map
            .get(&source)
            .copied()
            .zip(self.node_index_map.get(&target).copied())
            .map(|(source, target)| {
                self.graph
                    .edges_connecting(source, target)
                    .map(|edge| edge.weight())
            })
            .into_iter()
            .flatten()
    }

    fn outgoing_layer(&self, id: NodeId, layer: EdgeLayer) -> CpgEdges<'_> {
        self.projection_cache()
            .outgoing_layer(&self.graph, self.node_index(id), layer)
    }

    fn incoming_layer(&self, id: NodeId, layer: EdgeLayer) -> CpgEdges<'_> {
        self.projection_cache()
            .incoming_layer(&self.graph, self.node_index(id), layer)
    }

    fn node_index(&self, id: NodeId) -> Option<NodeIndex> {
        self.node_index_map.get(&id).copied()
    }

    fn projection_cache(&self) -> &CpgProjectionCache {
        self.projection_cache
            .get_or_init(|| Box::new(CpgProjectionCache::build(&self.graph)))
            .as_ref()
    }

    fn invalidate_projection_cache(&mut self) {
        let _ = self.projection_cache.take();
    }

    /// Returns all edges of a specific kind.
    pub fn edges_by_kind<F>(&self, predicate: F) -> impl Iterator<Item = &CpgEdge>
    where
        F: Fn(&CpgEdgeKind) -> bool,
    {
        self.edges().filter(move |e| predicate(&e.kind))
    }

    // ========== Graph Metrics ==========

    /// Returns the depth of the AST (longest path from root).
    ///
    /// A typed heap continuation machine tracks the current root-to-node path,
    /// the next child to evaluate, and the greatest completed child depth. A
    /// cyclic `AstChild` edge is cut when its target is already on the active
    /// path. For a well-formed tree this is exactly the longest root-to-leaf
    /// path, and native stack use is independent of represented AST depth.
    pub fn ast_depth(&self) -> usize {
        let root = match self.root {
            Some(r) => r,
            None => return 0,
        };

        enum DepthFrame<'a> {
            Enter(NodeId),
            Resume {
                node: NodeId,
                children: &'a [NodeId],
                next_child: usize,
                maximum_child_depth: usize,
            },
        }

        let node_count = self.node_count();
        let mut frames = Vec::with_capacity(node_count);
        let mut values = Vec::with_capacity(node_count);
        let mut on_path =
            rustc_hash::FxHashSet::with_capacity_and_hasher(node_count, Default::default());
        frames.push(DepthFrame::Enter(root));

        while let Some(frame) = frames.pop() {
            match frame {
                DepthFrame::Enter(node) => {
                    if !on_path.insert(node) {
                        values.push(0);
                        continue;
                    }

                    let children = self.ast_children(node);
                    match children.first().copied() {
                        Some(first_child) => {
                            frames.push(DepthFrame::Resume {
                                node,
                                children,
                                next_child: 1,
                                maximum_child_depth: 0,
                            });
                            frames.push(DepthFrame::Enter(first_child));
                        }
                        None => {
                            on_path.remove(&node);
                            values.push(1);
                        }
                    }
                }
                DepthFrame::Resume {
                    node,
                    children,
                    next_child,
                    maximum_child_depth,
                } => {
                    let child_depth = values
                        .pop()
                        .expect("every resumed AST-depth frame has one child value");
                    let maximum_child_depth = maximum_child_depth.max(child_depth);
                    if let Some(next) = children.get(next_child).copied() {
                        frames.push(DepthFrame::Resume {
                            node,
                            children,
                            next_child: next_child + 1,
                            maximum_child_depth,
                        });
                        frames.push(DepthFrame::Enter(next));
                    } else {
                        on_path.remove(&node);
                        values.push(1usize.saturating_add(maximum_child_depth));
                    }
                }
            }
        }

        values
            .pop()
            .expect("the AST-depth root produces exactly one value")
    }

    /// Returns the cyclomatic complexity (number of linearly independent paths).
    pub fn cyclomatic_complexity(&self) -> usize {
        // M = E - N + 2P where E = edges, N = nodes, P = connected components
        // For a single function, P = 1
        let cfg_edges = self.edges().filter(|e| e.kind.is_cfg()).count();
        let cfg_nodes = self.cfg_nodes().count();

        if cfg_nodes == 0 {
            1
        } else {
            cfg_edges.saturating_sub(cfg_nodes) + 2
        }
    }

    /// Returns statistics about the graph.
    pub fn stats(&self) -> CpgStats {
        let ast_edges = self.ast_overlay_edges().count();
        let cfg_edges = self.cfg_overlay_edges().count();
        let dfg_edges = self.dfg_overlay_edges().count();
        let heap_edges = self.heap_edges().count();
        let name_flow_edges = self.name_flow_edges().count();
        let rewrite_dep_edges = self.rewrite_dep_edges().count();
        let place_capability_edges = self.place_capability_edges().count();
        let call_edges = self.call_overlay_edges().count();

        CpgStats {
            node_count: self.node_count(),
            edge_count: self.edge_count(),
            ast_edges,
            cfg_edges,
            dfg_edges,
            heap_edges,
            name_flow_edges,
            rewrite_dep_edges,
            place_capability_edges,
            call_edges,
            function_count: self.functions().count(),
            class_count: self.classes().count(),
            cyclomatic_complexity: self.cyclomatic_complexity(),
        }
    }

    // ========== Subgraph Extraction ==========

    /// Extracts a subgraph containing the specified nodes and edges between them.
    pub fn subgraph(&self, node_ids: &[NodeId]) -> Self {
        let mut sub = Self::new(self.language);
        sub.source_path = self.source_path.clone();

        let id_set: rustc_hash::FxHashSet<NodeId> = node_ids.iter().copied().collect();

        // Add nodes
        for &id in node_ids {
            if let Some(node) = self.node(id) {
                let mut node = node.clone();
                node.children.clear();
                node.parent = node.parent.filter(|parent| id_set.contains(parent));
                sub.add_node_with_id(node);
            }
        }

        // Add edges between included nodes
        for edge in self.edges() {
            if id_set.contains(&edge.source) && id_set.contains(&edge.target) {
                sub.add_edge_with_id(edge.clone());
            }
        }

        sub
    }

    /// Extracts the CFG subgraph for a function.
    pub fn function_cfg(&self, function: NodeId) -> Self {
        let descendants = self.ast_descendants(function);
        let mut nodes: Vec<NodeId> = descendants
            .into_iter()
            .filter(|&id| {
                self.node(id)
                    .map(|n| n.is_control_flow() || n.is_expression())
                    .unwrap_or(false)
            })
            .collect();
        nodes.push(function);

        self.subgraph(&nodes)
    }

    /// Extracts the DFG subgraph for a function.
    pub fn function_dfg(&self, function: NodeId) -> Self {
        let descendants = self.ast_descendants(function);
        let mut nodes: SmallVec<[NodeId; 64]> = SmallVec::new();

        for id in descendants {
            if self.dfg_successors(id).is_empty() && self.dfg_predecessors(id).is_empty() {
                continue;
            }
            nodes.push(id);
        }

        self.subgraph(&nodes)
    }
}

impl Clone for CodePropertyGraph {
    fn clone(&self) -> Self {
        Self {
            graph: self.graph.clone(),
            node_index_map: self.node_index_map.clone(),
            language: self.language,
            source_path: self.source_path.clone(),
            source_code: self.source_code.clone(),
            next_node_id: self.next_node_id,
            next_edge_id: self.next_edge_id,
            root: self.root,
            cfg_entries: self.cfg_entries.clone(),
            cfg_exits: self.cfg_exits.clone(),
            projection_cache: OnceLock::new(),
        }
    }
}

impl Default for CodePropertyGraph {
    fn default() -> Self {
        Self::new(Language::Unknown)
    }
}

#[cfg(test)]
mod merge_tests {
    use super::*;
    use crate::{MethodSignature, ScopeId, Visibility};
    use smallvec::smallvec;

    fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
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
                visibility: Visibility::Public,
            }),
        }
    }

    pub(super) fn all_references_resolve(cpg: &CodePropertyGraph, nodes: &[NodeId]) -> bool {
        nodes.iter().all(|&id| {
            let Some(node) = cpg.node(id) else {
                return false;
            };
            node.parent.is_none_or(|target| cpg.contains_node(target))
                && node
                    .children
                    .iter()
                    .all(|&target| cpg.contains_node(target))
                && match &node.kind {
                    CpgNodeKind::Call { target, .. } => {
                        target.is_none_or(|target| cpg.contains_node(target))
                    }
                    CpgNodeKind::Identifier { definition, .. } => {
                        definition.is_none_or(|target| cpg.contains_node(target))
                    }
                    CpgNodeKind::Lambda { captures } => {
                        captures.iter().all(|&target| cpg.contains_node(target))
                    }
                    _ => true,
                }
        })
    }

    pub(super) fn shape(
        cpg: &CodePropertyGraph,
    ) -> (Vec<String>, Vec<String>, Vec<NodeId>, Vec<NodeId>) {
        let mut nodes: Vec<_> = cpg
            .nodes()
            .map(|node| {
                format!(
                    "{:?}|{:?}|{:?}|{:?}",
                    node.id, node.kind, node.parent, node.children
                )
            })
            .collect();
        nodes.sort();
        let mut edges: Vec<_> = cpg
            .edges()
            .map(|edge| {
                format!(
                    "{:?}|{:?}|{:?}|{:?}|{:?}",
                    edge.id, edge.source, edge.target, edge.kind, edge.label
                )
            })
            .collect();
        edges.sort();
        let mut entries = cpg.cfg_entries().to_vec();
        entries.sort_unstable();
        let mut exits = cpg.cfg_exits().to_vec();
        exits.sort_unstable();
        (nodes, edges, entries, exits)
    }

    #[test]
    fn merge_remaps_the_complete_embedded_reference_set_and_metadata() {
        let mut source = CodePropertyGraph::new(Language::Rust);
        let root = source.add_node_with_id(
            node(40, CpgNodeKind::Root)
                .with_child(NodeId::new(10))
                .with_child(NodeId::new(20))
                .with_child(NodeId::new(30)),
        );
        let declaration = source.add_node_with_id(node(10, function("target")).with_parent(root));
        let identifier = source.add_node_with_id(
            node(
                20,
                CpgNodeKind::Identifier {
                    name: "target".into(),
                    definition: Some(declaration),
                },
            )
            .with_parent(root),
        );
        let call = source.add_node_with_id(
            node(
                30,
                CpgNodeKind::Call {
                    target: Some(declaration),
                    is_method: false,
                },
            )
            .with_parent(root),
        );
        let lambda = source.add_node_with_id(
            node(
                50,
                CpgNodeKind::Lambda {
                    captures: smallvec![declaration, identifier],
                },
            )
            .with_parent(root),
        );
        source.connect(root, declaration, CpgEdgeKind::AstChild);
        source.connect(root, identifier, CpgEdgeKind::AstChild);
        source.connect(root, call, CpgEdgeKind::AstChild);
        source.connect(root, lambda, CpgEdgeKind::AstChild);
        source.add_edge(
            CpgEdge::new(
                EdgeId::new(900),
                identifier,
                declaration,
                CpgEdgeKind::Reference,
            )
            .with_label("fixture"),
        );
        source.add_cfg_entry(declaration);
        source.add_cfg_exit(call);

        let mut destination = CodePropertyGraph::new(Language::Rust);
        let retained = destination.add_node(node(
            0,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        ));
        let retained_root = destination.root();
        let map = destination.merge_from(&source);

        assert_eq!(map[&NodeId::new(10)], NodeId::new(1));
        assert_eq!(map[&NodeId::new(20)], NodeId::new(2));
        assert_eq!(map[&NodeId::new(30)], NodeId::new(3));
        assert_eq!(map[&NodeId::new(40)], NodeId::new(4));
        assert_eq!(map[&NodeId::new(50)], NodeId::new(5));
        assert_eq!(destination.root(), retained_root);
        assert_eq!(destination.node_count(), 6);
        assert_eq!(destination.node(retained).expect("retained").parent, None);

        let mapped_root = map[&root];
        let mapped_declaration = map[&declaration];
        let mapped_identifier = map[&identifier];
        let mapped_call = map[&call];
        let mapped_lambda = map[&lambda];
        assert_eq!(
            destination
                .node(mapped_root)
                .expect("mapped root")
                .children
                .as_slice(),
            &[
                mapped_declaration,
                mapped_identifier,
                mapped_call,
                mapped_lambda
            ]
        );
        assert_eq!(
            destination
                .node(mapped_identifier)
                .expect("identifier")
                .kind,
            CpgNodeKind::Identifier {
                name: "target".into(),
                definition: Some(mapped_declaration),
            }
        );
        assert_eq!(
            destination.node(mapped_call).expect("call").kind,
            CpgNodeKind::Call {
                target: Some(mapped_declaration),
                is_method: false,
            }
        );
        assert_eq!(
            destination.node(mapped_lambda).expect("lambda").kind,
            CpgNodeKind::Lambda {
                captures: smallvec![mapped_declaration, mapped_identifier],
            }
        );
        assert!(destination.cfg_entries().contains(&mapped_declaration));
        assert!(destination.cfg_exits().contains(&mapped_call));
        assert!(
            destination.edges_between(mapped_identifier, mapped_declaration)[0]
                .label
                .as_deref()
                .is_some_and(|label| label == "fixture")
        );
        let mapped_nodes: Vec<_> = map.values().copied().collect();
        assert!(all_references_resolve(&destination, &mapped_nodes));
    }

    #[test]
    fn merge_clears_source_dangling_references_instead_of_binding_collisions() {
        let absent = NodeId::new(777);
        let mut source = CodePropertyGraph::new(Language::Rust);
        let call = source.add_node(
            node(
                0,
                CpgNodeKind::Call {
                    target: Some(absent),
                    is_method: false,
                },
            )
            .with_parent(absent)
            .with_child(absent),
        );
        let identifier = source.add_node(node(
            0,
            CpgNodeKind::Identifier {
                name: "missing".into(),
                definition: Some(absent),
            },
        ));
        let lambda = source.add_node(node(
            0,
            CpgNodeKind::Lambda {
                captures: smallvec![absent],
            },
        ));

        let mut destination = CodePropertyGraph::new(Language::Rust);
        for _ in 0..=absent.as_u32() {
            destination.add_node(node(0, CpgNodeKind::Return));
        }
        assert!(destination.contains_node(absent));
        let map = destination.merge_from(&source);

        let mapped_call = destination.node(map[&call]).expect("call");
        assert!(mapped_call.parent.is_none());
        assert!(mapped_call.children.is_empty());
        assert!(matches!(
            mapped_call.kind,
            CpgNodeKind::Call { target: None, .. }
        ));
        assert!(matches!(
            destination.node(map[&identifier]).expect("identifier").kind,
            CpgNodeKind::Identifier {
                definition: None,
                ..
            }
        ));
        assert!(matches!(
            &destination.node(map[&lambda]).expect("lambda").kind,
            CpgNodeKind::Lambda { captures } if captures.is_empty()
        ));
    }

    #[test]
    fn merging_an_equal_snapshot_doubles_counts_without_mutating_the_snapshot() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let root = cpg.add_node(node(0, CpgNodeKind::Root));
        let child = cpg.add_node(node(0, CpgNodeKind::Return).with_parent(root));
        cpg.connect(root, child, CpgEdgeKind::AstChild);
        let snapshot = cpg.clone();
        let source_shape = shape(&snapshot);
        let node_count = cpg.node_count();
        let edge_count = cpg.edge_count();

        let map = cpg.merge_from(&snapshot);

        assert_eq!(cpg.node_count(), node_count * 2);
        assert_eq!(cpg.edge_count(), edge_count * 2);
        assert_eq!(map.len(), node_count);
        assert_eq!(shape(&snapshot), source_shape);
    }
}

/// Statistics about a Code Property Graph.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CpgStats {
    /// Total number of nodes.
    pub node_count: usize,
    /// Total number of edges.
    pub edge_count: usize,
    /// Number of AST edges.
    pub ast_edges: usize,
    /// Number of CFG edges.
    pub cfg_edges: usize,
    /// Number of DFG edges.
    pub dfg_edges: usize,
    /// Number of abstract-heap edges.
    pub heap_edges: usize,
    /// Number of reflective-name provenance edges.
    #[cfg_attr(feature = "serde", serde(default))]
    pub name_flow_edges: usize,
    /// Number of call graph edges.
    pub call_edges: usize,
    /// Number of functions.
    pub function_count: usize,
    /// Number of classes.
    pub class_count: usize,
    /// Cyclomatic complexity.
    pub cyclomatic_complexity: usize,
    /// Number of term-rewriting dependency and conflict edges.
    ///
    /// Appended to preserve the serialized order of existing statistics.
    #[cfg_attr(feature = "serde", serde(default))]
    pub rewrite_dep_edges: usize,
    /// Number of ownership and place-capability edges.
    ///
    /// Appended to preserve the serialized order of existing statistics.
    #[cfg_attr(feature = "serde", serde(default))]
    pub place_capability_edges: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capacity_constructor_reserves_graph_and_node_identifier_map() {
        let cpg = CodePropertyGraph::with_capacity(Language::Rust, 17, 23);
        let (node_capacity, edge_capacity) = cpg.graph.capacity();

        assert!(node_capacity >= 17, "petgraph node arena capacity");
        assert!(edge_capacity >= 23, "petgraph edge arena capacity");
        assert!(cpg.node_index_map.capacity() >= 17, "node id-map capacity");
        assert_eq!(cpg.node_count(), 0);
        assert_eq!(cpg.edge_count(), 0);
    }

    #[test]
    fn dense_node_view_sorts_ids_and_counts_distinct_directed_neighbors() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        for id in [30, 10, 20] {
            cpg.add_node_with_id(CpgNode::new(
                NodeId::new(id),
                CpgNodeKind::If,
                SourceRange::default(),
            ));
        }
        let a = NodeId::new(10);
        let b = NodeId::new(20);
        cpg.connect(a, a, CpgEdgeKind::AstChild);
        cpg.connect(a, b, CpgEdgeKind::AstChild);
        cpg.connect(a, b, CpgEdgeKind::ControlDependence);
        cpg.connect(b, a, CpgEdgeKind::DataDependence);

        let dense = cpg.dense_node_view();
        assert_eq!(
            dense.ids,
            [NodeId::new(10), NodeId::new(20), NodeId::new(30)]
        );
        assert_eq!(dense.unique_out_degree, [2, 1, 0]);
        assert_eq!(dense.unique_in_degree, [2, 1, 0]);
    }

    #[test]
    fn maximum_stable_identifiers_are_representable_and_exhaustion_is_explicit() {
        let mut nodes = CodePropertyGraph::new(Language::Rust);
        let maximum = nodes.add_node_with_id(CpgNode::new(
            NodeId::new(u32::MAX),
            CpgNodeKind::Root,
            SourceRange::default(),
        ));
        assert_eq!(maximum, NodeId::new(u32::MAX));
        assert_eq!(nodes.outgoing_edges(maximum).len(), 0);
        assert!(nodes.projection_cache.get().is_some());

        let node_exhaustion = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            nodes.add_node(CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::Return,
                SourceRange::default(),
            ));
        }));
        assert!(node_exhaustion.is_err());
        assert_eq!(nodes.node_count(), 1);
        assert!(nodes.projection_cache.get().is_some());

        let mut edges = CodePropertyGraph::new(Language::Rust);
        let source = edges.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Root,
            SourceRange::default(),
        ));
        let target = edges.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Return,
            SourceRange::default(),
        ));
        let maximum = edges
            .add_edge_with_id(CpgEdge::new(
                EdgeId::new(u32::MAX),
                source,
                target,
                CpgEdgeKind::AstChild,
            ))
            .expect("maximum edge id is representable");
        assert_eq!(maximum, EdgeId::new(u32::MAX));
        assert_eq!(edges.outgoing_edges(source).len(), 1);
        assert!(edges.projection_cache.get().is_some());

        let edge_exhaustion = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            edges.connect(
                source,
                target,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
            );
        }));
        assert!(edge_exhaustion.is_err());
        assert_eq!(edges.edge_count(), 1);
        assert!(edges.projection_cache.get().is_some());
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn graph_layout_excludes_the_unread_edge_identifier_map() {
        let graph_bytes = std::mem::size_of::<CodePropertyGraph>();
        assert!(
            graph_bytes <= 200,
            "CodePropertyGraph grew to {graph_bytes} bytes"
        );
    }

    #[test]
    fn topology_mutation_invalidates_the_lazy_projection_cache() {
        let mut graph = CodePropertyGraph::new(Language::Rust);
        let first = graph.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Root,
            SourceRange::default(),
        ));
        let second = graph.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Return,
            SourceRange::default(),
        ));

        let debug_before_cache = format!("{graph:?}");
        assert!(graph.edges_between(first, second).is_empty());
        assert_eq!(
            format!("{graph:?}"),
            debug_before_cache,
            "derived cache state is not part of the logical graph snapshot"
        );
        graph.connect(first, second, CpgEdgeKind::AstChild);
        assert_eq!(graph.edges_between(first, second).len(), 1);
        assert_eq!(graph.ast_children(first), &[second]);

        graph.connect(
            first,
            second,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        );
        assert_eq!(graph.edges_between(first, second).len(), 2);
        assert_eq!(
            graph.cfg_successors(first).to_vec(),
            vec![(second, CfgEdgeKind::Sequential)]
        );

        assert!(graph
            .add_edge(CpgEdge::new(
                EdgeId::new(0),
                first,
                NodeId::new(u32::MAX),
                CpgEdgeKind::AstChild,
            ))
            .is_none());
        assert!(
            graph.projection_cache.get().is_some(),
            "a rejected non-mutation preserves the valid cache"
        );
    }
    use crate::graph::{MethodSignature, Visibility};

    #[test]
    fn test_create_cpg() {
        let cpg = CodePropertyGraph::new(Language::Rust);
        assert_eq!(cpg.node_count(), 0);
        assert_eq!(cpg.edge_count(), 0);
        assert_eq!(cpg.language(), Language::Rust);
    }

    #[test]
    fn test_add_nodes() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);

        let root = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Root,
            SourceRange::default(),
        ));

        let func = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Function {
                signature: Box::new(MethodSignature {
                    name: "main".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Public,
                }),
            },
            SourceRange::default(),
        ));

        assert_eq!(cpg.node_count(), 2);
        assert!(cpg.contains_node(root));
        assert!(cpg.contains_node(func));
    }

    #[test]
    fn test_add_edges() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);

        let n1 = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Root,
            SourceRange::default(),
        ));
        let n2 = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Root,
            SourceRange::default(),
        ));

        let edge_id = cpg.connect(n1, n2, CpgEdgeKind::AstChild);
        assert!(edge_id.is_some());
        assert_eq!(cpg.edge_count(), 1);
    }

    #[test]
    fn test_cfg_traversal() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);

        let entry = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::If,
            SourceRange::default(),
        ));
        let then_branch = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Block {
                scope: super::super::ScopeId::GLOBAL,
            },
            SourceRange::default(),
        ));
        let else_branch = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Block {
                scope: super::super::ScopeId::GLOBAL,
            },
            SourceRange::default(),
        ));

        cpg.connect(
            entry,
            then_branch,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue),
        );
        cpg.connect(
            entry,
            else_branch,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalFalse),
        );

        let successors = cpg.cfg_successors(entry);
        assert_eq!(successors.len(), 2);
    }
}

#[cfg(test)]
mod ext_tests {
    use super::*;
    use crate::graph::{MethodSignature, ScopeId, Visibility};
    use crate::CfgExtractor;

    fn mk(kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(0), kind, SourceRange::default())
    }

    fn mk_r(kind: CpgNodeKind, start: u32, end: u32) -> CpgNode {
        CpgNode::new(NodeId::new(0), kind, SourceRange::from_bytes(start, end))
    }

    fn func(name: &str) -> CpgNodeKind {
        CpgNodeKind::Function {
            signature: Box::new(MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Public,
            }),
        }
    }

    fn block() -> CpgNodeKind {
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        }
    }

    /// Wires an AST child edge (edges alone suffice for `ast_children`).
    fn ast_edge(g: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
        g.connect(parent, child, CpgEdgeKind::AstChild);
    }

    #[test]
    fn cyclomatic_no_cfg_is_one() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let a = g.add_node(mk(CpgNodeKind::Root));
        let b = g.add_node(mk(CpgNodeKind::Return));
        ast_edge(&mut g, a, b); // only AST, no CFG edges
        assert_eq!(g.cyclomatic_complexity(), 1);

        let empty = CodePropertyGraph::new(Language::Rust);
        assert_eq!(empty.cyclomatic_complexity(), 1);
    }

    #[test]
    fn cyclomatic_diamond_is_two() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let entry = g.add_node(mk(CpgNodeKind::If));
        let then_b = g.add_node(mk(block()));
        let else_b = g.add_node(mk(block()));
        let merge = g.add_node(mk(block()));
        g.connect(
            entry,
            then_b,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue),
        );
        g.connect(
            entry,
            else_b,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalFalse),
        );
        g.connect(
            then_b,
            merge,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        );
        g.connect(
            else_b,
            merge,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        );
        // 4 CFG edges, 4 CFG nodes -> 4 - 4 + 2 = 2.
        assert_eq!(g.cyclomatic_complexity(), 2);
    }

    #[test]
    fn cyclomatic_dense_uses_documented_formula() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let a = g.add_node(mk(block()));
        let b = g.add_node(mk(block()));
        let c = g.add_node(mk(block()));
        // 5 CFG edges across 3 CFG nodes -> 5 - 3 + 2 = 4.
        g.connect(a, b, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
        g.connect(b, c, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
        g.connect(c, a, CpgEdgeKind::ControlFlow(CfgEdgeKind::LoopBack));
        g.connect(a, c, CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue));
        g.connect(
            b,
            a,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalFalse),
        );
        assert_eq!(g.cyclomatic_complexity(), 4);
    }

    #[test]
    fn stats_all_fields() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let f = g.add_node(mk(func("f"))); // Function (also the root)
        let blk = g.add_node(mk(block()));
        let cls = g.add_node(mk(CpgNodeKind::Class {
            name: "C".into(),
            is_abstract: false,
        }));
        let call = g.add_node(mk(CpgNodeKind::Call {
            target: None,
            is_method: false,
        }));
        let var = g.add_node(mk(CpgNodeKind::Variable {
            name: "v".into(),
            var_type: None,
            scope: ScopeId::GLOBAL,
            is_mutable: false,
        }));
        let idn = g.add_node(mk(CpgNodeKind::Identifier {
            name: "x".into(),
            definition: None,
        }));

        // 4 AST edges.
        g.connect(f, blk, CpgEdgeKind::AstChild);
        g.connect(f, cls, CpgEdgeKind::AstChild);
        g.connect(blk, call, CpgEdgeKind::AstChild);
        g.connect(blk, var, CpgEdgeKind::AstChild);
        // 1 CFG edge (f, blk are the only CFG nodes).
        g.connect(f, blk, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
        // 1 DFG edge.
        g.connect(var, idn, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse));
        // 1 call edge.
        g.connect(call, f, CpgEdgeKind::CallSite);

        let s = g.stats();
        assert_eq!(s.node_count, 6);
        assert_eq!(s.edge_count, 7);
        assert_eq!(s.ast_edges, 4);
        assert_eq!(s.cfg_edges, 1);
        assert_eq!(s.dfg_edges, 1);
        assert_eq!(s.call_edges, 1);
        assert_eq!(s.function_count, 1);
        assert_eq!(s.class_count, 1);
        assert_eq!(s.cyclomatic_complexity, g.cyclomatic_complexity());
        // cfg_nodes = {f, blk} -> 1.saturating_sub(2) + 2 = 2.
        assert_eq!(s.cyclomatic_complexity, 2);
    }

    #[test]
    fn subgraph_full_preserves_everything() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let a = g.add_node(mk(CpgNodeKind::Root));
        let b = g.add_node(mk(CpgNodeKind::Return));
        let c = g.add_node(mk(CpgNodeKind::Break));
        g.connect(a, b, CpgEdgeKind::AstChild);
        g.connect(a, c, CpgEdgeKind::AstChild);

        let ids: Vec<NodeId> = g.node_ids().collect();
        let sub = g.subgraph(&ids);
        assert_eq!(sub.node_count(), g.node_count());
        assert_eq!(sub.edge_count(), g.edge_count());
        for id in g.node_ids() {
            assert_eq!(
                sub.node(id).map(|n| n.kind.clone()),
                g.node(id).map(|n| n.kind.clone())
            );
        }
    }

    #[test]
    fn subgraph_subset_keeps_internal_edges_only() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let a = g.add_node(mk(CpgNodeKind::Root));
        let b = g.add_node(mk(CpgNodeKind::Return));
        let c = g.add_node(mk(CpgNodeKind::Break));
        g.connect(a, b, CpgEdgeKind::AstChild); // internal to {a, b}
        g.connect(b, c, CpgEdgeKind::AstChild); // leaves {a, b}

        let sub = g.subgraph(&[a, b]);
        assert_eq!(sub.node_count(), 2);
        assert_eq!(sub.edge_count(), 1); // only a -> b survives
        assert!(sub.contains_node(a));
        assert!(sub.contains_node(b));
        assert!(!sub.contains_node(c));
    }

    #[test]
    fn function_cfg_keeps_control_and_expression_nodes() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let f = g.add_node(mk(func("f")));
        let blk = g.add_node(mk(block()));
        let iff = g.add_node(mk(CpgNodeKind::If));
        let idn = g.add_node(mk(CpgNodeKind::Identifier {
            name: "x".into(),
            definition: None,
        }));
        let var = g.add_node(mk(CpgNodeKind::Variable {
            name: "v".into(),
            var_type: None,
            scope: ScopeId::GLOBAL,
            is_mutable: false,
        }));
        let call = g.add_node(mk(CpgNodeKind::Call {
            target: None,
            is_method: false,
        }));
        ast_edge(&mut g, f, blk);
        ast_edge(&mut g, blk, iff);
        ast_edge(&mut g, iff, idn);
        ast_edge(&mut g, blk, var);
        ast_edge(&mut g, blk, call);

        let cfg = g.function_cfg(f);
        // Kept: f (always pushed), iff (control flow), idn + call (expressions).
        assert!(cfg.contains_node(f));
        assert!(cfg.contains_node(iff));
        assert!(cfg.contains_node(idn));
        assert!(cfg.contains_node(call));
        // Dropped: blk (neither) and var (a declaration).
        assert!(!cfg.contains_node(blk));
        assert!(!cfg.contains_node(var));
        assert_eq!(cfg.node_count(), 4);
        // The only internal AST edge among the kept nodes is iff -> idn.
        assert_eq!(cfg.edge_count(), 1);
    }

    #[test]
    fn function_dfg_keeps_only_dataflow_nodes() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let f = g.add_node(mk(func("f")));
        let blk = g.add_node(mk(block()));
        let def = g.add_node(mk(CpgNodeKind::Variable {
            name: "v".into(),
            var_type: None,
            scope: ScopeId::GLOBAL,
            is_mutable: false,
        }));
        let use_site = g.add_node(mk(CpgNodeKind::Identifier {
            name: "v".into(),
            definition: None,
        }));
        let other = g.add_node(mk(CpgNodeKind::Return));
        ast_edge(&mut g, f, blk);
        ast_edge(&mut g, blk, def);
        ast_edge(&mut g, blk, use_site);
        ast_edge(&mut g, blk, other);
        g.connect(def, use_site, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse));

        let dfg = g.function_dfg(f);
        assert!(dfg.contains_node(def));
        assert!(dfg.contains_node(use_site));
        assert!(!dfg.contains_node(other)); // no DFG edge
        assert!(!dfg.contains_node(blk)); // no DFG edge
        assert!(!dfg.contains_node(f)); // the function is not among its own descendants
        assert_eq!(dfg.node_count(), 2);
        assert_eq!(dfg.edge_count(), 1);
    }

    #[test]
    fn ast_depth_linear_and_empty() {
        let empty = CodePropertyGraph::new(Language::Rust);
        assert_eq!(empty.ast_depth(), 0);

        let mut g = CodePropertyGraph::new(Language::Rust);
        let a = g.add_node(mk(CpgNodeKind::Root));
        assert_eq!(g.ast_depth(), 1); // a single node
        let b = g.add_node(mk(block()));
        let c = g.add_node(mk(CpgNodeKind::Return));
        ast_edge(&mut g, a, b);
        ast_edge(&mut g, b, c);
        assert_eq!(g.ast_depth(), 3); // a -> b -> c
    }

    #[test]
    fn ast_depth_terminates_on_cycle() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let a = g.add_node(mk(CpgNodeKind::Root)); // root = a (first node)
        let b = g.add_node(mk(block()));
        ast_edge(&mut g, a, b);
        ast_edge(&mut g, b, a); // cycle a -> b -> a
        ast_edge(&mut g, a, a); // self-loop
                                // The visited-set guard makes this terminate; the value stays bounded.
        let d = g.ast_depth();
        assert!(d >= 1);
        assert!(d <= g.node_count());
    }

    #[test]
    fn ast_depth_crosses_one_hundred_thousand_nodes_on_a_small_stack() {
        const DEPTH: usize = 100_000;
        let mut graph = CodePropertyGraph::new(Language::Rust);
        let mut parent = graph.add_node(mk(CpgNodeKind::Root));
        for _ in 1..DEPTH {
            let child = graph.add_node(mk(CpgNodeKind::Return));
            ast_edge(&mut graph, parent, child);
            parent = child;
        }
        let graph = std::sync::Arc::new(graph);
        let worker_graph = std::sync::Arc::clone(&graph);

        let depth = std::thread::Builder::new()
            .name("ast-depth-small-stack".into())
            .stack_size(256 * 1024)
            .spawn(move || worker_graph.ast_depth())
            .expect("spawn AST-depth small-stack worker")
            .join()
            .expect("AST-depth small-stack worker completes");

        assert_eq!(depth, DEPTH);
    }

    #[test]
    fn projection_cache_build_and_walk_cross_one_hundred_thousand_nodes_on_a_small_stack() {
        const DEPTH: usize = 100_000;
        let mut graph = CodePropertyGraph::with_capacity(Language::Rust, DEPTH, (DEPTH - 1) * 2);
        let first = graph.add_node(mk(CpgNodeKind::Root));
        let mut previous = first;
        for _ in 1..DEPTH {
            let next = graph.add_node(mk(CpgNodeKind::Return));
            graph.connect(
                previous,
                next,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
            );
            graph.connect(first, next, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse));
            previous = next;
        }
        let graph = std::sync::Arc::new(graph);

        let worker_graph = std::sync::Arc::clone(&graph);
        let last = std::thread::Builder::new()
            .name("projection-cache-small-stack".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                assert_eq!(worker_graph.dfg_successors(first).count(), DEPTH - 1);
                let mut current = first;
                for _ in 1..DEPTH {
                    let mut successors = worker_graph.cfg_successors(current);
                    let (next, kind) = successors.next().expect("linear CFG successor");
                    assert_eq!(kind, CfgEdgeKind::Sequential);
                    assert!(successors.next().is_none());
                    assert_eq!(
                        worker_graph.edges_between(current, next).len(),
                        if current == first { 2 } else { 1 },
                    );
                    current = next;
                }
                current
            })
            .expect("spawn projection-cache small-stack worker")
            .join()
            .expect("projection-cache small-stack worker completes");

        assert_eq!(last, previous);
    }

    #[test]
    fn node_at_offset_picks_smallest_enclosing() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let big = g.add_node(mk_r(CpgNodeKind::Root, 0, 100));
        let mid = g.add_node(mk_r(block(), 10, 20));
        let small = g.add_node(mk_r(block(), 50, 60));
        assert_eq!(g.node_at_offset(15).map(|n| n.id), Some(mid));
        assert_eq!(g.node_at_offset(55).map(|n| n.id), Some(small));
        assert_eq!(g.node_at_offset(5).map(|n| n.id), Some(big));
        assert!(g.node_at_offset(200).is_none());
    }

    #[test]
    fn nodes_in_range_returns_overlaps() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let a = g.add_node(mk_r(CpgNodeKind::Root, 0, 100));
        let b = g.add_node(mk_r(CpgNodeKind::Return, 10, 20));
        let c = g.add_node(mk_r(CpgNodeKind::Break, 50, 60));

        // [21, 49) overlaps only the enclosing `a`.
        let hits: Vec<NodeId> = g
            .nodes_in_range(SourceRange::from_bytes(21, 49))
            .into_iter()
            .map(|n| n.id)
            .collect();
        assert_eq!(hits, vec![a]);

        // [0, 100) overlaps all three.
        let mut all: Vec<NodeId> = g
            .nodes_in_range(SourceRange::from_bytes(0, 100))
            .into_iter()
            .map(|n| n.id)
            .collect();
        all.sort_by_key(|id| id.0);
        assert_eq!(all, vec![a, b, c]);
    }

    #[test]
    fn scope_at_offset_only_blocks_and_functions() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let f = g.add_node(mk_r(func("f"), 0, 100));
        let blk = g.add_node(mk_r(block(), 10, 50));
        let _idn = g.add_node(mk_r(
            CpgNodeKind::Identifier {
                name: "x".into(),
                definition: None,
            },
            20,
            30,
        ));
        // At 25 the innermost *scope* is the block (the identifier is not a scope).
        assert_eq!(g.scope_at_offset(25).map(|n| n.id), Some(blk));
        // At 5 only the function encloses.
        assert_eq!(g.scope_at_offset(5).map(|n| n.id), Some(f));
        assert!(g.scope_at_offset(200).is_none());
    }

    #[test]
    fn edges_by_kind_filters() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let a = g.add_node(mk(CpgNodeKind::Root));
        let b = g.add_node(mk(CpgNodeKind::Return));
        g.connect(a, b, CpgEdgeKind::AstChild);
        g.connect(a, b, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
        g.connect(a, b, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse));
        assert_eq!(g.edges_by_kind(|k| k.is_ast()).count(), 1);
        assert_eq!(g.edges_by_kind(|k| k.is_cfg()).count(), 1);
        assert_eq!(g.edges_by_kind(|k| k.is_dfg()).count(), 1);
        assert_eq!(g.edges_by_kind(|k| k.is_call()).count(), 0);
    }

    #[test]
    fn call_graph_queries() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let f = g.add_node(mk(func("f")));
        let blk = g.add_node(mk(block()));
        let call = g.add_node(mk(CpgNodeKind::Call {
            target: None,
            is_method: false,
        }));
        let g2 = g.add_node(mk(func("g")));
        ast_edge(&mut g, f, blk);
        ast_edge(&mut g, blk, call);
        g.connect(call, g2, CpgEdgeKind::CallSite);

        assert_eq!(g.call_sites(f), vec![call]);
        assert_eq!(g.callees(call), vec![g2]);
        assert_eq!(g.callers(g2), vec![call]);
        assert!(g.callers(f).is_empty());
    }

    #[test]
    fn query_by_kind() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        g.add_node(mk(CpgNodeKind::Class {
            name: "C".into(),
            is_abstract: false,
        }));
        g.add_node(mk(CpgNodeKind::Class {
            name: "D".into(),
            is_abstract: true,
        }));
        g.add_node(mk(CpgNodeKind::Variable {
            name: "v".into(),
            var_type: None,
            scope: ScopeId::GLOBAL,
            is_mutable: false,
        }));
        g.add_node(mk(CpgNodeKind::Call {
            target: None,
            is_method: false,
        }));
        g.add_node(mk(func("f")));
        assert_eq!(g.classes().count(), 2);
        assert_eq!(g.variables().count(), 1);
        assert_eq!(g.calls().count(), 1);
        assert_eq!(g.functions().count(), 1);
        assert_eq!(
            g.nodes_by_kind(|k| matches!(k, CpgNodeKind::Class { .. }))
                .count(),
            2
        );
        assert_eq!(
            g.nodes_by_kind(|k| matches!(k, CpgNodeKind::Return))
                .count(),
            0
        );
    }

    #[test]
    fn add_node_with_id_advances_counter() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let id5 = g.add_node_with_id(CpgNode::new(
            NodeId::new(5),
            CpgNodeKind::Root,
            SourceRange::default(),
        ));
        assert_eq!(id5, NodeId::new(5));
        assert!(g.contains_node(NodeId::new(5)));
        assert_eq!(g.root(), Some(NodeId::new(5))); // first node becomes the root
                                                    // The auto-assigned id must be strictly greater than the reserved id.
        let auto = g.add_node(mk(CpgNodeKind::Return));
        assert_eq!(auto, NodeId::new(6));
    }

    #[test]
    fn add_edge_with_id_advances_counter() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let a = g.add_node(mk(CpgNodeKind::Root));
        let b = g.add_node(mk(CpgNodeKind::Return));
        let e10 = g.add_edge_with_id(CpgEdge::new(EdgeId::new(10), a, b, CpgEdgeKind::AstChild));
        assert_eq!(e10, Some(EdgeId::new(10)));
        assert_eq!(g.edge_count(), 1);
        let auto = g.connect(a, b, CpgEdgeKind::AstNextSibling);
        assert_eq!(auto, Some(EdgeId::new(11)));
        // An edge referencing a missing endpoint is rejected.
        let bad = g.add_edge_with_id(CpgEdge::new(
            EdgeId::new(20),
            a,
            NodeId::new(999),
            CpgEdgeKind::AstChild,
        ));
        assert_eq!(bad, None);
    }

    #[test]
    fn explicit_ast_edge_ids_define_borrowed_child_order() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let root = g.add_node(mk(CpgNodeKind::Root));
        let first = g.add_node(mk(CpgNodeKind::Return));
        let second = g.add_node(mk(CpgNodeKind::Break));
        let third = g.add_node(mk(CpgNodeKind::Continue));

        g.add_edge_with_id(CpgEdge::new(
            EdgeId::new(30),
            root,
            third,
            CpgEdgeKind::AstChild,
        ));
        g.add_edge_with_id(CpgEdge::new(
            EdgeId::new(10),
            root,
            first,
            CpgEdgeKind::AstChild,
        ));
        g.add_edge_with_id(CpgEdge::new(
            EdgeId::new(20),
            root,
            second,
            CpgEdgeKind::AstChild,
        ));

        assert_eq!(g.ast_children(root), [first, second, third]);
        assert_eq!(g.ast_parent(first), Some(root));
        assert_eq!(g.ast_children(NodeId::new(9_999)), &[]);
    }

    #[test]
    fn edges_between_returns_all_kinds() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let a = g.add_node(mk(CpgNodeKind::Root));
        let b = g.add_node(mk(CpgNodeKind::Return));
        g.connect(a, b, CpgEdgeKind::AstChild);
        g.connect(a, b, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
        assert_eq!(g.edges_between(a, b).len(), 2);
        // Edges are directed: nothing runs b -> a.
        assert!(g.edges_between(b, a).is_empty());
        // A missing endpoint yields no edges.
        assert!(g.edges_between(a, NodeId::new(999)).is_empty());
    }

    #[test]
    fn connect_unique_is_idempotent() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let a = g.add_node(mk(CpgNodeKind::Root));
        let b = g.add_node(mk(CpgNodeKind::Return));
        let e1 = g.connect_unique(a, b, CpgEdgeKind::AstChild);
        let e2 = g.connect_unique(a, b, CpgEdgeKind::AstChild);
        assert_eq!(e1, e2); // the same edge id is returned
        assert_eq!(g.edge_count(), 1); // and no duplicate is created
                                       // A different kind between the same endpoints IS added.
        g.connect_unique(a, b, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
        assert_eq!(g.edge_count(), 2);
        // A missing endpoint returns None and changes nothing.
        assert_eq!(
            g.connect_unique(a, NodeId::new(999), CpgEdgeKind::AstChild),
            None
        );
        assert_eq!(g.edge_count(), 2);
    }

    #[test]
    fn cfg_extractor_extract_is_idempotent() {
        // A minimal well-formed function: f -> block -> return.
        let mut g = CodePropertyGraph::new(Language::Rust);
        let f = g.add_node(mk(func("f")));
        let blk = g.add_node(mk(block()));
        let ret = g.add_node(mk(CpgNodeKind::Return));
        ast_edge(&mut g, f, blk);
        ast_edge(&mut g, blk, ret);

        let extractor = CfgExtractor::new();
        extractor.extract(&mut g);
        let after_first = g.edge_count();
        extractor.extract(&mut g);
        let after_second = g.edge_count();
        assert_eq!(
            after_first, after_second,
            "CFG extraction routes through connect_unique and must be idempotent"
        );
    }

    #[test]
    fn clone_preserves_shape_and_is_independent() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let a = g.add_node(mk(func("f")));
        let b = g.add_node(mk(CpgNodeKind::Return));
        g.connect(a, b, CpgEdgeKind::AstChild);

        let c = g.clone();
        assert_eq!(c.node_count(), g.node_count());
        assert_eq!(c.edge_count(), g.edge_count());
        assert_eq!(c.language(), g.language());
        for id in g.node_ids() {
            assert_eq!(
                c.node(id).map(|n| n.kind.clone()),
                g.node(id).map(|n| n.kind.clone())
            );
        }

        // Mutating the clone leaves the original untouched.
        let mut c2 = c;
        c2.add_node(mk(CpgNodeKind::Break));
        assert_eq!(g.node_count(), 2);
        assert_eq!(c2.node_count(), 3);
    }

    #[test]
    fn default_is_empty_unknown() {
        let g = CodePropertyGraph::default();
        assert_eq!(g.node_count(), 0);
        assert_eq!(g.edge_count(), 0);
        assert_eq!(g.language(), Language::Unknown);
        assert!(g.root().is_none());
    }

    #[test]
    fn node_tag_is_total_for_present_nodes_and_absent_nodes_are_none() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let root = g.add_node(mk(CpgNodeKind::Root));
        let function = g.add_node(mk(func("f")));
        assert_eq!(g.node_tag(root), Some(CpgNodeKindTag::Root));
        assert_eq!(g.node_tag(function), Some(CpgNodeKindTag::Function));
        assert_eq!(g.node_tag(NodeId::new(999)), None);
    }
}

/// REGRESSION: `ast_ancestors` follows raw `parent` pointers. A hand-built
/// graph may point at a node that was never added, and the walk used to return
/// that id — handing the caller an "ancestor" that `node()` cannot resolve.
/// Found by the `tests/robustness.rs` corruption suite.
#[cfg(test)]
mod dangling_parent_regression {
    use super::*;
    use crate::{CpgNode, Language, ScopeId, SourceRange};

    #[test]
    fn ast_ancestors_stops_at_a_dangling_parent() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let root = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
            SourceRange::default(),
        ));
        let child = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Return,
            SourceRange::default(),
        ));
        cpg.connect(root, child, CpgEdgeKind::AstChild);
        // The root's parent points at a node that does not exist.
        let absent = NodeId::new(9_999);
        cpg.node_mut(root).expect("root").parent = Some(absent);
        assert!(cpg.node(absent).is_none(), "the id really is absent");

        let ancestors = cpg.ast_ancestors(child);
        assert_eq!(
            ancestors,
            vec![root],
            "the chain stops before the absent id"
        );
        for id in &ancestors {
            assert!(cpg.node(*id).is_some(), "every ancestor resolves");
        }
        // The raw accessor still reports the pointer as stored.
        assert_eq!(cpg.ast_parent(root), Some(absent));
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::testutil::*;
    use crate::CfgExtractor;
    use proptest::prelude::*;

    fn edge_sorted_ast_children(cpg: &CodePropertyGraph, node: NodeId) -> Vec<NodeId> {
        let mut edges: Vec<_> = cpg
            .outgoing_edges(node)
            .filter(|edge| edge.kind == CpgEdgeKind::AstChild)
            .map(|edge| (edge.id, edge.target))
            .collect();
        edges.sort_unstable_by_key(|(edge_id, _)| edge_id.0);
        edges.into_iter().map(|(_, target)| target).collect()
    }

    fn sorted_edge_ids<'a>(edges: impl IntoIterator<Item = &'a CpgEdge>) -> Vec<u32> {
        let mut ids: Vec<_> = edges.into_iter().map(|edge| edge.id.as_u32()).collect();
        ids.sort_unstable();
        ids
    }

    fn cfg_signature(cpg: &CodePropertyGraph, node: NodeId, outgoing: bool) -> Vec<String> {
        let mut signature: Vec<_> = cpg
            .edges()
            .filter_map(|edge| {
                let CpgEdgeKind::ControlFlow(kind) = edge.kind else {
                    return None;
                };
                let neighbor = if outgoing {
                    (edge.source == node).then_some(edge.target)
                } else {
                    (edge.target == node).then_some(edge.source)
                }?;
                Some(format!("{}:{kind:?}", neighbor.as_u32()))
            })
            .collect();
        signature.sort_unstable();
        signature
    }

    fn dfg_signature(cpg: &CodePropertyGraph, node: NodeId, outgoing: bool) -> Vec<String> {
        let mut signature: Vec<_> = cpg
            .edges()
            .filter_map(|edge| {
                let CpgEdgeKind::DataFlow(kind) = edge.kind else {
                    return None;
                };
                let neighbor = if outgoing {
                    (edge.source == node).then_some(edge.target)
                } else {
                    (edge.target == node).then_some(edge.source)
                }?;
                Some(format!("{}:{kind:?}", neighbor.as_u32()))
            })
            .collect();
        signature.sort_unstable();
        signature
    }

    fn cached_order(cpg: &CodePropertyGraph) -> Vec<(NodeId, Vec<u32>, Vec<u32>)> {
        let mut nodes: Vec<_> = cpg.node_ids().collect();
        nodes.sort_unstable();
        nodes
            .into_iter()
            .map(|node| {
                (
                    node,
                    cpg.outgoing_edges(node)
                        .map(|edge| edge.id.as_u32())
                        .collect(),
                    cpg.incoming_edges(node)
                        .map(|edge| edge.id.as_u32())
                        .collect(),
                )
            })
            .collect()
    }

    fn reverse_insertion(cpg: &CodePropertyGraph) -> CodePropertyGraph {
        let mut reversed = CodePropertyGraph::new(cpg.language());
        let mut nodes: Vec<_> = cpg.nodes().cloned().collect();
        nodes.reverse();
        for node in nodes {
            reversed.add_node_with_id(node);
        }
        let mut edges: Vec<_> = cpg.edges().cloned().collect();
        edges.reverse();
        for edge in edges {
            reversed
                .add_edge_with_id(edge)
                .expect("reversed edge endpoints exist");
        }
        reversed
    }

    fn recursive_ast_depth_oracle(
        cpg: &CodePropertyGraph,
        node: NodeId,
        on_path: &mut rustc_hash::FxHashSet<NodeId>,
    ) -> usize {
        if !on_path.insert(node) {
            return 0;
        }
        let children = cpg.ast_children(node);
        let depth = children
            .iter()
            .map(|&child| recursive_ast_depth_oracle(cpg, child, on_path))
            .max()
            .map_or(1, |maximum| 1usize.saturating_add(maximum));
        on_path.remove(&node);
        depth
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn prop_add_node_count(kinds in prop::collection::vec(arb_node_kind(), 0..=16)) {
            let mut g = CodePropertyGraph::new(Language::Rust);
            let n = kinds.len();
            for k in kinds {
                g.add_node(CpgNode::new(NodeId::new(0), k, SourceRange::default()));
            }
            prop_assert_eq!(g.node_count(), n);
            prop_assert_eq!(g.node_ids().count(), n);
            for id in g.node_ids() {
                prop_assert!(g.node(id).is_some());
            }
        }

        #[test]
        fn prop_node_ids_resolve(g in arb_cpg_raw()) {
            prop_assert_eq!(g.node_ids().count(), g.node_count());
            for id in g.node_ids() {
                prop_assert!(g.node(id).is_some());
            }
        }

        #[test]
        fn prop_ast_children_match_edge_sorted_oracle(g in arb_cpg_raw()) {
            for id in g.node_ids() {
                let expected = edge_sorted_ast_children(&g, id);
                prop_assert_eq!(g.ast_children(id), expected);
            }
        }

        #[test]
        fn prop_cached_projection_accessors_equal_direct_edge_filters(g in arb_cpg_raw()) {
            let mut nodes: Vec<_> = g.node_ids().collect();
            nodes.sort_unstable();

            let expected_ast = sorted_edge_ids(g.edges().filter(|edge| edge.kind.is_ast()));
            let expected_cfg = sorted_edge_ids(g.edges().filter(|edge| edge.kind.is_cfg()));
            let expected_dfg = sorted_edge_ids(g.edges().filter(|edge| edge.kind.is_dfg()));
            let expected_pdg = sorted_edge_ids(g.edges().filter(|edge| edge.kind.is_pdg()));
            let expected_call = sorted_edge_ids(g.edges().filter(|edge| edge.kind.is_call()));
            prop_assert_eq!(sorted_edge_ids(g.ast_overlay_edges()), expected_ast);
            prop_assert_eq!(sorted_edge_ids(g.cfg_overlay_edges()), expected_cfg);
            prop_assert_eq!(sorted_edge_ids(g.dfg_overlay_edges()), expected_dfg);
            prop_assert_eq!(sorted_edge_ids(g.pdg_overlay_edges()), expected_pdg);
            prop_assert_eq!(sorted_edge_ids(g.call_overlay_edges()), expected_call);

            for &source in &nodes {
                let expected_out = sorted_edge_ids(g.edges().filter(|edge| edge.source == source));
                let expected_in = sorted_edge_ids(g.edges().filter(|edge| edge.target == source));
                prop_assert_eq!(sorted_edge_ids(g.outgoing_edges(source)), expected_out);
                prop_assert_eq!(sorted_edge_ids(g.incoming_edges(source)), expected_in);

                let mut actual_cfg_out: Vec<_> = g
                    .cfg_successors(source)
                    .map(|(node, kind)| format!("{}:{kind:?}", node.as_u32()))
                    .collect();
                actual_cfg_out.sort_unstable();
                let mut actual_cfg_in: Vec<_> = g
                    .cfg_predecessors(source)
                    .map(|(node, kind)| format!("{}:{kind:?}", node.as_u32()))
                    .collect();
                actual_cfg_in.sort_unstable();
                let mut actual_dfg_out: Vec<_> = g
                    .dfg_successors(source)
                    .map(|(node, kind)| format!("{}:{kind:?}", node.as_u32()))
                    .collect();
                actual_dfg_out.sort_unstable();
                let mut actual_dfg_in: Vec<_> = g
                    .dfg_predecessors(source)
                    .map(|(node, kind)| format!("{}:{kind:?}", node.as_u32()))
                    .collect();
                actual_dfg_in.sort_unstable();
                prop_assert_eq!(actual_cfg_out, cfg_signature(&g, source, true));
                prop_assert_eq!(actual_cfg_in, cfg_signature(&g, source, false));
                prop_assert_eq!(actual_dfg_out, dfg_signature(&g, source, true));
                prop_assert_eq!(actual_dfg_in, dfg_signature(&g, source, false));

                for &target in &nodes {
                    let expected = sorted_edge_ids(
                        g.edges()
                            .filter(|edge| edge.source == source && edge.target == target),
                    );
                    prop_assert_eq!(sorted_edge_ids(g.edges_between(source, target)), expected);
                }
            }
        }

        #[test]
        fn prop_cached_projection_order_is_insertion_invariant(g in arb_cpg_raw()) {
            let reversed = reverse_insertion(&g);
            prop_assert_eq!(cached_order(&g), cached_order(&reversed));
        }

        #[test]
        fn prop_explicit_edge_reconstruction_is_insertion_order_invariant(
            insertion_keys in prop::collection::vec(any::<u32>(), 0..=24),
        ) {
            let mut g = CodePropertyGraph::new(Language::Rust);
            let root = g.add_node(CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::Root,
                SourceRange::default(),
            ));
            let children: Vec<_> = insertion_keys
                .iter()
                .map(|_| {
                    g.add_node(CpgNode::new(
                        NodeId::new(0),
                        CpgNodeKind::Return,
                        SourceRange::default(),
                    ))
                })
                .collect();
            let mut insertion_order: Vec<_> = (0..children.len()).collect();
            insertion_order.sort_unstable_by_key(|&index| (insertion_keys[index], index));

            for index in insertion_order {
                g.add_edge_with_id(CpgEdge::new(
                    EdgeId::new(u32::try_from(index).expect("bounded child index")),
                    root,
                    children[index],
                    CpgEdgeKind::AstChild,
                ));
            }

            prop_assert_eq!(g.ast_children(root), children);
            prop_assert_eq!(g.ast_children(root), edge_sorted_ast_children(&g, root));
        }

        #[test]
        fn prop_subgraph_full_is_isomorphic(g in arb_well_formed_cpg()) {
            let ids: Vec<NodeId> = g.node_ids().collect();
            let sub = g.subgraph(&ids);
            prop_assert_eq!(sub.node_count(), g.node_count());
            prop_assert_eq!(sub.edge_count(), g.edge_count());
            for id in g.node_ids() {
                prop_assert_eq!(
                    sub.node(id).map(|n| n.kind.clone()),
                    g.node(id).map(|n| n.kind.clone())
                );
            }
        }

        #[test]
        fn prop_ast_depth_bounds(g in arb_well_formed_cpg()) {
            let d = g.ast_depth();
            prop_assert!(d >= 1);
            prop_assert!(d <= g.node_count());
            let mut on_path = rustc_hash::FxHashSet::default();
            let expected = recursive_ast_depth_oracle(
                &g,
                g.root().expect("generated graph has a root"),
                &mut on_path,
            );
            prop_assert_eq!(d, expected);
        }

        #[test]
        fn prop_cyclomatic_matches_formula(g in arb_well_formed_cpg()) {
            let mut g = g;
            CfgExtractor::new().extract(&mut g);
            let cfg_edges = g.edges_by_kind(|k| k.is_cfg()).count();
            let cfg_nodes = g.cfg_nodes().count();
            let expected = if cfg_nodes == 0 {
                1
            } else {
                cfg_edges.saturating_sub(cfg_nodes) + 2
            };
            prop_assert_eq!(g.cyclomatic_complexity(), expected);
            prop_assert!(g.cyclomatic_complexity() >= 1);
        }

        #[test]
        fn prop_stats_consistency(g in arb_cpg_raw()) {
            let s = g.stats();
            prop_assert_eq!(s.node_count, g.node_count());
            prop_assert_eq!(s.edge_count, g.edge_count());
            prop_assert!(
                s.ast_edges + s.cfg_edges + s.dfg_edges + s.call_edges <= g.edge_count()
            );
            prop_assert_eq!(s.function_count, g.functions().count());
            prop_assert_eq!(s.class_count, g.classes().count());
            prop_assert_eq!(s.cyclomatic_complexity, g.cyclomatic_complexity());
        }

        #[test]
        fn prop_connect_unique_idempotent(g in arb_cpg_raw()) {
            let mut g = g;
            let ids: Vec<NodeId> = g.node_ids().collect();
            // `arb_cpg_raw` always produces at least one node.
            let s = ids[0];
            let t = ids[ids.len() - 1];
            g.connect_unique(s, t, CpgEdgeKind::TypeOf);
            let after_first = g.edge_count();
            g.connect_unique(s, t, CpgEdgeKind::TypeOf);
            prop_assert_eq!(g.edge_count(), after_first);
        }

        #[test]
        fn prop_merge_is_deterministic_append_only_and_reference_total(
            left in arb_well_formed_cpg(),
            right in arb_well_formed_cpg(),
        ) {
            let expected_nodes = left.node_count() + right.node_count();
            let expected_edges = left.edge_count() + right.edge_count();

            let mut first = CodePropertyGraph::new(Language::Rust);
            let left_map = first.merge_from(&left);
            let right_map = first.merge_from(&right);
            let mut second = CodePropertyGraph::new(Language::Rust);
            second.merge_from(&left);
            second.merge_from(&right);

            prop_assert_eq!(first.node_count(), expected_nodes);
            prop_assert_eq!(first.edge_count(), expected_edges);
            let mut ids: Vec<_> = first.node_ids().collect();
            ids.sort_unstable();
            let expected_ids: Vec<_> = (0..expected_nodes as u32).map(NodeId::new).collect();
            prop_assert_eq!(&ids, &expected_ids);
            let mut mapped: Vec<_> = left_map
                .values()
                .chain(right_map.values())
                .copied()
                .collect();
            mapped.sort_unstable();
            prop_assert_eq!(&mapped, &expected_ids);
            prop_assert!(crate::graph::cpg::merge_tests::all_references_resolve(
                &first,
                &mapped
            ));
            prop_assert_eq!(
                crate::graph::cpg::merge_tests::shape(&first),
                crate::graph::cpg::merge_tests::shape(&second)
            );
        }

        #[test]
        fn prop_merging_equal_snapshot_exactly_doubles_counts(
            mut graph in arb_well_formed_cpg(),
        ) {
            let snapshot = graph.clone();
            let nodes = graph.node_count();
            let edges = graph.edge_count();
            let map = graph.merge_from(&snapshot);
            prop_assert_eq!(graph.node_count(), nodes * 2);
            prop_assert_eq!(graph.edge_count(), edges * 2);
            prop_assert_eq!(map.len(), nodes);
            let mapped: Vec<_> = map.values().copied().collect();
            prop_assert!(crate::graph::cpg::merge_tests::all_references_resolve(
                &graph,
                &mapped
            ));
        }
    }
}

#[cfg(all(test, feature = "serde"))]
mod serde_tests {
    use super::*;
    use crate::testutil::*;
    use proptest::prelude::*;

    /// The full `arb_cpg_raw` strategy — no float exclusion. `serde_json` is
    /// built with the `float_roundtrip` feature, so finite float literals
    /// round-trip exactly through a whole-graph JSON serialization.
    fn arb_serdeable_cpg() -> impl Strategy<Value = CodePropertyGraph> {
        arb_cpg_raw()
    }

    /// Two graphs agree up to node ordering: same counts and same kind per id.
    fn same_shape(a: &CodePropertyGraph, b: &CodePropertyGraph) -> bool {
        if a.node_count() != b.node_count() || a.edge_count() != b.edge_count() {
            return false;
        }
        for id in a.node_ids() {
            match (a.node(id), b.node(id)) {
                (Some(na), Some(nb)) if na.kind == nb.kind => {}
                _ => return false,
            }
        }
        true
    }

    #[test]
    fn serde_stats_round_trip() {
        let stats = CpgStats {
            node_count: 5,
            edge_count: 7,
            ast_edges: 3,
            cfg_edges: 1,
            dfg_edges: 1,
            heap_edges: 1,
            name_flow_edges: 1,
            rewrite_dep_edges: 1,
            place_capability_edges: 1,
            call_edges: 1,
            function_count: 1,
            class_count: 1,
            cyclomatic_complexity: 2,
        };
        let s = serde_json::to_string(&stats).expect("serialize stats");
        let back: CpgStats = serde_json::from_str(&s).expect("deserialize stats");
        assert_eq!(stats.node_count, back.node_count);
        assert_eq!(stats.edge_count, back.edge_count);
        assert_eq!(stats.ast_edges, back.ast_edges);
        assert_eq!(stats.cfg_edges, back.cfg_edges);
        assert_eq!(stats.heap_edges, back.heap_edges);
        assert_eq!(stats.name_flow_edges, back.name_flow_edges);
        assert_eq!(stats.rewrite_dep_edges, back.rewrite_dep_edges);
        assert_eq!(stats.place_capability_edges, back.place_capability_edges);
        assert_eq!(stats.dfg_edges, back.dfg_edges);
        assert_eq!(stats.call_edges, back.call_edges);
        assert_eq!(stats.function_count, back.function_count);
        assert_eq!(stats.class_count, back.class_count);
        assert_eq!(stats.cyclomatic_complexity, back.cyclomatic_complexity);
    }

    #[test]
    fn serde_whole_graph_example() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let a = g.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Root,
            SourceRange::from_bytes(0, 10),
        ));
        let b = g.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Return,
            SourceRange::from_bytes(2, 4),
        ));
        g.connect(a, b, CpgEdgeKind::AstChild);
        g.connect(a, b, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));

        let s = serde_json::to_string(&g).expect("serialize graph");
        let back: CodePropertyGraph = serde_json::from_str(&s).expect("deserialize graph");
        assert_eq!(back.node_count(), 2);
        assert_eq!(back.edge_count(), 2);
        assert_eq!(back.language(), Language::Rust);
        assert!(same_shape(&g, &back));
    }

    #[test]
    fn serde_accepts_legacy_write_only_edge_map_field() {
        let mut graph = CodePropertyGraph::new(Language::Rust);
        let source = graph.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Root,
            SourceRange::default(),
        ));
        let target = graph.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Return,
            SourceRange::default(),
        ));
        graph.connect(source, target, CpgEdgeKind::AstChild);

        let mut json = serde_json::to_value(&graph).expect("serialize graph");
        let object = json.as_object_mut().expect("graph serializes as an object");
        assert!(!object.contains_key("edge_index_map"));
        assert!(!object.contains_key("projection_cache"));
        object.insert(
            "edge_index_map".to_owned(),
            serde_json::json!({"0": [0, 1]}),
        );

        let restored: CodePropertyGraph =
            serde_json::from_value(json).expect("deserialize legacy edge map field");
        assert!(same_shape(&graph, &restored));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn prop_whole_graph_round_trip(g in arb_serdeable_cpg()) {
            let s = serde_json::to_string(&g).expect("serialize graph");
            let back: CodePropertyGraph = serde_json::from_str(&s).expect("deserialize graph");
            prop_assert_eq!(g.node_count(), back.node_count());
            prop_assert_eq!(g.edge_count(), back.edge_count());
            prop_assert!(same_shape(&g, &back));
        }
    }
}
