//! CPG node types and related structures.

use std::sync::Arc;
use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use text_size::TextRange;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "serde")]
use super::serde_util::{arc_str, option_arc_str, smallvec_arc_str_2};

/// Unique identifier for a node in the CPG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NodeId(pub u32);

impl NodeId {
    /// Creates a new node ID.
    #[inline]
    pub fn new(id: u32) -> Self {
        Self(id)
    }

    /// Returns the raw ID value.
    #[inline]
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

impl From<u32> for NodeId {
    fn from(id: u32) -> Self {
        Self(id)
    }
}

impl From<NodeId> for u32 {
    fn from(id: NodeId) -> Self {
        id.0
    }
}

/// Source code location range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SourceRange {
    /// Start byte offset.
    pub start: u32,
    /// End byte offset (exclusive).
    pub end: u32,
    /// Start line (0-indexed).
    pub start_line: u32,
    /// Start column (0-indexed).
    pub start_col: u32,
    /// End line (0-indexed).
    pub end_line: u32,
    /// End column (0-indexed).
    pub end_col: u32,
}

impl SourceRange {
    /// Creates a new source range.
    pub fn new(
        start: u32,
        end: u32,
        start_line: u32,
        start_col: u32,
        end_line: u32,
        end_col: u32,
    ) -> Self {
        Self {
            start,
            end,
            start_line,
            start_col,
            end_line,
            end_col,
        }
    }

    /// Creates a source range from byte offsets only.
    pub fn from_bytes(start: u32, end: u32) -> Self {
        Self {
            start,
            end,
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 0,
        }
    }

    /// Returns the byte length of this range.
    #[inline]
    pub fn len(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    /// Returns true if this range is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.start >= self.end
    }

    /// Converts to a text_size TextRange.
    pub fn to_text_range(&self) -> TextRange {
        TextRange::new(self.start.into(), self.end.into())
    }
}

impl Default for SourceRange {
    fn default() -> Self {
        Self {
            start: 0,
            end: 0,
            start_line: 0,
            start_col: 0,
            end_line: 0,
            end_col: 0,
        }
    }
}

/// Property key for node metadata.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum PropertyKey {
    /// Node name/label from AST.
    Name,
    /// Type information.
    Type,
    /// Scope identifier.
    Scope,
    /// Visibility modifier.
    Visibility,
    /// Whether the node is mutable.
    Mutable,
    /// Whether the node is static.
    Static,
    /// Whether the node is async.
    Async,
    /// Custom property key.
    Custom(#[cfg_attr(feature = "serde", serde(with = "arc_str"))] Arc<str>),
}

/// Property value for node metadata.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum PropertyValue {
    /// String value.
    String(#[cfg_attr(feature = "serde", serde(with = "arc_str"))] Arc<str>),
    /// Integer value.
    Int(i64),
    /// Unsigned integer value.
    Uint(u64),
    /// Boolean value.
    Bool(bool),
    /// Float value.
    Float(f64),
    /// List of values.
    List(Vec<PropertyValue>),
    /// Null/absent value.
    Null,
}

impl PropertyValue {
    /// Returns the value as a string reference if it is a string.
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    /// Returns the value as an integer if it is an integer.
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            _ => None,
        }
    }

    /// Returns the value as a boolean if it is a boolean.
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }
}

/// Type information for a node.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TypeInfo {
    /// The type name.
    #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
    pub name: Arc<str>,
    /// Whether this is a reference type.
    pub is_reference: bool,
    /// Whether this is mutable.
    pub is_mutable: bool,
    /// Generic type parameters.
    #[cfg_attr(feature = "serde", serde(with = "smallvec_arc_str_2"))]
    pub generics: SmallVec<[Arc<str>; 2]>,
}

impl TypeInfo {
    /// Creates a new type info with the given name.
    pub fn new(name: impl Into<Arc<str>>) -> Self {
        Self {
            name: name.into(),
            is_reference: false,
            is_mutable: false,
            generics: SmallVec::new(),
        }
    }

