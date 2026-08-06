//! CPG node types and related structures.

use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use std::sync::Arc;
use text_size::TextRange;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "serde")]
use super::serde_util::{arc_str, option_arc_str, smallvec_arc_str_2};

/// Unique identifier for a node in the CPG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
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
///
/// # Variant size
///
/// `Function { signature }` is by far the largest variant (~400 bytes, dominated
/// by [`MethodSignature`]'s inline `SmallVec` of parameters), while most
/// variants are a word or two. Since every [`CpgNode`] stores one of these
/// inline, the enum's size is paid per node, and `clippy::large_enum_variant`
/// flags it.
///
/// Boxing the signature would shrink every non-`Function` node, but it is
/// **deliberately not done here**: it would change the public shape of the enum
/// (every construction site would need `Box::new`, and downstream matches would
/// bind a `Box<MethodSignature>`), and the benefit is a memory/indirection
/// trade-off that has not been measured. This crate's optimization policy is to
/// benchmark and profile before trading one cost for another, and there are no
/// benchmarks yet (see [`docs/engineering/03-performance.md`]). The lint is
/// therefore silenced with this rationale rather than acted on blind; revisit it
/// together with the first real memory profile.
///
/// [`docs/engineering/03-performance.md`]: https://github.com/f1r3fly-io/libcpg/blob/main/docs/engineering/03-performance.md
#[allow(clippy::large_enum_variant)]
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

#[cfg(test)]
mod tests {
    use super::*;

