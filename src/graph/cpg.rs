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
    pub fn ast_descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut result = Vec::new();
        let mut stack = vec![id];

        while let Some(current) = stack.pop() {
            for child in self.ast_children(current) {
                result.push(child);
                stack.push(child);
            }
        }

        result
    }

    /// Returns AST ancestors of a node (towards root).
    pub fn ast_ancestors(&self, id: NodeId) -> Vec<NodeId> {
        let mut result = Vec::new();
        let mut current = self.ast_parent(id);

        while let Some(parent) = current {
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
    pub fn ast_depth(&self) -> usize {
        let root = match self.root {
            Some(r) => r,
            None => return 0,
        };

        fn depth_from(cpg: &CodePropertyGraph, id: NodeId) -> usize {
            let children = cpg.ast_children(id);
            if children.is_empty() {
                1
            } else {
                1 + children.iter().map(|&c| depth_from(cpg, c)).max().unwrap_or(0)
            }
        }

        depth_from(self, root)
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
