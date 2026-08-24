# Analysis-expansion requirement and documentation matrix

This page is the audit index for the libcpg analysis-expansion campaign. It
connects each epic family to its public contract, Cargo feature boundary,
semantic documentation, validation evidence, security interpretation, and
operational entry point. It is not a substitute for those documents: it makes
omissions and contradictions visible across them.

**Status boundary.** Rows marked *landed* describe code present on
`feature/libcpg-ax-epic`. Rows marked *mandatory successor* are not claims that
an API already exists; their implementation task must replace the row with
concrete API, test, and documentation links when it lands. This distinction
prevents both invented documentation and the opposite error—describing an
already emitted relation as reserved.

## Vocabulary and ownership

A **Code Property Graph (CPG)** overlays an Abstract Syntax Tree (AST), Control
Flow Graph (CFG), Data Flow Graph (DFG), Program Dependence Graph (PDG), and
related typed relations on one node-identity space. A **cap** is an explicit
resource bound. A **complete** report is one whose modeled input was admitted
and whose solver reached its documented fixed point without a cap or structural
issue. A **positive projection** returns findings but may omit completeness
metadata; it cannot support a negative claim.

The ownership boundary is deliberate:

| Owner | Responsibility |
|---|---|
| libcpg | source-language lowering, typed graph construction, deterministic analysis kernels, witnesses, caps, and completeness evidence |
| pgmcp | indexed-source admission, policy selection, libcpg orchestration, identity remapping, persistence, response limits, and presentation |
| caller | language/feature selection, security and catalog policy, resource budgets, and final interpretation |

pgmcp does not parse source with regular expressions or maintain competing
LCOM, clone, taint, or structural graph kernels. Its authoritative downstream
guide is `docs/quality/live-cpg-analysis.md` in the pgmcp repository; libcpg's
[program-composition](../components/graph/program-composition.md),
[call-resolution](../components/graph/call-resolution.md), and
[taint-analysis](../components/graph/taint-analysis.md) guides define the
semantic side of that boundary.

## Requirement-to-document matrix

The tracker identifiers in the first column are stable public IDs. “Feature
free” means available with `default = []`; parser-backed examples still require
the relevant `lang-*` grammar.