    fn node(kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(0), kind, SourceRange::default())
    }

    fn sig(name: &str) -> MethodSignature {
        MethodSignature {
            name: name.into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Public,
        }
    }

    #[test]
    fn node_id_round_trips() {
        assert_eq!(NodeId::new(7).as_u32(), 7);
        assert_eq!(NodeId::from(9u32).as_u32(), 9);
        assert_eq!(u32::from(NodeId::new(11)), 11);
        assert_eq!(NodeId::new(3), NodeId(3));
    }

    #[test]
    fn source_range_new_fields_and_len() {
        let r = SourceRange::new(2, 7, 0, 2, 1, 3);
        assert_eq!(r.start, 2);
        assert_eq!(r.end, 7);
        assert_eq!(r.start_line, 0);
        assert_eq!(r.start_col, 2);
        assert_eq!(r.end_line, 1);
        assert_eq!(r.end_col, 3);
        assert_eq!(r.len(), 5);
        assert!(!r.is_empty());
    }

    #[test]
    fn source_range_from_bytes_and_empty() {
        let empty = SourceRange::from_bytes(4, 4);
        assert_eq!(empty.len(), 0);
        assert!(empty.is_empty());
        // start > end still saturates to an empty range.
        let inverted = SourceRange::from_bytes(9, 4);
        assert_eq!(inverted.len(), 0);
        assert!(inverted.is_empty());
        let non_empty = SourceRange::from_bytes(1, 5);
        assert_eq!(non_empty.len(), 4);
        assert!(!non_empty.is_empty());
    }

    #[test]
    fn source_range_to_text_range_and_default() {
        let r = SourceRange::from_bytes(2, 7);
        let tr = r.to_text_range();
        assert_eq!(u32::from(tr.start()), 2);
        assert_eq!(u32::from(tr.end()), 7);
        assert_eq!(u32::from(tr.len()), 5);
        assert_eq!(SourceRange::default(), SourceRange::from_bytes(0, 0));
    }

    #[test]
    fn property_value_accessors() {
        assert_eq!(PropertyValue::String("hi".into()).as_str(), Some("hi"));
        assert_eq!(PropertyValue::Int(3).as_int(), Some(3));
        assert_eq!(PropertyValue::Bool(true).as_bool(), Some(true));
        // Wrong-variant accessors return None.
        assert_eq!(PropertyValue::Int(3).as_str(), None);
        assert_eq!(PropertyValue::Bool(true).as_int(), None);
        assert_eq!(PropertyValue::Null.as_bool(), None);
        assert_eq!(PropertyValue::Uint(5).as_int(), None);
    }

    #[test]
    fn type_info_builder() {
        let t = TypeInfo::new("Vec")
            .with_reference(true)
            .with_mutable(true)
            .with_generic("T")
            .with_generic("U");
        assert_eq!(&*t.name, "Vec");
        assert!(t.is_reference);
        assert!(t.is_mutable);
        assert_eq!(t.generics.len(), 2);
        assert_eq!(&*t.generics[0], "T");
        assert_eq!(&*t.generics[1], "U");

        let d = TypeInfo::new("i64");
        assert!(!d.is_reference);
        assert!(!d.is_mutable);
        assert!(d.generics.is_empty());
    }

    #[test]
    fn method_signature_fields() {
        let s = MethodSignature {
            name: "f".into(),
            params: Default::default(),
            return_type: Some(TypeInfo::new("i64")),
            is_static: true,
            is_async: false,
            visibility: Visibility::Crate,
        };
        assert_eq!(&*s.name, "f");
        assert!(s.is_static);
        assert!(!s.is_async);
        assert_eq!(s.visibility, Visibility::Crate);
        assert_eq!(s.return_type.as_ref().map(|t| &*t.name), Some("i64"));
    }

    #[test]
    fn visibility_default_is_private() {
        assert_eq!(Visibility::default(), Visibility::Private);
    }

    #[test]
    fn scope_id_global_and_new() {
        assert_eq!(ScopeId::GLOBAL, ScopeId(0));
        assert_eq!(ScopeId::GLOBAL.0, 0);
        assert_eq!(ScopeId::new(5).0, 5);
        assert_ne!(ScopeId::new(1), ScopeId::GLOBAL);
    }

    #[test]
    fn node_builders() {
        let n = CpgNode::new(NodeId::new(1), CpgNodeKind::Root, SourceRange::default())
            .with_text("src")
            .with_property(PropertyKey::Name, PropertyValue::String("x".into()))
            .with_child(NodeId::new(2))
            .with_child(NodeId::new(3))
            .with_parent(NodeId::new(0));
        assert_eq!(n.text.as_deref(), Some("src"));
        assert_eq!(n.children.len(), 2);
        assert_eq!(n.children[0], NodeId::new(2));
        assert_eq!(n.children[1], NodeId::new(3));
        assert_eq!(n.parent, Some(NodeId::new(0)));
        assert_eq!(
            n.properties
                .get(&PropertyKey::Name)
                .and_then(|v| v.as_str()),
            Some("x")
        );
    }

    #[test]
    fn name_for_representative_variants() {
        assert_eq!(
            node(CpgNodeKind::Module { name: "m".into() }).name(),
            Some("m")
        );
        assert_eq!(
            node(CpgNodeKind::Class {
                name: "C".into(),
                is_abstract: false
            })
            .name(),
            Some("C")
        );
        assert_eq!(
            node(CpgNodeKind::Struct { name: "S".into() }).name(),
            Some("S")
        );
        assert_eq!(
            node(CpgNodeKind::Enum { name: "E".into() }).name(),
            Some("E")
        );
        assert_eq!(
            node(CpgNodeKind::Trait { name: "T".into() }).name(),
            Some("T")
        );
        assert_eq!(
            node(CpgNodeKind::Function {
                signature: sig("f")
            })
            .name(),
            Some("f")
        );
        assert_eq!(
            node(CpgNodeKind::Variable {
                name: "v".into(),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: false
            })
            .name(),
            Some("v")
        );
        assert_eq!(
            node(CpgNodeKind::Field {
                name: "fld".into(),
                field_type: None,
                visibility: Visibility::Private
            })
            .name(),
            Some("fld")
        );
        assert_eq!(
            node(CpgNodeKind::Parameter {
                name: "p".into(),
                param_type: None,
                is_variadic: false
            })
            .name(),
            Some("p")
        );
        assert_eq!(
            node(CpgNodeKind::Identifier {
                name: "x".into(),
                definition: None
            })
            .name(),
            Some("x")
        );
        assert_eq!(
            node(CpgNodeKind::MemberAccess {
                member: "mem".into()
            })
            .name(),
            Some("mem")
        );
        assert_eq!(
            node(CpgNodeKind::Import {
                path: "std::io".into()
            })
            .name(),
            Some("std::io")
        );
        assert_eq!(
            node(CpgNodeKind::Attribute {
                name: "attr".into()
            })
            .name(),
            Some("attr")
        );
        assert_eq!(
            node(CpgNodeKind::Macro { name: "mac".into() }).name(),
            Some("mac")
        );
        assert_eq!(
            node(CpgNodeKind::GenericParam { name: "G".into() }).name(),
            Some("G")
        );

        // Variants that carry no name.
        assert_eq!(node(CpgNodeKind::Root).name(), None);
        assert_eq!(node(CpgNodeKind::Return).name(), None);
        assert_eq!(
            node(CpgNodeKind::Literal {
                kind: LiteralKind::Null
            })
            .name(),
            None
        );
        assert_eq!(node(CpgNodeKind::IndexAccess).name(), None);
    }

    #[test]
    fn is_declaration_predicate() {
        assert!(node(CpgNodeKind::Function {
            signature: sig("f")
        })
        .is_declaration());
        assert!(node(CpgNodeKind::Variable {
            name: "v".into(),
            var_type: None,
            scope: ScopeId::GLOBAL,
            is_mutable: false
        })
        .is_declaration());
        assert!(node(CpgNodeKind::Class {
            name: "C".into(),
            is_abstract: false
        })
        .is_declaration());
        assert!(node(CpgNodeKind::Parameter {
            name: "p".into(),
            param_type: None,
            is_variadic: false
        })
        .is_declaration());
        assert!(!node(CpgNodeKind::If).is_declaration());
        assert!(!node(CpgNodeKind::Literal {
            kind: LiteralKind::Null
        })
        .is_declaration());
        assert!(!node(CpgNodeKind::Root).is_declaration());
    }

    #[test]
    fn is_statement_predicate() {
        for k in [
            CpgNodeKind::Return,
            CpgNodeKind::If,
            CpgNodeKind::While,
            CpgNodeKind::For,
            CpgNodeKind::Loop,
            CpgNodeKind::Match,
            CpgNodeKind::Break,
            CpgNodeKind::Continue,
            CpgNodeKind::Throw,
            CpgNodeKind::Try,
        ] {
            assert!(
                node(k.clone()).is_statement(),
                "{k:?} should be a statement"
            );
        }
        assert!(!node(CpgNodeKind::Function {
            signature: sig("f")
        })
        .is_statement());
        assert!(!node(CpgNodeKind::BinaryOp {
            operator: "+".into()
        })
        .is_statement());
        assert!(!node(CpgNodeKind::Else).is_statement());
    }

    #[test]
    fn is_expression_predicate() {
        for k in [
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
            CpgNodeKind::UnaryOp {
                operator: "-".into(),
            },
            CpgNodeKind::Assignment {
                operator: "=".into(),
            },
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
            CpgNodeKind::MemberAccess { member: "m".into() },
            CpgNodeKind::IndexAccess,
            CpgNodeKind::Identifier {
                name: "x".into(),
                definition: None,
            },
            CpgNodeKind::Literal {
                kind: LiteralKind::Bool(true),
            },
            CpgNodeKind::Await,
            CpgNodeKind::Yield,
        ] {
            assert!(
                node(k.clone()).is_expression(),
                "{k:?} should be an expression"
            );
        }
        assert!(!node(CpgNodeKind::If).is_expression());
        assert!(!node(CpgNodeKind::Function {
            signature: sig("f")
        })
        .is_expression());
    }

    #[test]
    fn is_control_flow_predicate() {
        for k in [
            CpgNodeKind::If,
            CpgNodeKind::While,
            CpgNodeKind::For,
            CpgNodeKind::Loop,
            CpgNodeKind::Match,
            CpgNodeKind::Break,
            CpgNodeKind::Continue,
            CpgNodeKind::Return,
            CpgNodeKind::Throw,
            CpgNodeKind::Try,
        ] {
            assert!(
                node(k.clone()).is_control_flow(),
                "{k:?} should be control flow"
            );
        }
        assert!(!node(CpgNodeKind::Block {
            scope: ScopeId::GLOBAL
        })
        .is_control_flow());
        assert!(!node(CpgNodeKind::BinaryOp {
            operator: "+".into()
        })
        .is_control_flow());
        assert!(!node(CpgNodeKind::Function {
            signature: sig("f")
        })
        .is_control_flow());
    }

    #[test]
    fn is_error_predicate() {
        assert!(node(CpgNodeKind::Error {
            message: "oops".into()
        })
        .is_error());
        assert!(!node(CpgNodeKind::Root).is_error());
        assert!(!node(CpgNodeKind::Unknown { kind: "x".into() }).is_error());
    }

    #[test]
    fn literal_kind_variety() {
        assert_eq!(LiteralKind::Integer(5), LiteralKind::Integer(5));
        assert_ne!(LiteralKind::Integer(5), LiteralKind::Integer(6));
        assert_ne!(LiteralKind::Integer(1), LiteralKind::Float(1.0));
        assert_eq!(
            LiteralKind::String("a".into()),
            LiteralKind::String("a".into())
        );
        assert_eq!(LiteralKind::Char('z'), LiteralKind::Char('z'));
        assert_eq!(LiteralKind::Bool(false), LiteralKind::Bool(false));
        assert_eq!(LiteralKind::Null, LiteralKind::Null);
        assert_eq!(LiteralKind::Array, LiteralKind::Array);
        assert_eq!(LiteralKind::Object, LiteralKind::Object);
        assert_eq!(
            LiteralKind::Regex("[a-z]".into()),
            LiteralKind::Regex("[a-z]".into())
        );
        assert_ne!(LiteralKind::Array, LiteralKind::Object);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::testutil::*;
    use proptest::prelude::*;

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn prop_node_id_u32_round_trip(x in any::<u32>()) {
            prop_assert_eq!(NodeId::from(x).as_u32(), x);
            prop_assert_eq!(u32::from(NodeId::from(x)), x);
            prop_assert_eq!(NodeId::new(x).as_u32(), x);
        }

        #[test]
        fn prop_source_range_len_and_empty(a in any::<u32>(), b in any::<u32>()) {
            let r = SourceRange::from_bytes(a, b);
            prop_assert_eq!(r.len(), b.saturating_sub(a));
            // Comparing `len() == 0` with `is_empty()` is the point of this
            // assertion: the two must agree. Clippy's suggestion would compare
            // `is_empty()` with itself.
            #[allow(clippy::len_zero)]
            {
                prop_assert_eq!(r.len() == 0, r.is_empty());
            }
        }

        #[test]
        fn prop_node_predicate_partition(kind in arb_node_kind()) {
            let n = CpgNode::new(NodeId::new(0), kind.clone(), SourceRange::default());
            // declaration / statement / expression are pairwise disjoint.
            let held = (n.is_declaration() as u8)
                + (n.is_statement() as u8)
                + (n.is_expression() as u8);
            prop_assert!(held <= 1, "kind {:?} is in multiple predicate classes", kind);
            // Control-flow kinds are a subset of statements.
            prop_assert!(!n.is_control_flow() || n.is_statement());
            // is_error iff the Error variant.
            prop_assert_eq!(n.is_error(), matches!(kind, CpgNodeKind::Error { .. }));
        }
    }
}

