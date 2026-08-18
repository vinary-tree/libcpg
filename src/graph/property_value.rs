//! Stack-safe property values and their canonical flat codec.
//!
//! A [`PropertyValue`] is logically a tree, but it is owned as a postorder
//! arena. Child indices precede their parent and the root is the final node.
//! Clone and destruction are therefore flat `Vec` operations; equality,
//! formatting, validation, encoding, and decoding use explicit heap-backed
//! cursors rather than native recursion.

use std::fmt;
use std::sync::Arc;

use thiserror::Error;

#[cfg(feature = "serde")]
use serde::{
    de::{Error as _, SeqAccess, Visitor},
    ser::Error as _,
    Deserialize, Deserializer, Serialize, Serializer,
};

const WIRE_MAGIC: [u8; 4] = *b"CPV\x01";

const TAG_NULL: u8 = 0;
const TAG_STRING: u8 = 1;
const TAG_INT: u8 = 2;
const TAG_UINT: u8 = 3;
const TAG_BOOL: u8 = 4;
const TAG_FLOAT_BITS: u8 = 5;
const TAG_LIST: u8 = 6;

/// A resource dimension enforced by [`PropertyValueLimits`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropertyValueLimit {
    /// Number of logical scalar or list nodes.
    Nodes,
    /// Number of ordered parent-to-child list edges.
    Edges,
    /// Total bytes across all string payloads.
    StringBytes,
    /// Bytes in the canonical binary representation.
    EncodedBytes,
    /// Bounded node/edge machine steps.
    Work,
}

/// Independent resource limits for property construction and persistence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropertyValueLimits {
    /// Maximum number of logical nodes.
    pub max_nodes: usize,
    /// Maximum number of ordered list edges.
    pub max_edges: usize,
    /// Maximum aggregate UTF-8 string bytes.
    pub max_string_bytes: usize,
    /// Maximum canonical encoded bytes.
    pub max_encoded_bytes: usize,
    /// Maximum node/edge work-machine steps.
    pub max_work: usize,
}

impl PropertyValueLimits {
    /// Limits with no semantic cap below the host's address-space bound.
    pub const UNBOUNDED: Self = Self {
        max_nodes: usize::MAX,
        max_edges: usize::MAX,
        max_string_bytes: usize::MAX,
        max_encoded_bytes: usize::MAX,
        max_work: usize::MAX,
    };
}

impl Default for PropertyValueLimits {
    fn default() -> Self {
        Self {
            max_nodes: 1_000_000,
            max_edges: 1_000_000,
            max_string_bytes: 64 * 1024 * 1024,
            max_encoded_bytes: 256 * 1024 * 1024,
            max_work: 4_000_000,
        }
    }
}

/// Exact resource measurements for a validated property tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropertyValueMetrics {
    /// Number of logical nodes, including a standalone null.
    pub nodes: usize,
    /// Number of ordered list edges.
    pub edges: usize,
    /// Aggregate bytes across string payloads.
    pub string_bytes: usize,
    /// Node-plus-edge work-machine steps.
    pub work: usize,
}

impl PropertyValueMetrics {
    const NULL: Self = Self {
        nodes: 1,
        edges: 0,
        string_bytes: 0,
        work: 1,
    };
}

/// A typed failure from property construction, validation, or decoding.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum PropertyValueError {
    /// A named resource limit was exceeded.
    #[error("property-value {limit:?} limit exceeded: observed {actual}, maximum {maximum}")]
    LimitExceeded {
        /// Resource dimension that failed.
        limit: PropertyValueLimit,
        /// Observed amount.
        actual: usize,
        /// Configured maximum.
        maximum: usize,
    },
    /// Size or index arithmetic overflowed.
    #[error("property-value arithmetic overflow while computing {context}")]
    ArithmeticOverflow {
        /// Operation whose checked arithmetic failed.
        context: &'static str,
    },
    /// A fallible heap reservation failed.
    #[error("property-value allocation failed while reserving {context}: {message}")]
    AllocationFailed {
        /// Buffer or arena being reserved.
        context: &'static str,
        /// Allocator error text.
        message: String,
    },
    /// The canonical wire prefix or version is unsupported.
    #[error("invalid property-value wire prefix or version")]
    InvalidWirePrefix,
    /// The byte stream ended before a complete field was available.
    #[error("unexpected end of property-value bytes while reading {context}")]
    UnexpectedEof {
        /// Field being read.
        context: &'static str,
    },
    /// A node tag is outside the closed wire alphabet.
    #[error("invalid property-value node tag {tag}")]
    InvalidTag {
        /// Unknown tag byte.
        tag: u8,
    },
    /// A boolean payload was neither zero nor one.
    #[error("invalid property-value boolean byte {byte}")]
    InvalidBool {
        /// Invalid boolean byte.
        byte: u8,
    },
    /// A string payload was not UTF-8.
    #[error("invalid UTF-8 in property-value string")]
    InvalidUtf8,
    /// A child index does not precede its parent.
    #[error("property-value child {child} does not precede parent {parent}")]
    InvalidChildReference {
        /// Parent arena index.
        parent: usize,
        /// Invalid child arena index.
        child: u64,
    },
    /// Direct child roots are not in canonical source order.
    #[error("property-value children of node {parent} are not strictly increasing")]
    NonCanonicalChildOrder {
        /// Parent arena index.
        parent: usize,
    },
    /// A list's edge range is out of bounds or not the next canonical range.
    #[error(
        "property-value list node {parent} has edge range {first}..{end}; expected start {expected_start} within {edge_count} edges"
    )]
    InvalidEdgeRange {
        /// Parent arena index.
        parent: usize,
        /// First edge index declared by the node.
        first: u64,
        /// Exclusive edge end, or `u64::MAX` when arithmetic overflowed.
        end: u64,
        /// Next canonical edge offset.
        expected_start: usize,
        /// Total edge-tape length.
        edge_count: usize,
    },
    /// A non-root node has zero or multiple parents.
    #[error("property-value node {node} has {actual} parents; expected {expected}")]
    InvalidParentCount {
        /// Arena node with the invalid parent count.
        node: usize,
        /// Observed direct-parent count.
        actual: usize,
        /// Required direct-parent count.
        expected: usize,
    },
    /// A standalone null used a redundant arena node instead of the canonical empty arena.
    #[error("standalone null must use the canonical empty property-value arena")]
    NonCanonicalRootNull,
    /// Bytes remained after the one canonical value.
    #[error("{remaining} trailing bytes follow the property value")]
    TrailingBytes {
        /// Number of unconsumed bytes.
        remaining: usize,
    },
}

