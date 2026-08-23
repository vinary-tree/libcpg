# Scientific validation — overview

This pillar documents **what `libcpg` claims to be true, and the experiments that decide whether those claims hold.** It treats the library's inline test suite as a *scientific ledger*: every property the analyses depend on is stated as a falsifiable hypothesis, encoded as a deterministic test, and settled by an assertion that either passes (the hypothesis survives) or fails (the hypothesis is refuted and the code is wrong).

`libcpg` is a [Code Property Graph](../GLOSSARY.md#code-property-graph-cpg) (CPG) library: it merges a program's [Abstract Syntax Tree](../GLOSSARY.md#abstract-syntax-tree-ast), [Control Flow Graph](../GLOSSARY.md#control-flow-graph-cfg), [Data Flow Graph](../GLOSSARY.md#data-flow-graph-dfg), and — on demand — its [Program Dependence Graph](../GLOSSARY.md#program-dependence-graph-pdg) onto one shared node set, following Yamaguchi et al. [[1]](#references). Downstream consumers (pattern detection, slicing, embeddings, and the `pgmcp` indexer that drives Rholang/MeTTa via [Mode B](../GLOSSARY.md#mode-b--build_from_tree)) trust that graph. If the graph is malformed — duplicated edges, a use wired to the wrong definition, a pattern search that silently drops a match — every analysis built on top inherits the error. Validation is therefore not an afterthought; it is the contract.

## 1. Why a static-analysis library must be validated, not just tested

A CPG library makes three kinds of assertion, and each demands a different standard of evidence:

| Kind of claim | Example | Standard of evidence | Where it lives |
|---|---|---|---|
| **Structural invariant** | "Re-running an extractor adds no duplicate edges." | A deterministic property that must hold on *every* input; validated by construction + regression tests. | [01 — Invariants & equivalence](01-cpg-invariants-and-equivalence.md) |
| **Semantic correctness** | "A use resolves to its latest reaching definition, not a killed one." | Matches the textbook data-flow answer on a corpus of hand-audited scenarios. | [02 — Reaching-defs validation](02-reaching-defs-validation.md) |
| **Algorithmic completeness** | "The subgraph matcher finds *all* embeddings, no more and no fewer." | An exact count on a *discriminating* input that would expose a partial or over-eager search. | [03 — VF2 completeness](03-vf2-completeness.md) |

These are distinct from *performance* claims ("construction is fast", "VF2
prunes well"), which require controlled measurement rather than assertion.
`libcpg` ships thirty oracle-guarded Criterion targets (forty-three
feature-free operation groups, three parser-gated groups, and the gated
complexity-evidence group), but their evidence is limited to
the named graph families, sizes, environment, and operation. The
protocol and remaining gaps are specified in [04 — Measurement
methodology](04-measurement-methodology.md).

## 2. Tests as experiments: the scientific method applied

Every validation in this pillar follows the same loop, recorded so it can be audited and reconstructed:

```text
Hypothesis   A precise, falsifiable statement about the CPG or an algorithm
             (e.g. "build and build_from_tree produce graphs of identical shape").
     │
     ▼
Experiment   A deterministic inline #[cfg(test)] test with a fixed input, a fixed
             grammar/build path, and no randomness in the assertion path.
     │
     ▼
Result       The test's assertions. A pass corroborates the hypothesis; a failing
             assertion refutes it and localizes the defect.
     │
     ▼
Conclusion   Either the invariant holds (and the test guards it against regression),
             or a new hypothesis is derived and the loop repeats.
```

Two properties of the suite make this rigorous rather than merely suggestive:

- **Determinism.** The grounding tests fix their inputs and their build path. The one search that could in principle vary its *order* — VF2 — is pinned by a test that asserts an *exact set* of embeddings, not just a count, so a re-ordering cannot mask a dropped match (see [03](03-vf2-completeness.md)).
- **Discrimination.** A good experiment can *fail*. The VF2 regression deliberately uses a diamond target rather than a path, precisely because a path would pass even with a broken backtracker; only the diamond forces the mid-search retry that exposes the bug. A test that cannot distinguish correct from incorrect behaviour is not evidence.

## 3. The evidence base

The all-features Cargo inventory contains **1,262 library tests**, **146
integration/robustness tests**, and **3 rustdoc examples**, for **1,411
executable cases**. The integration inventory includes public and parser-backed gates for graph construction,
optimization, architecture, concurrency, summaries, slicing, persistence, and
typestate plus resource/definition/exception, population, clone, and path bug detectors,
including aggregate, Rust, Java dual-build, exact 4/5 and 9/10, and
depth-100,000 detector targets. These cases combine fixed examples,
differential oracles, and 99 exact
source-level `proptest!` blocks. The feature-free matrix remains
independently runnable and exercises the exact graph/data-flow substrate
without compiling any grammar. Thirty active Criterion targets supplement
correctness with forty-three feature-free operation groups, three parser-gated
groups, and a forty-seventh when `algorithm-detection` is enabled; see
[Testing](../engineering/02-testing.md) for the census and [04](04-measurement-methodology.md)
for the measurement contract.

Because `default = []`, many tests are **feature-gated**. The end-to-end reaching-defs and PDG scenarios that parse real Rust are annotated `#[cfg(feature = "lang-rust")]`; the graph-algebra and VF2 tests are feature-free (they build CPGs by hand). The map below notes which gate each grounding test sits behind.

## 4. Map of the validation pillar

| Page | Property validated | Anchoring inline test(s) | Gate |
|---|---|---|---|
| [01 — Invariants & equivalence](01-cpg-invariants-and-equivalence.md) | Single shared node set; graph-assigned unique IDs; AST child/parent consistency; idempotent extractors; `build` ≡ `build_from_tree` (identical node payloads, typed edges, and language) | example + property `build_from_tree_matches_build` tests, `parsed_shadowing_and_idempotent`, `def_use_backward_slice`, `parsed_nested_use_resolves_and_chains` | `lang-rust` (equivalence, DFG) / feature-free (slice) |
| [02 — Reaching-defs validation](02-reaching-defs-validation.md) | Nested-expression use resolution; latest-definition (strong update); non-flow of unrelated variables; shadowing scope; idempotency | `parsed_nested_use_resolves_and_chains`, `parsed_reassignment_uses_latest_definition`, `parsed_unrelated_variable_does_not_flow`, `parsed_shadowing_and_idempotent` | `lang-rust` |
| [03 — VF2 completeness](03-vf2-completeness.md) | Soundness floor (empty pattern → no match); kind-feasibility pruning; **all** embeddings found (diamond → exactly 2) with exact backtracking | `test_empty_pattern`, `test_single_node_match`, `test_multi_embedding_backtracking` | feature-free |
| [04 — Measurement methodology](04-measurement-methodology.md) | Environment control and exact-oracle timing for SCC, dominators/post-dominators, condensation analytics, arena construction, bounded and batched VF2, serial/parallel CFG/DFG/PDG, liveness, loop induction, DSM, WL, centrality, community, cohesion, spectral, cycle, motif, GED, function clones, clone/feasibility, API-usage/temporal, CWE-template, program-composition, IFDS/IDE, and taint-summary operations | thirty `benches/*.rs` Criterion targets | 43 feature-free groups; three parser-gated groups; a 47th with `algorithm-detection` |
| [05 — SCCP validation](05-sccp-validation.md) | Concrete value soundness, lattice laws, conditional-edge partition, refinement monotonicity, determinism, parser integration | `straight_line_literal_programs_match_the_concrete_interpreter`, SCCP unit/property tests, `sccp_integration` | feature-free / `lang-rust` parser case |
| [06 — Interval validation](06-interval-analysis-validation.md) | Concrete range soundness, hull/widen/narrow laws, loop convergence, narrowing descent, determinism, SCCP cross-check, parser integration | interval unit/property tests, `intervals_integration` | feature-free / `lang-rust` parser case |
| [07 — Loop-induction validation](07-induction-analysis-validation.md) | DFG invariant contract, unique latch-dominating recurrences, checked affine steps, comparison direction, determinism, parser integration, malformed/stack safety | induction unit/property tests, `induction_integration`, robustness harness | feature-free / `serde` / `lang-rust` parser case |
| [08 — Complexity-evidence validation](08-complexity-evidence-validation.md) | Counted-loop polynomial degree/provenance, confidence precedence, exact legacy fallback, storage determinism, parser/default-detector integration | complexity unit/property tests, `complexity_evidence_integration`, robustness harness, gated benchmark oracle | `algorithm-detection` / `lang-rust` parser case |
| [09 — Condensation validation](09-condensation-validation.md) | Weighted critical paths, source levels, width-bounded CG precedence, exact/approximate schedule height, cap/malformed behavior, storage determinism, parser integration | condensation unit/property tests, exhaustive path/schedule oracles, `condensation_integration`, benchmark oracle | feature-free / `serde` / `lang-rust` parser case |
| [10 — DSM validation](10-dsm-validation.md) | Transitive visibility vectors, propagation cost, median roles, largest multi-vertex SCC, cap/malformed/provenance behavior, storage determinism, parser integration | DSM unit/property tests, Floyd–Warshall/mutual-reachability oracles, `dsm_integration`, robustness harness | feature-free / `serde` / `lang-rust` parser case |
| [11 — WL validation](11-wl-validation.md) | Directed-degree labels, stable mixing, isomorphic-relabeling invariance, clone ordering, round cap, malformed behavior, public composition | WL unit/property tests, `wl_integration`, robustness harness, benchmark oracles | feature-free |
| [12 — Centrality validation](12-centrality-validation.md) | Alignment, closed forms, petgraph/naive differentials, Brandes composition, stable-id relabeling, numerical/cap boundaries, non-mutation | centrality unit/property tests, `centrality_integration`, robustness harness, benchmark oracles | feature-free |
| [13 — Community validation](13-community-validation.md) | Planted partitions, independent generalized modularity, true aggregation, exact repeat/permutation bytes, connected refinement, caps, malformed numerics/CSR, parser integration | community unit/property tests, `community_integration`, robustness harness | feature-free / `serde` / `lang-rust` parser case |
| [14 — Cohesion validation](14-cohesion-validation.md) | Independent core/truss peeling, brute cut removal, exhaustive minimum cuts, attack BFS/AUC, exact repeat/permutation, caps, malformed CSR/numerics/orders, stack safety, parser integration | cohesion unit/property tests, `cohesion_integration`, robustness harness, benchmark oracles | feature-free / `serde` / `lang-rust` parser case |
| [15 — Spectral validation](15-spectral-validation.md) | Independent dense Jacobi spectra, graph-family closed forms, exact disconnected certificates, vector/residual invariants, repeat/permutation, caps, malformed options/CSR, stack safety, parser integration | spectral unit/property tests, `spectral_integration`, robustness harness, benchmark oracles | feature-free / `serde` / `lang-rust` parser case |
| [16 — Cycle-enumeration validation](16-cycle-enumeration-validation.md) | Independent exhaustive permutations, exact cycle/prefix equality, edge/simple/canonical invariants, insertion-order determinism, explicit caps, malformed CSR, stack safety, parser integration | cycle unit/property tests, `cycles_integration`, robustness harness, benchmark oracles | feature-free / `serde` / `lang-rust` parser case |
| [17 — Motif-census validation](17-motif-census-validation.md) | Complete sixteen-class taxonomy, all 64 labeled tricodes, exact generated-graph census, `C(n,3)` conservation, graphlet semantics, insertion-order determinism, explicit budgets, malformed CSR, parser integration | motif unit/property tests, `motifs_integration`, robustness harness, benchmark oracles | feature-free / `serde` / `lang-rust` parser case |
| [18 — GED validation](18-ged-validation.md) | Hungarian optimality, concrete-script upper bound versus exhaustive exact GED, symmetry, normalization, typed multiedges, caps/numerics, stable WL migration, parser integration | GED unit/property tests, `ged_integration`, robustness harness, benchmark oracles | feature-free / `serde` / `lang-rust` parser case |
| [19 — Martin-metrics validation](19-martin-metrics-validation.md) | Distinct external coupling, CPG-backed abstractness, exact instability/distance, strict stable-dependency witnesses, assignment/projection order invariance, malformed values, parser integration | exact three-module oracle, 256-case property, `martin_integration`, robustness harness | feature-free / `serde` / `lang-rust` parser case |
| [20 — Reflexion-model validation](20-reflexion-model-validation.md) | First-prefix mapping, exhaustive verdicts, exact realized pairs/absences, multiset reversal, duplicates, uncovered paths, serialization | exact layered oracle, 256-case property, `reflexion_integration` | feature-free / `serde` |
| [21 — Layering validation](21-layering-validation.md) | Bottom-first longest-path reversal, exact cycle/skip/upward witnesses, one-step descent, determinism, hostile public values, serialization | exact layered/stale-level oracles, 256-case DAG property, `layering_integration` | feature-free / `serde` |
| [22 — Architectural-smell validation](22-architectural-smells-validation.md) | Exact cycle/hub/instability/size findings, clean topology, evidence-bundle rejection, deterministic ordering, serialization | four isolated smell oracles, 256-case property, hostile-value tables, `smells_integration` | feature-free / `serde` |
| [23 — Member-field flow validation](23-member-field-flow-validation.md) | Field/index writes, conservative member-to-field references, gate/idempotency/append-only behavior, storage determinism, Mode-A/Mode-B parity | exact six-edge oracle, ambiguity counterexamples, 256-case property, seven frontend declaration fixtures, public/parser integration | feature-free / `lang-all` |
| [24 — Code-metrics validation](24-code-metrics-validation.md) | Real method-field evidence, exact CK/LCOM1–5, Halstead/cyclomatic/cognitive/MI arithmetic, storage determinism, scoped implementations, hostile selectors/cycles, parser integration | exact hand-class/formula oracles, generated insertion-order property, public and Rust-parser integration | feature-free / `serde` / `lang-rust` |
| [25 — Function-clone validation](25-function-clone-validation.md) | Type-1/2/3/4 fixtures, complete cap errors, malformed-AST termination, byte determinism, stable-ID relabeling, serde and public API | exact taxonomy examples, generated unique-ID permutation property, black-box integration | feature-free / `serde` |
| [26 — Program-composition validation](26-program-composition-validation.md) | Collision-free fresh ids, complete embedded-reference translation, append-only merge, exact self-copy counts, deterministic file scopes, exact/ambiguous/external imports, idempotent topology, cross-file call selection, serialization | sparse/dangling-id oracles, three 256-case generated properties, black-box integration, benchmark oracles | feature-free / `serde` |
| [27 — IFDS/IDE validation](27-ifds-ide-validation.md) | Callee-first summaries, recursive convergence, lattice laws, deterministic insertion order, explicit cap monotonicity, resolver boundaries, malformed/stale SCC rejection, serialization | exact closure examples, four 256-case generated properties, independent BFS oracle, black-box integration, benchmark oracles | feature-free / `serde` |
| [28 — Effect-summary validation](28-effect-summary-validation.md) | Direct and transitive place effects, purity, actual-to-formal remapping, return dependencies/constants, recursion, lattice laws, insertion-order determinism, and visible cap widening | exact examples, three 256-case generated properties, independent purity DFS, black-box integration, serialization | feature-free / `serde` |
| [29 — Taint/nullflow validation](29-taint-nullflow-validation.md) | Data-driven source/sink/sanitizer/external policy, typed value propagation, interprocedural composition, canonical witness validity, maybe-null origins/dereferences, lattice laws, independent reachability, insertion determinism, and visible caps | twelve named scenarios, eight 256-case generated properties, independent BFS oracles, black-box integration, benchmark oracles, serialization | feature-free / `serde` |
| [30 — SDG slicing validation](30-sdg-slicing-validation.md) | Labeled summary construction, append-only idempotency, backward/forward two-pass context discipline, deterministic caps, and legacy PDG preservation | exact two-caller oracle, three 256-case generated properties, black-box effect-to-slice integration, serialization | feature-free / `serde` |
| [31 — Channel-topology validation](31-channel-topology-validation.md) | Frontend message normalization, statement-precise definition identity, orphan/blocked/linearity predicates, non-consuming peek/persistent semantics, determinism, and advisory immutability | exact pgmcp-lineage examples, three 128-case generated properties, crate-root and real Rholang Mode-B integration, serialization | feature-free / `rholang` / `serde` |
| [32 — Deadlock-candidate validation](32-deadlock-validation.md) | Initially blocked communication cycles, typed read/write lock order, interprocedural acquisition summaries, lexical capture, precise witnesses, determinism, and advisory immutability | direct and interprocedural examples, three 128-case generated properties, crate-root and real Rholang Mode-B integration, serialization | feature-free / `rholang` / `serde` |
| [33 — MHP validation](33-mhp-validation.md) | Direct-or-wrapped parallel operands, cross-operand execution-site pairs, nested-region composition, function boundaries, canonical symmetry, determinism, visible caps, and immutability | exact examples, one 128-case generated Cartesian-product property, crate-root and real Rholang Mode-B integration, serialization | feature-free / `rholang` / `serde` |
| [34 — Session-sketch validation](34-session-sketch-validation.md) | Definition-resolved endpoint traces, shallow payload typing, binary linear duality, three-valued mismatch/uncertainty semantics, determinism, and immutability | exact examples, one 128-case exact-dual property, crate-root typed oracle and real Rholang Mode-B integration, serialization | `rholang` / `serde` |
| [35 — Flat property-value validation](35-property-value-validation.md) | Logical-view equivalence, CPV1 canonical bytes, typed independent caps, malformed rejection, and constant native call depth | scalar/golden/adversarial examples, two 256-case properties, depth-100,000 public lifecycle/codec/Serde gates, pgmcp cache integration | feature-free / `serde` |
| [36 — Shared expression-fact arena validation](36-expression-fact-arena-validation.md) | Shared-arena structural equivalence, exact class identity, CEK1 canonical bytes, typed caps, malformed rejection, linear physical ownership, and constant native call depth | operand/sharing/cycle examples, generated comparison/hash/round-trip property, adversarial bytes, 100,000-level key and public fact-universe gates | feature-free / `serde` |
| [37 — Stack-safe CFG extraction validation](37-cfg-extraction-machine-validation.md) | Exact former-equation preservation, handler/config completeness, active-path malformed semantics, insertion-order determinism, idempotency, and constant native call depth | comprehensive frozen oracle; five 192-case campaigns; cycle/share/config examples; public parser-built depth-100,000/256 KiB gate | feature-free / `lang-rust` parser case |
| [38 — Typestate validation](38-typestate-validation.md) | Finite property validation, allocation/alias identity, exact forbidden transitions, may-joins, canonical witnesses, deterministic insertion order, explicit cap monotonicity, and constant native call depth | fourteen examples/properties plus serde; three 256-case campaigns; public hand/parser/depth-100,000 integration | feature-free / `serde` / `lang-rust` |
| [39 — Bug-detector validation](39-bug-detector-validation.md) | Non-accepting leak terminals, empty scoped reaching definitions, finite exception summaries, lexical handler dispatch, root uncaught witnesses, dead catches, typed incompleteness, and constant native call depth | 21 feature-free plus three serde tests; three independent generated oracles; public aggregate/Rust/Java/depth-100,000 integration | feature-free / `serde` / `lang-rust` / `lang-java` |
| [40 — Population-anomaly validation](40-population-anomaly-validation.md) | Exact missing-check, call-order, and return-check support; semantic fingerprints; peer scopes; typed incompleteness; determinism; and constant native call depth | exact 4/5 and 9/10 examples, two generated population oracles, public aggregate/Rust/depth-100,000 integration, serde regressions | feature-free / `serde` / `lang-rust` |
| [41 — Clone and feasibility validation](41-clone-feasibility-validation.md) | Exact rename/guard witnesses, pair-local admission, Boolean path partitions, SCCP/interval proof composition, typed incompleteness, determinism, and constant native call depth | 128 alignment/insertion and 256 exhaustive integer-oracle cases, public/parser integration, serde, depth 100,000, Criterion smoke | feature-free / `serde` / `lang-rust` |
| [42 — API-usage and temporal validation](42-api-usage-temporal-validation.md) | Exact strict-majority AUG fragments, usage deviations, named temporal findings, qualification, determinism, and constant native call depth | fourteen examples, two 128-case properties, public/parser integration, serde, depth 100,000/64 KiB, oracle-guarded Criterion target | feature-free / `serde` / `lang-rust` |
| [43 — Declarative CWE-template validation](43-cwe-template-validation.md) | Exact ten-template registry, structural/semantic intersection, vulnerable/clean pairs, typed ambiguity and caps, determinism, and constant native call depth | twelve examples plus serde, one 128-case property, public/parser integration, depth 100,000/64 KiB, oracle-guarded Criterion target | feature-free / `serde` / `lang-rust` |
| [44 — Cryptographic API-misuse validation](44-cryptographic-api-misuse-validation.md) | CrySL-style call order, parameter constraints, literal-origin taint, cap qualification, deterministic aggregation, and stack-safe provenance | eight focused examples including serde and caps, one 128-case property, public/parser integration, depth 100,000/64 KiB | feature-free / `serde` / `lang-rust` |
| [45 — Secret-flow and access-policy validation](45-security-policy-validation.md) | Entropy/name qualification, positional source-to-sink flow, exact authN/authZ guard populations, qualification, determinism, and constant native call depth | paired examples, three 128-case properties, public/parser integration, serde, two depth-100,000/64 KiB gates | feature-free / `serde` / `lang-rust` |
| [46 — Vulnerability-extrapolation validation](46-vulnerability-extrapolation-validation.md) | Shared-vocabulary WL cosine, deterministic bounded ranking, relaxed VF2 qualification, complete boundary evidence, and constant native call depth | planted near duplicate, 256/128-case properties, public/parser integration, serde, depth-100,000/64 KiB gate | feature-free / `serde` / `lang-rust` |
| [47 — Virtual-call type-refinement validation](47-type-refinement-validation.md) | Exact represented CHA/RTA/VTA target lattice, inherited/default dispatch, deterministic insertion behavior, and constant native call depth | exact three-level hierarchy, 128-case property, public C1 integration, three parser allocation frontends, serde, depth-100,000/256 KiB gate | feature-free / `serde` / `lang-javascript` / `lang-typescript` / `lang-java` |
| [48 — Race and atomicity validation](48-concurrency-bug-validation.md) | Exact represented shared-location conflicts, lock-mode exclusion, RMW release/reacquire gaps, hostile MHP normalization, determinism, and constant native call depth | conflict/lock/gap examples, two 128-case insertion-order properties, public composition, serde, depth-100,000/256 KiB gate | feature-free / `serde` |
| [49 — Deterministic ML-export validation](49-ml-export-validation.md) | Exact 69-column row alignment, all 47 typed edge classes, complete PDG localization, reconstructable AST paths, determinism, and constant native call depth | exact graph/path/malformed/cap examples, one 128-case insertion-order property, public composition, serde, depth-100,000/256 KiB gate | feature-free / `serde` |
| [50 — Deterministic CPG-diff validation](50-cpg-diff-validation.md) | Exact aligned node/typed-edge scripts, translated embedded ids, PDG-sensitive classification, deterministic multiplicity, complete budgets, immutability, and constant native call depth | nine focused cases, one 128-case insertion-order property, public/parser-PDG composition, serde, depth-100,000/256 KiB gate | feature-free / `serde` / `lang-rust` |
| [51 — Lazy exact-edge projection-cache validation](51-projection-cache-validation.md) | Direct-filter equality, insertion-order determinism, mutation invalidation, serde omission, warm traversal/allocator effects, honest cold/peak costs, and constant native call depth | two 256-case properties, parser/call/PDG integration, exact mutation/persistence examples, Criterion corpus/traversal controls, headless DHAT/Massif/perf, depth-100,000 and degree-99,999/256 KiB gate | feature-free / `serde` / `lang-rust` |
| [52 — Dense stack-safe VF2 validation](52-dense-vf2-validation.md) | Exhaustive-oracle equality, canonical sequence/caps, non-induced typed-multigraph semantics, dense ordered-set equivalence, isolated timing/allocation effects, and constant native call depth | 128-case exhaustive/metamorphic campaigns, recursive differential oracle, `BTreeSet` model, exact examples, parser integration, Criterion/corpus/headless profiles, depth-100,000/256 KiB gate | feature-free / `lang-rust` |
| [53 — Borrowed-CSR dominator validation](53-dominator-optimization-validation.md) | Exact dominance/frontier/post-dominance relations, insertion determinism, borrowed-overlay performance, PDG consumer effect, and constant native call depth | 8,329 exhaustive graphs, four 256-case properties, petgraph/former-PDG/removal oracles, parser integration, Criterion/headless profiles, depth-100,000/256 KiB gate | feature-free / `lang-rust` |
| [54 — Deterministic parallel-analysis validation](54-parallel-analysis-validation.md) | Exact serial/parallel CFG, DFG, PDG, and ordered VF2-batch equality; worker-count invariance; scaling; allocation and counter effects; constant native call depth | four 128-case properties, 1/2/4/8/16/32-worker integration, serde-byte equality, parser integration, Criterion/headless profiles, depth-100,000/256 KiB gate | feature-free / `serde` / `lang-rust` |
| [55 — Build-local string-storage validation](55-string-storage-validation.md) | One allocation per distinct mapped value within a build; unchanged contents/clone/Serde behavior; measured live-memory and hasher effects; constant native call depth | 256 generated ownership cases, Unicode/large examples, public clone/Serde/parser integration, real-source inventory, Criterion/headless profiles, depth-100,000/256 KiB gate | feature-free pool / `serde` / `lang-rust` integration |
| [56 — Reusable WL-buffer validation](56-wl-buffer-reuse-validation.md) | Exact committed-predecessor compatibility; deterministic similarity laws; measured allocation/latency effects; rejected hasher/zero-fill prototypes; constant native call depth | two 256-case properties, independent fresh-allocation oracle, exact topology/parser cases, Criterion/headless profiles, depth/wide-100,000/256 KiB gate | feature-free / `lang-rust` parser integration |
| [57 — Function-analysis cache validation](57-function-analysis-cache-validation.md) | Exact cold/hit/edit equivalence, collision safety, deterministic eviction, represented dependency bypass, measured reuse/overhead, and constant native call depth | exact examples, 32 generated revisions, Mode-A/Mode-B parser integration, 100,000-node deep/wide 256 KiB gates, Criterion/perf/DHAT/Massif/RSS evidence | feature-free core / `serde` / `lang-rust` parser integration |
| [58 — Runtime-dispatched bit-set validation](58-runtime-dispatched-bitset-validation.md) | Scalar/selected lattice equality, safe runtime AVX2 admission, exact tails/change flags, measured crossover, portable fallback, and constant native call depth | arbitrary-vector property differential, named width/tail examples, parser May/Must integration, Miri, AddressSanitizer, wasm check, Criterion/perf/DHAT/RSS, 100,000-wide/deep 256 KiB gates | feature-free core / `lang-rust` parser integration / x86-64 AVX2 profile |

## 5. What is proven, and what is advisory

Scientific honesty requires separating the two:

- **Proven (guarded by the tests above).** The CPG's structural invariants; deterministic graph projection; immediate dominance (differentially checked against `petgraph`); post-dominance and control-dependence equivalence with the former production oracle; natural back edges and loop bodies (checked against independent brute-force dominance and closure oracles); exact SCC partitioning and condensation acyclicity (differentially checked against `petgraph`'s Tarjan implementation); condensation weighted paths (exhaustive path oracle), source levels, CG coverage/precedence/width, and exact/approximate unit-schedule height (exhaustive subset-schedule oracle); liveness fixed points (checked against an independent Gauss–Seidel solver); available/very-busy Must states (checked by exhaustive entry/exit path enumeration on generated acyclic CFGs); SCCP constants on the generated `StmtSpec` language (checked by a concrete mini-interpreter), conditional-edge partitions, and deterministic insertion permutations; interval membership on generated straight-line and bounded-loop programs, widening/narrowing laws, convergence caps, SCCP singleton containment, and deterministic reports; loop-invariant DFG contracts, checked additive/affine IV steps, direction compatibility, insertion-order determinism, and conservative rejection cases; the reaching-definition semantics on the audited corpus; exact field/index write directions and uniquely evidenced `self`/`this` field references; and VF2's completeness on the discriminating diamond. These are deterministic and regression-locked.
  DSM visibility vectors and propagation cost are independently checked by
  Floyd–Warshall closure; largest cyclic groups are checked from mutual
  reachability; insertion/duplicate invariance and malformed/provenance states
  are regression-locked as well.
  Projection-WL label multisets and logical clone partitions are invariant
  under 256 generated isomorphic relabelings, reordered input, and duplicate
  input edges; fixed vectors lock the mixer and boundary behavior. A second
  256-case campaign and independent fresh-allocation predecessor additionally
  lock H12's reused buffers to exact public bits and similarity laws.
  Centrality vectors are aligned, exact-repeat deterministic, and checked on
  analytic fixtures; directed Brandes agrees with naive simple-path
  enumeration and composes across source partitions.
  Community assignments are exact-repeat and serialization-byte deterministic,
  their scores agree with an independent generalized-modularity formula, and
  every refined group passes an independent induced-connectivity traversal.
  Cohesion core and truss labels agree with independent fixed-point peelers;
  articulation/bridge output agrees with brute deletion, minimum-cut weight
  with exhaustive bipartitions, and every attack trace with independent BFS.
- **Advisory (policy-sensitive by design).** The available/very-busy states are
  exact for the modeled structural expressions and CFG, while their CSE/PRE
  transformation candidates still require language side-effect policy.
  Loop-induction `hoistable` and `Counted` evidence likewise requires
  language-aware transformation and termination policy.
  Evidence-aware complexity deterministically composes that structural proof
  material and preserves exact fallback identity; its $`O(n^d)`$ interpretation
  and confidence remain an advisory cost model rather than a symbolic bound.
  Louvain likewise returns one bounded deterministic modularity optimum, not a
  globally optimal or semantically authoritative architecture partition.
  [Design-pattern detection](../GLOSSARY.md#design-pattern),
  [algorithm-family](../GLOSSARY.md#algorithm-family) recognition, and
  [GNN](../GLOSSARY.md#graph-neural-network-gnn) embeddings are heuristics: they
  score likelihood, they do not prove identity. Treat all such evidence at its
  documented boundary rather than as ground truth.
  WL structural clone classes are likewise candidate evidence because
  one-dimensional refinement and a finite digest are not an exact isomorphism
  proof.
  Function clone classes are exact for their documented fingerprint,
  multiset, and relaxed-PDG relations, but those relations—and especially
  missing PDG evidence—do not establish behavioral equivalence or authorize
  automatic extraction.
  Centrality is also advisory: a structural score does not prove runtime
  importance, causal defect risk, or architectural quality.
  Cohesion and robustness output is similarly structural evidence: a bridge,
  light cut, core/truss value, or attack area does not itself prove a runtime
  failure or authorize a refactoring.
  Spectral connectivity is likewise qualified evidence: a bounded Fiedler
  approximation or sign split does not certify an exact cut or architectural
  boundary.
  Motif counts exactly classify the admitted projection topology, but their
  architectural meaning remains advisory and projection-policy dependent.
  An uncapped GED value is proven to price one concrete edit script, but its
  bipartite correspondence is heuristic and `NodeKindTag` is coarse; a capped
  compatibility result is not a GED upper bound at all.
  A CPG diff is exact relative to that selected heuristic correspondence and
  the represented node/edge payload. `BehaviorChanging` proves only a PDG-edge
  edit in the script, while `Refactoring` proves no such represented edit—not
  language-semantic equivalence.
  Martin coupling and scalar arithmetic are exact for the supplied projection,
  assignment, and CPG type taxonomy; the boundary policy, dependency semantics,
  and resulting SDP recommendation remain advisory.
  Reflexion categories are exact for the declared prefix policy and observed
  path edges; the policy, dependency extractor, and remediation remain advisory.
  Architectural-layer indices and violation categories are exact for the
  supplied SCC/level values; dependency extraction, intended boundaries, and
  remediation remain advisory.
  Architectural-smell classification and numerical evidence are exact for the
  validated projection/SCC/assignment/Martin snapshot and options; whether a
  finding is harmful or warrants refactoring remains advisory.
  MHP pairs are exact for the represented typed parallel AST up to the visible
  output cap; runtime overlap, shared-memory conflict, and race severity remain
  advisory questions outside this relation.
  Race and atomicity candidates are exact for the represented typed access,
  exact declaration, lockset, lexical-order, and supplied MHP evidence; runtime
  reachability, aliases, atomics, memory-model behavior, defect severity, and
  safe remediation remain advisory.
  ML rows, typed edge tuples, complete admitted localization, and AST paths are
  exact for the represented CPG and configured filters/bounds; downstream
  labels, train/test isolation, feature utility, model accuracy, privacy, and
  security interpretation remain outside the export claim.
  Session-sketch verdicts are exact for the represented ordered shallow endpoint
  evidence; dynamic endpoint instances, operational reductions, recursive or
  multiparty protocols, progress, and deadlock freedom remain advisory or out of
  scope.
  Missing-check and deviant-behavior fractions and findings are exact for their
  represented catalog, identity, peer, CFG, DFG, and PDG evidence; a dominant
  convention is not a specification, and semantic correctness, population
  integrity, path feasibility, and security impact remain advisory.
  Clone rename/guard witnesses and SCCP/interval feasibility findings are exact
  for their admitted pair, AST, CFG, DFG, operator, and resource evidence;
  developer intent, complete runtime semantics, defect impact, and remediation
  safety remain advisory.
  Centered AUGs, exact fragment support, and deviations are exact for admitted
  call identity, AST, CFG, DFG, PDG, and configured population evidence; a
  dominant convention is not a specification. Temporal findings are exact for
  the caller FSM and represented typestate evidence; neither path authorizes
  repair or proves all runtime executions.
  CWE structural embeddings and primitive-specific evidence are exact for the
  admitted graph, templates, role catalogs, and bounds; exploitability,
  severity, frontend completeness, and unmodeled weakness families remain
  advisory.
  CHA/RTA/VTA candidate sets are exact for represented nominal, hierarchy,
  allocation, receiver, and DFG evidence under the selected closed-world mode;
  runtime loading, reflection, omitted libraries, and full points-to semantics
  remain outside that result.
- **Empirically bounded.** Performance results apply only to a target's named
  fixture, size, and controlled environment. Deterministic CPG differencing and
  GNN propagation remain measurement gaps; no result
  from the thirty active targets (or forty-seven fully enabled operation groups) may be
  generalized to them. Martin metrics, reflexion conformance, architectural
  layering, and architectural-smell detection have no dedicated targets, so
  their validation supports correctness but makes no
  timing claim.

## 6. Reproducing the experiments

The suite is the executable form of this documentation. To re-run every corroborating experiment:

```text
# Maximal supported correctness matrix:
cargo test --features "full rholang metta"

# The reaching-defs and PDG end-to-end corpus (needs the Rust grammar):
cargo test --features lang-rust reaching       # dfg.rs scenarios
cargo test --features lang-rust                 # + pdg.rs parsed tests

# The feature-free graph-algebra and VF2 evidence (no grammar needed):
cargo test vf2
cargo test def_use_backward_slice

# One oracle-guarded controlled benchmark:
./scripts/bench.sh dataflow_liveness

# Evidence-aware complexity public/parser acceptance:
cargo test --no-default-features --features "algorithm-detection lang-rust" \
  --test complexity_evidence_integration
```

Each subsequent page names its tests, quotes the exact assertions that decide the hypothesis, and cross-links the theory (`../theory/`) and builder-component (`../components/builder/`) pages that explain the mechanism under test.

## References

1. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and Discovering Vulnerabilities with Code Property Graphs.* 2014 IEEE Symposium on Security and Privacy. DOI: [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44)
