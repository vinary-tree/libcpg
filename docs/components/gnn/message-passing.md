# Message Passing

Message passing is the engine of `CpgGnn`. Each node repeatedly gathers its neighbours' current vectors, averages them together with its own, and squashes the result — building up a representation that encodes the node's *context* across all three CPG overlays. This page gives the exact rule `libcpg` runs, honestly.

> **Feature gate.** Everything here requires the `gnn` feature (see [Overview](overview.md)). Without it there is no `CpgGnn`.

## The telephone-game intuition

Think of message passing as a game of telephone, but structured and simultaneous:

1. Every node starts with its own features (a code of its node type, plus a little noise).
2. Every node reads its neighbours' current vectors.
3. Every node averages those messages together with its own vector, then applies a nonlinearity.
4. Repeat steps 2–3 for a fixed number of rounds.
5. After $`K`$ rounds, a node's vector reflects everything within $`K`$ hops of it.

Unlike telephone, the message is not distorted as it hops — it is *averaged in*. And unlike telephone, the whole graph passes messages at once, in lockstep: round $`k`$ reads only the vectors produced in round $`k-1`$.

## The aggregation rule

For node $`v`$ at round $`k`$, `libcpg` computes:

```math
h_v^{(k)} = \mathrm{ReLU}\!\left( \mathrm{mean}\left( \{ h_u^{(k-1)} : u \in \mathcal{N}(v) \} \cup \{ h_v^{(k-1)} \} \right) \right)
```

where:

