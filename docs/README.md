# libcpg documentation

**libcpg** is a Rust library for constructing and analyzing **Code Property
Graphs** (CPGs): a single `petgraph`-backed directed graph that overlays the
Abstract Syntax Tree (AST), Control Flow Graph (CFG), Data Flow Graph (DFG),
and — on demand — the Program Dependence Graph (PDG) on **one shared node set**.

## What is a Code Property Graph?

A CPG (Yamaguchi et al. 2014) represents source code as a graph that merges
complementary program views so a single query can reason over syntax, control
flow, and data flow at once:

- **AST** — syntactic structure (how the code parses);
- **CFG** — execution order between nodes;
- **DFG** — how values flow from definitions to uses;
- **PDG** — control- and data-dependence, built on demand for program slicing.

![A Code Property Graph overlays AST, control-flow, and data-flow edges on one shared node set](diagrams/cpg-overlay.svg)

*Figure — the unified CPG for a small function. Source: [`diagrams/cpg-overlay.dot`](diagrams/cpg-overlay.dot).*

Combining these views enables, for example: **taint / vulnerability queries**
(untrusted data reaching a sensitive sink), **design-pattern recognition**
(subgraph matching), **exact dominance analysis** (dominators, post-dominators,
and control frontiers), **exact SCC decomposition** (cycles and recursion),
**condensation-DAG analytics** (canonical depth/width, SCC-weighted critical
paths, and width-bounded Coffman–Graham layers),
**Design Structure Matrix analytics** (transitive visibility fan-in/fan-out,
propagation cost, median core–periphery roles, and largest cyclic group),
**Martin package metrics** (afferent/efferent coupling, instability,
abstractness, main-sequence distance, and witnessed stable-dependency
violations over explicit module assignments),
**CK and function metrics** (resolved method-field graphs, WMC/DIT/NOC/CBO/RFC,
LCOM1–5, Halstead, cyclomatic and cognitive complexity, and maintainability
index),
**reflexion-model conformance** (ordered path layers, convergences,
divergences, uncovered edges, and declared absences),
**advisory architectural smells** (multi-node cycles, two-sided hubs,
less-stable dependency shares, and module-size outliers with stable evidence),
**Weisfeiler–Lehman structural clones** (bounded deterministic projection
labels and ordered clone-candidate classes),
**projection centrality** (PageRank, personalized and reverse PageRank,
vertex/edge betweenness, spectral, distance, HITS, and structural-hole scores),
**community detection** (bounded deterministic multi-level Louvain plus checked
connected-community refinement),
**cohesion and structural robustness** (k-core, k-truss, articulation/bridge,
two-edge-connected, minimum-cut, assortativity, and attack analyses),
**spectral connectivity and Fiedler partitioning** (combinatorial-Laplacian
algebraic connectivity, aligned sign cuts, and bounded residual-aware iteration),
**bounded simple-cycle enumeration** (canonical directed witnesses, SCC pruning,
and explicit first-omission evidence),
**directed motif census** (all sixteen induced triad classes plus bounded
four-node clique and out-star graphlets),
**bounded bipartite graph-edit distance** (deterministic typed edit-script
upper bounds with an explicit compatibility cap),
**natural-loop forests** (headers, latches, nesting, and irreducibility),
**CFG reachability reports** (unreachable nodes and structural dead branches),
**basic-block CFGs** (id-preserving blocks, typed edges, and traversal orders),
**monotone data-flow solving** (forward/backward lattices, canonical facts,
widening/narrowing, and bounded convergence),
**variable liveness and dead-store reports** (backward-May facts and advisory
unused-write evidence),
**available and very-busy expressions** (dual Must analyses with conservative
CSE/PRE evidence),
**sparse conditional constant propagation** (checked scalar constants,
executable/dead CFG edges, and value-sensitive reachability refinement),
**finite-state typestate analysis** (allocation-site identities, typed
object/event evidence, deterministic CFG may-facts, forbidden-transition
witnesses, terminal states, and explicit completeness bounds),
**advisory bug detectors** (non-accepting resource exits, binder-aware
uninitialized uses, finite exception summaries, uncaught call-root witnesses,
dead catches, and explicit incomplete-result evidence),
**population anomaly detectors** (taint-catalog-selected missing checks,
call-order and return-check beliefs, exact rational peer support, and qualified
deviant-behavior evidence),
**clone and feasibility advisories** (exact Type-2/3 rename/guard divergence,
SCCP/interval branch infeasibility, redundant guards, and predicate-backed
integer contradictions),
**API-usage and temporal protocol mining** (centered typed AUGs, exact
strict-majority following/argument/receiver/control fragments, advisory
deviations, and named finite-state lifecycle properties),
**declarative CWE-template analysis** (bounded structural qualification,
shared taint/null/typestate/interval evidence, ten shipped weakness templates,
and explicit incomplete-result semantics),
**cryptographic API-misuse analysis** (CrySL-style call-order properties,
typed parameter constraints, literal-origin key taint, and qualified advisory
evidence for weak hashes, ECB, predictable IVs, and hardcoded keys),
**secret-flow and missing access-policy analysis** (entropy/name-qualified
literal provenance to authentication/network/key sinks and exact peer-supported
missing authN/authZ guard evidence),
**vulnerability extrapolation** (joint-vocabulary PDG WL ranking from one
caller-labeled seed with bounded relaxed VF2 confirmation and typed omissions),
**virtual-call type refinement** (immutable CHA/RTA/VTA target sets over exact
nominal, hierarchy, allocation, and receiver-reaching definition evidence),
**interval value-range analysis** (branch constraints, loop-header widening,
bounded narrowing, and immutable block-entry ranges),
**loop-induction analysis** (DFG-backed invariants, additive/affine induction
variables, conservative hoist candidates, and counted-trip evidence),
**multi-file program composition** (collision-free stable-id translation,
ordered file scopes, evidence-bounded import/export overlays, and cross-file
`ExactViaImport` call targets),
**algorithm detection** (from control-flow structure), **evidence-aware
complexity analysis** (validated loop depth plus DFG-backed IV/bound evidence,
with exact AST fallback), **function clone detection** (Type-1/2 AST Merkle,
Type-3 multiset, and bounded Type-4 PDG WL/VF2 evidence), and
**graph-neural-network embeddings**.

