//! CPG node types and related structures.

use rustc_hash::FxHashMap;
use smallvec::SmallVec;
use std::sync::Arc;
use text_size::TextRange;

use super::property_value::PropertyValue;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "serde")]
use super::serde_util::{arc_str, option_arc_str, smallvec_arc_str_2};

#[cfg(feature = "serde")]
mod boxed_properties {
    use super::{PropertyKey, PropertyValue};
    use rustc_hash::FxHashMap;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(
        properties: &Option<Box<FxHashMap<PropertyKey, PropertyValue>>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match properties {
            Some(properties) => properties.serialize(serializer),
            None => FxHashMap::<PropertyKey, PropertyValue>::default().serialize(serializer),
        }
    }

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<Option<Box<FxHashMap<PropertyKey, PropertyValue>>>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let properties = FxHashMap::deserialize(deserializer)?;
        Ok((!properties.is_empty()).then(|| Box::new(properties)))
    }
}

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

/// Language-neutral message-passing operation attached to a CPG node.
///
/// Frontends translate grammar-specific syntax into this closed vocabulary at
/// construction time. Analyses can therefore reason about concurrency without
/// inspecting source text or depending on a language grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum MsgKind {
    /// An asynchronous send.
    Send,
    /// A synchronous send whose continuation waits for acknowledgement.
    SendSync,
    /// A linear, consuming receive.
    Recv,
    /// A persistent receive that remains available after matching.
    RecvPersistent,
    /// A non-consuming receive that observes without removing a message.
    Peek,
    /// A non-deterministic selection among receive branches.
    Select,
    /// Concurrent process creation or parallel composition.
    Spawn,
}

/// Frontend-normalized reflective-name operation.
///
/// Rholang supplies these markers at the grammar boundary so downstream
/// name-flow analysis never has to inspect source text or reconstruct concrete
/// syntax. The vocabulary is language-neutral enough for another reflective
/// process-calculus frontend to reuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum NameOperationKind {
    /// A restriction creates a fresh lexical name scope (Rholang `new`).
    Restriction,
    /// A definition introduces the fresh name bound by a restriction.
    FreshBinder,
    /// A process is reified into a name (Rholang quote, `@P`).
    Quote,
    /// A name is reflected back into a process (Rholang drop/eval, `*x`).
    Drop,
}

/// Frontend-normalized ownership operation at one expression node.
///
/// Language frontends attach these markers from grammar roles while the
/// concrete syntax tree is available. Ownership analyses can consequently
/// consume typed evidence without inspecting retained source text or matching
/// grammar-specific node names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum OwnershipOperationKind {
    /// A value-position expression that may transfer ownership.
    Move,
    /// Creation of a shared, read-only borrow.
    SharedBorrow,
    /// Creation of an exclusive, mutable borrow.
    MutableBorrow,
}

impl MsgKind {
    /// Returns true for asynchronous and synchronous producers.
    pub const fn is_send(self) -> bool {
        matches!(self, Self::Send | Self::SendSync)
    }

    /// Returns true only for a linear, consuming receive.
    pub const fn is_linear_receive(self) -> bool {
        matches!(self, Self::Recv)
    }

    /// Returns true for consuming, persistent, and peeking receivers.
    pub const fn is_receive(self) -> bool {
        matches!(self, Self::Recv | Self::RecvPersistent | Self::Peek)
    }

    /// Returns true for operations that identify a channel.
    pub const fn uses_channel(self) -> bool {
        matches!(
            self,
            Self::Send | Self::SendSync | Self::Recv | Self::RecvPersistent | Self::Peek
        )
    }
}

/// Lock operation attached to a call-like CPG node.
///
/// Frontends and semantic adapters assign this metadata after resolving a
/// synchronization API. The feature-free concurrency core therefore consumes
/// typed lock evidence without guessing from call spelling or parsing source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum LockKind {
    /// Acquire a shared/read lock.
    AcquireRead,
    /// Acquire an exclusive/write lock.
    AcquireWrite,
    /// Release the identified lock.
    Release,
}

impl LockKind {
    /// Returns the acquisition mode, or `None` for a release.
    pub const fn acquisition_mode(self) -> Option<LockMode> {
        match self {
            Self::AcquireRead => Some(LockMode::Read),
            Self::AcquireWrite => Some(LockMode::Write),
            Self::Release => None,
        }
    }
}

