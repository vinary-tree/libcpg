//! Deterministic edit scripts between two Code Property Graph versions.
//!
//! Node correspondence reuses the bounded bipartite GED assignment from
//! [`crate::pattern`]. Once nodes are aligned, exact typed edge multisets are
//! compared in the target coordinate system. All traversal and matching state
//! is heap-backed; neither graph is mutated.

use std::cmp::Ordering;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::pattern::{ged_alignment, GedOptions, MAX_GED_NODES};
use crate::{
    AnalysisError, CodePropertyGraph, CpgEdge, CpgEdgeKind, CpgNode, CpgNodeKind, EdgeId, LockKind,
    MsgKind, NodeId, PropertyKey, PropertyValue, SourceRange, TypeInfo,
};

use super::edge_kind_class;

/// Hard ceiling for nodes admitted from either input graph.
pub const MAX_CPG_DIFF_NODES: usize = MAX_GED_NODES;
/// Hard ceiling for edges admitted from either input graph.
pub const MAX_CPG_DIFF_EDGES: usize = 2_000_000;
/// Hard ceiling for edit records returned by one diff.
pub const MAX_CPG_DIFF_EDITS: usize = 2_000_000;

/// Resource limits for [`cpg_diff`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CpgDiffOptions {
    /// Largest node count admitted for either input graph.
    ///
    /// The effective value is clamped to [`MAX_GED_NODES`] because alignment
    /// uses T2.9's cubic Hungarian assignment.
    pub max_nodes: usize,
    /// Largest represented edge count admitted from either graph.
    ///
    /// The effective value is clamped to [`MAX_CPG_DIFF_EDGES`]. This bounds
    /// the auxiliary exact-edge index independently of the returned edit count.
    pub max_edges: usize,
    /// Largest aggregate number of added, removed, or changed records.
    ///
    /// The effective value is clamped to [`MAX_CPG_DIFF_EDITS`]. A breach is
    /// an error; [`cpg_diff`] never returns a truncated edit script.
    pub max_edits: usize,
}

impl Default for CpgDiffOptions {
    fn default() -> Self {
        Self {
            max_nodes: MAX_CPG_DIFF_NODES,
            max_edges: 200_000,
            max_edits: 200_000,
        }
    }
}

/// Whether the represented dependence structure changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ChangeKind {
    /// No inserted, removed, or substituted PDG dependence edge was observed.
    Refactoring,
    /// At least one represented PDG dependence edge changed.
    BehaviorChanging,
}

/// One matched node pair in old/new graph coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NodeAlignment {
    /// Node identifier in the old graph.
    pub old: NodeId,
    /// Corresponding node identifier in the new graph.
    pub new: NodeId,
}

/// One canonically ordered node property.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NodePropertyRecord {
    /// Property key.
    pub key: PropertyKey,
    /// Stack-safe property value.
    pub value: PropertyValue,
}

/// Owned, deterministic snapshot of one CPG node.
///
/// AST links remain present even though ordinary AST structure is also
/// represented by edges: this makes inconsistent or partially reconstructed
/// graphs observable instead of silently normalizing them.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DiffNode {
    /// Node identifier in this snapshot's graph coordinate system.
    pub id: NodeId,
    /// Exact node kind and attached semantic payload.
    pub kind: CpgNodeKind,
    /// Exact source range.
    pub range: SourceRange,
    /// Retained source text, when present.
    pub text: Option<Arc<str>>,
    /// Normalized message operation, when present.
    pub message_kind: Option<MsgKind>,
    /// Normalized lock operation, when present.
    pub lock_kind: Option<LockKind>,
    /// Normalized allocation type, when present.
    pub allocation_type: Option<TypeInfo>,
    /// Properties ordered by their closed key vocabulary and custom key text.
    pub properties: Vec<NodePropertyRecord>,
    /// Cached AST children in their represented order.
    pub children: Vec<NodeId>,
    /// Cached AST parent.
    pub parent: Option<NodeId>,
}

/// Owned snapshot of one typed, directed CPG edge.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DiffEdge {
    /// Edge identifier in this snapshot's graph coordinate system.
    pub id: EdgeId,
    /// Source node.
    pub source: NodeId,
    /// Target node.
    pub target: NodeId,
    /// Exact typed relation.
    pub kind: CpgEdgeKind,
    /// Optional relation label.
    pub label: Option<String>,
}

