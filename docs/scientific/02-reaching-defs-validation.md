# Reaching-definitions validation

The [Data Flow Graph](../GLOSSARY.md#data-flow-graph-dfg) is the semantic heart of a [Code Property Graph](../GLOSSARY.md#code-property-graph-cpg): it is the overlay that answers "which definition produced the value read here?" Every taint query, slice, and data-dependence edge rests on it being *correct*. This page validates `libcpg`'s [reaching-definitions](../GLOSSARY.md#reaching-definition) analysis against a corpus of five hand-audited scenarios, each parsed from real Rust and each mapped to a specific inline test with its deciding assertion quoted.

## 0. The theory backdrop, and what libcpg does differently

Classically, reaching definitions is a **forward "may" data-flow analysis**: over the [CFG](../GLOSSARY.md#control-flow-graph-cfg), each program point $`n`$ has an in-set and an out-set of definitions related by the transfer function

```math
\mathrm{out}(n) = \mathrm{gen}(n) \cup \bigl(\mathrm{in}(n) \setminus \mathrm{kill}(n)\bigr),
\qquad
\mathrm{in}(n) = \bigcup_{p \,\in\, \mathrm{pred}(n)} \mathrm{out}(p),
```

iterated over the powerset [lattice](../GLOSSARY.md#lattice-data-flow) of definitions to a fixed point (Kildall [[5]](#references); the standard formulation is Aho, Lam, Sethi & Ullman [[13]](#references)). Processing a definition of a variable $`x`$ [**gens**](../GLOSSARY.md#kill--gen-data-flow) the new definition and [**kills**](../GLOSSARY.md#kill--gen-data-flow) prior ones.

`libcpg` deliberately does **not** run that CFG fixed point, and it does **not** use [SSA](../GLOSSARY.md#static-single-assignment-ssa). It uses [**AST-ordered reaching definitions**](../GLOSSARY.md#ast-ordered-reaching-definitions): a single flow-sensitive sweep over AST nodes *in source order*, maintaining a `ReachingEnv` that maps each variable name to the definitions currently reaching it. It applies a [strong update](../GLOSSARY.md#strong-update--weak-update) (kill + gen) in straight-line context, a weak update (gen only) inside a conditional region, and sweeps loop bodies twice for loop-carried dependencies. The design rationale is in [`design/0003-ast-ordered-reaching-defs.md`](../design/0003-ast-ordered-reaching-defs.md) and the mechanism in [`components/builder/dfg.md`](../components/builder/dfg.md).

The scientific question is therefore sharp: **does the AST-ordered sweep produce the same answers the textbook analysis would, on cases that distinguish a correct analysis from a plausible-but-wrong one?** The corpus below is chosen so that each scenario fails under a specific, realistic bug.

![How a reaching definition flows from a binding to a nested use](../diagrams/def-use-example.svg)

*Figure — a definition reaches a use through a def-use edge; the analysis must thread the definition into uses that appear deep inside expression trees. Source: [`diagrams/def-use-example.dot`](../diagrams/def-use-example.dot).*

## 1. The test harness

The corpus lives in `src/builder/dfg.rs` behind `#[cfg(feature = "lang-rust")]`, so every scenario is parsed by the real grammar and built through `build` (Mode A) — and, by the equivalence proved in [01](01-cpg-invariants-and-equivalence.md#4-the-equivalence-theorem-build--build_from_tree), the results transfer to the Mode-B path too. Two helpers make the assertions precise:

```rust
#[cfg(feature = "lang-rust")]
fn build_rust(src: &str) -> CodePropertyGraph {
    use crate::{CpgBuilder, TreeSitterCpgBuilder};
    TreeSitterCpgBuilder::new()
        .build(src, Language::Rust)
        .expect("parse + build should succeed")
}

/// The identifier *use* of `name`: an `Identifier` node that actually
/// received a reaching definition (i.e. it is not the binder of `name`).
#[cfg(feature = "lang-rust")]
fn ident_use(cpg: &CodePropertyGraph, name: &str) -> NodeId {
    nodes_where(cpg, |k| {
        matches!(k, CpgNodeKind::Identifier { name: n, .. } if &**n == name)
    })
    .into_iter()
    .find(|&id| !cpg.reaching_definitions(id).is_empty())
    .unwrap_or_else(|| panic!("no reaching-def use of `{name}` (bug: DFG empty?)"))
}
```

`ident_use` is itself a validation lever: it selects the identifier whose [`reaching_definitions`](../GLOSSARY.md#def-use-chain--definition--use) set is *non-empty*. If the sweep failed to wire any definition to a use, `ident_use` would panic ("bug: DFG empty?") before a single assertion ran — so the harness cannot silently pass on an empty DFG.

The five scenarios and their tests:

| # | Property under test | Test (`src/builder/dfg.rs`) | Source fixture |
|---|---|---|---|
| 1 | Nested-expression use resolution + chaining | `parsed_nested_use_resolves_and_chains` | `let buf = read(fd); let out = decode(buf); sink(out);` |
| 2 | Latest definition (strong update) | `parsed_reassignment_uses_latest_definition` | `let mut x = a; x = b; use_it(x);` |
| 3 | Non-flow of unrelated variables | `parsed_unrelated_variable_does_not_flow` | `let x = mk(); let y = mk(); use_x(x); use_y(y);` |
| 4 | Shadowing scope | `parsed_shadowing_and_idempotent` | `let n = n + 1; consume(n);` |
| 5 | Idempotency | `parsed_shadowing_and_idempotent` | *(same fixture)* |

## 2. Scenario 1 — nested-expression use resolution and chaining (the P7c property)

**Hypothesis.** A use that appears *inside an expression* — an AST `Identifier` that is **not** itself a CFG node, e.g. `buf` inside the call argument `decode(buf)` — must still resolve to its binding, and the resulting def-use edges must chain: `buf → out → sink`.

This is the property fixed by the P7c work (threading reaching-defs down to AST-expression identifier uses); before it, a purely CFG-node-level analysis would leave nested argument identifiers with no reaching definition and the DFG would be empty on ordinary code.

**Experiment.** `parsed_nested_use_resolves_and_chains` first insists the parsed function has real data flow at all, then checks the nested resolution and the chain:

```rust
let cpg = build_rust(
    "fn foo(fd: i32) { let buf = read(fd); let out = decode(buf); sink(out); }",
);

// The whole point: a normally-parsed function now has DFG edges.
assert!(
    cpg.stats().dfg_edges >= 3,
    "parsed code must have def-use edges; got {}",
    cpg.stats().dfg_edges
);

let buf_def = var_named(&cpg, "buf");
let out_def = var_named(&cpg, "out");
let buf_use = ident_use(&cpg, "buf"); // the `buf` inside decode(buf)
let out_use = ident_use(&cpg, "out"); // the `out` inside sink(out)

// reaching_definitions of the nested `buf` use returns the `let buf` def.
assert!(
    cpg.reaching_definitions(buf_use).contains(&buf_def),
    "reaching defs of the nested `buf` use must include `let buf`"
);
// dfg_successors(let buf) reaches the `buf` use.
assert!(
    cpg.dfg_successors(buf_def).iter().any(|(t, _)| *t == buf_use),
    "dfg_successors(let buf) must reach the `buf` use"
);
// The `buf` use lives inside `let out`, and `out`'s use resolves to `let out`.
assert!(
    cpg.ast_descendants(out_def).contains(&buf_use),
    "the `buf` use lives inside the `let out` statement"
);
assert!(
    cpg.reaching_definitions(out_use).contains(&out_def),
    "reaching defs of the sink argument `out` must include `let out`"
);
```

**Result.** The nested `buf` resolves to `let buf` in *both* directions (`reaching_definitions` backward and `dfg_successors` forward), the `buf` use is confirmed to sit inside the `let out` statement (invariant [I2](01-cpg-invariants-and-equivalence.md#2-invariant-i2--ast-childparent-consistency)), and `out` resolves to `let out` — establishing the full `buf → out → sink` data path. The `dfg_edges >= 3` floor guarantees this is genuine flow, not an artifact. Hypothesis corroborated.

## 3. Scenario 2 — latest definition wins (strong update)

**Hypothesis.** After a reassignment in straight-line code, a use sees the **latest** definition and *not* the killed earlier one; flow-sensitivity means exactly one definition reaches.

**Experiment.** `parsed_reassignment_uses_latest_definition`:

```rust
let cpg = build_rust("fn g(a: i32, b: i32) { let mut x = a; x = b; use_it(x); }");

let let_x = var_named(&cpg, "x");
let assign_x = /* the single Assignment `x = b` */;
let x_use = ident_use(&cpg, "x");

let reaching = cpg.reaching_definitions(x_use);
assert!(
    reaching.contains(&assign_x),
    "the use must see the latest def `x = b`"
);
assert!(
    !reaching.contains(&let_x),
    "the use must NOT see the killed def `let mut x = a`"
);
assert_eq!(
    reaching.len(),
    1,
    "straight-line code: exactly one reaching def"
);
```

**Result.** The use of `x` reaches `x = b` and *not* `let mut x = a`, and the reaching set has cardinality exactly one. This is the [strong update](../GLOSSARY.md#strong-update--weak-update) contract — the second definition [killed](../GLOSSARY.md#kill--gen-data-flow) the first — and it is precisely the answer the textbook fixed point gives on a straight-line block. A flow-*insensitive* analysis would report both definitions (cardinality 2) and fail the last two assertions. Hypothesis corroborated.

## 4. Scenario 3 — unrelated variables do not flow

**Hypothesis.** Definitions of one variable never reach uses of a *different* variable; the environment keys by name and does not leak across names.

**Experiment.** `parsed_unrelated_variable_does_not_flow`:

```rust
let cpg = build_rust("fn f() { let x = mk(); let y = mk(); use_x(x); use_y(y); }");

let x_def = var_named(&cpg, "x");
let y_def = var_named(&cpg, "y");
let x_use = ident_use(&cpg, "x");
let y_use = ident_use(&cpg, "y");

assert!(cpg.reaching_definitions(x_use).contains(&x_def));
assert!(cpg.reaching_definitions(y_use).contains(&y_def));
assert!(
    !cpg.reaching_definitions(x_use).contains(&y_def),
    "`y` must not flow to `x`'s use"
);
assert!(
    !cpg.reaching_definitions(y_use).contains(&x_def),
    "`x` must not flow to `y`'s use"
);
assert!(
    !cpg.dfg_successors(x_def).iter().any(|(t, _)| *t == y_use),
    "`x`'s def must not reach `y`'s use"
);
```

**Result.** Each use resolves to its own definition, and the four cross-checks confirm *no* spurious edge in either direction (checked both from the use via `reaching_definitions` and from the def via `dfg_successors`). This is the analysis's *precision* floor: it validates that the DFG is not simply linking every definition to every later identifier. Hypothesis corroborated.

## 5. Scenario 4 — shadowing sees the right scope

**Hypothesis.** In `let n = n + 1`, the `n` on the right-hand side is the **initializer** use: it must resolve to the *outer* definition (here the parameter `n`), because the new binding is not in scope until after its own initializer. A later `consume(n)` must instead see the *shadowing* `let n`.

**Experiment.** `parsed_shadowing_and_idempotent` (first half):

```rust
let mut cpg = build_rust("fn f(n: i32) { let n = n + 1; consume(n); }");

// Two distinct definitions of `n`: the parameter and the shadowing let.
let param_n = /* the Parameter node */;
let let_n = var_named(&cpg, "n");

// The `n` in `n + 1` is the shadowing initializer use: it must resolve
// to the parameter (the pre-binding definition), not to the `let n`.
let init_use = /* the `n` whose reaching defs contain param_n */;
assert!(!cpg.reaching_definitions(init_use).contains(&let_n));

// `consume(n)` sees the shadowing `let n`.
let consume_use = /* the `n` whose reaching defs contain let_n */;
assert!(consume_use != init_use);
```

**Result.** The initializer `n` binds to the parameter and explicitly *not* to `let n`, while `consume(n)` binds to `let n`, and the two uses are distinct nodes. The sweep respects the subtlety that a binding's own initializer is evaluated in the *enclosing* scope — an ordering guarantee the AST-ordered sweep gets from visiting the initializer expression before installing the new binding. Hypothesis corroborated.

## 6. Scenario 5 — idempotency of the sweep

**Hypothesis.** Re-running the extractor produces no new edges — the same [idempotency](../GLOSSARY.md#idempotent) invariant [I3](01-cpg-invariants-and-equivalence.md#3-invariant-i3--idempotent-extractors) validated structurally, here confirmed on real parsed code.

**Experiment.** `parsed_shadowing_and_idempotent` (second half):

```rust
// Idempotency: a second extraction pass must not add any edge.
let before = cpg.stats().dfg_edges;
DfgExtractor::new().extract(&mut cpg);
assert_eq!(before, cpg.stats().dfg_edges, "extract must be idempotent");
```

**Result.** The DFG edge count is identical before and after a second sweep — the extractor's snapshot-and-skip guard holds on a graph that already carries shadowing and nested-use edges. Hypothesis corroborated.

## 7. Coverage, and an honest boundary

The sweep's flow-sensitive machinery is illustrated end to end below.

![The AST-ordered reaching-definitions sweep maintaining ReachingEnv](../diagrams/reaching-defs-sweep.svg)

*Figure — the single source-order sweep threading `ReachingEnv` through bindings, reassignments, nested expressions, and shadowing scopes. Source: [`diagrams/reaching-defs-sweep.puml`](../diagrams/reaching-defs-sweep.puml).*

What the corpus establishes: nested-expression resolution and chaining (§2), strong update / latest-definition in straight-line code (§3), name-scoped non-flow (§4), scope-correct shadowing (§5), and idempotency (§6).

What it does **not** directly assert: the **loop-carried** double-sweep is a documented mechanism of the extractor (a loop body is swept twice so a definition at the bottom of the loop reaches a use at the top), but this corpus does not contain a dedicated loop-back regression; the flow-sensitivity evidence here is the straight-line strong update of §3 plus the scope handling of §5. The [weak update](../GLOSSARY.md#strong-update--weak-update) inside conditional regions is likewise a mechanism described in [`components/builder/dfg.md`](../components/builder/dfg.md) and [`theory/03-data-flow-and-reaching-definitions.md`](../theory/03-data-flow-and-reaching-definitions.md) rather than a line in this five-scenario corpus. Stating these boundaries is part of the ledger: the validated claims are exactly the six above, no more.

## References

5. Kildall, G. A. (1973). *A Unified Approach to Global Program Optimization.* POPL '73. DOI: [10.1145/512927.512945](https://doi.org/10.1145/512927.512945)
13. Aho, A. V., Lam, M. S., Sethi, R., Ullman, J. D. (2006). *Compilers: Principles, Techniques, and Tools* (2nd ed.). Addison-Wesley. ISBN 978-0321486813 (no DOI). *(Reaching definitions, data-flow analysis.)*
