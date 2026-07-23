# Graph Traversal

`libcpg` navigates the [Code Property Graph](../../GLOSSARY.md#code-property-graph-cpg)
through a set of accessor methods on `CodePropertyGraph`, one family per overlay:
AST, CFG, DFG, the call graph, and positional/subgraph queries. This page
catalogues those accessors *with their exact return types*, then works through
the recurring analysis patterns — reachability, def-use following, and a
[taint](../../GLOSSARY.md#taint-analysis) walk — over the real API.

## Return types at a glance

The single most important thing to internalise: the AST and flow accessors
return **owned `Vec`s of ids** (or id/edge-kind pairs), *not* iterators of
`&CpgNode`. You resolve an id to a node with `node(id) -> Option<&CpgNode>`.

| Accessor | Returns |
|----------|---------|
| `node(id)` | `Option<&CpgNode>` |
| `ast_children(id)` | `Vec<NodeId>` (source order) |
| `ast_parent(id)` | `Option<NodeId>` |
| `ast_descendants(id)` | `Vec<NodeId>` (depth-first) |
| `ast_ancestors(id)` | `Vec<NodeId>` (toward the root) |
| `cfg_successors(id)` / `cfg_predecessors(id)` | `Vec<(NodeId, CfgEdgeKind)>` |
| `cfg_entries()` / `cfg_exits()` | `&[NodeId]` |
| `cfg_nodes()` | `impl Iterator<Item = &CpgNode>` |
| `reaching_definitions(use)` / `uses_of_definition(def)` | `Vec<NodeId>` |
| `dfg_successors(id)` / `dfg_predecessors(id)` | `Vec<(NodeId, DfgEdgeKind)>` |
| `call_sites(fn)` / `callees(call)` / `callers(fn)` | `Vec<NodeId>` |
| `node_at_offset(off)` / `scope_at_offset(off)` | `Option<&CpgNode>` |
| `nodes_in_range(range)` | `Vec<&CpgNode>` |
| `outgoing_edges(id)` / `incoming_edges(id)` | `impl Iterator<Item = &CpgEdge>` |
| `edges_between(a, b)` | `Vec<&CpgEdge>` |
| `subgraph(&[NodeId])` / `function_cfg(fn)` / `function_dfg(fn)` | `CodePropertyGraph` |

There is no generic `successors` / `predecessors` / `neighbors` method and no
`ast_next_sibling`; sibling and neighbour queries are derived from the accessors
above (shown below). `node(id)` returns an `Option`, so use `?` in a function
that returns `Option`, or `map_or` / `if let` inline.

## AST traversal

`ast_children` gives a node's immediate children in source order;
`ast_descendants` returns the whole subtree (depth-first); `ast_ancestors` walks
up to the root. All three return `Vec<NodeId>`, so pair them with `node(id)` to
read kinds:

```rust
use libcpg::{CodePropertyGraph, CpgNodeKind, NodeId};

fn is_loop_kind(k: &CpgNodeKind) -> bool {
    matches!(k, CpgNodeKind::While | CpgNodeKind::For | CpgNodeKind::Loop)
}

/// Loops nested inside another loop, with their nesting depth.
fn nested_loops(cpg: &CodePropertyGraph) -> Vec<(NodeId, usize)> {
    let mut out = Vec::new();
    for node in cpg.nodes_by_kind(is_loop_kind) {
        let depth = cpg
            .ast_ancestors(node.id)                                   // Vec<NodeId>
            .into_iter()
            .filter(|&a| cpg.node(a).map_or(false, |n| is_loop_kind(&n.kind)))
            .count();
        if depth > 0 {
            out.push((node.id, depth));
        }
    }
    out
}
```

Because a node knows its `parent` and its ordered `children`, sibling navigation
is a two-line derivation rather than a dedicated method:

```rust
use libcpg::{CodePropertyGraph, NodeId};

fn next_sibling(cpg: &CodePropertyGraph, id: NodeId) -> Option<NodeId> {
    let parent = cpg.ast_parent(id)?;                 // Option<NodeId>
    let siblings = cpg.ast_children(parent);          // Vec<NodeId>, source order
    let pos = siblings.iter().position(|&c| c == id)?;
    siblings.get(pos + 1).copied()
}
```

## CFG traversal

`cfg_successors` and `cfg_predecessors` return `Vec<(NodeId, CfgEdgeKind)>` — the
neighbour paired with the labelled control-flow edge. A depth-first reachability
check is the canonical use; it runs in `` $`O(V + E)`$ `` over the CFG:

```rust
use std::collections::HashSet;
use libcpg::{CodePropertyGraph, NodeId};

/// True if `to` is reachable from `from` following control-flow edges.
fn cfg_reachable(cpg: &CodePropertyGraph, from: NodeId, to: NodeId) -> bool {
    let mut seen = HashSet::new();
    let mut stack = vec![from];
    while let Some(cur) = stack.pop() {
        if cur == to {
            return true;
        }
        if !seen.insert(cur) {
            continue;                                 // already visited
        }
        for (succ, _kind) in cpg.cfg_successors(cur) {   // Vec<(NodeId, CfgEdgeKind)>
            stack.push(succ);
        }
    }
    false
}
```

The set of function entry points is `cfg_entries()` (a `&[NodeId]` slice), so a
whole-program forward walk starts there:

```rust
for &entry in cpg.cfg_entries() {
    // e.g. mark everything reachable from each function entry ...
    let _ = entry;
}
```

`cfg_nodes()` iterates the nodes that actually participate in the CFG (those with
at least one incident control-flow edge), which is what
[`cyclomatic_complexity`](../../GLOSSARY.md#cyclomatic-complexity) counts.

## DFG traversal

Data-flow navigation comes in a def-use direction and a use-def direction:

```rust
// Uses reached by a definition (outgoing DefUse edges).
let direct_uses = cpg.uses_of_definition(def_id);        // Vec<NodeId>

// Definitions that reach a use (incoming DefUse / ReachingDef edges).
let reaching = cpg.reaching_definitions(use_id);         // Vec<NodeId>

// Every data-flow successor, with the edge kind that links it.
for (next, kind) in cpg.dfg_successors(def_id) {         // Vec<(NodeId, DfgEdgeKind)>
    println!("{def_id:?} --{kind:?}--> {next:?}");
}
```

`uses_of_definition` follows only `DefUse` edges, while `dfg_successors` follows
*every* [`DfgEdgeKind`](edges.md#dfg-edges) (parameter passing, field/index
access, aliasing, …). Choose the narrower accessor when you want pure def-use
chains and the broader one when you want all value flow.

## Worked example: forward taint / reachability

[Taint analysis](../../GLOSSARY.md#taint-analysis) asks whether a value from an
untrusted **source** can reach a sensitive **sink**. Over a CPG this is a graph
reachability query on the data-flow overlay. The algorithm is a breadth-first
sweep that follows data-flow successors until the frontier is exhausted:

```text
taint_set(source):
  tainted ← {}
  queue   ← [source]
  while queue not empty:
    cur ← dequeue
    if cur already in tainted: continue      # visited-set guards against cycles
    insert cur into tainted
    for (next, _kind) in dfg_successors(cur): # every DFG edge kind
      enqueue next
  return tainted                              # a sink is tainted iff it is a member
```

In Rust over the real API:

```rust
use std::collections::{HashSet, VecDeque};
use libcpg::{CodePropertyGraph, NodeId};

/// Every node reachable from `source` along data-flow edges (the forward taint set).
fn taint_set(cpg: &CodePropertyGraph, source: NodeId) -> HashSet<NodeId> {
    let mut tainted = HashSet::new();
    let mut queue = VecDeque::new();
    queue.push_back(source);

    while let Some(cur) = queue.pop_front() {
        if !tainted.insert(cur) {
            continue;                                    // already visited
        }
        for (next, _kind) in cpg.dfg_successors(cur) {   // follow all DFG edges
            queue.push_back(next);
        }
    }
    tainted
}

/// True if untrusted data at `source` can flow to `sink`.
fn reaches_sink(cpg: &CodePropertyGraph, source: NodeId, sink: NodeId) -> bool {
    taint_set(cpg, source).contains(&sink)
}
```

This visits each node at most once and scans its data-flow out-edges once, so it
is `` $`O(V + E)`$ `` over the DFG. Two refinements are worth knowing:

- **Require an executable path.** Pure data-flow reachability does not check that
  a control-flow path also exists. Intersect the taint set with a
  [`cfg_reachable`](#cfg-traversal) test when you need both.
- **Use dependence, not raw flow, for precision.** After running
  [`PdgBuilder`](../builder/pdg-and-slicing.md), the
  [`DataDependence`](../../GLOSSARY.md#data-dependence) /
  [`ControlDependence`](../../GLOSSARY.md#control-dependence) edges support the
  bounded [`backward_slice`](../../GLOSSARY.md#backward-slice--forward-slice) /
  `forward_slice` traversals, which give dependence-accurate answers and a size
  bound. See [`theory/04-program-dependence-and-slicing.md`](../../theory/04-program-dependence-and-slicing.md).

## Positional queries

When you start from a source location rather than a node, the positional
accessors map byte offsets to nodes. Offsets are `u32` and match a node whose
`range` covers them; `node_at_offset` and `scope_at_offset` return the
*innermost* (smallest-span) match:

```rust
use libcpg::SourceRange;

if let Some(node) = cpg.node_at_offset(42) {
    println!("at offset 42: {:?}", node.kind);
}

// Innermost enclosing Block or Function.
if let Some(scope) = cpg.scope_at_offset(42) {
    println!("enclosing scope: {:?}", scope.kind);
}

// All nodes overlapping a byte range.
let overlapping = cpg.nodes_in_range(SourceRange::from_bytes(10, 50));
println!("{} nodes overlap [10, 50)", overlapping.len());
```

## Call-graph traversal

The call overlay connects call sites to callees. `call_sites(fn)` finds the
`Call` nodes inside a function; `callees(call)` and `callers(fn)` follow the
`CallSite` / `StaticCall` / `DynamicCall` edges:

```rust
for func in cpg.functions() {
    for call in cpg.call_sites(func.id) {        // Vec<NodeId>
        for callee in cpg.callees(call) {        // Vec<NodeId>
            let _ = callee;                       // resolved target(s)
        }
    }
}
```

## Subgraph extraction

Three methods carve a smaller `CodePropertyGraph` out of a larger one — handy
for scoping an analysis or a visualisation to a single function:

```rust
for func in cpg.functions() {
    let cfg = cpg.function_cfg(func.id);   // control-flow + expression nodes
    let dfg = cpg.function_dfg(func.id);   // nodes that carry data-flow edges
    println!(
        "{}: {} CFG nodes, {} DFG nodes",
        func.name().unwrap_or("?"),
        cfg.node_count(),
        dfg.node_count(),
    );
}
```

`subgraph(&[NodeId])` is the general form: it copies the given nodes and every
edge whose *both* endpoints are in the set, preserving ids so the result stays
comparable to the parent graph.

## Parallelism with rayon

`libcpg` links [rayon](https://docs.rs/rayon) as a dependency but its own
traversal accessors are **sequential** — they return owned `Vec`s or borrowing
iterators, and any graph mutation needs `&mut CodePropertyGraph`. Parallelism is
therefore a *read-only* concern on your side: because `&CodePropertyGraph` is
`Sync`, you can share one across rayon threads. The robust pattern is to collect
a work-list first, then fan out:

```rust
// Add `rayon` to your own Cargo.toml — libcpg links it but does not re-export it.
use rayon::prelude::*;
use libcpg::{CodePropertyGraph, NodeId};

fn per_function_complexity(cpg: &CodePropertyGraph) -> Vec<(NodeId, usize)> {
    // 1. Collect the work-list (cheap: NodeId is Copy).
    let functions: Vec<NodeId> = cpg.functions().map(|n| n.id).collect();

    // 2. Map in parallel; each closure only *reads* the shared &cpg.
    functions
        .par_iter()
        .map(|&f| (f, cpg.function_cfg(f).cyclomatic_complexity()))
        .collect()
}
```

Collecting node ids before parallelising avoids sharing a borrowing iterator
across threads and keeps every closure a pure read, which is exactly what the
`Send + Sync` guarantee on `CodePropertyGraph` permits.

## Where to go next

- [Nodes](nodes.md) — the node kinds you resolve ids into.
- [Edges](edges.md) — the edge kinds these accessors follow.
- [Overview](overview.md) — the CPG and its overlays at a glance.
- [`components/builder/pdg-and-slicing.md`](../builder/pdg-and-slicing.md) —
  dependence edges and bounded slicing for precise reachability.
- [`api/graph-reference.md`](../../api/graph-reference.md) — the complete
  method-by-method reference.
</content>