#[derive(Clone)]
enum PropertyValueNode {
    String(Arc<str>),
    Int(i64),
    Uint(u64),
    Bool(bool),
    FloatBits(u64),
    List { first: u64, len: u64 },
    Null,
}

/// A node metadata value with flat, stack-safe ownership.
///
/// Existing constructor spellings such as `PropertyValue::Int(3)`,
/// `PropertyValue::List(values)`, and `PropertyValue::Null` remain available.
/// Pattern matching moves to [`PropertyValue::kind`] because the physical
/// representation is deliberately not the logical recursive tree.
#[derive(Clone)]
pub struct PropertyValue {
    nodes: Vec<PropertyValueNode>,
    edges: Vec<u64>,
    metrics: PropertyValueMetrics,
}

/// A borrowed logical view of one property node.
#[derive(Clone, Copy)]
pub enum PropertyValueKind<'a> {
    /// UTF-8 string value.
    String(&'a str),
    /// Signed integer value.
    Int(i64),
    /// Unsigned integer value.
    Uint(u64),
    /// Boolean value.
    Bool(bool),
    /// IEEE-754 floating-point value.
    Float(f64),
    /// Ordered child values.
    List(PropertyList<'a>),
    /// Null or absent value.
    Null,
}

/// A borrowed reference to any node in a flat [`PropertyValue`] arena.
#[derive(Clone, Copy)]
pub struct PropertyValueRef<'a> {
    value: &'a PropertyValue,
    node: Option<u64>,
}

/// A borrowed, ordered list view.
#[derive(Clone, Copy)]
pub struct PropertyList<'a> {
    value: &'a PropertyValue,
    children: &'a [u64],
}

/// Iterator over a borrowed [`PropertyList`].
pub struct PropertyListIter<'a> {
    value: &'a PropertyValue,
    children: std::slice::Iter<'a, u64>,
}

#[allow(non_snake_case, non_upper_case_globals)]
impl PropertyValue {
    /// Canonical null value.
    pub const Null: Self = Self {
        nodes: Vec::new(),
        edges: Vec::new(),
        metrics: PropertyValueMetrics::NULL,
    };

    /// Constructs a string value while retaining the historical constructor spelling.
    pub fn String(value: Arc<str>) -> Self {
        let string_bytes = value.len();
        Self::single(PropertyValueNode::String(value), string_bytes)
    }

    /// Constructs a signed integer value.
    pub fn Int(value: i64) -> Self {
        Self::single(PropertyValueNode::Int(value), 0)
    }

    /// Constructs an unsigned integer value.
    pub fn Uint(value: u64) -> Self {
        Self::single(PropertyValueNode::Uint(value), 0)
    }

    /// Constructs a boolean value.
    pub fn Bool(value: bool) -> Self {
        Self::single(PropertyValueNode::Bool(value), 0)
    }

    /// Constructs a float while preserving its exact IEEE-754 bit pattern.
    pub fn Float(value: f64) -> Self {
        Self::single(PropertyValueNode::FloatBits(value.to_bits()), 0)
    }

    /// Constructs an ordered list with no semantic cap.
    ///
    /// This retains the historical infallible constructor. Persistence and
    /// hostile-input boundaries should use [`Self::try_list_with_limits`].
    pub fn List(children: Vec<Self>) -> Self {
        Self::try_list_with_limits(children, PropertyValueLimits::UNBOUNDED)
            .expect("a host-sized property list must fit its flat arena")
    }

    /// Constructs an ordered list while enforcing independent resource limits.
    pub fn try_list_with_limits(
        children: Vec<Self>,
        limits: PropertyValueLimits,
    ) -> Result<Self, PropertyValueError> {
        let mut metrics = PropertyValueMetrics {
            nodes: 1,
            edges: children.len(),
            string_bytes: 0,
            work: 0,
        };
        for child in &children {
            metrics.nodes = checked_add(metrics.nodes, child.metrics.nodes, "list node count")?;
            metrics.edges = checked_add(metrics.edges, child.metrics.edges, "list edge count")?;
            metrics.string_bytes = checked_add(
                metrics.string_bytes,
                child.metrics.string_bytes,
                "list string bytes",
            )?;
        }
        metrics.work = checked_add(metrics.nodes, metrics.edges, "list work")?;
        enforce_metrics(metrics, limits)?;

        if children.len() == 1 {
            let mut child = children.into_iter().next().expect("one list child");
            if child.nodes.is_empty() {
                child
                    .nodes
                    .try_reserve(2)
                    .map_err(|error| allocation("unary property-value nodes", error))?;
                child.nodes.push(PropertyValueNode::Null);
            } else {
                child
                    .nodes
                    .try_reserve(1)
                    .map_err(|error| allocation("unary property-value node", error))?;
            }
            child
                .edges
                .try_reserve(1)
                .map_err(|error| allocation("unary property-value edge", error))?;
            let root = u64_index(child.nodes.len() - 1, "unary child root")?;
            let first = u64_index(child.edges.len(), "unary list edge range")?;
            child.edges.push(root);
            child.nodes.push(PropertyValueNode::List { first, len: 1 });
            child.metrics = metrics;
            return Ok(child);
        }

        let physical_nodes = metrics.nodes;
        let mut nodes = Vec::new();
        nodes
            .try_reserve(physical_nodes)
            .map_err(|error| allocation("property-value list arena", error))?;
        let mut edges = Vec::new();
        edges
            .try_reserve(metrics.edges)
            .map_err(|error| allocation("property-value edge tape", error))?;
        let mut roots = Vec::new();
        roots
            .try_reserve(children.len())
            .map_err(|error| allocation("property-value child roots", error))?;

        for mut child in children {
            if child.nodes.is_empty() {
                let root = u64_index(nodes.len(), "null child index")?;
                nodes.push(PropertyValueNode::Null);
                roots.push(root);
                continue;
            }

            let node_offset = u64_index(nodes.len(), "child arena offset")?;
            let edge_offset = u64_index(edges.len(), "child edge offset")?;
            let child_root = checked_add_u64(
                node_offset,
                u64_index(child.nodes.len() - 1, "child root index")?,
                "shifted child root",
            )?;
            if node_offset == 0 && edge_offset == 0 && nodes.is_empty() && edges.is_empty() {
                nodes = child.nodes;
                edges = child.edges;
            } else {
                for node in &mut child.nodes {
                    if let PropertyValueNode::List { first, .. } = node {
                        *first = checked_add_u64(*first, edge_offset, "shifted edge range")?;
                    }
                }
                for index in &mut child.edges {
                    *index = checked_add_u64(*index, node_offset, "shifted child index")?;
                }
                nodes.extend(child.nodes);
                edges.extend(child.edges);
            }
            roots.push(child_root);
        }
        let first = u64_index(edges.len(), "list edge range")?;
        let len = u64_index(roots.len(), "list child count")?;
        edges.extend(roots);
        nodes.push(PropertyValueNode::List { first, len });

        let value = Self {
            nodes,
            edges,
            metrics,
        };
        Ok(value)
    }