    /// Sets whether this is a reference type.
    pub fn with_reference(mut self, is_ref: bool) -> Self {
        self.is_reference = is_ref;
        self
    }

    /// Sets whether this is mutable.
    pub fn with_mutable(mut self, is_mut: bool) -> Self {
        self.is_mutable = is_mut;
        self
    }

    /// Adds a generic type parameter.
    pub fn with_generic(mut self, generic: impl Into<Arc<str>>) -> Self {
        self.generics.push(generic.into());
        self
    }
}

/// Method signature information.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MethodSignature {
    /// Method name.
    #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
    pub name: Arc<str>,
    /// Parameter types.
    pub params: SmallVec<[TypeInfo; 4]>,
    /// Return type.
    pub return_type: Option<TypeInfo>,
    /// Whether the method is static.
    pub is_static: bool,
    /// Whether the method is async.
    pub is_async: bool,
    /// Visibility.
    pub visibility: Visibility,
}

/// Visibility modifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Visibility {
    /// Public visibility.
    Public,
    /// Private visibility.
    #[default]
    Private,
    /// Protected visibility (for languages that support it).
    Protected,
    /// Package/module visibility.
    Package,
    /// Crate visibility (Rust-specific).
    Crate,
}

/// Scope identifier for tracking lexical scopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ScopeId(pub u32);

impl ScopeId {
    /// The global scope.
    pub const GLOBAL: Self = Self(0);

    /// Creates a new scope ID.
    pub fn new(id: u32) -> Self {
        Self(id)
    }
}

/// Kind of CPG node.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CpgNodeKind {
    // === Structural nodes ===
    /// Root of the AST / compilation unit.
    Root,
    /// Module or namespace.
    Module {
        /// Module name.
        #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
        name: Arc<str>,
    },
    /// Class definition.
    Class {
        /// Class name.
        #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
        name: Arc<str>,
        /// Whether this is abstract.
        is_abstract: bool,
    },
    /// Struct definition.
    Struct {
        /// Struct name.
        #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
        name: Arc<str>,
    },
    /// Enum definition.
    Enum {
        /// Enum name.
        #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
        name: Arc<str>,
    },
    /// Trait/interface definition.
    Trait {
        /// Trait name.
        #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
        name: Arc<str>,
    },
    /// Implementation block.
    Impl {
        /// Type being implemented for.
        #[cfg_attr(feature = "serde", serde(with = "option_arc_str"))]
        for_type: Option<Arc<str>>,
        /// Trait being implemented.
        #[cfg_attr(feature = "serde", serde(with = "option_arc_str"))]
        trait_name: Option<Arc<str>>,
    },

    // === Function-level nodes ===
    /// Function or method definition.
    Function {
        /// Function signature.
        signature: MethodSignature,
    },
    /// Function parameter.
    Parameter {
        /// Parameter name.
        #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
        name: Arc<str>,
        /// Parameter type.
        param_type: Option<TypeInfo>,
        /// Whether this is a rest/variadic parameter.
        is_variadic: bool,
    },
    /// Code block.
    Block {
        /// Scope ID for this block.
        scope: ScopeId,
    },

    // === Variable nodes ===
    /// Variable declaration.
    Variable {
        /// Variable name.
        #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
        name: Arc<str>,
        /// Variable type.
        var_type: Option<TypeInfo>,
        /// Scope where this variable is defined.
        scope: ScopeId,
        /// Whether this is mutable.
        is_mutable: bool,
    },
    /// Field declaration in a struct/class.
    Field {
        /// Field name.
        #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
        name: Arc<str>,
        /// Field type.
        field_type: Option<TypeInfo>,
        /// Visibility.
        visibility: Visibility,
    },

    // === Statement nodes ===
    /// Return statement.
    Return,
    /// If statement.
    If,
    /// Else branch.
    Else,
    /// While loop.
    While,
    /// For loop.
    For,
    /// Loop (infinite or with break).
    Loop,
    /// Match/switch statement.
    Match,
    /// Match arm/case.
    MatchArm,
    /// Break statement.
    Break,
    /// Continue statement.
    Continue,
    /// Throw/raise statement.
    Throw,
    /// Try block.
    Try,
    /// Catch/except block.
    Catch,
    /// Finally block.
    Finally,

    // === Expression nodes ===
    /// Binary operation.
    BinaryOp {
        /// The operator.
        #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
        operator: Arc<str>,
    },
    /// Unary operation.
    UnaryOp {
        /// The operator.
        #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
        operator: Arc<str>,
    },
    /// Assignment.
    Assignment {
        /// The assignment operator (=, +=, etc.).
        #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
        operator: Arc<str>,
    },
    /// Function/method call.
    Call {
        /// Target function/method (if statically known).
        target: Option<NodeId>,
        /// Whether this is a method call.
        is_method: bool,
    },
    /// Member access (field or method).
    MemberAccess {
        /// Member name.
        #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
        member: Arc<str>,
    },
    /// Array/index access.
    IndexAccess,
    /// Identifier reference.
    Identifier {
        /// The identifier name.
        #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
        name: Arc<str>,
        /// The definition this refers to (if resolved).
        definition: Option<NodeId>,
    },
    /// Literal value.
    Literal {
        /// The literal kind.
        kind: LiteralKind,
    },
    /// Lambda/closure expression.
    Lambda {
        /// Captured variables.
        captures: SmallVec<[NodeId; 4]>,
    },
    /// Await expression.
    Await,
    /// Yield expression.
    Yield,

    // === Type nodes ===
    /// Type annotation.
    TypeAnnotation {
        /// The type.
        type_info: TypeInfo,
    },
    /// Generic type parameter.
    GenericParam {
        /// Parameter name.
        #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
        name: Arc<str>,
    },

    // === Special nodes ===
    /// Comment.
    Comment {
        /// Whether this is a doc comment.
        is_doc: bool,
    },
    /// Import/use statement.
    Import {
        /// The import path.
        #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
        path: Arc<str>,
    },
    /// Attribute/decorator.
    Attribute {
        /// Attribute name.
        #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
        name: Arc<str>,
    },
    /// Macro invocation.
    Macro {
        /// Macro name.
        #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
        name: Arc<str>,
    },
    /// Error node (from parser error recovery).
    Error {
        /// Error message.
        #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
        message: Arc<str>,
    },
    /// Unknown/other node kind.
    Unknown {
        /// The original node kind string.
        #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
        kind: Arc<str>,
    },
}

