# Architecture Overview

libcpg's architecture centers on the **Code Property Graph** (CPG), a unified graph representation that combines three complementary views of source code.

## Design Principles

### 1. Unified Representation

Rather than maintaining separate AST, CFG, and DFG structures, libcpg merges them into a single graph. This enables analyses that span multiple abstractions:

```
                    Traditional Approach
     ┌─────────────────────────────────────────────┐
     │                                             │
     │    AST          CFG           DFG          │
     │   ┌───┐        ┌───┐        ┌───┐         │
     │   │   │        │   │        │   │         │
     │   └───┘        └───┘        └───┘         │
     │      │            │            │           │
     │      ▼            ▼            ▼           │
     │   Separate     Separate     Separate      │
     │   Analysis     Analysis     Analysis      │
     │                                             │
     └─────────────────────────────────────────────┘

                      libcpg Approach
     ┌─────────────────────────────────────────────┐
     │                                             │
     │              Code Property Graph            │
     │         ┌─────────────────────────┐        │
     │         │  ○───AST───○            │        │
     │         │  │         │            │        │
     │         │  CFG      DFG           │        │
     │         │  │         │            │        │
     │         │  ○─────────○            │        │
     │         └─────────────────────────┘        │
     │                    │                        │
     │                    ▼                        │
     │            Unified Analysis                 │
     │                                             │
     └─────────────────────────────────────────────┘
```

### 2. Language Agnostic

libcpg abstracts over language-specific syntax through:

- **Tree-sitter** for parsing (40+ languages)
- **Normalized node kinds** that map language-specific constructs to common types
- **Language hints** for analyses that benefit from language-specific knowledge

### 3. Incremental Construction

The CPG is built in stages, allowing partial graphs when full analysis isn't needed:

```
   Source Code
        │
        ▼
  ┌─────────────┐
  │  Parse AST  │  ← Always performed
  └─────────────┘
        │
        ▼
  ┌─────────────┐
  │ Extract CFG │  ← Optional (config.enable_cfg)
  └─────────────┘
        │
        ▼
  ┌─────────────┐
  │ Extract DFG │  ← Optional (config.enable_dfg)
  └─────────────┘
        │
        ▼
   Complete CPG
```

### 4. Parallelism

Analysis operations are parallelized where possible:

- File-level parallelism for multi-file projects
- Node-level parallelism for graph algorithms
- SIMD operations for embedding computations

## Core Components

### Graph Module (`graph/`)

The foundation of libcpg, providing:

- **`CodePropertyGraph`**: The main graph structure
- **`CpgNode`**: Nodes with kind, source location, and properties
- **`CpgEdge`**: Typed edges connecting nodes

```rust
// Node kinds abstract over language-specific syntax
pub enum CpgNodeKind {
    // Control flow
    Function, Block, If, Loop, Return,
    // Data
    Variable, Parameter, Literal, BinaryOp,
    // Declarations
    Class, Method, Field,
    // ...
}

// Edge kinds distinguish relationship types
pub enum CpgEdgeKind {
    AstChild,           // AST structure
    CfgEdge(CfgEdgeKind), // Control flow
    DfgEdge(DfgEdgeKind), // Data flow
}
```

### Builder Module (`builder/`)

Constructs CPGs from source code:

- **`CpgBuilder`**: Trait for building CPGs
- **`TreeSitterCpgBuilder`**: Tree-sitter based implementation
- **`CfgExtractor`**: Extracts control flow from AST
- **`DfgExtractor`**: Extracts data flow from AST + CFG

### Pattern Module (`pattern/`)

Detects patterns via subgraph matching:

- **`SubgraphMatcher`**: General subgraph isomorphism
- **`Vf2Matcher`**: VF2 algorithm implementation
- **`PatternMatch`**: Match result with confidence

### GNN Module (`gnn/`) [feature = "gnn"]

Graph neural network operations:

- **`GraphNeuralNetwork`**: Trait for GNN implementations
- **`CpgGnn`**: Message-passing GNN on CPGs
- Node and subgraph embeddings

