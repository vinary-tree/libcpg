# Diagram catalog

Every figure in the `libcpg` documentation is a **committed source** (`.puml` for PlantUML, `.dot` for Graphviz) rendered to a **committed `.svg`** that the Markdown pages embed. Sources are version-controlled so figures are reproducible and reviewable; SVGs are committed so they render on GitHub (which does not render PlantUML/Graphviz inline). This directory holds all 151 figures plus the shared theme.

## Conventions

- **Theme.** Every `.puml` begins with `!include _libcpg-theme.iuml`, which fixes the font, white background, no-shadow, and arrow style so all figures read as one system. Graphviz `.dot` files reuse the same palette by literal hex.
- **Colour → concept palette** (fill / line):

  | Concept | Fill | Line | | Concept | Fill | Line |
  |---|---|---|---|---|---|---|
  | AST layer | `#DBE4FF` | `#4C6EF5` | | Type / inheritance | `#E9ECEF` | `#868E96` |
  | CFG layer | `#D3F9D8` | `#2F9E44` | | Pattern match / highlight | `#FFE3E3` | `#E03131` |
  | DFG layer | `#FFF3BF` | `#F08C00` | | GNN / embedding | `#FCC2D7` | `#C2255C` |
  | PDG layer | `#E5DBFF` | `#7048E8` | | Import / scope | `#FFE8CC` | `#E8590C` |
  | Call graph | `#C3FAE8` | `#0CA678` | | Generic node | `#F1F3F5` | `#495057` |

- **Colouring syntax** (PlantUML build quirks pinned): activity nodes colour with a *trailing* stereotype — `:label;<<#HEX>>` (never `#HEX:label;`); classes/states/components/participants use a `#HEX` background; mindmap nodes use `*[#HEX] text`. Do not use `skinparam Padding` (deprecated). Math in labels uses `<latex>…</latex>` (JLaTeXMath).
- **Legends.** Every figure carries a self-contained legend mapping its colours to concepts.

## Rendering & the render gate

```bash
./scripts/verify-diagrams.sh
```

The gate sets `JAVA_TOOL_OPTIONS=-Djava.awt.headless=true` and clears `DISPLAY`,
so PlantUML cannot open a window. It copies all sources and the shared theme to
two disk-backed directories under `target/verification`, renders both passes
with mandatory `-nometadata`, and requires pass one, pass two, and the committed
SVG to be byte-identical. It also renders every `.dot` file with Graphviz and
removes only its own checked temporary directory.

Toolchain used for the current committed set: the system PlantUML distribution
(JLaTeXMath-enabled; its package reports placeholder version fields) and
Graphviz `dot` 15.1.1. Reviewers must record tool versions whenever a renderer
upgrade changes bytes. `scripts/verify-docs.sh` additionally requires exact
source/SVG pairing in both directions, one catalog entry per source, the shared
PlantUML theme, explicit semantic color, no error/deprecation marker, and an SVG
size of at least 1 KiB.

## Figure index

Grouped by subject. Each row: the source (its `.svg` sibling is embedded by the listed pages).

### Core & architecture
| Source | Type | Depicts | Embedded by |
|---|---|---|---|
| `cpg-overlay.dot` | graph | AST⊕CFG⊕DFG overlays on one shared node set (hero figure) | README, docs/README, theory/00–01, architecture/graph-data-model, api/graph-reference, components/graph/overview, design/0001 |
| `cpg-vs-traditional.dot` | graph | Three separate views vs one unified CPG | theory/00, architecture/overview |
| `module-architecture.puml` | component | Crate modules, always-on projections/block-CFG/data-flow/liveness/induction/reachability/dominator/loop/SCC/condensation/layering/DSM/Martin/smell/WL/centrality/community/cohesion/spectral/cycle/motif analysis, and feature gates | architecture/overview, design/00 |
| `construction-pipeline.puml` | activity | parse → AST → CFG → optional definite-call resolution → DFG | architecture/data-flow, components/builder/overview, api/builder-reference, usage/01 |
| `function-analysis-cache-lifecycle.puml` | activity/data flow | caller-owned cache → exact key comparison → local plan replay or fresh planning → ordered emission → bounded deterministic eviction | architecture/data-flow, components/builder/overview, design/0062, theory/46, usage/31 |
| `function-analysis-cache-boundary.puml` | component/evidence boundary | admitted function-local key dimensions and plans versus cross-boundary evidence that forces exact recomputation | architecture/data-flow, components/builder/incremental-analysis-cache, design/0062, security/21 |
| `build-local-string-pool.puml` | activity/ownership flow | Mode-A/Mode-B tree → explicit AST work stack → per-node mapped-string canonicalization → pool drop → unchanged public CPG | architecture/language-frontends, design/0060, scientific/55 |
| `analysis-pipeline.puml` | activity | CPG → blocks/data-flow/liveness/induction/reachability/dominance/loops/SCC/condensation/layering/DSM/Martin/smells/WL/centrality/community/cohesion/spectral/cycles/motifs, PDG/slice, VF2/GoF, algorithm, GNN | architecture/data-flow |
| `feature-flag-map.puml` | mindmap | Cargo feature taxonomy (`default = []`) | design/0005, engineering/01, usage/00 |