/// An inserted or deleted graph element.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CpgElement {
    /// A node snapshot.
    Node(Box<DiffNode>),
    /// An edge snapshot.
    Edge(DiffEdge),
}

/// A node field that differs across an aligned pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum NodeChangeField {
    /// Exact node kind or semantic payload.
    Kind,
    /// Source range.
    Range,
    /// Retained source text.
    Text,
    /// Normalized message operation.
    MessageKind,
    /// Normalized lock operation.
    LockKind,
    /// Normalized allocation type.
    AllocationType,
    /// Property map.
    Properties,
    /// Cached AST parent or ordered children.
    AstLinks,
}

/// An edge field that differs across an aligned endpoint pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum EdgeChangeField {
    /// Exact typed relation.
    Kind,
    /// Optional relation label.
    Label,
}

/// A substitution in the concrete edit script.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CpgChange {
    /// Changed payload on one aligned node pair.
    Node {
        /// Old node snapshot.
        old: Box<DiffNode>,
        /// New node snapshot.
        new: Box<DiffNode>,
        /// Exact changed fields, in declaration order.
        fields: Vec<NodeChangeField>,
    },
    /// Changed kind or label on one aligned endpoint pair.
    Edge {
        /// Old edge snapshot.
        old: DiffEdge,
        /// New edge snapshot.
        new: DiffEdge,
        /// Exact changed fields, in declaration order.
        fields: Vec<EdgeChangeField>,
    },
}

/// Canonical, owned edit script from an old CPG to a new CPG.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CpgDiff {
    /// GED-induced node correspondence, sorted by old then new id.
    pub alignment: Vec<NodeAlignment>,
    /// Concrete cost of the retained T2.9 alignment script.
    pub alignment_cost: f64,
    /// New-graph elements not represented by the old graph.
    pub added: Vec<CpgElement>,
    /// Old-graph elements not represented by the new graph.
    pub removed: Vec<CpgElement>,
    /// Aligned elements whose represented payload changed.
    pub changed: Vec<CpgChange>,
    /// Dependence-sensitive classification of the edit script.
    pub classification: ChangeKind,
}

