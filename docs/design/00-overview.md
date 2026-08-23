# Design Decisions (ADR index)

This directory holds `libcpg`'s **Architecture Decision Records** (ADRs): short,
durable notes that capture *why* a load-bearing design choice was made, not just
*what* the code does. The API reference in [`../api/`](../api/graph-reference.md)
tells you the shapes; the [component guides](../components/) tell you how to use
them; the [theory pillar](../theory/00-overview.md) explains the science. An ADR
answers the remaining question — **"why this and not the obvious alternative?"** —
so that a future reader (or a rewrite from scratch) inherits the reasoning and
does not silently undo a decision that was made for a good reason.

## What an ADR is

An ADR records **one** decision. Following Michael Nygard's original format, each
record here has a fixed skeleton so it can be skimmed:

| Section | Question it answers |
| :--- | :--- |
| **Status** | Is this decision proposed, accepted, superseded? |
| **Context** | What forces (requirements, constraints, prior art) made a decision necessary? |
| **Decision** | What did we choose, stated plainly? |
| **Consequences** | What do we gain and what do we pay — the honest pro/con ledger? |
| **Alternatives considered** | What else was on the table, and why was it rejected? |

The value is in the last two sections: a decision without its rejected
alternatives is folklore, and a decision without its costs is marketing.

## How to read one

1. Start with **Status** and the one-sentence **Decision** to know what you are
   looking at.
2. Read **Context** only if you need to understand the pressure behind it.
3. Consult **Consequences** before you build on the decision — the *Negative*
   column is where the sharp edges live (traversal must filter by edge kind,
   the data-flow sweep over-approximates in branches, relaxed matching trades
   precision for recall, and so on).
4. Follow the cross-links. Each ADR points back into
   [`../theory/`](../theory/00-overview.md) for the formal grounding and into
   [`../architecture/`](../architecture/overview.md) for where the decision
   lives in the module map, and links every term to the
   [glossary](../GLOSSARY.md).

## Index

