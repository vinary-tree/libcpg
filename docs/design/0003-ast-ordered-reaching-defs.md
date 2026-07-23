# 0003 — AST-ordered reaching definitions for the DFG

## Status

**Accepted.** Realised in `src/builder/dfg.rs` (`DfgExtractor::build_def_use_edges`,
the `visit_reaching` walk, `ReachingEnv`, and `bind_definition`). Two earlier
CFG-based approaches are retained in the same file, compiled out under
`#[cfg(any())]`, as an executable record of what was superseded. Pinned by four
inline `#[cfg(feature = "lang-rust")]` tests (the *P7c* prerequisite).

## Context

The [Data Flow Graph](../GLOSSARY.md#data-flow-graph-dfg) overlay must connect
every [definition](../GLOSSARY.md#def-use-chain--definition--use) of a variable
to the [uses](../GLOSSARY.md#def-use-chain--definition--use) it
[reaches](../GLOSSARY.md#reaching-definition). The dominant case in real code is a
use nested deep inside an expression: the `buf` in `decode(buf)`, the `x` in
`return f(x)`. In `libcpg` those uses are AST `Identifier` nodes — they are *not*
control-flow nodes.

The two textbook ways to compute reaching definitions both assume a control-flow
graph fine-grained enough to carry a definition to each such use:

- **Classic iterative data-flow** (Kildall's fixed-point framework [2]; the
  gen/kill reaching-definitions instance in the Dragon book [3]) iterates
  $`\mathit{IN}[n] = \bigcup_{p \in \mathit{pred}(n)} \mathit{OUT}[p]`$ and
  $`\mathit{OUT}[n] = \mathit{gen}[n] \cup (\mathit{IN}[n] \setminus \mathit{kill}[n])`$
  over CFG nodes until nothing changes.
- **[Static Single Assignment](../GLOSSARY.md#static-single-assignment-ssa)**
  (Cytron et al. [1]) renames each definition uniquely and inserts
  $`\phi`$-functions at control-flow joins, after which def-use is a direct
  lookup.

Both are keyed on the CFG. But `libcpg`'s [CFG](../GLOSSARY.md#control-flow-graph-cfg)
for *parsed* code is deliberately coarse: it wires statements and control
constructs, not the interior of every expression. That coarseness broke the
first two implementations, which are still in the file under `#[cfg(any())]`:

1. `compute_reaching_definitions` — an iterative gen/kill fixpoint over the
   `ControlFlow` edges, keyed **only by CFG nodes**. On parsed code it produced
   no usable def-use edges, for two compounding reasons its doc-comment records
   verbatim: (a) nested-expression identifier uses are AST nodes that are not
   CFG nodes, so `reaching_defs.get(&use_site)` returned `None`; and (b) even for
   uses that *do* carry a CFG edge, the coarse CFG for straight-line
   `let`/assignment chains does not carry a definition from one statement to the
   next, so the reaching set was empty anyway.
2. `build_def_use_edges_cfg` — consumed those CFG-keyed reaching sets and keyed
   emission by `use_site`; with nested uses absent from the map, it emitted zero
   edges on parsed code.

The consequence was stark: a normally-parsed function had an **empty DFG**. That
is the problem this record fixes — the *P7c prerequisite*: parsed code must have
a correct, non-empty data-flow overlay, with reaching definitions threaded all
the way down to nested-expression uses.

## Decision

**Build the DFG from a single, AST-ordered, flow-sensitive reaching-definitions
sweep** — [AST-ordered reaching definitions](../GLOSSARY.md#ast-ordered-reaching-definitions) —
that abstract-interprets each function body in **source (execution) order** over
the AST, threading a per-name environment of currently-reaching definitions down
into nested expression uses. It is **not** SSA and **not** a CFG fixed point.

### The environment

```rust
// A per-variable set of currently-reaching definition NODES, keyed by name.
// The on-stack SmallVec keeps the common single-definition case allocation-free.
type ReachingEnv = FxHashMap<Arc<str>, SmallVec<[NodeId; 2]>>;
```

`ReachingEnv` is a map into the powerset [lattice](../GLOSSARY.md#lattice-data-flow)
of definition nodes: each name maps to the set of definitions live *at the walk's
current position*.

### The sweep

The walk `visit_reaching(node, env, conditional, pairs)` recurses over AST
children in order, updating `env` in place and pushing `(def, use)` pairs.
Expressed as literate pseudocode:

```text
visit(node, env, conditional, pairs):
  1. kind ← kind(node); if the node is gone, return.
  2. dispatch on kind:

     Variable{name}  (a `let` / local declaration):
        for each child EXCEPT the binder identifier:
            visit(child, env, conditional, pairs)      # initializer sees the PRE-binding env
        bind(env, name, node, conditional)             # then the name is (re)bound

     Parameter{name}:
        like Variable — the identifier child is the binder (a def, not a use);
        visit any remaining children (e.g. a default value), then bind(name).

     Assignment{operator}:
        compound ← operator != "="                     # `+=` reads before it writes
        target  ← first child (the l-value)
        for each child:
            if child is the simple-identifier target AND not compound:
                skip it                                # plain `x = …`: target is written, not read
            else visit(child, env, conditional, pairs)
        if target is a simple identifier: bind(env, targetName, node, conditional)

     Identifier{name}  (a USE):
        for each def in env[name]: emit pair (def, node)

     otherwise (blocks, calls, arguments, control flow, …):
        child_conditional ← conditional OR is_conditional_region(kind)
        passes ← 2 if is_loop_region(kind) else 1      # loop bodies swept twice
        repeat `passes` times:
            for each child: visit(child, env, child_conditional, pairs)

bind(env, name, def, conditional):
    if conditional:  add def to env[name]              # WEAK update: no kill
    else:            replace env[name] with just {def} # STRONG update: kill + gen
```

Four design points make this correct where the CFG approach failed:

- **Uses are reached because the environment is threaded, not looked up.** When
  the walk reaches the `buf` `Identifier` inside `decode(buf)`, `env["buf"]`
  already holds the `let buf` definition — no CFG node needed.
- **[Strong vs weak update](../GLOSSARY.md#strong-update--weak-update).** In
  straight-line context a definition performs a strong update (kill prior defs of
  the name, gen the new one — *the latest write wins*, giving flow-sensitivity).
  Inside a [conditional region](../GLOSSARY.md#kill--gen-data-flow) (an `If`,
  `Else`, `While`, `For`, `Loop`, `Match`, `MatchArm`, `Try`, `Catch`,
  `Finally`) it performs a weak update — add without kill — so a write on one
  branch cannot erase a write live on a sibling path. Weak update is a *sound
  over-approximation*: it may add an extra def-use edge, never drop a real one.
- **Initializer before rebind.** A definition's right-hand side is evaluated
  before the name is (re)bound, so a shadowing use in the initializer
  (`let x = x + 1`, or the parameter-shadowing `let n = n + 1`) resolves to the
  *pre-existing* definition.
- **Loop bodies swept twice.** A bounded two-pass fixpoint lets a use near the
  top of a loop body observe a definition made lower in the same body
  (loop-carried dependence) without an unbounded iteration count.

### Emission and idempotency

The sweep collects `(def, use)` pairs while borrowing the graph immutably, then
emits `DataFlow(DfgEdgeKind::DefUse)` edges — but only for pairs that are not
already present. Re-running the extractor is therefore a no-op:

```rust
// feature-free: the extractor runs on ANY CodePropertyGraph; build() also calls it.
// `cpg` here is a `let mut cpg: CodePropertyGraph` you constructed earlier.
use libcpg::{DfgExtractor, DfgExtractorConfig};

let mut extractor = DfgExtractor::new();                 // defaults below
// Or tune it: field access / parameter / return-value edges, alias tracking off.
extractor = DfgExtractor::with_config(DfgExtractorConfig {
    include_field_access: true,
    include_parameters: true,
    include_return_values: true,
    track_aliases: false,      // more complex, off by default
    max_iterations: 100,       // bounded the RETIRED fixpoint; the sweep uses a 2-pass loop
});
extractor.extract(&mut cpg);   // idempotent: a second call adds no edges
extractor.extract(&mut cpg);   // no-op
```

Querying is feature-free. After extraction (or after `build`, which runs it),
`cpg.reaching_definitions(use_node)` returns the definitions reaching a use and
`cpg.dfg_successors(def_node)` returns the uses a definition reaches:

```rust
// requires: features = ["lang-rust"]  (to parse; the DFG queries are feature-free)
use libcpg::{CpgBuilder, TreeSitterCpgBuilder, Language};

let cpg = TreeSitterCpgBuilder::new().build(
    "fn foo(fd: i32) { let buf = read(fd); let out = decode(buf); sink(out); }",
    Language::Rust,
)?;
// The nested `buf` inside decode(buf) resolves to `let buf`, and the whole
// buf → out → sink-argument data path exists — exactly what the CFG path missed.
```

## Consequences

### Positive

- **Parsed code has a real DFG.** Nested-expression uses resolve; the empty-DFG
  failure of the CFG approach is gone. The test
  `parsed_nested_use_resolves_and_chains` asserts a parsed function has
  `dfg_edges >= 3` and that `buf → out → sink` chains end to end.
- **Flow-sensitive in straight-line code.** `parsed_reassignment_uses_latest_definition`
  pins that after `let mut x = a; x = b; use_it(x)` the use sees *only* `x = b`
  (the killed `let mut x = a` does not reach) — exactly one reaching def.
- **No spurious cross-variable flow.** `parsed_unrelated_variable_does_not_flow`
  pins that `x` and `y` never leak into each other's uses.
- **Correct shadowing.** `parsed_shadowing_and_idempotent` pins that the `n` in
  `let n = n + 1` binds to the *parameter* while `consume(n)` binds to the
  shadowing `let n`.
- **Simple, language-agnostic, and cheap.** The sweep dispatches on
  [`CpgNodeKind`](../GLOSSARY.md#node-kind--edge-kind), never on grammar
  specifics, so it works for every frontend including the Mode-B F1R3FLY
  languages ([ADR-0002](0002-mode-b-build-from-tree.md)). It runs in time linear
  in the number of AST nodes, $`O(n)`$, up to a small constant from the
  two-pass loop bodies — no global fixed-point iteration.
- **Idempotent.** Re-running `extract` adds nothing, so construction stages
  compose safely (see [idempotent](../GLOSSARY.md#idempotent)).

### Negative

- **Intraprocedural and heuristic, not a proof.** The sweep does not model
  interprocedural flow, pointer aliasing (`track_aliases` is off by default), or
  precise branch conditions. Its guarantees are the ones the tests state, not
  soundness-and-completeness in the dataflow-theory sense.
- **Weak update over-approximates.** Inside branches and loops the *add-without-kill*
  policy can attach a def-use edge for a definition that a more precise analysis
  would rule out on a given path — extra edges, never missing ones, but callers
  doing precision-sensitive work should know the bias.
- **No $`\phi`$-node disambiguation.** Because there is no SSA renaming, a
  use at a merge point can carry several reaching definitions with no explicit
  join node distinguishing them; the DFG records all of them.
- **Two dead code paths carried in the source.** The retired
  `compute_reaching_definitions` and `build_def_use_edges_cfg` remain under
  `#[cfg(any())]`. They cost reading weight (and must not bit-rot into
  compiling), but they document *why* the CFG approach was abandoned so it is not
  re-attempted — a deliberate trade of tidiness for institutional memory.

## Alternatives considered

1. **Classic CFG-keyed iterative reaching definitions** (Kildall/Dragon-book
   gen/kill fixpoint [2][3]) — the retired `compute_reaching_definitions`.
   *Rejected/retired.* On `libcpg`'s coarse parsed CFG it produced an empty DFG:
   nested uses are not CFG nodes, and straight-line `let` chains do not propagate
   a def across the CFG. Making it work would have required a much finer CFG
   (an edge into every sub-expression), inflating the control overlay for a
   result the AST already orders for free.

2. **Full SSA construction** (Cytron et al. [1]). *Rejected.* SSA buys precise,
   $`\phi`$-mediated joins, but at the cost of dominance-frontier
   $`\phi`$-placement and a renaming pass — real machinery for a library
   whose DFG is a *query substrate*, not an optimizer IR. The AST-ordered sweep
   delivers the def-use edges callers need at a fraction of the complexity;
   SSA remains defined in the glossary precisely to mark this contrast.

3. **Resolve def-use purely from the parser's scope/identifier resolution**
   (the `Identifier { definition: Option<NodeId> }` back-pointer). *Rejected as
   insufficient alone.* Scope resolution binds a *name* to its declaration but
   does not model *flow* — it cannot express that `use_it(x)` sees `x = b` rather
   than `let mut x = a`, nor loop-carried dependence. Flow-sensitivity requires
   the ordered sweep.

4. **Delete the failed CFG approaches outright.** *Rejected.* Removing them would
   erase the recorded reason the CFG path fails on parsed code, inviting a future
   contributor to reintroduce it. Keeping them compiled-out (`#[cfg(any())]`)
   with explanatory doc-comments preserves that reasoning at near-zero runtime
   cost.

![The AST-ordered reaching-definitions sweep threading an environment down into nested expression uses](../diagrams/reaching-defs-sweep.svg)

*Figure — the sweep in execution order: `let`/assignment nodes update `ReachingEnv` (strong update in straight-line code, weak update in branch/loop bodies) and each identifier use links to every currently-reaching definition. Source: [`diagrams/reaching-defs-sweep.puml`](../diagrams/reaching-defs-sweep.puml).*

## Related decisions and further reading

- The theory: data-flow lattices, gen/kill, and why the sweep replaces the
  fixed point — [`../theory/03-data-flow-and-reaching-definitions.md`](../theory/03-data-flow-and-reaching-definitions.md).
- The extractor's full behaviour, config, and the retired paths:
  [`../components/builder/dfg.md`](../components/builder/dfg.md).
- The graph substrate the edges land on:
  [ADR-0001](0001-unified-overlay-graph.md).
- The test corpus mapped to each invariant:
  [`../scientific/02-reaching-defs-validation.md`](../scientific/02-reaching-defs-validation.md).

## References

1. Cytron, R., Ferrante, J., Rosen, B. K., Wegman, M. N., Zadeck, F. K. (1991). *Efficiently Computing Static Single Assignment Form and the Control Dependence Graph.* ACM TOPLAS 13(4). DOI: [10.1145/115372.115320](https://doi.org/10.1145/115372.115320)
2. Kildall, G. A. (1973). *A Unified Approach to Global Program Optimization.* POPL '73. DOI: [10.1145/512927.512945](https://doi.org/10.1145/512927.512945)
3. Aho, A. V., Lam, M. S., Sethi, R., Ullman, J. D. (2006). *Compilers: Principles, Techniques, and Tools* (2nd ed.). Addison-Wesley. ISBN 978-0321486813 (no DOI). *(Reaching definitions, data-flow analysis.)*