## Quick start

```rust
// requires: features = ["lang-rust"]
use libcpg::{TreeSitterCpgBuilder, CpgBuilder, Language};

let builder = TreeSitterCpgBuilder::new();
let source = r#"
    fn factorial(n: u64) -> u64 {
        if n <= 1 { 1 } else { n * factorial(n - 1) }
    }
"#;
let cpg = builder.build(source, Language::Rust)?;

println!("nodes: {}, edges: {}", cpg.node_count(), cpg.edge_count());
for func in cpg.functions() {
    println!("function: {}", func.name().unwrap_or("<anonymous>"));
}
# Ok::<(), libcpg::Error>(())
```

With `default = []` no grammars are compiled in — enable a `lang-*` feature (as
above) or build from a caller-supplied tree with `build_from_tree` (Mode B).

### Detecting design patterns

```rust
// requires: features = ["lang-rust", "design-patterns"]
use libcpg::patterns::design::{GofPatternDetector, PatternDetector};

let detector = GofPatternDetector::new();
for m in detector.detect(&cpg) {
    println!("{} (confidence {:.2})", m.pattern_name, m.confidence);
}
```

### Program slicing

```rust
// requires: features = ["lang-rust"]
use libcpg::{TreeSitterCpgBuilder, CpgBuilder, PdgBuilder, backward_slice, Language};

let mut cpg = TreeSitterCpgBuilder::new().build(source, Language::Rust)?;
let func = cpg.functions().map(|n| n.id).next().expect("a function node");
PdgBuilder::new().build(&mut cpg, func);            // add PDG edges
let slice = backward_slice(&cpg, func, 256);         // bounded backward slice
# Ok::<(), libcpg::Error>(())
```

## Documentation map