    fn single(node: PropertyValueNode, string_bytes: usize) -> Self {
        Self {
            nodes: vec![node],
            edges: Vec::new(),
            metrics: PropertyValueMetrics {
                nodes: 1,
                edges: 0,
                string_bytes,
                work: 1,
            },
        }
    }

    fn list_children(&self, first: u64, len: u64) -> &[u64] {
        let first = usize::try_from(first).expect("validated edge-range start");
        let len = usize::try_from(len).expect("validated edge-range length");
        &self.edges[first..first + len]
    }

    /// Returns a borrowed logical view of the root.
    pub fn kind(&self) -> PropertyValueKind<'_> {
        self.as_ref().kind()
    }

    /// Returns a borrowed reference to the logical root.
    pub fn as_ref(&self) -> PropertyValueRef<'_> {
        PropertyValueRef {
            value: self,
            node: (!self.nodes.is_empty())
                .then(|| u64::try_from(self.nodes.len() - 1).expect("arena index fits u64")),
        }
    }

    /// Returns the value as a string reference if it is a string.
    pub fn as_str(&self) -> Option<&str> {
        self.as_ref().as_str()
    }

    /// Returns the value as an integer if it is a signed integer.
    pub fn as_int(&self) -> Option<i64> {
        self.as_ref().as_int()
    }

    /// Returns the value as an unsigned integer if it is an unsigned integer.
    pub fn as_uint(&self) -> Option<u64> {
        self.as_ref().as_uint()
    }

    /// Returns the value as a boolean if it is a boolean.
    pub fn as_bool(&self) -> Option<bool> {
        self.as_ref().as_bool()
    }

    /// Returns the value as a float if it is a float.
    pub fn as_float(&self) -> Option<f64> {
        self.as_ref().as_float()
    }

    /// Returns the value as an ordered list if it is a list.
    pub fn as_list(&self) -> Option<PropertyList<'_>> {
        self.as_ref().as_list()
    }

    /// Returns cached exact metrics for the canonical arena.
    pub fn metrics(&self) -> PropertyValueMetrics {
        self.metrics
    }

    /// Validates canonical tree shape and all configured resource limits.
    pub fn validate_with_limits(
        &self,
        limits: PropertyValueLimits,
    ) -> Result<PropertyValueMetrics, PropertyValueError> {
        if self.nodes.is_empty() {
            enforce_metrics(PropertyValueMetrics::NULL, limits)?;
            return Ok(PropertyValueMetrics::NULL);
        }

        let mut parents = Vec::new();
        parents
            .try_reserve(self.nodes.len())
            .map_err(|error| allocation("property-value parent census", error))?;
        parents.resize(self.nodes.len(), 0usize);

        let mut edges = 0usize;
        let mut string_bytes = 0usize;
        let mut work = 0usize;
        let mut expected_edge_start = 0usize;
        for (parent, node) in self.nodes.iter().enumerate() {
            work = checked_add(work, 1, "validation node work")?;
            match node {
                PropertyValueNode::String(value) => {
                    string_bytes =
                        checked_add(string_bytes, value.len(), "validation string bytes")?;
                }
                PropertyValueNode::List { first, len } => {
                    let invalid_range = |end| PropertyValueError::InvalidEdgeRange {
                        parent,
                        first: *first,
                        end,
                        expected_start: expected_edge_start,
                        edge_count: self.edges.len(),
                    };
                    let first_index =
                        usize::try_from(*first).map_err(|_| invalid_range(u64::MAX))?;
                    let len_index = usize::try_from(*len).map_err(|_| invalid_range(u64::MAX))?;
                    let end = first_index
                        .checked_add(len_index)
                        .ok_or_else(|| invalid_range(u64::MAX))?;
                    if first_index != expected_edge_start || end > self.edges.len() {
                        return Err(invalid_range(u64::try_from(end).unwrap_or(u64::MAX)));
                    }
                    let children = &self.edges[first_index..end];
                    expected_edge_start = end;
                    edges = checked_add(edges, children.len(), "validation edges")?;
                    work = checked_add(work, children.len(), "validation edge work")?;
                    let mut previous = None;
                    for &child in children {
                        let child_index = usize::try_from(child).map_err(|_| {
                            PropertyValueError::InvalidChildReference { parent, child }
                        })?;
                        if child_index >= parent {
                            return Err(PropertyValueError::InvalidChildReference {
                                parent,
                                child,
                            });
                        }
                        if previous.is_some_and(|prior| child <= prior) {
                            return Err(PropertyValueError::NonCanonicalChildOrder { parent });
                        }
                        previous = Some(child);
                        parents[child_index] =
                            checked_add(parents[child_index], 1, "validation parent count")?;
                    }
                }
                PropertyValueNode::Int(_)
                | PropertyValueNode::Uint(_)
                | PropertyValueNode::Bool(_)
                | PropertyValueNode::FloatBits(_)
                | PropertyValueNode::Null => {}
            }
        }

        if expected_edge_start != self.edges.len() {
            return Err(PropertyValueError::InvalidEdgeRange {
                parent: self.nodes.len(),
                first: u64::try_from(expected_edge_start).unwrap_or(u64::MAX),
                end: u64::try_from(expected_edge_start).unwrap_or(u64::MAX),
                expected_start: expected_edge_start,
                edge_count: self.edges.len(),
            });
        }

        if matches!(self.nodes.last(), Some(PropertyValueNode::Null)) {
            return Err(PropertyValueError::NonCanonicalRootNull);
        }
        let root = self.nodes.len() - 1;
        for (node, actual) in parents.into_iter().enumerate() {
            let expected = usize::from(node != root);
            if actual != expected {
                return Err(PropertyValueError::InvalidParentCount {
                    node,
                    actual,
                    expected,
                });
            }
        }

        let metrics = PropertyValueMetrics {
            nodes: self.nodes.len(),
            edges,
            string_bytes,
            work,
        };
        enforce_metrics(metrics, limits)?;
        if metrics != self.metrics {
            return Err(PropertyValueError::ArithmeticOverflow {
                context: "cached property-value metrics disagree with arena",
            });
        }
        Ok(metrics)
    }

    /// Encodes the value into the canonical, versioned `CPV1` byte format.
    pub fn encode(&self) -> Result<Vec<u8>, PropertyValueError> {
        self.encode_with_limits(PropertyValueLimits::default())
    }

    /// Encodes the value while enforcing explicit resource limits.
    pub fn encode_with_limits(
        &self,
        limits: PropertyValueLimits,
    ) -> Result<Vec<u8>, PropertyValueError> {
        self.validate_with_limits(limits)?;
        let encoded_len = self.encoded_len()?;
        enforce_limit(
            PropertyValueLimit::EncodedBytes,
            encoded_len,
            limits.max_encoded_bytes,
        )?;
        let mut output = Vec::new();
        output
            .try_reserve(encoded_len)
            .map_err(|error| allocation("canonical property-value bytes", error))?;
        output.extend_from_slice(&WIRE_MAGIC);
        put_u64(&mut output, u64_index(self.nodes.len(), "wire node count")?);
        for node in &self.nodes {
            match node {
                PropertyValueNode::Null => output.push(TAG_NULL),
                PropertyValueNode::String(value) => {
                    output.push(TAG_STRING);
                    put_u64(&mut output, u64_index(value.len(), "wire string length")?);
                    output.extend_from_slice(value.as_bytes());
                }
                PropertyValueNode::Int(value) => {
                    output.push(TAG_INT);
                    output.extend_from_slice(&value.to_le_bytes());
                }
                PropertyValueNode::Uint(value) => {
                    output.push(TAG_UINT);
                    put_u64(&mut output, *value);
                }
                PropertyValueNode::Bool(value) => {
                    output.push(TAG_BOOL);
                    output.push(u8::from(*value));
                }
                PropertyValueNode::FloatBits(bits) => {
                    output.push(TAG_FLOAT_BITS);
                    put_u64(&mut output, *bits);
                }
                PropertyValueNode::List { first, len } => {
                    let children = self.list_children(*first, *len);
                    output.push(TAG_LIST);
                    put_u64(&mut output, u64_index(children.len(), "wire list length")?);
                    for child in children {
                        put_u64(&mut output, *child);
                    }
                }
            }
        }
        debug_assert_eq!(output.len(), encoded_len);
        Ok(output)
    }

    /// Decodes one canonical property value with the default limits.
    pub fn decode(bytes: &[u8]) -> Result<Self, PropertyValueError> {
        Self::decode_with_limits(bytes, PropertyValueLimits::default())
    }

    /// Decodes one canonical property value using an explicit cursor machine.
    pub fn decode_with_limits(
        bytes: &[u8],
        limits: PropertyValueLimits,
    ) -> Result<Self, PropertyValueError> {
        enforce_limit(
            PropertyValueLimit::EncodedBytes,
            bytes.len(),
            limits.max_encoded_bytes,
        )?;
        let mut reader = Reader::new(bytes);
        if reader.take(4, "wire prefix")? != WIRE_MAGIC {
            return Err(PropertyValueError::InvalidWirePrefix);
        }
        let node_count = reader.usize("node count")?;
        let logical_node_count = node_count.max(1);
        enforce_limit(
            PropertyValueLimit::Nodes,
            logical_node_count,
            limits.max_nodes,
        )?;
        enforce_limit(
            PropertyValueLimit::Work,
            logical_node_count,
            limits.max_work,
        )?;
        let mut nodes = Vec::new();
        nodes
            .try_reserve(node_count)
            .map_err(|error| allocation("decoded property-value arena", error))?;
        let mut edges = Vec::new();
        let mut aggregate_edges = 0usize;
        let mut aggregate_strings = 0usize;

        for _ in 0..node_count {
            let node = match reader.byte("node tag")? {
                TAG_NULL => PropertyValueNode::Null,
                TAG_STRING => {
                    let len = reader.usize("string length")?;
                    aggregate_strings =
                        checked_add(aggregate_strings, len, "decoded aggregate string bytes")?;
                    enforce_limit(
                        PropertyValueLimit::StringBytes,
                        aggregate_strings,
                        limits.max_string_bytes,
                    )?;
                    let value = std::str::from_utf8(reader.take(len, "string payload")?)
                        .map_err(|_| PropertyValueError::InvalidUtf8)?;
                    PropertyValueNode::String(Arc::from(value))
                }
                TAG_INT => PropertyValueNode::Int(reader.i64("signed integer")?),
                TAG_UINT => PropertyValueNode::Uint(reader.u64("unsigned integer")?),
                TAG_BOOL => match reader.byte("boolean")? {
                    0 => PropertyValueNode::Bool(false),
                    1 => PropertyValueNode::Bool(true),
                    byte => return Err(PropertyValueError::InvalidBool { byte }),
                },
                TAG_FLOAT_BITS => PropertyValueNode::FloatBits(reader.u64("float bits")?),
                TAG_LIST => {
                    let child_count = reader.usize("list child count")?;
                    aggregate_edges =
                        checked_add(aggregate_edges, child_count, "decoded aggregate edge count")?;
                    enforce_limit(PropertyValueLimit::Edges, aggregate_edges, limits.max_edges)?;
                    enforce_limit(
                        PropertyValueLimit::Work,
                        checked_add(
                            logical_node_count,
                            aggregate_edges,
                            "decoded aggregate work",
                        )?,
                        limits.max_work,
                    )?;
                    edges
                        .try_reserve(child_count)
                        .map_err(|error| allocation("decoded list children", error))?;
                    let first = u64_index(edges.len(), "decoded list edge range")?;
                    for _ in 0..child_count {
                        edges.push(reader.u64("child index")?);
                    }
                    PropertyValueNode::List {
                        first,
                        len: u64_index(child_count, "decoded list child count")?,
                    }
                }
                tag => return Err(PropertyValueError::InvalidTag { tag }),
            };
            nodes.push(node);
        }
        if reader.remaining() != 0 {
            return Err(PropertyValueError::TrailingBytes {
                remaining: reader.remaining(),
            });
        }

        let mut value = Self {
            nodes,
            edges,
            metrics: PropertyValueMetrics::NULL,
        };
        value.metrics = value.measure()?;
        value.validate_with_limits(limits)?;
        Ok(value)
    }

    fn measure(&self) -> Result<PropertyValueMetrics, PropertyValueError> {
        if self.nodes.is_empty() {
            Ok(PropertyValueMetrics::NULL)
        } else {
            let string_bytes = self
                .nodes
                .iter()
                .try_fold(0usize, |total, node| match node {
                    PropertyValueNode::String(value) => {
                        checked_add(total, value.len(), "decoded string metrics")
                    }
                    _ => Ok(total),
                })?;
            Ok(PropertyValueMetrics {
                nodes: self.nodes.len(),
                edges: self.edges.len(),
                string_bytes,
                work: checked_add(self.nodes.len(), self.edges.len(), "decoded work metrics")?,
            })
        }
    }

    fn encoded_len(&self) -> Result<usize, PropertyValueError> {
        let mut len = 12usize;
        for node in &self.nodes {
            let node_len = match node {
                PropertyValueNode::Null => 1,
                PropertyValueNode::String(value) => {
                    checked_add(9, value.len(), "encoded string length")?
                }
                PropertyValueNode::Int(_)
                | PropertyValueNode::Uint(_)
                | PropertyValueNode::FloatBits(_) => 9,
                PropertyValueNode::Bool(_) => 2,
                PropertyValueNode::List { len, .. } => checked_add(
                    9,
                    usize::try_from(*len)
                        .map_err(|_| PropertyValueError::ArithmeticOverflow {
                            context: "encoded list length",
                        })?
                        .checked_mul(8)
                        .ok_or(PropertyValueError::ArithmeticOverflow {
                            context: "encoded list indices",
                        })?,
                    "encoded list length",
                )?,
            };
            len = checked_add(len, node_len, "encoded property-value length")?;
        }
        Ok(len)
    }
}