/// Computes a deterministic structural diff without mutating either graph.
///
/// Nodes are aligned by the symmetric T2.9 bipartite GED correspondence. Node
/// ids and edge ids are coordinate labels, not semantic identity: renumbered
/// but aligned elements are unchanged. Exact typed edge multisets (including
/// parallel multiplicity and labels) are then compared under that node map.
/// Leftover edges on the same aligned endpoints become substitutions; all
/// other leftovers become insertions or deletions.
///
/// [`ChangeKind::BehaviorChanging`] means the edit script inserted, removed,
/// or substituted an edge whose old or new kind is
/// [`CpgEdgeKind::ControlDependence`] or [`CpgEdgeKind::DataDependence`]. It is
/// evidence about the represented PDG, not a proof that runtime behavior is
/// equivalent when no PDG was constructed.
///
/// The node, edge, and edit caps are complete-or-error boundaries. No partial
/// diff is returned. All continuations, assignment matrices, buckets, and
/// output state are heap-resident.
pub fn cpg_diff(
    old: &CodePropertyGraph,
    new: &CodePropertyGraph,
    options: &CpgDiffOptions,
) -> Result<CpgDiff, AnalysisError> {
    let node_cap = options.max_nodes.min(MAX_CPG_DIFF_NODES);
    let edge_cap = options.max_edges.min(MAX_CPG_DIFF_EDGES);
    let needed_edges = old.edge_count().max(new.edge_count());
    if needed_edges > edge_cap {
        return Err(AnalysisError::BudgetExceeded {
            analysis: "cpg_diff",
            unit: "edges per graph",
            needed: u64::try_from(needed_edges).unwrap_or(u64::MAX),
            cap: u64::try_from(edge_cap).unwrap_or(u64::MAX),
        });
    }
    let edit_cap = options.max_edits.min(MAX_CPG_DIFF_EDITS);
    let alignment = ged_alignment(
        old,
        new,
        &GedOptions {
            max_nodes: node_cap,
            ..GedOptions::default()
        },
    )?;
    let alignment_cost = alignment.cost;
    let alignment = alignment
        .pairs
        .into_iter()
        .map(|(old, new)| NodeAlignment { old, new })
        .collect::<Vec<_>>();
    let old_to_new: FxHashMap<_, _> = alignment.iter().map(|pair| (pair.old, pair.new)).collect();
    let matched_old: FxHashSet<_> = old_to_new.keys().copied().collect();
    let matched_new: FxHashSet<_> = old_to_new.values().copied().collect();

    let mut budget = EditBudget::new(edit_cap);
    let mut added = Vec::new();
    let mut removed = Vec::new();
    let mut changed = Vec::new();

    let mut old_nodes = old.nodes().collect::<Vec<_>>();
    let mut new_nodes = new.nodes().collect::<Vec<_>>();
    old_nodes.sort_by_key(|node| node.id);
    new_nodes.sort_by_key(|node| node.id);

    for pair in &alignment {
        let old_node = old
            .node(pair.old)
            .expect("GED correspondence references an old-graph node");
        let new_node = new
            .node(pair.new)
            .expect("GED correspondence references a new-graph node");
        let fields = changed_node_fields(old_node, new_node, &old_to_new);
        if !fields.is_empty() {
            budget.claim()?;
            changed.push(CpgChange::Node {
                old: Box::new(snapshot_node(old_node)),
                new: Box::new(snapshot_node(new_node)),
                fields,
            });
        }
    }
    for node in old_nodes {
        if !matched_old.contains(&node.id) {
            budget.claim()?;
            removed.push(CpgElement::Node(Box::new(snapshot_node(node))));
        }
    }
    for node in new_nodes {
        if !matched_new.contains(&node.id) {
            budget.claim()?;
            added.push(CpgElement::Node(Box::new(snapshot_node(node))));
        }
    }

    let mut old_edges = old.edges().collect::<Vec<_>>();
    let mut new_edges = new.edges().collect::<Vec<_>>();
    old_edges.sort_by(|left, right| edge_order(left, right));
    new_edges.sort_by(|left, right| edge_order(left, right));
    let mut new_exact = BTreeMap::<EdgeKey, VecDeque<usize>>::new();
    for (index, edge) in new_edges.iter().enumerate() {
        new_exact
            .entry(EdgeKey::of(edge))
            .or_default()
            .push_back(index);
    }

    let mut old_used = vec![false; old_edges.len()];
    let mut new_used = vec![false; new_edges.len()];
    for (old_index, edge) in old_edges.iter().enumerate() {
        let (Some(source), Some(target)) = (
            old_to_new.get(&edge.source).copied(),
            old_to_new.get(&edge.target).copied(),
        ) else {
            continue;
        };
        let key = EdgeKey::translated(edge, source, target);
        let Some(candidates) = new_exact.get_mut(&key) else {
            continue;
        };
        if let Some(new_index) = candidates.pop_front() {
            old_used[old_index] = true;
            new_used[new_index] = true;
        }
    }

    let mut old_by_endpoints = BTreeMap::<(NodeId, NodeId), Vec<usize>>::new();
    let mut new_by_endpoints = BTreeMap::<(NodeId, NodeId), Vec<usize>>::new();
    for (old_index, edge) in old_edges.iter().enumerate() {
        if old_used[old_index] {
            continue;
        }
        if let (Some(source), Some(target)) = (
            old_to_new.get(&edge.source).copied(),
            old_to_new.get(&edge.target).copied(),
        ) {
            old_by_endpoints
                .entry((source, target))
                .or_default()
                .push(old_index);
        }
    }
    for (new_index, edge) in new_edges.iter().enumerate() {
        if !new_used[new_index] {
            new_by_endpoints
                .entry((edge.source, edge.target))
                .or_default()
                .push(new_index);
        }
    }

    let mut behavior_changing = false;
    for (endpoints, old_indices) in old_by_endpoints {
        let Some(new_indices) = new_by_endpoints.get(&endpoints) else {
            continue;
        };
        for (&old_index, &new_index) in old_indices.iter().zip(new_indices) {
            let old_edge = old_edges[old_index];
            let new_edge = new_edges[new_index];
            let mut fields = Vec::with_capacity(2);
            if old_edge.kind != new_edge.kind {
                fields.push(EdgeChangeField::Kind);
            }
            if old_edge.label != new_edge.label {
                fields.push(EdgeChangeField::Label);
            }
            debug_assert!(!fields.is_empty(), "exact edges were matched first");
            budget.claim()?;
            behavior_changing |= old_edge.kind.is_pdg() || new_edge.kind.is_pdg();
            changed.push(CpgChange::Edge {
                old: snapshot_edge(old_edge),
                new: snapshot_edge(new_edge),
                fields,
            });
            old_used[old_index] = true;
            new_used[new_index] = true;
        }
    }

    for (index, edge) in old_edges.into_iter().enumerate() {
        if !old_used[index] {
            budget.claim()?;
            behavior_changing |= edge.kind.is_pdg();
            removed.push(CpgElement::Edge(snapshot_edge(edge)));
        }
    }
    for (index, edge) in new_edges.into_iter().enumerate() {
        if !new_used[index] {
            budget.claim()?;
            behavior_changing |= edge.kind.is_pdg();
            added.push(CpgElement::Edge(snapshot_edge(edge)));
        }
    }

    Ok(CpgDiff {
        alignment,
        alignment_cost,
        added,
        removed,
        changed,
        classification: if behavior_changing {
            ChangeKind::BehaviorChanging
        } else {
            ChangeKind::Refactoring
        },
    })
}

