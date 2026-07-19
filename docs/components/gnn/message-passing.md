# Message Passing

Message passing is the core algorithm that powers GNN-based code embeddings. Each node iteratively gathers information from its neighbors, building up a representation that captures the node's context in the graph.

## How Message Passing Works

### The Basic Idea

Think of message passing like a game of telephone, but more structured:

1. Each node starts with its own features (node type, etc.)
2. Each node sends its current representation to neighbors
3. Each node combines received messages with its own representation
4. Repeat steps 2-3 for several iterations
5. Final representations encode neighborhood information

```
Iteration 0:         Iteration 1:         Iteration 2:
    [A₀]                 [A₁]                 [A₂]
     │                    │                    │
     ▼                    ▼                    ▼
    [B₀] ─▶ [C₀]        [B₁] ─▶ [C₁]        [B₂] ─▶ [C₂]

A₀ = init(A)         A₁ = agg(A₀, B₀)     A₂ = agg(A₁, B₁)
B₀ = init(B)         B₁ = agg(B₀, A₀, C₀) B₂ = agg(B₁, A₁, C₁)
C₀ = init(C)         C₁ = agg(C₀, B₀)     C₂ = agg(C₁, B₁)

After 2 iterations, C knows about A (2 hops away)
```

### The CPGNN Algorithm

CPGNN uses a simple but effective message passing scheme:

```
Algorithm: CPGNN Message Passing
────────────────────────────────
Input: CPG G = (V, E), iterations K
Output: Node embeddings {h_v | v ∈ V}

1. Initialize: For each node v:
   h_v⁰ = init_embedding(v.type)

2. Propagate: For k = 1 to K:
   For each node v:
     // Gather messages from all neighbor types
     messages = []

     // AST neighbors
     for u in ast_children(v) ∪ ast_parent(v):
       messages.append(h_u^(k-1))

     // CFG neighbors
     for u in cfg_successors(v) ∪ cfg_predecessors(v):
       messages.append(h_u^(k-1))

     // DFG neighbors
     for u in dfg_successors(v) ∪ dfg_predecessors(v):
       messages.append(h_u^(k-1))

     // Aggregate
     h_v^k = ReLU(mean(h_v^(k-1), messages))

3. Return: {h_v^K | v ∈ V}
```

## Multi-Edge Type Aggregation

What makes CPGNN special is aggregating from multiple edge types simultaneously:

```
                    ┌─────────────────┐
                    │   Current Node  │
                    │     h_v^(k-1)   │
                    └────────┬────────┘
                             │
         ┌───────────────────┼───────────────────┐
         │                   │                   │
         ▼                   ▼                   ▼
    ┌─────────┐        ┌─────────┐        ┌─────────┐
    │   AST   │        │   CFG   │        │   DFG   │
    │ Messages│        │ Messages│        │ Messages│
    └────┬────┘        └────┬────┘        └────┬────┘
         │                   │                   │
         │   ┌───────────────┼───────────────┐   │
         │   │               │               │   │
         └───┴───────────────┴───────────────┴───┘
                             │
                             ▼
                    ┌─────────────────┐
                    │    Aggregate    │
                    │   mean(all)     │
                    └────────┬────────┘
                             │
                             ▼
                    ┌─────────────────┐
                    │    ReLU(·)      │
                    └────────┬────────┘
                             │
                             ▼
                    ┌─────────────────┐
                    │   h_v^k         │
                    └─────────────────┘
```

Each edge type contributes different information:

| Edge Type | Information Captured |
|-----------|---------------------|
| AST | Syntactic structure, nesting |
| CFG | Control flow paths, reachability |
| DFG | Data dependencies, def-use chains |

## Implementation Details

### Node Initialization

Before message passing, nodes are initialized with type-based features:

```rust
fn node_type_features(&self, kind: &CpgNodeKind) -> Vec<f32> {
    let mut features = vec![0.0; 16];

    match kind {
        CpgNodeKind::Root => features[0] = 1.0,
        CpgNodeKind::Module { .. } => features[1] = 1.0,
        CpgNodeKind::Class { .. } | CpgNodeKind::Struct { .. } => features[2] = 1.0,
        CpgNodeKind::Function { .. } => features[3] = 1.0,
        CpgNodeKind::Variable { .. } | CpgNodeKind::Field { .. } => features[4] = 1.0,
        CpgNodeKind::If | CpgNodeKind::While | CpgNodeKind::For | CpgNodeKind::Loop => features[5] = 1.0,
        CpgNodeKind::Return | CpgNodeKind::Break | CpgNodeKind::Continue => features[6] = 1.0,
        CpgNodeKind::Call { .. } => features[7] = 1.0,
        CpgNodeKind::BinaryOp { .. } | CpgNodeKind::UnaryOp { .. } => features[8] = 1.0,
        CpgNodeKind::Assignment { .. } => features[9] = 1.0,
        CpgNodeKind::Identifier { .. } => features[10] = 1.0,
        CpgNodeKind::Literal { .. } => features[11] = 1.0,
        CpgNodeKind::Block { .. } => features[12] = 1.0,
        CpgNodeKind::Parameter { .. } => features[13] = 1.0,
        CpgNodeKind::Try | CpgNodeKind::Catch | CpgNodeKind::Throw => features[14] = 1.0,
        _ => features[15] = 1.0,
    }

    features
}
```

