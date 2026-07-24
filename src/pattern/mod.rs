//! Pattern matching and subgraph isomorphism.
//!
//! This module provides algorithms for detecting patterns in Code Property Graphs
//! using subgraph isomorphism (VF2/VF3 algorithms) and similarity metrics.

mod vf2;
mod similarity;

pub use vf2::{Vf2Matcher, Vf2State};
pub use similarity::{GraphSimilarity, SimilarityMetric};

use rustc_hash::FxHashMap;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::{CodePropertyGraph, NodeId, CpgNodeKind, CpgEdgeKind};

/// A match of a pattern in a target graph.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PatternMatch {
    /// Name/identifier of the matched pattern.
    pub pattern_name: String,
    /// Confidence score (0.0 to 1.0).
    pub confidence: f64,
    /// Mapping from pattern node IDs to target node IDs.
    pub node_mapping: FxHashMap<NodeId, NodeId>,
    /// The root node of the match in the target graph.
    pub root: NodeId,
    /// Optional metadata about the match.
    pub metadata: FxHashMap<String, String>,
}

impl PatternMatch {
    /// Creates a new pattern match.
    pub fn new(pattern_name: impl Into<String>, root: NodeId, confidence: f64) -> Self {
        Self {
            pattern_name: pattern_name.into(),
            confidence,
            node_mapping: FxHashMap::default(),
            root,
            metadata: FxHashMap::default(),
        }
    }

    /// Adds a node mapping.
    pub fn with_mapping(mut self, pattern_node: NodeId, target_node: NodeId) -> Self {
        self.node_mapping.insert(pattern_node, target_node);
        self
    }

    /// Adds metadata.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Returns the matched nodes in the target graph.
    pub fn matched_nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.node_mapping.values().copied()
    }

    /// Returns the number of matched nodes.
    pub fn match_size(&self) -> usize {
        self.node_mapping.len()
    }
}

/// Trait for subgraph pattern matching algorithms.
///
/// Implementations of this trait find occurrences of a pattern graph
/// within a larger target graph.
pub trait SubgraphMatcher: Send + Sync {
    /// Finds all matches of a pattern in the target graph.
    ///
    /// # Arguments
    /// * `pattern` - The pattern graph to search for
    /// * `target` - The target graph to search in
    ///
    /// # Returns
    /// A vector of pattern matches
    fn find_matches(
        &self,
        pattern: &CodePropertyGraph,
        target: &CodePropertyGraph,
    ) -> Vec<PatternMatch>;

    /// Finds up to `limit` matches of a pattern in the target graph.
    fn find_matches_limited(
        &self,
        pattern: &CodePropertyGraph,
        target: &CodePropertyGraph,
        limit: usize,
    ) -> Vec<PatternMatch> {
        let mut matches = self.find_matches(pattern, target);
        matches.truncate(limit);
        matches
    }

    /// Returns true if the pattern exists in the target graph.
    fn contains_pattern(
        &self,
        pattern: &CodePropertyGraph,
        target: &CodePropertyGraph,
    ) -> bool {
        !self.find_matches_limited(pattern, target, 1).is_empty()
    }

    /// Returns the name of this matcher algorithm.
    fn algorithm_name(&self) -> &str;
}

/// A pattern template that can be matched against CPGs.
///
/// Pattern templates define structural constraints that must be
/// satisfied for a match to occur.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PatternTemplate {
    /// Name of the pattern.
    pub name: String,
    /// Description of what this pattern represents.
    pub description: String,
    /// Node constraints (node index -> required kind).
    pub node_constraints: Vec<NodeConstraint>,
    /// Edge constraints between nodes.
    pub edge_constraints: Vec<EdgeConstraint>,
    /// Minimum confidence threshold for matches.
    pub min_confidence: f64,
}

impl PatternTemplate {
    /// Creates a new pattern template.
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            node_constraints: Vec::new(),
            edge_constraints: Vec::new(),
            min_confidence: 0.8,
        }
    }

    /// Adds a node constraint.
    pub fn with_node(mut self, constraint: NodeConstraint) -> Self {
        self.node_constraints.push(constraint);
        self
    }

    /// Adds an edge constraint.
    pub fn with_edge(mut self, constraint: EdgeConstraint) -> Self {
        self.edge_constraints.push(constraint);
        self
    }

    /// Sets the minimum confidence threshold.
    pub fn with_min_confidence(mut self, confidence: f64) -> Self {
        self.min_confidence = confidence;
        self
    }

    /// Converts this template to a CPG pattern graph.
    ///
    /// Creates a CPG from the template's node and edge constraints.
    /// This allows the template to be used with subgraph matching algorithms.
    ///
    /// # Returns
    /// A `CodePropertyGraph` representing the pattern structure.
    pub fn to_pattern_graph(&self) -> CodePropertyGraph {
        use crate::{CpgNode, SourceRange, Language};

        let mut cpg = CodePropertyGraph::new(Language::Unknown);
        let mut node_id_map: FxHashMap<usize, NodeId> = FxHashMap::default();

        // Phase 1: Create nodes from node constraints
        for constraint in &self.node_constraints {
            let cpg_kind = constraint.to_cpg_node_kind();
            let node = CpgNode::new(NodeId::new(0), cpg_kind, SourceRange::default());
            let node_id = cpg.add_node(node);
            node_id_map.insert(constraint.index, node_id);
        }

        // Phase 2: Create edges from edge constraints
        for edge in &self.edge_constraints {
            // Skip edges where either node doesn't exist
            let Some(&src_id) = node_id_map.get(&edge.source) else {
                continue;
            };
            let Some(&tgt_id) = node_id_map.get(&edge.target) else {
                continue;
            };

            let edge_kind = edge.to_cpg_edge_kind();
            cpg.connect(src_id, tgt_id, edge_kind);
        }

        cpg
    }
}