### Graph data model
| Source | Type | Depicts | Embedded by |
|---|---|---|---|
| `node-kind-taxonomy.puml` | mindmap | The 45 `CpgNodeKind` variants by group | theory/01, architecture/graph-data-model, api/graph-reference, components/graph/nodes |
| `edge-kind-taxonomy.puml` | mindmap | `CpgEdgeKind` grouped, including CFG/DFG/Heap/NameFlow/RewriteDep sub-kinds | theory/01, architecture/graph-data-model, api/graph-reference, components/graph/edges |
| `projection-csr.puml` | component | CPG overlay selection and deterministic forward/reverse CSR construction | components/graph/projections |
| `projection-cache-lifecycle.puml` | component/lifecycle flow | first exact-edge read → deterministic cached CSR rows → borrowed views → mutation invalidation/rebuild | architecture/graph-data-model, design/0056 |
| `centrality-pipeline.puml` | component/algorithm flow | validated projection → bounded directed/undirected centrality kernels → aligned node/edge scores | components/graph/centrality-analysis, theory/12, design/0017 |
| `community-pipeline.puml` | component/algorithm flow | validated projection → deterministic local moving and bounded aggregation → canonical assignment → optional connected refinement | components/graph/community-detection, theory/13, design/0018 |
| `cohesion-pipeline.puml` | component/algorithm flow | validated projection → simple topology or weighted matrix → peeling, low-link, cut, correlation, and attack kernels → canonical results | components/graph/cohesion-analysis, theory/14, design/0019 |
| `spectral-pipeline.puml` | component/algorithm flow | validated projection → simple topology → exact disconnected certificate or bounded sparse Fiedler iteration → aligned eigenpair/partition evidence | components/graph/spectral-analysis, theory/15, design/0020 |
| `cycle-enumeration-pipeline.puml` | activity/algorithm flow | validated directed projection → SCC pruning → bounded canonical DFS → complete result or first-omission witness | components/graph/cycle-analysis, theory/16, design/0021 |
| `motif-census-pipeline.puml` | activity/algorithm flow | validated directed projection → bounded bit rows → complete 64-tricode classification and selected graphlets → complete fixed-array census | components/graph/motif-analysis, theory/17, design/0022 |
| `martin-metrics-pipeline.puml` | activity/algorithm flow | explicit module assignment + validated dependency projection + CPG type declarations → coupling/abstractness → instability/distance + witnessed SDP violations | components/graph/martin-metrics, theory/18, design/0024, scientific/19 |
| `reflexion-model-pipeline.puml` | component/decision flow | ordered path-layer policy + observed dependency multiset → convergence/divergence/unlayered evidence + realized pairs and absences | components/graph/reflexion-model, theory/19, design/0025, scientific/20 |
| `layering-analysis-pipeline.puml` | component/decision flow | SCC condensation + longest-path metrics → validated bottom-first layers + cyclic/skip/upward witnesses | components/graph/layering-analysis, theory/20, design/0026, scientific/21 |
| `architecture-smells-pipeline.puml` | component/decision flow | projection + exact SCC + explicit module assignment + Martin metrics → validation → four advisory smell families + canonical evidence | components/graph/architecture-smells, theory/21, design/0027, scientific/22 |
| `code-metrics-pipeline.puml` | activity/evidence flow | type/function selection + AST/CFG/DFG/call evidence → method–field graph, CK/LCOM1–5, Halstead, complexity, and maintainability reports | components/graph/code-metrics, theory/22, design/0029, scientific/24 |
| `dominator-analysis.puml` | component | Shared CHK engine for forward dominance, reversed-CFG post-dominance, and PDG control edges | components/graph/dominator-analysis |
| `loop-analysis.puml` | activity/component flow | Shared CFG scope and dominance evidence → natural/irreducible regions → strict nesting; builder continuation → `LoopExit` | components/graph/loop-analysis |
| `reachability-analysis.puml` | component/decision flow | Function scope and iterative DFS → reachable/unreachable partition → structural dead branches; builder terminators → disconnected islands | components/graph/reachability-analysis |
| `basic-block-cfg.puml` | component flow | Function CFG → leader rules → sequential blocks → typed block edges and deterministic traversal orders | components/graph/basic-block-cfg |
| `dataflow-framework.puml` | component flow | Block CFG + lattice + facts + loop evidence + convergence controls → deterministic fixed-point solution | components/graph/dataflow-framework |
| `liveness-analysis.puml` | component flow | CPG + block CFG + canonical variable facts → backward-May fixed point → per-node liveness and advisory dead stores | components/graph/liveness-analysis |
| `expression-analysis.puml` | component flow | CPG + canonical expression facts → forward/backward Must fixed points → dominance-backed advisory CSE/PRE evidence | components/graph/expression-analysis |
| `sccp-analysis.puml` | coupled worklist flow | CFG executability + sparse value propagation → constants, dead edges, unreachable nodes, and structural-reachability refinement | components/graph/sccp-analysis |
| `condensation-analytics.puml` | component/algorithm flow | SCC decomposition → validated condensation DAG → Kahn levels, weighted critical path, and optional Coffman–Graham layers | components/graph/condensation-analysis, design/0014 |
| `condensation-layer-orientation.puml` | comparative DAG views | One contracted DAG under source-level and sink-oriented CG layer numbering | components/graph/condensation-analysis, theory/02 |
| `dsm-visibility-pipeline.puml` | component/algorithm flow | Validated projection → repeated BFS visibility and propagation cost + shared SCC cyclic core → immutable reports | components/graph/dsm-analysis, design/0015 |
| `dsm-core-periphery.puml` | comparative quadrant view | Median VFI/VFO classes and the distinct largest multi-vertex SCC | components/graph/dsm-analysis, theory/10, design/0015 |
| `wl-clone-pipeline.puml` | component/algorithm flow | Validated projection → directed-degree seeds → bounded incident refinement → canonical labels → ordered clone classes | components/graph/wl-analysis, theory/11, design/0016 |
| `wl-buffer-reuse.puml` | activity/state/ownership flow | validated CSR → seed matrix → capacity-matched next matrix → iterative refinement/canonicalization → explicit matrix swap → unchanged public labels | theory/11, components/graph/wl-analysis, design/0061, scientific/56 |
| `interval-analysis.puml` | component/fixed-point flow | Block/variable/loop evidence → interval transfer, branch meet, widening, narrowing, and canonical report | components/graph/interval-analysis |
| `interval-expression-machine.puml` | continuation-machine flow | semantic expression → typed enter/unary/binary/exit heap frames → exact interval or conservative top | components/graph/interval-analysis, design/0011, scientific/06 |
| `relational-domain-precision.puml` | precision/component map | concrete states → intervals → octagons → exact polyhedra plus static reduced-product composition | theory/50, components/graph/relational-abstract-domains, design/0067 |
| `polyhedron-projection-machine.puml` | activity/stack-safe algorithm flow | exact rational constraints → deterministic Fourier–Motzkin partition/pair rounds → canonical projection or contradiction | theory/50, components/graph/relational-abstract-domains, scientific/62, security/25 |
| `reduced-product-cycle.puml` | component/invariant cycle | component construction/transfer/join/widen/narrow → static reduction → solver-visible invariant | theory/50, components/graph/relational-abstract-domains, usage/35 |
| `induction-analysis.puml` | component/evidence flow | Semantic AST/CFG/DFG plus loop/dominance evidence → invariants, hoist candidates, induction variables, and counted bounds | components/graph/induction-analysis |
| `call-resolution.puml` | activity | Lexical symbols and syntax evidence crossing the guarded topology boundary | components/graph/call-resolution |
| `program-composition.puml` | component/evidence flow | Per-file identity translation → one program CPG → exact/ambiguous/external import evidence → cross-file resolution | components/graph/program-composition, design/0031, theory/24 |
| `ifds-summary-fixpoint.puml` | component/fixed-point flow | validated call SCCs → callee-first one-pass or recursive bounded summaries → ordered completeness evidence | components/graph/ifds-ide-framework, design/0032, theory/25, scientific/27 |
| `effect-summary-composition.puml` | component/fixed-point flow | typed direct evidence + callee summaries → lawful effect/constant join → complete or visibly capped result | components/graph/effect-summaries, design/0033, theory/26, scientific/28, usage/09 |
| `effect-evaluation-machines.puml` | two-lane pushdown flow | dependency and constant return evaluators → typed heap continuations, active paths, terminal caches, and conservative outputs | components/graph/effect-summaries, design/0033, scientific/28 |
| `biabduction-summary-flow.puml` | component/fixed-point flow | typed heap evidence + call SCCs → local footprints and callee composition → qualified findings or typed overlay | theory/51, components/graph/compositional-heap-analysis, design/0068, usage/36 |
| `heap-lifetime-machine.puml` | finite-state/worklist flow | allocation/live/freed facts through a heap-resident CFG queue → UAF/leak/postcondition evidence | theory/51, components/graph/compositional-heap-analysis, scientific/63 |
| `heap-overlay-boundary.puml` | transaction boundary | ordered summary relations → complete endpoint validation → idempotent append or zero mutation | design/0068, security/26, usage/36 |
| `name-flow-fixed-point.puml` | component/fixed-point flow | fresh binders + typed definitions + reflection + matched payload transfer → qualified NameFlow closure | theory/52, components/graph/rholang-name-flow, scientific/64 |
| `obligation-capability-scc.puml` | constraint/SCC flow | receive-before-send inequalities → channel projection → finite ranks or cyclic witnesses through shared SCCs | theory/52, design/0069, usage/37 |
| `name-flow-overlay-boundary.puml` | transaction boundary | immutable qualified NameFlow/level reports → complete endpoint validation → idempotent append | design/0069, components/graph/rholang-name-flow, security/27 |
| `metta-rewrite-analysis-pipeline.puml` | component/evidence boundary | normalized MeTTa rule nodes → flat term arenas → exact dependency/confluence analyses → qualified immutable evidence or transactional RewriteDep overlay | design/0070, components/graph/metta-rewriting-analysis |
| `dependency-pair-reduction-proof.puml` | proof-obligation/SCC flow | exact dependency pairs → shared iterative SCC decomposition → global weak-rule and cyclic strict-pair obligations → proof or qualified unknown | theory/53 |
| `critical-pair-joinability.puml` | two-frontier state-space flow | renamed-apart overlap → shared-variable branches → bounded symmetric rewrite BFS → common reduct, finite exhaustion, or incompleteness | theory/53 |
| `taint-summary-flow.puml` | component/fixed-point flow | typed CPG evidence + caller-owned catalog → origin-sensitive facts → bounded summaries, witnesses, and qualified findings | components/graph/taint-analysis, design/0034, theory/27, scientific/29, security/02 |
| `taint-witness-sequence.puml` | sequence | source classification, interprocedural propagation, sink matching, and canonical witness emission | components/graph/taint-analysis, usage/10 |
| `sdg-summary-overlay.puml` | component/overlay flow | call, DFG, PDG, and effect evidence → labeled caller-local summary dependences → bounded slice | components/graph/system-dependence-slicing, theory/28, security/03 |
| `sdg-two-pass-slicing.puml` | two-lane phase flow | backward and forward ascent/descent masks, depth state, and context-preserving ordered results | theory/28, design/0035, usage/11, scientific/30 |
| `channel-event-extraction.puml` | component/evidence flow | caller-owned parse → frontend `MsgKind` normalization → statement-local def-use/reference channel identity → ordered events | components/graph/channel-topology, theory/29, security/04 |
| `channel-topology-classification.puml` | component/decision flow | per-channel producer/linear/persistent/peek tallies → orphan, blocked, and consumption-contention evidence | components/graph/channel-topology, theory/29, design/0036 |
| `wait-for-deadlock.puml` | component/evidence flow | ordered channel events → initial waits and producer gate → process projection → cyclic SCC witnesses | components/graph/deadlock-analysis, theory/30, design/0037, security/05 |
| `lock-order-deadlock.puml` | component/fixed-point flow | typed lock calls + resource identity + callee summaries → held-stack order → conflicting resource SCC witnesses | components/graph/deadlock-analysis, theory/30, design/0037, usage/13 |
| `mhp-par-operands.puml` | component/relation flow | typed parallel block → direct operand subtrees → execution sites → canonical bounded symmetric pairs | components/graph/mhp-analysis, theory/31, design/0038, scientific/33, security/06, usage/14 |
| `concurrency-bug-evidence-pipeline.puml` | component/evidence flow | typed access/reference/channel/lock evidence plus canonical MHP → stack-safe site/strand/lockset machine → race and atomicity candidates | architecture/data-flow, components/graph/concurrency-bug-detection, design/0053, theory/43 |
| `atomicity-lock-gap.puml` | sequence | protected read → release gap → represented interferer → reacquire/write, with continuous-lock qualification | design/0053, theory/43, architecture/data-flow, scientific/48 |
| `ml-export-pipeline.puml` | component/evidence flow | immutable typed CPG → stable dense identity, typed edges, AST structure, centrality, complete localization → aligned external-ML vectors | architecture/data-flow, components/graph/ml-export, design/0054, theory/44, usage/27, security/19 |
| `ast-path-context.puml` | syntax/path flow | source-ordered terminals → unique heap-backed parent chains → exact lowest-common-ancestor path and aligned context | components/graph/ml-export, design/0054, theory/44, usage/27, scientific/49 |
| `cpg-diff-pipeline.puml` | two-input component/evidence flow | old/new typed CPGs → bounded symmetric GED alignment → exact node/edge multiset edits → complete immutable `CpgDiff` | architecture/data-flow, components/graph/cpg-diff, design/0055, theory/45, usage/28, scientific/50 |
| `cpg-diff-classification.puml` | decision/activity flow | complete edge edits → represented control/data-dependence test → evidence-bounded `ChangeKind` | components/graph/cpg-diff, theory/45, security/20 |
| `session-duality.puml` | component/decision flow | resolved Rholang events → ordered endpoint traces → conformant, nonconformant, or inconclusive binary duality evidence | components/graph/session-analysis, theory/32, design/0039, scientific/34, security/07, usage/15 |
| `typestate-analysis.puml` | component/evidence flow | caller property + typed AST/CFG/DFG/calls → allocation-site facts → bounded transfer, witnesses, and qualified report | components/graph/typestate-analysis, theory/33, design/0043, scientific/38, security/08 |
| `typestate-property-fsm.puml` | state | allocation, legal use/close, accepting exit, forbidden transitions, and absorbing error for the resource example | theory/33, usage/16 |
| `bug-detector-pipeline.puml` | component/evidence flow | typed CPG plus resource policy → typestate terminal, scoped definition, and exception-summary paths → qualified advisory findings | components/graph/bug-detectors, theory/34, design/0044, scientific/39, security/09, usage/17 |
| `population-anomaly-pipeline.puml` | component/evidence flow | catalog, API identity, call/CFG/DFG/PDG evidence → flat checks and implicit beliefs → exact support decisions, qualified findings, and issues | components/graph/population-anomaly-detectors, theory/35, design/0045, scientific/40, security/10, usage/18 |
| `population-support-oracles.puml` | exact-oracle flow | four guarded of five sensitive sites and nine checked of ten calls → exact 4/5 and 9/10 findings | theory/35, design/0045, scientific/40, usage/18 |
| `clone-feasibility-advisory-pipeline.puml` | two-lane component/evidence flow | exact Type-2/3 pair alignment/GED and SCCP/guard-local intervals → typed clone and branch/condition findings with explicit issues | components/graph/clone-feasibility-bug-detectors, theory/36, design/0046, scientific/41, security/11, usage/19 |
| `boolean-condition-pushdown-machine.puml` | state/pushdown flow | typed Boolean AST + entry ranges → explicit continuation frames, short-circuit partitions, interval meets, live alternatives, contradictions, or issues | components/graph/clone-feasibility-bug-detectors, theory/36, design/0046, scientific/41, security/11, usage/19 |
| `exception-flow-summary.puml` | component/fixed-point flow | typed throws and calls → bounded SCC exception summaries → lexical handler dispatch, uncaught witnesses, dead catches, and qualifications | components/graph/bug-detectors, theory/34, design/0044, scientific/39, security/09, usage/17 |
| `property-value-flat-arena.puml` | component/storage/codec flow | logical properties → postorder node and edge tapes → explicit lifecycle machines → bounded CPV1/Serde persistence | components/graph/nodes, design/0040, scientific/35, usage/05, security/01 |
| `expression-key-flat-arena.puml` | component/storage/identity/codec flow | CPG expressions → explicit postorder extraction → shared node/operand arena → exact structural classes → bounded CEK1/Serde persistence | components/graph/dataflow-framework, components/graph/expression-analysis, design/0041, scientific/36, usage/05, security/01 |
| `cwe-template-pipeline.puml` | component/evidence flow | declarative roles/shapes → bounded stack-safe VF2 sites plus shared taint/null/typestate/interval/literal evidence → qualified immutable CWE report | architecture/data-flow, components/graph/cwe-template-analysis, design/0048, theory/38, security/13 |
| `cwe-evidence-qualification.puml` | activity/decision flow | validated request → bounded structural and semantic evidence → candidate/no-match → complete or explicitly incomplete interpretation | design/0048, theory/38, scientific/43, security/13 |
| `crypto-misuse-pipeline.puml` | component/evidence flow | declarative crypto policy → shared typestate/taint plus explicit argument provenance → canonical advisory findings | architecture/data-flow, components/graph/crypto-misuse-analysis, design/0049, theory/39 |
| `crypto-evidence-qualification.puml` | activity/decision flow | policy validation → three composed evidence branches → complete/advisory or explicitly incomplete interpretation | design/0049, theory/39, scientific/44, security/14 |
| `security-policy-pipeline.puml` | two-lane component/evidence flow | entropy/name-qualified literal taint and exact missing-check populations → secret-flow and missing authN/authZ reports | architecture/data-flow, components/graph/security-policy-analysis, design/0050, theory/40 |
| `security-policy-qualification.puml` | two-lane activity/decision flow | literal qualification and access-guard population inference → complete/advisory or explicitly incomplete interpretation | design/0050, theory/40, scientific/45, security/15 |
| `vulnerability-extrapolation-pipeline.puml` | component/evidence flow | labeled function + PDG projections → joint-vocabulary WL cosine → bounded relaxed VF2 → ranked report and typed issues | architecture/data-flow, components/graph/vulnerability-extrapolation, design/0051, theory/41 |
| `vulnerability-extrapolation-ranking.puml` | activity/decision flow | candidate/cap admission → WL/VF2 scoring → deterministic retained prefix → completeness and manual review | design/0051, theory/41, scientific/46, security/16 |
| `type-refinement-pipeline.puml` | component/evidence flow | immutable CPG + C1 record → typed index → hierarchy/dispatch/allocation/definition worklists → immutable CHA/RTA/VTA resolution | architecture/data-flow, components/graph/type-refinement, design/0052, theory/42 |
| `type-refinement-lattice.puml` | precision-lattice flow | CHA compatible types → RTA allocation intersection → VTA receiver-assignment intersection → zero/singleton/ambiguous targets | design/0052, theory/42, scientific/47, security/17 |