impl<'a> PropertyValueRef<'a> {
    /// Returns this node's logical kind.
    pub fn kind(self) -> PropertyValueKind<'a> {
        let Some(node) = self.node else {
            return PropertyValueKind::Null;
        };
        match &self.value.nodes[usize::try_from(node).expect("validated arena index")] {
            PropertyValueNode::String(value) => PropertyValueKind::String(value),
            PropertyValueNode::Int(value) => PropertyValueKind::Int(*value),
            PropertyValueNode::Uint(value) => PropertyValueKind::Uint(*value),
            PropertyValueNode::Bool(value) => PropertyValueKind::Bool(*value),
            PropertyValueNode::FloatBits(bits) => PropertyValueKind::Float(f64::from_bits(*bits)),
            PropertyValueNode::List { first, len } => PropertyValueKind::List(PropertyList {
                value: self.value,
                children: self.value.list_children(*first, *len),
            }),
            PropertyValueNode::Null => PropertyValueKind::Null,
        }
    }

    /// Returns a string payload when this node is a string.
    pub fn as_str(self) -> Option<&'a str> {
        match self.kind() {
            PropertyValueKind::String(value) => Some(value),
            _ => None,
        }
    }

    /// Returns a signed integer payload when this node is signed.
    pub fn as_int(self) -> Option<i64> {
        match self.kind() {
            PropertyValueKind::Int(value) => Some(value),
            _ => None,
        }
    }

    /// Returns an unsigned integer payload when this node is unsigned.
    pub fn as_uint(self) -> Option<u64> {
        match self.kind() {
            PropertyValueKind::Uint(value) => Some(value),
            _ => None,
        }
    }

    /// Returns a boolean payload when this node is boolean.
    pub fn as_bool(self) -> Option<bool> {
        match self.kind() {
            PropertyValueKind::Bool(value) => Some(value),
            _ => None,
        }
    }

    /// Returns a float payload when this node is a float.
    pub fn as_float(self) -> Option<f64> {
        match self.kind() {
            PropertyValueKind::Float(value) => Some(value),
            _ => None,
        }
    }

    /// Returns a list view when this node is a list.
    pub fn as_list(self) -> Option<PropertyList<'a>> {
        match self.kind() {
            PropertyValueKind::List(value) => Some(value),
            _ => None,
        }
    }
}