impl NodeConstraint {
    /// Converts this constraint to a CpgNodeKind.
    ///
    /// If no kind matcher is specified, returns a wildcard Unknown node.
    fn to_cpg_node_kind(&self) -> CpgNodeKind {
        use std::sync::Arc;

        match &self.kind {
            Some(matcher) => matcher.to_cpg_node_kind(&self.name_pattern),
            None => CpgNodeKind::Unknown {
                kind: Arc::from("pattern_any"),
            },
        }
    }
}

impl NodeKindMatcher {
    /// Converts this matcher to a representative CpgNodeKind.
    fn to_cpg_node_kind(&self, name_pattern: &Option<String>) -> CpgNodeKind {
        use std::sync::Arc;

        match self {
            Self::Exact(tag) => tag.to_cpg_node_kind(name_pattern),
            Self::AnyOf(tags) if !tags.is_empty() => {
                // Use the first tag as a representative
                tags[0].to_cpg_node_kind(name_pattern)
            }
            Self::AnyDeclaration => CpgNodeKind::Unknown {
                kind: Arc::from("pattern_declaration"),
            },
            Self::AnyExpression => CpgNodeKind::Unknown {
                kind: Arc::from("pattern_expression"),
            },
            Self::AnyStatement => CpgNodeKind::Unknown {
                kind: Arc::from("pattern_statement"),
            },
            Self::Any | Self::AnyOf(_) => CpgNodeKind::Unknown {
                kind: Arc::from("pattern_any"),
            },
        }
    }
}

impl NodeKindTag {
    /// Converts this tag to a CpgNodeKind.
    fn to_cpg_node_kind(self, name_pattern: &Option<String>) -> CpgNodeKind {
        use std::sync::Arc;
        use crate::{MethodSignature, Visibility, ScopeId};
        use smallvec::smallvec;

        // Use name pattern if provided, otherwise use a placeholder
        let name: Arc<str> = name_pattern
            .as_ref()
            .map(|s| Arc::from(s.as_str()))
            .unwrap_or_else(|| Arc::from("_"));

        match self {
            Self::Root => CpgNodeKind::Root,
            Self::Module => CpgNodeKind::Module { name },
            Self::Class => CpgNodeKind::Class { name, is_abstract: false },
            Self::Struct => CpgNodeKind::Struct { name },
            Self::Enum => CpgNodeKind::Enum { name },
            Self::Trait => CpgNodeKind::Trait { name },
            Self::Impl => CpgNodeKind::Impl {
                for_type: Some(name),
                trait_name: None,
            },
            Self::Function => CpgNodeKind::Function {
                signature: MethodSignature {
                    name,
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Public,
                },
            },
            Self::Parameter => CpgNodeKind::Parameter {
                name,
                param_type: None,
                is_variadic: false,
            },
            Self::Block => CpgNodeKind::Block { scope: ScopeId::GLOBAL },
            Self::Variable => CpgNodeKind::Variable {
                name,
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: false,
            },
            Self::Field => CpgNodeKind::Field {
                name,
                field_type: None,
                visibility: Visibility::Private,
            },
            Self::Return => CpgNodeKind::Return,
            Self::If => CpgNodeKind::If,
            Self::While => CpgNodeKind::While,
            Self::For => CpgNodeKind::For,
            Self::Loop => CpgNodeKind::Loop,
            Self::Match => CpgNodeKind::Match,
            Self::BinaryOp => CpgNodeKind::BinaryOp { operator: Arc::from("_") },
            Self::UnaryOp => CpgNodeKind::UnaryOp { operator: Arc::from("_") },
            Self::Assignment => CpgNodeKind::Assignment { operator: Arc::from("=") },
            Self::Call => CpgNodeKind::Call { target: None, is_method: false },
            Self::MemberAccess => CpgNodeKind::MemberAccess { member: name },
            Self::IndexAccess => CpgNodeKind::IndexAccess,
            Self::Identifier => CpgNodeKind::Identifier { name, definition: None },
            Self::Literal => CpgNodeKind::Literal { kind: crate::LiteralKind::Null },
            Self::Lambda => CpgNodeKind::Lambda {
                captures: smallvec![],
            },
            Self::Import => CpgNodeKind::Import { path: name },
            Self::Unknown => CpgNodeKind::Unknown { kind: Arc::from("pattern_unknown") },
        }
    }
}

impl EdgeConstraint {
    /// Converts this constraint to a CpgEdgeKind.
    fn to_cpg_edge_kind(&self) -> CpgEdgeKind {
        match &self.kind {
            Some(matcher) => matcher.to_cpg_edge_kind(),
            None => CpgEdgeKind::AstChild, // Default to AST edge
        }
    }
}

impl EdgeKindMatcher {
    /// Converts this matcher to a representative CpgEdgeKind.
    fn to_cpg_edge_kind(&self) -> CpgEdgeKind {
        use crate::graph::CfgEdgeKind;
        use crate::graph::DfgEdgeKind;

        match self {
            Self::AnyAst => CpgEdgeKind::AstChild,
            Self::AnyCfg => CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
            Self::AnyDfg => CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
            Self::AnyCall => CpgEdgeKind::StaticCall,
            Self::Any => CpgEdgeKind::AstChild,
        }
    }
}

/// Constraint on a pattern node.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NodeConstraint {
    /// Index of this node in the pattern.
    pub index: usize,
    /// Required node kind (if any).
    pub kind: Option<NodeKindMatcher>,
    /// Required name pattern (regex).
    pub name_pattern: Option<String>,
    /// Additional property constraints.
    pub properties: FxHashMap<String, String>,
}

