//! Stack-safe structural expression keys.
//!
//! Logical expression trees are represented by roots in a shared postorder
//! arena. Nested-expression operands store checked node identifiers, never
//! owned `Box` links. Public construction, comparison, hashing, formatting,
//! persistence, and destruction therefore have constant native call depth.

use std::{
    cmp::Ordering,
    collections::BTreeMap,
    fmt,
    hash::{Hash, Hasher},
    sync::{Arc, Mutex},
};

use thiserror::Error;

use crate::NodeId;

#[cfg(feature = "serde")]
use serde::{
    de::{Error as _, SeqAccess, Visitor},
    ser::Error as _,
    Deserialize, Deserializer, Serialize, Serializer,
};

const WIRE_MAGIC: [u8; 4] = *b"CEK\x01";
const WIRE_HEADER_LEN: usize = 20;

const TAG_VARIABLE: u8 = 0;
const TAG_INTEGER: u8 = 1;
const TAG_FLOAT_BITS: u8 = 2;
const TAG_BOOL: u8 = 3;
const TAG_STRING: u8 = 4;
const TAG_CHAR: u8 = 5;
const TAG_NULL: u8 = 6;
const TAG_ARRAY: u8 = 7;
const TAG_OBJECT: u8 = 8;
const TAG_REGEX: u8 = 9;
const TAG_EXPRESSION: u8 = 10;
const TAG_OPAQUE: u8 = 11;

/// A resource dimension enforced by [`ExprKeyLimits`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExprKeyLimit {
    /// Logical expression nodes reachable from one key.
    Nodes,
    /// Logical operands reachable from one key.
    Operands,
    /// Aggregate UTF-8 bytes in operators and string-like operands.
    StringBytes,
    /// Bytes in the canonical `CEK1` representation.
    EncodedBytes,
    /// Node-plus-operand machine steps.
    Work,
}

/// Independent limits for expression construction and persistence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExprKeyLimits {
    /// Maximum logical or physical expression nodes.
    pub max_nodes: usize,
    /// Maximum logical or physical operands.
    pub max_operands: usize,
    /// Maximum aggregate operator and operand string bytes.
    pub max_string_bytes: usize,
    /// Maximum bytes in one canonical encoded key.
    pub max_encoded_bytes: usize,
    /// Maximum node-plus-operand work-machine steps.
    pub max_work: usize,
}

impl ExprKeyLimits {
    /// Limits bounded only by the host address space.
    pub const UNBOUNDED: Self = Self {
        max_nodes: usize::MAX,
        max_operands: usize::MAX,
        max_string_bytes: usize::MAX,
        max_encoded_bytes: usize::MAX,
        max_work: usize::MAX,
    };
}

impl Default for ExprKeyLimits {
    fn default() -> Self {
        Self {
            max_nodes: 1_000_000,
            max_operands: 4_000_000,
            max_string_bytes: 64 * 1024 * 1024,
            max_encoded_bytes: 256 * 1024 * 1024,
            max_work: 5_000_000,
        }
    }
}

/// Exact logical measurements for one expression root.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExprKeyMetrics {
    /// Expression nodes, counting repeated shared children per occurrence.
    pub nodes: usize,
    /// Ordered operands, counting repeated shared children per occurrence.
    pub operands: usize,
    /// Aggregate UTF-8 bytes in operators and string-like operands.
    pub string_bytes: usize,
    /// Bytes in the canonical `CEK1` representation.
    pub encoded_bytes: usize,
    /// Node-plus-operand work-machine steps.
    pub work: usize,
}