### Algorithms Module (`algorithms/`) [feature = "algorithm-detection"]

Algorithm family detection:

- **`AlgorithmDetector`**: Identifies algorithm patterns
- **`ComplexityEstimator`**: Estimates Big-O complexity

### Patterns Module (`patterns/`) [feature = "design-patterns"]

Design pattern detection:

- **`GofPatternDetector`**: Gang of Four patterns
- Pattern templates for each GoF pattern

## Data Flow

### CPG Construction Pipeline

```
   ┌───────────────────────────────────────────────────────────────┐
   │                    CPG Construction Pipeline                   │
   ├───────────────────────────────────────────────────────────────┤
   │                                                               │
   │   Source          Tree-sitter        AST                     │
   │   Code      ───▶   Parser      ───▶  Nodes                   │
   │                                         │                     │
   │                                         ▼                     │
   │                              ┌─────────────────────┐         │
   │                              │   CfgExtractor      │         │
   │                              │                     │         │
   │                              │ • Basic blocks      │         │
   │                              │ • Branch edges      │         │
   │                              │ • Loop back edges   │         │
   │                              └─────────────────────┘         │
   │                                         │                     │
   │                                         ▼                     │
   │                              ┌─────────────────────┐         │
   │                              │   DfgExtractor      │         │
   │                              │                     │         │
   │                              │ • Def-use chains    │         │
   │                              │ • Data dependencies │         │
   │                              │ • Reaching defs     │         │
   │                              └─────────────────────┘         │
   │                                         │                     │
   │                                         ▼                     │
   │                              ┌─────────────────────┐         │
   │                              │  CodePropertyGraph  │         │
   │                              │                     │         │
   │                              │  Unified structure  │         │
   │                              │  ready for analysis │         │
   │                              └─────────────────────┘         │
   │                                                               │
   └───────────────────────────────────────────────────────────────┘
```

### Analysis Pipeline

Once constructed, the CPG supports multiple analysis passes:

```
                              CodePropertyGraph
                                      │
            ┌─────────────────────────┼─────────────────────────┐
            │                         │                         │
            ▼                         ▼                         ▼
   ┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
   │ Pattern Detect  │     │  GNN Embedding  │     │ Algorithm Detect│
   │                 │     │                 │     │                 │
   │ • GoF patterns  │     │ • Node embed    │     │ • Family ID     │
   │ • VF2 matching  │     │ • Subgraph      │     │ • Complexity    │
   └─────────────────┘     └─────────────────┘     └─────────────────┘
            │                         │                         │
            └─────────────────────────┴─────────────────────────┘
                                      │
                                      ▼
                              Analysis Results
```

## Memory Layout

The CPG uses a compact representation to minimize memory overhead:

| Component | Per Node | Notes |
|-----------|----------|-------|
| NodeId | 4 bytes | u32 index |
| Kind | 1 byte | Discriminant |
| Source range | 16 bytes | Start/end positions |
| Properties | Variable | Interned strings |
| Adjacency | 8+ bytes | Edge lists |

For a typical 10,000-node CPG (medium-sized file):
- Base structure: ~300 KB
- With CFG: ~400 KB
- With DFG: ~500 KB

## Thread Safety

All public types are designed for safe concurrent access:

- `CodePropertyGraph` is immutable after construction
- Pattern matchers are stateless and `Send + Sync`
- GNN operations use atomic counters for progress

## Error Handling

libcpg uses the `Error` type for all fallible operations:

```rust
pub enum Error {
    Construction(String),    // CPG building failed
    PatternMatch(String),    // Pattern matching error
    Gnn(String),            // GNN operation failed
    InvalidNodeId(NodeId),   // Bad node reference
    InvalidEdgeId(EdgeId),   // Bad edge reference
    UnsupportedLanguage(String), // Language not supported
    Io(std::io::Error),      // I/O error
}
```

## Next Steps

- [Graph Components](../components/graph/overview.md) - Deep dive into CPG structure
- [Pattern Detection](../components/patterns/overview.md) - Design pattern recognition
- [GNN Operations](../components/gnn/overview.md) - Graph neural networks