| ADR | Decision | Status |
| :--- | :--- | :--- |
| [0001 — Unified overlay graph on petgraph](0001-unified-overlay-graph.md) | One shared node set; AST / CFG / DFG / PDG are typed *edge overlays* over it, stored in a `petgraph` `DiGraph`. | Accepted |
| [0002 — Mode B: `build_from_tree`](0002-mode-b-build-from-tree.md) | Accept a caller-parsed tree-sitter tree; keep the `rholang` / `metta` features empty `cfg` toggles; match grammar pins to pgmcp to avoid duplicate C symbols. | Accepted |
| [0003 — AST-ordered reaching definitions](0003-ast-ordered-reaching-defs.md) | Build the DFG from a single AST-ordered, flow-sensitive reaching-definitions sweep rather than SSA or a CFG fixed point. | Accepted |
| [0004 — Relaxed VF2 detection](0004-relaxed-vf2-detection.md) | Detect Gang-of-Four patterns with a *relaxed* (category-level) VF2 matcher plus a completeness-scaled confidence, not strict isomorphism or pure ML. | Accepted |
| [0005 — Feature-flag taxonomy](0005-feature-flag-taxonomy.md) | `default = []`: every grammar and optional analysis family is opt-in via Cargo features, grouped into `lang-*` sets and an umbrella `full`; core graph analyses remain feature-free. | Accepted |
| [0006 — Evidence-bounded call resolution](0006-evidence-bounded-call-resolution.md) | Record every call-resolution outcome out of graph, but materialize call topology only when an approved evidence tier identifies one unique target. | Accepted |
| [0007 — Dominator-based natural-loop forest](0007-dominator-natural-loop-forest.md) | Qualify natural back edges with dominance, expose irreducible SCC regions explicitly, nest by strict body containment, and retype loop-leaving continuations as `LoopExit`. | Accepted |
| [0008 — Id-preserving basic-block CFG view](0008-id-preserving-basic-block-cfg.md) | Partition the reachable function CFG without rewriting CPG nodes; assign blocks by sorted leaders and preserve every typed non-internal edge. | Accepted |
| [0009 — Deterministic monotone data-flow solver](0009-deterministic-monotone-dataflow.md) | Share a bounded forward/backward Kildall worklist over `BlockCfg`, with canonical facts and loop-header widening. | Accepted |
| [0010 — Dual-worklist sparse conditional constant propagation](0010-dual-worklist-sccp.md) | Couple executable CFG edges with sparse AST/DFG value cells; keep proof results out of graph and composable with reachability. | Accepted |
| [0011 — Loop-header interval widening](0011-loop-header-interval-widening.md) | Compute sound integer ranges with pointwise interval environments, header-only widening, bounded narrowing, and typed comparison-edge meets. | Accepted |
| [0012 — DFG-backed loop-induction evidence](0012-dfg-backed-loop-induction-evidence.md) | Compute reaching-definition-backed invariants, unique latch-dominating induction variables, conservative hoist candidates, and direction-compatible trip evidence outside the graph. | Accepted |
| [0013 — Evidence-bounded complexity estimates](0013-evidence-bounded-complexity-estimates.md) | Compose validated CFG loop depth and DFG-backed induction bounds into advisory complexity, with an exact legacy AST fallback. | Accepted |
| [0014 — Deterministic condensation-DAG analytics](0014-deterministic-condensation-analytics.md) | Compute canonical SCC-weighted critical paths and source levels, plus sink-first width-bounded Coffman–Graham layers, without rebuilding graph identity. | Accepted |
| [0015 — Bounded DSM visibility analysis](0015-bounded-dsm-visibility-analysis.md) | Compute transitive visibility and median roles over validated projections, reuse the shared SCC engine for cyclic core, and expose cap/malformed states explicitly. | Accepted |
| [0016 — Stable WL projection labels](0016-stable-wl-projection-labels.md) | Refine directed-degree labels with a fixed mixer, sorted round canonicalization, a hard iteration ceiling, and deterministic stable-id clone classes. | Accepted |
| [0017 — Deterministic bounded centrality](0017-deterministic-bounded-centrality.md) | Port centrality onto dense projections with aligned results, explicit numerical/traversal bounds, stable reductions, and a serial source-partition seam. | Accepted |
| [0018 — Deterministic multi-level community detection](0018-deterministic-multilevel-community-detection.md) | Port Louvain onto validated projections, add deterministic aggregation, canonical tie/label rules, bounded work, and a separate checked connected-component refinement. | Accepted |
| [0019 — Deterministic bounded cohesion analysis](0019-deterministic-bounded-cohesion-analysis.md) | Share one simple-undirected topology across deterministic core/truss/connectivity/assortativity/attack kernels, while bounding and validating weighted Stoer–Wagner separately. | Accepted |
| [0020 — Deterministic bounded spectral analysis](0020-deterministic-bounded-spectral-analysis.md) | Compute combinatorial-Laplacian algebraic connectivity and a stable Fiedler sign split with exact disconnected handling, bounded sparse iteration, and explicit residual evidence. | Accepted |
| [0021 — Deterministic bounded cycle enumeration](0021-deterministic-bounded-cycle-enumeration.md) | Reuse projection SCCs for canonical directed cycle witnesses with hard length/count ceilings and a concrete first-omission boundary. | Accepted |
| [0022 — Complete bounded directed-motif census](0022-deterministic-motif-census.md) | Classify all sixteen directed triads with dense bit rows and return typed errors instead of silently omitted graphlets. | Accepted |
| [0023 — Deterministic bounded bipartite GED](0023-deterministic-bipartite-ged.md) | Select a stable node correspondence with Hungarian assignment, price a concrete typed edit script, and expose the hard-cap fallback. | Accepted |
| [0024 — Explicit Martin module assignments](0024-explicit-martin-module-assignments.md) | Keep dependency topology and module boundaries explicit, count distinct external vertices, derive abstractness from CPG types, and retain stable-dependency witnesses. | Accepted |
| [0025 — Pure path reflexion model](0025-pure-path-reflexion-model.md) | Decouple ordered path-layer conformance from pgmcp configuration and make every convergence, divergence, realization, and absence deterministic. | Accepted |
| [0026 — Longest-path architectural layering](0026-longest-path-architectural-layering.md) | Reverse validated condensation levels into bottom-first architecture and retain honest cyclic, skip-layer, and upward witnesses. | Accepted |
| [0027 — Validated advisory architectural smells](0027-validated-advisory-architectural-smells.md) | Validate projection, SCC, module, and Martin evidence as one snapshot before emitting canonical advisory smell findings. | Accepted |
| [0028 — Evidence-bounded member-to-field resolution](0028-evidence-bounded-member-field-resolution.md) | Emit complex-l-value writes and resolve only uniquely supported `self`/`this` accesses to same-scope field declarations. | Accepted |
| [0029 — Evidence-bounded code metrics](0029-evidence-bounded-code-metrics.md) | Derive CK/LCOM and function metrics from resolved CPG evidence, preserve method-field witnesses, condense inheritance cycles, and align numerical scoring with pgmcp. | Accepted |
| [0030 — Bounded function clone detection](0030-bounded-function-clone-detection.md) | Classify strongest Type-1–4 function relations with stable AST Merkle, multiset, WL, and bounded existing-VF2 evidence. | Accepted |
| [0031 — Deterministic program composition](0031-deterministic-program-composition.md) | Translate every per-file identity into one program CPG and emit import/export topology only for uniquely supported file aliases. | Accepted |
| [0032 — Deterministic bounded IFDS/IDE summaries](0032-deterministic-ifds-ide-summaries.md) | Validate current call topology, schedule callees before callers, and expose bounded recursive summary fixed points as ordered evidence. | Accepted |
| [0033 — Compositional effect summaries](0033-compositional-effect-summaries.md) | Specialize the shared summary solver with typed storage places, transitive purity, return dependencies, and a lawful flat constant lattice. | Accepted |
| [0034 — Data-driven advisory taint](0034-data-driven-advisory-taint.md) | Specialize the shared summary solver with caller-owned source/sink policy, typed value flow, canonical witnesses, maybe-null evidence, and explicit partial-result semantics. | Accepted |
| [0035 — Overlay SDG summary edges and two-pass slicing](0035-system-dependence-summary-slicing.md) | Materialize labeled caller-local parameter-to-return dependences and apply deterministic HRB phase masks with explicit node/depth caps. | Accepted |
| [0036 — Typed channel topology](0036-typed-channel-topology.md) | Preserve language-neutral message semantics at the frontend boundary and classify def-use-resolved channel matching/linearity as ordered out-of-graph evidence. | Accepted |
| [0037 — Typed deadlock witnesses](0037-typed-deadlock-witnesses.md) | Build communication wait-for and interprocedural lock-order projections from typed identity evidence, reuse shared SCCs, and retain statement-precise advisory witnesses. | Accepted |
| [0038 — Typed AST MHP](0038-typed-ast-mhp.md) | Derive bounded canonical may-happen-in-parallel pairs from direct operand subtrees of typed parallel blocks, independent of parser wrapper shape. | Accepted |
| [0039 — Advisory session sketches](0039-advisory-session-sketches.md) | Group definition-resolved Rholang actions into deterministic endpoint traces and classify binary linear duality without guessing through missing evidence. | Accepted |
| [0040 — Flat property-value arena](0040-flat-property-value-arena.md) | Own logical property trees as canonical postorder node and edge tapes, with explicit lifecycle machines and a bounded versioned CPV1 persistence boundary. | Accepted |
| [0041 — Shared flat expression facts](0041-shared-flat-expression-facts.md) | Represent expression roots in one shared postorder arena, deduplicate exact structural classes, and persist standalone keys through bounded canonical CEK1 bytes. | Accepted |
| [0042 — Stack-safe CFG extraction](0042-stack-safe-cfg-extraction.md) | Defunctionalize the complete recursive CFG-handler component into typed heap continuations while preserving its exact edge stream, context lifetime, and malformed-path semantics. | Accepted |
| [0043 — Deterministic typestate analysis](0043-deterministic-typestate-analysis.md) | Validate caller-defined finite-state properties, propagate allocation-site object/state facts over typed CPG evidence, and expose forbidden transitions with bounded canonical witnesses. | Accepted |
| [0044 — Evidence-composed bug detectors](0044-evidence-composed-bug-detectors.md) | Project leak and uninitialized-use evidence from shared facts, normalize typed handler entry, and propagate finite exception summaries through call SCCs without parsing or graph mutation. | Accepted |
| [0045 — Exact population anomaly detectors](0045-exact-population-anomaly-detectors.md) | Infer missing checks and deviant call behavior from canonical CPG evidence, exact peer fractions, and explicit incomplete-result semantics. | Accepted |
| [0046 — Evidence-bounded clone and feasibility advisories](0046-evidence-bounded-clone-and-feasibility-advisories.md) | Diagnose typed divergences in exact low-edit clone pairs and compose SCCP with path-partitioned intervals for qualified branch/condition findings. | Accepted |
| [0047 — Evidence-bounded API-usage and temporal mining](0047-evidence-bounded-api-usage-and-temporal-mining.md) | Build centered typed AUGs, infer exact strict-majority fragments, and reuse typestate for named temporal properties with explicit completeness. | Accepted |
| [0048 — Declarative qualified CWE templates](0048-declarative-qualified-cwe-templates.md) | Intersect bounded VF2 structural sites with shared taint, null-flow, typestate, interval, and literal evidence while retaining explicit completeness. | Accepted |
| [0049 — Composed cryptographic API misuse](0049-composed-cryptographic-api-misuse.md) | Compile CrySL-style call order and positional constraints onto shared typestate, literal-source taint, and stack-safe provenance machines. | Accepted |
| [0050 — Composed secret and access-policy analysis](0050-composed-secret-and-access-policy-analysis.md) | Qualify typed secret literals before shared taint propagation and specialize exact missing-check populations for typed authN/authZ guards. | Accepted |
| [0051 — Evidence-bounded vulnerability extrapolation](0051-evidence-bounded-vulnerability-extrapolation.md) | Rank seed-related function PDGs with joint-vocabulary WL cosine and explicitly bounded relaxed VF2 confirmation. | Accepted |
| [0052 — Closed-world virtual-call type refinement](0052-closed-world-type-refinement.md) | Refine C1 method candidates through immutable CHA/RTA/VTA evidence while preserving typed allocations at the frontend boundary. | Accepted |
| [0053 — Evidence-bounded concurrency bug candidates](0053-evidence-bounded-concurrency-bug-candidates.md) | Compose exact access identity, canonical MHP, and typed locksets into immutable race and RMW-gap witnesses. | Accepted |
| [0054 — Deterministic CPG ML exports](0054-deterministic-cpg-ml-exports.md) | Canonicalize typed CPG tensors and exact AST terminal paths without an ML runtime, source reparsing, graph mutation, or source-depth recursion. | Accepted |
| [0055 — Deterministic dependence-aware CPG diff](0055-deterministic-dependence-aware-cpg-diff.md) | Reuse bounded GED correspondence for exact canonical node/edge edit scripts and classify only represented PDG-edge changes as behavior-changing. | Accepted |
| [0056 — Lazy deterministic exact-edge projection cache](0056-lazy-deterministic-edge-projection-cache.md) | Cache exact edge indices in deterministic forward/reverse/endpoint CSR rows, invalidate on mutation, and retain `GraphProjection` as a separate owned abstraction. | Accepted |
| [0057 — Dense stack-safe VF2 state](0057-dense-stack-safe-vf2-state.md) | Replace hash-owned candidates with lazy dense graph metadata, Fenwick-ordered sets, flat undo logs, cursor-only heap frames, and a sound distinct-degree prune without changing canonical match semantics. | Accepted |
| [0058 — Borrowed-CSR dominator overlay](0058-borrowed-csr-dominator-overlay.md) | Run bounded CHK over borrowed forward/reverse CSR, model post-dominance with a synthetic-root overlay, flatten frontier ownership, and eliminate the duplicate function projection while preserving exact public relations. | Accepted |
| [0059 — Deterministic parallel plan emission](0059-deterministic-parallel-plan-emission.md) | Compute immutable per-function and per-pattern plans with indexed Rayon collection, then retain one serial ordered graph emitter for exact deterministic identities. | Accepted |
| [0060 — Build-local string canonicalization](0060-build-local-string-canonicalization.md) | Canonicalize equal mapper-produced `Arc<str>` contents within one CPG build using a temporary Fx set, without global state or public representation changes. | Accepted |
| [0061 — Reusable WL refinement buffers](0061-reusable-wl-refinement-buffers.md) | Alternate call-local dense label matrices and reuse canonical/incident capacities across bounded rounds while preserving the fixed public labels and constant native call depth. | Accepted |
| [0062 — Bounded exact function-analysis cache](0062-bounded-function-analysis-cache.md) | Reuse canonical function-local CFG/DFG/PDG plans across revisions through caller-owned hard bounds, full-key equality, deterministic LRU eviction, and exact cross-boundary bypasses. | Accepted |
| [0063 — Runtime-dispatched dense bit-set kernels](0063-runtime-dispatched-bitset-kernels.md) | Preserve one portable lattice API while selecting measured AVX2 union/intersection kernels on capable x86/x86-64 hosts, with scalar tails, non-x86 fallback, and scalar early-exit subset. | Accepted |

