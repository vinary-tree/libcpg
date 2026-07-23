# Graph Neural Networks — Overview

A **[Graph Neural Network (GNN)](../../GLOSSARY.md#graph-neural-network-gnn)** learns a vector for every node by repeatedly mixing in information from its neighbours. `libcpg` ships one — `CpgGnn` — that runs [message passing](message-passing.md) over the three overlays of a [Code Property Graph](../../GLOSSARY.md#code-property-graph-cpg) (AST, CFG, DFG) and produces dense **[embeddings](../../GLOSSARY.md#embedding)** you can compare with [cosine similarity](../../GLOSSARY.md#cosine-similarity).

> **Feature gate.** The whole `gnn` module is compiled only under the `gnn` feature (`default = []`). Without it, `libcpg::gnn` and `CpgGnn` do not exist. The feature also pulls in `ndarray` (vector math) and `rand` (initialisation):
>
> ```toml
> [dependencies]
> libcpg = { version = "0.1", features = ["gnn"] }
> ```
>
> To build a CPG from source in the same program you additionally need a grammar feature such as `lang-rust`.

## Why a GNN for code?

Classic code metrics — line counts, [cyclomatic complexity](../../GLOSSARY.md#cyclomatic-complexity) — summarise a function with a scalar. A GNN instead gives every node a vector shaped by its *context*: a variable use is pulled toward its definitions along [data-flow](../../GLOSSARY.md#data-flow-graph-dfg) edges, a statement toward its predecessors along [control-flow](../../GLOSSARY.md#control-flow-graph-cfg) edges, and every node toward its syntactic parent and children along [AST](../../GLOSSARY.md#abstract-syntax-tree-ast) edges. After a few rounds, structurally similar code lands near similar code in vector space, which is exactly what clone detection, code search, and ML-feature extraction want.

`libcpg`'s design follows the message-passing GNN of Scarselli et al. [[1]](#references) and the CPG-for-vulnerability lineage of Devign [[2]](#references), specialised to mean aggregation over the three CPG overlays.

## `CpgGnn` at a glance

```rust
// requires: features = ["gnn"]
use libcpg::gnn::CpgGnn;
use libcpg::GraphNeuralNetwork; // the trait; re-exported at the crate root under `gnn`

// CpgGnn OWNS the CPG — it takes it by value (not an Arc, and there is no GnnConfig).
let mut gnn = CpgGnn::new(cpg)
    .with_embedding_dim(128)  // vector width      (default 128)
    .with_num_layers(3)       // configured depth  (default 3)
    .with_dropout(0.1);       // training dropout  (default 0.1)
```

`CpgGnn::new` consumes a `CodePropertyGraph` by value; the GNN then holds it for the lifetime of the network. You can borrow it back with `gnn.cpg() -> &CodePropertyGraph`. There is no `Arc`, no shared ownership, and no separate configuration struct — the three `with_*` builders are the entire knob set.

![CpgGnn architecture: type-based node initialisation feeds message-passing rounds over AST, CFG, and DFG, yielding node and subgraph embeddings.](../../diagrams/gnn-architecture.svg)

*Figure — `CpgGnn` from initialisation to embeddings. Source: [`diagrams/gnn-architecture.puml`](../../diagrams/gnn-architecture.puml).*

### Configuration and what is actually consumed

| Builder | Field | Default | Consumed by `propagate`? |
|---|---|---|---|
| `with_embedding_dim(usize)` | `embedding_dim` | `128` | **Yes** — sets the vector width for init and every round. |
| `with_num_layers(usize)` | `num_layers` | `3` | **No** — see below. |
| `with_dropout(f32)` | `dropout` | `0.1` | **No** — reserved for a training regime not yet implemented. |

**Honesty note — the round count comes from the argument, not `num_layers`.** The forward pass is driven by the explicit `iterations` argument to `propagate(iterations)`. The stored `num_layers` records an *intended* depth but the shipped, forward-only propagation does not read it; likewise `dropout` is recorded for a future trainable pass and is not applied. So `gnn.with_num_layers(3)` followed by `gnn.propagate(5)` runs **five** rounds, not three. Choose the round count at the `propagate` call site.

## The `GraphNeuralNetwork` trait

`CpgGnn` implements this trait, which is re-exported at the crate root (under the `gnn` feature). Bring it into scope to call the methods.

```rust
// requires: features = ["gnn"]
use libcpg::NodeId;
use ndarray::Array1;

pub trait GraphNeuralNetwork: Send + Sync {
    fn propagate(&mut self, iterations: usize);
    fn node_embedding(&self, node: NodeId) -> Option<Array1<f32>>; // requires `gnn`
    fn subgraph_embedding(&self, nodes: &[NodeId]) -> Array1<f32>;  // requires `gnn`
    fn embedding_dim(&self) -> usize;
    fn is_initialized(&self) -> bool;
    fn reset(&mut self);
}
```

`node_embedding` returns a **clone** of a node's vector (or `None` if that node has no embedding yet); `subgraph_embedding` mean-pools the vectors of the nodes you pass. Both return raw `ndarray::Array1<f32>` values — the richer `NodeEmbedding`/`SubgraphEmbedding` wrapper types (with `norm`/`cosine_similarity`) are covered in [Embeddings](embeddings.md).

## Quick start

```rust
// requires: features = ["gnn", "lang-rust"]
use libcpg::{TreeSitterCpgBuilder, CpgBuilder, Language};
use libcpg::gnn::CpgGnn;
use libcpg::GraphNeuralNetwork;

fn main() -> libcpg::Result<()> {
    let source = r#"
        fn factorial(n: i32) -> i32 {
            if n <= 1 { 1 } else { n * factorial(n - 1) }
        }
    "#;

    let cpg = TreeSitterCpgBuilder::new().build(source, Language::Rust)?;

    let mut gnn = CpgGnn::new(cpg).with_embedding_dim(64);
    gnn.propagate(3);                 // run 3 message-passing rounds
    assert!(gnn.is_initialized());
    assert_eq!(gnn.embedding_dim(), 64);

    // Borrow the CPG back to look up node ids, then read an embedding.
    if let Some(func) = gnn.cpg().functions().next() {
        if let Some(vec) = gnn.node_embedding(func.id) {
            println!("factorial embedding has {} dims", vec.len());
        }
    }
    Ok(())
}
```

`propagate` initialises embeddings lazily on the first call, so you never call an explicit "init" step. Calling `reset()` drops the computed vectors; the next `propagate` re-initialises from scratch.

## Receptive field

Each round lets information travel one more hop. After $`K`$ rounds a node's embedding reflects every node within $`K`$ edges of it — its **receptive field**.

![Receptive-field growth: with each additional message-passing round, a node aggregates information from one more hop away.](../../diagrams/gnn-receptive-field.svg)

*Figure — receptive-field growth per round. Source: [`diagrams/gnn-receptive-field.dot`](../../diagrams/gnn-receptive-field.dot).*

| Rounds $`K`$ | Typically captures |
|---|---|
| 1 | immediate statements / operands |
| 2 | the containing block or expression tree |
| 3 | cross-statement context within a function |
| 4+ | broader structure, at rising risk of *over-smoothing* (all vectors converging) |

For function-level code analysis, $`K`$ between 2 and 4 is a sensible starting range. More rounds cost more and, past a point, blur nodes together rather than distinguishing them.

## Use cases

All three below rely only on real API. Comparing subgraph vectors uses the `SubgraphEmbedding` wrapper from [Embeddings](embeddings.md).

- **Clone detection** — embed two functions (each as its node set), then compare with cosine similarity; a high score flags near-duplicates.
- **ML features** — a function's `subgraph_embedding` is a fixed-width feature vector you can feed to an external classifier (e.g. via the `ml-linfa`/`ml-rules` features or your own model).
- **Semantic code search** — precompute one embedding per function and rank candidates by cosine distance to a query embedding.

```rust
// requires: features = ["gnn"]
use libcpg::gnn::{CpgGnn, SubgraphEmbedding};
use libcpg::{GraphNeuralNetwork, NodeId};

// Wrap a function (its node plus AST descendants) as one mean-pooled embedding.
fn subgraph(gnn: &CpgGnn, func: NodeId) -> SubgraphEmbedding {
    let nodes: Vec<NodeId> = std::iter::once(func)
        .chain(gnn.cpg().ast_descendants(func))
        .collect();
    let vector = gnn.subgraph_embedding(&nodes);
    SubgraphEmbedding::new(nodes, vector, Default::default()) // AggregationMethod::Mean
}

// After `gnn.propagate(k)`, embed two functions and compare them.
fn similarity(gnn: &CpgGnn, a: NodeId, b: NodeId) -> f32 {
    subgraph(gnn, a).cosine_similarity(&subgraph(gnn, b)) // 1.0 = identical direction
}
```

Note `cpg.ast_descendants(func)` returns a `Vec<NodeId>` (not an iterator), so it is chained via `into_iter`/`chain` directly.

## Honest limitations

- **Untrained, forward-only.** `CpgGnn` has no learned weights — aggregation is a fixed mean plus [ReLU](../../GLOSSARY.md#relu). Embeddings capture *structural neighbourhood*, not task-specific semantics. `dropout` and `num_layers` anticipate a trainable version that does not yet exist.
- **`Mean` is the only aggregation computed.** The `AggregationMethod` enum also lists `Sum`, `Max`, `Attention`, and `Hierarchical`; `Attention` and `Hierarchical` are **reserved placeholders**, and even `Sum`/`Max` are not applied by the built-in GNN (both message passing and subgraph pooling hard-code mean). See [Embeddings](embeddings.md).
- **No GPU, no SIMD.** The `gpu` feature is reserved with no wired code, and no SIMD path exists; `propagate` is a plain sequential loop over nodes.
- **No runnable benchmarks yet.** Costs below are analytical, not measured.

## Performance (analytical)

Let $`N`$ be the node count, $`E`$ the edge count over the three overlays, $`d`$ the embedding dimension, and $`K`$ the number of rounds.

| Operation | Time | Extra memory |
|---|---|---|
| Initialisation | $`O(N d)`$ | $`O(N d)`$ |
| One round | $`O((N + E)\,d)`$ | $`O(N d)`$ (a fresh generation) |
| `propagate(K)` | $`O(K (N + E)\,d)`$ | $`O(N d)`$ |
| `node_embedding` | $`O(d)`$ (clone) | $`O(d)`$ |
| `subgraph_embedding(S)` | $`O(|S|\,d)`$ | $`O(d)`$ |

## Related reading

- [Message passing](message-passing.md) — the aggregation equation, initialisation, and the receptive field in detail.
- [Embeddings](embeddings.md) — `NodeEmbedding`/`SubgraphEmbedding`, cosine similarity, aggregation methods, serde behaviour.
- [Theory: graph neural networks](../../theory/09-graph-neural-networks.md) — the formal model and lineage.
- [API: pattern & analysis reference](../../api/pattern-reference.md) — exact signatures for the `gnn` surface.

## References

1. Scarselli, F., Gori, M., Tsoi, A. C., Hagenbuchner, M., Monfardini, G. (2009). *The Graph Neural Network Model.* IEEE Transactions on Neural Networks 20(1). DOI: [10.1109/TNN.2008.2005605](https://doi.org/10.1109/TNN.2008.2005605)
2. Zhou, Y., Liu, S., Siow, J., Du, X., Liu, Y. (2019). *Devign: Effective Vulnerability Identification by Learning Comprehensive Program Semantics via Graph Neural Networks.* NeurIPS 2019. arXiv:[1909.03496](https://arxiv.org/abs/1909.03496) (no DOI).