#[derive(Debug)]
struct EditBudget {
    cap: usize,
    used: usize,
}

impl EditBudget {
    fn new(cap: usize) -> Self {
        Self { cap, used: 0 }
    }

    fn claim(&mut self) -> Result<(), AnalysisError> {
        let needed = self.used.saturating_add(1);
        if needed > self.cap {
            return Err(AnalysisError::BudgetExceeded {
                analysis: "cpg_diff",
                unit: "edit records",
                needed: u64::try_from(needed).unwrap_or(u64::MAX),
                cap: u64::try_from(self.cap).unwrap_or(u64::MAX),
            });
        }
        self.used = needed;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct EdgeKey {
    source: NodeId,
    target: NodeId,
    kind: u16,
    label: Option<String>,
}

impl EdgeKey {
    fn of(edge: &CpgEdge) -> Self {
        Self::translated(edge, edge.source, edge.target)
    }

    fn translated(edge: &CpgEdge, source: NodeId, target: NodeId) -> Self {
        Self {
            source,
            target,
            kind: edge_kind_class(&edge.kind),
            label: edge.label.clone(),
        }
    }
}

fn edge_order(left: &CpgEdge, right: &CpgEdge) -> Ordering {
    (
        left.source,
        left.target,
        edge_kind_class(&left.kind),
        left.label.as_deref(),
        left.id.as_u32(),
    )
        .cmp(&(
            right.source,
            right.target,
            edge_kind_class(&right.kind),
            right.label.as_deref(),
            right.id.as_u32(),
        ))
}

fn changed_node_fields(
    old: &CpgNode,
    new: &CpgNode,
    old_to_new: &FxHashMap<NodeId, NodeId>,
) -> Vec<NodeChangeField> {
    let mut fields = Vec::with_capacity(8);
    if !node_kinds_equivalent(&old.kind, &new.kind, old_to_new) {
        fields.push(NodeChangeField::Kind);
    }
    if old.range != new.range {
        fields.push(NodeChangeField::Range);
    }
    if old.text != new.text {
        fields.push(NodeChangeField::Text);
    }
    if old.message_kind != new.message_kind {
        fields.push(NodeChangeField::MessageKind);
    }
    if old.lock_kind != new.lock_kind {
        fields.push(NodeChangeField::LockKind);
    }
    if old.allocation_type != new.allocation_type {
        fields.push(NodeChangeField::AllocationType);
    }
    if old.properties != new.properties {
        fields.push(NodeChangeField::Properties);
    }
    if !mapped_optional(old.parent, new.parent, old_to_new)
        || !mapped_slice(&old.children, &new.children, old_to_new)
    {
        fields.push(NodeChangeField::AstLinks);
    }
    fields
}

fn node_kinds_equivalent(
    old: &CpgNodeKind,
    new: &CpgNodeKind,
    old_to_new: &FxHashMap<NodeId, NodeId>,
) -> bool {
    match (old, new) {
        (
            CpgNodeKind::Call {
                target: old_target,
                is_method: old_method,
            },
            CpgNodeKind::Call {
                target: new_target,
                is_method: new_method,
            },
        ) => old_method == new_method && mapped_optional(*old_target, *new_target, old_to_new),
        (
            CpgNodeKind::Identifier {
                name: old_name,
                definition: old_definition,
            },
            CpgNodeKind::Identifier {
                name: new_name,
                definition: new_definition,
            },
        ) => old_name == new_name && mapped_optional(*old_definition, *new_definition, old_to_new),
        (
            CpgNodeKind::Lambda {
                captures: old_captures,
            },
            CpgNodeKind::Lambda {
                captures: new_captures,
            },
        ) => mapped_slice(old_captures, new_captures, old_to_new),
        _ => old == new,
    }
}

fn mapped_optional(
    old: Option<NodeId>,
    new: Option<NodeId>,
    old_to_new: &FxHashMap<NodeId, NodeId>,
) -> bool {
    match (old, new) {
        (None, None) => true,
        (Some(old), Some(new)) => old_to_new.get(&old).copied() == Some(new),
        _ => false,
    }
}

fn mapped_slice(old: &[NodeId], new: &[NodeId], old_to_new: &FxHashMap<NodeId, NodeId>) -> bool {
    old.len() == new.len()
        && old
            .iter()
            .zip(new)
            .all(|(old, new)| old_to_new.get(old).copied() == Some(*new))
}

fn snapshot_node(node: &CpgNode) -> DiffNode {
    let mut properties = node
        .properties
        .iter()
        .map(|(key, value)| NodePropertyRecord {
            key: key.clone(),
            value: value.clone(),
        })
        .collect::<Vec<_>>();
    properties.sort_by(|left, right| property_key_cmp(&left.key, &right.key));
    DiffNode {
        id: node.id,
        kind: node.kind.clone(),
        range: node.range,
        text: node.text.clone(),
        message_kind: node.message_kind,
        lock_kind: node.lock_kind,
        allocation_type: node.allocation_type.clone(),
        properties,
        children: node.children.to_vec(),
        parent: node.parent,
    }
}

fn property_key_cmp(left: &PropertyKey, right: &PropertyKey) -> Ordering {
    fn parts(key: &PropertyKey) -> (u8, &str) {
        match key {
            PropertyKey::Name => (0, ""),
            PropertyKey::Type => (1, ""),
            PropertyKey::Scope => (2, ""),
            PropertyKey::Visibility => (3, ""),
            PropertyKey::Mutable => (4, ""),
            PropertyKey::Static => (5, ""),
            PropertyKey::Async => (6, ""),
            PropertyKey::Custom(name) => (7, name),
        }
    }
    parts(left).cmp(&parts(right))
}

fn snapshot_edge(edge: &CpgEdge) -> DiffEdge {
    DiffEdge {
        id: edge.id,
        source: edge.source,
        target: edge.target,
        kind: edge.kind.clone(),
        label: edge.label.clone(),
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::{DfgEdgeKind, Language, LiteralKind, PropertyValue};

    fn add_node(graph: &mut CodePropertyGraph, id: u32, kind: CpgNodeKind) -> NodeId {
        let id = NodeId::new(id);
        graph.add_node_with_id(CpgNode::new(id, kind, SourceRange::default()));
        id
    }

    fn add_edge(
        graph: &mut CodePropertyGraph,
        id: u32,
        source: NodeId,
        target: NodeId,
        kind: CpgEdgeKind,
    ) {
        graph
            .add_edge_with_id(CpgEdge::new(EdgeId::new(id), source, target, kind))
            .expect("fixture endpoints exist");
    }

    fn renamed_graph(new: bool) -> CodePropertyGraph {
        let mut graph = CodePropertyGraph::new(Language::Rust);
        let (root, variable, identifier) = if new { (101, 303, 707) } else { (10, 30, 70) };
        let variable_id = NodeId::new(variable);
        add_node(&mut graph, root, CpgNodeKind::Root);
        graph.add_node_with_id(
            CpgNode::new(
                variable_id,
                CpgNodeKind::Variable {
                    name: if new { "renamed" } else { "original" }.into(),
                    var_type: None,
                    scope: crate::ScopeId::GLOBAL,
                    is_mutable: false,
                },
                SourceRange::new(2, 3, 0, 2, 0, 3),
            )
            .with_text(if new { "renamed" } else { "original" }),
        );
        graph.add_node_with_id(
            CpgNode::new(
                NodeId::new(identifier),
                CpgNodeKind::Identifier {
                    name: if new { "renamed" } else { "original" }.into(),
                    definition: Some(variable_id),
                },
                SourceRange::new(4, 5, 0, 4, 0, 5),
            )
            .with_text(if new { "renamed" } else { "original" }),
        );
        add_edge(
            &mut graph,
            1,
            NodeId::new(root),
            variable_id,
            CpgEdgeKind::AstChild,
        );
        add_edge(
            &mut graph,
            2,
            NodeId::new(root),
            NodeId::new(identifier),
            CpgEdgeKind::AstChild,
        );
        add_edge(
            &mut graph,
            3,
            NodeId::new(identifier),
            variable_id,
            CpgEdgeKind::Reference,
        );
        graph
    }

    #[test]
    fn identical_graphs_have_alignment_but_an_empty_refactoring_diff() {
        let graph = renamed_graph(false);
        let shape = (graph.node_count(), graph.edge_count());
        let diff = cpg_diff(&graph, &graph, &CpgDiffOptions::default()).expect("identical diff");

        assert_eq!(diff.alignment.len(), graph.node_count());
        assert_eq!(diff.alignment_cost, 0.0);
        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
        assert!(diff.changed.is_empty());
        assert_eq!(diff.classification, ChangeKind::Refactoring);
        assert_eq!((graph.node_count(), graph.edge_count()), shape);
    }

    #[test]
    fn pure_rename_is_a_node_substitution_not_a_behavior_change() {
        let old = renamed_graph(false);
        let new = renamed_graph(true);
        let diff = cpg_diff(&old, &new, &CpgDiffOptions::default()).expect("rename diff");

        assert_eq!(
            diff.alignment_cost, 0.0,
            "GED tags and topology are unchanged"
        );
        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
        assert_eq!(diff.changed.len(), 2);
        assert_eq!(diff.classification, ChangeKind::Refactoring);
        for change in &diff.changed {
            let CpgChange::Node { fields, .. } = change else {
                panic!("rename must only change nodes");
            };
            assert_eq!(fields, &[NodeChangeField::Kind, NodeChangeField::Text]);
        }
        assert!(diff.alignment.contains(&NodeAlignment {
            old: NodeId::new(70),
            new: NodeId::new(707),
        }));
    }

    #[test]
    fn an_added_guard_with_control_dependence_is_behavior_changing() {
        let mut old = CodePropertyGraph::new(Language::Rust);
        let old_root = add_node(&mut old, 1, CpgNodeKind::Root);
        let old_value = add_node(
            &mut old,
            9,
            CpgNodeKind::Literal {
                kind: LiteralKind::Integer(1),
            },
        );
        add_edge(&mut old, 1, old_root, old_value, CpgEdgeKind::AstChild);

        let mut new = CodePropertyGraph::new(Language::Rust);
        let new_root = add_node(&mut new, 101, CpgNodeKind::Root);
        let guard = add_node(&mut new, 202, CpgNodeKind::If);
        let new_value = add_node(
            &mut new,
            909,
            CpgNodeKind::Literal {
                kind: LiteralKind::Integer(1),
            },
        );
        add_edge(&mut new, 11, new_root, guard, CpgEdgeKind::AstChild);
        add_edge(&mut new, 12, guard, new_value, CpgEdgeKind::AstChild);
        add_edge(
            &mut new,
            13,
            guard,
            new_value,
            CpgEdgeKind::ControlDependence,
        );

        let diff = cpg_diff(&old, &new, &CpgDiffOptions::default()).expect("guard diff");
        assert_eq!(diff.classification, ChangeKind::BehaviorChanging);
        assert!(diff.added.iter().any(|element| matches!(
            element,
            CpgElement::Node(node) if node.id == guard
        )));
        assert!(diff.added.iter().any(|element| matches!(
            element,
            CpgElement::Edge(edge) if edge.kind == CpgEdgeKind::ControlDependence
        )));
    }

    #[test]
    fn typed_parallel_edges_are_matched_as_multisets_then_substituted() {
        let mut old = CodePropertyGraph::new(Language::Rust);
        let old_source = add_node(&mut old, 1, CpgNodeKind::If);
        let old_target = add_node(&mut old, 2, CpgNodeKind::Return);
        add_edge(
            &mut old,
            4,
            old_source,
            old_target,
            CpgEdgeKind::DataDependence,
        );
        old.add_edge_with_id(
            CpgEdge::new(
                EdgeId::new(5),
                old_source,
                old_target,
                CpgEdgeKind::DataDependence,
            )
            .with_label("left"),
        )
        .expect("old labeled edge endpoints exist");

        let mut new = CodePropertyGraph::new(Language::Rust);
        let new_source = add_node(&mut new, 11, CpgNodeKind::If);
        let new_target = add_node(&mut new, 22, CpgNodeKind::Return);
        add_edge(
            &mut new,
            40,
            new_source,
            new_target,
            CpgEdgeKind::DataDependence,
        );
        new.add_edge_with_id(
            CpgEdge::new(
                EdgeId::new(50),
                new_source,
                new_target,
                CpgEdgeKind::ControlDependence,
            )
            .with_label("right"),
        )
        .expect("new labeled edge endpoints exist");

        let diff = cpg_diff(&old, &new, &CpgDiffOptions::default()).expect("typed edge diff");
        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.classification, ChangeKind::BehaviorChanging);
        assert!(matches!(
            &diff.changed[0],
            CpgChange::Edge { fields, .. }
                if fields == &[EdgeChangeField::Kind, EdgeChangeField::Label]
        ));
    }

    #[test]
    fn node_snapshots_are_complete_and_property_order_is_canonical() {
        let mut first = CpgNode::new(NodeId::new(1), CpgNodeKind::If, SourceRange::default());
        first.properties.insert(
            PropertyKey::Custom("zeta".into()),
            PropertyValue::List(vec![PropertyValue::Int(1)]),
        );
        first
            .properties
            .insert(PropertyKey::Name, PropertyValue::String("name".into()));
        first.properties.insert(
            PropertyKey::Custom("alpha".into()),
            PropertyValue::Bool(true),
        );
        let mut second = first.clone();
        second.properties.clear();
        second.properties.insert(
            PropertyKey::Custom("alpha".into()),
            PropertyValue::Bool(true),
        );
        second
            .properties
            .insert(PropertyKey::Name, PropertyValue::String("name".into()));
        second.properties.insert(
            PropertyKey::Custom("zeta".into()),
            PropertyValue::List(vec![PropertyValue::Int(1)]),
        );

        let first = snapshot_node(&first);
        let second = snapshot_node(&second);
        assert_eq!(first, second);
        assert!(matches!(first.properties[0].key, PropertyKey::Name));
        assert!(matches!(
            &first.properties[1].key,
            PropertyKey::Custom(name) if name.as_ref() == "alpha"
        ));
        assert!(matches!(
            &first.properties[2].key,
            PropertyKey::Custom(name) if name.as_ref() == "zeta"
        ));
    }

    #[test]
    fn embedded_node_references_are_compared_in_aligned_coordinates() {
        let mut old = CodePropertyGraph::new(Language::Rust);
        let old_definition = add_node(
            &mut old,
            5,
            CpgNodeKind::Variable {
                name: "x".into(),
                var_type: None,
                scope: crate::ScopeId::GLOBAL,
                is_mutable: false,
            },
        );
        add_node(
            &mut old,
            8,
            CpgNodeKind::Identifier {
                name: "x".into(),
                definition: Some(old_definition),
            },
        );

        let mut new = CodePropertyGraph::new(Language::Rust);
        let new_definition = add_node(
            &mut new,
            50,
            CpgNodeKind::Variable {
                name: "x".into(),
                var_type: None,
                scope: crate::ScopeId::GLOBAL,
                is_mutable: false,
            },
        );
        add_node(
            &mut new,
            80,
            CpgNodeKind::Identifier {
                name: "x".into(),
                definition: Some(new_definition),
            },
        );

        let diff = cpg_diff(&old, &new, &CpgDiffOptions::default()).expect("renumbered refs");
        assert!(diff.added.is_empty());
        assert!(diff.removed.is_empty());
        assert!(diff.changed.is_empty());
    }

    #[test]
    fn node_and_edit_caps_fail_closed_without_partial_output() {
        let graph = renamed_graph(false);
        assert!(matches!(
            cpg_diff(
                &graph,
                &graph,
                &CpgDiffOptions {
                    max_nodes: 2,
                    ..CpgDiffOptions::default()
                }
            ),
            Err(AnalysisError::BudgetExceeded {
                analysis: "cpg_diff",
                unit: "nodes per graph",
                needed: 3,
                cap: 2,
            })
        ));

        assert!(matches!(
            cpg_diff(
                &graph,
                &graph,
                &CpgDiffOptions {
                    max_edges: 2,
                    ..CpgDiffOptions::default()
                }
            ),
            Err(AnalysisError::BudgetExceeded {
                analysis: "cpg_diff",
                unit: "edges per graph",
                needed: 3,
                cap: 2,
            })
        ));

        assert!(matches!(
            cpg_diff(
                &renamed_graph(false),
                &renamed_graph(true),
                &CpgDiffOptions {
                    max_edits: 1,
                    ..CpgDiffOptions::default()
                }
            ),
            Err(AnalysisError::BudgetExceeded {
                analysis: "cpg_diff",
                unit: "edit records",
                needed: 2,
                cap: 1,
            })
        ));
        let empty = CodePropertyGraph::new(Language::Rust);
        assert!(cpg_diff(
            &empty,
            &empty,
            &CpgDiffOptions {
                max_nodes: 0,
                max_edges: 0,
                max_edits: 0,
            }
        )
        .expect("zero caps admit an empty diff")
        .changed
        .is_empty());
    }

    fn graph_from_bits(
        node_count: usize,
        labels: u16,
        edges: u64,
        reverse_storage: bool,
    ) -> CodePropertyGraph {
        let mut graph = CodePropertyGraph::new(Language::Rust);
        let nodes = if reverse_storage {
            (0..node_count).rev().collect::<Vec<_>>()
        } else {
            (0..node_count).collect::<Vec<_>>()
        };
        for node in nodes {
            add_node(
                &mut graph,
                (node * 13 + 7) as u32,
                if labels & (1 << node) == 0 {
                    CpgNodeKind::If
                } else {
                    CpgNodeKind::While
                },
            );
        }
        let mut represented = (0..node_count)
            .flat_map(|source| {
                (0..node_count).filter_map(move |target| {
                    let bit = source * 8 + target;
                    (bit < 64 && edges & (1 << bit) != 0).then_some((source, target, bit))
                })
            })
            .collect::<Vec<_>>();
        if reverse_storage {
            represented.reverse();
        }
        for (source, target, bit) in represented {
            add_edge(
                &mut graph,
                bit as u32,
                NodeId::new((source * 13 + 7) as u32),
                NodeId::new((target * 13 + 7) as u32),
                if bit % 3 == 0 {
                    CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse)
                } else {
                    CpgEdgeKind::AstChild
                },
            );
        }
        graph
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn diff_is_invariant_to_node_and_edge_storage_order(
            old_nodes in 0usize..=8,
            new_nodes in 0usize..=8,
            old_labels in any::<u16>(),
            new_labels in any::<u16>(),
            old_edges in any::<u64>(),
            new_edges in any::<u64>(),
        ) {
            let old_forward = graph_from_bits(old_nodes, old_labels, old_edges, false);
            let old_reverse = graph_from_bits(old_nodes, old_labels, old_edges, true);
            let new_forward = graph_from_bits(new_nodes, new_labels, new_edges, false);
            let new_reverse = graph_from_bits(new_nodes, new_labels, new_edges, true);

            let forward = cpg_diff(&old_forward, &new_forward, &CpgDiffOptions::default())
                .expect("small generated diff");
            let reversed = cpg_diff(&old_reverse, &new_reverse, &CpgDiffOptions::default())
                .expect("storage-permuted generated diff");
            prop_assert_eq!(forward, reversed);
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn options_and_complete_diff_round_trip_with_serde() {
        let options = CpgDiffOptions::default();
        let diff =
            cpg_diff(&renamed_graph(false), &renamed_graph(true), &options).expect("rename diff");
        assert_eq!(
            serde_json::from_slice::<CpgDiff>(
                &serde_json::to_vec(&diff).expect("serialize complete diff")
            )
            .expect("deserialize complete diff"),
            diff
        );
        assert_eq!(
            serde_json::from_slice::<CpgDiffOptions>(
                &serde_json::to_vec(&options).expect("serialize options")
            )
            .expect("deserialize options"),
            options
        );
    }
}