/// A typed expression construction, validation, or persistence failure.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ExprKeyError {
    /// A named independent resource limit was exceeded.
    #[error("expression-key {limit:?} limit exceeded: observed {actual}, maximum {maximum}")]
    LimitExceeded {
        /// Resource dimension that failed.
        limit: ExprKeyLimit,
        /// Observed amount.
        actual: usize,
        /// Configured maximum.
        maximum: usize,
    },
    /// Index or size arithmetic overflowed.
    #[error("expression-key arithmetic overflow while computing {context}")]
    ArithmeticOverflow {
        /// Checked operation that overflowed.
        context: &'static str,
    },
    /// A fallible heap reservation failed.
    #[error("expression-key allocation failed while reserving {context}: {message}")]
    AllocationFailed {
        /// Arena or worklist being reserved.
        context: &'static str,
        /// Allocator diagnostic.
        message: String,
    },
    /// The canonical wire prefix or version is unsupported.
    #[error("invalid expression-key wire prefix or version")]
    InvalidWirePrefix,
    /// The byte stream ended in a field.
    #[error("unexpected end of expression-key bytes while reading {context}")]
    UnexpectedEof {
        /// Field being decoded.
        context: &'static str,
    },
    /// An operand tag is outside the closed alphabet.
    #[error("invalid expression-key operand tag {tag}")]
    InvalidTag {
        /// Unknown tag byte.
        tag: u8,
    },
    /// A boolean payload was neither zero nor one.
    #[error("invalid expression-key boolean byte {byte}")]
    InvalidBool {
        /// Invalid byte.
        byte: u8,
    },
    /// A character payload was not a Unicode scalar value.
    #[error("invalid expression-key character value {value:#x}")]
    InvalidChar {
        /// Invalid `u32` value.
        value: u32,
    },
    /// A string payload was not valid UTF-8.
    #[error("invalid UTF-8 in expression-key string")]
    InvalidUtf8,
    /// A node's operand range was not the next checked tape range.
    #[error(
        "expression node {node} has operand range {first}..{end}; expected start {expected_start} within {operand_count} operands"
    )]
    InvalidOperandRange {
        /// Node with the invalid range.
        node: usize,
        /// Declared first operand.
        first: u64,
        /// Declared exclusive end, or `u64::MAX` on overflow.
        end: u64,
        /// Required next tape position.
        expected_start: usize,
        /// Physical operand-tape length.
        operand_count: usize,
    },
    /// A nested expression did not precede its parent.
    #[error("expression child {child} does not precede parent {parent}")]
    InvalidChildReference {
        /// Parent arena node.
        parent: usize,
        /// Invalid child identifier.
        child: u64,
    },
    /// Direct expression children were not in canonical source order.
    #[error("expression children of node {parent} are not in canonical postorder")]
    NonCanonicalChildOrder {
        /// Parent node.
        parent: usize,
    },
    /// A decoded non-root node did not have exactly one parent.
    #[error("expression node {node} has {actual} parents; expected {expected}")]
    InvalidParentCount {
        /// Arena node.
        node: usize,
        /// Observed parent count.
        actual: usize,
        /// Required parent count.
        expected: usize,
    },
    /// Canonical encoded keys contain exactly one non-empty expression tree.
    #[error("a canonical expression key must contain at least one node")]
    EmptyArena,
    /// Bytes remained after one canonical key.
    #[error("{remaining} trailing bytes follow the expression key")]
    TrailingBytes {
        /// Unconsumed byte count.
        remaining: usize,
    },
    /// Cached metrics disagreed with the validated logical tree.
    #[error("cached expression-key metrics disagree with the arena")]
    InvalidCachedMetrics,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct ExprNode {
    operator: Arc<str>,
    first: u64,
    len: u64,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum StoredOperand {
    Variable(Arc<str>),
    Integer(i64),
    Float(u64),
    Bool(bool),
    String(Arc<str>),
    Char(char),
    Null,
    Array,
    Object,
    Regex(Arc<str>),
    Expression(u64),
    Opaque(NodeId),
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum ClassOperand {
    Variable(Arc<str>),
    Integer(i64),
    Float(u64),
    Bool(bool),
    String(Arc<str>),
    Char(char),
    Null,
    Array,
    Object,
    Regex(Arc<str>),
    Expression(u64),
    Opaque(NodeId),
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ClassDescriptor {
    operator: Arc<str>,
    operands: Vec<ClassOperand>,
}

pub(crate) struct ExprArena {
    nodes: Vec<ExprNode>,
    operands: Vec<StoredOperand>,
    metrics: Vec<ExprKeyMetrics>,
    class_ids: Vec<u64>,
    class_interner: BTreeMap<ClassDescriptor, u64>,
    comparisons: Mutex<BTreeMap<(u64, u64), Ordering>>,
}

/// One owned operand accepted by [`ExprKey::try_new`].
///
/// `Expression` owns a flat arena handle, not a recursive box. Dropping even
/// a deeply nested constructor value therefore remains stack safe.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ExprOperand {
    /// Variable read by name.
    Variable(Arc<str>),
    /// Signed integer literal.
    Integer(i64),
    /// IEEE-754 floating literal represented by exact bits.
    Float(u64),
    /// Boolean literal.
    Bool(bool),
    /// String literal.
    String(Arc<str>),
    /// Character literal.
    Char(char),
    /// Null or nil literal.
    Null,
    /// Array or list literal.
    Array,
    /// Object or map literal.
    Object,
    /// Regular-expression literal.
    Regex(Arc<str>),
    /// Nested structural expression.
    Expression(ExprKey),
    /// Unsupported operand retained by identity.
    Opaque(NodeId),
}

/// Structural key for one expression root in a shared flat arena.
#[derive(Clone)]
pub struct ExprKey {
    arena: Arc<ExprArena>,
    root: u64,
    metrics: ExprKeyMetrics,
    canonical_tree: bool,
}

/// Borrowed reference to any expression node in a flat arena.
#[derive(Clone, Copy)]
pub struct ExprKeyRef<'a> {
    arena: &'a ExprArena,
    node: u64,
}

/// Borrowed logical view of one operand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ExprOperandRef<'a> {
    /// Variable read by name.
    Variable(&'a str),
    /// Signed integer literal.
    Integer(i64),
    /// Exact floating-point bits.
    Float(u64),
    /// Boolean literal.
    Bool(bool),
    /// String literal.
    String(&'a str),
    /// Character literal.
    Char(char),
    /// Null or nil literal.
    Null,
    /// Array or list literal.
    Array,
    /// Object or map literal.
    Object,
    /// Regular-expression literal.
    Regex(&'a str),
    /// Nested expression reference.
    Expression(ExprKeyRef<'a>),
    /// Unsupported CPG node identity.
    Opaque(NodeId),
}

/// Ordered borrowed operands of one expression node.
#[derive(Clone, Copy)]
pub struct ExprOperands<'a> {
    arena: &'a ExprArena,
    values: &'a [StoredOperand],
}

/// Iterator over [`ExprOperands`].
pub struct ExprOperandsIter<'a> {
    arena: &'a ExprArena,
    values: std::slice::Iter<'a, StoredOperand>,
}

/// Crate-internal operand used while assembling the shared fact arena.
pub(crate) enum ExprBuildOperand {
    Variable(Arc<str>),
    Integer(i64),
    Float(u64),
    Bool(bool),
    String(Arc<str>),
    Char(char),
    Null,
    Array,
    Object,
    Regex(Arc<str>),
    Expression(u64),
    Opaque(NodeId),
}

/// Crate-internal fallible builder for a deterministic shared postorder arena.
pub(crate) struct ExprArenaBuilder {
    nodes: Vec<ExprNode>,
    operands: Vec<StoredOperand>,
    metrics: Vec<ExprKeyMetrics>,
    class_ids: Vec<u64>,
    class_interner: BTreeMap<ClassDescriptor, u64>,
    limits: ExprKeyLimits,
}

impl ExprArenaBuilder {
    pub(crate) fn new(limits: ExprKeyLimits) -> Self {
        Self {
            nodes: Vec::new(),
            operands: Vec::new(),
            metrics: Vec::new(),
            class_ids: Vec::new(),
            class_interner: BTreeMap::new(),
            limits,
        }
    }

    fn from_arena(arena: ExprArena, limits: ExprKeyLimits) -> Self {
        Self {
            nodes: arena.nodes,
            operands: arena.operands,
            metrics: arena.metrics,
            class_ids: arena.class_ids,
            class_interner: arena.class_interner,
            limits,
        }
    }

    pub(crate) fn push(
        &mut self,
        operator: Arc<str>,
        values: Vec<ExprBuildOperand>,
    ) -> Result<u64, ExprKeyError> {
        let node_count = checked_add(self.nodes.len(), 1, "physical arena nodes")?;
        let operand_count =
            checked_add(self.operands.len(), values.len(), "physical operand tape")?;
        enforce_limit(ExprKeyLimit::Nodes, node_count, self.limits.max_nodes)?;
        enforce_limit(
            ExprKeyLimit::Operands,
            operand_count,
            self.limits.max_operands,
        )?;
        self.nodes
            .try_reserve(1)
            .map_err(|error| allocation("expression arena node", error))?;
        self.metrics
            .try_reserve(1)
            .map_err(|error| allocation("expression metrics", error))?;
        self.class_ids
            .try_reserve(1)
            .map_err(|error| allocation("expression structural classes", error))?;
        self.operands
            .try_reserve(values.len())
            .map_err(|error| allocation("expression operand tape", error))?;

        let mut metrics = ExprKeyMetrics {
            nodes: 1,
            operands: values.len(),
            string_bytes: operator.len(),
            encoded_bytes: checked_add(24, operator.len(), "operator wire record")?,
            work: 0,
        };
        metrics.encoded_bytes = checked_add(
            metrics.encoded_bytes,
            WIRE_HEADER_LEN,
            "expression wire header",
        )?;

        let first = u64_index(self.operands.len(), "operand range start")?;
        let mut class_operands = Vec::new();
        class_operands
            .try_reserve(values.len())
            .map_err(|error| allocation("expression class operands", error))?;
        for value in values {
            let (stored, class_operand) = match value {
                ExprBuildOperand::Variable(value) => {
                    add_string_operand_metrics(&mut metrics, value.len())?;
                    (
                        StoredOperand::Variable(value.clone()),
                        ClassOperand::Variable(value),
                    )
                }
                ExprBuildOperand::Integer(value) => {
                    metrics.encoded_bytes = checked_add(metrics.encoded_bytes, 9, "integer wire")?;
                    (StoredOperand::Integer(value), ClassOperand::Integer(value))
                }
                ExprBuildOperand::Float(value) => {
                    metrics.encoded_bytes = checked_add(metrics.encoded_bytes, 9, "float wire")?;
                    (StoredOperand::Float(value), ClassOperand::Float(value))
                }
                ExprBuildOperand::Bool(value) => {
                    metrics.encoded_bytes = checked_add(metrics.encoded_bytes, 2, "boolean wire")?;
                    (StoredOperand::Bool(value), ClassOperand::Bool(value))
                }
                ExprBuildOperand::String(value) => {
                    add_string_operand_metrics(&mut metrics, value.len())?;
                    (
                        StoredOperand::String(value.clone()),
                        ClassOperand::String(value),
                    )
                }
                ExprBuildOperand::Char(value) => {
                    metrics.encoded_bytes =
                        checked_add(metrics.encoded_bytes, 5, "character wire")?;
                    (StoredOperand::Char(value), ClassOperand::Char(value))
                }
                ExprBuildOperand::Null => {
                    metrics.encoded_bytes = checked_add(metrics.encoded_bytes, 1, "null wire")?;
                    (StoredOperand::Null, ClassOperand::Null)
                }
                ExprBuildOperand::Array => {
                    metrics.encoded_bytes = checked_add(metrics.encoded_bytes, 1, "array wire")?;
                    (StoredOperand::Array, ClassOperand::Array)
                }
                ExprBuildOperand::Object => {
                    metrics.encoded_bytes = checked_add(metrics.encoded_bytes, 1, "object wire")?;
                    (StoredOperand::Object, ClassOperand::Object)
                }
                ExprBuildOperand::Regex(value) => {
                    add_string_operand_metrics(&mut metrics, value.len())?;
                    (
                        StoredOperand::Regex(value.clone()),
                        ClassOperand::Regex(value),
                    )
                }
                ExprBuildOperand::Expression(child) => {
                    let child_index = usize::try_from(child).map_err(|_| {
                        ExprKeyError::InvalidChildReference {
                            parent: self.nodes.len(),
                            child,
                        }
                    })?;
                    let child_metrics = self.metrics.get(child_index).ok_or(
                        ExprKeyError::InvalidChildReference {
                            parent: self.nodes.len(),
                            child,
                        },
                    )?;
                    metrics.nodes =
                        checked_add(metrics.nodes, child_metrics.nodes, "logical nodes")?;
                    metrics.operands =
                        checked_add(metrics.operands, child_metrics.operands, "logical operands")?;
                    metrics.string_bytes = checked_add(
                        metrics.string_bytes,
                        child_metrics.string_bytes,
                        "logical string bytes",
                    )?;
                    metrics.encoded_bytes = checked_add(
                        metrics.encoded_bytes,
                        child_metrics
                            .encoded_bytes
                            .checked_sub(WIRE_HEADER_LEN)
                            .ok_or(ExprKeyError::ArithmeticOverflow {
                                context: "nested wire payload",
                            })?,
                        "logical nested bytes",
                    )?;
                    metrics.encoded_bytes =
                        checked_add(metrics.encoded_bytes, 9, "expression-reference wire")?;
                    (
                        StoredOperand::Expression(child),
                        ClassOperand::Expression(self.class_ids[child_index]),
                    )
                }
                ExprBuildOperand::Opaque(value) => {
                    metrics.encoded_bytes = checked_add(metrics.encoded_bytes, 5, "opaque wire")?;
                    (StoredOperand::Opaque(value), ClassOperand::Opaque(value))
                }
            };
            self.operands.push(stored);
            class_operands.push(class_operand);
        }
        metrics.work = checked_add(metrics.nodes, metrics.operands, "logical expression work")?;
        enforce_metrics(metrics, self.limits)?;
        let len = u64_index(self.operands.len(), "operand range end")?
            .checked_sub(first)
            .ok_or(ExprKeyError::ArithmeticOverflow {
                context: "operand range length",
            })?;
        self.nodes.push(ExprNode {
            operator: operator.clone(),
            first,
            len,
        });
        self.metrics.push(metrics);
        let descriptor = ClassDescriptor {
            operator,
            operands: class_operands,
        };
        let next_class = u64_index(self.class_interner.len(), "structural class id")?;
        let class = *self.class_interner.entry(descriptor).or_insert(next_class);
        self.class_ids.push(class);
        u64_index(self.nodes.len() - 1, "expression node id")
    }

    pub(crate) fn finish(self) -> Arc<ExprArena> {
        Arc::new(ExprArena {
            nodes: self.nodes,
            operands: self.operands,
            metrics: self.metrics,
            class_ids: self.class_ids,
            class_interner: self.class_interner,
            comparisons: Mutex::new(BTreeMap::new()),
        })
    }
}

impl ExprKey {
    /// Constructs a canonical key with default independent limits.
    ///
    /// # Errors
    ///
    /// Returns [`ExprKeyError`] when a default limit is exceeded, checked
    /// index/size arithmetic overflows, or a required reservation fails.
    pub fn try_new(
        operator: impl Into<Arc<str>>,
        operands: Vec<ExprOperand>,
    ) -> Result<Self, ExprKeyError> {
        Self::try_new_with_limits(operator, operands, ExprKeyLimits::default())
    }

    /// Constructs a canonical key while enforcing explicit limits.
    ///
    /// # Errors
    ///
    /// Returns [`ExprKeyError`] when a configured limit is exceeded, checked
    /// index/size arithmetic overflows, or a required reservation fails.
    pub fn try_new_with_limits(
        operator: impl Into<Arc<str>>,
        operands: Vec<ExprOperand>,
        limits: ExprKeyLimits,
    ) -> Result<Self, ExprKeyError> {
        enum Input {
            Scalar(ExprOperand),
            Nested(ExprKey),
        }

        let mut inputs = Vec::new();
        inputs
            .try_reserve(operands.len())
            .map_err(|error| allocation("constructor operands", error))?;
        let mut nested_count = 0usize;
        for operand in operands {
            match operand {
                ExprOperand::Expression(key) => {
                    nested_count = checked_add(nested_count, 1, "nested operand count")?;
                    inputs.push(Input::Nested(key));
                }
                scalar => inputs.push(Input::Scalar(scalar)),
            }
        }

        let mut builder = if nested_count == 1 {
            let candidate = inputs.iter_mut().find_map(|input| match input {
                Input::Nested(key) => Some(key),
                Input::Scalar(_) => None,
            });
            if let Some(key) = candidate {
                if key.canonical_tree
                    && key.root as usize + 1 == key.arena.nodes.len()
                    && Arc::strong_count(&key.arena) == 1
                {
                    let placeholder = Arc::new(ExprArena {
                        nodes: Vec::new(),
                        operands: Vec::new(),
                        metrics: Vec::new(),
                        class_ids: Vec::new(),
                        class_interner: BTreeMap::new(),
                        comparisons: Mutex::new(BTreeMap::new()),
                    });
                    let arena = std::mem::replace(&mut key.arena, placeholder);
                    match Arc::try_unwrap(arena) {
                        Ok(arena) => ExprArenaBuilder::from_arena(arena, limits),
                        Err(arena) => {
                            key.arena = arena;
                            ExprArenaBuilder::new(limits)
                        }
                    }
                } else {
                    ExprArenaBuilder::new(limits)
                }
            } else {
                ExprArenaBuilder::new(limits)
            }
        } else {
            ExprArenaBuilder::new(limits)
        };

        let adopted_nodes = builder.nodes.len();
        let mut built = Vec::new();
        built
            .try_reserve(inputs.len())
            .map_err(|error| allocation("parent operands", error))?;
        for input in inputs {
            match input {
                Input::Scalar(value) => built.push(build_operand(value)?),
                Input::Nested(key) if adopted_nodes != 0 && Arc::strong_count(&key.arena) == 1 => {
                    built.push(ExprBuildOperand::Expression(key.root));
                }
                Input::Nested(key) => {
                    let child = copy_key_into(&mut builder, &key)?;
                    built.push(ExprBuildOperand::Expression(child));
                }
            }
        }
        let root = builder.push(operator.into(), built)?;
        let arena = builder.finish();
        Ok(Self::from_arena(arena, root, true))
    }

    pub(crate) fn from_shared(arena: Arc<ExprArena>, root: u64) -> Self {
        Self::from_arena(arena, root, false)
    }

    pub(crate) fn class_id(&self) -> u64 {
        self.arena.class_ids[self.root as usize]
    }

    pub(crate) fn shares_arena_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.arena, &other.arena)
    }

    fn from_arena(arena: Arc<ExprArena>, root: u64, canonical_tree: bool) -> Self {
        let metrics = arena.metrics[root as usize];
        Self {
            arena,
            root,
            metrics,
            canonical_tree,
        }
    }

    /// Returns the root operator spelling.
    pub fn operator(&self) -> &str {
        self.as_ref().operator()
    }

    /// Returns the root operands in source order.
    pub fn operands(&self) -> ExprOperands<'_> {
        self.as_ref().operands()
    }

    /// Returns a borrowed root reference.
    pub fn as_ref(&self) -> ExprKeyRef<'_> {
        ExprKeyRef {
            arena: &self.arena,
            node: self.root,
        }
    }

    /// Returns exact cached logical metrics.
    pub const fn metrics(&self) -> ExprKeyMetrics {
        self.metrics
    }

    /// Validates the reachable logical expression and configured limits.
    ///
    /// # Errors
    ///
    /// Returns [`ExprKeyError`] when cached/recomputed metrics disagree or a
    /// configured limit, checked arithmetic operation, or reservation fails.
    pub fn validate_with_limits(
        &self,
        limits: ExprKeyLimits,
    ) -> Result<ExprKeyMetrics, ExprKeyError> {
        enforce_metrics(self.metrics, limits)?;
        let measured = measure_logical(self.as_ref())?;
        if measured != self.metrics {
            return Err(ExprKeyError::InvalidCachedMetrics);
        }
        Ok(measured)
    }

    /// Encodes this root as one canonical `CEK1` postorder tree.
    ///
    /// # Errors
    ///
    /// Returns [`ExprKeyError`] for a default limit, invalid cached arena
    /// invariant, arithmetic overflow, or allocation failure.
    pub fn encode(&self) -> Result<Vec<u8>, ExprKeyError> {
        self.encode_with_limits(ExprKeyLimits::default())
    }

    /// Encodes this root with explicit independent limits.
    ///
    /// # Errors
    ///
    /// Returns [`ExprKeyError`] for a configured limit, invalid cached arena
    /// invariant, arithmetic overflow, or allocation failure.
    pub fn encode_with_limits(&self, limits: ExprKeyLimits) -> Result<Vec<u8>, ExprKeyError> {
        self.validate_with_limits(limits)?;
        let canonical = if self.canonical_tree {
            self.clone()
        } else {
            let mut builder = ExprArenaBuilder::new(limits);
            let root = copy_key_into(&mut builder, self)?;
            Self::from_arena(builder.finish(), root, true)
        };
        encode_canonical(&canonical, limits)
    }

    /// Decodes one canonical `CEK1` expression with default limits.
    ///
    /// # Errors
    ///
    /// Returns [`ExprKeyError`] for malformed, noncanonical, truncated, or
    /// trailing bytes; invalid scalar payloads; a default limit; checked
    /// arithmetic; or allocation failure.
    pub fn decode(bytes: &[u8]) -> Result<Self, ExprKeyError> {
        Self::decode_with_limits(bytes, ExprKeyLimits::default())
    }

    /// Decodes and validates one canonical `CEK1` expression.
    ///
    /// # Errors
    ///
    /// Returns [`ExprKeyError`] for malformed, noncanonical, truncated, or
    /// trailing bytes; invalid scalar payloads; a configured limit; checked
    /// arithmetic; or allocation failure.
    pub fn decode_with_limits(bytes: &[u8], limits: ExprKeyLimits) -> Result<Self, ExprKeyError> {
        enforce_limit(
            ExprKeyLimit::EncodedBytes,
            bytes.len(),
            limits.max_encoded_bytes,
        )?;
        let mut reader = Reader::new(bytes);
        if reader.take(4, "wire prefix")? != WIRE_MAGIC {
            return Err(ExprKeyError::InvalidWirePrefix);
        }
        let node_count = reader.usize("node count")?;
        let operand_count = reader.usize("operand count")?;
        if node_count == 0 {
            return Err(ExprKeyError::EmptyArena);
        }
        enforce_limit(ExprKeyLimit::Nodes, node_count, limits.max_nodes)?;
        enforce_limit(ExprKeyLimit::Operands, operand_count, limits.max_operands)?;
        enforce_limit(
            ExprKeyLimit::Work,
            checked_add(node_count, operand_count, "decoded work")?,
            limits.max_work,
        )?;

        let mut nodes = Vec::new();
        nodes
            .try_reserve(node_count)
            .map_err(|error| allocation("decoded expression nodes", error))?;
        let mut aggregate_strings = 0usize;
        for _ in 0..node_count {
            let operator = reader.arc_str("operator", &mut aggregate_strings, limits)?;
            let first = reader.u64("operand range start")?;
            let len = reader.u64("operand range length")?;
            nodes.push(ExprNode {
                operator,
                first,
                len,
            });
        }

        let mut operands = Vec::new();
        operands
            .try_reserve(operand_count)
            .map_err(|error| allocation("decoded operand tape", error))?;
        for _ in 0..operand_count {
            let operand = match reader.byte("operand tag")? {
                TAG_VARIABLE => StoredOperand::Variable(reader.arc_str(
                    "variable",
                    &mut aggregate_strings,
                    limits,
                )?),
                TAG_INTEGER => StoredOperand::Integer(reader.i64("integer")?),
                TAG_FLOAT_BITS => StoredOperand::Float(reader.u64("float bits")?),
                TAG_BOOL => match reader.byte("boolean")? {
                    0 => StoredOperand::Bool(false),
                    1 => StoredOperand::Bool(true),
                    byte => return Err(ExprKeyError::InvalidBool { byte }),
                },
                TAG_STRING => StoredOperand::String(reader.arc_str(
                    "string",
                    &mut aggregate_strings,
                    limits,
                )?),
                TAG_CHAR => {
                    let value = reader.u32("character")?;
                    StoredOperand::Char(
                        char::from_u32(value).ok_or(ExprKeyError::InvalidChar { value })?,
                    )
                }
                TAG_NULL => StoredOperand::Null,
                TAG_ARRAY => StoredOperand::Array,
                TAG_OBJECT => StoredOperand::Object,
                TAG_REGEX => StoredOperand::Regex(reader.arc_str(
                    "regular expression",
                    &mut aggregate_strings,
                    limits,
                )?),
                TAG_EXPRESSION => StoredOperand::Expression(reader.u64("expression child")?),
                TAG_OPAQUE => StoredOperand::Opaque(NodeId::new(reader.u32("opaque node")?)),
                tag => return Err(ExprKeyError::InvalidTag { tag }),
            };
            operands.push(operand);
        }
        if reader.remaining() != 0 {
            return Err(ExprKeyError::TrailingBytes {
                remaining: reader.remaining(),
            });
        }

        let metrics = validate_canonical(&nodes, &operands, limits)?;
        let root = u64_index(nodes.len() - 1, "decoded root")?;
        let key_metrics = metrics[root as usize];
        if key_metrics.encoded_bytes != bytes.len() {
            return Err(ExprKeyError::InvalidCachedMetrics);
        }
        let (class_ids, class_interner) = structural_classes(&nodes, &operands)?;
        let arena = Arc::new(ExprArena {
            nodes,
            operands,
            metrics,
            class_ids,
            class_interner,
            comparisons: Mutex::new(BTreeMap::new()),
        });
        Ok(Self {
            arena,
            root,
            metrics: key_metrics,
            canonical_tree: true,
        })
    }
}

