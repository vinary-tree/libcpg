# Graph API Reference

This reference documents the core graph types and traits in libcpg.

## Core Types

### CodePropertyGraph

The main graph structure combining AST, CFG, and DFG.

```rust
pub struct CodePropertyGraph {
    language: Language,
    nodes: FxHashMap<NodeId, CpgNode>,
    edges: Vec<CpgEdge>,
    ast_root: Option<NodeId>,
    functions: Vec<NodeId>,
    // ...
}
```

#### Construction

| Method | Description |
|--------|-------------|
| `CodePropertyGraph::new(language: Language)` | Create empty graph |
| `TreeSitterCpgBuilder::build(source, language)` | Build from source |

#### Node Operations

| Method | Returns | Description |
|--------|---------|-------------|
| `node(id: NodeId)` | `Option<&CpgNode>` | Get node by ID |
| `node_mut(id: NodeId)` | `Option<&mut CpgNode>` | Get mutable node |
| `add_node(node: CpgNode)` | `NodeId` | Add node, return ID |
| `remove_node(id: NodeId)` | `Option<CpgNode>` | Remove and return node |
| `nodes()` | `impl Iterator<Item = &CpgNode>` | All nodes |
| `node_count()` | `usize` | Number of nodes |

#### Edge Operations

| Method | Returns | Description |
|--------|---------|-------------|
| `edge(id: EdgeId)` | `Option<&CpgEdge>` | Get edge by ID |
| `add_edge(edge: CpgEdge)` | `EdgeId` | Add edge |
| `edges()` | `impl Iterator<Item = &CpgEdge>` | All edges |
| `edges_from(node: NodeId)` | `impl Iterator<Item = &CpgEdge>` | Outgoing edges |
| `edges_to(node: NodeId)` | `impl Iterator<Item = &CpgEdge>` | Incoming edges |
| `edge_count()` | `usize` | Number of edges |

#### Query Operations

| Method | Returns | Description |
|--------|---------|-------------|
| `nodes_of_kind(kind: CpgNodeKind)` | `impl Iterator<Item = &CpgNode>` | Filter by kind |
| `ast_root()` | `Option<NodeId>` | Root of AST |
| `functions()` | `impl Iterator<Item = &CpgNode>` | All function nodes |
| `ast_children(node: NodeId)` | `impl Iterator<Item = NodeId>` | Direct AST children |
| `ast_parent(node: NodeId)` | `Option<NodeId>` | AST parent |
| `ast_descendants(node: NodeId)` | `impl Iterator<Item = NodeId>` | All AST descendants |
| `cfg_successors(node: NodeId)` | `impl Iterator<Item = NodeId>` | CFG successors |
| `cfg_predecessors(node: NodeId)` | `impl Iterator<Item = NodeId>` | CFG predecessors |
| `dfg_uses(node: NodeId)` | `impl Iterator<Item = NodeId>` | Data flow uses |
| `dfg_defs(node: NodeId)` | `impl Iterator<Item = NodeId>` | Data flow definitions |

#### Example

```rust
use libcpg::{CodePropertyGraph, Language, TreeSitterCpgBuilder};

// Build CPG
let builder = TreeSitterCpgBuilder::new();
let cpg = builder.build(source_code, Language::Rust)?;

// Query nodes
for func in cpg.functions() {
    println!("Function: {}", func.name().unwrap_or("<anonymous>"));

    // Get all statements in function
    for node_id in cpg.ast_descendants(func.id()) {
        if let Some(node) = cpg.node(node_id) {
            if node.is_statement() {
                println!("  Statement at line {}", node.source_range.start_line);
            }
        }
    }
}
```

---

### NodeId

Unique identifier for nodes.

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct NodeId(u32);
```

| Method | Returns | Description |
|--------|---------|-------------|
| `NodeId::new(id: u32)` | `NodeId` | Create from raw ID |
| `index(&self)` | `u32` | Get raw index |

---

### EdgeId

Unique identifier for edges.

```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct EdgeId(u32);
```

| Method | Returns | Description |
|--------|---------|-------------|
| `EdgeId::new(id: u32)` | `EdgeId` | Create from raw ID |
| `index(&self)` | `u32` | Get raw index |

---

### CpgNode

Represents a node in the CPG.

```rust
pub struct CpgNode {
    pub id: NodeId,
    pub kind: CpgNodeKind,
    pub source_range: SourceRange,
    pub name: Option<String>,
    pub type_info: Option<TypeInfo>,
}
```

| Method | Returns | Description |
|--------|---------|-------------|
| `new(id, kind, source_range)` | `CpgNode` | Create node |
| `id()` | `NodeId` | Node ID |
| `kind()` | `&CpgNodeKind` | Node kind |
| `name()` | `Option<&str>` | Optional name |
| `source_range()` | `&SourceRange` | Source location |
| `with_name(name: String)` | `Self` | Builder: set name |
| `with_type_info(info: TypeInfo)` | `Self` | Builder: set type |
| `is_declaration()` | `bool` | Is declaration kind |
| `is_expression()` | `bool` | Is expression kind |
| `is_statement()` | `bool` | Is statement kind |

---

### CpgNodeKind

Enumeration of all node types.

```rust
pub enum CpgNodeKind {
    // Declarations
    Module,
    Class,
    Struct,
    Enum,
    Trait,
    Function,
    Variable,
    Field,
    Parameter,
    TypeAlias,
    Constant,

    // Expressions
    BinaryOp(BinaryOperator),
    UnaryOp(UnaryOperator),
    Call,
    MemberAccess,
    IndexAccess,
    Identifier,
    Literal(LiteralKind),
    Lambda,