impl<'a> PropertyList<'a> {
    /// Returns the number of direct children.
    pub fn len(self) -> usize {
        self.children.len()
    }

    /// Returns whether the list has no direct children.
    pub fn is_empty(self) -> bool {
        self.children.is_empty()
    }

    /// Returns one direct child by source-order index.
    pub fn get(self, index: usize) -> Option<PropertyValueRef<'a>> {
        self.children
            .get(index)
            .copied()
            .map(|node| PropertyValueRef {
                value: self.value,
                node: Some(node),
            })
    }

    /// Iterates over direct children in source order.
    pub fn iter(self) -> PropertyListIter<'a> {
        PropertyListIter {
            value: self.value,
            children: self.children.iter(),
        }
    }
}

impl<'a> IntoIterator for PropertyList<'a> {
    type Item = PropertyValueRef<'a>;
    type IntoIter = PropertyListIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl<'a> Iterator for PropertyListIter<'a> {
    type Item = PropertyValueRef<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.children.next().copied().map(|node| PropertyValueRef {
            value: self.value,
            node: Some(node),
        })
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.children.size_hint()
    }
}

impl ExactSizeIterator for PropertyListIter<'_> {}

impl PartialEq for PropertyValue {
    fn eq(&self, other: &Self) -> bool {
        enum Op<'a> {
            Compare(PropertyValueRef<'a>, PropertyValueRef<'a>),
            List {
                left: PropertyList<'a>,
                right: PropertyList<'a>,
                next: usize,
            },
        }

        let mut work = vec![Op::Compare(self.as_ref(), other.as_ref())];
        while let Some(op) = work.pop() {
            match op {
                Op::Compare(left, right) => match (left.kind(), right.kind()) {
                    (PropertyValueKind::String(left), PropertyValueKind::String(right))
                        if left == right => {}
                    (PropertyValueKind::Int(left), PropertyValueKind::Int(right))
                        if left == right => {}
                    (PropertyValueKind::Uint(left), PropertyValueKind::Uint(right))
                        if left == right => {}
                    (PropertyValueKind::Bool(left), PropertyValueKind::Bool(right))
                        if left == right => {}
                    (PropertyValueKind::Float(left), PropertyValueKind::Float(right))
                        if left == right => {}
                    (PropertyValueKind::Null, PropertyValueKind::Null) => {}
                    (PropertyValueKind::List(left), PropertyValueKind::List(right))
                        if left.len() == right.len() =>
                    {
                        work.push(Op::List {
                            left,
                            right,
                            next: 0,
                        });
                    }
                    _ => return false,
                },
                Op::List { left, right, next } => {
                    if next < left.len() {
                        work.push(Op::List {
                            left,
                            right,
                            next: next + 1,
                        });
                        work.push(Op::Compare(
                            left.get(next).expect("validated left child"),
                            right.get(next).expect("validated right child"),
                        ));
                    }
                }
            }
        }
        true
    }
}