/// Sharing mode of a lock acquisition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum LockMode {
    /// Shared acquisition; other readers may proceed.
    Read,
    /// Exclusive acquisition; readers and writers conflict.
    Write,
}

/// Source code location range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
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
        /// Function signature, boxed to bound the enum's inline size.
        signature: Box<MethodSignature>,
    },
    /// Function parameter.
    Parameter {
        /// Parameter name.
        #[cfg_attr(feature = "serde", serde(with = "arc_str"))]
        name: Arc<str>,
        /// Parameter type, boxed when present to bound the enum's inline size.
        param_type: Option<Box<TypeInfo>>,
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
        /// Variable type, boxed when present to bound the enum's inline size.
        var_type: Option<Box<TypeInfo>>,
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
        /// Field type, boxed when present to bound the enum's inline size.
        field_type: Option<Box<TypeInfo>>,
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
        /// The boxed type information.
        type_info: Box<TypeInfo>,
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

/// Copy-sized discriminant for every [`CpgNodeKind`] variant.
///
/// Unlike [`crate::pattern::NodeKindTag`], which deliberately folds kinds not
/// used by structural pattern templates into `Unknown`, this tag is a complete
/// one-to-one classification. Analyses that only branch on a node variant can
/// copy this single byte before mutating the graph instead of cloning the
/// payload-bearing [`CpgNodeKind`].
#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CpgNodeKindTag {
    /// [`CpgNodeKind::Root`].
    Root,
    /// [`CpgNodeKind::Module`].
    Module,
    /// [`CpgNodeKind::Class`].
    Class,
    /// [`CpgNodeKind::Struct`].
    Struct,
    /// [`CpgNodeKind::Enum`].
    Enum,
    /// [`CpgNodeKind::Trait`].
    Trait,
    /// [`CpgNodeKind::Impl`].
    Impl,
    /// [`CpgNodeKind::Function`].
    Function,
    /// [`CpgNodeKind::Parameter`].
    Parameter,
    /// [`CpgNodeKind::Block`].
    Block,
    /// [`CpgNodeKind::Variable`].
    Variable,
    /// [`CpgNodeKind::Field`].
    Field,
    /// [`CpgNodeKind::Return`].
    Return,
    /// [`CpgNodeKind::If`].
    If,
    /// [`CpgNodeKind::Else`].
    Else,
    /// [`CpgNodeKind::While`].
    While,
    /// [`CpgNodeKind::For`].
    For,
    /// [`CpgNodeKind::Loop`].
    Loop,
    /// [`CpgNodeKind::Match`].
    Match,
    /// [`CpgNodeKind::MatchArm`].
    MatchArm,
    /// [`CpgNodeKind::Break`].
    Break,
    /// [`CpgNodeKind::Continue`].
    Continue,
    /// [`CpgNodeKind::Throw`].
    Throw,
    /// [`CpgNodeKind::Try`].
    Try,
    /// [`CpgNodeKind::Catch`].
    Catch,
    /// [`CpgNodeKind::Finally`].
    Finally,
    /// [`CpgNodeKind::BinaryOp`].
    BinaryOp,
    /// [`CpgNodeKind::UnaryOp`].
    UnaryOp,
    /// [`CpgNodeKind::Assignment`].
    Assignment,
    /// [`CpgNodeKind::Call`].
    Call,
    /// [`CpgNodeKind::MemberAccess`].
    MemberAccess,
    /// [`CpgNodeKind::IndexAccess`].
    IndexAccess,
    /// [`CpgNodeKind::Identifier`].
    Identifier,
    /// [`CpgNodeKind::Literal`].
    Literal,
    /// [`CpgNodeKind::Lambda`].
    Lambda,
    /// [`CpgNodeKind::Await`].
    Await,
    /// [`CpgNodeKind::Yield`].
    Yield,
    /// [`CpgNodeKind::TypeAnnotation`].
    TypeAnnotation,
    /// [`CpgNodeKind::GenericParam`].
    GenericParam,
    /// [`CpgNodeKind::Comment`].
    Comment,
    /// [`CpgNodeKind::Import`].
    Import,
    /// [`CpgNodeKind::Attribute`].
    Attribute,
    /// [`CpgNodeKind::Macro`].
    Macro,
    /// [`CpgNodeKind::Error`].
    Error,
    /// [`CpgNodeKind::Unknown`].
    Unknown,
}