| Epic family and tasks | Public surface and state | Cargo boundary | Semantic and API documentation | Validation, security, and usage |
|---|---|---|---|---|
| SE-0: `tp0-1`, `tp1-1`, `tp1-2`, `tp2-1` | petgraph 0.7; `GraphProjection`; CFG/call SCCs; dominators and post-dominators; `CallGraphResolution`, symbol table, emitted scope/reference/call topology — **landed** | feature free | [projections](../components/graph/projections.md), [SCC](../components/graph/scc-analysis.md), [dominators](../components/graph/dominator-analysis.md), [call resolution](../components/graph/call-resolution.md), [ADRs 0006–0008](../design/00-overview.md), [graph API](../api/graph-reference.md) | [CPG invariants](../scientific/01-cpg-invariants-and-equivalence.md), [threat model](../security/00-threat-model.md), [querying](../usage/02-querying-and-traversal.md) |
| SE-1: `t1-1` … `t1-11` | function CFG scope, natural-loop forest, reachability, `BlockCfg`, monotone solver, liveness/dead stores, available/very-busy expressions, sparse conditional constant propagation (SCCP), intervals, induction variables, and complexity evidence — **landed** | feature free; legacy heuristic complexity also under `algorithm-detection` | [graph components](../components/graph/overview.md), [data-flow architecture](../architecture/data-flow.md), [Theory 02–03](../theory/00-overview.md), [graph API](../api/graph-reference.md), [pattern API](../api/pattern-reference.md) | [Scientific 05–08](../scientific/00-overview.md), [input hardening](../security/01-input-and-resource-hardening.md), [data-flow framework](../components/graph/dataflow-framework.md) |
| SE-2: `t2-1` … `t2-17` | centrality; Louvain/Leiden communities; cores, trusses, cuts, robustness; algebraic connectivity/Fiedler eigenpair; Weisfeiler–Lehman (WL) classes; cycles; motifs; condensation scheduling; graph-edit bound; Martin/DSM/reflexion/layering/smells; typed field writes; CK/Halstead/LCOM; clone types 1–4 — **landed** | feature free; GoF/legacy pattern detectors remain `design-patterns` | [graph components](../components/graph/overview.md), [pattern components](../components/patterns/overview.md), [Theory 10–23](../theory/00-overview.md), [graph API](../api/graph-reference.md) | [Scientific 09–25](../scientific/00-overview.md), [security 00–01](../security/00-threat-model.md), [usage 03](../usage/03-pattern-detection.md) |
| SE-3: `t3-1` … `t3-9` | program CPG/import graph, IFDS/IDE summaries, taint/null flow, system-dependence slicing, effect summaries, channel topology, communication/lock deadlock candidates, may-happen-in-parallel (MHP), Rholang session sketches — **landed** | core feature free; session sketches require `rholang` | [Theory 24–32](../theory/00-overview.md), [data-flow architecture](../architecture/data-flow.md), [graph API](../api/graph-reference.md) | [Scientific 26–34](../scientific/00-overview.md), [security 02–07](../security/02-taint-analysis.md), [usage 08–15](../usage/08-ifds-summary-domains.md) |
| SE-4: `t4-0` … `t4-6` | pgmcp adapters, exact structural delegation, float-tolerant migration, semantic-delta migration, parallel Brandes orchestration, duplicate-kernel removal, live LCOM/clone/taint tools — **landed in pgmcp** | pgmcp selects libcpg features; libcpg kernels retain their own boundary | pgmcp `docs/quality/live-cpg-analysis.md`; libcpg [program composition](../components/graph/program-composition.md), [code metrics](../components/graph/code-metrics.md), [clones](../components/patterns/clone-detection.md), [taint](../components/graph/taint-analysis.md) | pgmcp adapter/integration/property tests; [taint security](../security/02-taint-analysis.md); [taint usage](../usage/10-taint-and-nullflow.md) |
| SE-6: `t6-1` … `t6-5` | typestate, resource leaks, uninitialized use, exception flow, missing checks, deviant behavior, inconsistent clones, infeasible conditions, API-usage graphs, and temporal properties — **landed** | feature free | [Theory 33–37](../theory/00-overview.md), [ADRs 0043–0047](../design/00-overview.md), [graph API](../api/graph-reference.md) | [Scientific 38–42](../scientific/00-overview.md), [security 08–12](../security/08-typestate-analysis.md), [usage 16–20](../usage/16-typestate-analysis.md) |
| SE-7: `t7-1` … `t7-4` | declarative Common Weakness Enumeration (CWE) templates, cryptographic misuse, secret flow, missing authentication/authorization, and vulnerability extrapolation — **landed** | feature free; caller supplies policy/catalog where documented | [Theory 38–41](../theory/00-overview.md), [ADRs 0048–0051](../design/00-overview.md), [graph API](../api/graph-reference.md) | [Scientific 43–46](../scientific/00-overview.md), [security 13–16](../security/13-cwe-template-analysis.md), [usage 21–24](../usage/21-cwe-template-analysis.md) |
| SE-8: `t8-1` … `t8-4` | closed-world CHA/RTA/VTA call refinement; race and atomicity candidates; deterministic CPG tensor/line/AST-path export; structural CPG diff — **landed** | feature free | [Theory 42–45](../theory/00-overview.md), [ADRs 0052–0055](../design/00-overview.md), [graph API](../api/graph-reference.md) | [Scientific 47–50](../scientific/00-overview.md), [security 17–20](../security/17-type-refinement-boundary.md), [usage 25–28](../usage/25-virtual-call-type-refinement.md) |
| SE-9: `t9-0` | immutable benchmark/profile baseline over 34 workload cases and 28 Criterion targets — **landed** | `criterion`/profilers are development tooling; no runtime feature | [performance](03-performance.md), [optimization ledger](optimization-ledger.md) | [testing](02-testing.md), `scripts/bench.sh`, `scripts/profile.sh` |
| SE-9: `t9-1` … `t9-4` | benchmark-gated mechanical, structural, parallel/interning, incremental, and SIMD work — **mandatory successors** | changes may activate currently unwired dependencies only when evidence justifies them | each phase must update [performance](03-performance.md) and the [ledger](optimization-ledger.md) before claiming a public or operational change | each experiment must retain its oracle, statistics, allocation/RSS evidence, rollback rule, and feature/platform assumptions |
| SE-10: `ft-1` … `ft-2` | canonical Datalog EDB export; explicit/implicit PDG/SDG information-flow witnesses; hard declassification cuts; secret-dependent branch/index evidence — **landed** | feature free; parser examples use `lang-rust`; persistence derives under `serde` | [Datalog](../components/graph/datalog-export.md), [IFC](../components/graph/information-flow-control.md), [Theory 48–49](../theory/00-overview.md), [ADRs 0065–0066](../design/00-overview.md), [graph API](../api/graph-reference.md) | [Scientific 60–61](../scientific/00-overview.md), [security 23–24](../security/23-datalog-export-boundary.md), [usage 33–34](../usage/33-datalog-export.md) |
| SE-10: `ft-3` … `ft-8` | relational numeric domains, separation logic, rho-calculus liveness, MeTTa rewriting, place-capability ownership, heterogeneous tensors, and directed-fuzzing distance — **mandatory successors; no current API claim** | task-specific; MeTTa rewriting is `metta`, remaining core overlays are feature free unless their acceptance contract says otherwise | each landing must add design, theory, scientific, architecture/component, security, API, usage, glossary, and diagram entries | each landing must add hand or published oracles, property tests, public integration, deterministic ordering, cap/completeness evidence, and small-stack depth tests where input depth applies |
| SE-5: `t5-1` … `t5-4` and stack-safety bugs | documentation, benchmark substrate, pushdown-automata field guide/audit, and final one-time integration gate — **T5.1–T5.3 landed or active; T5.4 terminal** | all relevant configurations | this matrix, [testing](02-testing.md), [performance](03-performance.md), [diagram catalog](../diagrams/README.md), and pgmcp `docs/design/stack-safe-pushdown-flattening.md` | `scripts/verify-rustdoc.sh`, `verify-docs.sh`, `verify-diagrams.sh`, `verify-dois.sh`; terminal full verification remains T5.4-only |

