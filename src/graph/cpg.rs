//! Code Property Graph implementation.
//!
//! The main graph data structure that combines AST, CFG, and DFG into a unified representation.

use std::sync::Arc;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::Direction;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "serde")]
use super::serde_util::option_arc_str;

use super::{
    CpgEdge, CpgEdgeKind, CpgNode, CpgNodeKind, CfgEdgeKind, DfgEdgeKind,
    EdgeId, Language, NodeId, SourceRange,
};

/// A Code Property Graph combining AST, CFG, and DFG.
///
/// This is the main data structure for program analysis, providing:
/// - Abstract Syntax Tree (AST) structure via parent/child relationships
/// - Control Flow Graph (CFG) via control flow edges
/// - Data Flow Graph (DFG) via def-use and use-def chains
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CodePropertyGraph {
    /// The underlying directed graph.
    graph: DiGraph<CpgNode, CpgEdge>,
    /// Map from NodeId to petgraph NodeIndex.
    node_index_map: FxHashMap<NodeId, NodeIndex>,
    /// Map from EdgeId to (source NodeIndex, target NodeIndex).
    edge_index_map: FxHashMap<EdgeId, (NodeIndex, NodeIndex)>,
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
}

impl CodePropertyGraph {
    /// Creates a new empty Code Property Graph.
    pub fn new(language: Language) -> Self {
        Self {
            graph: DiGraph::new(),
            node_index_map: FxHashMap::default(),
            edge_index_map: FxHashMap::default(),
            language,
            source_path: None,
            source_code: None,
            next_node_id: 0,
            next_edge_id: 0,
            root: None,
            cfg_entries: Vec::new(),
            cfg_exits: Vec::new(),
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
    pub fn add_node(&mut self, mut node: CpgNode) -> NodeId {
        let id = NodeId::new(self.next_node_id);
        self.next_node_id += 1;
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
        let id = node.id;
        self.next_node_id = self.next_node_id.max(id.0 + 1);

        let index = self.graph.add_node(node);
        self.node_index_map.insert(id, index);

        if self.root.is_none() {
            self.root = Some(id);
        }

        id
    }

    /// Returns a reference to a node by ID.
    pub fn node(&self, id: NodeId) -> Option<&CpgNode> {
        self.node_index_map
            .get(&id)
            .and_then(|&idx| self.graph.node_weight(idx))
    }

    /// Returns a mutable reference to a node by ID.
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut CpgNode> {
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

    // ========== Edge Operations ==========

    /// Adds an edge to the graph and returns its ID.
    pub fn add_edge(&mut self, mut edge: CpgEdge) -> Option<EdgeId> {
        let source_idx = self.node_index_map.get(&edge.source)?;
        let target_idx = self.node_index_map.get(&edge.target)?;

        let id = EdgeId::new(self.next_edge_id);
        self.next_edge_id += 1;
        edge.id = id;

        self.graph.add_edge(*source_idx, *target_idx, edge);
        self.edge_index_map.insert(id, (*source_idx, *target_idx));

        Some(id)
    }

    /// Adds an edge with a specific ID (for reconstruction from serialization).
    pub fn add_edge_with_id(&mut self, edge: CpgEdge) -> Option<EdgeId> {
        let source_idx = self.node_index_map.get(&edge.source)?;
        let target_idx = self.node_index_map.get(&edge.target)?;

        let id = edge.id;
        self.next_edge_id = self.next_edge_id.max(id.0 + 1);

        self.graph.add_edge(*source_idx, *target_idx, edge);
        self.edge_index_map.insert(id, (*source_idx, *target_idx));

        Some(id)
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
        let existing = self
            .edges_between(source, target)
            .into_iter()
            .find(|e| e.kind == kind)
            .map(|e| e.id);
        match existing {
            Some(id) => Some(id),
            None => self.connect(source, target, kind),
        }
    }

    /// Returns edges between two nodes.
    pub fn edges_between(&self, source: NodeId, target: NodeId) -> Vec<&CpgEdge> {
        let source_idx = match self.node_index_map.get(&source) {
            Some(&idx) => idx,
            None => return Vec::new(),
        };
        let target_idx = match self.node_index_map.get(&target) {
            Some(&idx) => idx,
            None => return Vec::new(),
        };

        self.graph
            .edges_connecting(source_idx, target_idx)
            .map(|e| e.weight())
            .collect()
    }

    /// Returns an iterator over all edges.
    pub fn edges(&self) -> impl Iterator<Item = &CpgEdge> {
        self.graph.edge_weights()
    }

    // ========== AST Traversal ==========

    /// Returns AST children of a node, in source order.
    ///
    /// petgraph iterates a node's outgoing edges newest-first (reverse
    /// insertion order), but AST child edges are inserted in source order and
    /// the CFG/DFG builders rely on that order (e.g. an `if`'s children as
    /// `[condition, then, else]`, an assignment's first child as its LHS).
    /// Sorting by edge id — assigned monotonically as edges are added — recovers
    /// the true source order for both parser-built and hand-built graphs.
    pub fn ast_children(&self, id: NodeId) -> Vec<NodeId> {
        let mut children: Vec<(EdgeId, NodeId)> = self
            .outgoing_edges(id)
            .filter(|e| matches!(e.kind, CpgEdgeKind::AstChild))
            .map(|e| (e.id, e.target))
            .collect();
        children.sort_unstable_by_key(|(edge_id, _)| edge_id.0);
        children.into_iter().map(|(_, target)| target).collect()
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
            for child in self.ast_children(current) {
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
    pub fn cfg_successors(&self, id: NodeId) -> Vec<(NodeId, CfgEdgeKind)> {
        self.outgoing_edges(id)
            .filter_map(|e| {
                if let CpgEdgeKind::ControlFlow(kind) = &e.kind {
                    Some((e.target, *kind))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Returns CFG predecessors of a node.
    pub fn cfg_predecessors(&self, id: NodeId) -> Vec<(NodeId, CfgEdgeKind)> {
        self.incoming_edges(id)
            .filter_map(|e| {
                if let CpgEdgeKind::ControlFlow(kind) = &e.kind {
                    Some((e.source, *kind))
                } else {
                    None
                }
            })
            .collect()
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
                if let CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse | DfgEdgeKind::ReachingDef) = &e.kind {
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
    pub fn dfg_successors(&self, id: NodeId) -> Vec<(NodeId, DfgEdgeKind)> {
        self.outgoing_edges(id)
            .filter_map(|e| {
                if let CpgEdgeKind::DataFlow(kind) = &e.kind {
                    Some((e.target, *kind))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Returns data flow predecessors of a node.
    pub fn dfg_predecessors(&self, id: NodeId) -> Vec<(NodeId, DfgEdgeKind)> {
        self.incoming_edges(id)
            .filter_map(|e| {
                if let CpgEdgeKind::DataFlow(kind) = &e.kind {
                    Some((e.source, *kind))
                } else {
                    None
                }
            })
            .collect()
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
        self.outgoing_edges(call_site)
            .filter_map(|e| {
                if matches!(e.kind, CpgEdgeKind::CallSite | CpgEdgeKind::StaticCall | CpgEdgeKind::DynamicCall) {
                    Some(e.target)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Returns callers of a function.
    pub fn callers(&self, function: NodeId) -> Vec<NodeId> {
        self.incoming_edges(function)
            .filter_map(|e| {
                if matches!(e.kind, CpgEdgeKind::CallSite | CpgEdgeKind::StaticCall | CpgEdgeKind::DynamicCall) {
                    Some(e.source)
                } else {
                    None
                }
            })
            .collect()
    }

    // ========== Query by Kind ==========

    /// Returns all function nodes.
    pub fn functions(&self) -> impl Iterator<Item = &CpgNode> {
        self.nodes().filter(|n| matches!(n.kind, CpgNodeKind::Function { .. }))
    }

    /// Returns all class nodes.
    pub fn classes(&self) -> impl Iterator<Item = &CpgNode> {
        self.nodes().filter(|n| matches!(n.kind, CpgNodeKind::Class { .. }))
    }

    /// Returns all variable declaration nodes.
    pub fn variables(&self) -> impl Iterator<Item = &CpgNode> {
        self.nodes().filter(|n| matches!(n.kind, CpgNodeKind::Variable { .. }))
    }

    /// Returns all call expression nodes.
    pub fn calls(&self) -> impl Iterator<Item = &CpgNode> {
        self.nodes().filter(|n| matches!(n.kind, CpgNodeKind::Call { .. }))
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
            .filter(|n| {
                n.range.start < range.end && n.range.end > range.start
            })
            .collect()
    }

    /// Returns the innermost scope at an offset.
    pub fn scope_at_offset(&self, offset: u32) -> Option<&CpgNode> {
        self.nodes()
            .filter(|n| {
                n.range.start <= offset && offset < n.range.end
                    && matches!(n.kind, CpgNodeKind::Block { .. } | CpgNodeKind::Function { .. })
            })
            .min_by_key(|n| n.range.len())
    }

    // ========== Edge Helpers ==========

    /// Returns outgoing edges from a node.
    pub fn outgoing_edges(&self, id: NodeId) -> impl Iterator<Item = &CpgEdge> {
        self.node_index_map
            .get(&id)
            .map(|&idx| {
                self.graph
                    .edges_directed(idx, Direction::Outgoing)
                    .map(|e| e.weight())
            })
            .into_iter()
            .flatten()
    }

    /// Returns incoming edges to a node.
    pub fn incoming_edges(&self, id: NodeId) -> impl Iterator<Item = &CpgEdge> {
        self.node_index_map
            .get(&id)
            .map(|&idx| {
                self.graph
                    .edges_directed(idx, Direction::Incoming)
                    .map(|e| e.weight())
            })
            .into_iter()
            .flatten()
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
    /// The recursion tracks the current root-to-node path in `visited` so a
    /// cyclic `AstChild` edge cannot cause unbounded recursion (stack overflow);
    /// a node already on the path is treated as a leaf. For a well-formed tree
    /// this is exactly the longest root-to-leaf path.
    pub fn ast_depth(&self) -> usize {
        let root = match self.root {
            Some(r) => r,
            None => return 0,
        };

        fn depth_from(
            cpg: &CodePropertyGraph,
            id: NodeId,
            visited: &mut rustc_hash::FxHashSet<NodeId>,
        ) -> usize {
            if !visited.insert(id) {
                return 0; // already on the current path: cycle, stop descending
            }
            let children = cpg.ast_children(id);
            let depth = if children.is_empty() {
                1
            } else {
                1 + children
                    .iter()
                    .map(|&c| depth_from(cpg, c, visited))
                    .max()
                    .unwrap_or(0)
            };
            visited.remove(&id);
            depth
        }

        let mut visited: rustc_hash::FxHashSet<NodeId> = rustc_hash::FxHashSet::default();
        depth_from(self, root, &mut visited)
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
        let ast_edges = self.edges().filter(|e| e.kind.is_ast()).count();
        let cfg_edges = self.edges().filter(|e| e.kind.is_cfg()).count();
        let dfg_edges = self.edges().filter(|e| e.kind.is_dfg()).count();
        let call_edges = self.edges().filter(|e| e.kind.is_call()).count();

        CpgStats {
            node_count: self.node_count(),
            edge_count: self.edge_count(),
            ast_edges,
            cfg_edges,
            dfg_edges,
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
                sub.add_node_with_id(node.clone());
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
            edge_index_map: self.edge_index_map.clone(),
            language: self.language,
            source_path: self.source_path.clone(),
            source_code: self.source_code.clone(),
            next_node_id: self.next_node_id,
            next_edge_id: self.next_edge_id,
            root: self.root,
            cfg_entries: self.cfg_entries.clone(),
            cfg_exits: self.cfg_exits.clone(),
        }
    }
}

impl Default for CodePropertyGraph {
    fn default() -> Self {
        Self::new(Language::Unknown)
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
    /// Number of call graph edges.
    pub call_edges: usize,
    /// Number of functions.
    pub function_count: usize,
    /// Number of classes.
    pub class_count: usize,
    /// Cyclomatic complexity.
    pub cyclomatic_complexity: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
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
                signature: MethodSignature {
                    name: "main".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Public,
                },
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

        let n1 = cpg.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::Root, SourceRange::default()));
        let n2 = cpg.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::Root, SourceRange::default()));

        let edge_id = cpg.connect(n1, n2, CpgEdgeKind::AstChild);
        assert!(edge_id.is_some());
        assert_eq!(cpg.edge_count(), 1);
    }

    #[test]
    fn test_cfg_traversal() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);

        let entry = cpg.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::If, SourceRange::default()));
        let then_branch = cpg.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::Block { scope: super::super::ScopeId::GLOBAL }, SourceRange::default()));
        let else_branch = cpg.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::Block { scope: super::super::ScopeId::GLOBAL }, SourceRange::default()));

        cpg.connect(entry, then_branch, CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue));
        cpg.connect(entry, else_branch, CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalFalse));

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
            signature: MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Public,
            },
        }
    }

    fn block() -> CpgNodeKind {
        CpgNodeKind::Block { scope: ScopeId::GLOBAL }
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
        g.connect(entry, then_b, CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue));
        g.connect(entry, else_b, CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalFalse));
        g.connect(then_b, merge, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
        g.connect(else_b, merge, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));
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
        g.connect(b, a, CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalFalse));
        assert_eq!(g.cyclomatic_complexity(), 4);
    }