/// Literal value kinds.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum LiteralKind {
    /// Integer literal.
    Integer(i64),
    /// Float literal.
    Float(f64),
    /// String literal.
    String(#[cfg_attr(feature = "serde", serde(with = "arc_str"))] Arc<str>),
    /// Character literal.
    Char(char),
    /// Boolean literal.
    Bool(bool),
    /// Null/nil literal.
    Null,
    /// Array/list literal.
    Array,
    /// Object/map literal.
    Object,
    /// Regex literal.
    Regex(#[cfg_attr(feature = "serde", serde(with = "arc_str"))] Arc<str>),
}

/// A node in the Code Property Graph.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CpgNode {
    /// Unique node identifier.
    pub id: NodeId,
    /// Node kind with associated data.
    pub kind: CpgNodeKind,
    /// Source code range.
    pub range: SourceRange,
    /// Original source text (optional, for terminals).
    #[cfg_attr(feature = "serde", serde(with = "option_arc_str"))]
    pub text: Option<Arc<str>>,
    /// Additional properties.
    pub properties: FxHashMap<PropertyKey, PropertyValue>,
    /// AST children (indices into the node array).
    pub children: SmallVec<[NodeId; 4]>,
    /// Parent node (if any).
    pub parent: Option<NodeId>,
}

impl CpgNode {
    /// Creates a new CPG node.
    pub fn new(id: NodeId, kind: CpgNodeKind, range: SourceRange) -> Self {
        Self {
            id,
            kind,
            range,
            text: None,
            properties: FxHashMap::default(),
            children: SmallVec::new(),
            parent: None,
        }
    }