## Typed-overlay reconciliation

The graph vocabulary is larger than any single producer. “Emitted” below means
libcpg currently materializes the relation from represented evidence; it does
not mean every language can supply that evidence.

| Overlay | Emitted relations relevant to this campaign | Intentionally non-emitted or conditional relations |
|---|---|---|
| CFG | `Sequential`, both conditional kinds, `LoopBack`, `LoopExit`, `Break`, `Continue`, `Throw`, `Catch`, `Call`, and `Case` | `Return`, `CallReturn`, and `DefaultCase` are vocabulary-only in `CfgExtractor`; `Call` is configuration-dependent |
| DFG | definition/use/value flow plus `FieldRead`, `FieldWrite`, `IndexRead`, and `IndexWrite` | alias tracking remains caller/model dependent; `track_aliases` is not silently approximated |
| call/reference/scope | `StaticCall`, `DynamicCall`, `CallSite`, `Reference`, `EnclosingScope`, and `ContainedIn` for uniquely supported evidence | ambiguous, unresolved, and external outcomes remain explicit out-of-graph resolution records |
| program/import | declaration-level `Imports` and `Exports`, translated file scopes, and collision-free program identities | absent or ambiguous imports never fabricate a call target |
| PDG/SDG | typed control/data dependence plus labeled interprocedural summary dependence | bounded slices and summaries remain qualified rather than pretending to be whole-runtime proofs |
| analysis overlays | immutable reports for loops, blocks, summaries, metrics, findings, tensors, and diffs | reports do not mutate or create a competing authoritative CPG |

## Feature, migration, and compatibility contract

| Concern | Contract | Documentation |
|---|---|---|
| Empty default | `default = []`; all core graph construction from caller-supplied nodes/trees and analysis kernels remain usable | [build and features](01-build-and-features.md) |
| Built-in grammars | sixteen `lang-*` features and their groups gate internal tree-sitter parsing | [builder API](../api/builder-reference.md), [frontends](../architecture/language-frontends.md) |
| Mode B | `rholang` and `metta` gate mapping only; callers supply compatible parse trees | [Mode-B usage](../usage/06-f1r3fly-rholang-metta.md), [ADR-0002](../design/0002-mode-b-build-from-tree.md) |
| Optional analyses | `design-patterns`, `algorithm-detection`, `gnn`, `ml-rules`, and `ml-linfa` expose only their documented families | [build and features](01-build-and-features.md) |
| Persistence | `serde` exposes validated, versioned, stack-safe property/expression representations and rejects malformed bounds before reconstruction | [node API](../components/graph/nodes.md), [security hardening](../security/01-input-and-resource-hardening.md) |
| Call-resolution migration | resolution is opt-in; enabling it changes graph shape by adding only evidence-supported topology, while default shape remains compatible | [ADR-0006](../design/0006-evidence-bounded-call-resolution.md) |
| Basic-block migration | `BlockCfg` preserves CPG node IDs; `BasicBlockIdentifier` remains a documented compatibility view | [ADR-0008](../design/0008-id-preserving-basic-block-cfg.md) |
| pgmcp migration | adapters convert stable identities, invoke libcpg, validate invariants, and map results; no source parser or duplicate semantic kernel remains | pgmcp `docs/quality/live-cpg-analysis.md` |

## Caps, completeness, errors, and security