### Theory — the foundations and the "why"
[Overview](theory/00-overview.md) · [Code property graphs](theory/01-code-property-graphs.md) · [Control flow & complexity](theory/02-control-flow-and-complexity.md) · [Data flow & reaching definitions](theory/03-data-flow-and-reaching-definitions.md) · [Program dependence & slicing](theory/04-program-dependence-and-slicing.md) · [Subgraph isomorphism (VF2)](theory/05-subgraph-isomorphism-vf2.md) · [Graph similarity](theory/06-graph-similarity.md) · [Design-pattern detection](theory/07-design-pattern-detection.md) · [Algorithm & complexity analysis](theory/08-algorithm-and-complexity-analysis.md) · [Graph neural networks](theory/09-graph-neural-networks.md) · [Design Structure Matrices](theory/10-design-structure-matrix-analysis.md) · [WL structural clones](theory/11-weisfeiler-lehman-structural-clones.md) · [Centrality and structural importance](theory/12-centrality-and-structural-importance.md) · [Community structure and modularity](theory/13-community-structure-and-modularity.md) · [Graph cohesion and robustness](theory/14-graph-cohesion-and-robustness.md) · [Spectral connectivity and bisection](theory/15-spectral-connectivity-and-bisection.md) · [Bounded simple-cycle enumeration](theory/16-bounded-simple-cycle-enumeration.md) · [Directed motif census](theory/17-directed-motif-census.md) · [Martin package metrics](theory/18-martin-package-metrics.md) · [Reflexion-model conformance](theory/19-reflexion-model-conformance.md) · [Longest-path architectural layering](theory/20-longest-path-architectural-layering.md) · [Advisory architectural smells](theory/21-advisory-architectural-smells.md) · [Object-oriented and function metrics](theory/22-object-oriented-and-function-metrics.md) · [Function clone detection](theory/23-function-clone-detection.md) · [Multi-file program composition](theory/24-program-composition.md)

[IFDS/IDE summary fixed points](theory/25-ifds-ide-summary-solving.md)

[Interprocedural effect lattice](theory/26-interprocedural-effect-lattice.md)

[Interprocedural taint and maybe-null summaries](theory/27-interprocedural-taint-and-nullflow.md)

[System dependence and context-sensitive slicing](theory/28-system-dependence-and-slicing.md)

[Channel topology and linear consumption](theory/29-channel-topology-and-linearity.md)

[Cyclic wait and lock-order deadlock candidates](theory/30-cyclic-wait-and-lock-order.md)

[May-happen-in-parallel relations](theory/31-may-happen-in-parallel.md)

[Advisory session-protocol duality](theory/32-session-protocol-duality.md)

[Finite-state typestate properties](theory/33-finite-state-typestate-properties.md)

[Advisory resource, definition, and exception analysis](theory/34-advisory-bug-detection.md)

[Exact population anomaly inference](theory/35-population-anomaly-inference.md)

[Clone consistency and abstract feasibility](theory/36-clone-consistency-and-abstract-feasibility.md)

[API-usage graphs and temporal properties](theory/37-api-usage-graphs-and-temporal-properties.md)

[Declarative CWE-query semantics](theory/38-declarative-cwe-query-semantics.md)

[Cryptographic API-misuse semantics](theory/39-cryptographic-api-misuse.md)

[Secret-flow and access-policy semantics](theory/40-secret-flow-and-access-policy-inference.md)

[Evidence-bounded vulnerability extrapolation](theory/41-vulnerability-extrapolation.md)

[Closed-world virtual-call type refinement](theory/42-closed-world-type-refinement.md)

[Evidence-bounded race and atomicity candidates](theory/43-evidence-bounded-race-and-atomicity.md)

[Deterministic CPG representations for external ML](theory/44-deterministic-cpg-ml-representations.md)

### Architecture — the "what / where"
[Overview](architecture/overview.md) · [Graph data model](architecture/graph-data-model.md) · [Construction & analysis data flow](architecture/data-flow.md) · [Language frontends](architecture/language-frontends.md)

