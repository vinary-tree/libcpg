# Engineering Overview

This is the entry point to `libcpg`'s **engineering** documentation: how the crate
is built and configured, how it is tested, how it performs (and how to measure
that rigorously), and how to contribute to it. Its sibling pillar,
[`security/`](../security/00-threat-model.md), documents the threat model and the
input/resource bounds that keep analysis finite on untrusted input.

`libcpg` is a Rust library for constructing and querying [Code Property
Graphs](../GLOSSARY.md#code-property-graph-cpg) (CPGs) — a single graph that
overlays a program's [AST](../GLOSSARY.md#abstract-syntax-tree-ast),
[CFG](../GLOSSARY.md#control-flow-graph-cfg),
[DFG](../GLOSSARY.md#data-flow-graph-dfg), and, on demand, its
[PDG](../GLOSSARY.md#program-dependence-graph-pdg) — plus exact SCC decomposition,
deterministic projected graph analyses, pattern detection, algorithm/complexity
heuristics, and graph-neural embeddings on top of that
substrate. The conceptual "why" lives in [`theory/`](../theory/00-overview.md);
the "what/where" of the code lives in
[`architecture/`](../architecture/overview.md); this pillar is the "how to work
on it".

## Crate metadata

The following is taken verbatim from [`Cargo.toml`](../../Cargo.toml).

| Field | Value |
| --- | --- |
| Package name | `libcpg` |
| Version | `0.1.1` |
| Rust edition | `2021` |
| License | `MIT OR Apache-2.0` |
| Repository | <https://github.com/f1r3fly-io/libcpg> |
| Author | Dylon Edwards `<dylon@f1r3fly.io>` |
| Keywords | `code-analysis`, `cpg`, `ast`, `cfg`, `dfg`, `pattern-detection`, `gnn` |
| Categories | `development-tools`, `parsing`, `algorithms` |
| Default features | `default = []` (nothing on by default) |
| MSRV | not declared — `Cargo.toml` carries no `rust-version`; any edition-2021 toolchain that satisfies `tree-sitter 0.26` should build it |

The single most important fact for a newcomer is the empty default feature set:
with no [feature flags](../GLOSSARY.md#feature-flag-cargo) enabled,
`TreeSitterCpgBuilder::build(source, language)` fails for *every* language,
because no [tree-sitter](../GLOSSARY.md#tree-sitter) grammar is compiled in. Only
the feature-free surface — hand-built CPGs, the
[Mode B](../GLOSSARY.md#mode-b--build_from_tree) `build_from_tree` entry point,
exact CFG/call-graph [SCC](../GLOSSARY.md#strongly-connected-component-scc)
decomposition and [condensation
analytics](../GLOSSARY.md#condensation-graph--condensation-metrics),
[architectural layering](../GLOSSARY.md#architectural-layer--layering-violation),
[DSM visibility/core–periphery analysis](../GLOSSARY.md#design-structure-matrix-dsm),
[Martin package metrics](../GLOSSARY.md#martin-package-metrics),
[interprocedural effect summaries](../GLOSSARY.md#effect-summary--place--purity),
[advisory taint and maybe-null summaries](../GLOSSARY.md#taint-catalog--taint-witness), pure
[resource-leak, uninitialized-use, and exception-flow detectors](../GLOSSARY.md#exception-summary--uncaught-exception--dead-catch),
[missing-check and deviant-behavior population detectors](../GLOSSARY.md#missing-check-analysis--check-fingerprint),
[architectural-smell analysis](../GLOSSARY.md#architectural-smell),
[reflexion-model conformance](../GLOSSARY.md#reflexion-model),
[WL structural-clone analysis](../GLOSSARY.md#weisfeilerlehman-wl-refinement),
[projection centrality](../GLOSSARY.md#centrality),
[community detection](../GLOSSARY.md#community-detection),
[cohesion and structural-robustness analysis](../GLOSSARY.md#cohesion--structural-robustness),
[spectral connectivity](../GLOSSARY.md#algebraic-connectivity--fiedler-vector),
[bounded simple cycles](../GLOSSARY.md#elementary-cycle--simple-cycle),
[directed motif census](../GLOSSARY.md#directed-triad--motif-census),
[bounded bipartite GED](../GLOSSARY.md#graph-edit-distance),
[VF2](../GLOSSARY.md#vf2) matching, and PDG slicing — works out of the box. The
[build and features](01-build-and-features.md) page explains how to turn on what
you need.

## Ecosystem context

`libcpg` is a standalone crate in the [F1R3FLY.io](https://f1r3fly.io) workspace.
It has one especially relevant downstream consumer, **pgmcp**, which links its own
tree-sitter grammars and drives `libcpg` through
[Mode B](../GLOSSARY.md#mode-b--build_from_tree). Two engineering decisions follow
directly from that relationship, and both are visible in `Cargo.toml`:

- **Grammar version pins are matched to pgmcp** so the two crates do not each try
  to link a different copy of the same `tree_sitter_<lang>` C symbol (a duplicate
  symbol is a hard link-time error). The [build and features](01-build-and-features.md)
  page reproduces the pin table.
- **Rholang and MeTTa are Mode-B-only.** Their `rholang` / `metta` features are
  *empty* (`= []`) logical toggles that gate only the `map_rholang` / `map_metta`
  node-mapper arms; the grammars are linked solely as **test-only** path
  `[dev-dependencies]` (which Cargo never propagates downstream). See
  [`design/0002-mode-b-build-from-tree.md`](../design/0002-mode-b-build-from-tree.md).

## Map of the engineering pillar

| Page | What it covers |
| --- | --- |
| [01 — Build and features](01-build-and-features.md) | The full feature matrix: 16 `lang-*` grammars, the language groups, the analysis features, the Mode-B toggles, `serde`, the reserved `gpu` flag, and the `full` umbrella; the pgmcp grammar-pin table; how to enable a language. |
| [02 — Testing](02-testing.md) | The example-based and `proptest` suite plus the general, robustness, SCCP, interval, loop-induction, complexity-evidence, condensation, layering, architectural-smell, DSM, Martin, reflexion, WL, centrality, community, cohesion, spectral, cycle, motif, GED, deterministic CPG-diff, function-clone, clone-feasibility, API-usage/temporal, CWE-template, deterministic ML-export, concurrency-bug, program-composition, IFDS/IDE, effect-summary, taint, maybe-null, typestate, bug-detector, and population-anomaly public-API/stack suites; the `arb_*` generators; how to run each feature subset; measured coverage; the defects the suite found; and the active Criterion benchmark substrate. |
| [03 — Performance](03-performance.md) | The data-structure rationale, asymptotic costs including typestate, bug detectors, population anomaly inference, clone/feasibility advisories, exact call resolution, API-usage/temporal mining, declarative CWE templates, and CPG differencing, thirty-three active oracle-guarded Criterion targets, controlled parallel scaling, and explicit remaining benchmark gaps. |
| [Optimization ledger](optimization-ledger.md) | The reproducible T9.0 `main` baseline: 34 Criterion cases, per-workload RSS and DHAT allocations, Massif and type-size snapshots, perf counters and ranked hot paths, profiler availability boundaries, and the evidence-ranked experiment queue. |
| [Analysis-expansion matrix](analysis-expansion-matrix.md) | The epic-wide requirement-to-public-API-to-feature-to-document map, including typed overlays, caps/completeness, migration, security, examples, operations, and T5.1 reconciliation evidence. |
| [Dovetail extraction/optimization ledger](dovetail-extraction-optimization-evidence-ledger.md) | The follow-up campaign boundary and stable formal, implementation, test, and documentation evidence for libcpg-owned leaves. |
| [04 — Contributing](04-contributing.md) | Step-by-step recipes: add a language mapper, add a Gang-of-Four template, add a similarity metric; plus the coding conventions the crate follows. |

## Map of the security pillar

| Page | What it covers |
| --- | --- |
| [`security/00` — Threat model](../security/00-threat-model.md) | Assets, attackers, and the two trust boundaries (untrusted source bytes; untrusted serialized graphs); why tree-sitter parsing executes no analyzed code; why detection results are advisory. |
| [`security/01` — Input and resource hardening](../security/01-input-and-resource-hardening.md) | The concrete bounds — `max_file_size`, DFG structural passes, IFDS/IDE functions/facts/work/recursion/depth, effect-summary traversal widening, condensation/layering and DSM/WL/centrality/community limits, Martin/architectural-smell evidence validation, reflexion path-policy admission, k-truss/min-cut/attack ceilings, VF2 `max_matches`, and slice `max_nodes` — with recommended caps for hostile input and where caller-side parser timeouts belong. |
| [`security/02` — Taint-analysis boundary](../security/02-taint-analysis.md) | Why catalog policy is caller-owned, why findings are advisory, how confidence and witnesses should be interpreted, and why capped empty results cannot support absence claims. |
| [`security/03` — Interprocedural-slicing boundary](../security/03-system-dependence-slicing.md) | Summary-edge trust, context/depth caps, graph mutation, and safe interpretation of bounded slices. |
| [`security/04` — Channel-topology boundary](../security/04-channel-topology.md) | Typed message evidence, channel identity, linear-consumption candidates, and false-positive/negative limits. |
| [`security/05` — Deadlock-analysis boundary](../security/05-deadlock-analysis.md) | Communication and lock-order candidate evidence, SCC witnesses, resource identity, and review requirements. |
| [`security/06` — MHP-analysis boundary](../security/06-mhp-analysis.md) | Structural may-happen-in-parallel pairs, output caps, and the distinction from races or runtime overlap. |
| [`security/07` — Session-analysis boundary](../security/07-session-analysis.md) | Shallow endpoint duality, missing evidence, and limits of binary linear protocol sketches. |
| [`security/08` — Typestate-analysis boundary](../security/08-typestate-analysis.md) | Caller property trust, allocation abstraction, capped may-facts, terminal policy, and advisory witnesses. |
| [`security/09` — Bug-detector boundary](../security/09-bug-detectors.md) | Resource, definition, and exception evidence; incomplete-result controls; stack/resource safety; and impact review. |
| [`security/10` — Population-anomaly boundary](../security/10-population-anomaly-detectors.md) | Catalog and peer trust, exact support, population poisoning, incomplete-result controls, stack/resource safety, and impact review. |
| [`security/11` — Clone/feasibility boundary](../security/11-clone-feasibility-detectors.md) | Pair/guard evidence trust, SCCP/interval proof limits, disjoint-region and short-circuit controls, stack/resource safety, and remediation review. |
| [`security/12` — API-usage protocol boundary](../security/12-api-usage-protocol-mining.md) | Population/FSM trust, exact support, poisoning and reachability limits, stack/resource safety, completeness, and repair separation. |
| [`security/13` — CWE-template boundary](../security/13-cwe-template-analysis.md) | Template/catalog trust, represented-domain limits, secret minimization, resource/stack safety, completeness, and verdict separation. |
| [`security/22` — Resolver symbol-index boundary](../security/22-resolver-symbol-index-boundary.md) | Exact spelling and ambiguity integrity, call-local ownership, deterministic order, Unicode boundaries, fixed native stack, and the rejected fuzzy quadratic-heap exposure. |

## Honest project status

`libcpg` is at version `0.1.1`; several surfaces are intentionally minimal or
aspirational. This pillar documents them plainly rather than implying more
maturity than exists. The most consequential items:

| Aspect | Reality | Detailed in |
| --- | --- | --- |
| Default build | `default = []`; `build` fails without a `lang-*` feature | [01](01-build-and-features.md) |
| `examples/` directory | contains feature-free runnable `population_anomalies` (I6/I7), `clone_feasibility_advisories` (I8/I9), `api_usage_protocols` (I12), and `cwe_templates` exact-oracle examples; `tests/` holds integration and robustness suites | [02](02-testing.md), [Usage 18](../usage/18-population-anomaly-detectors.md), [Usage 19](../usage/19-clone-feasibility-advisories.md), [Usage 20](../usage/20-api-usage-protocol-mining.md), [Usage 21](../usage/21-cwe-template-analysis.md) |
| Benchmarks | thirty-three oracle-guarded Criterion targets are active; the T9.0 baseline covers construction, traversal, DFG, PDG, SCC, VF2, WL, and whole-corpus analysis, while newer controls cover incremental reuse, bit-set kernels, exact call-resolution families, and parallel scaling; effect summaries, selected graph metrics, and GNN propagation remain broader gaps | [02](02-testing.md), [03](03-performance.md), [ledger](optimization-ledger.md) |
| Resolver symbol index | call-local exact ordered map retained; DAT, DynamicDawg, prefix, fuzzy, SIMD, release, and resource controls reject a runtime dependency without changing topology semantics | [ADR-0064](../design/0064-retain-ordered-resolver-symbol-index.md), [validation 59](../scientific/59-resolver-symbol-index-validation.md), [Security 22](../security/22-resolver-symbol-index-boundary.md), [Usage 32](../usage/32-exact-call-resolution.md) |
| `rayon` | wired into explicit deterministic CFG/DFG/PDG and ordered VF2-batch parallel entry points; serial APIs and builder defaults remain serial | [03](03-performance.md), [Usage 29](../usage/29-parallel-analysis.md), [validation 54](../scientific/54-parallel-analysis-validation.md) |
| Mapped strings | build-local Fx canonicalization retains public `Arc<str>` fields while collapsing equal allocations inside each Mode-A/Mode-B build; no global pool or pointer-identity contract | [03](03-performance.md), [ADR-0060](../design/0060-build-local-string-canonicalization.md), [validation 55](../scientific/55-string-storage-validation.md) |
| WL scratch storage | two call-local dense label matrices plus canonical/incident vectors retain capacity across bounded rounds; no cross-call cache, label change, or native recursion | [03](03-performance.md), [ADR-0061](../design/0061-reusable-wl-refinement-buffers.md), [validation 56](../scientific/56-wl-buffer-reuse-validation.md) |
| `regex` | declared in `Cargo.toml` but **not yet wired** into `src/`; rejected `ahash` and `string_cache` dependencies were removed after H11 measurement | [02](02-testing.md), [03](03-performance.md) |
| `gpu` feature | **reserved** — enables `wgpu`/`pollster` but no GPU code is written | [01](01-build-and-features.md) |
| Detection results | pattern/algorithm/complexity outputs are **heuristics**, not proofs | [`security/00`](../security/00-threat-model.md) |

None of these gaps affect the correctness of the features that *are* implemented —
CPG construction, CFG/DFG extraction, PDG slicing, VF2 matching, and (behind their
features) Gang-of-Four detection, algorithm heuristics, and GNN embeddings — each
of which is exercised by the inline test suite catalogued in
[testing](02-testing.md).

## Where to go next

- New to CPGs? Start with [`theory/00-overview.md`](../theory/00-overview.md) and
  the [glossary](../GLOSSARY.md).
- Want to build something today? Enable a grammar per
  [build and features](01-build-and-features.md), then follow
  [`usage/00-getting-started.md`](../usage/00-getting-started.md).
- Integrating Rholang/MeTTa? See
  [`usage/06-f1r3fly-rholang-metta.md`](../usage/06-f1r3fly-rholang-metta.md) and
  the Mode-B decision record,
  [`design/0002-mode-b-build-from-tree.md`](../design/0002-mode-b-build-from-tree.md).