impl NodeConstraint {
    /// Creates a new node constraint.
    pub fn new(index: usize) -> Self {
        Self {
            index,
            kind: None,
            name_pattern: None,
            properties: FxHashMap::default(),
        }
    }

    /// Sets the required node kind.
    pub fn with_kind(mut self, kind: NodeKindMatcher) -> Self {
        self.kind = Some(kind);
        self
    }

    /// Sets a name pattern.
    pub fn with_name_pattern(mut self, pattern: impl Into<String>) -> Self {
        self.name_pattern = Some(pattern.into());
        self
    }

    /// Adds a property constraint.
    pub fn with_property(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.properties.insert(key.into(), value.into());
        self
    }
}

/// Matcher for node kinds.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum NodeKindMatcher {
    /// Exact match of a specific kind.
    Exact(NodeKindTag),
    /// Match any of the specified kinds.
    AnyOf(Vec<NodeKindTag>),
    /// Match any declaration.
    AnyDeclaration,
    /// Match any expression.
    AnyExpression,
    /// Match any statement.
    AnyStatement,
    /// Match any node.
    Any,
}

/// Simplified tag for matching node kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum NodeKindTag {
    /// Root node.
    Root,
    /// Module.
    Module,
    /// Class.
    Class,
    /// Struct.
    Struct,
    /// Enum.
    Enum,
    /// Trait/interface.
    Trait,
    /// Implementation block.
    Impl,
    /// Function.
    Function,
    /// Parameter.
    Parameter,
    /// Block.
    Block,
    /// Variable.
    Variable,
    /// Field.
    Field,
    /// Return statement.
    Return,
    /// If statement.
    If,
    /// While loop.
    While,
    /// For loop.
    For,
    /// Loop.
    Loop,
    /// Match/switch.
    Match,
    /// Binary operation.
    BinaryOp,
    /// Unary operation.
    UnaryOp,
    /// Assignment.
    Assignment,
    /// Function call.
    Call,
    /// Member access.
    MemberAccess,
    /// Index access.
    IndexAccess,
    /// Identifier.
    Identifier,
    /// Literal.
    Literal,
    /// Lambda/closure.
    Lambda,
    /// Import.
    Import,
    /// Unknown.
    Unknown,
}

impl NodeKindTag {
    /// Converts a CpgNodeKind to its tag.
    pub fn from_kind(kind: &CpgNodeKind) -> Self {
        match kind {
            CpgNodeKind::Root => Self::Root,
            CpgNodeKind::Module { .. } => Self::Module,
            CpgNodeKind::Class { .. } => Self::Class,
            CpgNodeKind::Struct { .. } => Self::Struct,
            CpgNodeKind::Enum { .. } => Self::Enum,
            CpgNodeKind::Trait { .. } => Self::Trait,
            CpgNodeKind::Impl { .. } => Self::Impl,
            CpgNodeKind::Function { .. } => Self::Function,
            CpgNodeKind::Parameter { .. } => Self::Parameter,
            CpgNodeKind::Block { .. } => Self::Block,
            CpgNodeKind::Variable { .. } => Self::Variable,
            CpgNodeKind::Field { .. } => Self::Field,
            CpgNodeKind::Return => Self::Return,
            CpgNodeKind::If => Self::If,
            CpgNodeKind::While => Self::While,
            CpgNodeKind::For => Self::For,
            CpgNodeKind::Loop => Self::Loop,
            CpgNodeKind::Match => Self::Match,
            CpgNodeKind::BinaryOp { .. } => Self::BinaryOp,
            CpgNodeKind::UnaryOp { .. } => Self::UnaryOp,
            CpgNodeKind::Assignment { .. } => Self::Assignment,
            CpgNodeKind::Call { .. } => Self::Call,
            CpgNodeKind::MemberAccess { .. } => Self::MemberAccess,
            CpgNodeKind::IndexAccess => Self::IndexAccess,
            CpgNodeKind::Identifier { .. } => Self::Identifier,
            CpgNodeKind::Literal { .. } => Self::Literal,
            CpgNodeKind::Lambda { .. } => Self::Lambda,
            CpgNodeKind::Import { .. } => Self::Import,
            _ => Self::Unknown,
        }
    }

    /// Returns true if the tag matches the kind.
    pub fn matches(&self, kind: &CpgNodeKind) -> bool {
        *self == Self::from_kind(kind)
    }
}

impl NodeKindMatcher {
    /// Returns true if this matcher matches the given kind.
    pub fn matches(&self, kind: &CpgNodeKind) -> bool {
        match self {
            Self::Exact(tag) => tag.matches(kind),
            Self::AnyOf(tags) => tags.iter().any(|t| t.matches(kind)),
            Self::AnyDeclaration => matches!(
                kind,
                CpgNodeKind::Module { .. }
                    | CpgNodeKind::Class { .. }
                    | CpgNodeKind::Struct { .. }
                    | CpgNodeKind::Enum { .. }
                    | CpgNodeKind::Trait { .. }
                    | CpgNodeKind::Function { .. }
                    | CpgNodeKind::Variable { .. }
                    | CpgNodeKind::Field { .. }
            ),
            Self::AnyExpression => matches!(
                kind,
                CpgNodeKind::BinaryOp { .. }
                    | CpgNodeKind::UnaryOp { .. }
                    | CpgNodeKind::Call { .. }
                    | CpgNodeKind::MemberAccess { .. }
                    | CpgNodeKind::IndexAccess
                    | CpgNodeKind::Identifier { .. }
                    | CpgNodeKind::Literal { .. }
                    | CpgNodeKind::Lambda { .. }
            ),
            Self::AnyStatement => matches!(
                kind,
                CpgNodeKind::Return
                    | CpgNodeKind::If
                    | CpgNodeKind::While
                    | CpgNodeKind::For
                    | CpgNodeKind::Loop
                    | CpgNodeKind::Match
                    | CpgNodeKind::Break
                    | CpgNodeKind::Continue
            ),
            Self::Any => true,
        }
    }
}

