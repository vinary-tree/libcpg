# Builder — Control-Flow Graph Extraction

The `CfgExtractor` is construction phase 3 (see [`overview.md`](overview.md)). It overlays
a [Control-Flow Graph](../../GLOSSARY.md#control-flow-graph-cfg) onto the AST nodes the
builder already created, by adding `ControlFlow(CfgEdgeKind)` edges that encode the
possible execution order between constructs. Because the edges share the AST's node set,
the resulting CFG is one of the four overlays of the unified CPG rather than a separate
graph.

This page covers the extractor's API and configuration, the per-construct handlers, the
loop/try context stacks, the 14 `CfgEdgeKind` variants, `BasicBlockIdentifier`, and the
[cyclomatic complexity](../../GLOSSARY.md#cyclomatic-complexity) metric the CFG makes
computable.

---

## API and configuration

`CfgExtractor` is a **struct**, not a trait. It carries a `CfgExtractorConfig` and exposes
`extract`, which mutates the CPG in place and returns `()` (it adds edges as a side
effect):

```rust
use libcpg::{CfgExtractor, CfgExtractorConfig};

// Default configuration.
let extractor = CfgExtractor::new();

// Or a custom one.
let config = CfgExtractorConfig {
    include_fallthrough: true,
    include_exceptions: true,
    include_call_edges: true,
};
let extractor = CfgExtractor::with_config(config);

// `extract` overlays ControlFlow edges onto every function in the CPG.
// (Given a `cpg` that already has AST nodes.)
// extractor.extract(&mut cpg);
```

`extract(&mut cpg)` finds all `Function` nodes via `cpg.functions()` and runs
`extract_function_cfg` on each. The `TreeSitterCpgBuilder` runs this for you when
`build_cfg` is enabled (the default); call it manually only when you built the AST some
other way.

### `CfgExtractorConfig`

| Field | Default | Effect |
|-------|---------|--------|
| `include_call_edges` | `true` | Emit a `Call` edge from a `Call` node to its resolved `target` (if any). |
| `include_exceptions` | `true` | Model `Try`/`Throw`/`Catch`/`Finally` flow; when `false`, `try` is treated as a plain sequence and `throw` as a leaf. |
| `include_fallthrough` | `true` | Declared for future implicit-fallthrough modelling; **not currently consulted** by the extractor. |

> **Honesty.** `include_fallthrough` is part of the config vocabulary but the shipped
> handlers do not read it — sequential fall-through is always modelled with `Sequential`
> edges. It is documented here as-is rather than implied to be wired.

---

## The last-child-is-body rule

`extract_function_cfg` establishes a function's CFG in three moves:

1. Mark the function node as a CFG **entry** (`cpg.add_cfg_entry`).
2. Treat the function's **last AST child** as its body, and connect the function to it with
   a `Sequential` edge.
3. Process the body recursively, then mark every returned exit node as a CFG **exit**
   (`cpg.add_cfg_exit`).

The "last child is the body" convention is deliberate and load-bearing: across grammars, a
function's signature/parameters/return-type precede its block in source order, so the block
is the final child. `cpg.ast_children` returns children in source order (it sorts by edge
id), which makes "last child" well defined. The Rholang and MeTTa mappers are written to
uphold the same convention so this single rule works for them too (see
[`node-mapper.md`](node-mapper.md)).

```text
extract_function_cfg(function):
    cpg.add_cfg_entry(function)
    body ← cpg.ast_children(function).last()        # the block
    if body exists:
        cpg.connect(function, body, ControlFlow(Sequential))
        exits ← process_node(body, ctx)             # ctx = fresh CfgContext
        for e in exits: cpg.add_cfg_exit(e)
```

---

## Per-construct handlers

`process_node` dispatches on the node's `CpgNodeKind` and returns the set of **exit
points** — the nodes from which control leaves the construct — as a `SmallVec<[NodeId; 4]>`
(kept on the stack for the common small-fan-out case). A construct wires its own internal
edges and hands its exits back to the caller, which chains them into the next statement.

![Control-flow constructs: how if/else, while, for, loop, match, and try are wired with ConditionalTrue/ConditionalFalse/LoopBack/Case edges.](../../diagrams/cfg-control-constructs.svg)

*Figure — the edge patterns each control construct emits. Source: [`diagrams/cfg-control-constructs.dot`](../../diagrams/cfg-control-constructs.dot).*

| `CpgNodeKind` | Handler | Edges emitted | Exits returned |
|---------------|---------|---------------|----------------|
| `Block { .. }` | `process_block` | `Sequential` between consecutive statements | last statement's exits (or the block if empty) |
| `If` | `process_if` | condition → then `ConditionalTrue`; condition → else `ConditionalFalse` | then-exits + else-exits (or the condition, when there is no `else`) |
| `While` | `process_while` | condition → body `ConditionalTrue`; body-exits → condition `LoopBack` | the condition (false branch) + break targets |
| `For` | `process_for` | header → body `ConditionalTrue`; body-exits → header `LoopBack` | the `for` node + break targets |
| `Loop` | `process_loop` | loop → body `Sequential`; body-exits → loop `LoopBack` | **break targets only** (an infinite loop exits only via `break`) |
| `Match` | `process_match` | source → each arm `Case` | union of arm-body exits |
| `Return` | `process_return` | return → value expr `Sequential` | the return node (a terminator) |
| `Break` | `process_break` | break → loop header `Break` | none (removed from normal flow) |
| `Continue` | `process_continue` | continue → continue target `Continue` | none |
| `Try` | `process_try` | body/catch → finally `Sequential`; (`throw` wires the `Throw` edges) | finally-exits, or body+catch exits |
| `Throw` | `process_throw` | throw → value `Sequential`; throw → each in-scope catch `Throw` | none |
| `Call { .. }` | `process_call` *(if `include_call_edges`)* | call → args `Sequential`; call → resolved `target` `Call` | the call node |
| *anything else* | `process_sequential` | node → first child `Sequential` | last child's exits |

### `if` — the shape of a branch

`process_if` reads the `if` node's children as `[condition, then, else?]`. It threads the
condition, forks `ConditionalTrue` to the `then` branch and `ConditionalFalse` to the
`else` branch, and — crucially — when there is **no** `else`, it returns the condition
node itself as an exit, so the "false, fall-through" path continues to the next statement.
An `Else` wrapper node is unwrapped to its inner block so the false edge lands on real code.

```text
process_if(if_node):
    [cond, then, else?] ← cpg.ast_children(if_node)
    connect(if_node, cond, Sequential)
    connect(cond, then, ConditionalTrue)
    exits ← process_node(then)
    if else exists:
        actual_else ← unwrap Else → inner block
        connect(cond, actual_else, ConditionalFalse)
        exits ← exits ∪ process_node(actual_else)
    else:
        exits ← exits ∪ { cond }        # false branch falls through
    return exits
```

### `block` — chaining and terminators

`process_block` connects the block entry to its first statement, then chains each
statement's exits into the next with `Sequential` edges. A **terminator**
(`Return`, `Break`, `Continue`, `Throw`) does not pass control to the following statement:
the block records its exits and stops threading (subsequent statements are still processed
for completeness but are unreachable).

---

## Loop and exception context: `CfgContext`

Some edges cannot be wired locally — a `break` deep inside a loop body must jump to *that*
loop's header, and a `throw` must reach the enclosing `try`'s catch handlers. `CfgContext`
carries this non-local information down the walk as two stacks:

- **`loop_stack`** of `LoopContext { loop_id, continue_target, break_targets }`. Each loop
  pushes a frame on entry (`push_loop`) and pops it on exit (`pop_loop`). `process_break`
  appends the break node to the innermost frame's `break_targets` and wires a `Break` edge
  to `loop_id`; `process_continue` wires a `Continue` edge to `continue_target`. On
  `pop_loop`, the collected `break_targets` become additional exits of the loop.
- **`try_stack`** of `TryContext { catch_handlers }`. `process_try` pushes the catch blocks
  before processing the try body; `process_throw` reads the innermost frame and wires a
  `Throw` edge from the throw to each catch handler.

This is a textbook use of an explicit stack to reconstruct lexical nesting during a single
pass — the same shape the DFG uses for its reaching environment
([`dfg.md`](dfg.md)) and the PDG for its post-dominator walk
([`pdg-and-slicing.md`](pdg-and-slicing.md)).

---

## The 14 `CfgEdgeKind` variants

Every control-flow edge is `CpgEdgeKind::ControlFlow(CfgEdgeKind)`. The `CfgEdgeKind`
enumeration is the full vocabulary of control transitions; the current `CfgExtractor`
emits nine of the fourteen, and the rest are reserved for richer producers or downstream
tools. Being explicit about which are emitted keeps the documentation honest.

| Variant | Meaning | Emitted by `CfgExtractor`? |
|---------|---------|:--:|
| `Sequential` | Ordinary fall-through to the next node | ✓ |
| `ConditionalTrue` | Branch taken when the condition holds | ✓ |
| `ConditionalFalse` | Branch taken when the condition fails | ✓ |
| `LoopBack` | Back edge from a loop body to its header | ✓ |
| `LoopExit` | Edge leaving a loop | — (loop exit is modelled by returning the guard as an exit) |
| `Break` | `break` to a loop header | ✓ |
| `Continue` | `continue` to a loop's continue target | ✓ |
| `Return` | Return transfer | — (`return` wires its value with `Sequential`; the node is marked a CFG exit) |
| `Throw` | Exception raise to a handler | ✓ |
| `Catch` | Entry into a catch handler | — (catch bodies are entered with `Sequential`) |
| `Call` | Call site to callee | ✓ (when `include_call_edges`) |
| `CallReturn` | Return from a callee to the call site | — |
| `Case` | Match/switch arm selection | ✓ |
| `DefaultCase` | Default/wildcard arm | — (all arms use `Case`) |

The classifier helpers `CfgEdgeKind::is_conditional()`
(`ConditionalTrue`/`ConditionalFalse`/`Case`/`DefaultCase`), `is_loop()`
(`LoopBack`/`LoopExit`/`Break`/`Continue`), and `is_exception()` (`Throw`/`Catch`) let
consumers filter edges by role; `BasicBlockIdentifier` uses the first two.

---

## Worked example

![CFG example: a small function's control-flow overlay with Sequential and conditional edges between statements.](../../diagrams/cfg-example.svg)

*Figure — the CFG overlay for a short function. Source: [`diagrams/cfg-example.dot`](../../diagrams/cfg-example.dot).*

```rust
// requires: features = ["lang-rust"]
use libcpg::{CpgBuilder, TreeSitterCpgBuilder, Language};

let src = "fn classify(n: i32) -> i32 { if n > 0 { 1 } else { -1 } }";
let cpg = TreeSitterCpgBuilder::new().build(src, Language::Rust)?;

// The `if` produces a fork: its condition has both a true and a false successor.
let func = cpg.functions().next().expect("one function");
for (succ, kind) in cpg.cfg_successors(func) {
    // `kind` is a CfgEdgeKind; the first hop is Sequential into the body.
    let _ = (succ, kind);
}
# Ok::<(), libcpg::Error>(())
```

`cpg.cfg_successors(id)` returns `Vec<(NodeId, CfgEdgeKind)>` and `cpg.cfg_predecessors(id)`
the incoming counterpart, so the overlay is navigable in both directions. See
[`../graph/traversal.md`](../graph/traversal.md) for the full navigation surface.

---

## Basic blocks

A [basic block](../../GLOSSARY.md#basic-block) is a maximal straight-line run of statements
with a single entry and single exit. `BasicBlockIdentifier` groups a function's CFG nodes
into blocks keyed by their **leader** (the first node of each block):

```rust
use libcpg::BasicBlockIdentifier;

// Given a `cpg` whose CFG has been extracted and a `function: NodeId`:
// let blocks = BasicBlockIdentifier::new().identify(&cpg, function);
// `blocks` maps each leader NodeId to the Vec<NodeId> of nodes in its block.
```

`identify(&cpg, function) -> FxHashMap<NodeId, Vec<NodeId>>` computes leaders as: the
function entry; any node reached by a conditional or loop edge
(`edge_kind.is_conditional() || edge_kind.is_loop()`); and every successor of a branching
node (`If`/`While`/`For`/`Loop`/`Match`/`Return`/`Break`/`Continue`). It then extends each
leader forward while there is exactly one `Sequential` successor that is not itself a
leader. libcpg operates at AST-node granularity rather than compacting blocks into single
nodes, so this identifier is a *view* over the CFG, not a rewrite of it.

---

## Cyclomatic complexity

McCabe's cyclomatic complexity [[1]](#references) counts the linearly independent paths
through a function's CFG. `CodePropertyGraph::cyclomatic_complexity()` computes it directly
from the overlay this extractor produced:

```math
M = E - N + 2
```

where $`E`$ is the number of CFG edges and $`N`$ the number of CFG nodes, for a single
connected component with one entry and one exit (McCabe's general form is
$`M = E - N + 2P`$ for $`P`$ components; libcpg fixes $`P = 1`$ per function). The
implementation counts edges with `kind.is_cfg()` and nodes with an incident CFG edge, then
returns $`E - N + 2`$ (saturating the subtraction, and returning $`1`$ for a CFG with
no nodes):

```rust
// requires: features = ["lang-rust"]
use libcpg::{CpgBuilder, TreeSitterCpgBuilder, Language};

let src = "fn m(n: i32) { if n > 0 { a(); } else { b(); } while n > 0 { c(); } }";
let cpg = TreeSitterCpgBuilder::new().build(src, Language::Rust)?;
let m = cpg.cyclomatic_complexity(); // usize
assert!(m >= 1);
# Ok::<(), libcpg::Error>(())
```

Each branch and loop the handlers above introduce adds an edge without adding a
proportional node, raising $`M`$ — which is exactly why the metric tracks the number of
decisions in the function. The theory behind the metric, worked derivations, and its
relation to test-path counts live in
[`../../theory/02-control-flow-and-complexity.md`](../../theory/02-control-flow-and-complexity.md).

---

## Re-running the extractor

`extract` is meant to run **once** per build, as construction phase 3. The CFG entry/exit
registries deduplicate (`add_cfg_entry`/`add_cfg_exit` ignore repeats), but the control-flow
*edges* are emitted via `cpg.connect`, which appends an edge unconditionally; re-running
`extract` on the same graph would therefore add parallel `ControlFlow` edges. Run it once
on a freshly built AST (which is what `TreeSitterCpgBuilder` does), or clear the CFG first
if you must re-extract. (By contrast the [DFG def-use sweep](dfg.md#idempotency) and the
[PDG builder](pdg-and-slicing.md) deduplicate explicitly and are safe to re-run.)

---

## See also

- [`overview.md`](overview.md) — where CFG extraction sits in the pipeline.
- [`dfg.md`](dfg.md) — the next phase, which reads no CFG edges but shares the node set.
- [`pdg-and-slicing.md`](pdg-and-slicing.md) — how the CFG's edges and recorded exits feed
  post-dominator and control-dependence computation.
- [`../graph/edges.md`](../graph/edges.md) — the full `CpgEdgeKind` / `CfgEdgeKind` tables.
- [`../../api/builder-reference.md`](../../api/builder-reference.md) — `CfgExtractor`,
  `CfgExtractorConfig`, and `BasicBlockIdentifier` signatures.
- [`../../theory/02-control-flow-and-complexity.md`](../../theory/02-control-flow-and-complexity.md)
  — CFG theory and cyclomatic complexity.

---

## References

1. McCabe, T. J. (1976). *A Complexity Measure.* IEEE Transactions on Software Engineering SE-2(4). DOI: [10.1109/TSE.1976.233837](https://doi.org/10.1109/TSE.1976.233837)
