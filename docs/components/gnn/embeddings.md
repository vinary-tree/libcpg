# Embeddings

After message passing, each node in the CPG has a learned embedding vector. This document covers how to work with these embeddings for code analysis tasks.

## Embedding Types

libcpg provides two main embedding types:

### NodeEmbedding

Represents the embedding for a single node:

```rust
pub struct NodeEmbedding {
    /// The node this embedding represents
    pub node_id: NodeId,
    /// The embedding vector
    pub vector: Array1<f32>,
    /// Embedding dimensionality
    pub dim: usize,
}
```

**Creating a NodeEmbedding:**

```rust
use libcpg::{NodeEmbedding, NodeId};
use ndarray::array;

// From GNN output
let embedding = gnn.node_embedding(node_id)?;
let node_emb = NodeEmbedding::new(node_id, embedding);

// Check properties
println!("Dimension: {}", node_emb.dim);
println!("L2 norm: {}", node_emb.norm());
```

### SubgraphEmbedding

Represents an aggregated embedding for multiple nodes (e.g., a function, class, or code block):

```rust
pub struct SubgraphEmbedding {
    /// The nodes in this subgraph
    pub node_ids: Vec<NodeId>,
    /// The aggregated embedding vector
    pub vector: Array1<f32>,
    /// Embedding dimensionality
    pub dim: usize,
    /// Aggregation method used
    pub aggregation: AggregationMethod,
}
```

**Creating a SubgraphEmbedding:**

```rust
use libcpg::{SubgraphEmbedding, AggregationMethod};

// Get function nodes
let func_nodes: Vec<_> = cpg.ast_descendants(func_id).collect();

// Compute subgraph embedding
let embedding = gnn.subgraph_embedding(&func_nodes);
let subgraph_emb = SubgraphEmbedding::new(
    func_nodes,
    embedding,
    AggregationMethod::Mean
);

println!("Nodes in subgraph: {}", subgraph_emb.node_count());
```

## Aggregation Methods

When creating subgraph embeddings, several aggregation methods are available:

```rust
pub enum AggregationMethod {
    Mean,         // Average of node embeddings
    Sum,          // Sum of node embeddings
    Max,          // Element-wise maximum
    Attention,    // Attention-weighted average
    Hierarchical, // AST-structure-aware aggregation
}
```

### Mean (Default)

The most common choice. Produces stable embeddings regardless of subgraph size:

```rust
// Mean: sum(embeddings) / count
let embedding = gnn.subgraph_embedding(&nodes);  // Uses mean
```

**Properties:**
- Normalized by size
- Good for comparing different-sized code fragments
- Loses some magnitude information

### Sum

Preserves the "amount" of information:

```rust
fn sum_aggregate(embeddings: &[Array1<f32>]) -> Array1<f32> {
    embeddings.iter().fold(
        Array1::zeros(dim),
        |acc, e| acc + e
    )
}
```

**Properties:**
- Larger subgraphs = larger embeddings
- Good when size is meaningful
- Can cause scale issues

### Max Pooling

Captures the most prominent features:

```rust
fn max_aggregate(embeddings: &[Array1<f32>]) -> Array1<f32> {
    let mut result = Array1::from_elem(dim, f32::NEG_INFINITY);
    for emb in embeddings {
        for i in 0..dim {
            result[i] = result[i].max(emb[i]);
        }
    }
    result
}
```

**Properties:**
- Highlights dominant patterns
- Good for detecting specific features
- Ignores frequency information

### Hierarchical

Follows AST structure for aggregation:

```
           func_emb
          /        \
     block_emb    params_emb
        |
   stmt_emb
```

Aggregates bottom-up, preserving structural relationships.

## Similarity Computation

### Cosine Similarity

The primary metric for comparing embeddings:

```rust
impl NodeEmbedding {
    pub fn cosine_similarity(&self, other: &NodeEmbedding) -> f32 {
        if self.dim != other.dim {
            return 0.0;
        }

        let dot: f32 = self.vector.iter()
            .zip(other.vector.iter())
            .map(|(a, b)| a * b)
            .sum();

        let norm_self = self.norm();
        let norm_other = other.norm();

        if norm_self == 0.0 || norm_other == 0.0 {
            0.0
        } else {
            dot / (norm_self * norm_other)
        }
    }
}
```

**Example usage:**

```rust
let emb1 = gnn.node_embedding(node1)?;
let emb2 = gnn.node_embedding(node2)?;

let node_emb1 = NodeEmbedding::new(node1, emb1);
let node_emb2 = NodeEmbedding::new(node2, emb2);

let similarity = node_emb1.cosine_similarity(&node_emb2);
println!("Similarity: {:.3}", similarity);  // 0.0 to 1.0
```

### Interpreting Similarity Scores

| Score | Interpretation |
|-------|----------------|
| 0.95+ | Near-identical (clones, trivial differences) |
| 0.80-0.95 | Very similar (same algorithm, minor changes) |
| 0.60-0.80 | Related (similar structure, different details) |
| 0.40-0.60 | Weak similarity (some common patterns) |
| < 0.40 | Unrelated |

### L2 Norm

The magnitude of an embedding:

```rust
pub fn norm(&self) -> f32 {
    self.vector.iter()
        .map(|x| x * x)
        .sum::<f32>()
        .sqrt()
}
```

Useful for detecting unusual nodes or normalizing embeddings.

## Common Patterns

### Finding Similar Functions