    #[test]
    fn stats_all_fields() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let f = g.add_node(mk(func("f"))); // Function (also the root)
        let blk = g.add_node(mk(block()));
        let cls = g.add_node(mk(CpgNodeKind::Class { name: "C".into(), is_abstract: false }));
        let call = g.add_node(mk(CpgNodeKind::Call { target: None, is_method: false }));
        let var = g.add_node(mk(CpgNodeKind::Variable {
            name: "v".into(),
            var_type: None,
            scope: ScopeId::GLOBAL,
            is_mutable: false,
        }));
        let idn = g.add_node(mk(CpgNodeKind::Identifier { name: "x".into(), definition: None }));

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
        let idn = g.add_node(mk(CpgNodeKind::Identifier { name: "x".into(), definition: None }));
        let var = g.add_node(mk(CpgNodeKind::Variable {
            name: "v".into(),
            var_type: None,
            scope: ScopeId::GLOBAL,
            is_mutable: false,
        }));
        let call = g.add_node(mk(CpgNodeKind::Call { target: None, is_method: false }));
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
        let use_site = g.add_node(mk(CpgNodeKind::Identifier { name: "v".into(), definition: None }));
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
        let _idn =
            g.add_node(mk_r(CpgNodeKind::Identifier { name: "x".into(), definition: None }, 20, 30));
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
        let call = g.add_node(mk(CpgNodeKind::Call { target: None, is_method: false }));
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
        g.add_node(mk(CpgNodeKind::Class { name: "C".into(), is_abstract: false }));
        g.add_node(mk(CpgNodeKind::Class { name: "D".into(), is_abstract: true }));
        g.add_node(mk(CpgNodeKind::Variable {
            name: "v".into(),
            var_type: None,
            scope: ScopeId::GLOBAL,
            is_mutable: false,
        }));
        g.add_node(mk(CpgNodeKind::Call { target: None, is_method: false }));
        g.add_node(mk(func("f")));
        assert_eq!(g.classes().count(), 2);
        assert_eq!(g.variables().count(), 1);
        assert_eq!(g.calls().count(), 1);
        assert_eq!(g.functions().count(), 1);
        assert_eq!(g.nodes_by_kind(|k| matches!(k, CpgNodeKind::Class { .. })).count(), 2);
        assert_eq!(g.nodes_by_kind(|k| matches!(k, CpgNodeKind::Return)).count(), 0);
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
        let bad =
            g.add_edge_with_id(CpgEdge::new(EdgeId::new(20), a, NodeId::new(999), CpgEdgeKind::AstChild));
        assert_eq!(bad, None);
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
        assert_eq!(g.connect_unique(a, NodeId::new(999), CpgEdgeKind::AstChild), None);
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
            CpgNodeKind::Block { scope: ScopeId::GLOBAL },
            SourceRange::default(),
        ));
        let child = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Return,
            SourceRange::default(),
        ));
        cpg.connect(root, child, CpgEdgeKind::AstChild);
        cpg.node_mut(child).expect("child").parent = Some(root);
        // The root's parent points at a node that does not exist.
        let absent = NodeId::new(9_999);
        cpg.node_mut(root).expect("root").parent = Some(absent);
        assert!(cpg.node(absent).is_none(), "the id really is absent");

        let ancestors = cpg.ast_ancestors(child);
        assert_eq!(ancestors, vec![root], "the chain stops before the absent id");
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
        fn prop_ast_children_match_child_list(g in arb_well_formed_cpg()) {
            // `wf_child` appends to `children` and inserts the AstChild edge in
            // the same order, so the edge-id-ordered `ast_children` reproduces
            // the stored child list exactly.
            for id in g.node_ids() {
                let listed: Vec<NodeId> = g.node(id).expect("node").children.to_vec();
                prop_assert_eq!(g.ast_children(id), listed);
            }
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
        assert_eq!(stats.dfg_edges, back.dfg_edges);
        assert_eq!(stats.call_edges, back.call_edges);
        assert_eq!(stats.function_count, back.function_count);
        assert_eq!(stats.class_count, back.class_count);
        assert_eq!(stats.cyclomatic_complexity, back.cyclomatic_complexity);
    }

    #[test]
    fn serde_whole_graph_example() {
        let mut g = CodePropertyGraph::new(Language::Rust);
        let a = g.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::Root, SourceRange::from_bytes(0, 10)));
        let b = g.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::Return, SourceRange::from_bytes(2, 4)));
        g.connect(a, b, CpgEdgeKind::AstChild);
        g.connect(a, b, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));

        let s = serde_json::to_string(&g).expect("serialize graph");
        let back: CodePropertyGraph = serde_json::from_str(&s).expect("deserialize graph");
        assert_eq!(back.node_count(), 2);
        assert_eq!(back.edge_count(), 2);
        assert_eq!(back.language(), Language::Rust);
        assert!(same_shape(&g, &back));
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