### Control flow, data flow, dependence
| Source | Type | Depicts | Embedded by |
|---|---|---|---|
| `cfg-example.dot` | graph | CFG of a small function with `CfgEdgeKind` labels | theory/02, components/builder/cfg |
| `cfg-control-constructs.dot` | graph | if / while / try CFG shapes | theory/02, components/graph/edges, components/builder/cfg |
| `cfg-extraction-machine.puml` | activity/pushdown flow | function body → typed `Enter`/continuation/`Leave` frames → exact edge emission, lexical context, and one exit set | components/builder/cfg, design/0042, scientific/37, security/01 |
| `reaching-defs-sweep.puml` | activity | AST-ordered reaching-defs sweep (strong/weak/loop) | theory/03, design/0003, components/builder/dfg, scientific/02 |
| `bitset-kernel-dispatch.puml` | activity/decision flow | dense union/intersection → measured width gate → cached runtime AVX2 or portable scalar → exact scalar tail and change flag | components/graph/dataflow-framework, design/0063, architecture/data-flow, scientific/58 |
| `resolver-index-decision.puml` | activity/evidence decision | exact scope/import names → ordered symbol buckets → unique topology or conservative negative result, with measured DAT/DAWG/prefix/fuzzy rejection paths | components/graph/call-resolution, design/0064, security/22, usage/32 |
| `resolver-index-resource-boundary.puml` | component/resource growth | 256 KiB iterative index lifecycles versus query-local dense transducer rows, geometric RSS observations, and the 4 GiB no-swap ceiling | design/0064, scientific/59, security/22 |
| `datalog-export-pipeline.puml` | component/ownership flow | immutable typed CPG → iterative canonical EDB and lossless edge snapshot → in-memory fact buffers → external caller-owned fixed point | components/graph/datalog-export, theory/48, design/0065, usage/33, scientific/60, security/23 |
| `ifc-evidence-pipeline.puml` | two-lane component/evidence flow | immutable PDG/SDG + labels/secrets → product or simple BFS → canonical flow/timing witnesses → advisory review | architecture/data-flow, components/graph/information-flow-control, design/0066, theory/49 |
| `ifc-policy-paths.puml` | directed policy/evidence graph | explicit data flow, implicit control flow, hard declassification cut, and branch/index timing sites | theory/49, security/24 |
| `bitset-crossover-evidence.puml` | experiment/decision flow | scalar control → four/eight-word candidate measurements → rejected tail regression, retained crossover, and scalar subset policy | design/0063, engineering/optimization-ledger, scientific/58 |
| `member-field-flow.puml` | typed topology | member/index reads and writes plus evidence-bounded member-to-field resolution | theory/03, design/0028, components/builder/dfg, components/graph/edges, architecture/data-flow, scientific/23 |
| `def-use-example.dot` | graph | Def-use / reaching-def edges | theory/03, components/graph/edges, components/builder/dfg, scientific/02 |
| `pdg-construction.puml` | activity | PDG build via reverse dominance frontier | theory/04, components/builder/pdg-and-slicing |
| `dominance-frontier.dot` | graph | Control dependence = reverse dominance frontier | theory/04, components/builder/pdg-and-slicing |
| `slice-example.dot` | graph | A backward slice highlighted over the PDG | theory/04, components/builder/pdg-and-slicing, usage/04 |
| `slicing-bfs.puml` | activity | Bounded backward/forward slice BFS | theory/04, components/builder/pdg-and-slicing, usage/04 |