### Design decisions (ADRs)
[Overview](design/00-overview.md) · [0001 Unified overlay graph](design/0001-unified-overlay-graph.md) · [0002 Mode B: build_from_tree](design/0002-mode-b-build-from-tree.md) · [0003 AST-ordered reaching definitions](design/0003-ast-ordered-reaching-defs.md) · [0004 Relaxed VF2 detection](design/0004-relaxed-vf2-detection.md) · [0005 Feature-flag taxonomy](design/0005-feature-flag-taxonomy.md) · [0006 Evidence-bounded call resolution](design/0006-evidence-bounded-call-resolution.md) · [0007 Dominator-based natural-loop forest](design/0007-dominator-natural-loop-forest.md) · [0008 Id-preserving basic-block CFG](design/0008-id-preserving-basic-block-cfg.md) · [0009 Deterministic monotone data flow](design/0009-deterministic-monotone-dataflow.md) · [0010 Dual-worklist SCCP](design/0010-dual-worklist-sccp.md) · [0011 Loop-header interval widening](design/0011-loop-header-interval-widening.md) · [0012 DFG-backed loop induction](design/0012-dfg-backed-loop-induction-evidence.md) · [0013 Evidence-bounded complexity](design/0013-evidence-bounded-complexity-estimates.md) · [0014 Deterministic condensation analytics](design/0014-deterministic-condensation-analytics.md) · [0015 Bounded DSM visibility](design/0015-bounded-dsm-visibility-analysis.md) · [0016 Stable WL projection labels](design/0016-stable-wl-projection-labels.md) · [0017 Deterministic bounded centrality](design/0017-deterministic-bounded-centrality.md) · [0018 Deterministic multi-level community detection](design/0018-deterministic-multilevel-community-detection.md) · [0019 Deterministic bounded cohesion analysis](design/0019-deterministic-bounded-cohesion-analysis.md) · [0020 Deterministic bounded spectral analysis](design/0020-deterministic-bounded-spectral-analysis.md) · [0021 Deterministic bounded cycle enumeration](design/0021-deterministic-bounded-cycle-enumeration.md) · [0022 Complete bounded motif census](design/0022-deterministic-motif-census.md) · [0023 Deterministic bounded bipartite GED](design/0023-deterministic-bipartite-ged.md) · [0024 Explicit Martin module assignments](design/0024-explicit-martin-module-assignments.md) · [0025 Pure path reflexion model](design/0025-pure-path-reflexion-model.md) · [0026 Longest-path architectural layering](design/0026-longest-path-architectural-layering.md) · [0027 Validated advisory architectural smells](design/0027-validated-advisory-architectural-smells.md) · [0028 Evidence-bounded member-field resolution](design/0028-evidence-bounded-member-field-resolution.md) · [0029 Evidence-bounded code metrics](design/0029-evidence-bounded-code-metrics.md) · [0030 Bounded function clone detection](design/0030-bounded-function-clone-detection.md) · [0031 Deterministic program composition](design/0031-deterministic-program-composition.md)

[0032 Deterministic bounded IFDS/IDE summaries](design/0032-deterministic-ifds-ide-summaries.md)

[0033 Compositional effect summaries](design/0033-compositional-effect-summaries.md)

[0034 Data-driven advisory taint](design/0034-data-driven-advisory-taint.md)

[0035 Overlay SDG summary edges and two-pass slicing](design/0035-system-dependence-summary-slicing.md)

[0036 Typed channel topology](design/0036-typed-channel-topology.md)

[0037 Typed deadlock witnesses](design/0037-typed-deadlock-witnesses.md)

[0038 Typed AST may-happen-in-parallel](design/0038-typed-ast-mhp.md)

[0039 Advisory session-protocol sketches](design/0039-advisory-session-sketches.md)

[0040 Flat property-value arena](design/0040-flat-property-value-arena.md)

[0041 Shared flat expression facts](design/0041-shared-flat-expression-facts.md)

[0042 Stack-safe CFG extraction](design/0042-stack-safe-cfg-extraction.md)

[0043 Deterministic typestate analysis](design/0043-deterministic-typestate-analysis.md)

[0044 Evidence-composed bug detectors](design/0044-evidence-composed-bug-detectors.md)

[0045 Exact population anomaly detectors](design/0045-exact-population-anomaly-detectors.md)

[0046 Evidence-bounded clone and feasibility advisories](design/0046-evidence-bounded-clone-and-feasibility-advisories.md)