    // Statements
    Return,
    If,
    While,
    For,
    Loop,
    Match,
    Break,
    Continue,
    Block,
    ExpressionStatement,

    // Control Flow
    Entry,
    Exit,

    // Special
    Unknown,
}
```

**Helper enums:**

```rust
pub enum BinaryOperator {
    Add, Sub, Mul, Div, Mod,
    Eq, Ne, Lt, Le, Gt, Ge,
    And, Or, Xor,
    BitAnd, BitOr, BitXor,
    Shl, Shr,
    Assign, AddAssign, SubAssign, /* ... */
}

pub enum UnaryOperator {
    Neg, Not, Deref, Ref, RefMut,
}

pub enum LiteralKind {
    Integer, Float, String, Char, Bool, Null,
}
```

---

### CpgEdge

Represents an edge in the CPG.

```rust
pub struct CpgEdge {
    pub id: EdgeId,
    pub source: NodeId,
    pub target: NodeId,
    pub kind: CpgEdgeKind,
}
```

| Method | Returns | Description |
|--------|---------|-------------|
| `new(id, source, target, kind)` | `CpgEdge` | Create edge |
| `source()` | `NodeId` | Source node |
| `target()` | `NodeId` | Target node |
| `kind()` | `&CpgEdgeKind` | Edge kind |
| `is_ast()` | `bool` | Is AST edge |
| `is_cfg()` | `bool` | Is CFG edge |
| `is_dfg()` | `bool` | Is DFG edge |

---

### CpgEdgeKind

Enumeration of edge types.

```rust
pub enum CpgEdgeKind {
    // AST edges
    AstChild,
    AstNextSibling,

    // CFG edges
    CfgNext,
    CfgTrue,
    CfgFalse,
    CfgException,

    // DFG edges
    DfgDef,
    DfgUse,
    DfgReach,

    // Type edges
    TypeOf,
    InstanceOf,

    // Call edges
    CallTarget,
    CallArgument,
    CallReturn,
}
```

| Method | Returns | Description |
|--------|---------|-------------|
| `category()` | `EdgeCategory` | AST, CFG, or DFG |
| `is_ast()` | `bool` | Is AST category |
| `is_cfg()` | `bool` | Is CFG category |
| `is_dfg()` | `bool` | Is DFG category |

---

### SourceRange

Source code location.

```rust
pub struct SourceRange {
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
    pub start_byte: usize,
    pub end_byte: usize,
}
```

| Method | Returns | Description |
|--------|---------|-------------|
| `new(start_line, start_col, end_line, end_col)` | `SourceRange` | Create range |
| `default()` | `SourceRange` | Empty range |
| `contains(&self, other: &SourceRange)` | `bool` | Contains other range |
| `overlaps(&self, other: &SourceRange)` | `bool` | Overlaps with other |
| `byte_len()` | `usize` | Length in bytes |

---

### Language

Supported programming languages.

```rust
pub enum Language {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Go,
    Java,
    C,
    Cpp,
    CSharp,
    Ruby,
    Kotlin,
    Scala,
    Swift,
    Haskell,
    OCaml,
    Elixir,
    Bash,
    Rholang,
    MeTTa,
    // ...
}
```

| Method | Returns | Description |
|--------|---------|-------------|
| `from_extension(ext: &str)` | `Option<Language>` | Detect from file extension |
| `name(&self)` | `&'static str` | Human-readable name |
| `extension(&self)` | `&'static str` | Primary file extension |

---

## Builder Traits

### CpgBuilder

Trait for constructing CPGs.

```rust
pub trait CpgBuilder: Send + Sync {
    fn build(&self, source: &str, language: Language) -> Result<CodePropertyGraph, CpgError>;
    fn build_from_file(&self, path: &Path) -> Result<CodePropertyGraph, CpgError>;
}
```

### TreeSitterCpgBuilder

The primary builder implementation.

```rust
pub struct TreeSitterCpgBuilder {
    // configuration...
}
```

| Method | Returns | Description |
|--------|---------|-------------|
| `new()` | `Self` | Create with defaults |
| `with_cfg(enabled: bool)` | `Self` | Enable/disable CFG |
| `with_dfg(enabled: bool)` | `Self` | Enable/disable DFG |
| `build(source, language)` | `Result<CodePropertyGraph>` | Build from source |

---

## Graph Extraction

### CfgExtractor

Extracts Control Flow Graph.

```rust
pub trait CfgExtractor: Send + Sync {
    fn extract(&self, cpg: &mut CodePropertyGraph, function: NodeId) -> Result<(), CpgError>;
}
```

### DfgExtractor

Extracts Data Flow Graph.

```rust
pub trait DfgExtractor: Send + Sync {
    fn extract(&self, cpg: &mut CodePropertyGraph, function: NodeId) -> Result<(), CpgError>;
}
```

---

## Error Types

### CpgError

Error type for CPG operations.

```rust
pub enum CpgError {
    ParseError { message: String, location: Option<SourceRange> },
    LanguageNotSupported(Language),
    NodeNotFound(NodeId),
    EdgeNotFound(EdgeId),
    InvalidGraph(String),
    IoError(std::io::Error),
}
```

---

## Feature Flags

| Feature | Description |
|---------|-------------|
| `default` | Core graph types only |
| `tree-sitter` | Enable tree-sitter builders |
| `serde` | Serialization support |
| `rayon` | Parallel iterators |

---

## See Also

- [Graph Overview](../components/graph/overview.md)
- [Nodes Reference](../components/graph/nodes.md)
- [Edges Reference](../components/graph/edges.md)
- [Pattern API Reference](pattern-reference.md)