### Patterns & similarity
| Source | Type | Depicts | Embedded by |
|---|---|---|---|
| `vf2-state-machine.puml` | state | VF2 search states + backtracking | theory/05, design/0004, components/patterns/vf2-matching |
| `vf2-dense-state.puml` | component/data flow | lazy dense node metadata → Fenwick ordered sets → cursor-only search PDA → flat exact undo → canonical match | theory/05, design/0057, components/patterns/vf2-matching, scientific/52 |
| `parallel-plan-emit.puml` | component/concurrency flow | immutable indexed CFG/DFG/PDG or VF2 workers → ordered owned plans → one deterministic serial emitter | design/0059, usage/29, scientific/54, builder and VF2 components |
| `vf2-pattern-target.dot` | graph | Pattern vs target with one embedding | theory/05, api/pattern-reference, components/patterns/vf2-matching, scientific/03 |
| `vf2-diamond.dot` | graph | Diamond target, exactly two embeddings | components/patterns/vf2-matching, scientific/03 |
| `similarity-metrics.puml` | mindmap | Jaccard / Cosine / WL / GraphEdit | theory/06, components/patterns/vf2-matching |
| `ged-pipeline.puml` | activity | Budgeted star costs → Hungarian assignment → concrete typed edit scripts → symmetric normalized bound | theory/06, components/patterns/ged-analysis |
| `clone-detection-pipeline.puml` | activity/decision flow | Ordered Type-1/2 AST fingerprints → bounded Type-3 multiset comparison → PDG WL prefilter and relaxed VF2 Type-4 verification → canonical classes | theory/23, design/0030, components/patterns/clone-detection |
| `wl-kernel.dot` | graph | Weisfeiler-Lehman label refinement | theory/06, components/patterns/vf2-matching |
| `pattern-detection-pipeline.puml` | activity | GoF detect: template → relaxed VF2 → confidence | theory/07, design/0004, components/patterns/overview, usage/03 |
| `gof-taxonomy.puml` | mindmap | 23 GoF patterns by category | theory/07, api/pattern-reference, components/patterns/gang-of-four |
| `gof-singleton.puml` | class | Singleton structure & detection signature | theory/07, components/patterns/gang-of-four |
| `gof-factory-method.puml` | class | Factory Method structure | components/patterns/gang-of-four |
| `gof-observer.puml` | class | Observer structure | components/patterns/gang-of-four |
| `gof-strategy.puml` | class | Strategy structure | components/patterns/gang-of-four |
| `gof-command.puml` | class | Command structure | components/patterns/gang-of-four |
| `gof-decorator.puml` | class | Decorator structure | components/patterns/gang-of-four |
| `gof-adapter.puml` | class | Adapter structure | components/patterns/gang-of-four |
| `dpml-flow.puml` | activity | DPML template → pattern graph | components/patterns/dpml |
| `classification-flow.puml` | activity | FeatureVector → ClassificationMode → label | components/patterns/classification |