impl fmt::Debug for PropertyValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        enum Op<'a> {
            Value(PropertyValueRef<'a>),
            List { list: PropertyList<'a>, next: usize },
        }

        let mut work = vec![Op::Value(self.as_ref())];
        while let Some(op) = work.pop() {
            match op {
                Op::Value(value) => match value.kind() {
                    PropertyValueKind::String(value) => {
                        write!(formatter, "String({value:?})")?;
                    }
                    PropertyValueKind::Int(value) => write!(formatter, "Int({value:?})")?,
                    PropertyValueKind::Uint(value) => write!(formatter, "Uint({value:?})")?,
                    PropertyValueKind::Bool(value) => write!(formatter, "Bool({value:?})")?,
                    PropertyValueKind::Float(value) => write!(formatter, "Float({value:?})")?,
                    PropertyValueKind::Null => formatter.write_str("Null")?,
                    PropertyValueKind::List(list) => {
                        formatter.write_str("List([")?;
                        work.push(Op::List { list, next: 0 });
                    }
                },
                Op::List { list, next } => {
                    if next == list.len() {
                        formatter.write_str("])")?;
                    } else {
                        if next != 0 {
                            formatter.write_str(", ")?;
                        }
                        work.push(Op::List {
                            list,
                            next: next + 1,
                        });
                        work.push(Op::Value(list.get(next).expect("validated debug child")));
                    }
                }
            }
        }
        Ok(())
    }
}

#[cfg(feature = "serde")]
impl Serialize for PropertyValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let bytes = self.encode().map_err(S::Error::custom)?;
        serializer.serialize_bytes(&bytes)
    }
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for PropertyValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct BytesVisitor;

        impl<'de> Visitor<'de> for BytesVisitor {
            type Value = PropertyValue;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("canonical versioned CPV1 bytes")
            }

            fn visit_bytes<E>(self, bytes: &[u8]) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                PropertyValue::decode(bytes).map_err(E::custom)
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
                let limits = PropertyValueLimits::default();
                let initial = sequence
                    .size_hint()
                    .unwrap_or(0)
                    .min(limits.max_encoded_bytes);
                let mut bytes = Vec::new();
                bytes.try_reserve(initial).map_err(A::Error::custom)?;
                while let Some(byte) = sequence.next_element::<u8>()? {
                    if bytes.len() == limits.max_encoded_bytes {
                        return Err(A::Error::custom(PropertyValueError::LimitExceeded {
                            limit: PropertyValueLimit::EncodedBytes,
                            actual: bytes.len().saturating_add(1),
                            maximum: limits.max_encoded_bytes,
                        }));
                    }
                    bytes.try_reserve(1).map_err(A::Error::custom)?;
                    bytes.push(byte);
                }
                PropertyValue::decode_with_limits(&bytes, limits).map_err(A::Error::custom)
            }
        }

        deserializer.deserialize_bytes(BytesVisitor)
    }
}

fn enforce_metrics(
    metrics: PropertyValueMetrics,
    limits: PropertyValueLimits,
) -> Result<(), PropertyValueError> {
    enforce_limit(PropertyValueLimit::Nodes, metrics.nodes, limits.max_nodes)?;
    enforce_limit(PropertyValueLimit::Edges, metrics.edges, limits.max_edges)?;
    enforce_limit(
        PropertyValueLimit::StringBytes,
        metrics.string_bytes,
        limits.max_string_bytes,
    )?;
    enforce_limit(PropertyValueLimit::Work, metrics.work, limits.max_work)
}

fn enforce_limit(
    limit: PropertyValueLimit,
    actual: usize,
    maximum: usize,
) -> Result<(), PropertyValueError> {
    if actual > maximum {
        Err(PropertyValueError::LimitExceeded {
            limit,
            actual,
            maximum,
        })
    } else {
        Ok(())
    }
}

fn checked_add(
    left: usize,
    right: usize,
    context: &'static str,
) -> Result<usize, PropertyValueError> {
    left.checked_add(right)
        .ok_or(PropertyValueError::ArithmeticOverflow { context })
}

fn checked_add_u64(
    left: u64,
    right: u64,
    context: &'static str,
) -> Result<u64, PropertyValueError> {
    left.checked_add(right)
        .ok_or(PropertyValueError::ArithmeticOverflow { context })
}

fn u64_index(value: usize, context: &'static str) -> Result<u64, PropertyValueError> {
    u64::try_from(value).map_err(|_| PropertyValueError::ArithmeticOverflow { context })
}

fn allocation(
    context: &'static str,
    error: std::collections::TryReserveError,
) -> PropertyValueError {
    PropertyValueError::AllocationFailed {
        context,
        message: error.to_string(),
    }
}

fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