/// Constraint on an edge in a pattern.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct EdgeConstraint {
    /// Source node index.
    pub source: usize,
    /// Target node index.
    pub target: usize,
    /// Required edge kind (if any).
    pub kind: Option<EdgeKindMatcher>,
}

impl EdgeConstraint {
    /// Creates a new edge constraint.
    pub fn new(source: usize, target: usize) -> Self {
        Self {
            source,
            target,
            kind: None,
        }
    }

    /// Sets the required edge kind.
    pub fn with_kind(mut self, kind: EdgeKindMatcher) -> Self {
        self.kind = Some(kind);
        self
    }
}

/// Matcher for edge kinds.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum EdgeKindMatcher {
    /// Any AST edge.
    AnyAst,
    /// Any CFG edge.
    AnyCfg,
    /// Any DFG edge.
    AnyDfg,
    /// Any call graph edge.
    AnyCall,
    /// Any edge.
    Any,
}

impl EdgeKindMatcher {
    /// Returns true if this matcher matches the given edge kind.
    pub fn matches(&self, kind: &CpgEdgeKind) -> bool {
        match self {
            Self::AnyAst => kind.is_ast(),
            Self::AnyCfg => kind.is_cfg(),
            Self::AnyDfg => kind.is_dfg(),
            Self::AnyCall => kind.is_call(),
            Self::Any => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pattern_match_creation() {
        let pm = PatternMatch::new("TestPattern", NodeId::new(1), 0.95)
            .with_mapping(NodeId::new(0), NodeId::new(10))
            .with_mapping(NodeId::new(1), NodeId::new(11))
            .with_metadata("category", "structural");

        assert_eq!(pm.pattern_name, "TestPattern");
        assert_eq!(pm.confidence, 0.95);
        assert_eq!(pm.match_size(), 2);
        assert_eq!(pm.metadata.get("category"), Some(&"structural".to_string()));
    }

    #[test]
    fn test_node_kind_matcher() {
        use std::sync::Arc;

        let class_kind = CpgNodeKind::Class {
            name: Arc::from("MyClass"),
            is_abstract: false,
        };

        assert!(NodeKindMatcher::Exact(NodeKindTag::Class).matches(&class_kind));
        assert!(NodeKindMatcher::AnyDeclaration.matches(&class_kind));
        assert!(!NodeKindMatcher::AnyExpression.matches(&class_kind));
        assert!(NodeKindMatcher::Any.matches(&class_kind));
    }

    #[test]
    fn test_pattern_template() {
        let template = PatternTemplate::new("Singleton", "Singleton design pattern")
            .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
            .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Field)))
            .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
            .with_min_confidence(0.9);