impl<'a> ExprKeyRef<'a> {
    /// Returns this expression node's operator spelling.
    pub fn operator(self) -> &'a str {
        &self.node_record().operator
    }

    /// Returns this expression node's operands in source order.
    pub fn operands(self) -> ExprOperands<'a> {
        let node = self.node_record();
        let first = usize::try_from(node.first).expect("validated operand start");
        let len = usize::try_from(node.len).expect("validated operand length");
        ExprOperands {
            arena: self.arena,
            values: &self.arena.operands[first..first + len],
        }
    }

    fn node_record(self) -> &'a ExprNode {
        &self.arena.nodes[usize::try_from(self.node).expect("validated expression node")]
    }
}

impl fmt::Debug for ExprKeyRef<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        debug_key(*self, formatter)
    }
}

impl PartialEq for ExprKeyRef<'_> {
    fn eq(&self, other: &Self) -> bool {
        if std::ptr::eq(self.arena, other.arena)
            && self.arena.class_ids[self.node as usize]
                == other.arena.class_ids[other.node as usize]
        {
            return true;
        }
        compare_keys(*self, *other) == Ordering::Equal
    }
}

impl Eq for ExprKeyRef<'_> {}

impl<'a> ExprOperands<'a> {
    /// Returns the number of direct operands.
    pub fn len(self) -> usize {
        self.values.len()
    }