The full initialization combines:
1. Random values (for diversity)
2. Type-based features (for structure)

### Aggregation Step

The aggregation uses mean pooling with self-connection:

```rust
// Aggregate neighbor embeddings
let mut aggregated = current.clone();
let mut neighbor_count = 0;

// Collect from all edge types...
for child_id in cpg.ast_children(node.id()) {
    if let Some(child_emb) = embeddings.get(&child_id) {
        aggregated = aggregated + child_emb;
        neighbor_count += 1;
    }
}
// ... similar for CFG and DFG ...

// Mean aggregation (including self)
if neighbor_count > 0 {
    aggregated = aggregated / (neighbor_count as f32 + 1.0);
}

// Non-linearity
for i in 0..embedding_dim {
    aggregated[i] = aggregated[i].max(0.0);  // ReLU
}
```

### Why Mean Aggregation?

Mean pooling is chosen for:

- **Permutation invariance**: Order of neighbors doesn't matter
- **Stability**: Doesn't explode with high-degree nodes
- **Simplicity**: Easy to implement efficiently

Alternative aggregations (not currently used):

| Method | Formula | Use Case |
|--------|---------|----------|
| Mean | Σm / n | General purpose |
| Max | max(m) | Capturing dominant features |
| Sum | Σm | When count matters |
| Attention | Σ(α·m) | Learning importance |

## Receptive Field

The **receptive field** is the set of nodes that influence a node's final embedding:

```
K=1:   Only immediate neighbors
        ●─○   ○ influences ●

K=2:   Up to 2 hops away
        ●─○─◇  both ○ and ◇ influence ●

K=3:   Up to 3 hops away
        ●─○─◇─△  all three influence ●
```

For code analysis:

| Iterations | Captures |
|------------|----------|
| 1 | Immediate statements |
| 2 | Containing block/function |
| 3 | Cross-function context (via calls) |
| 4+ | Module-level patterns |

**Trade-off**: More iterations = larger context, but:
- Over-smoothing: All embeddings become similar
- Computational cost: O(iterations × edges)

Typically 2-4 iterations work best for code.

## Example: Following the Messages

Consider this simple code:

```python
x = 1       # Node A (assignment)
if x > 0:   # Node B (condition)
    y = x   # Node C (assignment, use of x)
```

The CPG has these edges:
- AST: A → B → C (children)
- CFG: A → B → C (sequential)
- DFG: A → B (def-use x), A → C (def-use x)

**Iteration 0 (initialization):**
```
A: [0.1, 0.9, 0.2, ...]  (assignment features)
B: [0.8, 0.1, 0.3, ...]  (condition features)
C: [0.1, 0.9, 0.2, ...]  (assignment features)
```

**Iteration 1:**
```
A₁ = ReLU(mean(A₀, B₀))           # A gets info from B
B₁ = ReLU(mean(B₀, A₀, C₀))       # B gets from A and C
C₁ = ReLU(mean(C₀, B₀, A₀))       # C gets from B and A (via DFG)
```

**Iteration 2:**
```
A₂ = ReLU(mean(A₁, B₁))           # A now knows about C (via B)
B₂ = ReLU(mean(B₁, A₁, C₁))       # B has full context
C₂ = ReLU(mean(C₁, B₁, A₁))       # C has full context
```

After 2 iterations, each node's embedding captures the entire code snippet.

## Parallelization

Message passing is embarrassingly parallel per iteration:

```rust
// Parallel node update using rayon
use rayon::prelude::*;

let new_embeddings: FxHashMap<NodeId, Array1<f32>> = cpg.nodes()
    .par_bridge()
    .map(|node| {
        let aggregated = aggregate_neighbors(node.id(), &embeddings);
        (node.id(), apply_relu(aggregated))
    })
    .collect();
```

However, iterations must be sequential (each depends on the previous).

## Handling Graph Structure

### Disconnected Components

If the CPG has disconnected components (e.g., unrelated functions), nodes in different components don't influence each other:

```
Component 1:       Component 2:
   A ─── B            X ─── Y

A and X never exchange messages.
```

This is usually desirable for function-level analysis.

### Cycles

The algorithm handles cycles naturally through fixed-point iteration:

```
    A ─── B
    │     │
    └─ C ─┘

Info flows: A→B→C→A→B→... until convergence
```

## Next Steps

- [Embeddings](embeddings.md) - Working with computed embeddings
- [GNN Overview](overview.md) - Back to overview
- [Traversal](../graph/traversal.md) - CPG navigation
