# Querying and Traversal

A [`CodePropertyGraph`](../GLOSSARY.md#code-property-graph-cpg) is only as useful as the questions you can ask it. This guide is a cookbook of traversal recipes over the four overlays — [AST](../GLOSSARY.md#abstract-syntax-tree-ast), [CFG](../GLOSSARY.md#control-flow-graph-cfg), [DFG](../GLOSSARY.md#data-flow-graph-dfg), and the call graph — plus positional lookup, kind queries, the built-in metrics, and subgraph extraction. It finishes with a worked [taint](../GLOSSARY.md#taint-analysis)-style data-flow walk, the query shape that motivated CPGs in the first place [[1]](#references).

Every method here reads an immutable `&CodePropertyGraph`, so all of it is **feature-free** — it works on any CPG, however you built it. The examples assume you already have a `cpg` in hand (see [Building CPGs](01-building-cpgs.md)).

A note on return types, because they are consistent and easy to remember:

- Navigation that follows a single overlay returns an **owned `Vec`** (e.g. `ast_children -> Vec<NodeId>`, `cfg_successors -> Vec<(NodeId, CfgEdgeKind)>`). Owning the result ends the borrow immediately, so you can mutate the graph inside the loop if you need to.
- Whole-graph scans return **iterators** (`nodes()`, `functions()`, `nodes_by_kind(..)`).
- Lookups return **`Option`** (`node(id)`, `node_at_offset(off)`), never `Result`.

---

## Finding nodes by kind

The fastest way into a graph is to select the nodes you care about. libcpg provides shorthands for the common kinds and a general predicate filter, `nodes_by_kind`.

```rust
use libcpg::{CodePropertyGraph, CpgNodeKind};

fn survey(cpg: &CodePropertyGraph) {
    // Convenience iterators for the frequent kinds:
    let n_functions = cpg.functions().count();
    let n_classes = cpg.classes().count();
    let n_variables = cpg.variables().count();
    let n_calls = cpg.calls().count();
    println!("{n_functions} fns, {n_classes} classes, {n_variables} vars, {n_calls} calls");

    // The general form takes a predicate over `&CpgNodeKind`.
    // (Note: the method is `nodes_by_kind`, and it takes a predicate — not a kind value.)
    let ifs = cpg
        .nodes_by_kind(|k| matches!(k, CpgNodeKind::If))
        .count();
    println!("{ifs} if-statements");

    // `name()` returns the identifier for the kinds that carry one.
    for func in cpg.functions() {
        if let Some(name) = func.name() {
            println!("function: {name}");
        }
    }
}
```

`CpgNode` fields are public, so `func.kind`, `func.range`, and `func.id` are direct reads; `name()`, `is_declaration()`, `is_statement()`, `is_expression()`, `is_control_flow()`, and `is_error()` are convenience predicates. The 45 node kinds are catalogued in the [node reference](../components/graph/nodes.md).

---

## AST traversal

The AST is the base layer: every node is an AST node, and the other overlays add edges between the *same* nodes. Four methods walk it, all reconstructing true **source order** (AST child edges are sorted by insertion id, so `[condition, then, else]` on an `if` come back in that order).

```rust
use libcpg::{CodePropertyGraph, NodeId};

fn ast_walk(cpg: &CodePropertyGraph, node: NodeId) {
    let children = cpg.ast_children(node);        // Vec<NodeId>, source order
    let parent = cpg.ast_parent(node);            // Option<NodeId>
    let descendants = cpg.ast_descendants(node);  // Vec<NodeId>, depth-first
    let ancestors = cpg.ast_ancestors(node);      // Vec<NodeId>, towards root

    println!(
        "{} children, {} descendants, {} ancestors, parent = {:?}",
        children.len(), descendants.len(), ancestors.len(), parent,
    );
}
```

`ast_descendants` is the workhorse for "everything inside this function/class", and it underpins `call_sites`, `function_cfg`, and `function_dfg` below.

---

## CFG traversal

CFG edges encode possible execution order, each typed by a [`CfgEdgeKind`](../GLOSSARY.md#control-flow-graph-cfg) (14 variants: `Sequential`, `ConditionalTrue`, `ConditionalFalse`, `LoopBack`, …). The successor/predecessor methods return the neighbour **and** the edge kind, so you can distinguish the true branch from the false one.

```rust
use libcpg::{CfgEdgeKind, CodePropertyGraph, NodeId};

fn branches_of(cpg: &CodePropertyGraph, node: NodeId) {
    for (succ, kind) in cpg.cfg_successors(node) {
        match kind {
            CfgEdgeKind::ConditionalTrue => println!("true branch -> {succ:?}"),
            CfgEdgeKind::ConditionalFalse => println!("false branch -> {succ:?}"),
            other => println!("{other:?} -> {succ:?}"),
        }
    }

    // Predecessors carry the edge kind too.
    let _preds = cpg.cfg_predecessors(node);

    // Function entry/exit seeds recorded during construction:
    let _entries = cpg.cfg_entries(); // &[NodeId]
    let _exits = cpg.cfg_exits();     // &[NodeId]
}
```

To iterate only the nodes that participate in control flow, use `cfg_nodes()` (an iterator over `&CpgNode`). The [`basic block`](../GLOSSARY.md#basic-block) grouping and the per-construct edge semantics are covered in [CFG construction](../components/builder/cfg.md).

---

## DFG traversal

DFG edges track values from definitions to uses, typed by [`DfgEdgeKind`](../GLOSSARY.md#data-flow-graph-dfg) (13 variants: `DefUse`, `ReachingDef`, `Parameter`, `FieldRead`, …). Two named helpers express the classic queries:

```rust
use libcpg::{CodePropertyGraph, NodeId};

fn data_flow(cpg: &CodePropertyGraph, use_site: NodeId, definition: NodeId) {
    // Which definitions reach this use? (incoming DefUse / ReachingDef edges)
    let defs = cpg.reaching_definitions(use_site);   // Vec<NodeId>

    // Which uses does this definition flow to? (outgoing DefUse edges)
    let uses = cpg.uses_of_definition(definition);   // Vec<NodeId>

    // General neighbours, with the edge kind:
    let succ = cpg.dfg_successors(definition);        // Vec<(NodeId, DfgEdgeKind)>
    let pred = cpg.dfg_predecessors(use_site);        // Vec<(NodeId, DfgEdgeKind)>

    println!(
        "{} reaching defs, {} uses, {} dfg-succ, {} dfg-pred",
        defs.len(), uses.len(), succ.len(), pred.len(),
    );
}
```

libcpg's [reaching definitions](../GLOSSARY.md#reaching-definition) are computed by a single AST-ordered sweep, **not** [SSA](../GLOSSARY.md#static-single-assignment-ssa) and not a CFG fixed point; the rationale and the loop-double-sweep detail are in [DFG construction](../components/builder/dfg.md) and [ADR-0003](../design/0003-ast-ordered-reaching-defs.md).

---

## Call-graph traversal

The call overlay connects call sites to callees. Three methods cover it:

```rust
use libcpg::{CodePropertyGraph, NodeId};

fn calls(cpg: &CodePropertyGraph, function: NodeId, call_site: NodeId) {
    let sites = cpg.call_sites(function); // Vec<NodeId>: Call nodes inside `function`
    let callees = cpg.callees(call_site); // Vec<NodeId>: targets of a call site
    let callers = cpg.callers(function);  // Vec<NodeId>: call sites invoking `function`
    println!("{} sites, {} callees, {} callers", sites.len(), callees.len(), callers.len());
}
```

`call_sites` is a pure AST query (it filters `ast_descendants` for `Call` nodes), so it works even before any call edges are resolved; `callees`/`callers` follow `CallSite`/`StaticCall`/`DynamicCall` edges.

---

## Positional lookup

When you have a byte offset or range — say, from an editor cursor — three methods map position back to structure:

```rust
use libcpg::{CodePropertyGraph, SourceRange};

fn at_position(cpg: &CodePropertyGraph, offset: u32) {
    // The smallest node covering `offset`.
    if let Some(node) = cpg.node_at_offset(offset) {
        println!("innermost node kind: {:?}", node.kind);
    }
    // The innermost Block/Function enclosing `offset`.
    if let Some(scope) = cpg.scope_at_offset(offset) {
        println!("scope: {:?}", scope.kind);
    }
    // Every node overlapping a range.
    let overlapping = cpg.nodes_in_range(SourceRange::from_bytes(0, 32));
    println!("{} nodes overlap [0,32)", overlapping.len());
}
```

`SourceRange` carries six `u32` fields — `start`, `end`, `start_line`, `start_col`, `end_line`, `end_col` — and `from_bytes(start, end)` fills the byte offsets while zeroing the line/column fields.

---

## Raw edge access and filtering

For queries that cut across overlays, iterate edges directly. Each [`CpgEdge`](../components/graph/edges.md) exposes public `source`, `target`, `kind`, and `label` fields, and `CpgEdgeKind` offers classifier predicates — `is_ast`, `is_cfg`, `is_dfg`, `is_pdg`, `is_call`, `is_type`.

```rust
use libcpg::{CodePropertyGraph, NodeId};

fn edge_queries(cpg: &CodePropertyGraph, node: NodeId) {
    // Directed adjacency:
    let out = cpg.outgoing_edges(node).count();
    let inc = cpg.incoming_edges(node).count();

    // All DFG edges in the whole graph:
    let dfg_total = cpg.edges_by_kind(|k| k.is_dfg()).count();

    // Edges between a specific pair:
    let _between = cpg.edges_between(node, node); // Vec<&CpgEdge>

    println!("{out} out, {inc} in, {dfg_total} dfg edges total");
}
```

---

## Metrics

Two structural metrics are computed on demand:

```rust
use libcpg::CodePropertyGraph;

fn metrics(cpg: &CodePropertyGraph) {
    let depth = cpg.ast_depth();               // longest root-to-leaf AST path
    let complexity = cpg.cyclomatic_complexity();
    println!("ast depth {depth}, cyclomatic complexity {complexity}");
}
```

<a id="cyclomatic-complexity"></a>
`cyclomatic_complexity()` is McCabe's [metric](../GLOSSARY.md#cyclomatic-complexity), the number of linearly independent paths through the CFG:

```math
M = E - N + 2
```

where $`E`$ is the count of CFG edges and $`N`$ the count of CFG nodes (one connected component, one entry, one exit). An empty CFG yields `1`. The theory and worked examples are in [Control Flow and Complexity](../theory/02-control-flow-and-complexity.md); the full `stats()` bundle is described in [Getting Started](00-getting-started.md#step-4c--inspect-counts-and-statistics).

---

## Subgraph extraction

To hand a *slice of the graph* to another algorithm — or to serialise just one function — extract a subgraph. All three extractors return a fresh `CodePropertyGraph`, preserving node/edge ids via [`add_node_with_id`/`add_edge_with_id`](05-serialization.md#pattern-b-a-portable-nodeedge-snapshot).

```rust
use libcpg::{CodePropertyGraph, NodeId};

fn subgraphs(cpg: &CodePropertyGraph, function: NodeId, ids: &[NodeId]) {
    let arbitrary = cpg.subgraph(ids);          // exactly these nodes + edges among them
    let cfg_only = cpg.function_cfg(function);  // control-flow/expression nodes of a fn
    let dfg_only = cpg.function_dfg(function);  // nodes with any DFG edge in a fn
    println!(
        "{} / {} / {} nodes",
        arbitrary.node_count(), cfg_only.node_count(), dfg_only.node_count(),
    );
}
```

---

## Worked example: a taint-style data-flow walk

[Taint analysis](../GLOSSARY.md#taint-analysis) asks whether a value from an *untrusted source* can reach a *sensitive sink* without sanitisation. In a CPG this is a reachability query over DFG edges — precisely the cross-layer question Yamaguchi et al. introduced CPGs to answer [[1]](#references). Here is the reachability core, written feature-free with only the standard library:

```rust
use std::collections::{HashSet, VecDeque};
use libcpg::{CodePropertyGraph, NodeId};

/// Every node reachable from `source` by following DFG edges forward —
/// the taint frontier of `source`.
fn dfg_reachable(cpg: &CodePropertyGraph, source: NodeId) -> HashSet<NodeId> {
    let mut seen = HashSet::new();
    seen.insert(source);
    let mut queue = VecDeque::from([source]);
    while let Some(node) = queue.pop_front() {
        for (succ, _kind) in cpg.dfg_successors(node) {
            if seen.insert(succ) {
                queue.push_back(succ);
            }
        }
    }
    seen
}

/// Does tainted data from `source` reach `sink`?
fn taint_reaches(cpg: &CodePropertyGraph, source: NodeId, sink: NodeId) -> bool {
    dfg_reachable(cpg, source).contains(&sink)
}
```

To turn this into a real analysis you supply the two node sets: **sources** (e.g. `Call` nodes whose name is a request/read function, or `Parameter` nodes of a handler) and **sinks** (e.g. `Call` nodes whose name is a query/exec function). Select them with `nodes_by_kind` and `name()`, then run `taint_reaches` for each pair:

```rust
use libcpg::{CodePropertyGraph, CpgNodeKind};

fn find_flows(cpg: &CodePropertyGraph) {
    // Sources: calls named like input readers.
    let sources: Vec<_> = cpg
        .nodes_by_kind(|k| matches!(k, CpgNodeKind::Call { .. }))
        .filter(|n| n.name().is_some_and(|s| s.contains("read_input")))
        .map(|n| n.id)
        .collect();

    // Sinks: calls named like a dangerous execution primitive.
    let sinks: Vec<_> = cpg
        .nodes_by_kind(|k| matches!(k, CpgNodeKind::Call { .. }))
        .filter(|n| n.name().is_some_and(|s| s.contains("exec")))
        .map(|n| n.id)
        .collect();

    for &src in &sources {
        for &sink in &sinks {
            if taint_reaches(cpg, src, sink) {
                println!("tainted flow: {src:?} -> {sink:?}");
            }
        }
    }
}
```

For flows that must respect **both** control and data dependence (a stronger, dependence-aware notion of "affects"), build the [PDG](../GLOSSARY.md#program-dependence-graph-pdg) and take a [forward slice](../GLOSSARY.md#backward-slice--forward-slice) instead — see [Program Slicing](04-program-slicing.md). For very large graphs, the per-source walks are independent and parallelise cleanly with `rayon`.

---

## Next steps

- Detect design patterns over the graph you just queried: [Pattern Detection](03-pattern-detection.md).
- Turn "affects" into a precise slice: [Program Slicing](04-program-slicing.md).
- The complete method tables (construction, AST, CFG, DFG, call, query, metrics, subgraph) are in the [graph reference](../api/graph-reference.md); traversal internals are in [Graph Traversal](../components/graph/traversal.md).

---

## References

1. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and Discovering Vulnerabilities with Code Property Graphs.* 2014 IEEE Symposium on Security and Privacy. DOI: [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44)
</content>