    /// Returns whether there are no operands.
    pub fn is_empty(self) -> bool {
        self.values.is_empty()
    }

    /// Returns one direct operand by source-order index.
    pub fn get(self, index: usize) -> Option<ExprOperandRef<'a>> {
        self.values
            .get(index)
            .map(|value| operand_ref(self.arena, value))
    }

    /// Iterates direct operands in source order.
    pub fn iter(self) -> ExprOperandsIter<'a> {
        ExprOperandsIter {
            arena: self.arena,
            values: self.values.iter(),
        }
    }
}

impl<'a> IntoIterator for ExprOperands<'a> {
    type Item = ExprOperandRef<'a>;
    type IntoIter = ExprOperandsIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> Iterator for ExprOperandsIter<'a> {
    type Item = ExprOperandRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.values
            .next()
            .map(|value| operand_ref(self.arena, value))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.values.size_hint()
    }
}

impl ExactSizeIterator for ExprOperandsIter<'_> {}

impl DoubleEndedIterator for ExprOperandsIter<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.values
            .next_back()
            .map(|value| operand_ref(self.arena, value))
    }
}

fn operand_ref<'a>(arena: &'a ExprArena, operand: &'a StoredOperand) -> ExprOperandRef<'a> {
    match operand {
        StoredOperand::Variable(value) => ExprOperandRef::Variable(value),
        StoredOperand::Integer(value) => ExprOperandRef::Integer(*value),
        StoredOperand::Float(value) => ExprOperandRef::Float(*value),
        StoredOperand::Bool(value) => ExprOperandRef::Bool(*value),
        StoredOperand::String(value) => ExprOperandRef::String(value),
        StoredOperand::Char(value) => ExprOperandRef::Char(*value),
        StoredOperand::Null => ExprOperandRef::Null,
        StoredOperand::Array => ExprOperandRef::Array,
        StoredOperand::Object => ExprOperandRef::Object,
        StoredOperand::Regex(value) => ExprOperandRef::Regex(value),
        StoredOperand::Expression(node) => {
            ExprOperandRef::Expression(ExprKeyRef { arena, node: *node })
        }
        StoredOperand::Opaque(node) => ExprOperandRef::Opaque(*node),
    }
}

impl PartialEq for ExprKey {
    fn eq(&self, other: &Self) -> bool {
        if self.shares_arena_with(other) && self.class_id() == other.class_id() {
            return true;
        }
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for ExprKey {}

impl PartialOrd for ExprKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ExprKey {
    fn cmp(&self, other: &Self) -> Ordering {
        compare_keys(self.as_ref(), other.as_ref())
    }
}

impl Hash for ExprKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        enum Event<'a> {
            Key(ExprKeyRef<'a>),
            Operand(ExprOperandRef<'a>),
            EndKey,
        }
        let mut work = vec![Event::Key(self.as_ref())];
        while let Some(event) = work.pop() {
            match event {
                Event::Key(key) => {
                    0xE1u8.hash(state);
                    key.operator().hash(state);
                    key.operands().len().hash(state);
                    work.push(Event::EndKey);
                    for operand in key.operands().iter().rev() {
                        work.push(Event::Operand(operand));
                    }
                }
                Event::Operand(operand) => match operand {
                    ExprOperandRef::Variable(value) => {
                        TAG_VARIABLE.hash(state);
                        value.hash(state);
                    }
                    ExprOperandRef::Integer(value) => {
                        TAG_INTEGER.hash(state);
                        value.hash(state);
                    }
                    ExprOperandRef::Float(value) => {
                        TAG_FLOAT_BITS.hash(state);
                        value.hash(state);
                    }
                    ExprOperandRef::Bool(value) => {
                        TAG_BOOL.hash(state);
                        value.hash(state);
                    }
                    ExprOperandRef::String(value) => {
                        TAG_STRING.hash(state);
                        value.hash(state);
                    }
                    ExprOperandRef::Char(value) => {
                        TAG_CHAR.hash(state);
                        value.hash(state);
                    }
                    ExprOperandRef::Null => TAG_NULL.hash(state),
                    ExprOperandRef::Array => TAG_ARRAY.hash(state),
                    ExprOperandRef::Object => TAG_OBJECT.hash(state),
                    ExprOperandRef::Regex(value) => {
                        TAG_REGEX.hash(state);
                        value.hash(state);
                    }
                    ExprOperandRef::Expression(key) => {
                        TAG_EXPRESSION.hash(state);
                        work.push(Event::Key(key));
                    }
                    ExprOperandRef::Opaque(node) => {
                        TAG_OPAQUE.hash(state);
                        node.hash(state);
                    }
                },
                Event::EndKey => 0xE2u8.hash(state),
            }
        }
    }
}

impl fmt::Debug for ExprKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        debug_key(self.as_ref(), formatter)
    }
}

#[cfg(feature = "serde")]
impl Serialize for ExprKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_bytes(&self.encode().map_err(S::Error::custom)?)
    }
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for ExprKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct BytesVisitor;

        impl<'de> Visitor<'de> for BytesVisitor {
            type Value = ExprKey;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("canonical versioned CEK1 bytes")
            }

            fn visit_bytes<E>(self, bytes: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                ExprKey::decode(bytes).map_err(E::custom)
            }

            fn visit_borrowed_bytes<E>(self, bytes: &'de [u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_bytes(bytes)
            }

            fn visit_byte_buf<E>(self, bytes: Vec<u8>) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                self.visit_bytes(&bytes)
            }

            fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let limits = ExprKeyLimits::default();
                let initial = sequence
                    .size_hint()
                    .unwrap_or(0)
                    .min(limits.max_encoded_bytes);
                let mut bytes = Vec::new();
                bytes.try_reserve(initial).map_err(A::Error::custom)?;
                while let Some(byte) = sequence.next_element::<u8>()? {
                    if bytes.len() == limits.max_encoded_bytes {
                        return Err(A::Error::custom(ExprKeyError::LimitExceeded {
                            limit: ExprKeyLimit::EncodedBytes,
                            actual: bytes.len().saturating_add(1),
                            maximum: limits.max_encoded_bytes,
                        }));
                    }
                    bytes.try_reserve(1).map_err(A::Error::custom)?;
                    bytes.push(byte);
                }
                ExprKey::decode_with_limits(&bytes, limits).map_err(A::Error::custom)
            }
        }

        deserializer.deserialize_bytes(BytesVisitor)
    }
}

fn build_operand(value: ExprOperand) -> Result<ExprBuildOperand, ExprKeyError> {
    Ok(match value {
        ExprOperand::Variable(value) => ExprBuildOperand::Variable(value),
        ExprOperand::Integer(value) => ExprBuildOperand::Integer(value),
        ExprOperand::Float(value) => ExprBuildOperand::Float(value),
        ExprOperand::Bool(value) => ExprBuildOperand::Bool(value),
        ExprOperand::String(value) => ExprBuildOperand::String(value),
        ExprOperand::Char(value) => ExprBuildOperand::Char(value),
        ExprOperand::Null => ExprBuildOperand::Null,
        ExprOperand::Array => ExprBuildOperand::Array,
        ExprOperand::Object => ExprBuildOperand::Object,
        ExprOperand::Regex(value) => ExprBuildOperand::Regex(value),
        ExprOperand::Opaque(value) => ExprBuildOperand::Opaque(value),
        ExprOperand::Expression(_) => {
            return Err(ExprKeyError::ArithmeticOverflow {
                context: "nested operand dispatch",
            })
        }
    })
}