        assert_eq!(template.name, "Singleton");
        assert_eq!(template.node_constraints.len(), 2);
        assert_eq!(template.edge_constraints.len(), 1);
        assert_eq!(template.min_confidence, 0.9);
    }

    #[test]
    fn test_to_pattern_graph() {
        // Create a template with 3 nodes and 2 edges
        let template = PatternTemplate::new("TestPattern", "Test pattern")
            .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
            .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
            .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::Exact(NodeKindTag::Field)))
            .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
            .with_edge(EdgeConstraint::new(0, 2).with_kind(EdgeKindMatcher::AnyAst));

        let cpg = template.to_pattern_graph();

        // Verify node count
        assert_eq!(cpg.node_count(), 3, "Expected 3 nodes in pattern graph");

        // Verify edge count (the CPG stores edges in adjacency lists)
        let mut edge_count = 0;
        for node_id in cpg.node_ids() {
            edge_count += cpg.outgoing_edges(node_id).count();
        }
        assert_eq!(edge_count, 2, "Expected 2 edges in pattern graph");

        // Verify node kinds are correctly converted
        let mut has_class = false;
        let mut has_function = false;
        let mut has_field = false;

        for node in cpg.nodes() {
            match &node.kind {
                CpgNodeKind::Class { .. } => has_class = true,
                CpgNodeKind::Function { .. } => has_function = true,
                CpgNodeKind::Field { .. } => has_field = true,
                _ => {}
            }
        }

        assert!(has_class, "Expected a Class node");
        assert!(has_function, "Expected a Function node");
        assert!(has_field, "Expected a Field node");
    }

    // ==================== added coverage: example-based ====================

    fn func_sig(name: &str) -> crate::MethodSignature {
        crate::MethodSignature {
            name: name.into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: crate::Visibility::Public,
        }
    }

    /// One representative `CpgNodeKind` per `NodeKindTag`, plus several kinds
    /// that deliberately fold onto `NodeKindTag::Unknown` (the `_` arm of
    /// `from_kind`). Exercises every tag category.
    pub(super) fn representative_kinds() -> Vec<(CpgNodeKind, NodeKindTag)> {
        use std::sync::Arc;
        use crate::{ScopeId, Visibility, LiteralKind};
        vec![
            (CpgNodeKind::Root, NodeKindTag::Root),
            (CpgNodeKind::Module { name: Arc::from("m") }, NodeKindTag::Module),
            (CpgNodeKind::Class { name: Arc::from("C"), is_abstract: false }, NodeKindTag::Class),
            (CpgNodeKind::Struct { name: Arc::from("S") }, NodeKindTag::Struct),
            (CpgNodeKind::Enum { name: Arc::from("E") }, NodeKindTag::Enum),
            (CpgNodeKind::Trait { name: Arc::from("T") }, NodeKindTag::Trait),
            (CpgNodeKind::Impl { for_type: Some(Arc::from("C")), trait_name: None }, NodeKindTag::Impl),
            (CpgNodeKind::Function { signature: func_sig("f") }, NodeKindTag::Function),
            (CpgNodeKind::Parameter { name: Arc::from("p"), param_type: None, is_variadic: false }, NodeKindTag::Parameter),
            (CpgNodeKind::Block { scope: ScopeId::GLOBAL }, NodeKindTag::Block),
            (CpgNodeKind::Variable { name: Arc::from("v"), var_type: None, scope: ScopeId::GLOBAL, is_mutable: false }, NodeKindTag::Variable),
            (CpgNodeKind::Field { name: Arc::from("fld"), field_type: None, visibility: Visibility::Private }, NodeKindTag::Field),
            (CpgNodeKind::Return, NodeKindTag::Return),
            (CpgNodeKind::If, NodeKindTag::If),
            (CpgNodeKind::While, NodeKindTag::While),
            (CpgNodeKind::For, NodeKindTag::For),
            (CpgNodeKind::Loop, NodeKindTag::Loop),
            (CpgNodeKind::Match, NodeKindTag::Match),
            (CpgNodeKind::BinaryOp { operator: Arc::from("+") }, NodeKindTag::BinaryOp),
            (CpgNodeKind::UnaryOp { operator: Arc::from("!") }, NodeKindTag::UnaryOp),
            (CpgNodeKind::Assignment { operator: Arc::from("=") }, NodeKindTag::Assignment),
            (CpgNodeKind::Call { target: None, is_method: false }, NodeKindTag::Call),
            (CpgNodeKind::MemberAccess { member: Arc::from("x") }, NodeKindTag::MemberAccess),
            (CpgNodeKind::IndexAccess, NodeKindTag::IndexAccess),
            (CpgNodeKind::Identifier { name: Arc::from("id"), definition: None }, NodeKindTag::Identifier),
            (CpgNodeKind::Literal { kind: LiteralKind::Null }, NodeKindTag::Literal),
            (CpgNodeKind::Lambda { captures: Default::default() }, NodeKindTag::Lambda),
            (CpgNodeKind::Import { path: Arc::from("std") }, NodeKindTag::Import),
            // Kinds without a dedicated tag collapse onto `Unknown`.
            (CpgNodeKind::Comment { is_doc: false }, NodeKindTag::Unknown),
            (CpgNodeKind::Break, NodeKindTag::Unknown),
            (CpgNodeKind::Error { message: Arc::from("e") }, NodeKindTag::Unknown),
            (CpgNodeKind::Unknown { kind: Arc::from("x") }, NodeKindTag::Unknown),
        ]
    }

    #[test]
    fn test_node_kind_tag_from_kind_and_matches_all_categories() {
        for (kind, expected) in representative_kinds() {
            assert_eq!(NodeKindTag::from_kind(&kind), expected, "from_kind({kind:?})");
            // `matches` agrees with `from_kind`.
            assert!(expected.matches(&kind), "{expected:?}.matches({kind:?})");
            // A tag different from the kind's own tag must not match it.
            let other = if expected == NodeKindTag::Root { NodeKindTag::If } else { NodeKindTag::Root };
            assert!(!other.matches(&kind), "{other:?} should not match {kind:?}");
        }
    }

    #[test]
    fn test_node_kind_matcher_all_variants() {
        use std::sync::Arc;
        use crate::ScopeId;
        let class = CpgNodeKind::Class { name: Arc::from("C"), is_abstract: false };
        let func = CpgNodeKind::Function { signature: func_sig("f") };
        let var = CpgNodeKind::Variable { name: Arc::from("v"), var_type: None, scope: ScopeId::GLOBAL, is_mutable: false };
        let call = CpgNodeKind::Call { target: None, is_method: false };
        let lit = CpgNodeKind::Literal { kind: crate::LiteralKind::Null };
        let if_kind = CpgNodeKind::If;
        let ret = CpgNodeKind::Return;

        // Exact
        assert!(NodeKindMatcher::Exact(NodeKindTag::Function).matches(&func));
        assert!(!NodeKindMatcher::Exact(NodeKindTag::Function).matches(&class));

        // AnyOf
        let any_of = NodeKindMatcher::AnyOf(vec![NodeKindTag::Call, NodeKindTag::Literal]);
        assert!(any_of.matches(&call));
        assert!(any_of.matches(&lit));
        assert!(!any_of.matches(&class));
        assert!(!NodeKindMatcher::AnyOf(vec![]).matches(&class), "empty AnyOf matches nothing");

        // AnyDeclaration
        let decl = NodeKindMatcher::AnyDeclaration;
        assert!(decl.matches(&class));
        assert!(decl.matches(&func));
        assert!(decl.matches(&var));
        assert!(!decl.matches(&call));
        assert!(!decl.matches(&if_kind));

        // AnyExpression
        let expr = NodeKindMatcher::AnyExpression;
        assert!(expr.matches(&call));
        assert!(expr.matches(&lit));
        assert!(!expr.matches(&class));
        assert!(!expr.matches(&ret));

        // AnyStatement
        let stmt = NodeKindMatcher::AnyStatement;
        assert!(stmt.matches(&if_kind));
        assert!(stmt.matches(&ret));
        assert!(!stmt.matches(&class));
        assert!(!stmt.matches(&call));

        // Any
        let any = NodeKindMatcher::Any;
        for k in [&class, &func, &var, &call, &lit, &if_kind, &ret] {
            assert!(any.matches(k));
        }
    }

    #[test]
    fn test_edge_kind_matcher_all_variants() {
        use crate::{CfgEdgeKind, DfgEdgeKind};
        let ast = CpgEdgeKind::AstChild;
        let cfg = CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential);
        let dfg = CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse);
        let call = CpgEdgeKind::StaticCall;
        let dyn_call = CpgEdgeKind::DynamicCall;
        let call_site = CpgEdgeKind::CallSite;

        assert!(EdgeKindMatcher::AnyAst.matches(&ast));
        assert!(!EdgeKindMatcher::AnyAst.matches(&cfg));

        assert!(EdgeKindMatcher::AnyCfg.matches(&cfg));
        assert!(!EdgeKindMatcher::AnyCfg.matches(&ast));

        assert!(EdgeKindMatcher::AnyDfg.matches(&dfg));
        assert!(!EdgeKindMatcher::AnyDfg.matches(&cfg));

        assert!(EdgeKindMatcher::AnyCall.matches(&call));
        assert!(EdgeKindMatcher::AnyCall.matches(&dyn_call));
        assert!(EdgeKindMatcher::AnyCall.matches(&call_site));
        assert!(!EdgeKindMatcher::AnyCall.matches(&ast));

        for e in [&ast, &cfg, &dfg, &call, &dyn_call, &call_site] {
            assert!(EdgeKindMatcher::Any.matches(e));
        }
    }

    #[test]
    fn test_pattern_match_matched_nodes() {
        let pm = PatternMatch::new("P", NodeId::new(100), 1.0)
            .with_mapping(NodeId::new(0), NodeId::new(10))
            .with_mapping(NodeId::new(1), NodeId::new(11))
            .with_mapping(NodeId::new(2), NodeId::new(12));

        let got: std::collections::HashSet<u32> = pm.matched_nodes().map(|n| n.as_u32()).collect();
        let expected: std::collections::HashSet<u32> = [10, 11, 12].into_iter().collect();
        assert_eq!(got, expected);
        assert_eq!(pm.matched_nodes().count(), 3);
        assert_eq!(pm.match_size(), 3);
    }

    #[test]
    fn test_node_constraint_builders() {
        let c = NodeConstraint::new(3)
            .with_kind(NodeKindMatcher::Exact(NodeKindTag::Function))
            .with_name_pattern("run.*")
            .with_property("visibility", "public")
            .with_property("static", "true");

        assert_eq!(c.index, 3);
        assert!(matches!(c.kind, Some(NodeKindMatcher::Exact(NodeKindTag::Function))));
        assert_eq!(c.name_pattern.as_deref(), Some("run.*"));
        assert_eq!(c.properties.get("visibility").map(|s| s.as_str()), Some("public"));
        assert_eq!(c.properties.get("static").map(|s| s.as_str()), Some("true"));
        assert_eq!(c.properties.len(), 2);

        // Defaults on a bare constraint.
        let bare = NodeConstraint::new(0);
        assert!(bare.kind.is_none());
        assert!(bare.name_pattern.is_none());
        assert!(bare.properties.is_empty());
    }

    #[test]
    fn test_edge_constraint_builders() {
        let e = EdgeConstraint::new(2, 5).with_kind(EdgeKindMatcher::AnyDfg);
        assert_eq!(e.source, 2);
        assert_eq!(e.target, 5);
        assert!(matches!(e.kind, Some(EdgeKindMatcher::AnyDfg)));

        let e2 = EdgeConstraint::new(0, 1);
        assert!(e2.kind.is_none(), "default edge kind is None");
    }

    #[test]
    fn test_to_pattern_graph_name_flow_and_dangling_edge() {
        let template = PatternTemplate::new("T", "desc")
            .with_node(
                NodeConstraint::new(0)
                    .with_kind(NodeKindMatcher::Exact(NodeKindTag::Function))
                    .with_name_pattern("run"),
            )
            .with_node(NodeConstraint::new(1)) // no kind -> wildcard `pattern_any`
            .with_node(NodeConstraint::new(2).with_kind(NodeKindMatcher::AnyExpression))
            .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
            .with_edge(EdgeConstraint::new(0, 9)); // index 9 does not exist -> skipped

        let cpg = template.to_pattern_graph();
        assert_eq!(cpg.node_count(), 3);

        // Only the (0 -> 1) edge survives; the dangling (0 -> 9) is skipped.
        let edge_count: usize = cpg.node_ids().map(|id| cpg.outgoing_edges(id).count()).sum();
        assert_eq!(edge_count, 1, "dangling edge constraint must be skipped");

        // The name pattern flows into the Function node's signature name.
        let func = cpg
            .nodes()
            .find(|n| matches!(n.kind, CpgNodeKind::Function { .. }))
            .expect("a Function node");
        assert_eq!(func.name(), Some("run"));

        // A constraint with no kind matcher becomes a wildcard `Unknown`.
        assert!(cpg.nodes().any(|n| matches!(&n.kind, CpgNodeKind::Unknown { kind } if &**kind == "pattern_any")));
        // `AnyExpression` becomes `Unknown{"pattern_expression"}`.
        assert!(cpg.nodes().any(|n| matches!(&n.kind, CpgNodeKind::Unknown { kind } if &**kind == "pattern_expression")));
    }

    #[test]
    fn test_subgraph_matcher_default_methods() {
        use crate::{CpgNode, SourceRange, Language};
        use std::sync::Arc;

        // pattern: a single `If` node.
        let mut pattern = CodePropertyGraph::new(Language::Rust);
        pattern.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::If, SourceRange::default()));

        // target: two `If` nodes and a `While`.
        let mut target = CodePropertyGraph::new(Language::Rust);
        target.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::If, SourceRange::default()));
        target.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::If, SourceRange::default()));
        target.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::While, SourceRange::default()));

        let matcher = Vf2Matcher::new().with_strict_kinds(true);

        // Both `If` nodes match.
        assert_eq!(matcher.find_matches(&pattern, &target).len(), 2);

        // `find_matches_limited` (default trait method) truncates to `limit`.
        assert_eq!(matcher.find_matches_limited(&pattern, &target, 1).len(), 1);
        assert!(matcher.find_matches_limited(&pattern, &target, 0).is_empty());
        assert_eq!(matcher.find_matches_limited(&pattern, &target, 10).len(), 2);

        // `contains_pattern` (default trait method) is true when present.
        assert!(matcher.contains_pattern(&pattern, &target));

        // An absent pattern (a `Class` node) is not contained.
        let mut absent = CodePropertyGraph::new(Language::Rust);
        absent.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Class { name: Arc::from("C"), is_abstract: false },
            SourceRange::default(),
        ));
        assert!(!matcher.contains_pattern(&absent, &target));
        assert!(matcher.find_matches_limited(&absent, &target, 5).is_empty());
    }
}

