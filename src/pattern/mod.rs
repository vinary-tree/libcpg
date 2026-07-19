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
    fn to_cpg_node_kind(&self, name_pattern: &Option<String>) -> CpgNodeKind {
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
}