### Algorithms & complexity
| Source | Type | Depicts | Embedded by |
|---|---|---|---|
| `algorithm-detection-pipeline.puml` | activity | Per-function detection pipeline | theory/08, components/algorithms/overview |
| `algorithm-family-taxonomy.puml` | mindmap | `AlgorithmFamily` heuristic detectors | components/algorithms/families |
| `complexity-ladder.dot` | graph | `ComplexityClass` ordering with Big-O | theory/08, api/pattern-reference, components/algorithms/complexity |
| `complexity-heuristics.puml` | activity | Structure → complexity class (Master Theorem) | theory/08, components/algorithms/complexity |
| `complexity-evidence.puml` | activity/evidence flow | Validated CFG/DFG loop evidence, confidence policy, recursion composition, and exact legacy fallback | theory/08, components/algorithms/complexity, scientific/08 |

### Graph neural network
| Source | Type | Depicts | Embedded by |
|---|---|---|---|
| `gnn-architecture.puml` | component | `CpgGnn` init → layers → embeddings | theory/09, components/gnn/overview |
| `gnn-message-passing.dot` | graph | Mean aggregation over AST/CFG/DFG neighbours | theory/09, components/gnn/message-passing |
| `gnn-receptive-field.dot` | graph | Receptive-field growth per layer | theory/09, components/gnn/overview, components/gnn/message-passing |
| `embedding-space.dot` | graph | Subgraph embeddings & cosine similarity | components/gnn/embeddings |