/// Materialization: a [`PatternTemplate`] is a *declarative* description, and
/// [`PatternTemplate::to_pattern_graph`] turns it into the concrete
/// [`CodePropertyGraph`] the matchers consume. These tests pin that
/// translation — every node tag, every matcher kind, every edge matcher, and
/// the treatment of dangling edge constraints.
#[cfg(test)]
mod materialization {
    use super::*;
    use crate::{CfgEdgeKind, DfgEdgeKind};

    /// Every `NodeKindTag` round-trips through materialization: classifying the
    /// node a tag materializes to recovers exactly that tag
    /// (`from_kind ∘ to_cpg_node_kind = id`). This is what lets a template and
    /// a real CPG be compared tag-wise by the matchers.
    #[test]
    fn every_tag_round_trips_through_the_pattern_graph() {
        // The distinct tags of the representative table (`Unknown` repeats).
        let mut tags: Vec<NodeKindTag> = super::tests::representative_kinds()
            .into_iter()
            .map(|(_, tag)| tag)
            .collect();
        tags.dedup();

        for tag in tags {
            let template = PatternTemplate::new("t", "d")
                .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(tag)));
            let graph = template.to_pattern_graph();

            assert_eq!(graph.node_count(), 1, "one constraint ⇒ one node");
            let kind = &graph.nodes().next().expect("the materialized node").kind.clone();
            assert_eq!(
                NodeKindTag::from_kind(kind),
                tag,
                "materializing {tag:?} produced {kind:?}, which classifies differently"
            );
            // The tag also *matches* the node it materializes.
            assert!(tag.matches(kind));
        }
    }

    /// A name pattern is threaded into every name-bearing materialized kind;
    /// without one, a placeholder is used.
    #[test]
    fn name_pattern_is_threaded_into_named_kinds() {
        let named = PatternTemplate::new("t", "d")
            .with_node(
                NodeConstraint::new(0)
                    .with_kind(NodeKindMatcher::Exact(NodeKindTag::Class))
                    .with_name_pattern("Widget"),
            )
            .to_pattern_graph();
        match &named.nodes().next().expect("node").kind {
            CpgNodeKind::Class { name, is_abstract } => {
                assert_eq!(&**name, "Widget");
                assert!(!is_abstract);
            }
            other => panic!("expected Class, got {other:?}"),
        }

        let anonymous = PatternTemplate::new("t", "d")
            .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
            .to_pattern_graph();
        match &anonymous.nodes().next().expect("node").kind {
            CpgNodeKind::Class { name, .. } => assert_eq!(&**name, "_", "placeholder name"),
            other => panic!("expected Class, got {other:?}"),
        }

        // Name-bearing kinds beyond `Class`: the pattern reaches each of them.
        for (tag, extract) in [
            (
                NodeKindTag::Module,
                (|k: &CpgNodeKind| match k {
                    CpgNodeKind::Module { name } => Some(name.to_string()),
                    _ => None,
                }) as fn(&CpgNodeKind) -> Option<String>,
            ),
            (NodeKindTag::Function, |k| match k {
                CpgNodeKind::Function { signature } => Some(signature.name.to_string()),
                _ => None,
            }),
            (NodeKindTag::Impl, |k| match k {
                CpgNodeKind::Impl { for_type, .. } => for_type.as_ref().map(|t| t.to_string()),
                _ => None,
            }),
            (NodeKindTag::Identifier, |k| match k {
                CpgNodeKind::Identifier { name, .. } => Some(name.to_string()),
                _ => None,
            }),
            (NodeKindTag::Import, |k| match k {
                CpgNodeKind::Import { path } => Some(path.to_string()),
                _ => None,
            }),
            (NodeKindTag::MemberAccess, |k| match k {
                CpgNodeKind::MemberAccess { member } => Some(member.to_string()),
                _ => None,
            }),
        ] {
            let g = PatternTemplate::new("t", "d")
                .with_node(
                    NodeConstraint::new(0)
                        .with_kind(NodeKindMatcher::Exact(tag))
                        .with_name_pattern("Widget"),
                )
                .to_pattern_graph();
            let kind = g.nodes().next().expect("node").kind.clone();
            assert_eq!(
                extract(&kind).as_deref(),
                Some("Widget"),
                "{tag:?} must carry the name pattern"
            );
        }
    }

    /// The non-`Exact` matchers materialize to their wildcard placeholders, and
    /// a constraint with no kind at all is the universal wildcard.
    #[test]
    fn wildcard_matchers_materialize_to_placeholders() {
        let placeholder = |matcher: Option<NodeKindMatcher>| {
            let mut c = NodeConstraint::new(0);
            if let Some(m) = matcher {
                c = c.with_kind(m);
            }
            let g = PatternTemplate::new("t", "d").with_node(c).to_pattern_graph();
            let kind = g.nodes().next().expect("node").kind.clone();
            match kind {
                CpgNodeKind::Unknown { kind } => kind.to_string(),
                other => panic!("expected a wildcard Unknown, got {other:?}"),
            }
        };

        assert_eq!(placeholder(None), "pattern_any", "no kind ⇒ match anything");
        assert_eq!(placeholder(Some(NodeKindMatcher::Any)), "pattern_any");
        assert_eq!(
            placeholder(Some(NodeKindMatcher::AnyDeclaration)),
            "pattern_declaration"
        );
        assert_eq!(
            placeholder(Some(NodeKindMatcher::AnyExpression)),
            "pattern_expression"
        );
        assert_eq!(
            placeholder(Some(NodeKindMatcher::AnyStatement)),
            "pattern_statement"
        );
        // An empty alternation has no representative, so it degrades to `any`.
        assert_eq!(placeholder(Some(NodeKindMatcher::AnyOf(vec![]))), "pattern_any");
        // `Unknown` is a tag in its own right, distinct from the wildcards.
        assert_eq!(
            placeholder(Some(NodeKindMatcher::Exact(NodeKindTag::Unknown))),
            "pattern_unknown"
        );
    }

    /// A non-empty alternation materializes as its first alternative — the
    /// representative the matcher then generalizes from.
    #[test]
    fn alternation_materializes_as_its_first_alternative() {
        let g = PatternTemplate::new("t", "d")
            .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::AnyOf(vec![
                NodeKindTag::While,
                NodeKindTag::For,
            ])))
            .to_pattern_graph();
        assert!(matches!(
            g.nodes().next().expect("node").kind,
            CpgNodeKind::While
        ));
    }

    /// Every `EdgeKindMatcher` materializes to its representative edge kind,
    /// and an edge constraint without a matcher defaults to the AST edge.
    #[test]
    fn edge_matchers_materialize_to_representative_edges() {
        let build = |matcher: Option<EdgeKindMatcher>| {
            let mut e = EdgeConstraint::new(0, 1);
            if let Some(m) = matcher {
                e = e.with_kind(m);
            }
            let g = PatternTemplate::new("t", "d")
                .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
                .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Block)))
                .with_edge(e)
                .to_pattern_graph();
            assert_eq!(g.edge_count(), 1, "one constraint ⇒ one edge");
            let kind = g.edges().next().expect("the materialized edge").kind.clone();
            kind
        };

        assert_eq!(build(None), CpgEdgeKind::AstChild, "default is the AST edge");
        assert_eq!(build(Some(EdgeKindMatcher::Any)), CpgEdgeKind::AstChild);
        assert_eq!(build(Some(EdgeKindMatcher::AnyAst)), CpgEdgeKind::AstChild);
        assert_eq!(
            build(Some(EdgeKindMatcher::AnyCfg)),
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential)
        );
        assert_eq!(
            build(Some(EdgeKindMatcher::AnyDfg)),
            CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse)
        );
        assert_eq!(build(Some(EdgeKindMatcher::AnyCall)), CpgEdgeKind::StaticCall);
    }

    /// An edge constraint naming an index with no node constraint is dropped
    /// rather than fabricating a node — the template stays well-formed even
    /// when it is written incorrectly.
    #[test]
    fn dangling_edge_constraints_are_dropped() {
        let g = PatternTemplate::new("t", "d")
            .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Function)))
            // 7 has no node constraint: dangling on the target …
            .with_edge(EdgeConstraint::new(0, 7))
            // … and on the source.
            .with_edge(EdgeConstraint::new(7, 0))
            .to_pattern_graph();

        assert_eq!(g.node_count(), 1, "no node is fabricated for index 7");
        assert_eq!(g.edge_count(), 0, "both dangling constraints are dropped");
    }

    /// The materialized graph is language-agnostic, and its node order follows
    /// the constraint order (so `EdgeConstraint` indices address the intended
    /// nodes even when the indices are not contiguous).
    #[test]
    fn materialized_graph_is_language_agnostic_and_order_preserving() {
        let template = PatternTemplate::new("t", "d")
            .with_node(NodeConstraint::new(10).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
            .with_node(NodeConstraint::new(20).with_kind(NodeKindMatcher::Exact(NodeKindTag::Field)))
            .with_edge(
                EdgeConstraint::new(10, 20).with_kind(EdgeKindMatcher::AnyAst),
            );
        let g = template.to_pattern_graph();

        assert_eq!(g.language(), crate::Language::Unknown);
        let kinds: Vec<_> = g.nodes().map(|n| NodeKindTag::from_kind(&n.kind)).collect();
        assert_eq!(kinds, vec![NodeKindTag::Class, NodeKindTag::Field]);

        let edge = g.edges().next().expect("edge");
        let src = g.node(edge.source).expect("source node");
        let tgt = g.node(edge.target).expect("target node");
        assert_eq!(NodeKindTag::from_kind(&src.kind), NodeKindTag::Class);
        assert_eq!(NodeKindTag::from_kind(&tgt.kind), NodeKindTag::Field);
    }
}
