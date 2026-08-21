# Querying and Traversal

A [`CodePropertyGraph`](../GLOSSARY.md#code-property-graph-cpg) is only as useful as the questions you can ask it. This guide is a cookbook of traversal recipes over the four overlays — [AST](../GLOSSARY.md#abstract-syntax-tree-ast), [CFG](../GLOSSARY.md#control-flow-graph-cfg), [DFG](../GLOSSARY.md#data-flow-graph-dfg), and the call graph — plus positional lookup, kind queries, the built-in metrics, and subgraph extraction. It finishes with a worked [taint](../GLOSSARY.md#taint-catalog--taint-witness)-style data-flow walk, the query shape that motivated CPGs in the first place [[1]](#references).

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

To iterate only the nodes that participate in control flow, use `cfg_nodes()`
(an iterator over `&CpgNode`). For deterministic block membership, typed block
edges, and traversal orders, use the feature-free
[`block_cfg`](../components/graph/basic-block-cfg.md) view. Per-construct edge
semantics are covered in [CFG construction](../components/builder/cfg.md).

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

libcpg's [reaching definitions](../GLOSSARY.md#reaching-definition) are computed by a single AST-ordered sweep, **not** [SSA](../GLOSSARY.md#static-single-assignment-ssa) and not a CFG fixed point. Typed heap frames retain the sweep's continuation, so AST depth does not consume native call-stack frames. The rationale, stack discipline, and loop-double-sweep detail are in [DFG construction](../components/builder/dfg.md) and [ADR-0003](../design/0003-ast-ordered-reaching-defs.md).

### Finding resolved field writes

A member write has two complementary edges: `Assignment -> MemberAccess` as
`FieldWrite`, and — when `self`/`this` resolution is uniquely supported —
`MemberAccess -> Field` as `Reference`. The following feature-free query returns
their three node IDs:

```rust
use libcpg::{CodePropertyGraph, CpgEdgeKind, CpgNodeKind, DfgEdgeKind, NodeId};

fn resolved_field_writes(cpg: &CodePropertyGraph) -> Vec<(NodeId, NodeId, NodeId)> {
    let mut writes = Vec::new();
    for access in cpg.nodes().filter(|node| {
        matches!(node.kind, CpgNodeKind::MemberAccess { .. })
    }) {
        let assignment = cpg.incoming_edges(access.id).find_map(|edge| {
            matches!(
                edge.kind,
                CpgEdgeKind::DataFlow(DfgEdgeKind::FieldWrite)
            )
            .then_some(edge.source)
        });
        let field = cpg.outgoing_edges(access.id).find_map(|edge| {
            (matches!(edge.kind, CpgEdgeKind::Reference)
                && matches!(
                    cpg.node(edge.target).map(|node| &node.kind),
                    Some(CpgNodeKind::Field { .. })
                ))
            .then_some(edge.target)
        });
        if let (Some(assignment), Some(field)) = (assignment, field) {
            writes.push((assignment, access.id, field));
        }
    }
    writes.sort_unstable();
    writes
}
```

Keep unresolved accesses rather than treating them as evidence of no field:
arbitrary receivers, external types, aliases, and ambiguous fields deliberately
receive no `Reference`. See the [builder semantics](../components/builder/dfg.md#complex-l-values-and-declaration-resolution).

### Proving constants and dead branches

SCCP consumes the CFG and those `DefUse` edges together. It remains feature-free
once a CPG exists:

```rust
use libcpg::{sccp, unreachable_code, CodePropertyGraph, NodeId};

fn feasible_paths(cpg: &CodePropertyGraph, function: NodeId) -> libcpg::Result<()> {
    let constants = sccp(cpg, function)?;
    for (node, value) in &constants.constants {
        println!("proven {node:?} = {value:?}");
    }

    let plain = unreachable_code(cpg, function)?;
    let refined = plain.refine_with_sccp(&constants)?;
    for node in refined.unreachable {
        println!("unreachable after constant-path refinement: {node:?}");
    }
    Ok(())
}
```

The result is evidence, not a source rewrite. Unsupported operations, calls,
external values, and conflicting executable definitions remain unproven rather
than guessed. See the [SCCP component guide](../components/graph/sccp-analysis.md)
for the full lattice and edge semantics.

### Querying integer value ranges

`value_ranges` uses the same feature-free graph substrate but answers
non-singleton bound questions at basic-block entries:

```rust
use libcpg::{value_ranges, CodePropertyGraph, NodeId};

fn integer_bounds(cpg: &CodePropertyGraph, function: NodeId) -> libcpg::Result<()> {
    let report = value_ranges(cpg, function)?;
    for (leader, variables) in &report.at_block_entry {
        println!("entry {leader:?}: {variables:?}");
    }
    Ok(())
}
```

For identifier/literal comparisons, true and false edges refine the variable
to the matching half-line. Integer literal assignments and constant `+`/`-`
shifts remain precise; unsupported right-hand sides become the top interval.
Loop headers use widening and bounded narrowing, so callers receive a result or
an explicit iteration error rather than an unbounded computation. Reports are
immutable evidence, not source rewrites. Expression depth is held in typed heap
continuations; no thread-stack tuning or caller-selected depth cutoff is
required. A malformed cycle or inability to retain required machine state
becomes the top interval. See the [interval component
guide](../components/graph/interval-analysis.md) for the complete domain and
transfer contract.

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

[Taint analysis](../GLOSSARY.md#taint-catalog--taint-witness) asks whether a value from an *untrusted source* can reach a *sensitive sink* without sanitisation. In a CPG this is a reachability query over DFG edges — precisely the cross-layer question Yamaguchi et al. introduced CPGs to answer [[1]](#references). Here is the reachability core, written feature-free with only the standard library:

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

## Ranking a projection by centrality

When the question is comparative rather than reachability-based, project the
relevant relation and select a centrality definition. The following
feature-free example ranks resolved functions by directed PageRank:

```rust
use libcpg::{
    call_graph_projection, pagerank, CodePropertyGraph, PageRankOptions,
};

fn ranked_functions(cpg: &CodePropertyGraph) -> Vec<(libcpg::NodeId, f64)> {
    let graph = call_graph_projection(cpg);
    let result = pagerank(&graph, PageRankOptions::default());
    assert_eq!(result.nodes, graph.nodes);

    let mut ranked = result
        .nodes
        .into_iter()
        .zip(result.scores)
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .1
            .total_cmp(&left.1)
            .then_with(|| left.0.cmp(&right.0))
    });
    ranked
}
```

Keep `CentralityKind`, options, projection provenance, and `converged` with any
persisted ranking. Do not compare PageRank to closeness as though their scales
had the same meaning. See [projection centrality](../components/graph/centrality-analysis.md)
for the measure-selection table, formulas, budgets, and examples.

---

## Partitioning a projection into communities

When the question concerns cohesive groups rather than individual importance,
run deterministic multi-level Louvain on the relevant projection. This
feature-free example partitions the resolved call graph and then guarantees
that each returned group is internally connected:

```rust
use libcpg::{
    call_graph_projection, louvain, try_refine_connected, AnalysisError,
    CodePropertyGraph, CommunityAssignment, LouvainOptions,
};

fn function_communities(
    cpg: &CodePropertyGraph,
) -> Result<CommunityAssignment, AnalysisError> {
    let graph = call_graph_projection(cpg);
    let partition = louvain(&graph, LouvainOptions::default())?;
    let connected = try_refine_connected(&graph, &partition)?;
    assert_eq!(connected.nodes, graph.nodes);
    Ok(connected)
}
```

Persist the projection provenance, `CommunityKind`, and resolution alongside a
partition. Community identifiers are canonical labels, not durable subsystem
identities. See [community detection](../components/graph/community-detection.md)
for weight semantics, the modularity formula, work ceilings, and interpretation
limits.

---

## Inspecting cohesion and failure seams

When the question concerns structural persistence or fragmentation, compose
the feature-free cohesion functions over the relevant projection. This example
finds resilient call-graph blocks, single-edge seams, and a degree-first attack
trace:

```rust
use libcpg::{
    attack_simulation, call_graph_projection, cut_vertices_and_bridges,
    k_core_decomposition, two_edge_connected_components, AnalysisError,
    CodePropertyGraph,
};

fn inspect_call_robustness(cpg: &CodePropertyGraph) -> Result<(), AnalysisError> {
    let graph = call_graph_projection(cpg);
    let core = k_core_decomposition(&graph)?;
    let cuts = cut_vertices_and_bridges(&graph)?;
    let blocks = two_edge_connected_components(&graph)?;

    let mut removal_order = graph.nodes.clone();
    removal_order.sort_by_key(|node| {
        let dense = graph.dense_index(*node).expect("node came from projection") as usize;
        (std::cmp::Reverse(core.coreness[dense]), *node)
    });
    let attack = attack_simulation(&graph, &removal_order, removal_order.len())?;

    println!("{} bridges", cuts.bridges.len());
    println!("{} robust blocks", blocks.components.len());
    println!("degree-first robustness area: {:.3}", attack.auc);
    Ok(())
}
```

Persist the projection provenance and the exact removal order with an attack
trace. Compare minimum-cut weights only when weight semantics match. See
[cohesion and structural robustness](../components/graph/cohesion-analysis.md)
for the k-truss, minimum-cut, assortativity, cap, and interpretation contracts.

---

## Finding a weak spectral seam

Use algebraic connectivity when you need a graph-wide connectivity signal and
a candidate two-way seam. The checked API keeps malformed input distinct from
a projection with fewer than two nodes:

```rust
use libcpg::{
    call_graph_projection, try_algebraic_connectivity, AnalysisError,
    CodePropertyGraph, SpectralOptions,
};

fn inspect_call_spectrum(cpg: &CodePropertyGraph) -> Result<(), AnalysisError> {
    let graph = call_graph_projection(cpg);
    let Some(spectral) =
        try_algebraic_connectivity(&graph, &SpectralOptions::default())?
    else {
        println!("fewer than two projected functions");
        return Ok(());
    };

    println!("algebraic connectivity: {:.6}", spectral.algebraic_connectivity);
    println!("converged: {}", spectral.converged);
    println!("residual: {:.3e}", spectral.residual_norm);
    for (node, side) in spectral.nodes.iter().zip(&spectral.partition) {
        let side_number = if *side { 1 } else { 0 };
        println!("{node:?}: side {side_number}");
    }
    Ok(())
}
```

Persist projection provenance, options, convergence, residual, and the aligned
vector—not only the boolean partition. The sign split is an unnormalized
Fiedler heuristic and can be non-unique under repeated eigenvalues. See
[spectral connectivity and Fiedler partitioning](../components/graph/spectral-analysis.md)
for topology semantics and numerical limits.

---

## Enumerating bounded directed cycles

Use `simple_cycles` when you need concrete recursion, dependency, or control-
flow cycle witnesses rather than SCC membership alone:

```rust
use libcpg::{
    call_graph_projection, simple_cycles, AnalysisError, CodePropertyGraph,
    CycleOptions,
};

fn inspect_recursive_cycles(cpg: &CodePropertyGraph) -> Result<(), AnalysisError> {
    let graph = call_graph_projection(cpg);
    let cycles = simple_cycles(
        &graph,
        &CycleOptions {
            max_length: 8,
            max_cycles: 256,
        },
    )?;

    for cycle in &cycles.cycles {
        println!("recursive call cycle: {cycle:?}");
    }
    if let Some(first_omitted) = &cycles.first_dropped_cycle {
        println!("more cycles exist; first omitted: {first_omitted:?}");
    }
    Ok(())
}
```

Each sequence omits the repeated closing start. Do not interpret an empty
retained vector as acyclic unless `truncated` is false: with `max_cycles = 0`,
the first cycle appears only as `first_dropped_cycle`. For a complete, cheaper
acyclicity test, use `projection_sccs`; for concrete bounded witnesses, see
[bounded simple-cycle analysis](../components/graph/cycle-analysis.md).

---

## Counting local directed motifs

Use `motif_census` when you need a complete local-topology profile rather than
individual cycle witnesses:

```rust
use libcpg::{
    call_graph_projection, motif_census, AnalysisError, CodePropertyGraph,
    MotifOptions, TRIAD_NAMES,
};

fn inspect_call_motifs(cpg: &CodePropertyGraph) -> Result<(), AnalysisError> {
    let graph = call_graph_projection(cpg);
    let census = motif_census(&graph, &MotifOptions::default())?;
    for (name, count) in TRIAD_NAMES.iter().zip(census.triads) {
        if count > 0 {
            println!("{name}: {count}");
        }
    }
    println!("bidirected K4: {}", census.graphlets_4[0]);
    println!("out-star embeddings: {}", census.graphlets_4[1]);
    Ok(())
}
```

Persist projection provenance and the options with the counts. Motif labels
describe induced topology, not source intent, and the combined call requires
the projection to fit both the 512-node triad and 50-node graphlet budgets.
See the [directed motif census](../components/graph/motif-analysis.md) for the
full class table and interpretation boundaries.

---

## Measuring package stability

Use Martin metrics when a directed projection represents dependencies and you
can state the architectural module boundary explicitly:

```rust
use libcpg::{
    martin_metrics, module_graph_projection, sdp_violations,
    CodePropertyGraph, ModuleAssignment,
};

fn inspect_module_stability(cpg: &CodePropertyGraph) {
    let dependencies = module_graph_projection(cpg);
    let assignment = ModuleAssignment::by_module_nodes(cpg);
    let metrics = martin_metrics(cpg, &assignment, &dependencies);

    for metric in &metrics {
        println!(
            "{}: Ca={}, Ce={}, I={:.3}, A={:.3}, D={:.3}",
            metric.module, metric.ca, metric.ce, metric.instability,
            metric.abstractness, metric.distance,
        );
    }
    for violation in sdp_violations(&metrics, &dependencies, &assignment) {
        println!(
            "SDP: {} -> {} ({} dependency edges)",
            violation.source_module,
            violation.target_module,
            violation.dependency_count,
        );
    }
}
```

The projection direction must mean “source depends on target.” Persist that
meaning and the assignment policy with the output: changing either changes
afferent/efferent coupling and can reverse a Stable Dependencies Principle
verdict. See [Martin package metrics and SDP
analysis](../components/graph/martin-metrics.md) for custom file/service
assignments, exact formulae, and interpretation boundaries.

---

## Reviewing advisory architectural smells

Use the same dependency direction and module policy to combine SCC and Martin
evidence into four review-oriented smell families:

```rust
use libcpg::{
    architecture_smells, martin_metrics, module_graph_projection,
    projection_sccs, CodePropertyGraph, ModuleAssignment, SmellOptions,
};

fn inspect_architectural_smells(cpg: &CodePropertyGraph) {
    let dependencies = module_graph_projection(cpg);
    let assignment = ModuleAssignment::by_module_nodes(cpg);
    let sccs = projection_sccs(&dependencies);
    let martin = martin_metrics(cpg, &assignment, &dependencies);
    let findings = architecture_smells(
        &dependencies,
        &sccs,
        &martin,
        &assignment,
        &SmellOptions::default(),
    );

    for finding in findings {
        println!(
            "{:?}: severity={:.3}, nodes={:?}, evidence={:?}",
            finding.kind, finding.severity, finding.nodes, finding.evidence,
        );
    }
}
```

The inputs must describe one snapshot. Invalid CSR, stale SCCs, partial module
assignment, missing/duplicate Martin metrics, or invalid numerical options
produce no advisory verdict. A valid empty result means no threshold was
crossed, so validate externally if an application must distinguish those two
states. Persist projection/assignment policy and options with findings, and
never use a smell alone to authorize refactoring. See [architectural-smell
analysis](../components/graph/architecture-smells.md).

---

## Inferring structural architectural layers

Use layering inference when a directed graph represents dependencies and you
want bottom-first layers derived from topology rather than a declared path
policy:

```rust
use libcpg::{
    condensation_metrics, infer_layering, module_graph_projection,
    projection_sccs, CodePropertyGraph, CondensationOptions,
};

fn inspect_inferred_layers(cpg: &CodePropertyGraph) {
    let modules = module_graph_projection(cpg);
    let sccs = projection_sccs(&modules);
    let metrics = condensation_metrics(&sccs, &CondensationOptions::default());
    let report = infer_layering(&sccs, &metrics);

    if !report.complete {
        println!("layering unavailable; capped={}", report.capped);
        return;
    }
    for (layer, components) in report.layers.iter().enumerate().rev() {
        println!("layer {layer}: {components:?}");
    }
    for violation in report.violations {
        println!("{:#?}", violation);
    }
}
```

Every unflagged condensation edge descends exactly one layer. Cycle witnesses
identify a contracted SCC, skip witnesses bypass intervening layers, and upward
witnesses indicate a lateral/ascending edge in stale or foreign level data.
The module graph retains `Root`/`Module` vertices and `Imports` edges. See
[architectural layering inference](../components/graph/layering-analysis.md)
for validation, direction, and evidence boundaries.

---

## Comparing intended and observed layers

Use the reflexion model when your dependency producer already emits path
pairs and you want to compare them with an explicit layered policy:

```rust
use libcpg::{reflexion, LayerSpec, ReflexionRules};

fn inspect_architecture() {
    let rules = ReflexionRules {
        layers: vec![
            LayerSpec {
                name: "api".into(),
                prefixes: vec!["src/api/".into()],
            },
            LayerSpec {
                name: "domain".into(),
                prefixes: vec!["src/domain/".into()],
            },
        ],
        allow: vec![("api".into(), "domain".into())],
    };
    let result = reflexion(
        &rules,
        [
            ("src/api/http.rs".into(), "src/domain/model.rs".into()),
            ("src/domain/model.rs".into(), "src/api/http.rs".into()),
        ],
    );

    for violation in result.divergences {
        println!("{} -> {}", violation.src_path, violation.dst_path);
    }
    for (source, target) in rules.absences(&result.realized_pairs) {
        println!("declared but absent: {source} -> {target}");
    }
}
```

Normalize all paths into the same caller-defined namespace before comparing.
Layer order controls overlapping-prefix precedence, and dependency direction is
semantic. See [reflexion-model conformance](../components/graph/reflexion-model.md)
for complete edge categories, duplicate semantics, and security boundaries.

---

## Auditing class cohesion and function complexity

Use the feature-free metrics API after DFG extraction and optional call
resolution. The evidence graph makes a high LCOM value auditable instead of
leaving only a scalar:

```rust
use libcpg::{
    ck_metrics, function_metrics, method_field_graph, AnalysisError,
    CkOptions, CodePropertyGraph,
};

fn audit_metrics(cpg: &CodePropertyGraph) -> Result<(), AnalysisError> {
    for row in ck_metrics(cpg, &CkOptions::default()) {
        println!(
            "class {:?}: WMC={} CBO={} RFC={} LCOM4={}",
            row.class, row.wmc, row.cbo, row.rfc, row.lcom.lcom4,
        );
        for access in method_field_graph(cpg, row.class).accesses {
            println!(
                "  {:?} -> {:?} via {:?} (read={}, write={})",
                access.method, access.field, access.access,
                access.reads, access.writes,
            );
        }
    }

    for function in cpg.functions() {
        let row = function_metrics(cpg, function.id)?;
        println!(
            "function {:?}: cyclomatic={} cognitive={} MI={:.2}",
            row.function,
            row.cyclomatic_complexity,
            row.cognitive_complexity,
            row.maintainability_index,
        );
    }
    Ok(())
}
```

`CkOptions::default()` emits classes and structs. Enable `include_enums` only
when enum bodies are class-like in your measurement policy. Unresolved fields
and calls do not contribute guessed links, so retain the builder and resolution
configuration with any stored metric snapshot. See [CK, LCOM, and function
metrics](../components/graph/code-metrics.md) for formulae and evidence limits.

---

## Next steps

- Detect design patterns over the graph you just queried: [Pattern Detection](03-pattern-detection.md).
- Turn "affects" into a precise slice: [Program Slicing](04-program-slicing.md).
- Rank a projected relation: [Projection Centrality](../components/graph/centrality-analysis.md).
- Partition a projected relation: [Community Detection](../components/graph/community-detection.md).
- Inspect cohesive regions and failure seams: [Cohesion and Structural Robustness](../components/graph/cohesion-analysis.md).
- Find a global spectral seam: [Spectral Connectivity](../components/graph/spectral-analysis.md).
- Enumerate concrete recursion/dependency witnesses: [Bounded Simple Cycles](../components/graph/cycle-analysis.md).
- Profile local directed topology: [Directed Motif Census](../components/graph/motif-analysis.md).
- Measure architectural coupling and stable dependencies: [Martin Package Metrics](../components/graph/martin-metrics.md).
- Audit class cohesion and function complexity: [CK, LCOM, and Function Metrics](../components/graph/code-metrics.md).
- Review cycles, hubs, instability direction, and size outliers: [Architectural Smells](../components/graph/architecture-smells.md).
- Infer bottom-first layers and structural violations: [Architectural Layering](../components/graph/layering-analysis.md).
- Compare declared and observed dependency layers: [Reflexion-model Conformance](../components/graph/reflexion-model.md).
- The complete method tables (construction, AST, CFG, DFG, call, query, metrics, subgraph) are in the [graph reference](../api/graph-reference.md); traversal internals are in [Graph Traversal](../components/graph/traversal.md).

---

## References

1. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and Discovering Vulnerabilities with Code Property Graphs.* 2014 IEEE Symposium on Security and Privacy. DOI: [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44)