[0047 Evidence-bounded API-usage and temporal mining](design/0047-evidence-bounded-api-usage-and-temporal-mining.md)

[0048 Declarative qualified CWE templates](design/0048-declarative-qualified-cwe-templates.md)

[0049 Composed cryptographic API misuse](design/0049-composed-cryptographic-api-misuse.md)

[0050 Composed secret and access-policy analysis](design/0050-composed-secret-and-access-policy-analysis.md)

[0051 Evidence-bounded vulnerability extrapolation](design/0051-evidence-bounded-vulnerability-extrapolation.md)

[0052 Closed-world virtual-call type refinement](design/0052-closed-world-type-refinement.md)

[0053 Evidence-bounded concurrency bug candidates](design/0053-evidence-bounded-concurrency-bug-candidates.md)

[0054 Deterministic CPG ML exports](design/0054-deterministic-cpg-ml-exports.md)

[0055 Deterministic dependence-aware CPG diff](design/0055-deterministic-dependence-aware-cpg-diff.md)

### API reference
[Graph / analysis reference](api/graph-reference.md) · [Builder reference](api/builder-reference.md) · [Pattern / algorithm / GNN reference](api/pattern-reference.md)

### Components

#### Graph core
[Overview](components/graph/overview.md) · [Nodes](components/graph/nodes.md) · [Edges](components/graph/edges.md) · [Traversal](components/graph/traversal.md) · [Multi-file programs](components/graph/program-composition.md) · [Deterministic projections](components/graph/projections.md) · [Projection centrality](components/graph/centrality-analysis.md) · [Community detection](components/graph/community-detection.md) · [Cohesion and robustness](components/graph/cohesion-analysis.md) · [Spectral connectivity](components/graph/spectral-analysis.md) · [Bounded simple cycles](components/graph/cycle-analysis.md) · [Directed motifs](components/graph/motif-analysis.md) · [Basic-block CFGs](components/graph/basic-block-cfg.md) · [Monotone data-flow framework](components/graph/dataflow-framework.md) · [Variable liveness and dead stores](components/graph/liveness-analysis.md) · [Available and very-busy expressions](components/graph/expression-analysis.md) · [Sparse conditional constant propagation](components/graph/sccp-analysis.md) · [Interval value ranges](components/graph/interval-analysis.md) · [Loop induction](components/graph/induction-analysis.md) · [Dominators and post-dominators](components/graph/dominator-analysis.md) · [Strongly-connected components](components/graph/scc-analysis.md) · [Condensation-DAG analytics](components/graph/condensation-analysis.md) · [Architectural layering](components/graph/layering-analysis.md) · [Architectural smells](components/graph/architecture-smells.md) · [DSM visibility and core–periphery](components/graph/dsm-analysis.md) · [Martin package metrics](components/graph/martin-metrics.md) · [CK, LCOM, and function metrics](components/graph/code-metrics.md) · [Reflexion-model conformance](components/graph/reflexion-model.md) · [WL structural clones](components/graph/wl-analysis.md) · [Natural loops and nesting](components/graph/loop-analysis.md) · [CFG reachability and dead branches](components/graph/reachability-analysis.md) · [Evidence-bounded call resolution](components/graph/call-resolution.md)

[IFDS/IDE procedure summaries](components/graph/ifds-ide-framework.md)

[Interprocedural effect summaries](components/graph/effect-summaries.md)

[Advisory taint and maybe-null analysis](components/graph/taint-analysis.md)

[System dependence and interprocedural slicing](components/graph/system-dependence-slicing.md)

[Channel-event extraction and topology](components/graph/channel-topology.md)

[Communication and lock-order deadlock candidates](components/graph/deadlock-analysis.md)

[May-happen-in-parallel analysis](components/graph/mhp-analysis.md)

[Race and atomicity candidate detection](components/graph/concurrency-bug-detection.md)

[Deterministic ML export](components/graph/ml-export.md)

[Deterministic CPG diff](components/graph/cpg-diff.md)

[Advisory session-protocol analysis](components/graph/session-analysis.md)

[Deterministic typestate analysis](components/graph/typestate-analysis.md)