These sixty-two are not independent. ADR-0001 fixes the substrate that all analyses
write onto; ADR-0002 and ADR-0005 together explain how a language reaches that
substrate and what you must switch on to get there; ADR-0003 and ADR-0004 are
two analyses *over* the substrate (one always-on, one feature-gated) that each
chose an evidence-bounded algorithm over a textbook-maximal one; ADR-0006 defines when
parsed call syntax may become interprocedural graph topology; ADR-0007 combines
the shared CFG scope, dominance, and SCC substrate into natural-loop structure;
ADR-0008 projects that same scope into deterministic blocks without breaking
overlay identity; ADR-0009 supplies the generic fixed-point engine over those
blocks while keeping the AST-ordered DFG builder as a distinct materialization
pass.
ADR-0010 specializes the lattice and deterministic-worklist discipline of
ADR-0009 while using the sparse `DefUse` evidence produced by ADR-0003. Read
ADR-0011 instead instantiates ADR-0009 densely over `BlockCfg` environments and
uses the loop headers established by ADR-0007. Read 0001 first.
ADR-0012 composes ADR-0003's `DefUse` evidence with ADR-0007's loops and the
dominance substrate, while keeping its optimization and trip results advisory.
ADR-0013 consumes ADR-0012 and ADR-0007 from the feature-gated algorithm layer,
preserving the original AST heuristic as an exact compatibility boundary.
ADR-0014 consumes ADR-0001's out-of-graph result discipline and the canonical
SCC condensation substrate, then fixes deterministic tie, orientation, cap, and
malformed-input policy for downstream graph-theory analyses.
ADR-0015 consumes ADR-0001's projection/out-of-graph discipline and the shared
SCC engine, then fixes visibility direction, median thresholds, stable
provenance, malformed-CSR behavior, and the quadratic-work cap.
ADR-0016 consumes that projection discipline, centralizes complete CSR
validation for DSM and WL, and adds a separately bounded structural-clone view.
ADR-0017 consumes the same validated forward/reverse CSR, fixes direction and
weight semantics for the centrality family, and exposes ordered source
partitions rather than introducing a core-owned thread pool.
ADR-0018 consumes the canonical undirected projection view, fixes weighted
modularity and deterministic local-moving semantics, adds true multi-level
aggregation, and keeps connected refinement as an explicit separate guarantee.
ADR-0019 consumes the same validation and projection seam, fixes shared simple-
topology semantics, deterministic peeling/low-link/cut choices, and explicit
edge, node, and attack-step budgets for cohesion and robustness experiments.
ADR-0020 consumes the same simple-projection seam, selects the combinatorial
Laplacian, and fixes exact disconnected behavior, deterministic numerical
choices, a hard iteration cap, and explicit convergence/residual evidence.
ADR-0021 preserves directed topology and reuses the shared SCC substrate to
bound canonical concrete cycle evidence. ADR-0022 also preserves direction,
but fixes a complete tricode taxonomy, dense bounded adjacency, and explicit
all-or-error semantics for combinatorial motif work.
ADR-0023 keeps whole-graph similarity feature-free but replaces a count blend
with a deterministic concrete edit-script upper bound and an explicit capped
compatibility state.
ADR-0024 returns to the projection substrate: it composes caller-selected
dependency direction with a first-class module assignment so Martin metrics
and Stable Dependencies Principle findings remain reproducible and auditable.
ADR-0025 deliberately needs no projection: dependency producers pass path
edges into a pure first-prefix layer policy, keeping configuration conversion
at the application boundary and ordered conformance evidence in the library.
ADR-0026 returns to the SCC condensation substrate and consumes ADR-0014's
longest-path levels. It fixes architectural orientation, one-step descent, and
the evidence boundary created when SCC contraction removes internal edges.
ADR-0027 composes the projection/SCC substrate with ADR-0024's explicit Martin
module boundary. It rejects mismatched evidence bundles before classifying
cycles, two-sided hubs, less-stable dependency shares, and module-size outliers
as deterministic advisory findings.
ADR-0028 returns to the builder seam established by ADR-0003: it adds the
missing complex-l-value writes, indexes declarative fields once, and admits a
member-to-declaration edge only when receiver, owner, lexical scope, type, and
field-name evidence are unique.
ADR-0029 consumes those real field edges together with ADR-0006's resolved call
topology. It retains the method-access-field witnesses behind LCOM, fixes the
class/function metric vocabulary and degenerate cases, and keeps the scorer
feature-free by classifying common CPG node kinds rather than reparsing source.
ADR-0030 composes ADR-0016's fixed WL combiner and labels with ADR-0004's
corrected relaxed VF2 verifier over pure function PDGs, while keeping pair,
PDG-size, and invocation limits observable as typed errors.
ADR-0031 returns to ADR-0001's identity and overlay rules and ADR-0006's
evidence boundary. It translates complete per-file reference sets into one
program id space, retains ordered file scopes, and admits cross-file dependency
and call evidence only after a unique strongest file alias is represented.
ADR-0032 consumes that resolved call topology through the exact SCC substrate.
It validates the decomposition as current, orders callee components first, and
centralizes bounded monotone recursion for IFDS/IDE summary domains.
ADR-0033 instantiates that framework with typed CPG and DFG evidence. It maps
callee parameter effects through actual arguments, uses a worst-purity chain
and flat constant lattice, and widens visibly when a domain-local bound is
reached.
ADR-0034 builds on the same framework but keeps security policy outside the
graph. It classifies typed call paths with a caller-owned catalog, composes
source-to-return and parameter-to-sink jump functions, retains canonical
positive witnesses, and treats maybe-null dereferences as advisory evidence.
ADR-0035 composes those summaries with PDG/DFG overlays using labeled caller-
local edges and two-pass context discipline. ADR-0036 instead extends the
frontend-to-analysis seam: it preserves orthogonal typed concurrency metadata,
resolves stable channel identity from existing definition evidence, and keeps
matching/linearity findings immutable and advisory. ADR-0037 deepens that
evidence into process wait-for and lock-resource order projections, reuses the
shared SCC engine, and preserves exact sites/ranges for every cyclic witness.
ADR-0038 consumes ADR-0036's typed `Spawn` boundary directly: it treats every
direct AST child as one parallel operand, emits canonical cross-operand
execution-site pairs, excludes nested definitions, and exposes quadratic
truncation instead of hiding it. ADR-0039 reuses ADR-0036's definition-resolved
events to infer shallow endpoint traces, distinguishes proven incompatibility
from incomplete evidence, and remains out of graph. ADR-0040 repairs a lifecycle
property of ADR-0001's metadata side-table: logical property recursion is
projected from flat ownership, and only that recursive field crosses a versioned
canonical inner codec. ADR-0041 applies the same pushdown discipline to
data-flow facts but selects a shared directed acyclic arena, rather than one
strict tree, so all nested expression occurrences remain linear in physical
syntax size while exact structural classes preserve fact identity.
ADR-0042 applies whole-component defunctionalization to CFG construction:
`CfgFrame` values own every suspended handler call, while explicit exit values
and lexical context preserve the observable equations without source-depth
native recursion.
ADR-0043 composes typed call, DFG/reference, canonical CFG, and shared cap
evidence into a feature-free finite typestate may-analysis. It retains
allocation-site identity and graph-depth state in ordered heap structures and
keeps all policy-dependent facts out of graph.
ADR-0044 consumes ADR-0043's terminal evidence, ADR-0003's reaching-definition
relations, ADR-0032's bounded SCC summary solver, and ADR-0042's emitted
exception topology. It keeps syntax normalization in the frontend and composes
three advisory detector families from shared evidence.
ADR-0045 reuses ADR-0034's caller-owned taint catalog and dependence evidence,
ADR-0006's call identity, and deterministic call/CFG projections. It makes
population support an exact rational relation, normalizes checks into flat
postorder tapes, and qualifies every excluded occurrence.
ADR-0046 composes ADR-0030's exact clone pairs, ADR-0023's bounded GED,
ADR-0010's executable-edge facts, and ADR-0011's interval environments. It
keeps pair/condition recursion in explicit heap machines, preserves disjoint
comparison regions, and qualifies every unsupported or bounded proof.
ADR-0047 composes ADR-0006's stable call identity, canonical CFG/DFG/PDG
evidence, ADR-0045's exact population arithmetic, and ADR-0043's finite-state
typestate engine. It admits only complete call-site AUGs, enforces
strict-majority uniqueness, and preserves delegated temporal qualification.
ADR-0048 composes ADR-0004's bounded stack-safe VF2 engine, ADR-0034's
summary-based taint/null flow, ADR-0043's typestate worklist, and ADR-0011's
interval domain. It treats templates as data, intersects structural and
semantic evidence, and preserves every missing, contradictory, or capped fact.
ADR-0052 extends ADR-0006 without weakening its topology boundary: frontend-
normalized allocation metadata, exact nominal identity, subtype/implementation
edges, and reaching definitions feed deterministic heap worklists that return
monotone closed-world candidate sets without graph mutation.
ADR-0053 composes ADR-0038's MHP relation with ADR-0028's exact member-field
identity and ADR-0037's typed lock-resource discipline. The resulting race and
RMW-gap witnesses remain immutable and advisory because the inputs do not prove
a runtime schedule or complete memory model.
ADR-0054 composes ADR-0001's typed identity, ADR-0017's bounded deterministic
centrality, ADR-0004's stack-safe graph discipline, and ADR-0035's dependence
slicing boundary. It retains a frozen numerical schema and exact AST paths for
external ML consumers while keeping training, tensor runtimes, and labels out
of libcpg.
ADR-0055 reuses ADR-0023's deterministic bounded GED correspondence, then
compares exact typed node/edge evidence in aligned coordinates. Its
`BehaviorChanging` label is intentionally confined to represented PDG-edge
edits and therefore never claims language-semantic equivalence.
ADR-0056 optimizes ADR-0001's read boundary with exact edge-index CSR rows,
while preserving ADR-0016's owned normalized `GraphProjection` as a different
semantic layer. Its mutation invalidation and native-adjacency build phase
prevent stale indices and repeated whole-graph rebuilds.
ADR-0057 consumes ADR-0056's stable node/edge indexes for fully lazy dense VF2
metadata. It preserves ADR-0004's canonical non-induced match relation while
moving candidate order, mapping, terminal membership, and exact restoration
into compact heap structures with constant native call depth.
ADR-0058 consumes ADR-0016's owned forward/reverse CSR and ADR-0007's shared
function scope without adding another graph owner. It preserves bounded CHK and
the public dominance/PDG contracts while making the synthetic exit, traversal
continuations, and frontier canonicalization explicit heap data.
ADR-0059 applies the same immutable-view discipline to per-function CFG, DFG,
and PDG planning and to independent VF2 batches. Indexed collection preserves
input order; one serial emitter preserves graph identities, duplicate handling,
and exact serial results without locks or graph clones.
ADR-0060 applies a similarly narrow ownership boundary to mapper-produced
strings: one temporary per-build Fx set collapses equal `Arc<str>` allocations
before graph insertion, then disappears before analysis. It preserves the
public representation while rejecting global atoms, locks, and cross-graph
lifetime coupling.
ADR-0061 applies that ownership discipline inside ADR-0016's bounded WL
refinement: two dense label matrices, a canonical vocabulary, and an incident
buffer retain capacity only within one call. The fixed mixer remains unchanged,
so allocation reuse cannot redefine public numeric ranks.
ADR-0062 extends that ownership discipline across caller-defined revision
lifetimes. Exact canonical bytes and local ordinals permit reuse without
retaining graph identities; boundary crossings deliberately recompute through
the ordinary planners, and deterministic LRU limits both entry and byte growth.
ADR-0063 narrows hardware specialization beneath the same public data-flow
algebra. A measured width threshold and cached runtime capability select direct
AVX2 word chunks, while scalar tails, subset early exit, and non-x86 fallback
preserve exact behavior and portability.

