# Serialization

libcpg can persist and reload a [`CodePropertyGraph`](../GLOSSARY.md#code-property-graph-cpg) through [serde](https://serde.rs). The outer format remains caller-selected, with deliberate inner representations for logically recursive values:

> Graph serialization is **Serde-based**. Most graph types derive
> `Serialize`/`Deserialize`; `PropertyValue` and `ExprKey` implement the traits
> explicitly and embed flat, bounded, versioned CPV1 and CEK1 bytes. There is no whole-graph
> `export`/`import` function or prescribed outer format. You choose JSON,
> bincode, MessagePack, CBOR, or another Serde backend.

The CPV1 and CEK1 exceptions are load-bearing: the former recursive property
and expression types made lifecycle and persistence consume native stack in
proportion to logical depth. This guide shows the two graph round-trip
patterns, both migrations, the ordering rule for reconstruction, and one field
you must know is skipped.

---

## Step 1: enable the feature

Turn on `serde` for libcpg, and add whichever serde data format you want to your own `Cargo.toml`:

```toml
[dependencies]
libcpg = { version = "0.1", features = ["serde", "lang-rust"] }
serde = { version = "1", features = ["derive"] } # only needed for the portable pattern below
serde_json = "1"
```

The `serde` feature expands to `["dep:serde", "petgraph/serde-1"]` — it pulls in serde itself and turns on serde support in `petgraph`, so the underlying `DiGraph` (and therefore the whole `CodePropertyGraph`) becomes serialisable. Supported graph types include `CodePropertyGraph`, `CpgNode`, `CpgEdge`, `PropertyValue`, all the kind enums (`CpgNodeKind`, `CpgEdgeKind`, `CfgEdgeKind`, `DfgEdgeKind`, `MsgKind`, `LockKind`, and `LockMode`), `NodeId`/`EdgeId`, `SourceRange`, `Language`, and `CpgStats`.

Analysis result types include `ExprKey` and `ExprFacts`; their expression roots
serialize through CEK1 rather than a recursive Serde visitor.

`CpgNode.message_kind` and `CpgNode.lock_kind` are optional typed concurrency
metadata. Both fields deserialize with a `None` default, so snapshots produced
before either field existed remain readable. When present, the enum value is
preserved exactly across a serde round trip.

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

## Node properties: flat values, CPV1, and JSON keys

`CpgNode` has a `properties: FxHashMap<PropertyKey, PropertyValue>` field.
`PropertyValue` has logical string, signed, unsigned, boolean, float, list, and
null forms, but owns lists as flat postorder node and edge tapes. Constructors
retain their earlier spellings:

```rust
use libcpg::{PropertyValue, PropertyValueKind};

let value = PropertyValue::List(vec![
    PropertyValue::String("role".into()),
    PropertyValue::Int(7),
    PropertyValue::Null,
]);

if let PropertyValueKind::List(children) = value.kind() {
    assert_eq!(children.get(1).and_then(|child| child.as_int()), Some(7));
}
```

Recursive enum patterns such as `PropertyValue::List(children)` are the one
source migration: use `kind()` or `as_list()`. When Serde is enabled, the value
serializes as canonical CPV1 bytes. The inner format validates node/edge tree
shape and independent node, edge, string-byte, encoded-byte, and work limits;
it also preserves every floating-point bit pattern. See
[ADR-0040](../design/0040-flat-property-value-arena.md).

This CPV1 shape is intentionally incompatible with an old unversioned recursive
Serde property. Version the owning snapshot. For a cache, change its envelope
or builder signature and rebuild old rows; pgmcp uses cache envelope v2 with a
`property-cpv1` signature. Do not retry a failed CPV1 decode through the old
recursive visitor, because that would restore the stack-exhaustion path.

There is a separate JSON concern: `PropertyKey` is an enum, while JSON object
keys must be strings. Parser-built and mapper-built property maps are normally
empty. If you populate the map and the selected key variant cannot be a JSON
object key, use a format with arbitrary map keys or store a string-keyed
snapshot. This limitation belongs to JSON's object model, not CPV1.

---

## Expression keys: shared arenas and CEK1

Construct keys with the fallible API and observe them through borrowed views:

```rust
use libcpg::{ExprKey, ExprOperand, ExprOperandRef};

let nested = ExprKey::try_new("-", vec![ExprOperand::Variable("x".into())])?;
let key = ExprKey::try_new(
    "+",
    vec![ExprOperand::Expression(nested), ExprOperand::Integer(1)],
)?;

assert_eq!(key.operator(), "+");
assert!(matches!(
    key.operands().get(0),
    Some(ExprOperandRef::Expression(_))
));

let json = serde_json::to_vec(&key)?;
let decoded: ExprKey = serde_json::from_slice(&json)?;
assert_eq!(decoded, key);
# Ok::<(), Box<dyn std::error::Error>>(())
```

`expression_facts` shares one physical postorder arena across every root in its
`ExprUniverse`; this prevents quadratic ownership on nested syntax. A
standalone serialized key uses canonical CEK1 bytes. If a shared child appears
twice, CEK1 emits two tree occurrences so one byte string has one physical
interpretation. Node, operand, string-byte, encoded-byte, and work limits are
independent. `ExprKey::decode_with_limits` and
`expression_facts_with_limits` expose caller-selected policies.

The source migrations are intentional: use `ExprKey::try_new` instead of a
struct literal, `ExprOperand::Expression(key)` instead of a boxed key, and
`operator()`/`operands()` instead of public recursive fields. Failed CEK1
decoding is final; do not retry the retired recursive Serde shape. See
[ADR-0041](../design/0041-shared-flat-expression-facts.md).

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
