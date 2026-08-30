# Dovetail extraction and optimization evidence ledger

This ledger records libcpg-owned evidence for pgmcp campaign
`dovetail-extraction-optimization`. The pgmcp work-item tree is authoritative
for requirements, dependencies, ownership, and live status; this document maps
implemented repository artifacts to those requirements without duplicating the
tracker as an alternative source of truth.

## Repository boundary

The campaign separates three responsibilities:

1. libcpg owns source identity, typed program facts, semantic profiles, and
   rewrite admission;
2. Replete is a standalone generic e-graph, saturation, and extraction crate;
   and
3. adapters compose the two without making either repository depend on the
   other application's internals.

MeTTaIL is outside the campaign's write boundary. Its source, manifests,
generated files, worktrees, branches, commits, and tracker state must not be
modified. An immutable revision may be read as a provenance oracle for a
standalone extraction only. pgmcp leaf E3 remains pending and unclaimed until
the user grants separate permission.

## T4.1 -- typed pure-expression boundary

Tracker leaf `dovetail-x-t4-1` defines a language-neutral, flat expression IR
and explicit frontend semantic profiles. Its invariants and evidence are:

| Obligation | Durable evidence |
|---|---|
| A rewrite requires complete, pure, live evidence and a named law's precondition | [`ExpressionIr.tla`](../../formal/tla/ExpressionIr.tla), [`ExpressionIr.v`](../../formal/rocq/ExpressionIr.v), [`expression_ir.smt2`](../../formal/z3/expression_ir.smt2) |
| Persisted structure cannot recreate rewrite authority | formal models above; static negative deserialization assertion and generated serde tests |
| Invalid integer descriptors, shifts, overflow, exceptional floats, and signed zero fail closed | Rocq/Z3 numeric obligations and independent evaluator/property tests |
| Logical depth never becomes native call-stack depth | explicit `Enter`/`Exit` machine and the 100,000-level 256 KiB-stack lifecycle |
| Frontend parsing and semantic facts remain separate | nine real tree-sitter integrations; the IR never reads source text or operator spelling |
| Resource limits and malformed states are typed and exact | node, operand, work, cycle, topology, type, binding, call, literal, and language campaigns |

### Stable commits

| Commit | Major changes |
|---|---|
| `ec8ab04` | executable TLA+, Rocq, and Z3 admission model plus formal plan and trust-boundary diagram |
| `5eb0715` | live-versus-persisted provenance and certificate revocation |
| `70c7ad9` | numeric descriptors, overflow, shift, and finite-floating boundaries |
| `81e518e` | exclusion of widthless masked shifts |
| `6b7d86e` | typed flat production IR, public API, exhaustive tests, and nine parser integrations |
| `0fd84ec` | required documentation pillars, reproducible diagrams, and exhaustive effect-mask coverage |
| `79882fb` | formal raw/effective parent-context model discovered by the terminal gate |
| `c8dfcb0` | constant-time carried parent context preserving ownership and CPG topology |

### Terminal-gate defect resolved

The first maximal-matrix attempt exposed pgmcp bug
`dovetail-x-tree-parent-quadratic`: Rust ownership classification reconstructed
every tree-sitter parent. The headless profile, formal repair, invariant-derived
tests, and measured result are recorded in the
[parent-context plan](../verification/tree-parent-context-formal-plan.md) and
[performance guide](03-performance.md#tree-sitter-parent-context-scaling).

### Executable gates

```bash
scripts/verify-expression-ir-formal.sh all
cargo test --lib expression_ir::tests --features serde
cargo test --test expression_ir_integration \
  --features lang-rust,lang-c,lang-cpp,lang-go,lang-java,lang-python,lang-javascript,lang-typescript,lang-ruby
cargo clippy --lib --test expression_ir_integration \
  --features serde,lang-rust,lang-c,lang-cpp,lang-go,lang-java,lang-python,lang-javascript,lang-typescript,lang-ruby \
  -- -D warnings
```

The formal gate must pass before any production semantic change. Validation
commands run headlessly under a resident-memory ceiling with swap disabled and
one Cargo build job; logs belong under `target/verification`, never a
memory-backed temporary filesystem.

## Interpretation

These artifacts support the represented T4.1 contract. They do not prove a
frontend type checker, arbitrary source-language equivalence, Replete's future
congruence invariants, saturation completeness, extraction optimality, or any
pending campaign leaf. A terminal green run supports claimed completion; only
the pgmcp trust workflow may advance that claim to verified.

Related evidence is organized in [ADR-0074](../design/0074-typed-pure-expression-ir.md),
[Theory 57](../theory/57-datalog-egraphs-and-typed-expression-semantics.md),
[Scientific 69](../scientific/69-expression-ir-validation.md),
[Security 32](../security/32-expression-rewrite-boundary.md), and the
[API reference](../api/expression-ir-reference.md).
