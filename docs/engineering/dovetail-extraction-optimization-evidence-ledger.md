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

## T4.2 -- proof-producing expression equivalence

Tracker leaf `dovetail-x-t4-2` composes live T4.1 expressions with standalone
Replete without importing parser or application ownership into the e-graph
crate. Its invariants and evidence are:

| Obligation | Durable evidence |
|---|---|
| Positive pairs require admission, modeled equality, complete provenance, and replay | [`ExpressionEquivalence.tla`](../../formal/tla/ExpressionEquivalence.tla), [`ExpressionEquivalence.v`](../../formal/rocq/ExpressionEquivalence.v), [`expression_equivalence.smt2`](../../formal/z3/expression_equivalence.smt2) |
| Exact equality cannot cross binding contexts; alpha evidence retains a bijection | formal models above plus exact/alpha examples and non-bijective counterexamples |
| AC normalization preserves association, permutation, and multiplicity | Rocq multiset laws; all 12 three-operand forms and all 66 pair proofs; repeated-leaf counterexample |
| Nested closed folding reaches fixed point and rejects corrupted/open proofs | TLA+/Rocq/Z3 refinement plus independent full-term evaluator tests |
| Resource exhaustion is typed and deterministic | candidate, input, e-node, work, match, iteration, pair, and proof-cap campaigns |
| Expression/proof depth never becomes native call-stack depth | two 100,000-level lifecycle gates on 256 KiB worker stacks |
| Frontend and CPG behavior remain unchanged | nine real tree-sitter integrations with exact pre/post node and edge counts |

### Stable commits

| Commit | Major changes |
|---|---|
| `1a6425b` | TLA+, Rocq, and Z3 fixed-point, proof-replay, AC, cap, and closed-evaluation obligations |
| `5f31b17` | typed Replete adapter, exact/alpha passes, proof replay, canonical codecs, and exhaustive tests |
| `adf70ef` | separable pair-output and AC-normalization Criterion baseline |

### Executable gates

```bash
scripts/verify-expression-equivalence-formal.sh all
cargo test --lib expression_equivalence --all-features
cargo test --test expression_equivalence_integration \
  --features lang-rust,lang-c,lang-cpp,lang-go,lang-java,lang-python,lang-javascript,lang-typescript,lang-ruby
cargo clippy --all-targets --all-features -- -D warnings
cargo bench --bench expression_equivalence --no-default-features -- --quick
```