[Advisory bug detectors](components/graph/bug-detectors.md)

[Population anomaly detectors](components/graph/population-anomaly-detectors.md)

[Clone-consistency and path-feasibility detectors](components/graph/clone-feasibility-bug-detectors.md)

[API-usage protocol mining](components/graph/api-usage-protocol-mining.md)

[Declarative CWE-template analysis](components/graph/cwe-template-analysis.md)

[Cryptographic API-misuse analysis](components/graph/crypto-misuse-analysis.md)

[Secret-flow and access-policy analysis](components/graph/security-policy-analysis.md)

[Vulnerability extrapolation](components/graph/vulnerability-extrapolation.md)

[Virtual-call type refinement](components/graph/type-refinement.md)

#### Builder
[Overview](components/builder/overview.md) · [CFG extraction](components/builder/cfg.md) · [DFG extraction](components/builder/dfg.md) · [PDG & slicing](components/builder/pdg-and-slicing.md) · [Node mapper](components/builder/node-mapper.md)

#### Pattern detection
[Overview](components/patterns/overview.md) · [Function clone detection](components/patterns/clone-detection.md) · [VF2 matching & similarity](components/patterns/vf2-matching.md) · [Bipartite GED](components/patterns/ged-analysis.md) · [Gang of Four](components/patterns/gang-of-four.md) · [DPML](components/patterns/dpml.md) · [Classification](components/patterns/classification.md)

#### Algorithm detection
[Overview](components/algorithms/overview.md) · [Families](components/algorithms/families.md) · [Complexity](components/algorithms/complexity.md)

#### Graph neural networks
[Overview](components/gnn/overview.md) · [Message passing](components/gnn/message-passing.md) · [Embeddings](components/gnn/embeddings.md)

### Usage guides
[Getting started](usage/00-getting-started.md) · [Building CPGs](usage/01-building-cpgs.md) · [Querying & traversal](usage/02-querying-and-traversal.md) · [Pattern detection](usage/03-pattern-detection.md) · [Program slicing](usage/04-program-slicing.md) · [Serialization](usage/05-serialization.md) · [F1R3FLY: Rholang & MeTTa](usage/06-f1r3fly-rholang-metta.md) · [Multi-file programs](usage/07-multi-file-programs.md)

[IFDS/IDE summary domains](usage/08-ifds-summary-domains.md)

[Effect summaries](usage/09-effect-summaries.md)

[Taint and maybe-null analysis](usage/10-taint-and-nullflow.md)

[Context-sensitive interprocedural slicing](usage/11-interprocedural-slicing.md)

[Channel topology and linearity reports](usage/12-channel-topology.md)

[Communication and lock-order deadlock candidates](usage/13-deadlock-candidates.md)

[May-happen-in-parallel queries](usage/14-may-happen-in-parallel.md)

[Rholang session-protocol sketches](usage/15-session-protocol-sketches.md)

[Finite-state typestate analysis](usage/16-typestate-analysis.md)

[Advisory bug detectors](usage/17-advisory-bug-detectors.md)

[Population anomaly detectors](usage/18-population-anomaly-detectors.md)

[Clone and path-feasibility advisories](usage/19-clone-feasibility-advisories.md)

[API-usage and temporal protocol mining](usage/20-api-usage-protocol-mining.md)

[Declarative CWE-template analysis](usage/21-cwe-template-analysis.md)

[Cryptographic API-misuse analysis](usage/22-cryptographic-api-misuse.md)

[Secret flow and missing access guards](usage/23-secret-and-access-policy-analysis.md)

[Vulnerability extrapolation](usage/24-vulnerability-extrapolation.md)

[Virtual-call type refinement](usage/25-virtual-call-type-refinement.md)

[Race and atomicity candidates](usage/26-concurrency-bug-candidates.md)

[CPG tensors and exact AST paths](usage/27-ml-export-and-ast-paths.md)

[Compare two CPG versions](usage/28-cpg-diff.md)