struct Reader<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }

    fn take(
        &mut self,
        length: usize,
        context: &'static str,
    ) -> Result<&'a [u8], PropertyValueError> {
        if length > self.remaining() {
            return Err(PropertyValueError::UnexpectedEof { context });
        }
        let end = self.position + length;
        let value = &self.bytes[self.position..end];
        self.position = end;
        Ok(value)
    }

    fn byte(&mut self, context: &'static str) -> Result<u8, PropertyValueError> {
        Ok(self.take(1, context)?[0])
    }

    fn u64(&mut self, context: &'static str) -> Result<u64, PropertyValueError> {
        let bytes = self.take(8, context)?;
        Ok(u64::from_le_bytes(
            bytes
                .try_into()
                .expect("reader returned exactly eight bytes"),
        ))
    }

    fn i64(&mut self, context: &'static str) -> Result<i64, PropertyValueError> {
        let bytes = self.take(8, context)?;
        Ok(i64::from_le_bytes(
            bytes
                .try_into()
                .expect("reader returned exactly eight bytes"),
        ))
    }

    fn usize(&mut self, context: &'static str) -> Result<usize, PropertyValueError> {
        usize::try_from(self.u64(context)?)
            .map_err(|_| PropertyValueError::ArithmeticOverflow { context })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[derive(Clone, Debug)]
    enum OracleValue {
        String(String),
        Int(i64),
        Uint(u64),
        Bool(bool),
        Float(f64),
        List(Vec<OracleValue>),
        Null,
    }

    impl OracleValue {
        fn into_property(self) -> PropertyValue {
            match self {
                Self::String(value) => PropertyValue::String(value.into()),
                Self::Int(value) => PropertyValue::Int(value),
                Self::Uint(value) => PropertyValue::Uint(value),
                Self::Bool(value) => PropertyValue::Bool(value),
                Self::Float(value) => PropertyValue::Float(value),
                Self::List(children) => PropertyValue::List(
                    children
                        .into_iter()
                        .map(OracleValue::into_property)
                        .collect(),
                ),
                Self::Null => PropertyValue::Null,
            }
        }
    }

    fn oracle_strategy() -> impl Strategy<Value = OracleValue> {
        let leaf = prop_oneof![
            "[a-z]{0,8}".prop_map(OracleValue::String),
            any::<i64>().prop_map(OracleValue::Int),
            any::<u64>().prop_map(OracleValue::Uint),
            any::<bool>().prop_map(OracleValue::Bool),
            any::<i32>().prop_map(|value| OracleValue::Float(f64::from(value))),
            Just(OracleValue::Null),
        ];
        leaf.prop_recursive(6, 128, 6, |inner| {
            prop::collection::vec(inner, 0..6).prop_map(OracleValue::List)
        })
    }

    #[test]
    fn scalar_and_list_views_preserve_logical_semantics() {
        let value = PropertyValue::List(vec![
            PropertyValue::String("name".into()),
            PropertyValue::Int(-3),
            PropertyValue::Uint(5),
            PropertyValue::Bool(true),
            PropertyValue::Float(-0.0),
            PropertyValue::Null,
            PropertyValue::List(Vec::new()),
        ]);
        let list = value.as_list().expect("list root");
        assert_eq!(list.len(), 7);
        assert_eq!(list.get(0).and_then(PropertyValueRef::as_str), Some("name"));
        assert_eq!(list.get(1).and_then(PropertyValueRef::as_int), Some(-3));
        assert_eq!(list.get(2).and_then(PropertyValueRef::as_uint), Some(5));
        assert_eq!(list.get(3).and_then(PropertyValueRef::as_bool), Some(true));
        assert_eq!(
            list.get(4)
                .and_then(PropertyValueRef::as_float)
                .map(f64::to_bits),
            Some((-0.0f64).to_bits())
        );
        assert!(matches!(
            list.get(5).expect("null child").kind(),
            PropertyValueKind::Null
        ));
        assert!(list.get(6).and_then(PropertyValueRef::as_list).is_some());
        assert_eq!(
            format!("{value:?}"),
            "List([String(\"name\"), Int(-3), Uint(5), Bool(true), Float(-0.0), Null, List([])])"
        );
    }

    #[test]
    fn canonical_codec_has_a_stable_golden_and_rejects_trailing_bytes() {
        let value = PropertyValue::List(vec![PropertyValue::Int(-1), PropertyValue::Null]);
        let bytes = value.encode().expect("encode");
        assert_eq!(
            bytes,
            vec![
                b'C', b'P', b'V', 1, 3, 0, 0, 0, 0, 0, 0, 0, TAG_INT, 0xff, 0xff, 0xff, 0xff, 0xff,
                0xff, 0xff, 0xff, TAG_NULL, TAG_LIST, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 1, 0, 0, 0, 0, 0, 0, 0,
            ]
        );
        assert_eq!(PropertyValue::decode(&bytes).expect("decode"), value);

        let mut trailing = bytes;
        trailing.push(0xaa);
        assert!(matches!(
            PropertyValue::decode(&trailing),
            Err(PropertyValueError::TrailingBytes { remaining: 1 })
        ));
    }

    #[test]
    fn malformed_arenas_and_wire_tags_are_rejected() {
        let mut invalid_prefix = Vec::from(*b"XPV\x01");
        put_u64(&mut invalid_prefix, 0);
        assert!(matches!(
            PropertyValue::decode(&invalid_prefix),
            Err(PropertyValueError::InvalidWirePrefix)
        ));

        let mut forward = Vec::from(WIRE_MAGIC);
        put_u64(&mut forward, 1);
        forward.push(TAG_LIST);
        put_u64(&mut forward, 1);
        put_u64(&mut forward, 0);
        assert!(matches!(
            PropertyValue::decode(&forward),
            Err(PropertyValueError::InvalidChildReference {
                parent: 0,
                child: 0
            })
        ));

        let mut invalid_tag = Vec::from(WIRE_MAGIC);
        put_u64(&mut invalid_tag, 1);
        invalid_tag.push(99);
        assert!(matches!(
            PropertyValue::decode(&invalid_tag),
            Err(PropertyValueError::InvalidTag { tag: 99 })
        ));

        let mut root_null = Vec::from(WIRE_MAGIC);
        put_u64(&mut root_null, 1);
        root_null.push(TAG_NULL);
        assert!(matches!(
            PropertyValue::decode(&root_null),
            Err(PropertyValueError::NonCanonicalRootNull)
        ));

        let mut invalid_bool = Vec::from(WIRE_MAGIC);
        put_u64(&mut invalid_bool, 1);
        invalid_bool.extend_from_slice(&[TAG_BOOL, 2]);
        assert!(matches!(
            PropertyValue::decode(&invalid_bool),
            Err(PropertyValueError::InvalidBool { byte: 2 })
        ));

        let mut invalid_utf8 = Vec::from(WIRE_MAGIC);
        put_u64(&mut invalid_utf8, 1);
        invalid_utf8.push(TAG_STRING);
        put_u64(&mut invalid_utf8, 1);
        invalid_utf8.push(0xff);
        assert!(matches!(
            PropertyValue::decode(&invalid_utf8),
            Err(PropertyValueError::InvalidUtf8)
        ));

        assert!(matches!(
            PropertyValue::decode(&WIRE_MAGIC),
            Err(PropertyValueError::UnexpectedEof { .. })
        ));

        let mut noncanonical_order = Vec::from(WIRE_MAGIC);
        put_u64(&mut noncanonical_order, 3);
        noncanonical_order.push(TAG_INT);
        noncanonical_order.extend_from_slice(&1_i64.to_le_bytes());
        noncanonical_order.push(TAG_INT);
        noncanonical_order.extend_from_slice(&2_i64.to_le_bytes());
        noncanonical_order.push(TAG_LIST);
        put_u64(&mut noncanonical_order, 2);
        put_u64(&mut noncanonical_order, 1);
        put_u64(&mut noncanonical_order, 0);
        assert!(matches!(
            PropertyValue::decode(&noncanonical_order),
            Err(PropertyValueError::NonCanonicalChildOrder { parent: 2 })
        ));

        let mut missing_parent = Vec::from(WIRE_MAGIC);
        put_u64(&mut missing_parent, 3);
        missing_parent.push(TAG_INT);
        missing_parent.extend_from_slice(&1_i64.to_le_bytes());
        missing_parent.push(TAG_INT);
        missing_parent.extend_from_slice(&2_i64.to_le_bytes());
        missing_parent.push(TAG_LIST);
        put_u64(&mut missing_parent, 1);
        put_u64(&mut missing_parent, 0);
        assert!(matches!(
            PropertyValue::decode(&missing_parent),
            Err(PropertyValueError::InvalidParentCount {
                node: 1,
                actual: 0,
                expected: 1
            })
        ));

        let invalid_range = PropertyValue {
            nodes: vec![
                PropertyValueNode::Int(1),
                PropertyValueNode::List { first: 1, len: 1 },
            ],
            edges: vec![0],
            metrics: PropertyValueMetrics {
                nodes: 2,
                edges: 1,
                string_bytes: 0,
                work: 3,
            },
        };
        assert!(matches!(
            invalid_range.validate_with_limits(PropertyValueLimits::UNBOUNDED),
            Err(PropertyValueError::InvalidEdgeRange { parent: 1, .. })
        ));
    }

    #[test]
    fn every_limit_reports_its_own_dimension() {
        let value = PropertyValue::List(vec![PropertyValue::String("abc".into())]);
        let baseline = PropertyValueLimits::default();
        let cases = [
            (
                PropertyValueLimit::Nodes,
                PropertyValueLimits {
                    max_nodes: 1,
                    ..baseline
                },
            ),
            (
                PropertyValueLimit::Edges,
                PropertyValueLimits {
                    max_edges: 0,
                    ..baseline
                },
            ),
            (
                PropertyValueLimit::StringBytes,
                PropertyValueLimits {
                    max_string_bytes: 2,
                    ..baseline
                },
            ),
            (
                PropertyValueLimit::EncodedBytes,
                PropertyValueLimits {
                    max_encoded_bytes: 1,
                    ..baseline
                },
            ),
            (
                PropertyValueLimit::Work,
                PropertyValueLimits {
                    max_work: 2,
                    ..baseline
                },
            ),
        ];
        for (expected, limits) in cases {
            assert!(matches!(
                value.encode_with_limits(limits),
                Err(PropertyValueError::LimitExceeded { limit, .. }) if limit == expected
            ));
        }
    }

    #[test]
    fn nan_and_signed_zero_keep_the_old_f64_equality_rules() {
        let nan = PropertyValue::Float(f64::from_bits(0x7ff8_0000_0000_0042));
        assert_ne!(nan, nan.clone());
        let nan_bytes = nan.encode().expect("NaN bytes");
        assert_eq!(
            PropertyValue::decode(&nan_bytes)
                .expect("NaN decode")
                .as_float()
                .expect("float")
                .to_bits(),
            0x7ff8_0000_0000_0042
        );
        assert_eq!(PropertyValue::Float(-0.0), PropertyValue::Float(0.0));
        assert_ne!(
            PropertyValue::Float(-0.0).encode().expect("negative zero"),
            PropertyValue::Float(0.0).encode().expect("positive zero")
        );
        for infinity in [f64::NEG_INFINITY, f64::INFINITY] {
            let value = PropertyValue::Float(infinity);
            assert_eq!(
                PropertyValue::decode(&value.encode().expect("infinity bytes"))
                    .expect("infinity decode")
                    .as_float(),
                Some(infinity)
            );
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn flat_machine_matches_bounded_recursive_debug_and_round_trip(oracle in oracle_strategy()) {
            let expected_debug = format!("{oracle:?}");
            let value = oracle.into_property();
            prop_assert_eq!(format!("{value:?}"), expected_debug);
            prop_assert!(value == value.clone());

            let first = value.encode_with_limits(PropertyValueLimits::UNBOUNDED)?;
            let decoded = PropertyValue::decode_with_limits(
                &first,
                PropertyValueLimits::UNBOUNDED,
            )?;
            prop_assert!(decoded == value);
            prop_assert_eq!(decoded.encode_with_limits(PropertyValueLimits::UNBOUNDED)?, first);
        }

        #[test]
        fn exact_resource_limits_are_monotone(oracle in oracle_strategy()) {
            let value = oracle.into_property();
            let bytes = value.encode_with_limits(PropertyValueLimits::UNBOUNDED)?;
            let metrics = value.metrics();
            let exact = PropertyValueLimits {
                max_nodes: metrics.nodes,
                max_edges: metrics.edges,
                max_string_bytes: metrics.string_bytes,
                max_encoded_bytes: bytes.len(),
                max_work: metrics.work,
            };
            prop_assert!(value.encode_with_limits(exact).is_ok());
            prop_assert!(PropertyValue::decode_with_limits(&bytes, exact).is_ok());

            let dimensions = [
                (PropertyValueLimit::Nodes, metrics.nodes),
                (PropertyValueLimit::Edges, metrics.edges),
                (PropertyValueLimit::StringBytes, metrics.string_bytes),
                (PropertyValueLimit::EncodedBytes, bytes.len()),
                (PropertyValueLimit::Work, metrics.work),
            ];
            for (dimension, observed) in dimensions {
                if observed == 0 {
                    continue;
                }
                let mut smaller = exact;
                match dimension {
                    PropertyValueLimit::Nodes => smaller.max_nodes -= 1,
                    PropertyValueLimit::Edges => smaller.max_edges -= 1,
                    PropertyValueLimit::StringBytes => smaller.max_string_bytes -= 1,
                    PropertyValueLimit::EncodedBytes => smaller.max_encoded_bytes -= 1,
                    PropertyValueLimit::Work => smaller.max_work -= 1,
                }
                let failed_at_expected_dimension = matches!(
                    value.encode_with_limits(smaller),
                    Err(PropertyValueError::LimitExceeded { limit, .. }) if limit == dimension
                );
                prop_assert!(failed_at_expected_dimension);
            }
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_embeds_the_canonical_stack_safe_bytes() {
        let value = PropertyValue::List(vec![PropertyValue::Bool(true), PropertyValue::Null]);
        let json = serde_json::to_string(&value).expect("serialize property bytes");
        let decoded: PropertyValue = serde_json::from_str(&json).expect("deserialize property");
        assert_eq!(decoded, value);
        assert_eq!(
            decoded.encode().expect("canonical bytes"),
            value.encode().unwrap()
        );
    }
}
