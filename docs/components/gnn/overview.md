# GNN Overview

Graph Neural Networks (GNNs) provide a powerful way to learn representations from Code Property Graphs. libcpg includes a native GNN implementation that generates embeddings capturing both structural and semantic properties of code.

## What are Graph Neural Networks?

GNNs learn node representations by iteratively aggregating information from neighboring nodes. For code analysis, this means:

- A function node "learns about" its statements
- A variable use "learns about" its definitions
- A conditional "learns about" both branches

After several iterations, each node's embedding encodes information about its entire neighborhood in the graph.

```
                         After 3 iterations:
   Initial:              ┌─────────────────────────────┐
                         │ Each node knows about nodes │
   Node A ───▶ Node B    │ up to 3 hops away           │
      │                  └─────────────────────────────┘
      ▼
   Node C                A's embedding contains info about:
                         - B (1 hop)
                         - C (1 hop)
                         - B's neighbors (2 hops)
                         - ...
```

## Why GNNs for Code?

Traditional code features (line count, cyclomatic complexity) capture surface properties. GNN embeddings capture:

| Feature Type | Example | What GNN Captures |
|--------------|---------|-------------------|
| Structure | Loop nesting | Hierarchical AST context |
| Control flow | Branch patterns | CFG neighborhood |
| Data flow | Variable usage | Def-use chain context |
| Semantics | Similar algorithms | Combined embedding similarity |

## CPGNN Architecture

libcpg implements CPGNN (Code Property Graph Neural Network), inspired by Devign and related work:

```
┌─────────────────────────────────────────────────────────────────┐
│                      CPGNN Architecture                          │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│   ┌─────────────┐                                               │
│   │ CPG Input   │                                               │
│   │ AST+CFG+DFG │                                               │
│   └──────┬──────┘                                               │
│          │                                                       │
│          ▼                                                       │
│   ┌─────────────────────────────────────────┐                   │
│   │        Node Feature Initialization       │                   │
│   │  • Node type one-hot encoding           │                   │
│   │  • Random initialization for diversity   │                   │
│   └──────────────────┬──────────────────────┘                   │
│                      │                                           │
│          ┌───────────┴───────────┐                              │
│          │ Message Passing Layers │                              │
│          │                        │                              │
│          │  ┌──────────────────┐ │                              │
│          │  │ Layer 1          │ │                              │
│          │  │  Aggregate from: │ │                              │
│          │  │  - AST neighbors │ │                              │
│          │  │  - CFG neighbors │ │                              │
│          │  │  - DFG neighbors │ │                              │
│          │  └────────┬─────────┘ │                              │
│          │           │           │                              │
│          │           ▼           │                              │
│          │  ┌──────────────────┐ │                              │
│          │  │ Layer 2 ... N    │ │                              │
│          │  └────────┬─────────┘ │                              │
│          └───────────┼───────────┘                              │
│                      │                                           │
│                      ▼                                           │
│   ┌─────────────────────────────────────────┐                   │
│   │         Final Node Embeddings            │                   │
│   │   [n1_emb, n2_emb, ..., nm_emb]         │                   │
│   └──────────────────┬──────────────────────┘                   │
│                      │                                           │
│         ┌────────────┴────────────┐                             │
│         ▼                         ▼                             │
│  ┌──────────────┐         ┌──────────────────┐                  │
│  │ Node Queries │         │ Subgraph Queries │                  │
│  └──────────────┘         └──────────────────┘                  │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

## Quick Start

```rust
use libcpg::{CpgGnn, GraphNeuralNetwork, TreeSitterCpgBuilder, Language};

// Build a CPG from source code
let builder = TreeSitterCpgBuilder::new();
let source = r#"
    fn factorial(n: i32) -> i32 {
        if n <= 1 { 1 } else { n * factorial(n - 1) }
    }
"#;
let cpg = builder.build(source, Language::Rust)?;

// Create GNN with custom configuration
let mut gnn = CpgGnn::new(cpg)
    .with_embedding_dim(128)    // 128-dimensional embeddings
    .with_num_layers(3)          // 3 message passing layers
    .with_dropout(0.1);          // 10% dropout

// Run message passing
gnn.propagate(3);  // 3 iterations

// Query node embeddings
if let Some(embedding) = gnn.node_embedding(function_node_id) {
    println!("Embedding dim: {}", embedding.len());
}

// Get subgraph embeddings
let func_nodes = vec![func_id, body_id, return_id];
let func_embedding = gnn.subgraph_embedding(&func_nodes);
```

## Configuration

| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `embedding_dim` | `usize` | 128 | Size of embedding vectors |
| `num_layers` | `usize` | 3 | Number of GNN layers |
| `dropout` | `f32` | 0.1 | Dropout rate (training) |

**Choosing embedding dimension:**

- **64**: Fast, sufficient for simple patterns
- **128** (default): Good balance for most tasks
- **256+**: Better for complex semantic similarity

**Choosing iterations:**

- **2-3**: Local structure (immediate neighbors)
- **4-5**: Medium-range context
- **6+**: Global patterns (but diminishing returns)

## Use Cases

### 1. Code Clone Detection

Find similar code fragments by comparing embeddings:

```rust
// Get function embeddings
let emb1 = gnn.subgraph_embedding(&func1_nodes);
let emb2 = gnn.subgraph_embedding(&func2_nodes);

// Compute similarity
let similarity = cosine_similarity(&emb1, &emb2);
if similarity > 0.85 {
    println!("Potential code clone detected!");
}
```

### 2. Vulnerability Detection

Embeddings can be used as features for ML models:

```rust
// Extract embeddings for all functions
let mut features = Vec::new();
for func in cpg.nodes_of_kind(CpgNodeKind::Function) {
    let descendants: Vec<_> = cpg.ast_descendants(func.id()).collect();
    let embedding = gnn.subgraph_embedding(&descendants);
    features.push((func.id(), embedding));
}

// Use features for classification
let vulnerable = classifier.predict(&features);
```

### 3. Code Search

Index embeddings for semantic search:

```rust
// Build index
let mut index = EmbeddingIndex::new();
for func in cpg.nodes_of_kind(CpgNodeKind::Function) {
    let embedding = gnn.subgraph_embedding(&get_func_nodes(&cpg, func.id()));
    index.add(func.id(), embedding);
}

// Search by example
let query_embedding = gnn.subgraph_embedding(&query_nodes);
let results = index.search(&query_embedding, k=10);
```

## Feature Flags

GNN functionality requires the `gnn` feature:

```toml
[dependencies]
libcpg = { version = "0.1", features = ["gnn"] }
```

This enables:
- `ndarray` for vector operations
- `rand` for initialization
- Full embedding computation

Without the feature, the trait exists but embedding methods are stubbed.

## Performance

| Operation | Time | Memory |
|-----------|------|--------|
| Initialize embeddings | O(n) | O(n × d) |
| One propagation iteration | O(n × avg_degree) | O(n × d) |
| Node embedding query | O(1) | O(d) |
| Subgraph embedding | O(k) | O(d) |

Where:
- n = number of nodes
- d = embedding dimension
- k = subgraph size

**Memory estimation:**
- 10K nodes × 128 dim × 4 bytes = ~5 MB
- 100K nodes × 128 dim × 4 bytes = ~50 MB

## Next Steps

- [Message Passing](message-passing.md) - How message passing works
- [Embeddings](embeddings.md) - Working with embeddings
- [Graph Overview](../graph/overview.md) - CPG structure
