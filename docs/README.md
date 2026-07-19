# libcpg Documentation

**libcpg** is a Rust library for constructing and analyzing Code Property Graphs (CPGs). It combines Abstract Syntax Trees (AST), Control Flow Graphs (CFG), and Data Flow Graphs (DFG) into a unified representation for comprehensive program analysis.

## What is a Code Property Graph?

A Code Property Graph is a data structure that represents source code as a graph, merging three complementary views:

- **AST (Abstract Syntax Tree)**: Syntactic structure showing how code is parsed
- **CFG (Control Flow Graph)**: Execution paths through the code
- **DFG (Data Flow Graph)**: How data moves between definitions and uses

```
                         Code Property Graph
┌────────────────────────────────────────────────────────────────────────┐
│                                                                        │
│   Source Code           AST Nodes              CFG/DFG Edges          │
│                                                                        │
│   fn foo(x: i32) {      ┌──────────┐                                  │
│       let y = x + 1;    │ Function │──────── AST_CHILD ────┐         │
│       if y > 0 {        └──────────┘                       ▼         │
│           return y;          │                        ┌──────────┐   │
│       }                 AST_CHILD                     │  Param   │   │
│       return 0;              │                        └──────────┘   │
│   }                          ▼                                        │
│                         ┌──────────┐         ┌──────────┐            │
│                         │   Let    │── DFG ──│    If    │            │
│                         │  y = ... │         │  y > 0   │            │
│                         └──────────┘         └──────────┘            │
│                              │                    │                   │
│                           CFG_EDGE            CFG_BRANCH              │
│                              ▼                    ▼                   │
│                         ┌──────────┐         ┌──────────┐            │
│                         │  Binary  │         │  Return  │            │
│                         │  x + 1   │         │    y     │            │
│                         └──────────┘         └──────────┘            │
│                                                                        │
└────────────────────────────────────────────────────────────────────────┘
```

By combining these views, libcpg enables powerful analyses:

- **Vulnerability Detection**: Find tainted data flowing into sensitive sinks
- **Design Pattern Recognition**: Identify GoF patterns via subgraph matching
- **Algorithm Detection**: Recognize algorithm families from control flow structure
- **Code Clone Detection**: Find semantically similar code fragments
- **Complexity Analysis**: Estimate algorithmic complexity from graph properties

## Quick Start

```rust
use libcpg::{TreeSitterCpgBuilder, CpgBuilder, Language};

// Parse source code into a CPG
let builder = TreeSitterCpgBuilder::new();
let source = r#"
    fn factorial(n: u64) -> u64 {
        if n <= 1 { 1 } else { n * factorial(n - 1) }
    }
"#;

let cpg = builder.build(source, Language::Rust)?;

// Explore the graph
println!("Nodes: {}", cpg.node_count());
println!("Edges: {}", cpg.edge_count());

// Iterate over CFG nodes
for node in cpg.cfg_nodes() {
    println!("CFG: {:?}", node.kind());
}

// Find data flow edges
for edge in cpg.dfg_edges() {
    println!("DFG: {} -> {}", edge.source(), edge.target());
}
```

### Detecting Design Patterns

```rust
use libcpg::patterns::design::{GofPatternDetector, GofPattern};

let detector = GofPatternDetector::new();
let matches = detector.detect(&cpg);

for m in matches {
    println!("Found {} pattern with {} confidence",
             m.pattern.name(), m.confidence);
}
```

### Algorithm Detection

```rust
use libcpg::algorithms::{AlgorithmDetector, AlgorithmFamily};

let detector = AlgorithmDetector::new();
let detected = detector.detect(&cpg);

for algo in detected {
    println!("{:?} algorithm, estimated complexity: {:?}",
             algo.family, algo.complexity);
}
```

## Documentation Structure

### Architecture

- [Overview](architecture/overview.md) - High-level design and principles
- [Data Flow](architecture/data-flow.md) - Graph construction pipeline

### Components

#### Graph Core

- [Overview](components/graph/overview.md) - CPG structure and concepts
- [Nodes](components/graph/nodes.md) - CpgNode types and properties
- [Edges](components/graph/edges.md) - Edge types (AST, CFG, DFG)
- [Traversal](components/graph/traversal.md) - Graph navigation APIs

#### Graph Neural Networks

- [Overview](components/gnn/overview.md) - GNN concepts and architecture
- [Message Passing](components/gnn/message-passing.md) - Propagation algorithm
- [Embeddings](components/gnn/embeddings.md) - Node and subgraph embeddings

#### Pattern Detection

- [Overview](components/patterns/overview.md) - Pattern detection framework
- [Gang of Four](components/patterns/gang-of-four.md) - GoF pattern catalog
- [VF2 Matching](components/patterns/vf2-matching.md) - Subgraph isomorphism algorithm

#### Algorithm Detection

- [Overview](components/algorithms/overview.md) - Algorithm detection framework
- [Families](components/algorithms/families.md) - Supported algorithm families
- [Complexity](components/algorithms/complexity.md) - Complexity estimation

### API Reference

- [Graph Reference](api/graph-reference.md) - Core graph types
- [Pattern Reference](api/pattern-reference.md) - Pattern detection APIs

## Features

```toml
[dependencies]
libcpg = { version = "0.1", features = ["design-patterns", "algorithm-detection"] }
```

| Feature | Description |
|---------|-------------|
| `default` | Core CPG construction and traversal |
| `gnn` | Graph neural network support |
| `design-patterns` | GoF pattern detection |
| `algorithm-detection` | Algorithm family recognition |
| `serde` | Serialization/deserialization |
| `rholang` | Rholang-specific patterns |
| `metta` | MeTTa-specific patterns |

## Supported Languages

libcpg uses tree-sitter for parsing, supporting 20+ languages:

| Language | Status | Notes |
|----------|--------|-------|
| Rust | Full | Primary development language |
| Python | Full | |
| JavaScript/TypeScript | Full | |
| Java | Full | |
| C/C++ | Full | |
| Go | Full | |
| Rholang | Full | Process calculus for F1R3FLY.io |
| MeTTa | Full | Hypergraph AI language for F1R3FLY.io |

## Related Projects

- [libgrammstein](../libgrammstein): Hybrid language models with code embeddings
- [lling-llang](../lling-llang): WFST-based code correction
- [liblevenshtein](../liblevenshtein-rust): Fuzzy string matching

## License

MIT OR Apache-2.0