fn copy_key_into(builder: &mut ExprArenaBuilder, key: &ExprKey) -> Result<u64, ExprKeyError> {
    struct Frame<'a> {
        key: ExprKeyRef<'a>,
        next: usize,
        built: Vec<ExprBuildOperand>,
    }

    fn frame(key: ExprKeyRef<'_>) -> Result<Frame<'_>, ExprKeyError> {
        let mut built = Vec::new();
        built
            .try_reserve(key.operands().len())
            .map_err(|error| allocation("expression copy frame", error))?;
        Ok(Frame {
            key,
            next: 0,
            built,
        })
    }

    let mut work = Vec::new();
    work.try_reserve(key.metrics.nodes.min(builder.limits.max_nodes))
        .map_err(|error| allocation("expression copy worklist", error))?;
    work.push(frame(key.as_ref())?);
    loop {
        let current = work.last_mut().expect("copy starts with one frame");
        if let Some(operand) = current.key.operands().get(current.next) {
            current.next += 1;
            match operand {
                ExprOperandRef::Expression(child) => work.push(frame(child)?),
                scalar => current.built.push(build_operand_ref(scalar)),
            }
            continue;
        }
        let completed = work.pop().expect("completed copy frame");
        let node = builder.push(Arc::from(completed.key.operator()), completed.built)?;
        if let Some(parent) = work.last_mut() {
            parent.built.push(ExprBuildOperand::Expression(node));
        } else {
            return Ok(node);
        }
    }
}

fn build_operand_ref(value: ExprOperandRef<'_>) -> ExprBuildOperand {
    match value {
        ExprOperandRef::Variable(value) => ExprBuildOperand::Variable(Arc::from(value)),
        ExprOperandRef::Integer(value) => ExprBuildOperand::Integer(value),
        ExprOperandRef::Float(value) => ExprBuildOperand::Float(value),
        ExprOperandRef::Bool(value) => ExprBuildOperand::Bool(value),
        ExprOperandRef::String(value) => ExprBuildOperand::String(Arc::from(value)),
        ExprOperandRef::Char(value) => ExprBuildOperand::Char(value),
        ExprOperandRef::Null => ExprBuildOperand::Null,
        ExprOperandRef::Array => ExprBuildOperand::Array,
        ExprOperandRef::Object => ExprBuildOperand::Object,
        ExprOperandRef::Regex(value) => ExprBuildOperand::Regex(Arc::from(value)),
        ExprOperandRef::Opaque(value) => ExprBuildOperand::Opaque(value),
        ExprOperandRef::Expression(_) => unreachable!("nested expressions use copy frames"),
    }
}

struct ComparisonFrame<'a> {
    left: ExprKeyRef<'a>,
    right: ExprKeyRef<'a>,
    next: usize,
}

enum ComparisonStep<'a> {
    Done,
    Order(Ordering),
    Expressions(ExprKeyRef<'a>, ExprKeyRef<'a>),
}

fn compare_keys<'a>(left: ExprKeyRef<'a>, right: ExprKeyRef<'a>) -> Ordering {
    if std::ptr::eq(left.arena, right.arena)
        && left.arena.class_ids[left.node as usize] == right.arena.class_ids[right.node as usize]
    {
        return Ordering::Equal;
    }
    if std::ptr::eq(left.arena, right.arena) {
        if let Some(ordering) = comparison_cache(left.arena).get(&(left.node, right.node)) {
            return *ordering;
        }
    }

    let mut visited = Vec::new();
    let mut work = Vec::new();
    let mut current = (left, right);
    let result = loop {
        if std::ptr::eq(current.0.arena, current.1.arena) && current.0.node == current.1.node {
            match advance_comparison(&mut work) {
                ComparisonStep::Expressions(left, right) => {
                    current = (left, right);
                    continue;
                }
                ComparisonStep::Order(ordering) => break ordering,
                ComparisonStep::Done => break Ordering::Equal,
            }
        }
        if std::ptr::eq(current.0.arena, current.1.arena) {
            visited.push((current.0.node, current.1.node));
            if let Some(ordering) = comparison_cache(current.0.arena)
                .get(&(current.0.node, current.1.node))
                .copied()
            {
                break ordering;
            }
        }
        let operator = current.0.operator().cmp(current.1.operator());
        if operator != Ordering::Equal {
            break operator;
        }
        work.push(ComparisonFrame {
            left: current.0,
            right: current.1,
            next: 0,
        });
        match advance_comparison(&mut work) {
            ComparisonStep::Expressions(left, right) => {
                current = (left, right);
            }
            ComparisonStep::Order(ordering) => break ordering,
            ComparisonStep::Done => break Ordering::Equal,
        }
    };

    if std::ptr::eq(left.arena, right.arena) && !visited.is_empty() {
        let mut cache = comparison_cache(left.arena);
        for (left_node, right_node) in visited {
            cache.insert((left_node, right_node), result);
            cache.insert((right_node, left_node), result.reverse());
        }
    }
    result
}

fn advance_comparison<'a>(work: &mut Vec<ComparisonFrame<'a>>) -> ComparisonStep<'a> {
    while let Some(frame) = work.last_mut() {
        let left_operands = frame.left.operands();
        let right_operands = frame.right.operands();
        if frame.next == left_operands.len().min(right_operands.len()) {
            let ordering = left_operands.len().cmp(&right_operands.len());
            work.pop();
            if ordering != Ordering::Equal {
                return ComparisonStep::Order(ordering);
            }
            continue;
        }
        let next = frame.next;
        frame.next += 1;
        let left_operand = left_operands.get(next).expect("bounded left operand");
        let right_operand = right_operands.get(next).expect("bounded right operand");
        match compare_operands(left_operand, right_operand) {
            OperandComparison::Equal => {}
            OperandComparison::Order(ordering) => return ComparisonStep::Order(ordering),
            OperandComparison::Expressions(left, right) => {
                return ComparisonStep::Expressions(left, right)
            }
        }
    }
    ComparisonStep::Done
}

enum OperandComparison<'a> {
    Equal,
    Order(Ordering),
    Expressions(ExprKeyRef<'a>, ExprKeyRef<'a>),
}

fn compare_operands<'a>(
    left: ExprOperandRef<'a>,
    right: ExprOperandRef<'a>,
) -> OperandComparison<'a> {
    let left_tag = operand_tag(left);
    let right_tag = operand_tag(right);
    if left_tag != right_tag {
        return OperandComparison::Order(left_tag.cmp(&right_tag));
    }
    let ordering = match (left, right) {
        (ExprOperandRef::Variable(left), ExprOperandRef::Variable(right))
        | (ExprOperandRef::String(left), ExprOperandRef::String(right))
        | (ExprOperandRef::Regex(left), ExprOperandRef::Regex(right)) => left.cmp(right),
        (ExprOperandRef::Integer(left), ExprOperandRef::Integer(right)) => left.cmp(&right),
        (ExprOperandRef::Float(left), ExprOperandRef::Float(right)) => left.cmp(&right),
        (ExprOperandRef::Bool(left), ExprOperandRef::Bool(right)) => left.cmp(&right),
        (ExprOperandRef::Char(left), ExprOperandRef::Char(right)) => left.cmp(&right),
        (ExprOperandRef::Opaque(left), ExprOperandRef::Opaque(right)) => left.cmp(&right),
        (ExprOperandRef::Expression(left), ExprOperandRef::Expression(right)) => {
            return OperandComparison::Expressions(left, right)
        }
        _ => Ordering::Equal,
    };
    if ordering == Ordering::Equal {
        OperandComparison::Equal
    } else {
        OperandComparison::Order(ordering)
    }
}

fn operand_tag(value: ExprOperandRef<'_>) -> u8 {
    match value {
        ExprOperandRef::Variable(_) => TAG_VARIABLE,
        ExprOperandRef::Integer(_) => TAG_INTEGER,
        ExprOperandRef::Float(_) => TAG_FLOAT_BITS,
        ExprOperandRef::Bool(_) => TAG_BOOL,
        ExprOperandRef::String(_) => TAG_STRING,
        ExprOperandRef::Char(_) => TAG_CHAR,
        ExprOperandRef::Null => TAG_NULL,
        ExprOperandRef::Array => TAG_ARRAY,
        ExprOperandRef::Object => TAG_OBJECT,
        ExprOperandRef::Regex(_) => TAG_REGEX,
        ExprOperandRef::Expression(_) => TAG_EXPRESSION,
        ExprOperandRef::Opaque(_) => TAG_OPAQUE,
    }
}