#[cfg(all(test, feature = "serde"))]
mod serde_tests {
    use super::*;
    use crate::testutil::*;
    use proptest::prelude::*;

    fn node_eq(a: &CpgNode, b: &CpgNode) -> bool {
        a.id == b.id
            && a.kind == b.kind
            && a.range == b.range
            && a.text == b.text
            && a.children == b.children
            && a.parent == b.parent
            && a.properties == b.properties
    }

    #[test]
    fn serde_leaf_types_round_trip() {
        let id = NodeId::new(42);
        let s = serde_json::to_string(&id).expect("serialize NodeId");
        assert_eq!(
            serde_json::from_str::<NodeId>(&s).expect("deser NodeId"),
            id
        );

        let r = SourceRange::new(1, 9, 0, 1, 2, 3);
        let s = serde_json::to_string(&r).expect("serialize SourceRange");
        assert_eq!(
            serde_json::from_str::<SourceRange>(&s).expect("deser SourceRange"),
            r
        );

        let lk = LiteralKind::Integer(-7);
        let s = serde_json::to_string(&lk).expect("serialize LiteralKind");
        assert_eq!(
            serde_json::from_str::<LiteralKind>(&s).expect("deser LiteralKind"),
            lk
        );
    }

    #[test]
    fn serde_node_kind_round_trip() {
        let kinds = [
            CpgNodeKind::Root,
            CpgNodeKind::Function {
                signature: MethodSignature {
                    name: "f".into(),
                    params: Default::default(),
                    return_type: Some(TypeInfo::new("i64")),
                    is_static: true,
                    is_async: false,
                    visibility: Visibility::Public,
                },
            },
            CpgNodeKind::Identifier {
                name: "x".into(),
                definition: Some(NodeId::new(3)),
            },
            CpgNodeKind::Literal {
                kind: LiteralKind::String("hi".into()),
            },
            CpgNodeKind::Variable {
                name: "v".into(),
                var_type: Some(TypeInfo::new("u8")),
                scope: ScopeId::new(2),
                is_mutable: true,
            },
        ];
        for k in kinds {
            let s = serde_json::to_string(&k).expect("serialize kind");
            let back: CpgNodeKind = serde_json::from_str(&s).expect("deser kind");
            assert_eq!(k, back);
        }
    }