- $`h_v^{(k)}`$ is $`v`$'s embedding after round $`k`$;
- $`\mathcal{N}(v)`$ is $`v`$'s neighbourhood **across the three overlays** (defined below);
- the self-term $`\{ h_v^{(k-1)} \}`$ is the *self-connection* — $`v`$ always includes its own previous vector in the average;
- $`\mathrm{ReLU}(x) = \max(0, x)`$ is applied element-wise, per the [ReLU](../../GLOSSARY.md#relu) definition;
- $`k`$ runs $`1 \ldots K`$, and $`K`$ is the `iterations` argument to `propagate` — **not** the configured `num_layers` (which the forward pass does not read).

Because the mean's denominator is "number of neighbours $`+ 1`$", the self-connection keeps a node anchored to its own identity even in a dense neighbourhood.

## What `propagate` actually does

The following pseudocode mirrors the shipped implementation exactly, including the self-connection and the synchronous (previous-generation) read.

```text
propagate(iterations):
  if embeddings is empty: initialize_embeddings()      # lazy, one-time

  repeat `iterations` times:                           # K rounds; the argument, not num_layers
    next ← {}                                           # a FRESH generation
    for each node v in cpg:
      acc   ← copy of h[v]                              # start from self (the self-connection)
      count ← 0

      for u in ast_children(v):     acc += h[u]; count += 1
      if ast_parent(v) exists:      acc += h[parent];  count += 1
      for (u,_) in cfg_successors(v):   acc += h[u]; count += 1
      for (u,_) in cfg_predecessors(v): acc += h[u]; count += 1
      for (u,_) in dfg_successors(v):   acc += h[u]; count += 1
      for (u,_) in dfg_predecessors(v): acc += h[u]; count += 1

      if count > 0: acc ← acc / (count + 1)             # mean incl. self
      acc ← elementwise max(acc, 0.0)                    # ReLU
      next[v] ← acc

    h ← next                                            # swap in the new generation
  mark initialized
```

Two details follow from this that are easy to miss:

- **Synchronous update.** The new generation `next` is built entirely from the previous generation `h`, then swapped in at the end of the round. This is a Jacobi-style update: within a round, no node sees another node's *new* vector. That is what makes the equation's $`(k-1)`$ superscript literally true.
- **Multiplicity matters.** A node that neighbours $`v`$ through more than one overlay (say it is both $`v`$'s AST parent and its CFG predecessor) is added *once per overlay*, so it carries more weight in the mean. Aggregation is over the multiset of overlay-neighbours, not the set.

## Aggregating over three overlays

$`\mathcal{N}(v)`$ unions six directed neighbourhoods: AST children and parent, CFG successors and predecessors, DFG successors and predecessors. Each overlay contributes a different kind of context.

![Mean aggregation pulls a node's new vector from its AST, CFG, and DFG neighbours plus itself, followed by ReLU.](../../diagrams/gnn-message-passing.svg)

*Figure — mean aggregation over the three overlays. Source: [`diagrams/gnn-message-passing.dot`](../../diagrams/gnn-message-passing.dot).*

| Overlay | Neighbours used | Context it injects |
|---|---|---|
| [AST](../../GLOSSARY.md#abstract-syntax-tree-ast) | children + parent | syntactic structure and nesting |
| [CFG](../../GLOSSARY.md#control-flow-graph-cfg) | successors + predecessors | execution order and reachability |
| [DFG](../../GLOSSARY.md#data-flow-graph-dfg) | successors + predecessors | def-use / value flow |

Mixing all three in one average is what makes this a *code-property-graph* GNN rather than an AST-only or CFG-only one.

## Initialisation

Before the first round, `initialize_embeddings` gives each node a starting vector that combines two ingredients:

1. **Random noise** — every one of the `embedding_dim` components is drawn uniformly from $`[-0.1, 0.1]`$, so that otherwise-identical nodes are not perfectly degenerate.
2. **A node-type code** — a 16-slot one-hot vector for the node's kind is added into the **first 16 components**. (If `embedding_dim < 16`, only the leading components receive it.)

The 16 type buckets are:

| Slot | Node kinds |
|---|---|
| 0 | `Root` |
| 1 | `Module` |
| 2 | `Class`, `Struct` |
| 3 | `Function` |
| 4 | `Variable`, `Field` |
| 5 | `If`, `While`, `For`, `Loop` |
| 6 | `Return`, `Break`, `Continue` |
| 7 | `Call` |
| 8 | `BinaryOp`, `UnaryOp` |
| 9 | `Assignment` |
| 10 | `Identifier` |
| 11 | `Literal` |
| 12 | `Block` |
| 13 | `Parameter` |
| 14 | `Try`, `Catch`, `Throw` |
| 15 | everything else |

The exact dispatch, straight from the source:

```rust
// requires: features = ["gnn"]
use libcpg::CpgNodeKind;

fn node_type_features(kind: &CpgNodeKind) -> Vec<f32> {
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

One consequence of the $`\mathrm{ReLU}`$: the initial vector may contain small negatives, but after the first round every component is clamped to $`\ge 0`$, so embeddings are non-negative from round 1 onward.

## Why mean, and why a self-connection?

Mean pooling is chosen for three practical reasons:

- **Permutation invariance** — the order in which neighbours are visited does not change the result.
- **Scale stability** — averaging (rather than summing) keeps high-degree nodes from producing runaway magnitudes.
- **Simplicity** — it needs no learned weights, matching this GNN's untrained, forward-only design.

The self-connection (including $`h_v^{(k-1)}`$ in the average) prevents a node from forgetting itself as it absorbs context, a standard stabiliser in message-passing GNNs [[1]](#references).

## Receptive field and over-smoothing

Because each round adds one hop, the **receptive field** of a node grows with $`K`$.

![Receptive-field growth: each additional round widens the set of nodes that influence a node's embedding by one hop.](../../diagrams/gnn-receptive-field.svg)

*Figure — receptive-field growth per round. Source: [`diagrams/gnn-receptive-field.dot`](../../diagrams/gnn-receptive-field.dot).*

The trade-off is real:

- Too few rounds → embeddings see only local structure.
- Too many rounds → **over-smoothing**: as receptive fields overlap, every node's average trends toward the same value and distinctions wash out.

The per-call cost is $`O(K (N + E)\,d)`$ for $`N`$ nodes, $`E`$ overlay-edges, dimension $`d`$, and $`K`$ rounds. For code, 2–4 rounds is a good default.

## Following the messages

Consider three statements and their CPG edges:

```python
x = 1       # Node A (assignment)
if x > 0:   # Node B (condition)
    y = x   # Node C (assignment; uses x)
```

- **AST**: `A → B → C` (children under the enclosing block)
- **CFG**: `A → B → C` (sequential execution)
- **DFG**: `A → B` and `A → C` (the definition of `x` reaches both uses)

Averaging over these overlays, the vectors evolve like this (schematically):

```text
round 0 (init):   A0, B0, C0        # type code + noise
round 1:          A1 = ReLU(mean(A0, B0))            # A hears B
                  B1 = ReLU(mean(B0, A0, C0))         # B hears A and C
                  C1 = ReLU(mean(C0, B0, A0))         # C hears B (AST/CFG) and A (DFG)
round 2:          A2 = ReLU(mean(A1, B1))            # via B, A now reflects C
                  B2 = ReLU(mean(B1, A1, C1))
                  C2 = ReLU(mean(C1, B1, A1))
```

After two rounds every node's vector reflects the whole three-node snippet, because two hops span the graph.

## Graph structure: disconnected parts and cycles

- **Disconnected components.** If two functions share no edges, their nodes never exchange messages — an embedding stays confined to its own component. For function-level analysis this isolation is usually what you want.
- **Cycles.** Loops in the graph (a `LoopBack` CFG edge, say) are handled automatically by the synchronous update: information keeps circulating one hop per round. Note this is a **fixed $`K`$-round** computation, not an iterate-to-convergence fixed point — you get exactly the number of rounds you ask for.

## A note on parallelism

Within a single round the node updates are independent: each reads only the previous generation and writes into a fresh one. That makes a round *data-parallel in principle*. The shipped `propagate`, however, is a plain **sequential** loop over nodes — no `rayon`, no SIMD, and no GPU path (the `gpu` feature is reserved with no code). Rounds themselves are inherently sequential, since round $`k`$ depends on round $`k-1`$.

## Related reading

- [Overview](overview.md) — `CpgGnn` configuration and the `GraphNeuralNetwork` trait.
- [Embeddings](embeddings.md) — turning the vectors here into comparable `NodeEmbedding`/`SubgraphEmbedding` values.
- [Traversal](../graph/traversal.md) — the `ast_*`/`cfg_*`/`dfg_*` navigation the aggregation is built on.
- [Theory: graph neural networks](../../theory/09-graph-neural-networks.md) — the formal message-passing model.

## References

1. Scarselli, F., Gori, M., Tsoi, A. C., Hagenbuchner, M., Monfardini, G. (2009). *The Graph Neural Network Model.* IEEE Transactions on Neural Networks 20(1). DOI: [10.1109/TNN.2008.2005605](https://doi.org/10.1109/TNN.2008.2005605)