    /// Sets the source text.
    pub fn with_text(mut self, text: impl Into<Arc<str>>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Adds a property.
    pub fn with_property(mut self, key: PropertyKey, value: PropertyValue) -> Self {
        self.properties.insert(key, value);
        self
    }

    /// Adds a child node.
    pub fn with_child(mut self, child: NodeId) -> Self {
        self.children.push(child);
        self
    }

    /// Sets the parent node.
    pub fn with_parent(mut self, parent: NodeId) -> Self {
        self.parent = Some(parent);
        self
    }

    /// Returns the node's name if it has one.
    pub fn name(&self) -> Option<&str> {
        match &self.kind {
            CpgNodeKind::Module { name } => Some(name),
            CpgNodeKind::Class { name, .. } => Some(name),
            CpgNodeKind::Struct { name } => Some(name),
            CpgNodeKind::Enum { name } => Some(name),
            CpgNodeKind::Trait { name } => Some(name),
            CpgNodeKind::Function { signature } => Some(&signature.name),
            CpgNodeKind::Variable { name, .. } => Some(name),
            CpgNodeKind::Field { name, .. } => Some(name),
            CpgNodeKind::Parameter { name, .. } => Some(name),
            CpgNodeKind::Identifier { name, .. } => Some(name),
            CpgNodeKind::MemberAccess { member } => Some(member),
            CpgNodeKind::Import { path } => Some(path),
            CpgNodeKind::Attribute { name } => Some(name),
            CpgNodeKind::Macro { name } => Some(name),
            CpgNodeKind::GenericParam { name } => Some(name),
            _ => None,
        }
    }

    /// Returns true if this is a declaration node.
    pub fn is_declaration(&self) -> bool {
        matches!(
            self.kind,
            CpgNodeKind::Module { .. }
                | CpgNodeKind::Class { .. }
                | CpgNodeKind::Struct { .. }
                | CpgNodeKind::Enum { .. }
                | CpgNodeKind::Trait { .. }
                | CpgNodeKind::Function { .. }
                | CpgNodeKind::Variable { .. }
                | CpgNodeKind::Field { .. }
                | CpgNodeKind::Parameter { .. }
        )
    }

    /// Returns true if this is a statement node.
    pub fn is_statement(&self) -> bool {
        matches!(
            self.kind,
            CpgNodeKind::Return
                | CpgNodeKind::If
                | CpgNodeKind::While
                | CpgNodeKind::For
                | CpgNodeKind::Loop
                | CpgNodeKind::Match
                | CpgNodeKind::Break
                | CpgNodeKind::Continue
                | CpgNodeKind::Throw
                | CpgNodeKind::Try
        )
    }

    /// Returns true if this is an expression node.
    pub fn is_expression(&self) -> bool {
        matches!(
            self.kind,
            CpgNodeKind::BinaryOp { .. }
                | CpgNodeKind::UnaryOp { .. }
                | CpgNodeKind::Assignment { .. }
                | CpgNodeKind::Call { .. }
                | CpgNodeKind::MemberAccess { .. }
                | CpgNodeKind::IndexAccess
                | CpgNodeKind::Identifier { .. }
                | CpgNodeKind::Literal { .. }
                | CpgNodeKind::Lambda { .. }
                | CpgNodeKind::Await
                | CpgNodeKind::Yield
        )
    }

    /// Returns true if this is a control flow node.
    pub fn is_control_flow(&self) -> bool {
        matches!(
            self.kind,
            CpgNodeKind::If
                | CpgNodeKind::While
                | CpgNodeKind::For
                | CpgNodeKind::Loop
                | CpgNodeKind::Match
                | CpgNodeKind::Break
                | CpgNodeKind::Continue
                | CpgNodeKind::Return
                | CpgNodeKind::Throw
                | CpgNodeKind::Try
        )
    }

    /// Returns true if this is an error node.
    pub fn is_error(&self) -> bool {
        matches!(self.kind, CpgNodeKind::Error { .. })
    }
}