    #[test]
    fn serde_node_round_trip() {
        // Properties are left empty: serde_json requires string-shaped map keys,
        // and the property map is keyed by an enum, so exercising the map here
        // would test serde_json rather than CpgNode. Every other field is set.
        let node = CpgNode::new(
            NodeId::new(5),
            CpgNodeKind::Identifier {
                name: "x".into(),
                definition: None,
            },
            SourceRange::from_bytes(2, 4),
        )
        .with_text("x")
        .with_child(NodeId::new(6))
        .with_child(NodeId::new(7))
        .with_parent(NodeId::new(4));
        let s = serde_json::to_string(&node).expect("serialize node");
        let back: CpgNode = serde_json::from_str(&s).expect("deser node");
        assert!(node_eq(&node, &back));
    }

    /// The full `arb_node_kind` strategy — no float exclusion. `serde_json` is
    /// built with the `float_roundtrip` feature (see Cargo.toml), so every finite
    /// `f64` (the generator restricts floats to finite values) round-trips
    /// exactly through JSON, which libcpg forwards verbatim via its derives.
    fn arb_serdeable_node_kind() -> impl Strategy<Value = CpgNodeKind> {
        arb_node_kind()
    }

    #[test]
    fn serde_float_literal_round_trip() {
        // With `float_roundtrip` enabled, every finite f64 round-trips exactly —
        // including subnormals and extreme magnitudes that the fast parser lost.
        for f in [
            0.0f64,
            -0.0,
            0.5,
            -2.25,
            0.1,
            42.0,
            f64::MIN,
            f64::MAX,
            f64::MIN_POSITIVE,
            5e-324,
            2.0009e143,
            -1.797_693_134_862_315_7e308,
        ] {
            let k = CpgNodeKind::Literal {
                kind: LiteralKind::Float(f),
            };
            let s = serde_json::to_string(&k).expect("serialize float literal");
            let back: CpgNodeKind = serde_json::from_str(&s).expect("deser float literal");
            assert_eq!(k, back);
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn prop_source_range_serde(r in arb_source_range()) {
            let s = serde_json::to_string(&r).expect("ser");
            let back: SourceRange = serde_json::from_str(&s).expect("deser");
            prop_assert_eq!(r, back);
        }

        #[test]
        fn prop_node_kind_serde(kind in arb_serdeable_node_kind()) {
            let s = serde_json::to_string(&kind).expect("ser");
            let back: CpgNodeKind = serde_json::from_str(&s).expect("deser");
            prop_assert_eq!(kind, back);
        }
    }
}
