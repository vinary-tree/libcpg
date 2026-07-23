# Builder — Data-Flow Graph Extraction

The `DfgExtractor` is construction phase 4 (see [`overview.md`](overview.md)). It overlays
a [Data-Flow Graph](../../GLOSSARY.md#data-flow-graph-dfg) onto the shared AST nodes by
adding `DataFlow(DfgEdgeKind)` edges that connect **definitions** of a variable to the
**uses** they reach. The engine behind it is an
[AST-ordered reaching-definitions](../../GLOSSARY.md#ast-ordered-reaching-definitions)
sweep — a single flow-sensitive pass over AST nodes *in source order*.

> **What this is not.** libcpg's DFG is **not** [SSA](../../GLOSSARY.md#static-single-assignment-ssa)
> and **not** a classic CFG fixed-point propagation. It is an abstract interpretation of the
> AST in execution order. Two earlier CFG-based approaches were tried and are kept in the
> source, compiled out under `#[cfg(any())]`, as an executable record; see
> [Retired approaches](#retired-approaches-cfgany) below and
> [`../../design/0003-ast-ordered-reaching-defs.md`](../../design/0003-ast-ordered-reaching-defs.md).

---

## API and configuration

Like the [`CfgExtractor`](cfg.md), `DfgExtractor` is a **struct** whose `extract` mutates
the CPG in place and returns `()`:

```rust
use libcpg::{DfgExtractor, DfgExtractorConfig};

let extractor = DfgExtractor::new();

// Or a custom configuration.
let config = DfgExtractorConfig {
    include_field_access: true,
    include_parameters: true,
    include_return_values: true,
    track_aliases: false,
    max_iterations: 100,
};
let extractor = DfgExtractor::with_config(config);
// extractor.extract(&mut cpg);   // given a cpg with AST nodes
```

`extract(&mut cpg)` iterates `cpg.functions()` and runs `extract_function_dfg` on each. The
`TreeSitterCpgBuilder` runs it as phase 4 when `build_dfg` is enabled (the default).

### `DfgExtractorConfig`

| Field | Default | Effect |
|-------|---------|--------|
| `include_parameters` | `true` | Emit `Parameter` edges from call arguments to the matching parameters, and link parameter uses. |
| `include_return_values` | `true` | Emit `ReturnValue` edges from returned expressions to the callers. |
| `include_field_access` | `true` | Emit `FieldRead` / `IndexRead` / `DataDependency` edges for member and index access. |
| `track_aliases` | `false` | Reserved for alias tracking; off by default (more expensive, not yet modelled). |
| `max_iterations` | `100` | Iteration cap for the **retired** CFG fixed-point; **not consulted** by the current AST-ordered sweep (which bounds loops at two passes — see below). |

> **Honesty.** `max_iterations` configured the now-retired CFG fixed-point
> (`compute_reaching_definitions`, compiled out). The live sweep uses a fixed double-pass
> for loop bodies instead of iterating to `max_iterations`, so the field is documented
> as-is rather than implied to govern the current algorithm.

---

## What `extract_function_dfg` does

For each function the extractor runs five steps:

1. **Collect auxiliary sites.** A `DefUseCollector` records field-access, index-access, and
   parameter nodes (used by steps 3–5). Def-use *edges* are no longer driven from its
   name→site tables.
2. **AST-ordered def-use sweep.** `build_def_use_edges` performs the reaching-definitions
   walk and emits `DefUse` edges — including to identifier uses nested deep inside
   expressions.
3. **Parameter edges** (if `include_parameters`).
4. **Return-value edges** (if `include_return_values`).
5. **Field/index edges** (if `include_field_access`).

Step 2 is the heart of the module; the rest overlay auxiliary edge kinds.

---

## The reaching environment

The sweep threads one mutable value through the walk: the **reaching environment**, a map
from each in-scope variable **name** to the set of definition nodes currently reaching it.

```rust
use std::sync::Arc;
use smallvec::SmallVec;
use rustc_hash::FxHashMap;
use libcpg::NodeId;

// Per-name set of currently-reaching definition nodes.
// The inline capacity keeps the common single-definition (straight-line) case
// allocation-free; multiple entries appear only after a branch or loop merge.
type ReachingEnv = FxHashMap<Arc<str>, SmallVec<[NodeId; 2]>>;
```

Formally this is a map into the powerset [lattice](../../GLOSSARY.md#lattice-data-flow) of
definitions — the same object classic data-flow analysis iterates over (Kildall
[[1]](#references); Aho et al. [[2]](#references)) — but here it is updated by a single
ordered traversal rather than a worklist to a fixed point.

---

## Strong vs. weak updates (gen/kill)

Processing a definition of a variable $`x`$ at node $`d`$ performs a
[gen/kill](../../GLOSSARY.md#kill--gen-data-flow) update on `env[x]`. Which kind depends on
whether the definition sits in **straight-line** or **conditional** context:

**Strong update** (straight-line) kills prior definitions of $`x`$ and gens the new one —
the latest write wins:

```math
\mathrm{env}[x] \leftarrow \{\, d \,\}
```

**Weak update** (inside a conditional region — a branch or loop body) adds the new
definition *without* killing, because a write on one path must not erase a write on another:

```math
\mathrm{env}[x] \leftarrow \mathrm{env}[x] \cup \{\, d \,\}
```

The weak update is a sound over-approximation: it can only add edges a stricter analysis
would drop, never remove one it should keep. `bind_definition` implements exactly this — a
`clear()`-then-`push` for the strong case, a contains-guarded `push` for the weak case. See
[strong/weak update](../../GLOSSARY.md#strong-update--weak-update).

A node's children count as conditional when the node is one of `If`, `Else`, `While`,
`For`, `Loop`, `Match`, `MatchArm`, `Try`, `Catch`, or `Finally` (`is_conditional_region`);
once conditional, the flag stays set for all descendants.

---

## The core walk: `visit_reaching`

`visit_reaching(cpg, node, env, conditional, pairs)` recurses over the AST in source order,
mutating `env` and appending `(def, use)` pairs. Here it is as literate pseudocode,
faithful to the implementation:

```text
visit_reaching(node, env, conditional, pairs):
    kind ← cpg.node(node).kind

    match kind:

      # `let x = init` — evaluate the initializer FIRST (its uses see the
      # pre-binding env), then bind the name. The binder identifier (the `x`
      # itself) is a definition site, not a use, and is skipped.
      Variable { name }:
          binder ← binder_identifier(node, name)
          for child in cpg.ast_children(node):
              if child ≠ binder: visit_reaching(child, env, conditional, pairs)
          bind_definition(env, name, node, conditional)

      # A parameter binds its name; any non-binder child (e.g. a default value)
      # is visited as a use.
      Parameter { name }:
          binder ← binder_identifier(node, name)
          for child in cpg.ast_children(node):
              if child ≠ binder: visit_reaching(child, env, conditional, pairs)
          bind_definition(env, name, node, conditional)

      # Assignment. The first child is the target l-value.
      #  - simple `x = …`      : target is a pure WRITE → skip it as a use
      #  - compound `x += …`   : target is read-then-written → visit it
      #  - complex `a.f = …`   : target is not a bare identifier → visit it
      Assignment { operator }:
          compound ← (operator ≠ "=")
          [target, …] ← cpg.ast_children(node)
          target_is_ident ← target is an Identifier
          for child in cpg.ast_children(node):
              if child = target and target_is_ident and not compound: continue
              visit_reaching(child, env, conditional, pairs)
          if target_is_ident:
              bind_definition(env, name_of(target), node, conditional)

      # Identifier in USE position: link every currently-reaching definition of
      # its name to this node. Binder identifiers never reach here — their
      # parent construct skipped them above.
      Identifier { name }:
          for def in env.get(name):
              pairs.push( (def, node) )

      # Everything else: recurse in source order. Branch/loop bodies switch to
      # conditional (weak-update) context; loop bodies are swept TWICE.
      _:
          child_conditional ← conditional or is_conditional_region(kind)
          passes ← 2 if is_loop_region(kind) else 1
          repeat passes times:
              for child in cpg.ast_children(node): 
                  visit_reaching(child, env, child_conditional, pairs)
```

`build_def_use_edges` seeds this by binding the function's parameters (they precede the
body in AST child order) and then sweeping each child, finally emitting the deduplicated
`pairs` as `DefUse` edges.

### Why the ordering matters

Evaluating a definition's initializer *before* rebinding the name is what makes
`let x = x + 1` and `x = x + 1` correct: the `x` on the right sees the *pre-existing*
definition, then the statement installs the new one. The binder-skipping logic
(`binder_identifier` finds the first `Identifier` child whose text equals the declared
name) is what prevents the declared name from linking to itself as a spurious self-use;
because a pattern precedes its initializer in source order, a shadowing use of the same
name still comes *after* the binder and is visited normally.

---

## Loop double-sweep

A use near the **top** of a loop body can be reached by a definition made **lower** in the
same body on a previous iteration (a loop-carried dependence). A single forward pass would
miss it. `visit_reaching` therefore sweeps loop-region bodies (`While`, `For`, `Loop`)
**twice** — a bounded, two-iteration fixed point — so the second pass observes the
definitions the first pass installed.

![Reaching-definitions sweep: the AST-ordered walk maintains a per-name reaching set, applying strong updates in straight-line code, weak updates in branches, and a double pass over loop bodies.](../../diagrams/reaching-defs-sweep.svg)

*Figure — the flow-sensitive sweep: strong/weak updates and the loop double-pass. Source: [`diagrams/reaching-defs-sweep.puml`](../../diagrams/reaching-defs-sweep.puml).*

**Cost.** The sweep visits each node once outside loops; a node nested inside $`d`$ loop
regions is visited $`2^d`$ times, so the total work is $`O(n \cdot 2^d)`$ for $`n`$
AST nodes and maximum loop-nesting depth $`d`$ — and $`d`$ is a tiny constant in real
code. There is no worklist and no global fixed point.

---

## The P7c nested-use fix

The single most important property of this sweep is that it threads a statement's reaching
definitions **all the way down into the nested identifier uses inside it** — the ordinary
case that the earlier CFG-based approaches missed. Consider:

```rust
// requires: features = ["lang-rust"]
use libcpg::{CpgBuilder, TreeSitterCpgBuilder, Language};

let src = "fn foo(fd: i32) { let buf = read(fd); let out = decode(buf); sink(out); }";
let cpg = TreeSitterCpgBuilder::new().build(src, Language::Rust)?;

// The `buf` inside `decode(buf)` is an AST Identifier nested in a call
// argument — not a CFG node — yet it resolves to the `let buf` definition,
// and dfg_successors(let buf) reaches that use.
assert!(cpg.stats().dfg_edges >= 3);  // fd→read, buf→decode, out→sink
# Ok::<(), libcpg::Error>(())
```

`cpg.reaching_definitions(use_site)` returns the definitions reaching a use (the sources of
its incoming `DefUse`/`ReachingDef` edges), and `cpg.dfg_successors(def)` returns the uses a
definition reaches. The behaviour above — nested-use resolution, latest-definition on
reassignment, non-flow of unrelated variables, and shadowing — is pinned by the inline
tests `parsed_nested_use_resolves_and_chains`, `parsed_reassignment_uses_latest_definition`,
`parsed_unrelated_variable_does_not_flow`, and `parsed_shadowing_and_idempotent` in
`src/builder/dfg.rs` (see
[`../../scientific/02-reaching-defs-validation.md`](../../scientific/02-reaching-defs-validation.md)).

---

## Auxiliary edges

Beyond the `DefUse` edges from the sweep, `extract_function_dfg` overlays four more
`DfgEdgeKind`s, each gated by its config flag:

| Edge kind | Source → target | Emitted by | Gate |
|-----------|-----------------|-----------|------|
| `Parameter` | call argument → matching parameter | `build_parameter_edges` | `include_parameters` |
| `ReturnValue` | returned expression → each caller | `build_return_edges` | `include_return_values` |
| `FieldRead` | accessed object → the member-access node | `build_field_access_edges` | `include_field_access` |
| `IndexRead` | indexed array → the index-access node | `build_field_access_edges` | `include_field_access` |
| `DataDependency` | index expression → the index-access node | `build_field_access_edges` | `include_field_access` |

`build_parameter_edges` also adds a supplementary `DefUse` edge from a directly-declared
parameter to each of its uses, deduplicated against the sweep's edges so it never
double-links.

---

## Def-use chains

The overlay can be projected into per-variable [def-use chains](../../GLOSSARY.md#def-use-chain--definition--use).
The public building blocks are:

- `Definition { variable, node, kind }` with `DefinitionKind` one of `Declaration`,
  `Assignment`, `Parameter`, `FieldWrite`, `IndexWrite`.
- `Use { variable, node, kind }` with `UseKind` one of `Read`, `FieldRead`, `IndexRead`,
  `Argument`.
- `DefUseChain { variable, definitions, uses, def_to_uses, use_to_defs }`, with
  `add_definition`, `add_use`, `link`, `uses_of(def)`, and `definitions_of(use)`.
- `build_def_use_chains(&cpg, function) -> FxHashMap<Arc<str>, DefUseChain>`, which folds
  the function's `DefUse` edges into one chain per variable name.

```rust
// requires: features = ["lang-rust"]
use libcpg::{CpgBuilder, TreeSitterCpgBuilder, build_def_use_chains, Language};

let src = "fn f() { let x = mk(); use_x(x); use_x(x); }";
let cpg = TreeSitterCpgBuilder::new().build(src, Language::Rust)?;
let func = cpg.functions().next().expect("one function");

let chains = build_def_use_chains(&cpg, func);
if let Some(chain) = chains.get("x") {
    // Every use `x` reached by the `let x` definition.
    for def in &chain.definitions {
        let _uses = chain.uses_of(*def);
    }
}
# Ok::<(), libcpg::Error>(())
```

![Def-use example: a definition node linked by DefUse edges to each identifier use it reaches.](../../diagrams/def-use-example.svg)

*Figure — a definition and the uses it reaches. Source: [`diagrams/def-use-example.dot`](../../diagrams/def-use-example.dot).*

---

## Idempotency

The def-use sweep is **idempotent**. `build_def_use_edges` snapshots the existing `DefUse`
edges into a set keyed by `(source, target)` and skips any pair already present, so
re-running `extract` adds no duplicate def-use edge; the parameter-use supplement
deduplicates the same way. The inline test `parsed_shadowing_and_idempotent` runs `extract`
twice and asserts `cpg.stats().dfg_edges` is unchanged. (The auxiliary
`ReturnValue`/`FieldRead` builders emit via `cpg.connect` without their own dedup, so as
with the [CFG](cfg.md#re-running-the-extractor) the intended usage is a single extraction
pass during construction.)

---

## Retired approaches (`#[cfg(any())]`)

Two earlier CFG-based strategies are retained in `src/builder/dfg.rs`, compiled out via
`#[cfg(any())]` (an always-false `cfg`), as an executable record of what was tried and why
it was replaced:

- **`compute_reaching_definitions`** — an iterative dataflow fixed point over `ControlFlow`
  edges, keyed **only by CFG nodes**, bounded by `max_iterations`.
- **`build_def_use_edges_cfg`** — emitted `DefUse` edges keyed by `use_site` from that
  fixed point.

Both produced *no usable edges on parsed code*, for two compounding reasons: (1)
nested-expression identifier uses (the common case, `buf` in `decode(buf)`) are AST
`Identifier` nodes that are **not** CFG nodes, so the reaching set lookup returned nothing;
and (2) even for uses that do carry a CFG edge, the coarse parsed CFG for straight-line
`let`/assignment chains does not carry a definition from one statement to the next. The
AST-ordered sweep abstract-interprets the AST in execution order instead, sidestepping both
problems. The full rationale and trade-offs are recorded in
[`../../design/0003-ast-ordered-reaching-defs.md`](../../design/0003-ast-ordered-reaching-defs.md).

---

## See also

- [`overview.md`](overview.md) — where DFG extraction sits in the pipeline.
- [`cfg.md`](cfg.md) — the preceding phase (the DFG shares its node set).
- [`pdg-and-slicing.md`](pdg-and-slicing.md) — how `DefUse`/`ReachingDef` edges are
  re-projected as `DataDependence` for slicing.
- [`../graph/edges.md`](../graph/edges.md) — the full `DfgEdgeKind` table.
- [`../../api/builder-reference.md`](../../api/builder-reference.md) — `DfgExtractor`,
  `DfgExtractorConfig`, `Definition`/`Use`/`DefUseChain`, and `build_def_use_chains`.
- [`../../theory/03-data-flow-and-reaching-definitions.md`](../../theory/03-data-flow-and-reaching-definitions.md)
  — data-flow lattices, fixed points, and why libcpg uses an AST-ordered sweep.

---

## References

1. Kildall, G. A. (1973). *A Unified Approach to Global Program Optimization.* POPL '73. DOI: [10.1145/512927.512945](https://doi.org/10.1145/512927.512945)
2. Aho, A. V., Lam, M. S., Sethi, R., Ullman, J. D. (2006). *Compilers: Principles, Techniques, and Tools* (2nd ed.). Addison-Wesley. ISBN 978-0321486813 (no DOI). *(Reaching definitions, data-flow analysis.)*