### Language frontends (incl. F1R3FLY Mode B)
| Source | Type | Depicts | Embedded by |
|---|---|---|---|
| `language-frontend-pipeline.puml` | component | ParserRegistry / Mode-B tree → NodeMapper dispatch | architecture/language-frontends, design/0002, components/builder/node-mapper, engineering/04 |
| `mode-b-sequence.puml` | sequence | `build_from_tree` with a caller-supplied grammar | architecture/language-frontends, design/0002, api/builder-reference, usage/01, usage/06 |
| `rholang-mapping.dot` | graph | Rholang constructs → CPG node kinds | architecture/language-frontends, components/builder/node-mapper, usage/06 |
| `metta-mapping.dot` | graph | MeTTa S-expressions → CPG node kinds | architecture/language-frontends, components/builder/node-mapper, usage/06 |

### Engineering, science, security
| Source | Type | Depicts | Embedded by |
|---|---|---|---|
| `build-equivalence.puml` | activity | `build` ≡ `build_from_tree` node-payload and typed-edge equality | scientific/01, engineering/02 |
| `measurement-methodology.puml` | activity | Rigorous benchmarking method (a documented gap) | scientific/04, engineering/03 |
| `parallel-affinity-sweep.puml` | deployment/measurement map | CCD-balanced physical-core affinity for the 1/2/4/8/16/32-worker scaling sweep | scientific/54 |
| `threat-model-dataflow.puml` | activity | Untrusted input flow & trust boundaries | security/00 |
| `resource-bounds.puml` | mindmap | Construction/analysis caps and validation, including condensation/layering, DSM, Martin metrics, architectural smells, centrality, Louvain, k-truss, minimum-cut, and attack ceilings | security/01 |
| `api-usage-temporal-pipeline.puml` | component/evidence flow | Typed call admission → centered AUGs → exact fragments/deviations alongside named typestate properties → qualified report | architecture/data-flow, design/0047, components/graph/api-usage-protocol-mining, security/12 |
| `centered-api-usage-graph.puml` | directed labeled graph | Control/receiver/argument evidence → anchor API → CFG-reachable later API, with exact 19/20 fragment support | theory/37, design/0047, components/graph/api-usage-protocol-mining, usage/20 |