The benchmark separates quadratic public pair output from AC-width effects.
Its local smoke/profile evidence, limitations, and exact measurements are
recorded in [Scientific 70](../scientific/70-expression-equivalence-validation.md)
and [the performance guide](03-performance.md#expression-equivalence-baseline).

## T4.3 -- proof-qualified semantic expression flow

Tracker leaf `dovetail-x-t4-3` composes exact T4.2 equality with libcpg-owned
control-flow facts. It emits advisory CSE/PRE candidates and additive
cross-revision semantic evidence without changing the CPG or legacy structural
diff taxonomy.

| Obligation | Durable evidence |
|---|---|
| Equality never substitutes for availability, anticipability, dominance, or kill freedom | [`SemanticExpressionFlow.tla`](../../formal/tla/SemanticExpressionFlow.tla), [`SemanticExpressionFlow.v`](../../formal/rocq/SemanticExpressionFlow.v), [`semantic_expression_flow.smt2`](../../formal/z3/semantic_expression_flow.smt2) |
| Kills use resolved definitions and unknown effects fail closed | formal models plus shadowing, true-redefinition, unknown-assignment, call-barrier, and exceptional-exit tests |
| CSE requires a dominating producer; PRE requires a busy common placement | all sixteen diamond transfer combinations, loop/exit examples, premise counterexamples, and proof-index replay |
| Four semantic-diff results retain decisive-evidence precedence | formal classification plus example/property tests for equivalent, different, limited, unknown, and contradiction rejection |
| Revision-local `NodeId` values may overlap safely | private comparison-slot invariant and parser-built old/new CPG integrations |
| CPGs and legacy structural diff remain immutable | exact before/after node, edge, and `CpgDiff` checks across nine languages |
| CFG/expression depth never becomes native call-stack depth | 100,000-node full report lifecycle on a 256 KiB worker stack |

### Stable commits

| Commit | Major changes |
|---|---|
| `e8f8f98` | TLA+, Rocq, and Z3 equality/dataflow composition and four-way diff contract |
| `bf8cf87` | stack-safe semantic classes, resolved kills, must flow, CSE/PRE candidates, diff API, exhaustive unit/property/integration tests |
| `868cc78` | separable proof-plus-flow and cross-revision Criterion baselines |

### Executable gates

```bash
scripts/verify-semantic-expression-flow-formal.sh
cargo test --lib semantic_expression_flow --all-features
cargo test --test semantic_expression_flow_integration \
  --features lang-rust,lang-c,lang-cpp,lang-go,lang-java,lang-python,lang-javascript,lang-typescript,lang-ruby
cargo clippy --all-targets --all-features -- -D warnings
cargo bench --bench semantic_expression_flow --no-default-features -- --quick
```

The benchmark and its interpretation are recorded in
[Scientific 71](../scientific/71-semantic-expression-flow-validation.md) and
the [performance guide](03-performance.md#semantic-expression-flow-baseline).

## T4.4 -- Replete, Datalog, and CPG responsibility boundary

Tracker leaf `dovetail-x-t4-4` adds a one-way owned semantic EDB bridge. It
replays proof-qualified T4.3 evidence, exports stable source identities and
exact completion, and leaves rule evaluation to a caller-owned Datalog engine.

| Obligation | Durable evidence |
|---|---|
| Non-reflexive semantic relations require replayed proof connectivity | [`ResponsibilityBoundary.tla`](../../formal/tla/ResponsibilityBoundary.tla), [`ResponsibilityBoundary.v`](../../formal/rocq/ResponsibilityBoundary.v), [`responsibility_boundary.smt2`](../../formal/z3/responsibility_boundary.smt2), proof corruption and connectivity tests |
| Positive rows survive limits; absence requires both fixed points | formal completion laws plus generated testing of all 91 semantic/Datalog completion pairs |
| Public identity is stable and engine-local state cannot leak | canonical `NodeId` fact types, private bundle fields, compile-fail proof promotion, and dependency/source gate |
| CSE/PRE references match endpoints and span their classes | adversarial missing, mismatched, cross-class, and disconnected proof tests |
| Ordinary CPG export and the CPG remain immutable | parser-built before/after integration test with equal ordinary facts and graph snapshots |
| Replete, Datalog, and CPG responsibilities remain acyclic | `scripts/verify-reasoning-boundaries.sh` Cargo-tree and module checks |
| Fact and rendering growth is bounded and stack-safe | checked row/byte limits plus 100,000-row export/clone/render/serialize/drop on a 256 KiB stack |

### Stable commits

| Commit | Major changes |
|---|---|
| `14c29ad` | TLA+, Rocq, and Z3 proof/completion/identity/ownership/dependency contract |
| `68887f2` | owned semantic relations, proof/candidate validation, completion API, exhaustive tests, and architecture gate |

### Executable gates

```bash
scripts/verify-responsibility-boundary-formal.sh
scripts/verify-reasoning-boundaries.sh
cargo test --lib --all-features semantic_datalog
cargo test --all-features --test semantic_datalog_integration
cargo test --doc --all-features
cargo clippy --all-targets --all-features -- -D warnings
```

The schema, trust boundary, and interpretation limits are recorded in
[ADR-0077](../design/0077-owned-semantic-datalog-evidence.md),
[Scientific 72](../scientific/72-semantic-datalog-boundary-validation.md), and
[Security 35](../security/35-semantic-datalog-boundary.md).

## Interpretation

These artifacts support the represented T4.1 through T4.4 contracts. They do not prove a
frontend type checker, arbitrary source-language equivalence, Replete's future
extraction optimality, or any
pending campaign leaf. A terminal green run supports claimed completion; only
the pgmcp trust workflow may advance that claim to verified.

Related evidence is organized in [ADR-0074](../design/0074-typed-pure-expression-ir.md),
[Theory 57](../theory/57-datalog-egraphs-and-typed-expression-semantics.md),
[Scientific 69](../scientific/69-expression-ir-validation.md),
[Security 32](../security/32-expression-rewrite-boundary.md), and the
[expression-IR API reference](../api/expression-ir-reference.md). T4.2 adds
[ADR-0075](../design/0075-proof-producing-expression-equivalence.md),
[Scientific 70](../scientific/70-expression-equivalence-validation.md),
[Security 33](../security/33-expression-equivalence-boundary.md), and the
[expression-equivalence API reference](../api/expression-equivalence-reference.md). T4.3 adds
[ADR-0076](../design/0076-proof-qualified-semantic-expression-flow.md),
[Scientific 71](../scientific/71-semantic-expression-flow-validation.md),
[Security 34](../security/34-semantic-expression-flow-boundary.md), and the
[semantic expression-flow API reference](../api/semantic-expression-flow-reference.md).
T4.4 adds [ADR-0077](../design/0077-owned-semantic-datalog-evidence.md),
[Scientific 72](../scientific/72-semantic-datalog-boundary-validation.md),
[Security 35](../security/35-semantic-datalog-boundary.md), and the
[semantic Datalog API reference](../api/semantic-datalog-reference.md).