| Boundary | Required behavior | Negative-claim rule |
|---|---|---|
| malformed graph or selector | return a typed `AnalysisError`/domain issue; never panic, recurse without bound, or synthesize evidence | error is not absence |
| fixed-point work | expose `IfdsCaps`, typed cap events, processed work, and ordered issues | require the report's `is_complete()` predicate |
| enumerations/search | validate node/work/result/depth budgets before or during deterministic traversal; retain canonical prefixes only where the API explicitly defines them | capped empty output is inconclusive |
| parser and lifecycle depth | use heap worklists, typed continuation frames, or validated flat arenas; semantic depth limits and larger thread stacks are not substitutes | a stack-safe run can still be resource-incomplete |
| security findings | retain source/sink/object/type identity, canonical witnesses, model/catalog evidence, and cap qualifiers | findings are advisory; absence requires complete represented scope |
| response limits | solve first, then truncate presentation where documented; distinguish `total`, `returned`, and `truncated` | truncated output is not exhaustive |

The [threat model](../security/00-threat-model.md) defines the common trust
boundary. Each security family then narrows the evidence and false-positive /
false-negative interpretation in `docs/security/`.

## Examples and operations

The task-oriented entry point is [Usage 00](../usage/00-getting-started.md).
Feature-free executable examples currently include population anomaly, clone
feasibility, API-usage protocol, CWE-template, cryptographic misuse,
secret/access policy, vulnerability extrapolation, type refinement,
concurrency bugs, machine-learning export, and CPG-diff workflows. They are
compiled under `cargo test --all-features --examples`; focused commands appear
in their corresponding usage pages.

Repository operations are intentionally split:

```text
For a documentation change:
    run strict default and all-feature rustdoc plus doctests
    validate local links and fragments
    validate canonical DOI syntax; use --resolve when network is available
    render every diagram twice in headless mode
    require both renders and the committed SVG to be byte-identical

For an analysis change:
    run its hand/published example oracle
    run its generated/property oracle
    run its public integration and graph-invariant checks
    run its small-native-stack deep case when input depth can grow
    update the semantic, security, usage, and scientific documents together
```

The executable commands are:

```bash
./scripts/verify-rustdoc.sh
./scripts/verify-docs.sh
./scripts/verify-dois.sh --resolve
```

`verify-docs.sh` includes the headless, two-pass diagram gate. It keeps render
artifacts under `target/verification` and deletes only its own validated
temporary directory.

## T5.1 reconciliation evidence

| Audit | Before reconciliation | Current acceptance | Repair category |
|---|---:|---:|---|
| rustdoc warnings | 127 in the recorded pre-reconciliation all-feature baseline | 0 for default and all features under `RUSTDOCFLAGS=-D warnings` | public-item and feature-gated API documentation landed with their owning tasks; the final gate rejects any regression |
| broken local links/fragments | 17 | 0 | corrected glossary anchors for message passing, taint evidence, WL refinement, and program composition |
| invalid DOI identifiers | 3 | 0 | corrected Havlak, Komondoor–Horwitz, and both parts of Shannon using primary publication metadata |
| stale implemented-surface claims | scope and exception relations, exception nodes, and `Catch` emission described as reserved | 0 matched policy phrases | reconciled prose with actual builder/resolver emission sites and added a source policy |
| diagram inventory | 124 sources / 124 SVGs; 56 SVGs differed from a fresh render and 12 PlantUML sources duplicated global styling instead of including the shared theme | 124 paired, catalogued, colored, shared-theme, twice-rendered byte-identical sources and committed outputs | normalized the 12 sources, refreshed renderer output, and added inverse pairing, catalog, theme, color, error-card, size, and reproducibility checks |
| doctests | all three examples ignored | three compile-checked examples under default and all features; the feature-free root example also executes | replaced the aspirational ignored root snippet with a real CPG example and made parser snippets complete `no_run` programs |

The exact commands are:

```bash
RUSTDOCFLAGS='-D warnings' cargo doc --no-deps
RUSTDOCFLAGS='-D warnings' cargo doc --all-features --no-deps
RUSTDOCFLAGS='-D warnings' cargo test --doc
RUSTDOCFLAGS='-D warnings' cargo test --all-features --doc
lychee --offline --include-fragments --no-progress README.md docs
./scripts/verify-dois.sh --resolve
./scripts/verify-diagrams.sh
```

ADR-0006 originated in `10c1455`, ADR-0007 in `56a951a`, and ADR-0008 in
`d0f1268`; subsequent feature commits cross-link later consumers. The T5.1
reconciliation commit and its complete captured command results are linked to
tracker item `libcpg-ax-t5-1`, because a commit cannot contain its own hash.
Validation logs are captured under `target/verification` during the audit and
removed by exact path after inspection; no build, dependency, diagram, or log
artifact is written under `/tmp`.

This is a claimed implementation audit, not independent verification. The
terminal T5.4 task must re-walk every leaf and request gatekeeper/user
verification after the one permitted full pgmcp verification run.
