# 0001 — Unified overlay graph on petgraph

## Status

**Accepted.** Realised in `src/graph/cpg.rs` (the `CodePropertyGraph` type) and
exercised by every analysis module. Foundational: ADR-0003 (data flow),
ADR-0004 (pattern matching), and the PDG/slicing layer all read and write the
single graph this record describes.

## Context

A [Code Property Graph](../GLOSSARY.md#code-property-graph-cpg) (CPG), introduced
by Yamaguchi et al. [1], exists to answer questions that no single program view
can answer alone. A taint query, for example, asks: *can a value from an
untrusted source reach a sensitive sink?* — and to decide it you must follow
**data flow** (source → value), cross into the **syntax tree** (which expression
is the sink's argument), and check **control flow** (is the sink actually
reachable?). The whole point of a CPG is that these three views share structure.

That sharing is only cheap if it is *literal*. Consider the design space for how
to store the views:

- If each view is its **own graph** with its own node identities, then every
  hop that crosses from one view to another — and cross-view hops are the entire
  reason a CPG exists — must translate an identity from one structure into
  another via a side map. A four-view query pays that translation at every step.
- If the views **share one node set**, a cross-view hop is a local edge
  traversal: from a node you already hold, you follow its data-flow out-edges,
  then its syntactic parent, then its control-flow successors, never leaving the
  structure and never translating an identity.

`libcpg` also wants to reuse mature graph algorithms rather than re-implement
them — most concretely `petgraph`'s dominator routine, which the PDG builder
uses (`petgraph::algo::dominators::simple_fast`; see
[`0004`](0004-relaxed-vf2-detection.md)'s sibling PDG layer and
[`../theory/04-program-dependence-and-slicing.md`](../theory/04-program-dependence-and-slicing.md)).
And it wants **stable node identities**: a `NodeId` handed to a caller must keep
meaning for the life of the graph, even though a raw graph library's internal
indices can move.

## Decision

**Represent the CPG as one shared node set with typed edge *overlays*, stored in
a single `petgraph` directed graph.**

Concretely, in `src/graph/cpg.rs`:

```rust
// feature-free: this is the core surface, no Cargo feature required.
#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CodePropertyGraph {
    /// The underlying directed graph — nodes and edges live here, once.
    graph: DiGraph<CpgNode, CpgEdge>,
    /// Stable NodeId → petgraph NodeIndex.
    node_index_map: FxHashMap<NodeId, NodeIndex>,
    /// Stable EdgeId → petgraph EdgeIndex.
    edge_index_map: FxHashMap<EdgeId, EdgeIndex>,
    language: Language,
    // source_path, source_code, root, cfg_entries, cfg_exits …
}
```

Three commitments follow from that shape:

1. **One node, added once.** Every program construct — a function, an `if`, an
   identifier — becomes exactly one [`CpgNode`](../GLOSSARY.md#node-kind--edge-kind)
   with a stable `NodeId`. The [AST](../GLOSSARY.md#abstract-syntax-tree-ast) is
   the base layer; the [CFG](../GLOSSARY.md#control-flow-graph-cfg),
   [DFG](../GLOSSARY.md#data-flow-graph-dfg), and — on demand — the
   [PDG](../GLOSSARY.md#program-dependence-graph-pdg) add **edges over the same
   nodes**, never new copies of them.

2. **The overlay is encoded in the edge kind, not the storage.** A single
   [`CpgEdgeKind`](../GLOSSARY.md#node-kind--edge-kind) enum tags every edge with
   the layer it belongs to: `AstChild` / `AstParent` / `AstNextSibling` /
   `AstPrevSibling` for syntax; `ControlFlow(CfgEdgeKind)` for control;
   `DataFlow(DfgEdgeKind)` for data; `ControlDependence` / `DataDependence` for
   the PDG; plus call, type, scope, and import edges. The kind carries the
   classifier helpers `is_ast`, `is_cfg`, `is_dfg`, `is_pdg`, `is_call`,
   `is_type`, so an overlay is *a filter over one edge set*, not a separate
   container.

3. **Stable ids, decoupled from `petgraph`'s indices.** `NodeId(u32)` and
   `EdgeId` are the public currency; the two `FxHashMap`s translate them to and
   from `petgraph`'s internal `NodeIndex`/`EdgeIndex`. Callers hold `NodeId`s
   that never shift under them.

Navigation is exposed as **overlay-typed accessors** that each filter incident
edges by class, so callers never hand-roll the filtering:

```rust
// feature-free: hand-build two nodes and one AST edge, then read the overlay.
use libcpg::{CodePropertyGraph, CpgNode, CpgNodeKind, CpgEdgeKind, Language,
             NodeId, SourceRange, ScopeId};

let mut cpg = CodePropertyGraph::new(Language::Rust);
let block = cpg.add_node(CpgNode::new(
    NodeId::new(0), CpgNodeKind::Block { scope: ScopeId::GLOBAL }, SourceRange::default(),
));
let ret = cpg.add_node(CpgNode::new(
    NodeId::new(0), CpgNodeKind::Return, SourceRange::default(),
));
cpg.connect(block, ret, CpgEdgeKind::AstChild);

// The AST overlay: children of `block`.
for child in cpg.ast_children(block) {
    let _kind = &cpg.node(child).expect("node exists").kind; // node() -> Option, not Result
}
// Other overlays read the SAME nodes: cfg_successors / dfg_successors / callees …
```

The `is_*` edge classifiers let a query walk exactly one overlay while every
overlay shares the node it started from. Metrics computed over a single overlay
fall out directly — [cyclomatic complexity](../GLOSSARY.md#cyclomatic-complexity)
is a count over the control-flow edge subset,

```math
M = E - N + 2
```

where $`E`$ and $`N`$ are the CFG edge and node counts of one
function's control-flow overlay — McCabe's metric, surfaced as
`cyclomatic_complexity()` and defined with its citation in the
[glossary](../GLOSSARY.md#cyclomatic-complexity).

## Consequences

### Positive

- **Cross-layer queries are local traversals.** A taint walk — DFG successor,
  then syntactic parent, then CFG reachability — is a sequence of edge
  traversals on one graph with no identity translation between structures. This
  is the property that makes a CPG worth building [1].
- **Node payload is stored once.** A `CpgNode`'s `text: Option<Arc<str>>`,
  `range`, and properties are held a single time regardless of how many overlays
  touch the node; the [`Arc<str>`](../GLOSSARY.md#petgraph) interning means even
  the label text is shared.
- **Mature algorithms come for free.** Because the store *is* a `petgraph`
  graph, the PDG builder calls `petgraph`'s `dominators::simple_fast` directly on
  the (reversed) CFG overlay instead of shipping a bespoke dominator pass.
- **Stable ids survive mutation.** The `NodeId → NodeIndex` map means public ids
  are not perturbed by internal graph operations; callers can cache them.
- **One serialization boundary.** With the `serde` feature the entire graph —
  all overlays at once — round-trips through `serde_json` via a derive
  (`petgraph/serde-1`); there is no per-overlay export format to keep in sync.

### Negative

- **Every traversal must filter by kind.** A node's out-edges mix overlays, so
  code cannot assume "all out-edges are AST children." This is a real footgun,
  mitigated (not removed) by always going through the overlay accessors
  (`ast_children`, `cfg_successors`, `dfg_successors`, `callees`) rather than
  raw edge iteration.
- **Index invalidation is a latent hazard.** `petgraph`'s `NodeIndex` is
  invalidated by node *removal*; the id map hides this from callers, and the
  construction pipeline is append-mostly (extractors *add* edges — they are
  [idempotent](../GLOSSARY.md#idempotent) and never delete nodes), so the hazard
  does not arise in practice. A future feature that removes nodes would have to
  respect the map.
- **Two extra maps per graph.** The `node_index_map` and `edge_index_map` cost
  one `FxHashMap` entry per node and per edge on top of `petgraph`'s own
  storage — the price of decoupling public ids from internal indices.

## Alternatives considered

1. **Separate graphs per view** (one AST graph, one CFG graph, one DFG graph),
   keyed by a shared `NodeId`. *Rejected.* Every cross-view hop — the common
   case — becomes a lookup into another structure, and the
   [PDG](../GLOSSARY.md#program-dependence-graph-pdg), which spans control *and*
   data dependence, would need a fifth graph or an awkward join across two.
   Keeping four (or five) structures mutually consistent is exactly the
   bookkeeping the single-node-set design deletes.

2. **A hand-rolled adjacency store** — `Vec<CpgNode>` plus
   `HashMap<NodeId, Vec<EdgeId>>` adjacency and reverse-adjacency lists. This is
   what an early draft used (its shape still lingers in the pre-rewrite
   `components/graph/overview.md`). *Rejected.* It re-implements what `petgraph`
   already provides — traversal, and crucially the dominator algorithm the PDG
   needs — and a hand-maintained reverse-adjacency list is an easy place for
   consistency bugs. Delegating to `petgraph` is less code and more correct.

3. **An external property-graph database** (the Neo4j-backed approach of the
   original Joern tooling around Yamaguchi et al. [1]). *Rejected.* `libcpg` is
   an in-process Rust library meant to be embedded; a database dependency would
   impose a query language, a serialization boundary, and an out-of-process
   round-trip on callers who just want a graph in memory. An embedded `petgraph`
   keeps the CPG a plain Rust value.

4. **A single untyped edge set with edge *labels* checked by string.** *Rejected*
   in favour of the typed `CpgEdgeKind` enum: the enum makes the overlay a
   compile-time-checked, exhaustively-matchable category (with the `is_ast` /
   `is_cfg` / `is_dfg` / `is_pdg` helpers), where strings would push overlay
   membership to run-time and invite typos.

![The CPG overlay model: one node set with AST, CFG, and DFG edges drawn in distinct colours over the same nodes](../diagrams/cpg-overlay.svg)

*Figure — one shared node set with three edge overlays (AST solid, CFG and DFG typed) over the identical nodes; the PDG adds a fourth overlay on demand. Source: [`diagrams/cpg-overlay.dot`](../diagrams/cpg-overlay.dot).*

## Related decisions and further reading

- The formal model of the shared node set and typed overlays:
  [`../theory/01-code-property-graphs.md`](../theory/01-code-property-graphs.md).
- Where this type sits among the modules:
  [`../architecture/graph-data-model.md`](../architecture/graph-data-model.md)
  and [`../architecture/overview.md`](../architecture/overview.md).
- The overlays that write onto this graph: [ADR-0003](0003-ast-ordered-reaching-defs.md)
  (DFG) and [ADR-0004](0004-relaxed-vf2-detection.md) (pattern matching over it).

## References

1. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and Discovering Vulnerabilities with Code Property Graphs.* 2014 IEEE Symposium on Security and Privacy. DOI: [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44)