impl CpgNodeKind {
    /// Returns this kind's complete copy-sized discriminant.
    pub const fn tag(&self) -> CpgNodeKindTag {
        match self {
            Self::Root => CpgNodeKindTag::Root,
            Self::Module { .. } => CpgNodeKindTag::Module,
            Self::Class { .. } => CpgNodeKindTag::Class,
            Self::Struct { .. } => CpgNodeKindTag::Struct,
            Self::Enum { .. } => CpgNodeKindTag::Enum,
            Self::Trait { .. } => CpgNodeKindTag::Trait,
            Self::Impl { .. } => CpgNodeKindTag::Impl,
            Self::Function { .. } => CpgNodeKindTag::Function,
            Self::Parameter { .. } => CpgNodeKindTag::Parameter,
            Self::Block { .. } => CpgNodeKindTag::Block,
            Self::Variable { .. } => CpgNodeKindTag::Variable,
            Self::Field { .. } => CpgNodeKindTag::Field,
            Self::Return => CpgNodeKindTag::Return,
            Self::If => CpgNodeKindTag::If,
            Self::Else => CpgNodeKindTag::Else,
            Self::While => CpgNodeKindTag::While,
            Self::For => CpgNodeKindTag::For,
            Self::Loop => CpgNodeKindTag::Loop,
            Self::Match => CpgNodeKindTag::Match,
            Self::MatchArm => CpgNodeKindTag::MatchArm,
            Self::Break => CpgNodeKindTag::Break,
            Self::Continue => CpgNodeKindTag::Continue,
            Self::Throw => CpgNodeKindTag::Throw,
            Self::Try => CpgNodeKindTag::Try,
            Self::Catch => CpgNodeKindTag::Catch,
            Self::Finally => CpgNodeKindTag::Finally,
            Self::BinaryOp { .. } => CpgNodeKindTag::BinaryOp,
            Self::UnaryOp { .. } => CpgNodeKindTag::UnaryOp,
            Self::Assignment { .. } => CpgNodeKindTag::Assignment,
            Self::Call { .. } => CpgNodeKindTag::Call,
            Self::MemberAccess { .. } => CpgNodeKindTag::MemberAccess,
            Self::IndexAccess => CpgNodeKindTag::IndexAccess,
            Self::Identifier { .. } => CpgNodeKindTag::Identifier,
            Self::Literal { .. } => CpgNodeKindTag::Literal,
            Self::Lambda { .. } => CpgNodeKindTag::Lambda,
            Self::Await => CpgNodeKindTag::Await,
            Self::Yield => CpgNodeKindTag::Yield,
            Self::TypeAnnotation { .. } => CpgNodeKindTag::TypeAnnotation,
            Self::GenericParam { .. } => CpgNodeKindTag::GenericParam,
            Self::Comment { .. } => CpgNodeKindTag::Comment,
            Self::Import { .. } => CpgNodeKindTag::Import,
            Self::Attribute { .. } => CpgNodeKindTag::Attribute,
            Self::Macro { .. } => CpgNodeKindTag::Macro,
            Self::Error { .. } => CpgNodeKindTag::Error,
            Self::Unknown { .. } => CpgNodeKindTag::Unknown,
        }
    }
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
    /// Normalized message-passing semantics, when this node is a channel event.
    ///
    /// This metadata is populated by language frontends. It is intentionally
    /// independent of [`CpgNodeKind`] so a send can remain a call-like
    /// expression and a selection can remain a control-flow construct.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub message_kind: Option<MsgKind>,
    /// Normalized reflective-name semantics, when represented by this node.
    ///
    /// Like [`Self::message_kind`], this is assigned only by a language
    /// frontend. Analyses consume the closed enum and never parse source text.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub name_operation: Option<NameOperationKind>,
    /// Normalized ownership semantics, when represented by this expression.
    ///
    /// This metadata records only a grammar-level operation. Whether a Rust
    /// value-position expression copies or moves, and whether a borrow is a
    /// reborrow, are determined from typed CPG evidence by the PCG analysis.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub ownership_operation: Option<OwnershipOperationKind>,
    /// Normalized lock semantics, when this node is a synchronization call.
    ///
    /// This field is intentionally independent of [`CpgNodeKind::Call`]. A
    /// semantic adapter can preserve the ordinary call role while recording
    /// whether it acquires or releases a resolved lock resource.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub lock_kind: Option<LockKind>,
    /// Normalized nominal type allocated by this expression, when represented.
    ///
    /// Frontends attach this metadata to constructor/allocation calls at the
    /// grammar boundary. Analyses can therefore distinguish an allocation from
    /// an ordinary call without reparsing source text or matching spellings.
    /// The optional payload is boxed to keep every node's inline layout small.
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "Option::is_none")
    )]
    pub allocation_type: Option<Box<TypeInfo>>,
    /// Additional properties, allocated only when nonempty.
    #[cfg_attr(feature = "serde", serde(default, with = "boxed_properties"))]
    pub properties: Option<Box<FxHashMap<PropertyKey, PropertyValue>>>,
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
            message_kind: None,
            name_operation: None,
            ownership_operation: None,
            lock_kind: None,
            allocation_type: None,
            properties: None,
            children: SmallVec::new(),
            parent: None,
        }
    }

    /// Sets the source text.
    pub fn with_text(mut self, text: impl Into<Arc<str>>) -> Self {
        self.text = Some(text.into());
        self
    }

    /// Attaches normalized message-passing semantics to this node.
    pub fn with_message_kind(mut self, kind: MsgKind) -> Self {
        self.message_kind = Some(kind);
        self
    }

    /// Attaches normalized reflective-name semantics to this node.
    pub fn with_name_operation(mut self, kind: NameOperationKind) -> Self {
        self.name_operation = Some(kind);
        self
    }

    /// Attaches normalized ownership semantics to this expression node.
    pub fn with_ownership_operation(mut self, kind: OwnershipOperationKind) -> Self {
        self.ownership_operation = Some(kind);
        self
    }

    /// Attaches normalized lock semantics to this node.
    pub fn with_lock_kind(mut self, kind: LockKind) -> Self {
        self.lock_kind = Some(kind);
        self
    }

    /// Attaches normalized allocation semantics to this expression node.
    pub fn with_allocation_type(mut self, type_info: TypeInfo) -> Self {
        self.allocation_type = Some(Box::new(type_info));
        self
    }

    /// Adds a property.
    pub fn with_property(mut self, key: PropertyKey, value: PropertyValue) -> Self {
        self.properties
            .get_or_insert_with(|| Box::new(FxHashMap::default()))
            .insert(key, value);
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
    fn complete_kind_tag_is_one_copy_byte() {
        fn assert_copy<T: Copy>() {}
        assert_copy::<CpgNodeKindTag>();
        assert_eq!(std::mem::size_of::<CpgNodeKindTag>(), 1);
        assert_eq!(CpgNodeKind::Else.tag(), CpgNodeKindTag::Else);
        assert_eq!(CpgNodeKind::MatchArm.tag(), CpgNodeKindTag::MatchArm);
        assert_eq!(CpgNodeKind::Finally.tag(), CpgNodeKindTag::Finally);
        assert_eq!(CpgNodeKind::Await.tag(), CpgNodeKindTag::Await);
        assert_eq!(
            CpgNodeKind::TypeAnnotation {
                type_info: Box::new(TypeInfo::new("T")),
            }
            .tag(),
            CpgNodeKindTag::TypeAnnotation,
        );
        assert_eq!(
            CpgNodeKind::Unknown { kind: "x".into() }.tag(),
            CpgNodeKindTag::Unknown,
        );
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn boxed_node_payloads_bound_the_64_bit_node_layout() {
        let node_bytes = std::mem::size_of::<CpgNode>();
        assert!(node_bytes <= 144, "CpgNode grew to {node_bytes} bytes");
        assert!(node_bytes < 5 * 32);
    }

    #[test]
    fn message_kinds_expose_consumption_roles() {
        for kind in [MsgKind::Send, MsgKind::SendSync] {
            assert!(kind.is_send());
            assert!(kind.uses_channel());
            assert!(!kind.is_linear_receive());
        }
        assert!(MsgKind::Recv.is_linear_receive());
        for kind in [MsgKind::Recv, MsgKind::RecvPersistent, MsgKind::Peek] {
            assert!(kind.uses_channel());
            assert!(!kind.is_send());
        }
        for kind in [MsgKind::Select, MsgKind::Spawn] {
            assert!(!kind.uses_channel());
            assert!(!kind.is_send());
            assert!(!kind.is_linear_receive());
        }
    }

    #[test]
    fn node_builder_attaches_message_kind() {
        let n = node(CpgNodeKind::Root).with_message_kind(MsgKind::Spawn);
        assert_eq!(n.message_kind, Some(MsgKind::Spawn));
    }

    #[test]
    fn lock_kinds_expose_acquisition_modes() {
        assert_eq!(
            LockKind::AcquireRead.acquisition_mode(),
            Some(LockMode::Read)
        );
        assert_eq!(
            LockKind::AcquireWrite.acquisition_mode(),
            Some(LockMode::Write)
        );
        assert_eq!(LockKind::Release.acquisition_mode(), None);
    }

    #[test]
    fn node_builder_attaches_lock_kind() {
        let n = node(CpgNodeKind::Call {
            target: None,
            is_method: true,
        })
        .with_lock_kind(LockKind::AcquireWrite);
        assert_eq!(n.lock_kind, Some(LockKind::AcquireWrite));
    }

    #[test]
    fn node_builder_attaches_allocation_type() {
        let n = node(CpgNodeKind::Call {
            target: None,
            is_method: true,
        })
        .with_allocation_type(TypeInfo::new("Widget"));
        assert_eq!(n.allocation_type.as_deref(), Some(&TypeInfo::new("Widget")));
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
                .as_deref()
                .and_then(|properties| properties.get(&PropertyKey::Name))
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
                signature: Box::new(sig("f"))
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
            signature: Box::new(sig("f"))
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
            signature: Box::new(sig("f"))
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
            signature: Box::new(sig("f"))
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
            signature: Box::new(sig("f"))
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

        #[test]
        fn prop_complete_tag_equality_matches_rust_discriminant(
            left in arb_node_kind(),
            right in arb_node_kind(),
        ) {
            prop_assert_eq!(
                left.tag() == right.tag(),
                std::mem::discriminant(&left) == std::mem::discriminant(&right),
            );
        }

        #[test]
        fn prop_cold_properties_match_an_eager_map(
            entries in prop::collection::vec((0u8..8, any::<i64>()), 0..64),
        ) {
            let mut node = CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::Root,
                SourceRange::default(),
            );
            let mut expected = FxHashMap::default();

            for (tag, value) in entries {
                let key = match tag {
                    0 => PropertyKey::Name,
                    1 => PropertyKey::Type,
                    2 => PropertyKey::Scope,
                    3 => PropertyKey::Visibility,
                    4 => PropertyKey::Mutable,
                    5 => PropertyKey::Static,
                    6 => PropertyKey::Async,
                    _ => PropertyKey::Custom("generated".into()),
                };
                expected.insert(key.clone(), PropertyValue::Int(value));
                node = node.with_property(key, PropertyValue::Int(value));
            }

            if expected.is_empty() {
                prop_assert!(node.properties.is_none());
            } else {
                prop_assert_eq!(node.properties.as_deref(), Some(&expected));
            }
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
            && a.message_kind == b.message_kind
            && a.lock_kind == b.lock_kind
            && a.allocation_type == b.allocation_type
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
                signature: Box::new(MethodSignature {
                    name: "f".into(),
                    params: Default::default(),
                    return_type: Some(TypeInfo::new("i64")),
                    is_static: true,
                    is_async: false,
                    visibility: Visibility::Public,
                }),
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
                var_type: Some(Box::new(TypeInfo::new("u8"))),
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
    fn boxed_payloads_preserve_the_json_wire_shape() {
        let kind = CpgNodeKind::Function {
            signature: Box::new(MethodSignature {
                name: "f".into(),
                params: Default::default(),
                return_type: Some(TypeInfo::new("i64")),
                is_static: true,
                is_async: false,
                visibility: Visibility::Public,
            }),
        };
        assert_eq!(
            serde_json::to_value(kind).expect("serialize boxed function kind"),
            serde_json::json!({
                "Function": {
                    "signature": {
                        "name": "f",
                        "params": [],
                        "return_type": {
                            "name": "i64",
                            "is_reference": false,
                            "is_mutable": false,
                            "generics": []
                        },
                        "is_static": true,
                        "is_async": false,
                        "visibility": "Public"
                    }
                }
            })
        );

        let kind = CpgNodeKind::Variable {
            name: "value".into(),
            var_type: Some(Box::new(TypeInfo::new("u8"))),
            scope: ScopeId::new(7),
            is_mutable: true,
        };
        assert_eq!(
            serde_json::to_value(kind).expect("serialize boxed variable type"),
            serde_json::json!({
                "Variable": {
                    "name": "value",
                    "var_type": {
                        "name": "u8",
                        "is_reference": false,
                        "is_mutable": false,
                        "generics": []
                    },
                    "scope": 7,
                    "is_mutable": true
                }
            })
        );
    }

    #[test]
    fn boxed_properties_preserve_the_json_wire_shape() {
        let empty = CpgNode::new(NodeId::new(1), CpgNodeKind::Root, SourceRange::default());
        let empty_json = serde_json::to_value(&empty).expect("serialize empty properties");
        assert_eq!(empty_json["properties"], serde_json::json!({}));
        let empty_back: CpgNode =
            serde_json::from_value(empty_json).expect("deserialize empty properties");
        assert!(empty_back.properties.is_none());

        let populated = empty.with_property(PropertyKey::Name, PropertyValue::Int(7));
        let populated_json =
            serde_json::to_value(&populated).expect("serialize populated properties");
        assert_eq!(
            populated_json["properties"],
            serde_json::json!({
                "Name": [67, 80, 86, 1, 1, 0, 0, 0, 0, 0, 0, 0, 2, 7, 0, 0, 0, 0, 0, 0, 0]
            })
        );
        let populated_back: CpgNode =
            serde_json::from_value(populated_json).expect("deserialize populated properties");
        assert_eq!(populated_back.properties, populated.properties);
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
        .with_message_kind(MsgKind::Peek)
        .with_lock_kind(LockKind::AcquireRead)
        .with_allocation_type(TypeInfo::new("Widget"))
        .with_child(NodeId::new(6))
        .with_child(NodeId::new(7))
        .with_parent(NodeId::new(4));
        let s = serde_json::to_string(&node).expect("serialize node");
        let back: CpgNode = serde_json::from_str(&s).expect("deser node");
        assert!(node_eq(&node, &back));
    }

    #[test]
    fn serde_message_kind_round_trip() {
        for kind in [
            MsgKind::Send,
            MsgKind::SendSync,
            MsgKind::Recv,
            MsgKind::RecvPersistent,
            MsgKind::Peek,
            MsgKind::Select,
            MsgKind::Spawn,
        ] {
            let json = serde_json::to_string(&kind).expect("serialize message kind");
            let round_trip: MsgKind =
                serde_json::from_str(&json).expect("deserialize message kind");
            assert_eq!(round_trip, kind);
        }
    }

    #[test]
    fn serde_lock_kind_and_mode_round_trip() {
        for kind in [
            LockKind::AcquireRead,
            LockKind::AcquireWrite,
            LockKind::Release,
        ] {
            let json = serde_json::to_string(&kind).expect("serialize lock kind");
            let round_trip: LockKind = serde_json::from_str(&json).expect("deserialize lock kind");
            assert_eq!(round_trip, kind);
        }
        for mode in [LockMode::Read, LockMode::Write] {
            let json = serde_json::to_string(&mode).expect("serialize lock mode");
            let round_trip: LockMode = serde_json::from_str(&json).expect("deserialize lock mode");
            assert_eq!(round_trip, mode);
        }
    }

    #[test]
    fn serde_legacy_node_defaults_missing_message_kind() {
        let node = CpgNode::new(NodeId::new(9), CpgNodeKind::Root, SourceRange::default());
        let mut json = serde_json::to_value(node).expect("serialize node");
        json.as_object_mut()
            .expect("node object")
            .remove("message_kind");
        json.as_object_mut()
            .expect("node object")
            .remove("lock_kind");
        json.as_object_mut()
            .expect("node object")
            .remove("allocation_type");
        let round_trip: CpgNode = serde_json::from_value(json).expect("legacy node");
        assert_eq!(round_trip.message_kind, None);
        assert_eq!(round_trip.lock_kind, None);
        assert_eq!(round_trip.allocation_type, None);
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
