# Graph Core Overview

The graph module provides the fundamental data structures for representing code as a Code Property Graph (CPG).

## Key Concepts

### What is a CPG?

A Code Property Graph is a directed graph where:

- **Nodes** represent syntactic elements (functions, statements, expressions, etc.)
- **Edges** connect nodes with typed relationships

Three types of edges coexist in the same graph:

| Edge Type | Represents | Example |
|-----------|------------|---------|
| AST | Syntactic containment | Function → Body → Statement |
| CFG | Control flow | If → ThenBranch, If → ElseBranch |
| DFG | Data dependencies | Definition → Use |

### Why Combine These Views?

Combining AST, CFG, and DFG enables analyses that require information from multiple abstractions:

```
Traditional:                      CPG:

   Is 'x' tainted?               Is 'x' tainted?
         │                              │
   ┌─────┴─────┐                        ▼
   ▼           ▼                  Single query traversing
 Check       Check                AST + DFG edges
  AST         DFG
   │           │
   └─────┬─────┘
         ▼
   Correlate results
```

## Core Types

### CodePropertyGraph

The main container holding all nodes and edges:

```rust
pub struct CodePropertyGraph {
    nodes: Vec<CpgNode>,
    edges: Vec<CpgEdge>,
    adjacency: HashMap<NodeId, Vec<EdgeId>>,
    reverse_adjacency: HashMap<NodeId, Vec<EdgeId>>,
    language: Language,
    source_file: Option<PathBuf>,
}
```

**Creating a CPG**:

```rust
use libcpg::{TreeSitterCpgBuilder, CpgBuilder, Language};

let builder = TreeSitterCpgBuilder::new();
let source = "fn main() { println!(\"Hello\"); }";
let cpg = builder.build(source, Language::Rust)?;
```

**Accessing Statistics**:

```rust
let stats = cpg.stats();
println!("Nodes: {}", stats.node_count);
println!("AST edges: {}", stats.ast_edge_count);
println!("CFG edges: {}", stats.cfg_edge_count);
println!("DFG edges: {}", stats.dfg_edge_count);
```

### CpgNode

Represents a syntactic element in the code:

```rust
pub struct CpgNode {
    id: NodeId,
    kind: CpgNodeKind,
    source_range: SourceRange,
    text: Option<Arc<str>>,
    properties: HashMap<PropertyKey, PropertyValue>,
}
```

**Node Kinds**:

```rust
pub enum CpgNodeKind {
    // Functions and methods
    Function,
    Method,
    Constructor,

    // Control flow
    Block,
    If,
    Loop,
    Match,
    Return,
    Break,
    Continue,

    // Expressions
    BinaryOp,
    UnaryOp,
    Call,
    Index,
    FieldAccess,

    // Declarations
    Variable,
    Parameter,
    Constant,

    // Types
    Class,
    Struct,
    Enum,
    Interface,
    Trait,

    // Literals
    Literal(LiteralKind),

    // Other
    Comment,
    Unknown,
}
```

**Accessing Nodes**:

```rust
// Get a node by ID
let node = cpg.node(node_id)?;

// Iterate all nodes
for node in cpg.nodes() {
    println!("{:?} at {:?}", node.kind(), node.source_range());
}

// Filter by kind
for func in cpg.nodes_of_kind(CpgNodeKind::Function) {
    println!("Function: {:?}", func.text());
}
```

### CpgEdge

Connects two nodes with a typed relationship:

```rust
pub struct CpgEdge {
    id: EdgeId,
    source: NodeId,
    target: NodeId,
    kind: CpgEdgeKind,
}

pub enum CpgEdgeKind {
    // AST structure
    AstChild,
    AstNextSibling,

    // Control flow
    CfgEdge(CfgEdgeKind),

    // Data flow
    DfgEdge(DfgEdgeKind),
}
```

**CFG Edge Kinds**:

```rust
pub enum CfgEdgeKind {
    Sequential,     // Normal flow
    BranchTrue,     // Conditional true
    BranchFalse,    // Conditional false
    LoopBack,       // Back to loop header
    LoopExit,       // Exit from loop
    ExceptionThrow, // Exception thrown
    ExceptionCatch, // Exception caught
}
```

**DFG Edge Kinds**:

```rust
pub enum DfgEdgeKind {
    DefUse,   // Definition to use
    UseUse,   // Use to use (same def)
    Phi,      // SSA phi node
    Call,     // Argument passing
    Return,   // Return value
    Field,    // Field dependency
}
```

**Accessing Edges**:

```rust
// Get outgoing edges from a node
for edge_id in cpg.outgoing_edges(node_id) {
    let edge = cpg.edge(edge_id)?;
    println!("{:?} -> {:?}", edge.source(), edge.target());
}

// Get incoming edges
for edge_id in cpg.incoming_edges(node_id) {
    let edge = cpg.edge(edge_id)?;
    println!("{:?} <- {:?}", edge.target(), edge.source());
}

// Filter by edge type
for edge in cpg.cfg_edges() {
    println!("CFG: {:?}", edge.kind());
}
```

### SourceRange

Tracks the original source location:

```rust
pub struct SourceRange {
    start_byte: usize,
    end_byte: usize,
    start_line: u32,
    start_column: u32,
    end_line: u32,
    end_column: u32,
}
```

**Example**:

```rust
let range = node.source_range();
println!("Line {}-{}", range.start_line, range.end_line);
println!("Bytes {}-{}", range.start_byte, range.end_byte);
```

## Graph Properties

### Node Properties

Nodes can have additional properties stored as key-value pairs:

```rust
pub enum PropertyKey {
    Name,           // Identifier name
    Type,           // Type annotation
    Visibility,     // pub, private, etc.
    Modifiers,      // static, const, etc.
    Documentation,  // Doc comment
    Custom(String), // User-defined
}

pub enum PropertyValue {
    String(Arc<str>),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    List(Vec<PropertyValue>),
}
```

**Accessing Properties**:

```rust
if let Some(PropertyValue::String(name)) = node.property(PropertyKey::Name) {
    println!("Name: {}", name);
}

// Type information
if let Some(type_info) = node.type_info() {
    println!("Type: {:?}", type_info);
}
```

### Language Information

```rust
pub enum Language {
    Rust,
    Python,
    JavaScript,
    TypeScript,
    Java,
    CSharp,
    Cpp,
    C,
    Go,
    Ruby,
    Kotlin,
    Rholang,
    Metta,
    Unknown,
}

// Get the language of a CPG
let lang = cpg.language();
```

## Memory Efficiency

The CPG uses several techniques to minimize memory usage:

1. **String Interning**: Common strings are deduplicated via `Arc<str>`
2. **Compact IDs**: NodeId and EdgeId are 32-bit indices
3. **Lazy Text**: Source text is only stored when requested
4. **Property Compression**: Common properties use enum variants

## Thread Safety

- `CodePropertyGraph` is immutable after construction
- All ID types (`NodeId`, `EdgeId`) are `Copy` and safe to share
- Node and edge iterators are `Send + Sync`

## Next Steps

- [Nodes](nodes.md) - Detailed node type documentation
- [Edges](edges.md) - Edge types and semantics
- [Traversal](traversal.md) - Graph navigation patterns