```rust
/// Find functions similar to a query function
fn find_similar_functions(
    gnn: &CpgGnn,
    query_func_id: NodeId,
    threshold: f32,
) -> Vec<(NodeId, f32)> {
    let cpg = gnn.cpg();

    // Get query function embedding
    let query_nodes: Vec<_> = std::iter::once(query_func_id)
        .chain(cpg.ast_descendants(query_func_id))
        .collect();
    let query_emb = gnn.subgraph_embedding(&query_nodes);
    let query = SubgraphEmbedding::new(
        query_nodes,
        query_emb,
        AggregationMethod::Mean
    );

    // Compare to all other functions
    let mut results = Vec::new();

    for func in cpg.nodes_of_kind(CpgNodeKind::Function) {
        if func.id() == query_func_id {
            continue;
        }

        let func_nodes: Vec<_> = std::iter::once(func.id())
            .chain(cpg.ast_descendants(func.id()))
            .collect();
        let func_emb = gnn.subgraph_embedding(&func_nodes);
        let candidate = SubgraphEmbedding::new(
            func_nodes,
            func_emb,
            AggregationMethod::Mean
        );

        let similarity = query.cosine_similarity(&candidate);
        if similarity >= threshold {
            results.push((func.id(), similarity));
        }
    }

    // Sort by similarity (descending)
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    results
}
```

### Clustering Code

```rust
/// Cluster nodes by embedding similarity
fn cluster_by_embedding(
    gnn: &CpgGnn,
    node_ids: &[NodeId],
    num_clusters: usize,
) -> Vec<Vec<NodeId>> {
    // Extract embeddings
    let embeddings: Vec<_> = node_ids.iter()
        .filter_map(|&id| gnn.node_embedding(id).map(|e| (id, e)))
        .collect();

    // Simple k-means clustering (pseudocode)
    // In practice, use a library like linfa
    let clusters = kmeans(&embeddings, num_clusters);

    clusters
}
```

### Building a Search Index

```rust
use std::collections::HashMap;

/// Simple embedding index for similarity search
struct EmbeddingIndex {
    embeddings: HashMap<NodeId, Array1<f32>>,
    dim: usize,
}

impl EmbeddingIndex {
    fn new(dim: usize) -> Self {
        Self {
            embeddings: HashMap::new(),
            dim,
        }
    }

    fn add(&mut self, id: NodeId, embedding: Array1<f32>) {
        self.embeddings.insert(id, embedding);
    }

    fn search(&self, query: &Array1<f32>, k: usize) -> Vec<(NodeId, f32)> {
        let mut scores: Vec<_> = self.embeddings.iter()
            .map(|(&id, emb)| {
                let sim = cosine_similarity(query, emb);
                (id, sim)
            })
            .collect();

        scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
        scores.truncate(k);
        scores
    }
}

fn cosine_similarity(a: &Array1<f32>, b: &Array1<f32>) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if norm_a == 0.0 || norm_b == 0.0 {
        0.0
    } else {
        dot / (norm_a * norm_b)
    }
}
```

## Serialization

Embeddings can be serialized with the `serde` feature:

```rust
#[cfg(feature = "serde")]
{
    // Save embeddings
    let serialized = serde_json::to_string(&node_embedding)?;

    // Note: vector is skipped during serialization
    // Only metadata (node_id, dim) is saved
    // Embeddings must be recomputed after loading
}
```

To persist full embeddings, save the vector separately:

```rust
use std::fs::File;
use std::io::Write;

fn save_embeddings(
    gnn: &CpgGnn,
    node_ids: &[NodeId],
    path: &str,
) -> std::io::Result<()> {
    let mut file = File::create(path)?;

    for &id in node_ids {
        if let Some(emb) = gnn.node_embedding(id) {
            // Format: node_id,dim,values...
            write!(file, "{},{}", id.index(), emb.len())?;
            for val in emb.iter() {
                write!(file, ",{}", val)?;
            }
            writeln!(file)?;
        }
    }

    Ok(())
}
```

## Performance Tips

### 1. Batch Operations

Process multiple nodes together when possible:

```rust
// Slower: individual queries
for id in node_ids {
    let emb = gnn.node_embedding(id);
    // process...
}

// Faster: collect all embeddings first
let embeddings: Vec<_> = node_ids.iter()
    .filter_map(|&id| gnn.node_embedding(id).map(|e| (id, e)))
    .collect();
// then process...
```

### 2. Cache Subgraph Embeddings

Subgraph embeddings require aggregation; cache when reusing:

```rust
use rustc_hash::FxHashMap;

struct EmbeddingCache {
    node_embeddings: FxHashMap<NodeId, Array1<f32>>,
    subgraph_embeddings: FxHashMap<Vec<NodeId>, Array1<f32>>,
}

impl EmbeddingCache {
    fn get_subgraph(&mut self, gnn: &CpgGnn, nodes: &[NodeId]) -> &Array1<f32> {
        self.subgraph_embeddings.entry(nodes.to_vec())
            .or_insert_with(|| gnn.subgraph_embedding(nodes))
    }
}
```

### 3. Reduce Dimensionality for Storage

If storing many embeddings, consider PCA or random projection:

```rust
// Reduce 128-dim to 32-dim for storage
fn reduce_dimension(embedding: &Array1<f32>, target_dim: usize) -> Array1<f32> {
    // Simple random projection (in practice, use trained projection)
    let mut result = Array1::zeros(target_dim);
    let step = embedding.len() / target_dim;
    for i in 0..target_dim {
        result[i] = embedding[i * step];
    }
    result
}
```

## Next Steps

- [Message Passing](message-passing.md) - How embeddings are computed
- [GNN Overview](overview.md) - Back to overview
- [Pattern Detection](../patterns/overview.md) - Using embeddings for patterns