### Engineering
[Overview](engineering/00-overview.md) · [Build & features](engineering/01-build-and-features.md) · [Testing](engineering/02-testing.md) · [Performance](engineering/03-performance.md) · [Optimization ledger](engineering/optimization-ledger.md) · [Analysis-expansion matrix](engineering/analysis-expansion-matrix.md) · [Contributing](engineering/04-contributing.md)

### Scientific validation
[Overview](scientific/00-overview.md) · [CPG invariants & equivalence](scientific/01-cpg-invariants-and-equivalence.md) · [Reaching-defs validation](scientific/02-reaching-defs-validation.md) · [VF2 completeness](scientific/03-vf2-completeness.md) · [Measurement methodology](scientific/04-measurement-methodology.md) · [SCCP validation](scientific/05-sccp-validation.md) · [Interval validation](scientific/06-interval-analysis-validation.md) · [Loop-induction validation](scientific/07-induction-analysis-validation.md) · [Complexity-evidence validation](scientific/08-complexity-evidence-validation.md) · [Condensation validation](scientific/09-condensation-validation.md) · [DSM validation](scientific/10-dsm-validation.md) · [WL validation](scientific/11-wl-validation.md) · [Centrality validation](scientific/12-centrality-validation.md) · [Community validation](scientific/13-community-validation.md) · [Cohesion validation](scientific/14-cohesion-validation.md) · [Spectral validation](scientific/15-spectral-validation.md) · [Cycle-enumeration validation](scientific/16-cycle-enumeration-validation.md) · [Motif-census validation](scientific/17-motif-census-validation.md) · [GED validation](scientific/18-ged-validation.md) · [Martin-metrics validation](scientific/19-martin-metrics-validation.md) · [Reflexion-model validation](scientific/20-reflexion-model-validation.md) · [Layering validation](scientific/21-layering-validation.md) · [Architectural-smell validation](scientific/22-architectural-smells-validation.md) · [Member-field flow validation](scientific/23-member-field-flow-validation.md) · [Code-metrics validation](scientific/24-code-metrics-validation.md) · [Function-clone validation](scientific/25-function-clone-validation.md) · [Program-composition validation](scientific/26-program-composition-validation.md)

[IFDS/IDE validation](scientific/27-ifds-ide-validation.md)

[Effect-summary validation](scientific/28-effect-summary-validation.md)

[Taint and maybe-null validation](scientific/29-taint-nullflow-validation.md)

[SDG slicing validation](scientific/30-sdg-slicing-validation.md)

[Channel-topology validation](scientific/31-channel-topology-validation.md)

[Deadlock-candidate validation](scientific/32-deadlock-validation.md)

[May-happen-in-parallel validation](scientific/33-mhp-validation.md)

[Session-protocol sketch validation](scientific/34-session-sketch-validation.md)

[Flat property-value validation](scientific/35-property-value-validation.md)

[Shared expression-fact arena validation](scientific/36-expression-fact-arena-validation.md)

[Stack-safe CFG extraction validation](scientific/37-cfg-extraction-machine-validation.md)

[Finite-state typestate validation](scientific/38-typestate-validation.md)

[Advisory bug-detector validation](scientific/39-bug-detector-validation.md)

[Population-anomaly validation](scientific/40-population-anomaly-validation.md)

[Clone and feasibility validation](scientific/41-clone-feasibility-validation.md)

[API-usage and temporal validation](scientific/42-api-usage-temporal-validation.md)

[Declarative CWE-template validation](scientific/43-cwe-template-validation.md)

[Cryptographic API-misuse validation](scientific/44-cryptographic-api-misuse-validation.md)

[Secret-flow and access-policy validation](scientific/45-security-policy-validation.md)

[Vulnerability-extrapolation validation](scientific/46-vulnerability-extrapolation-validation.md)

[Virtual-call type-refinement validation](scientific/47-type-refinement-validation.md)

[Race and atomicity candidate validation](scientific/48-concurrency-bug-validation.md)

[Deterministic ML-export validation](scientific/49-ml-export-validation.md)

[Deterministic CPG-diff validation](scientific/50-cpg-diff-validation.md)