![Module architecture of libcpg, showing the graph core and the analysis modules layered over it](../diagrams/module-architecture.svg)

*Figure — the module map these decisions inhabit: the `graph` core (ADR-0001) with the `analysis`, `builder`, `pattern`/`patterns`, `algorithms`, and `gnn` modules layered on top. Source: [`diagrams/module-architecture.puml`](../diagrams/module-architecture.puml).*

## Conventions

- **Numbering.** Records are numbered in the order they were accepted
  (`0001`…). Numbers are never reused; a reversed decision is marked
  *Superseded* and a new record supersedes it, so history stays legible.
- **Status lifecycle.** `Proposed → Accepted → (Superseded by NNNN | Deprecated)`.
  All sixty-two current records are **Accepted** and reflect the shipped `v0.1.1`
  code.
- **Grounding.** Every claim here is traceable to source: the
  [`Cargo.toml`](../engineering/01-build-and-features.md) feature block, the
  `#[cfg(any())]` retired code paths in `src/builder/dfg.rs`, and the inline
  `#[cfg(test)]` tests that pin each behaviour. Where a record cites external
  literature it lists it verbatim under **References**; where a decision is a
  pure engineering trade-off with no literature behind it, that is stated
  plainly.

## References

This index cites no external literature; each decision record below carries its
own **References** section listing only the works it cites. Terminology is
defined once in the [glossary](../GLOSSARY.md) and linked from every record.