fn comparison_cache(
    arena: &ExprArena,
) -> std::sync::MutexGuard<'_, BTreeMap<(u64, u64), Ordering>> {
    arena
        .comparisons
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn debug_key(root: ExprKeyRef<'_>, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    enum Event<'a> {
        Key(ExprKeyRef<'a>),
        Operand(ExprOperandRef<'a>),
        Separator,
        CloseExpression,
        CloseKey,
    }
    let mut work = vec![Event::Key(root)];
    while let Some(event) = work.pop() {
        match event {
            Event::Key(key) => {
                write!(
                    formatter,
                    "ExprKey {{ operator: {:?}, operands: [",
                    key.operator()
                )?;
                work.push(Event::CloseKey);
                let operands = key.operands();
                for index in (0..operands.len()).rev() {
                    let operand = operands.get(index).expect("bounded debug operand");
                    work.push(Event::Operand(operand));
                    if index != 0 {
                        work.push(Event::Separator);
                    }
                }
            }
            Event::Operand(operand) => match operand {
                ExprOperandRef::Variable(value) => write!(formatter, "Variable({value:?})")?,
                ExprOperandRef::Integer(value) => write!(formatter, "Integer({value:?})")?,
                ExprOperandRef::Float(value) => write!(formatter, "Float({value:?})")?,
                ExprOperandRef::Bool(value) => write!(formatter, "Bool({value:?})")?,
                ExprOperandRef::String(value) => write!(formatter, "String({value:?})")?,
                ExprOperandRef::Char(value) => write!(formatter, "Char({value:?})")?,
                ExprOperandRef::Null => formatter.write_str("Null")?,
                ExprOperandRef::Array => formatter.write_str("Array")?,
                ExprOperandRef::Object => formatter.write_str("Object")?,
                ExprOperandRef::Regex(value) => write!(formatter, "Regex({value:?})")?,
                ExprOperandRef::Expression(key) => {
                    formatter.write_str("Expression(")?;
                    work.push(Event::CloseExpression);
                    work.push(Event::Key(key));
                }
                ExprOperandRef::Opaque(node) => write!(formatter, "Opaque({node:?})")?,
            },
            Event::Separator => formatter.write_str(", ")?,
            Event::CloseExpression => formatter.write_str(")")?,
            Event::CloseKey => formatter.write_str("] }")?,
        }
    }
    Ok(())
}

fn measure_logical(root: ExprKeyRef<'_>) -> Result<ExprKeyMetrics, ExprKeyError> {
    let mut nodes = 0usize;
    let mut operands = 0usize;
    let mut string_bytes = 0usize;
    let mut encoded_bytes = WIRE_HEADER_LEN;
    let mut work = vec![root];
    while let Some(key) = work.pop() {
        nodes = checked_add(nodes, 1, "measured nodes")?;
        string_bytes = checked_add(
            string_bytes,
            key.operator().len(),
            "measured operator bytes",
        )?;
        encoded_bytes = checked_add(
            encoded_bytes,
            checked_add(24, key.operator().len(), "measured node record")?,
            "measured encoded bytes",
        )?;
        for operand in key.operands() {
            operands = checked_add(operands, 1, "measured operands")?;
            match operand {
                ExprOperandRef::Variable(value)
                | ExprOperandRef::String(value)
                | ExprOperandRef::Regex(value) => {
                    string_bytes = checked_add(string_bytes, value.len(), "measured string bytes")?;
                    encoded_bytes = checked_add(
                        encoded_bytes,
                        checked_add(9, value.len(), "measured string record")?,
                        "measured encoded bytes",
                    )?;
                }
                ExprOperandRef::Integer(_)
                | ExprOperandRef::Float(_)
                | ExprOperandRef::Expression(_) => {
                    encoded_bytes = checked_add(encoded_bytes, 9, "measured wide operand")?;
                }
                ExprOperandRef::Bool(_) => {
                    encoded_bytes = checked_add(encoded_bytes, 2, "measured boolean")?;
                }
                ExprOperandRef::Char(_) | ExprOperandRef::Opaque(_) => {
                    encoded_bytes = checked_add(encoded_bytes, 5, "measured narrow operand")?;
                }
                ExprOperandRef::Null | ExprOperandRef::Array | ExprOperandRef::Object => {
                    encoded_bytes = checked_add(encoded_bytes, 1, "measured unit operand")?;
                }
            }
            if let ExprOperandRef::Expression(child) = operand {
                work.push(child);
            }
        }
    }
    Ok(ExprKeyMetrics {
        nodes,
        operands,
        string_bytes,
        encoded_bytes,
        work: checked_add(nodes, operands, "measured work")?,
    })
}

fn validate_canonical(
    nodes: &[ExprNode],
    operands: &[StoredOperand],
    limits: ExprKeyLimits,
) -> Result<Vec<ExprKeyMetrics>, ExprKeyError> {
    if nodes.is_empty() {
        return Err(ExprKeyError::EmptyArena);
    }
    let mut parents = vec![0usize; nodes.len()];
    let mut subtree_starts = vec![0usize; nodes.len()];
    let mut metrics: Vec<ExprKeyMetrics> = Vec::new();
    metrics
        .try_reserve(nodes.len())
        .map_err(|error| allocation("validated metrics", error))?;
    let mut expected_start = 0usize;
    for (parent, node) in nodes.iter().enumerate() {
        let invalid_range = |end| ExprKeyError::InvalidOperandRange {
            node: parent,
            first: node.first,
            end,
            expected_start,
            operand_count: operands.len(),
        };
        let first = usize::try_from(node.first).map_err(|_| invalid_range(u64::MAX))?;
        let len = usize::try_from(node.len).map_err(|_| invalid_range(u64::MAX))?;
        let end = first
            .checked_add(len)
            .ok_or_else(|| invalid_range(u64::MAX))?;
        if first != expected_start || end > operands.len() {
            return Err(invalid_range(u64::try_from(end).unwrap_or(u64::MAX)));
        }
        expected_start = end;
        let mut node_metrics = ExprKeyMetrics {
            nodes: 1,
            operands: len,
            string_bytes: node.operator.len(),
            encoded_bytes: checked_add(
                WIRE_HEADER_LEN,
                checked_add(24, node.operator.len(), "validated node record")?,
                "validated header",
            )?,
            work: 0,
        };
        let mut previous_child = None;
        let mut first_subtree = parent;
        for operand in &operands[first..end] {
            match operand {
                StoredOperand::Expression(child) => {
                    let child_index = usize::try_from(*child).map_err(|_| {
                        ExprKeyError::InvalidChildReference {
                            parent,
                            child: *child,
                        }
                    })?;
                    if child_index >= parent {
                        return Err(ExprKeyError::InvalidChildReference {
                            parent,
                            child: *child,
                        });
                    }
                    if let Some(previous) = previous_child {
                        if subtree_starts[child_index] != previous + 1 {
                            return Err(ExprKeyError::NonCanonicalChildOrder { parent });
                        }
                    } else {
                        first_subtree = subtree_starts[child_index];
                    }
                    previous_child = Some(child_index);
                    parents[child_index] =
                        checked_add(parents[child_index], 1, "validated parent count")?;
                    let child_metrics = metrics[child_index];
                    node_metrics.nodes = checked_add(
                        node_metrics.nodes,
                        child_metrics.nodes,
                        "validated logical nodes",
                    )?;
                    node_metrics.operands = checked_add(
                        node_metrics.operands,
                        child_metrics.operands,
                        "validated logical operands",
                    )?;
                    node_metrics.string_bytes = checked_add(
                        node_metrics.string_bytes,
                        child_metrics.string_bytes,
                        "validated string bytes",
                    )?;
                    node_metrics.encoded_bytes = checked_add(
                        node_metrics.encoded_bytes,
                        checked_add(
                            child_metrics.encoded_bytes - WIRE_HEADER_LEN,
                            9,
                            "validated nested record",
                        )?,
                        "validated encoded bytes",
                    )?;
                }
                StoredOperand::Variable(value)
                | StoredOperand::String(value)
                | StoredOperand::Regex(value) => {
                    node_metrics.string_bytes = checked_add(
                        node_metrics.string_bytes,
                        value.len(),
                        "validated operand string",
                    )?;
                    node_metrics.encoded_bytes = checked_add(
                        node_metrics.encoded_bytes,
                        checked_add(9, value.len(), "validated string record")?,
                        "validated encoded bytes",
                    )?;
                }
                StoredOperand::Integer(_) | StoredOperand::Float(_) => {
                    node_metrics.encoded_bytes =
                        checked_add(node_metrics.encoded_bytes, 9, "validated wide operand")?;
                }
                StoredOperand::Bool(_) => {
                    node_metrics.encoded_bytes =
                        checked_add(node_metrics.encoded_bytes, 2, "validated boolean")?;
                }
                StoredOperand::Char(_) | StoredOperand::Opaque(_) => {
                    node_metrics.encoded_bytes =
                        checked_add(node_metrics.encoded_bytes, 5, "validated narrow operand")?;
                }
                StoredOperand::Null | StoredOperand::Array | StoredOperand::Object => {
                    node_metrics.encoded_bytes =
                        checked_add(node_metrics.encoded_bytes, 1, "validated unit operand")?;
                }
            }
        }
        if let Some(last_child) = previous_child {
            if last_child + 1 != parent {
                return Err(ExprKeyError::NonCanonicalChildOrder { parent });
            }
            subtree_starts[parent] = first_subtree;
        } else {
            subtree_starts[parent] = parent;
        }
        node_metrics.work =
            checked_add(node_metrics.nodes, node_metrics.operands, "validated work")?;
        enforce_metrics(node_metrics, limits)?;
        metrics.push(node_metrics);
    }
    if expected_start != operands.len() {
        return Err(ExprKeyError::InvalidOperandRange {
            node: nodes.len(),
            first: u64::try_from(expected_start).unwrap_or(u64::MAX),
            end: u64::try_from(expected_start).unwrap_or(u64::MAX),
            expected_start,
            operand_count: operands.len(),
        });
    }
    let root = nodes.len() - 1;
    if subtree_starts[root] != 0 {
        return Err(ExprKeyError::NonCanonicalChildOrder { parent: root });
    }
    for (node, actual) in parents.into_iter().enumerate() {
        let expected = usize::from(node != root);
        if actual != expected {
            return Err(ExprKeyError::InvalidParentCount {
                node,
                actual,
                expected,
            });
        }
    }
    Ok(metrics)
}

fn structural_classes(
    nodes: &[ExprNode],
    operands: &[StoredOperand],
) -> Result<(Vec<u64>, BTreeMap<ClassDescriptor, u64>), ExprKeyError> {
    let mut class_ids = Vec::new();
    class_ids
        .try_reserve(nodes.len())
        .map_err(|error| allocation("decoded structural classes", error))?;
    let mut interner = BTreeMap::new();
    for node in nodes {
        let first = usize::try_from(node.first).expect("validated class operand start");
        let len = usize::try_from(node.len).expect("validated class operand length");
        let mut class_operands = Vec::new();
        class_operands
            .try_reserve(len)
            .map_err(|error| allocation("decoded class operands", error))?;
        for operand in &operands[first..first + len] {
            class_operands.push(match operand {
                StoredOperand::Variable(value) => ClassOperand::Variable(value.clone()),
                StoredOperand::Integer(value) => ClassOperand::Integer(*value),
                StoredOperand::Float(value) => ClassOperand::Float(*value),
                StoredOperand::Bool(value) => ClassOperand::Bool(*value),
                StoredOperand::String(value) => ClassOperand::String(value.clone()),
                StoredOperand::Char(value) => ClassOperand::Char(*value),
                StoredOperand::Null => ClassOperand::Null,
                StoredOperand::Array => ClassOperand::Array,
                StoredOperand::Object => ClassOperand::Object,
                StoredOperand::Regex(value) => ClassOperand::Regex(value.clone()),
                StoredOperand::Expression(child) => {
                    ClassOperand::Expression(class_ids[*child as usize])
                }
                StoredOperand::Opaque(value) => ClassOperand::Opaque(*value),
            });
        }
        let descriptor = ClassDescriptor {
            operator: node.operator.clone(),
            operands: class_operands,
        };
        let next_class = u64_index(interner.len(), "decoded structural class id")?;
        let class = *interner.entry(descriptor).or_insert(next_class);
        class_ids.push(class);
    }
    Ok((class_ids, interner))
}

fn encode_canonical(key: &ExprKey, limits: ExprKeyLimits) -> Result<Vec<u8>, ExprKeyError> {
    enforce_metrics(key.metrics, limits)?;
    let mut output = Vec::new();
    output
        .try_reserve(key.metrics.encoded_bytes)
        .map_err(|error| allocation("canonical expression bytes", error))?;
    output.extend_from_slice(&WIRE_MAGIC);
    put_u64(
        &mut output,
        u64_index(key.arena.nodes.len(), "wire node count")?,
    );
    put_u64(
        &mut output,
        u64_index(key.arena.operands.len(), "wire operand count")?,
    );
    for node in &key.arena.nodes {
        put_str(&mut output, &node.operator)?;
        put_u64(&mut output, node.first);
        put_u64(&mut output, node.len);
    }
    for operand in &key.arena.operands {
        match operand {
            StoredOperand::Variable(value) => {
                output.push(TAG_VARIABLE);
                put_str(&mut output, value)?;
            }
            StoredOperand::Integer(value) => {
                output.push(TAG_INTEGER);
                output.extend_from_slice(&value.to_le_bytes());
            }
            StoredOperand::Float(value) => {
                output.push(TAG_FLOAT_BITS);
                put_u64(&mut output, *value);
            }
            StoredOperand::Bool(value) => {
                output.push(TAG_BOOL);
                output.push(u8::from(*value));
            }
            StoredOperand::String(value) => {
                output.push(TAG_STRING);
                put_str(&mut output, value)?;
            }
            StoredOperand::Char(value) => {
                output.push(TAG_CHAR);
                output.extend_from_slice(&u32::from(*value).to_le_bytes());
            }
            StoredOperand::Null => output.push(TAG_NULL),
            StoredOperand::Array => output.push(TAG_ARRAY),
            StoredOperand::Object => output.push(TAG_OBJECT),
            StoredOperand::Regex(value) => {
                output.push(TAG_REGEX);
                put_str(&mut output, value)?;
            }
            StoredOperand::Expression(value) => {
                output.push(TAG_EXPRESSION);
                put_u64(&mut output, *value);
            }
            StoredOperand::Opaque(value) => {
                output.push(TAG_OPAQUE);
                output.extend_from_slice(&value.as_u32().to_le_bytes());
            }
        }
    }
    debug_assert_eq!(output.len(), key.metrics.encoded_bytes);
    Ok(output)
}

fn add_string_operand_metrics(
    metrics: &mut ExprKeyMetrics,
    len: usize,
) -> Result<(), ExprKeyError> {
    metrics.string_bytes = checked_add(metrics.string_bytes, len, "operand string bytes")?;
    metrics.encoded_bytes = checked_add(
        metrics.encoded_bytes,
        checked_add(9, len, "string operand record")?,
        "expression encoded bytes",
    )?;
    Ok(())
}

fn enforce_metrics(metrics: ExprKeyMetrics, limits: ExprKeyLimits) -> Result<(), ExprKeyError> {
    enforce_limit(ExprKeyLimit::Nodes, metrics.nodes, limits.max_nodes)?;
    enforce_limit(
        ExprKeyLimit::Operands,
        metrics.operands,
        limits.max_operands,
    )?;
    enforce_limit(
        ExprKeyLimit::StringBytes,
        metrics.string_bytes,
        limits.max_string_bytes,
    )?;
    enforce_limit(
        ExprKeyLimit::EncodedBytes,
        metrics.encoded_bytes,
        limits.max_encoded_bytes,
    )?;
    enforce_limit(ExprKeyLimit::Work, metrics.work, limits.max_work)
}

fn enforce_limit(limit: ExprKeyLimit, actual: usize, maximum: usize) -> Result<(), ExprKeyError> {
    if actual > maximum {
        Err(ExprKeyError::LimitExceeded {
            limit,
            actual,
            maximum,
        })
    } else {
        Ok(())
    }
}

fn checked_add(left: usize, right: usize, context: &'static str) -> Result<usize, ExprKeyError> {
    left.checked_add(right)
        .ok_or(ExprKeyError::ArithmeticOverflow { context })
}

fn u64_index(value: usize, context: &'static str) -> Result<u64, ExprKeyError> {
    u64::try_from(value).map_err(|_| ExprKeyError::ArithmeticOverflow { context })
}

fn allocation(context: &'static str, error: impl fmt::Display) -> ExprKeyError {
    ExprKeyError::AllocationFailed {
        context,
        message: error.to_string(),
    }
}

fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

fn put_str(output: &mut Vec<u8>, value: &str) -> Result<(), ExprKeyError> {
    put_u64(output, u64_index(value.len(), "wire string length")?);
    output.extend_from_slice(value.as_bytes());
    Ok(())
}

struct Reader<'a> {
    bytes: &'a [u8],
    cursor: usize,
}

impl<'a> Reader<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, cursor: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.cursor
    }

    fn take(&mut self, len: usize, context: &'static str) -> Result<&'a [u8], ExprKeyError> {
        let end = self
            .cursor
            .checked_add(len)
            .ok_or(ExprKeyError::ArithmeticOverflow { context })?;
        let value = self
            .bytes
            .get(self.cursor..end)
            .ok_or(ExprKeyError::UnexpectedEof { context })?;
        self.cursor = end;
        Ok(value)
    }

    fn byte(&mut self, context: &'static str) -> Result<u8, ExprKeyError> {
        Ok(self.take(1, context)?[0])
    }

    fn u32(&mut self, context: &'static str) -> Result<u32, ExprKeyError> {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(self.take(4, context)?);
        Ok(u32::from_le_bytes(bytes))
    }

    fn u64(&mut self, context: &'static str) -> Result<u64, ExprKeyError> {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(self.take(8, context)?);
        Ok(u64::from_le_bytes(bytes))
    }

    fn i64(&mut self, context: &'static str) -> Result<i64, ExprKeyError> {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(self.take(8, context)?);
        Ok(i64::from_le_bytes(bytes))
    }

    fn usize(&mut self, context: &'static str) -> Result<usize, ExprKeyError> {
        usize::try_from(self.u64(context)?)
            .map_err(|_| ExprKeyError::ArithmeticOverflow { context })
    }

    fn arc_str(
        &mut self,
        context: &'static str,
        aggregate: &mut usize,
        limits: ExprKeyLimits,
    ) -> Result<Arc<str>, ExprKeyError> {
        let len = self.usize(context)?;
        *aggregate = checked_add(*aggregate, len, "decoded string bytes")?;
        enforce_limit(
            ExprKeyLimit::StringBytes,
            *aggregate,
            limits.max_string_bytes,
        )?;
        let bytes = self.take(len, context)?;
        let value = std::str::from_utf8(bytes).map_err(|_| ExprKeyError::InvalidUtf8)?;
        Ok(Arc::from(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use std::{collections::hash_map::DefaultHasher, thread};

    fn leaf() -> ExprKey {
        ExprKey::try_new(
            "leaf",
            vec![
                ExprOperand::Variable("x".into()),
                ExprOperand::Integer(-7),
                ExprOperand::Float(f64::NAN.to_bits()),
                ExprOperand::Bool(true),
                ExprOperand::String("text".into()),
                ExprOperand::Char('🦀'),
                ExprOperand::Null,
                ExprOperand::Array,
                ExprOperand::Object,
                ExprOperand::Regex("a+".into()),
                ExprOperand::Opaque(NodeId::new(91)),
            ],
        )
        .expect("shallow leaf")
    }

    fn hash(key: &ExprKey) -> u64 {
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn logical_views_preserve_operator_operand_order_and_exact_scalars() {
        let nested = leaf();
        let key = ExprKey::try_new(
            "+",
            vec![
                ExprOperand::Integer(1),
                ExprOperand::Expression(nested.clone()),
                ExprOperand::Integer(2),
            ],
        )
        .expect("nested key");
        assert_eq!(key.operator(), "+");
        assert_eq!(key.operands().len(), 3);
        assert_eq!(key.operands().get(0), Some(ExprOperandRef::Integer(1)));
        assert_eq!(key.operands().get(2), Some(ExprOperandRef::Integer(2)));
        let ExprOperandRef::Expression(child) = key.operands().get(1).expect("nested operand")
        else {
            panic!("middle operand is structural");
        };
        assert_eq!(child.operator(), "leaf");
        assert_eq!(child, nested.as_ref());
        assert_eq!(key.metrics().nodes, 2);
        assert_eq!(key.metrics().operands, 14);
    }

    #[test]
    fn canonical_codec_and_serde_preserve_float_bits_and_structure() {
        let key =
            ExprKey::try_new("neg", vec![ExprOperand::Expression(leaf())]).expect("nested key");
        let bytes = key.encode().expect("encode");
        let decoded = ExprKey::decode(&bytes).expect("decode");
        assert_eq!(decoded, key);
        assert_eq!(decoded.encode().expect("re-encode"), bytes);
        assert_eq!(decoded.cmp(&key), Ordering::Equal);
        assert_eq!(hash(&decoded), hash(&key));
        assert_eq!(format!("{decoded:?}"), format!("{key:?}"));

        #[cfg(feature = "serde")]
        {
            let json = serde_json::to_vec(&key).expect("serialize key");
            let from_json: ExprKey = serde_json::from_slice(&json).expect("deserialize key");
            assert_eq!(from_json, key);
        }
    }

    #[test]
    fn ordered_operands_are_permutation_sensitive() {
        let left = ExprKey::try_new(
            "+",
            vec![ExprOperand::Variable("x".into()), ExprOperand::Integer(1)],
        )
        .expect("left");
        let right = ExprKey::try_new(
            "+",
            vec![ExprOperand::Integer(1), ExprOperand::Variable("x".into())],
        )
        .expect("right");
        assert_ne!(left, right);
        assert_ne!(left.cmp(&right), Ordering::Equal);
        assert_ne!(
            left.encode().expect("left bytes"),
            right.encode().expect("right bytes")
        );
    }

    #[test]
    fn every_constructor_and_codec_limit_is_typed_and_monotone() {
        let key = leaf();
        let metrics = key.metrics();
        let dimensions = [
            (ExprKeyLimit::Nodes, metrics.nodes),
            (ExprKeyLimit::Operands, metrics.operands),
            (ExprKeyLimit::StringBytes, metrics.string_bytes),
            (ExprKeyLimit::EncodedBytes, metrics.encoded_bytes),
            (ExprKeyLimit::Work, metrics.work),
        ];
        for (dimension, exact) in dimensions {
            let mut limits = ExprKeyLimits::UNBOUNDED;
            match dimension {
                ExprKeyLimit::Nodes => limits.max_nodes = exact,
                ExprKeyLimit::Operands => limits.max_operands = exact,
                ExprKeyLimit::StringBytes => limits.max_string_bytes = exact,
                ExprKeyLimit::EncodedBytes => limits.max_encoded_bytes = exact,
                ExprKeyLimit::Work => limits.max_work = exact,
            }
            assert!(key.encode_with_limits(limits).is_ok());
            let bytes = key.encode().expect("canonical cap bytes");
            assert!(ExprKey::decode_with_limits(&bytes, limits).is_ok());
            if exact != 0 {
                match dimension {
                    ExprKeyLimit::Nodes => limits.max_nodes -= 1,
                    ExprKeyLimit::Operands => limits.max_operands -= 1,
                    ExprKeyLimit::StringBytes => limits.max_string_bytes -= 1,
                    ExprKeyLimit::EncodedBytes => limits.max_encoded_bytes -= 1,
                    ExprKeyLimit::Work => limits.max_work -= 1,
                }
                assert!(matches!(
                    key.encode_with_limits(limits),
                    Err(ExprKeyError::LimitExceeded { limit, .. }) if limit == dimension
                ));
                assert!(matches!(
                    ExprKey::decode_with_limits(&bytes, limits),
                    Err(ExprKeyError::LimitExceeded { limit, .. }) if limit == dimension
                ));
            }
        }
    }

    #[test]
    fn malformed_canonical_bytes_are_rejected_without_fallback() {
        let nested = ExprKey::try_new(
            "p",
            vec![ExprOperand::Expression(
                ExprKey::try_new("l", Vec::new()).expect("leaf"),
            )],
        )
        .expect("nested");
        let bytes = nested.encode().expect("canonical bytes");

        let mut invalid_prefix = bytes.clone();
        invalid_prefix[3] = 2;
        assert_eq!(
            ExprKey::decode(&invalid_prefix),
            Err(ExprKeyError::InvalidWirePrefix)
        );

        let mut forward = bytes.clone();
        forward[71..79].copy_from_slice(&1u64.to_le_bytes());
        assert!(matches!(
            ExprKey::decode(&forward),
            Err(ExprKeyError::InvalidChildReference { .. })
        ));

        let mut invalid_range = bytes.clone();
        invalid_range[54..62].copy_from_slice(&1u64.to_le_bytes());
        assert!(matches!(
            ExprKey::decode(&invalid_range),
            Err(ExprKeyError::InvalidOperandRange { .. })
        ));

        let mut invalid_tag = bytes.clone();
        invalid_tag[70] = 0xff;
        assert_eq!(
            ExprKey::decode(&invalid_tag),
            Err(ExprKeyError::InvalidTag { tag: 0xff })
        );

        let mut trailing = bytes.clone();
        trailing.push(0);
        assert_eq!(
            ExprKey::decode(&trailing),
            Err(ExprKeyError::TrailingBytes { remaining: 1 })
        );
        assert!(matches!(
            ExprKey::decode(&bytes[..bytes.len() - 1]),
            Err(ExprKeyError::UnexpectedEof { .. })
        ));

        let empty = [
            WIRE_MAGIC.as_slice(),
            &0u64.to_le_bytes(),
            &0u64.to_le_bytes(),
        ]
        .concat();
        assert_eq!(ExprKey::decode(&empty), Err(ExprKeyError::EmptyArena));

        let ordered = ExprKey::try_new(
            "p",
            vec![
                ExprOperand::Expression(ExprKey::try_new("l", Vec::new()).expect("left leaf")),
                ExprOperand::Expression(ExprKey::try_new("r", Vec::new()).expect("right leaf")),
            ],
        )
        .expect("ordered children");
        let mut reversed_children = ordered.encode().expect("ordered bytes");
        reversed_children[96..104].copy_from_slice(&1u64.to_le_bytes());
        reversed_children[105..113].copy_from_slice(&0u64.to_le_bytes());
        assert!(matches!(
            ExprKey::decode(&reversed_children),
            Err(ExprKeyError::NonCanonicalChildOrder { .. })
        ));
    }

    #[test]
    fn malformed_scalar_payloads_are_rejected() {
        let bool_key = ExprKey::try_new("b", vec![ExprOperand::Bool(true)]).expect("bool key");
        let mut bool_bytes = bool_key.encode().expect("bool bytes");
        *bool_bytes.last_mut().expect("boolean payload") = 2;
        assert_eq!(
            ExprKey::decode(&bool_bytes),
            Err(ExprKeyError::InvalidBool { byte: 2 })
        );

        let char_key = ExprKey::try_new("c", vec![ExprOperand::Char('x')]).expect("char key");
        let mut char_bytes = char_key.encode().expect("char bytes");
        let len = char_bytes.len();
        char_bytes[len - 4..].copy_from_slice(&0xd800u32.to_le_bytes());
        assert_eq!(
            ExprKey::decode(&char_bytes),
            Err(ExprKeyError::InvalidChar { value: 0xd800 })
        );

        let mut utf8 = ExprKey::try_new("x", Vec::new())
            .expect("string key")
            .encode()
            .expect("string bytes");
        utf8[28] = 0xff;
        assert_eq!(ExprKey::decode(&utf8), Err(ExprKeyError::InvalidUtf8));
    }

    proptest! {
        #[test]
        fn generated_chains_round_trip_canonicalize_and_obey_order_hash_laws(
            seed in any::<i64>(),
            steps in prop::collection::vec((0u8..5, any::<i16>(), any::<bool>()), 0..96),
        ) {
            let mut left = ExprKey::try_new("seed", vec![ExprOperand::Integer(seed)])
                .expect("generated seed");
            let mut right = left.clone();
            for (operator, scalar, before) in steps {
                let spelling = ["+", "-", "*", "/", "%"][operator as usize];
                let left_nested = ExprOperand::Expression(left);
                let right_nested = ExprOperand::Expression(right);
                let scalar = ExprOperand::Integer(i64::from(scalar));
                let mirror = scalar.clone();
                let left_operands = if before {
                    vec![scalar, left_nested]
                } else {
                    vec![left_nested, scalar]
                };
                let right_operands = if before {
                    vec![mirror, right_nested]
                } else {
                    vec![right_nested, mirror]
                };
                left = ExprKey::try_new(spelling, left_operands).expect("generated left");
                right = ExprKey::try_new(spelling, right_operands).expect("generated right");
            }
            prop_assert_eq!(&left, &right);
            prop_assert_eq!(left.cmp(&right), Ordering::Equal);
            prop_assert_eq!(hash(&left), hash(&right));
            let bytes = left.encode().expect("generated encode");
            let decoded = ExprKey::decode(&bytes).expect("generated decode");
            prop_assert_eq!(&decoded, &left);
            prop_assert_eq!(decoded.encode().expect("generated canonical encode"), bytes);
        }
    }

    #[test]
    fn depth_100_000_lifecycle_and_codec_fit_a_small_ordinary_stack() {
        thread::Builder::new()
            .name("expr-key-small-stack".into())
            .stack_size(256 * 1024)
            .spawn(|| {
                let mut key = ExprKey::try_new("seed", vec![ExprOperand::Variable("x".into())])
                    .expect("seed key");
                for depth in 0..100_000 {
                    let nested = ExprOperand::Expression(key);
                    let operands = if depth % 2 == 0 {
                        vec![nested]
                    } else {
                        vec![ExprOperand::Integer(depth), nested]
                    };
                    key = ExprKey::try_new("-", operands).expect("deep key");
                }
                assert_eq!(key.metrics().nodes, 100_001);
                let cloned = key.clone();
                assert_eq!(cloned, key);
                assert_eq!(hash(&cloned), hash(&key));
                let debug = format!("{key:?}");
                assert!(debug.starts_with("ExprKey { operator: \"-\""));
                let bytes = key.encode().expect("deep encode");
                let decoded = ExprKey::decode(&bytes).expect("deep decode");
                assert_eq!(decoded, key);
                #[cfg(feature = "serde")]
                {
                    let json = serde_json::to_vec(&key).expect("deep serialize");
                    let from_json: ExprKey =
                        serde_json::from_slice(&json).expect("deep deserialize");
                    assert_eq!(from_json, key);
                    drop(json);
                    drop(from_json);
                }
                drop(debug);
                drop(bytes);
                drop(decoded);
                drop(cloned);
                drop(key);
            })
            .expect("spawn small-stack thread")
            .join()
            .expect("small-stack expression lifecycle");
    }
}