### Security
[Threat model](security/00-threat-model.md) · [Input & resource hardening](security/01-input-and-resource-hardening.md) · [Taint-analysis security boundary](security/02-taint-analysis.md) · [Interprocedural-slicing security boundary](security/03-system-dependence-slicing.md) · [Channel-topology security boundary](security/04-channel-topology.md) · [Deadlock-analysis security boundary](security/05-deadlock-analysis.md) · [MHP-analysis security boundary](security/06-mhp-analysis.md) · [Session-analysis security boundary](security/07-session-analysis.md) · [Typestate-analysis security boundary](security/08-typestate-analysis.md) · [Bug-detector security boundary](security/09-bug-detectors.md) · [Population-anomaly security boundary](security/10-population-anomaly-detectors.md) · [Clone/feasibility security boundary](security/11-clone-feasibility-detectors.md) · [API-usage protocol-mining security boundary](security/12-api-usage-protocol-mining.md) · [CWE-template security boundary](security/13-cwe-template-analysis.md) · [Cryptographic API-misuse security boundary](security/14-cryptographic-api-misuse.md) · [Secret-flow/access-policy security boundary](security/15-secret-and-access-policy-analysis.md) · [Vulnerability-extrapolation security boundary](security/16-vulnerability-extrapolation.md) · [Virtual-call type-refinement security boundary](security/17-type-refinement-boundary.md) · [Concurrency-bug detection security boundary](security/18-concurrency-bug-detection.md) · [ML-export security boundary](security/19-ml-export-boundary.md) · [CPG-diff security boundary](security/20-cpg-diff-boundary.md)

### Reference
[Glossary](GLOSSARY.md) · [Diagram catalog](diagrams/README.md)

## Features

`default = []` — nothing is enabled by default; opt in to what you need.

| Feature | Enables |
|---|---|
| `lang-rust` … `lang-ruby` (16 grammars) | the tree-sitter grammar for that language (enables internal `build`) |
| `lang-systems` / `lang-scripting` / `lang-web` / `lang-config` / `lang-all` | grammar groups |
| `design-patterns` | Gang-of-Four detection (`patterns::`) |
| `algorithm-detection` | algorithm-family recognition + complexity (`algorithms::`) |
| `serde` | `Serialize` / `Deserialize` derives |
| `gnn` | graph-neural-network embeddings (`gnn::CpgGnn`) |
| `ml-linfa` / `ml-rules` | ML- / rule-based pattern classification |
| `rholang` / `metta` | the Rholang / MeTTa Mode-B node mappers |
| `full` | `gnn + design-patterns + algorithm-detection + serde + ml-rules + lang-all` |

*(`gpu` is reserved and wires no code yet.)* Full detail:
[engineering/build-and-features](engineering/01-build-and-features.md).

## Supported languages

| Path | Languages | How |
|---|---|---|
| Internal parse (`build`) | Rust, Python, JavaScript, TypeScript, Go, Java, C, C++, JSON, HTML, CSS, Bash, TOML, YAML, Markdown, Ruby | enable the matching `lang-*` feature |
| Mode B (`build_from_tree`) | Rholang, MeTTa (F1R3FLY.io) | caller supplies a parse tree; features `rholang` / `metta` enable the mappers |

`TreeSitterCpgBuilder::supported_languages()` lists the 16 built-ins statically;
actual availability for a given build is `ParserRegistry::supports(lang)`.

## Documentation conventions

- **Accuracy** — every type, method, and snippet is written against the real
  API and annotated with the feature flag it needs.
- **Math** — expressions use GitHub math spans (inline ``$`…`$``, display
  fenced `math` blocks), never bare `$…$` or unicode literals.
- **Diagrams** — every figure is a committed PlantUML/Graphviz source rendered
  to a committed SVG; see the [diagram catalog](diagrams/README.md).
- **Citations** — claims from the literature link to DOIs where they exist; each
  substantive page ends with a *References* section. Terms are defined once in
  the [glossary](GLOSSARY.md).

## Related projects

Part of the [F1R3FLY.io](https://f1r3fly.io) ecosystem; libcpg's Rholang and
MeTTa frontends integrate with F1R3FLY tooling (e.g. the pgmcp code index, which
drives CPG construction in Mode B).

## License

Licensed under either of Apache License 2.0 or MIT, at your option.
