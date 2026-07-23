# Serialization

libcpg can persist and reload a [`CodePropertyGraph`](../GLOSSARY.md#code-property-graph-cpg) through [serde](https://serde.rs). The design is deliberately minimal and worth stating precisely, because it shapes how you use it:

> Serialisation is **derive-based**. libcpg puts `#[derive(Serialize, Deserialize)]` on its graph types (behind the `serde` feature) and stops there. There is **no bespoke `export`/`import` function** and **no on-disk format specification**. You choose the format — JSON, bincode, MessagePack, anything with a serde backend — and drive it with your own crate (e.g. `serde_json`).

That keeps the library format-agnostic and the surface tiny. This guide shows the two round-trip patterns, the ordering rule for reconstruction, and one field you must know is skipped.

---

## Step 1: enable the feature

Turn on `serde` for libcpg, and add whichever serde data format you want to your own `Cargo.toml`:

```toml
[dependencies]
libcpg = { version = "0.1", features = ["serde", "lang-rust"] }
serde = { version = "1", features = ["derive"] } # only needed for the portable pattern below
serde_json = "1"
```

The `serde` feature expands to `["dep:serde", "petgraph/serde-1"]` — it pulls in serde itself and turns on serde support in `petgraph`, so the underlying `DiGraph` (and therefore the whole `CodePropertyGraph`) becomes serialisable. The graph types that gain `Serialize`/`Deserialize` include `CodePropertyGraph`, `CpgNode`, `CpgEdge`, all the kind enums (`CpgNodeKind`, `CpgEdgeKind`, `CfgEdgeKind`, `DfgEdgeKind`), `NodeId`/`EdgeId`, `SourceRange`, `Language`, and `CpgStats`.

---

## Pattern A: round-trip the whole graph

Because `CodePropertyGraph` derives both traits, the simplest path is to serialise the graph value directly and deserialise it back. Serialisation errors here are your data format's errors (`serde_json::Error`), not `libcpg::Error`, so a `Box<dyn Error>` return keeps the example honest.

```rust
// requires: libcpg features = ["serde", "lang-rust"]; plus serde_json in your deps
use libcpg::{CodePropertyGraph, CpgBuilder, Language, TreeSitterCpgBuilder};

fn round_trip() -> Result<(), Box<dyn std::error::Error>> {
    let cpg = TreeSitterCpgBuilder::new()
        .build("fn main() { let x = 1; let y = x; }", Language::Rust)?;

    // Serialise the entire graph.
    let json = serde_json::to_string(&cpg)?;

    // Reload it into a CodePropertyGraph.
    let restored: CodePropertyGraph = serde_json::from_str(&json)?;

    // Same shape as the original.
    assert_eq!(restored.node_count(), cpg.node_count());
    assert_eq!(restored.edge_count(), cpg.edge_count());
    assert_eq!(restored.language(), cpg.language());
    Ok(())
}
```

This is the right choice when producer and consumer are both libcpg and you just want durable storage or IPC. The serialised form mirrors the internal `petgraph` layout.

---

## Pattern B: a portable node/edge snapshot

When you want a format that does **not** depend on `petgraph`'s internal representation — for interchange with another tool, a database schema, or a stable file format you control — serialise the **nodes and edges as plain lists** and rebuild the graph yourself. `CpgNode` and `CpgEdge` are self-contained (each carries its own `id`, and edges carry `source`/`target`), so the lists are all you need.

Reconstruction uses the two id-preserving methods that exist for exactly this purpose:

- `add_node_with_id(node)` inserts a node keeping its original `NodeId`.
- `add_edge_with_id(edge)` inserts an edge keeping its original `EdgeId` — but it returns `None` (and adds nothing) if either endpoint is not present yet.

```rust
// requires: libcpg features = ["serde"]; plus serde + serde_json in your deps
use libcpg::{CodePropertyGraph, CpgEdge, CpgNode, Language};

/// A portable, petgraph-independent snapshot.
#[derive(serde::Serialize, serde::Deserialize)]
struct CpgSnapshot {
    language: Language,
    nodes: Vec<CpgNode>,
    edges: Vec<CpgEdge>,
}

fn to_snapshot(cpg: &CodePropertyGraph) -> CpgSnapshot {
    CpgSnapshot {
        language: cpg.language(),
        nodes: cpg.nodes().cloned().collect(),
        edges: cpg.edges().cloned().collect(),
    }
}

fn from_snapshot(snap: CpgSnapshot) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(snap.language);

    // Nodes FIRST — add_edge_with_id needs both endpoints already present.
    for node in snap.nodes {
        cpg.add_node_with_id(node);
    }
    for edge in snap.edges {
        cpg.add_edge_with_id(edge);
    }
    cpg
}
```

**The ordering rule is not optional:** add all nodes before any edge. If you interleave, an edge whose target has not been inserted yet is silently dropped by `add_edge_with_id`. (This is precisely how libcpg's own `subgraph`/`function_cfg`/`function_dfg` extractors rebuild graphs internally: nodes first, then edges.)

A snapshot like this also gives you a natural place to store your own metadata (a schema version, a source hash) alongside the graph, which the whole-graph form does not.

---

## Caveat: JSON and node properties

`CpgNode` has a `properties: FxHashMap<PropertyKey, PropertyValue>` field where `PropertyKey` is an enum. For **parser-built and mapper-built graphs this map is always empty**, so it serialises to `{}` and JSON round-trips cleanly (as Pattern A demonstrates). If you populate `properties` yourself with the `PropertyKey::Custom(..)` variant, be aware that JSON object keys must be strings; a non-string map key will fail `serde_json`. For graphs with rich `properties`, prefer a format that supports arbitrary map keys (bincode, MessagePack, CBOR) or move the data into a string-keyed container. This is a property of JSON, not of libcpg.

---

## GNN embeddings: the vector is skipped

If you use the [GNN](../GLOSSARY.md#graph-neural-network-gnn) (feature `gnn`), note one deliberate omission. `NodeEmbedding` and `SubgraphEmbedding` derive serde, but their heavy numeric payload is **not** serialised:

```rust
// From libcpg's src/gnn/embeddings.rs — illustrative, not something you write.
pub struct NodeEmbedding {
    pub node_id: NodeId,
    #[cfg(feature = "gnn")]
    #[serde(skip)]          // <-- the vector is intentionally omitted
    pub vector: Array1<f32>,
    pub dim: usize,
}
```

So a serialised embedding preserves its `node_id` and `dim` (and, for a `SubgraphEmbedding`, its `node_ids` and `aggregation`) but **not the vector itself**. The rationale: embeddings are a *derived* product of the graph and the model — cheaper and more reliable to **recompute** than to store and risk staleness. After reloading a graph, regenerate embeddings by running message passing again (`CpgGnn::propagate`), as shown in [Embeddings](../components/gnn/embeddings.md). Do not expect embedding vectors to survive a round-trip.

---

## Choosing a pattern

| You want… | Use |
|-----------|-----|
| Durable storage / IPC between libcpg processes | **Pattern A** (whole-graph round-trip) |
| A stable, tool-independent interchange format | **Pattern B** (node/edge snapshot) |
| Compactness or non-string map keys | **Pattern B** with bincode/MessagePack |
| Embedding vectors preserved | Not possible — recompute with the GNN |

---

## Next steps

- [Building CPGs](01-building-cpgs.md) — produce the graphs you serialise.
- [Embeddings](../components/gnn/embeddings.md) — why the vector is derived, and how to recompute it.
- The id-preserving reconstruction methods (`add_node_with_id`, `add_edge_with_id`) are documented in the [builder reference](../api/builder-reference.md) and [graph reference](../api/graph-reference.md).

---

## References

This guide relies on no external literature; serialisation here is standard serde usage. See the [glossary references](../GLOSSARY.md#references) for the works underpinning the graph model it serialises.
</content>
