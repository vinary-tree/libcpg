# Glossary

This glossary is the **canonical definition** of every term, acronym, and symbol used across the `libcpg` documentation. Each term is defined once here; other pages link back to it rather than redefining. Entries are alphabetical.

## Notation conventions

Mathematical expressions use GitHub math spans: inline as ``$`…`$`` and display as fenced ` ```math ` blocks. A CPG is written as the graph $`G = (V, E)`$ with vertex set $`V`$ (nodes) and edge set $`E`$ (typed edges). $`N`$ and $`E`$ denote node and edge **counts** when a scalar is needed (e.g. in complexity formulae). Citations resolve to DOIs where they exist; see [References](#references).

---

### Abstract interpretation

A static-analysis method that executes a program over an **abstract domain**
whose elements summarize sets of concrete states. Sound transfer functions
over-approximate every concrete behavior; joins combine paths, while widening
and narrowing control convergence on infinite-height domains. libcpg's interval
value-range analysis follows Cousot and Cousot [[20]](#references). See
[`components/graph/interval-analysis.md`](components/graph/interval-analysis.md).

### Abstract Syntax Tree (AST)

The tree that a parser produces from source code: interior nodes are language constructs (functions, `if`, expressions) and children are their syntactic parts. In `libcpg` the AST is the **base layer** of the [Code Property Graph](#code-property-graph-cpg) — every other layer overlays edges on the *same* AST nodes. AST edges are `AstChild` / `AstParent` / `AstNextSibling` / `AstPrevSibling`. See [`architecture/graph-data-model.md`](architecture/graph-data-model.md).

### Afferent coupling / Efferent coupling

For a named module, **afferent coupling** $`C_a`$ counts distinct external
projection vertices that depend on vertices in the module. **Efferent
coupling** $`C_e`$ counts distinct external vertices on which module vertices
depend. An edge points from dependent to dependency; reversing it exchanges
the two meanings. libcpg ignores weights and intra-module edges. See [Martin
package metrics](#martin-package-metrics).

### Aggregation (GNN)

The step in a [Graph Neural Network](#graph-neural-network-gnn) that combines a node's neighbour vectors into one vector. `libcpg`'s `AggregationMethod` enumerates `Mean` (the default and the only one used in message passing), `Sum`, `Max`, `Attention`, and `Hierarchical`; the latter two are **reserved placeholders**, not yet wired. See [Message passing](#message-passing-gnn).

### Algebraic connectivity / Fiedler vector

For the combinatorial graph Laplacian $`L=D-A`$, **algebraic connectivity** is
its second-smallest eigenvalue $`\lambda_2`$. It is zero exactly when the
undirected graph is disconnected. A corresponding unit, zero-mean eigenvector
is a **Fiedler vector**; splitting vertices by its sign gives a spectral
partition heuristic [[32]](#references). libcpg uses an unweighted simple
projection topology and bounded deterministic sparse iteration. See
[`theory/15-spectral-connectivity-and-bisection.md`](theory/15-spectral-connectivity-and-bisection.md)
and [`components/graph/spectral-analysis.md`](components/graph/spectral-analysis.md).

### Algorithm family

A structural category of algorithm (sorting, searching, graph traversal, dynamic programming, …) that the `algorithm-detection` feature recognises from a function's control-flow and recursion shape via **heuristics** (e.g. "has a midpoint calculation", "has a visited set", "has a memoization table"). Exposed as `AlgorithmFamily`. Detection is heuristic, not a proof of identity. See [`components/algorithms/families.md`](components/algorithms/families.md).

### Analysis plan / ordered emitter

An **analysis plan** is a heap-owned, graph-local description of edges or
results computed while workers hold only immutable CPG borrows. An **ordered
emitter** consumes indexed plans in the original function or pattern order and
performs the graph mutation serially. This split makes worker scheduling
irrelevant to stable edge identities, duplicate handling, and public result
order. See [ADR-0059](design/0059-deterministic-parallel-plan-emission.md).

### Articulation vertex / Bridge / Two-edge-connected component

An **articulation vertex** is a vertex whose deletion increases connected-
component count. A **bridge** is an edge with the same property. A
**two-edge-connected component** is a maximal vertex region left after every
bridge is deleted; no single remaining edge can disconnect that region.
libcpg derives all three from iterative depth-first low links [[31d–31e]](#references).

### AST-ordered reaching definitions

`libcpg`'s [data-flow](#data-flow-graph-dfg) analysis strategy: a single **flow-sensitive** sweep over AST nodes *in source order*, maintaining an environment `ReachingEnv` mapping each variable name to the set of definitions currently reaching it. It applies a [strong update](#strong-update--weak-update) in straight-line context and a weak update inside conditional regions, and sweeps loop bodies twice for loop-carried dependencies. It is **not** [SSA](#static-single-assignment-ssa) and **not** classic CFG-fixed-point propagation; it is chosen for simplicity and for threading definitions into deeply nested expression uses. See [`theory/03-data-flow-and-reaching-definitions.md`](theory/03-data-flow-and-reaching-definitions.md) and [`design/0003-ast-ordered-reaching-defs.md`](design/0003-ast-ordered-reaching-defs.md).

### Attack simulation

An explicit robustness experiment that removes distinct vertices in a caller-
supplied order and records the largest connected component after each step.
libcpg also reports normalized trapezoidal area under that trace; lower area
means faster fragmentation under that particular order [[31h]](#references).

### Available expression

An expression is **available** before a program point when every path from the
function entry has evaluated the same structural expression and no later node
on any path has defined one of its operands. Availability is a forward
[Must analysis](#may-analysis--must-analysis) with intersection at joins and
supports conservative [CSE](#common-subexpression-elimination-cse) evidence.
See [`components/graph/expression-analysis.md`](components/graph/expression-analysis.md).

### Backward slice / Forward slice

A [program slice](#program-slicing). The **backward** slice of a node $`s`$ is the set of nodes that can affect $`s`$ (its transitive [PDG](#program-dependence-graph-pdg) predecessors); the **forward** slice is the set of nodes that $`s`$ can affect (its transitive successors). `libcpg` computes them as bounded breadth-first traversals — `backward_slice(&cpg, s, max_nodes)` and `forward_slice(&cpg, s, max_nodes)` — after [`ControlDependence`](#control-dependence) and [`DataDependence`](#data-dependence) edges have been added. The `max_nodes` argument caps the result. Introduced by Weiser [[8]](#references). See [`usage/04-program-slicing.md`](usage/04-program-slicing.md).

### Back edge / Header / Latch

For a control-flow graph with entry $`r`$, an edge $`t \to h`$ is a **natural
back edge** when $`h`$ dominates $`t`$. The target $`h`$ is the loop **header**
and the source $`t`$ is a **latch**. Multiple latches may share one header and
therefore one natural loop. See [Natural loop / Loop forest](#natural-loop--loop-forest).

### Basic block

A maximal straight-line run of statements with a single entry and single exit — no branches in except at the top, none out except at the bottom. `block_cfg` returns an immutable, id-preserving partition with typed block edges and traversal orders; `BasicBlockIdentifier` is its leader-map compatibility adapter. Basic blocks are the classic unit of control-flow analysis while the CPG itself remains at AST-node granularity. See [`components/graph/basic-block-cfg.md`](components/graph/basic-block-cfg.md).

### Call graph

The overlay whose edges connect call sites to the functions they invoke:
`StaticCall` (resolved statically), `DynamicCall` (method/virtual), and
`CallSite`. Parsed calls begin with no target unless the source mapper has
stronger information; `resolve_calls` can materialize definite targets through
[evidence-bounded call resolution](#confidence-call-resolution). Queried through
`call_sites`, `callees`, and `callers`. See
[`components/graph/edges.md`](components/graph/edges.md).

### Centrality

A family of graph measures that assigns structural importance to vertices or
edges. **PageRank** propagates probability through directed links;
**personalized PageRank** replaces uniform teleportation with a seed restart
vector; **reverse PageRank** applies PageRank to the transpose. **Betweenness**
counts fractions of shortest paths through a vertex or edge. **Eigenvector**
centrality rewards connections to highly scored neighbors; **Katz** centrality
adds an attenuated contribution from walks of every length. **Closeness** uses
inverse mean distance; **harmonic** centrality sums reciprocal distances.
**Hyperlink-Induced Topic Search (HITS)** separates hub and authority vectors.
**Burt constraint** measures local neighborhood redundancy and structural-hole
brokerage. Each definition has different direction and weight semantics
[[28]](#references). See
[`theory/12-centrality-and-structural-importance.md`](theory/12-centrality-and-structural-importance.md)
and [`components/graph/centrality-analysis.md`](components/graph/centrality-analysis.md).

### Community detection

The task of partitioning a graph into groups whose internal association is
strong relative to a stated baseline. `libcpg::louvain` optimizes
[generalized modularity](#modularity--resolution) over the canonical weighted
undirected view of a `GraphProjection`; it returns stable-node-aligned labels,
not a new graph or an ownership fact. `try_refine_connected` subsequently
splits any assigned group whose induced subgraph is disconnected. See
[`theory/13-community-structure-and-modularity.md`](theory/13-community-structure-and-modularity.md)
and [`components/graph/community-detection.md`](components/graph/community-detection.md).

### Cohesion / Structural robustness

**Cohesion** describes how persistently vertices or edges remain in mutually
supporting subgraphs; libcpg measures it with [k-core](#k-core) and
[k-truss](#k-truss) decompositions. **Structural robustness** describes how
connectivity responds to deletion or separation; articulation vertices,
bridges, two-edge-connected components, global minimum cut, and ordered attack
traces expose complementary failure scales [[31]](#references). See
[`theory/14-graph-cohesion-and-robustness.md`](theory/14-graph-cohesion-and-robustness.md)
and [`components/graph/cohesion-analysis.md`](components/graph/cohesion-analysis.md).

### Code Property Graph (CPG)

A single directed graph that merges several program views onto **one shared node set**: the [AST](#abstract-syntax-tree-ast), the [CFG](#control-flow-graph-cfg), the [DFG](#data-flow-graph-dfg), and — on demand — the [PDG](#program-dependence-graph-pdg). Introduced by Yamaguchi et al. [[1]](#references) to express vulnerability queries that need syntax, control, and data flow simultaneously. In `libcpg` the CPG is the type `CodePropertyGraph`, backed by a [petgraph](#petgraph) `DiGraph<CpgNode, CpgEdge>`. See [`theory/01-code-property-graphs.md`](theory/01-code-property-graphs.md).

### CPG ML export / PyG edge index / AST path context

A **CPG machine-learning (ML) export** is libcpg's deterministic feature-free
numerical view: 69 named node columns aligned with ascending stable ids/source
ranges, plus typed dense edge tuples covering all 47 relation classes. **PyG**
abbreviates PyTorch Geometric; libcpg's `edge_index` is an ordinary sorted
`(source_dense, target_dense, relation_class)` vector that a caller may
transpose for PyG, not a PyG runtime object. An **AST path context** retains two
source-ordered terminals, their exact terminal-to-lowest-common-ancestor-to-
terminal path, aligned node-kind classes/tokens, and the ancestor position,
following the path-representation lineage of code2vec [[67]](#references).
See [Theory 44](theory/44-deterministic-cpg-ml-representations.md) and the
[component contract](components/graph/ml-export.md).

### CPG diff / node alignment / dependence-aware classification

A **CPG diff** is libcpg's complete deterministic structural edit script
between two admitted Code Property Graphs. **Node alignment** is the partial
old-to-new correspondence selected by the symmetric bounded bipartite
[GED](#graph-edit-distance) assignment; it translates graph-local ids but is a
heuristic, not proof of semantic identity. **Dependence-aware classification**
labels the script `BehaviorChanging` exactly when an inserted, removed, or
substituted edge is represented `ControlDependence` or `DataDependence`;
otherwise it labels it `Refactoring`. That second label is not a behavioral-
equivalence claim, especially when no PDG was built. See [Theory
45](theory/45-deterministic-cpg-diff.md) and the [component
contract](components/graph/cpg-diff.md).

### Cryptographic API misuse

A violation of a cryptographic library's required call protocol or parameter
contract, such as a forbidden call order, weak hash, electronic-codebook mode,
predictable initialization vector, or embedded key. libcpg reports advisory
represented evidence by composing typestate, literal-origin taint, and typed
argument provenance. See
[`theory/39-cryptographic-api-misuse.md`](theory/39-cryptographic-api-misuse.md).

### Secret candidate / Secret flow

A **secret candidate** is a typed string literal qualified by a configured
Shannon-entropy threshold or typed binding-name hint. It is not necessarily a
live or valid credential. A **secret flow** additionally has a represented
taint witness from that candidate to a governed authentication, network, or
cryptographic-key argument. libcpg records node provenance and non-sensitive
qualification metadata rather than copying the literal value [[61]](#references),
[[62]](#references). See
[`theory/40-secret-flow-and-access-policy-inference.md`](theory/40-secret-flow-and-access-policy-inference.md).

### Authentication (authN) / Authorization (authZ)

**Authentication (authN)** establishes the identity associated with a request
or principal. **Authorization (authZ)** decides whether that identity may
perform a specific operation. libcpg's missing-access-guard analysis reports a
state-mutating call that lacks a normalized authN/authZ check supported by its
peer population; it does not prove that the peers implement correct access
policy [[52]](#references). See
[`components/graph/security-policy-analysis.md`](components/graph/security-policy-analysis.md).

### Vulnerability extrapolation / joint WL vocabulary

**Vulnerability extrapolation** ranks functions structurally related to one
caller-labeled vulnerable seed. In libcpg, seed and candidate Program
Dependence Graphs are refined in a **joint WL vocabulary**: raw
Weisfeiler--Lehman signatures from both projections are canonicalized together,
so an equal numeric label denotes an equal signature within that comparison.
The resulting cosine and optional relaxed VF2 confirmation are advisory
structural evidence, not vulnerability or behavioral-equivalence proof
[[63]](#references), [[64]](#references). See
[`theory/41-vulnerability-extrapolation.md`](theory/41-vulnerability-extrapolation.md).

### CrySL

**CrySL** is a specification language for cryptographic API usage rules. A rule
combines allowed call sequences with constraints on parameters and related
objects. libcpg adopts that separation of finite-state protocol and value
constraints but exposes its own language-agnostic Rust data model rather than
parsing CrySL source. See
[`design/0049-composed-cryptographic-api-misuse.md`](design/0049-composed-cryptographic-api-misuse.md).

### Code metrics

Static numerical summaries of type and function structure. The
Chidamber–Kemerer suite comprises **weighted methods per class (WMC)**,
**depth of inheritance tree (DIT)**, **number of children (NOC)**,
**coupling between object classes (CBO)**, **response for a class (RFC)**, and
**lack of cohesion of methods (LCOM)** [[15]](#references). `libcpg` reports
LCOM1–LCOM5 over an evidence-bounded method–field graph. At function scope,
**Halstead metrics** count distinct and total operators and operands,
**cyclomatic complexity** counts independent control-flow paths,
**cognitive complexity** weights nested control flow, and the
**maintainability index (MI)** combines Halstead volume, cyclomatic complexity,
logical lines, and comments [[40]](#references). These are descriptive signals,
not quality verdicts. See
[`theory/22-object-oriented-and-function-metrics.md`](theory/22-object-oriented-and-function-metrics.md)
and [`components/graph/code-metrics.md`](components/graph/code-metrics.md).

### Coffman–Graham layering (CG)

A deterministic assignment of a [DAG](#directed-acyclic-graph-dag) to ordered,
width-bounded layers. Coffman–Graham first gives each vertex a precedence-aware
label, then visits labels in reverse order and places each vertex in the lowest
non-full layer strictly above its successors [[25]](#references). `libcpg`
uses component-id tie breaks and exposes the result as `CgLayering`; layer zero
contains sinks, so every condensation edge points strictly downward. See
[`components/graph/condensation-analysis.md`](components/graph/condensation-analysis.md).

### Common-subexpression elimination (CSE)

An optimization that reuses a prior evaluation when the same expression is
already [available](#available-expression). `libcpg` reports a conservative
`(first, second)` pair only when the first structural occurrence dominates the
available second occurrence; it does not perform the rewrite. See
[`components/graph/expression-analysis.md`](components/graph/expression-analysis.md).

### Constant lattice

The SCCP abstract-value order
$`Unknown \sqsubset Constant(c) \sqsubset Overdefined`$. `Unknown` is bottom
and means no executable definition has supplied information; `Overdefined` is
top and means values conflict or the operation is not safely foldable. Distinct
constants join to `Overdefined`. See [Sparse conditional constant
propagation](#sparse-conditional-constant-propagation-sccp).

### Complexity class / Big-O

An asymptotic growth category for a function's running time. `libcpg`'s
`ComplexityClass` ladder is, from cheapest to most expensive, `Constant`
$`O(1)`$, `Logarithmic` $`O(\log n)`$, `Linear` $`O(n)`$,
`Linearithmic` $`O(n \log n)`$, `Quadratic` $`O(n^2)`$, `Cubic`
$`O(n^3)`$, `Polynomial(k)` $`O(n^k)`$, `Exponential` $`O(2^n)`$,
`Factorial` $`O(n!)`$, and `Unknown`. `Polynomial(0..=3)` compares equal in
growth to its named counterpart, and every finite polynomial compares better
than `Exponential`. See
[`components/algorithms/complexity.md`](components/algorithms/complexity.md).

### Condensation graph / Condensation metrics

The graph obtained by contracting every [SCC](#strongly-connected-component-scc)
to one vertex. It is necessarily a [DAG](#directed-acyclic-graph-dag).
`condensation_metrics` reports source-oriented longest-path levels, height,
maximum level width, an SCC-membership-weighted [critical
path](#critical-path), and optional [Coffman–Graham](#coffmangraham-layering-cg)
layers without mutating the decomposition. See
[`components/graph/condensation-analysis.md`](components/graph/condensation-analysis.md).

### Confidence (pattern match)

A score in $`[0, 1]`$ that a detected [design pattern](#design-pattern) attaches to each `PatternMatch`, measuring how completely the candidate subgraph fills the pattern's template. `GofPatternDetector` keeps only matches at or above `min_confidence` (default `0.7`). See [Gang of Four](#gang-of-four-gof).

### Confidence (call resolution)

The evidence tier attached to one `CallResolution`. C1 produces
`ExactInFile`, `ExactViaImport`, `ExactViaBinding`, `BareNameUnique`,
`BareNameAmbiguous`, `External`, or `Unresolved`; only its first four tiers can
materialize one definite target. Immutable type refinement additionally returns
`ClassHierarchy`, `RapidType`, or `VariableType`. A singleton typed set is
strong enough to name one represented closed-world target, but `devirtualize`
does not itself create [call-graph](#call-graph) topology. Scores order evidence
and are not probabilities. See
[`components/graph/call-resolution.md`](components/graph/call-resolution.md)
and [`components/graph/type-refinement.md`](components/graph/type-refinement.md).

### CHA / RTA / VTA

**Class-hierarchy analysis (CHA)** admits represented concrete subtypes of a
declared receiver. **Rapid-type analysis (RTA)** intersects that set with
represented allocated types. **Variable-type analysis (VTA)** further
intersects allocations in definitions reaching the receiver. libcpg's RTA is
a closed-world allocation filter rather than whole-program reachable-method
discovery, and its VTA is assignment-type refinement rather than a full
points-to analysis. See
[`theory/42-closed-world-type-refinement.md`](theory/42-closed-world-type-refinement.md).

### Confidence (complexity evidence)

A score in $`[0,1]`$ describing how completely the documented structural model
supports a `ComplexityEstimate`; it is not the probability that the Big-O class
is mathematically exact. Fully counted reducible loop nests receive `1.0`, an
unknown trip is capped at the legacy loop-heuristic confidence, and any
irreducible cyclic region receives `0.3`. See
[`components/algorithms/complexity.md`](components/algorithms/complexity.md#confidence-policy).

### Control dependence

A [PDG](#program-dependence-graph-pdg) edge kind (`ControlDependence`): node $`n`$ is control-dependent on branch $`b`$ when whether $`n`$ executes is decided by $`b`$. `libcpg` computes it as the **reverse [dominance frontier](#dominance-frontier--reverse-dominance-frontier)** over the reversed CFG, following Ferrante–Ottenstein–Warren [[2]](#references) and the frontier algorithm of Cytron et al. [[3]](#references). See [`theory/04-program-dependence-and-slicing.md`](theory/04-program-dependence-and-slicing.md).

### Control Flow Graph (CFG)

The overlay whose edges encode possible execution order between nodes, typed by `CfgEdgeKind` (14 variants: `Sequential`, `ConditionalTrue`, `ConditionalFalse`, `LoopBack`, `LoopExit`, `Break`, `Continue`, `Return`, `Throw`, `Catch`, `Call`, `CallReturn`, `Case`, `DefaultCase`). Wrapped in the CPG as `CpgEdgeKind::ControlFlow(CfgEdgeKind)`. Built by `CfgExtractor`. See [`components/builder/cfg.md`](components/builder/cfg.md).

### Cosine similarity

A similarity measure between two vectors, used to compare [embeddings](#embedding). For vectors $`u`$ and $`v`$:

```math
\cos(u, v) = \frac{u \cdot v}{\lVert u \rVert \, \lVert v \rVert}
```

It ranges over $`[-1, 1]`$ ($`1`$ = identical direction). Exposed as `NodeEmbedding::cosine_similarity`. Also the basis of the `Cosine` [similarity metric](#similarity-metric). See [`components/gnn/embeddings.md`](components/gnn/embeddings.md).

### Core–periphery classification / Cyclic core

A comparative classification of directed-graph vertices by high/low
[visibility fan-in and fan-out](#visibility-fan-in-vfi--visibility-fan-out-vfo).
libcpg uses inclusive per-axis medians to label `Core`, `Shared`, `Control`, and
`Peripheral`. The **cyclic core** is reported separately: it is the largest
multi-vertex [SCC](#strongly-connected-component-scc), following the
directed-network architectural focus of Baldwin, MacCormack, and Rusnak
[[27]](#references). See
[`components/graph/dsm-analysis.md`](components/graph/dsm-analysis.md).

### Critical path

A maximum-weight directed path through a [DAG](#directed-acyclic-graph-dag).
For condensation analytics, each component's weight is its SCC member count;
`critical_path_weight` is therefore the largest total number of original graph
nodes on any condensation path. Dynamic programming follows the canonical
[topological order](#topological-order) and resolves equal weights by component
id. See
[`components/graph/condensation-analysis.md`](components/graph/condensation-analysis.md).

### Cyclomatic complexity

McCabe's structural complexity metric [[7]](#references): the number of linearly independent paths through a function's CFG. `libcpg` computes `cyclomatic_complexity()` as

```math
M = E - N + 2
```

where $`E`$ is the number of CFG edges and $`N`$ the number of CFG nodes (for a single connected component with one entry and one exit). See [`theory/02-control-flow-and-complexity.md`](theory/02-control-flow-and-complexity.md).

### Data dependence

A [PDG](#program-dependence-graph-pdg) edge kind (`DataDependence`): node $`u`$ (a use) is data-dependent on node $`d`$ (a definition) when $`d`$ defines a value that $`u`$ reads and the definition can reach the use. `libcpg` derives these by re-projecting DFG `DefUse`/`ReachingDef` edges within a function. See [`theory/04-program-dependence-and-slicing.md`](theory/04-program-dependence-and-slicing.md).

### Data-flow analysis

A monotone fixed-point computation that propagates abstract facts along a CFG.
Forward analyses meet predecessor output at block input; backward analyses meet
successor input at block output. `libcpg::solve` runs either direction over a
`BlockCfg` with a caller-defined `Lattice` and `DataflowAnalysis`. See the
[data-flow framework guide](components/graph/dataflow-framework.md).

### Data Flow Graph (DFG)

The overlay whose edges track how values move from definitions to uses, typed by `DfgEdgeKind` (13 variants: `DefUse`, `UseDef`, `ReachingDef`, `DataDependency`, `Parameter`, `ReturnValue`, `FieldRead`, `FieldWrite`, `IndexRead`, `IndexWrite`, `Alias`, `Dereference`, `AddressOf`). Wrapped as `CpgEdgeKind::DataFlow(DfgEdgeKind)`. Built by `DfgExtractor` using [AST-ordered reaching definitions](#ast-ordered-reaching-definitions). See [`components/builder/dfg.md`](components/builder/dfg.md).

### Def-use chain / Definition / Use

A **definition** is a program point that assigns a variable a value; a **use** is a point that reads it. A **def-use chain** links a definition to every use it reaches. `libcpg` models these with `Definition` / `DefinitionKind`, `Use` / `UseKind`, and `DefUseChain`, built by `build_def_use_chains`. See [`components/builder/dfg.md`](components/builder/dfg.md).

### Degree assortativity

Newman's Pearson correlation between degrees at the two ends of an undirected
edge. Positive values indicate like-degree attachment; negative values indicate
high-to-low-degree attachment; a star has coefficient `-1` [[31g]](#references).

### Design pattern

A reusable solution to a recurring design problem. `libcpg` detects the 23 [Gang-of-Four](#gang-of-four-gof) patterns structurally, by matching each pattern's template graph against the CPG with a relaxed [VF2](#vf2) matcher. See [`components/patterns/gang-of-four.md`](components/patterns/gang-of-four.md).

### Design Structure Matrix (DSM)

A square matrix whose rows and columns represent the same component set and
whose marked cell $`(i,j)`$ records a directed dependency from component $`i`$
to component $`j`$. Its non-reflexive transitive closure is the visibility
matrix used to derive [propagation cost](#propagation-cost) and
[VFI/VFO](#visibility-fan-in-vfi--visibility-fan-out-vfo) [[26]](#references).
libcpg consumes the equivalent sparse [`GraphProjection`](#graph-projection)
rather than materializing the matrix. See
[`theory/10-design-structure-matrix-analysis.md`](theory/10-design-structure-matrix-analysis.md).

### Directed acyclic graph (DAG)

A directed graph with no directed cycle. Every DAG has a [topological
order](#topological-order); Kahn's source-removal algorithm constructs one or
detects a cycle when no source remains [[24]](#references). Contracting the
[SCCs](#strongly-connected-component-scc) of any directed graph produces its
condensation DAG. See
[`components/graph/condensation-analysis.md`](components/graph/condensation-analysis.md).

### Dominator / Post-dominator

Node $`d`$ **dominates** node $`n`$ if every path from entry to $`n`$ passes through $`d`$; $`p`$ **post-dominates** $`n`$ if every modeled path from $`n`$ to exit passes through $`p`$. Post-dominators are dominators computed on the *reversed* CFG. `libcpg` exposes deterministic `DominatorTree` and `PostDominatorTree` results from one feature-free Cooper–Harvey–Kennedy engine over borrowed CSR and a synthetic-exit overlay. Prerequisite for [control dependence](#control-dependence). See [`components/graph/dominator-analysis.md`](components/graph/dominator-analysis.md).

### Dead branch / Unreachable code

A CFG node is **unreachable** when no directed path reaches it from its function
entry. A plain `ReachabilityReport` calls a conditional CFG edge a **dead
branch** when its target lies in an unreachable CFG island; value-sensitive
refinement can additionally prove a branch dead from a reachable constant
guard. `libcpg` preserves the nodes and reports this evidence out of graph. See
[`components/graph/reachability-analysis.md`](components/graph/reachability-analysis.md).

### Dead store

A **dead store** is a reachable, non-parameter definition of a variable whose
current value cannot be read on any modeled future CFG path before another
definition replaces it. `libcpg::dead_stores` derives conservative advisory
evidence from [liveness](#liveness); callers must account for volatile, atomic,
reflective, foreign, and language-specific setter effects before deleting a
write. See
[`components/graph/liveness-analysis.md`](components/graph/liveness-analysis.md).

### Dominance frontier / Reverse dominance frontier

The **dominance frontier** of node $`d`$ is the set of nodes where $`d`$'s dominance "stops" — the points just beyond $`d`$'s strictly dominated region (Cytron et al. [[3]](#references)). A root self-loop or reachable backedge can therefore place the root in its own frontier because a node does not strictly dominate itself. Computing the frontier on the *reversed* CFG (the **reverse dominance frontier**) yields exactly the [control-dependence](#control-dependence) relation. See [`components/graph/dominator-analysis.md`](components/graph/dominator-analysis.md) and [`theory/04-program-dependence-and-slicing.md`](theory/04-program-dependence-and-slicing.md).

### DPML (Design-Pattern Markup Language)

`libcpg`'s small YAML/TOML schema for declaring a pattern as roles (`DpmlRole`) and constraints (`DpmlConstraint`), loaded into a `DpmlTemplate` and compiled to a [`PatternTemplate`](#pattern-template). Malformed input yields `DpmlError`. Lets users add pattern templates without writing Rust. See [`components/patterns/dpml.md`](components/patterns/dpml.md).

### Effect summary / Place / Purity

An **effect summary** is a finite, compositional description of one function's
observable reads, writes, return dependencies, and constant-return evidence. A
`Place` identifies the affected storage abstractly as a global name, a
zero-based parameter position, or a field name. `Purity` orders functions from
`Pure` through `ReadsGlobal` and `WritesGlobal` to `Impure`; calling an
unresolved or external target is conservatively impure. libcpg joins these
facts over validated call-graph [SCCs](#strongly-connected-component-scc),
using the IFDS/IDE summary schedule [[41]](#references) and a flat
constant-propagation lattice related to SCCP [[22]](#references). See
[`components/graph/effect-summaries.md`](components/graph/effect-summaries.md).

### Elementary cycle / simple cycle

An **elementary** or **simple** directed cycle is a closed directed walk whose
vertices are distinct except for the implicit closing return to its start.
Cyclic rotations denote the same cycle; reversing the vertex sequence is a
different directed cycle unless the reverse edges exist. libcpg returns the
rotation with the smallest stable `NodeId` first and bounds enumeration by
length and count [[33]](#references). See
[`theory/16-bounded-simple-cycle-enumeration.md`](theory/16-bounded-simple-cycle-enumeration.md)
and [`components/graph/cycle-analysis.md`](components/graph/cycle-analysis.md).

### Directed triad / motif census

A **directed triad** is the induced topology on three vertices. Its three dyads
are mutual, asymmetric, or null; the complete Davis–Leinhardt taxonomy has
sixteen isomorphism classes. A **motif census** counts each unordered triple in
exactly one class, so the counts sum to `C(n,3)` [[34]](#references). See
[`theory/17-directed-motif-census.md`](theory/17-directed-motif-census.md) and
[`components/graph/motif-analysis.md`](components/graph/motif-analysis.md).

### Embedding

A dense real-valued vector that summarises a node (`NodeEmbedding`) or a subgraph (`SubgraphEmbedding`) so that structurally/semantically similar code lands nearby in vector space. Produced by the [GNN](#graph-neural-network-gnn) and compared with [cosine similarity](#cosine-similarity). See [`components/gnn/embeddings.md`](components/gnn/embeddings.md).

### Fact universe

A sorted, deduplicated collection of analysis facts with stable dense `u32`
identifiers. `FactUniverse<T>` makes bit-domain states and serialized solutions
independent of source graph insertion order. See the
[data-flow framework guide](components/graph/dataflow-framework.md#built-in-domains-and-fact-universes).

### Field access / resolved field reference

A **field access** is a `MemberAccess { member }` expression. Under
`include_field_access`, its receiver points to the access with `FieldRead`; an
assignment whose first AST child is that access points to it with `FieldWrite`.
A **resolved field reference** is the separate `Reference` edge from a
`self`/`this` member access to the unique same-name `Field` declaration of the
enclosing class/implemented type. Missing or ambiguous type evidence leaves the
access unresolved. See [DFG extraction](components/builder/dfg.md#complex-l-values-and-declaration-resolution).

### Feature flag (cargo)

A compile-time switch declared in `Cargo.toml` that gates optional code and dependencies. `libcpg`'s default set is **empty** (`default = []`): language grammars, pattern/algorithm detection, serde, and the GNN are each opt-in. Key flags: `lang-*` (16 grammars), `design-patterns`, `algorithm-detection`, `serde`, `gnn`, `ml-linfa`/`ml-rules`, the Mode-B toggles `rholang`/`metta`, and the umbrella `full`. See [`engineering/01-build-and-features.md`](engineering/01-build-and-features.md).

### Feature vector (classification)

The fixed-length numeric summary of a candidate subgraph (12 fields) that `PatternClassifier` scores to label a [design pattern](#design-pattern) — an alternative to template matching. `ClassificationMode` selects rule-based, ML (`ml-linfa`), or hybrid scoring. Not to be confused with a graph feature vector used by the `Cosine` [similarity metric](#similarity-metric). See [`components/patterns/classification.md`](components/patterns/classification.md).

### Fenwick tree

A flat indexed tree for prefix populations and order-statistic selection in
$`O(\log n)`$ time. `Vf2State` pairs one with dense membership bits so it can
select the next canonical terminal or unused node without sorting or scanning
a mapped prefix. Initially full sets index absent entries through the
complement, allowing zero-filled lazy construction. See
[`design/0057-dense-stack-safe-vf2-state.md`](design/0057-dense-stack-safe-vf2-state.md).

### Gang of Four (GoF)

The four authors of *Design Patterns* [[11]](#references) and, by metonymy, the 23 patterns catalogued there, grouped into `GofCategory::Creational` (5), `Structural` (7), and `Behavioral` (11). `libcpg` names them with the `GofPattern` enum — note the variant is `FactoryMethod` (never `Factory`). See [`components/patterns/gang-of-four.md`](components/patterns/gang-of-four.md).

### Graph edit distance

The minimum cost of node/edge insertions, deletions, and substitutions that
turn one graph into another. `libcpg::pattern::ged_upper_bound` uses
Riesen–Bunke bipartite node assignment [[35a]](#references) solved by the
Hungarian method [[35b]](#references), then prices the induced concrete typed
edit script. Within 512 nodes per input it returns a symmetric upper bound;
beyond the cap it marks a legacy compatibility distance with `capped = true`.
See [`components/patterns/ged-analysis.md`](components/patterns/ged-analysis.md).

### Graph Neural Network (GNN)

A neural network that computes node representations by repeatedly [aggregating](#aggregation-gnn) information from neighbours ([message passing](#message-passing-gnn)), pioneered for graphs by Scarselli et al. [[9]](#references) and applied to vulnerability detection by Devign (Zhou et al. [[12]](#references)). `libcpg`'s `CpgGnn` (feature `gnn`) owns a CPG and produces [embeddings](#embedding). See [`components/gnn/overview.md`](components/gnn/overview.md).

### Graph projection

A deterministic, payload-free directed graph over sorted stable `NodeId`
values, stored as forward and reverse compressed-sparse-row adjacency.
`GraphProjection` selects one CPG overlay or accepts an external graph without
cloning node payloads. It is the common feature-free substrate for dominance,
SCC, DSM, WL, and other exact analyses. `is_well_formed` checks the complete
forward/reverse CSR contract before public-field consumers traverse it. See
[`components/graph/projections.md`](components/graph/projections.md).

### Idempotent

An operation that has the same effect whether applied once or many times. `CfgExtractor::extract`, `DfgExtractor::extract`, and `PdgBuilder::build` are idempotent: re-running them does not duplicate edges. This lets construction stages be re-applied safely.

### Indexed parallel collection / Rayon pool

An **indexed parallel collection** retains the input order of independent work
items even though a **Rayon pool** may execute them in another schedule.
libcpg uses this property to collect per-function and per-pattern
[analysis plans](#analysis-plan--ordered-emitter), then emits or returns them in
serial order. Callers control pool width and worker-stack size by installing an
operation in their own Rayon pool; the default builder path remains serial.

### IFDS / IDE

**Interprocedural Finite Distributive Subset (IFDS)** analysis reduces
finite-fact, distributive interprocedural data flow to reachability in an
exploded supergraph. **Interprocedural Distributive Environment (IDE)**
analysis generalizes the propagated value to a distributive environment
transformer. libcpg's feature-free framework schedules analysis-defined
procedure summaries in callee-first strongly-connected-component order and
returns explicit cap or structural evidence [[41]](#references). See
[`components/graph/ifds-ide-framework.md`](components/graph/ifds-ide-framework.md).

### Isomorphism / Subgraph isomorphism

A **graph isomorphism** is a bijection between two graphs' nodes that preserves edges. **Subgraph isomorphism** asks whether a (small) *pattern* graph is isomorphic to some subgraph of a (large) *target* graph — the core question in pattern detection. It is NP-complete in general; `libcpg` solves it with [VF2](#vf2). See [`theory/05-subgraph-isomorphism-vf2.md`](theory/05-subgraph-isomorphism-vf2.md).

### Irreducible loop region

A cyclic control-flow region with no single dominating header, commonly because
control enters at multiple nodes. `libcpg` detects non-dominating DFS retreating
edges and reports the whole cyclic SCC with `NaturalLoop::irreducible = true`,
making the conservative approximation explicit (Havlak [[17]](#references)).

### Induction variable / Loop invariant / Trip evidence

A **loop-invariant** computation has modeled operands whose reaching
definitions are outside the loop region or already invariant. A **basic
induction variable** changes by one invariant additive step on every path back
to the loop header; a **derived induction variable** is a one-level affine
expression $`a i + b`$ of a basic variable. `loop_induction` also reports
**trip evidence** when a reducible loop compares an induction variable with an
invariant bound in a compatible direction. `Counted` is structural evidence,
not an exact count or termination proof. Its `hoistable` invariant subset is
advisory [loop-invariant code motion](#loop-invariant-code-motion-licm)
evidence. See
[`components/graph/induction-analysis.md`](components/graph/induction-analysis.md).

### Interval / Value range

A non-empty closed set $`[lo,hi]`$ over extended integer bounds. `Bound` adds
negative and positive infinity to finite `i128` endpoints; `IntervalEnv` maps
stable variable ids to intervals, joins paths by convex hull, widens outward
loop movement to infinity, and narrows afterward. See
[`components/graph/interval-analysis.md`](components/graph/interval-analysis.md).

### Jaccard similarity

The size of the intersection over the size of the union of two sets:

```math
J(A, B) = \frac{|A \cap B|}{|A \cup B|}
```

`libcpg`'s default [similarity metric](#similarity-metric) applies it to the multisets of node kinds of two graphs. See [`theory/06-graph-similarity.md`](theory/06-graph-similarity.md).

### K-core

The unique maximal induced subgraph in which every retained vertex has degree
at least $`k`$. A vertex's **coreness** is the greatest threshold whose core
still contains it. libcpg computes every core number simultaneously using
deterministic bin-sort peeling [[31a–31b]](#references).

### K-truss

A maximal subgraph in which every retained edge participates in at least
$`k-2`$ triangles within that same subgraph. An edge's **trussness** is the
greatest threshold whose truss contains it. Unlike one-time triangle count,
truss decomposition repeatedly updates support as edges are peeled [[31c]](#references).

### Kill / Gen (data-flow)

In [reaching-definitions](#reaching-definition) analysis, processing a definition of variable $`x`$ **generates** (`gen`) the new definition and **kills** (`kill`) prior definitions of $`x`$. A [strong update](#strong-update--weak-update) does both; a weak update only generates. See [`theory/03-data-flow-and-reaching-definitions.md`](theory/03-data-flow-and-reaching-definitions.md).

### Lattice (data-flow)

The algebraic structure — a partially ordered set with meet/join — over which
classical data-flow analyses iterate to a fixed point, formalised by Kildall
[[6]](#references) and Kam–Ullman [[19]](#references). `libcpg::Lattice`
requires `join` and `leq`; built-in may and must bit domains use union/subset and
intersection/reverse-subset respectively. See
[`theory/03-data-flow-and-reaching-definitions.md`](theory/03-data-flow-and-reaching-definitions.md).

### Least-recently-used replacement (LRU)

A deterministic cache replacement policy that evicts the entry with the
oldest successful access or insertion stamp. `FunctionAnalysisCache` uses LRU
only after full canonical-key equality has established identity; the digest is
an index, not an identity proof. Entry and estimated resident-byte limits are
both hard bounds. See
[`components/builder/incremental-analysis-cache.md`](components/builder/incremental-analysis-cache.md).

### Function-analysis cache / Canonical analysis key / Function-local ordinal

A **function-analysis cache** is the caller-owned bounded store used by the
cached builder and PDG APIs. A **canonical analysis key** is a versioned exact
byte encoding of every represented input dimension for one overlay, including
language/configuration, local tree identity, and required dependency evidence.
A **function-local ordinal** is the position of a node in that function's
stable local order; cached plans store ordinals rather than build-specific
`NodeId` values. Cross-boundary evidence that cannot be represented exactly
causes an observable bypass and fresh planning.

### Leiden algorithm / Leiden-style refinement

The Leiden algorithm improves [Louvain](#louvain-algorithm) by interleaving
local moving, a probabilistic refinement phase, and aggregation, with stronger
community-connectivity guarantees [[30]](#references). libcpg does not
implement the complete Leiden algorithm. Its separately callable
`try_refine_connected` operation is **Leiden-style** only in the narrow sense
that it repairs disconnected assigned groups; it guarantees ordinary induced
connectivity and claims none of Leiden's stronger optimality properties.

### Liveness

A variable is **live** at a program point when some future control-flow path
may read its current value before another definition replaces it. Liveness is a
backward may analysis: successor inputs meet by union, and a node removes its
definitions before adding its uses. `libcpg::liveness` returns deterministic
per-node `live_in` and `live_out` sets. See
[`components/graph/liveness-analysis.md`](components/graph/liveness-analysis.md).

### Louvain algorithm

A multi-level [community-detection](#community-detection) heuristic introduced
by Blondel et al. [[29]](#references). Each level repeatedly moves vertices to
neighboring communities when that improves generalized modularity, then
contracts the resulting communities into weighted vertices. libcpg fixes
vertex order, candidate order, exact ties, aggregation reductions, and output
numbering so the bounded heuristic is reproducible.

### Loop-invariant code motion (LICM)

A compiler transformation that moves a loop-invariant computation to a
preheader so it executes once instead of once per iteration. Legal motion
requires more than invariance: execution-frequency, dominance, definition/use,
side-effect, trap, aliasing, volatile/atomic, and concurrency obligations can
all matter. `libcpg` reports only the structurally checked `hoistable` evidence
subset and does not rewrite source. See [Induction variable / Loop invariant /
Trip evidence](#induction-variable--loop-invariant--trip-evidence).

### May analysis / Must analysis

A **may** fact is true when it holds on at least one control-flow path, usually
using union at confluence. A **must** fact is true only when it holds on every
path, usually using intersection. `Confluence` records this intent; the domain's
`Lattice::join` supplies the actual operation.

### Channel event / Channel identity

A **channel event** is a frontend-normalized `Send`, `SendSync`, linear `Recv`,
persistent `RecvPersistent`, non-consuming `Peek`, `Select`, or `Spawn` site.
Its optional **channel identity** is the `NodeId` of the `Variable` definition
supported by `Identifier::definition`, `DefUse`/`ReachingDef`, or `Reference`
evidence—not a source spelling. libcpg groups these events per top-level process
and reports structural orphan, blocked, and linear-contention evidence. See
[Theory 29](theory/29-channel-topology-and-linearity.md) and Meredith–Radestock
[[43]](#references).

### Deadlock candidate / Wait-for graph / Lock-order graph

A **deadlock candidate** is a cyclic structural dependency, not a proof of a
reachable deadlocked schedule. In a process **wait-for graph**, `P -> Q` means
`P` is initially blocked on a resolved linear receive whose known producers are
all initially blocked, and `Q` is one producer. In a resource **lock-order
graph**, `A -> B` means typed evidence shows `B` can be acquired while `A` is
held. libcpg projects these relations into deterministic CSR graphs and reports
cyclic [strongly connected components](#strongly-connected-component-scc) with
statement sites and source ranges. See [Theory
30](theory/30-cyclic-wait-and-lock-order.md) and Kobayashi [[44]](#references).

### Lock kind / Lock mode

`LockKind` is language-neutral metadata attached orthogonally to a `Call`:
`AcquireRead`, `AcquireWrite`, or `Release`. `LockMode` is `Read` or `Write` and
is retained in lock-order witnesses. A semantic adapter assigns the kind after
API resolution; the deadlock analyzer does not guess from a function name.

### May-happen-in-parallel relation / Parallel operand

A **may-happen-in-parallel relation** (MHP relation) is a symmetric relation
between execution sites whose typed AST structure permits concurrent execution.
A **parallel operand** is one direct AST child of a `Block` carrying
`MsgKind::Spawn`; the child may be a direct process root or a wrapper block.
libcpg relates sites across different operands, stores each pair once in
canonical `NodeId` order, and exposes whether its quadratic-output cap was
reached. MHP is not by itself a conflicting access or data-race finding. See
[Theory 31](theory/31-may-happen-in-parallel.md) and Naumovich–Avrunin
[[45]](#references).

### Data-race candidate / Atomicity candidate / Execution strand / Lockset

A **data-race candidate** is a canonical MHP pair whose exact represented
location is accessed at least once for writing and whose endpoint
**locksets** contain no common mutually excluding acquisition. A lockset is the
multiset of exact typed lock acquisitions held at an access site; two shared
read modes do not exclude one another. An **atomicity candidate** is a
read-modify-write pair split by a resolved release/reacquire gap, together with
an other-strand same-location access that is MHP with both endpoints and is not
excluded by a different continuously held acquisition. An **execution strand**
is the unique nearest function or direct typed parallel operand used only for
lexical access and lock ordering. These are evidence-bounded advisories, not
runtime race proofs. See [Theory
43](theory/43-evidence-bounded-race-and-atomicity.md), RacerD [[65]](#references),
and RacerX [[66]](#references).

### Session sketch / Endpoint trace / Duality

A **session sketch** is libcpg's ordered, shallow behavioral summary for one
resolved channel. An **endpoint trace** is the sequence of send/receive actions
at one top-level process. Two binary linear traces satisfy **duality** when they
have equal length and payload arity and every corresponding direction is
opposite with an equal known payload type. `Conformant` is positive shallow
evidence, `Nonconformant` records a concrete mismatch, and `Inconclusive`
preserves missing, non-linear, or non-binary evidence. This is not a complete
session type system or deadlock-freedom proof. See [Theory
32](theory/32-session-protocol-duality.md) and Honda–Vasconcelos–Kubo
[[46]](#references).

### Typestate / Property automaton / Allocation-site abstraction

**Typestate** is a finite abstract state associated with an object's permitted
operations. A **property automaton** supplies states, call events, deterministic
transitions, allocation seeds, accepting terminal states, and an absorbing
error state; an absent ordinary transition is forbidden. **Allocation-site
abstraction** gives every allocation call one static object identity, so
different call sites remain distinct while repeated runtime instances at one
loop call may be merged. libcpg propagates these identities over typed
DFG/reference/alias evidence and evaluates property facts over the canonical
CFG. Results are advisory and require a complete report for negative claims.
See [Theory 33](theory/33-finite-state-typestate-properties.md), Strom–Yemini
[[48]](#references), and Fink et al. [[49]](#references).

### API Usage Graph (AUG) / API-usage fragment / Temporal property

An **API Usage Graph (AUG)** is libcpg's directed, labeled, acyclic multigraph
centered on one complete call occurrence. Typed argument, receiver, and control
nodes point into the anchor; CFG-reachable later calls point out. An
**API-usage fragment** is one comparable following-call, arity, positional
shape, receiver-shape, or control-context relation whose exact support reaches
a strict-majority threshold. A **temporal property** is an explicitly named
finite-state operation-order policy checked through typestate, independent of
the learned population. Findings are advisory and qualified by completeness.
See [Theory 37](theory/37-api-usage-graphs-and-temporal-properties.md),
ASAP-Repair [[56]](#references), and ProveNFix [[57]](#references).

### CWE template / Structural qualification / CWE candidate

A **Common Weakness Enumeration (CWE) template** is libcpg's declarative value
that combines a finite typed graph shape, origin/neutralizer/sensitive-use
roles, and one shared semantic primitive. **Structural qualification** is the
requirement that a semantic site also occur as the designated sink of a
retained bounded VF2 embedding. A **CWE candidate** is the resulting immutable,
out-of-graph advisory evidence; it is not an exploitability verdict. A negative
represented-domain claim requires `CweAnalysis::is_complete()`. See
[Theory 38](theory/38-declarative-cwe-query-semantics.md), Yamaguchi et al.
[[58]](#references), Li et al. [[59]](#references), and Lekssays et al.
[[60]](#references).

### Resource leak

A **resource leak** candidate is an allocation-site object that reaches a
represented semantic function exit in a non-accepting typestate. It is distinct
from a forbidden transition: a path can perform only legal operations yet omit
the release required by terminal policy. libcpg retains the allocation, exit,
state, and canonical typestate witness [[50]](#references). See [Theory
34](theory/34-advisory-bug-detection.md).

### Uninitialized use / Scoped reaching definition

An **uninitialized use** candidate is a binder-aware executable variable read
whose represented reaching-definition set contains no definition in the same
function scope. A **scoped reaching definition** is a reaching definition that
belongs to that scope; definitions from other functions are retained as
diagnostic evidence but do not initialize the local use. Declaration-signature
and unbound callee identifiers are syntax rather than reads. See [Theory
34](theory/34-advisory-bug-detection.md) and Reps–Horwitz–Sagiv
[[41a]](#references).

### Exception summary / Uncaught exception / Dead catch

An **exception summary** is the finite ordered set of nominal exception types
that may escape one represented function after lexical handler dispatch. An
**uncaught exception** candidate is a summary type escaping a function in a
source component of the call condensation graph, with a canonical call/throw
witness. A **dead catch** candidate is a represented catch clause to which no
direct or propagated exception type is dispatched. These are advisory static
relations, not runtime feasibility or impact verdicts [[51]](#references). See
[Theory 34](theory/34-advisory-bug-detection.md).

### Message passing (GNN)

One round of GNN computation: every node updates its vector from its neighbours' vectors. `libcpg` uses mean aggregation with a ReLU nonlinearity over AST, CFG, and DFG neighbourhoods:

```math
h_v^{(k)} = \mathrm{ReLU}\!\left( \mathrm{mean}\left( \{ h_u^{(k-1)} : u \in \mathcal{N}(v) \} \cup \{ h_v^{(k-1)} \} \right) \right)
```

where $`\mathcal{N}(v)`$ is $`v`$'s neighbourhood across the three overlays and $`k`$ indexes the layer (up to `num_layers`). See [`components/gnn/message-passing.md`](components/gnn/message-passing.md).

### MeTTa

A language for the [F1R3FLY.io](#rholang) / Hyperon ecosystem based on rewriting over symbolic [S-expressions](#s-expression). `libcpg` maps MeTTa to CPG nodes through [Mode B](#mode-b--build_from_tree) (`map_metta`): e.g. `(= (double $x) (* $x 2))` becomes a `Function` "double" with `$x` as a `Parameter` flowing to its use. See [`usage/06-f1r3fly-rholang-metta.md`](usage/06-f1r3fly-rholang-metta.md).

### Minimum cut

A nontrivial vertex bipartition whose crossing edges have minimum total
weight. `global_min_cut` uses deterministic bounded Stoer–Wagner phases and
returns the smaller, lexicographically canonical side [[31f]](#references).

### Mode B / `build_from_tree`

The construction path where the **caller supplies an already-parsed** `tree_sitter::Tree` and `libcpg` builds the CPG from it: `TreeSitterCpgBuilder::build_from_tree(&tree, source, language)`. It needs no `lang-*` feature (the caller owns the grammar), skips the `max_file_size` check, and is the only path for [Rholang](#rholang) and [MeTTa](#metta). "Mode A" is the internal-parse path (`build`). See [`design/0002-mode-b-build-from-tree.md`](design/0002-mode-b-build-from-tree.md).

### Modularity / Resolution

**Modularity** $`Q`$ compares the observed edge weight inside assigned
communities with the expected internal weight under a degree-preserving null
model. Generalized modularity introduces a nonnegative **resolution parameter**
$`\gamma`$: larger values usually favor smaller communities, while
$`\gamma=1`$ is standard modularity. The score is comparative evidence for one
projection and resolution; it is neither a probability nor proof of a correct
architecture. See [community structure
theory](theory/13-community-structure-and-modularity.md).

### Natural loop / Loop forest

A **natural loop** is the [header](#back-edge--header--latch) plus the reverse
predecessor closure of all its latches. A **loop forest** nests loop bodies by
strict set containment and records each node's innermost loop. `libcpg` exposes
`LoopForest`, `NaturalLoop`, and `LoopExitEdge` through feature-free
`loop_forest`. See [`components/graph/loop-analysis.md`](components/graph/loop-analysis.md).

### Node kind / Edge kind

The type tag on a CPG node (`CpgNodeKind`, 45 variants — a mix of unit variants such as `Root`, `If`, `Return` and data-carrying variants such as `Function { signature }`, `Call { target, is_method }`) or edge (`CpgEdgeKind`). Kinds drive every query and every pattern/complexity heuristic. See [`components/graph/nodes.md`](components/graph/nodes.md) and [`components/graph/edges.md`](components/graph/edges.md).

### Partial-redundancy elimination (PRE)

An optimization that makes an expression fully redundant by placing a
computation where it is [very busy](#very-busy-expression--anticipability),
then removes redundant evaluations. Morel and Renvoise [[21]](#references)
introduced the classical global formulation. `libcpg` reports advisory
structural-expression/common-dominator placements; it does not rewrite source.
See [`components/graph/expression-analysis.md`](components/graph/expression-analysis.md).

### Pattern template

A declarative description of a pattern as node constraints (`NodeConstraint`) and edge constraints (`EdgeConstraint`) that `.to_pattern_graph()` compiles into a target graph for [VF2](#vf2) matching. `PatternTemplate` lives in the `pattern::` module; the [DPML](#dpml-design-pattern-markup-language) loader compiles YAML/TOML into one. See [`components/patterns/vf2-matching.md`](components/patterns/vf2-matching.md).

### petgraph

The Rust graph library `libcpg` builds on. `CodePropertyGraph` wraps a `petgraph::graph::DiGraph<CpgNode, CpgEdge>` for storage and stable `NodeIndex`/`EdgeIndex` adjacency. Exact dominance uses libcpg's deterministic `GraphProjection` engine; petgraph's independent dominator implementation is a test oracle. See [`design/0001-unified-overlay-graph.md`](design/0001-unified-overlay-graph.md).

### Program Dependence Graph (PDG)

The overlay of [control-dependence](#control-dependence) and [data-dependence](#data-dependence) edges, introduced by Ferrante–Ottenstein–Warren [[2]](#references). It is the substrate for [program slicing](#program-slicing). Added on demand by `PdgBuilder::build(&mut cpg, function)` (it is *not* built during initial construction). See [`theory/04-program-dependence-and-slicing.md`](theory/04-program-dependence-and-slicing.md).

### System Dependence Graph (SDG) / Summary edge

A **System Dependence Graph (SDG)** extends per-function PDGs with
actual-to-formal `Parameter`, return-expression-to-call `ReturnValue`, and
caller-local **summary edges**. In libcpg a summary edge is
`DataDependence(label="summary")` from an actual argument to its call when the
callee effect summary says that formal may influence the return. The SDG is an
overlay on the CPG's shared node set, not a parallel graph. Two-pass slicing
excludes return flow while ascending and parameter flow while descending to
avoid unrealizable sibling-call paths, following Horwitz, Reps, and Binkley
[[42]](#references). See
[`theory/28-system-dependence-and-slicing.md`](theory/28-system-dependence-and-slicing.md).

### Program CPG / File scope / Import graph

A **program CPG** (`ProgramCpg`) is one collision-free, append-only
[CPG](#code-property-graph-cpg) made from independently identified file CPGs.
A **file scope** (`FileScope`) records one input's metadata, translated root,
sorted program nodes, and source-to-program `NodeId` map. The **import graph**
(`ImportGraph`) is an ordered evidence summary paired with `Imports` and
`Exports` overlay edges on those same nodes; it is not a second authoritative
graph store. Declaration-level `Imports` labels name the local binding used by
cross-file call resolution. See
[`components/graph/program-composition.md`](components/graph/program-composition.md).

### Program slicing

Reducing a program to just the statements that affect (or are affected by) a chosen point of interest, the **slicing criterion**. Introduced by Weiser [[8]](#references). `libcpg` computes [backward and forward slices](#backward-slice--forward-slice) over the PDG. See [`usage/04-program-slicing.md`](usage/04-program-slicing.md).

### Propagation cost

The density of the non-reflexive transitive-closure visibility matrix of a
[DSM](#design-structure-matrix-dsm): the number of reachable ordered pairs
divided by $`n^2`$ for $`n`$ components [[26]](#references). libcpg computes it
from repeated graph searches without storing the $`n\times n`$ matrix. See
[`components/graph/dsm-analysis.md`](components/graph/dsm-analysis.md).

### Reaching definition

A definition of a variable that reaches a program point with no intervening redefinition on some path. The `ReachingDef` DFG edge connects such a definition to the use it reaches. Computed by [AST-ordered reaching definitions](#ast-ordered-reaching-definitions). See [`theory/03-data-flow-and-reaching-definitions.md`](theory/03-data-flow-and-reaching-definitions.md).

### ReLU

The **Rectified Linear Unit** activation $`\mathrm{ReLU}(x) = \max(0, x)`$, applied element-wise after each GNN [aggregation](#aggregation-gnn) to introduce nonlinearity. See [Message passing](#message-passing-gnn).

### Rholang

The concurrent process-calculus language of the [F1R3FLY.io](https://f1r3fly.io) / RChain ecosystem, based on the reflective higher-order **ρ-calculus** (a **process calculus**: a formalism for concurrent, communicating processes). `libcpg` maps Rholang onto CPG vocabulary through [Mode B](#mode-b--build_from_tree) (`map_rholang`): `contract` → `Function`, `x!(…)` send → `Call`, `new`-bound channel → `Variable`, a `rho:` URI → `Import`. See [`usage/06-f1r3fly-rholang-metta.md`](usage/06-f1r3fly-rholang-metta.md).

### S-expression

A symbolic expression: either an atom or a parenthesised list of S-expressions — the uniform syntax of Lisp-family and [MeTTa](#metta) code. `libcpg`'s MeTTa mapper dispatches on each list's head atom. See [`usage/06-f1r3fly-rholang-metta.md`](usage/06-f1r3fly-rholang-metta.md).

### Similarity metric

The strategy `GraphSimilarity` uses to score two graphs' likeness:
`SimilarityMetric::Jaccard` (default), `Cosine`, `WeisfeilerLehman`, or
`GraphEdit`. The first three use their own direct signals. `GraphEdit` is one
minus the normalized bipartite [GED](#graph-edit-distance) upper bound within
its budget; structural/label weights are retained only for its capped legacy
fallback. See [`theory/06-graph-similarity.md`](theory/06-graph-similarity.md).

### Sparse conditional constant propagation (SCCP)

A fixed-point analysis that proves scalar constants and executable CFG edges
together. Boolean constants prune one true/false edge; value cells propagate
through AST dependencies and DFG `DefUse` edges, joining only executable
definitions. libcpg implements Wegman–Zadeck SCCP [[22]](#references) and can
refine structural reachability without mutating the CPG. See
[`components/graph/sccp-analysis.md`](components/graph/sccp-analysis.md).

### SIMD / AVX2 / runtime feature dispatch

**Single instruction, multiple data** (SIMD) applies one operation to several
independent lanes. **Advanced Vector Extensions 2** (AVX2) is the x86/x86-64
256-bit integer-vector instruction set used by libcpg's private dense bit-set
union and intersection kernels. **Runtime feature dispatch** checks the actual
CPU before calling target-feature code; it differs from compiling the whole
crate for that CPU. Widths below the measured crossover, subset checks, CPUs
without AVX2, and non-x86 targets use the scalar oracle. See
[ADR-0063](design/0063-runtime-dispatched-bitset-kernels.md) and
[scientific validation 58](scientific/58-runtime-dispatched-bitset-validation.md).

### Resolver symbol index / DAT / DAWG

A **resolver symbol index** maps an exact represented identifier spelling and
its lexical/import context to canonically ordered declaration candidates. A
**double-array trie** (DAT) places trie transitions in parallel BASE/CHECK
arrays; a **directed acyclic word graph** (DAWG) merges nodes with equivalent
right languages. Both can accelerate dictionary operations, but neither
changes libcpg's evidence boundary: names remain exact, case-sensitive `str`
values and ambiguity uses the full pre-cap candidate count. libcpg retains its
call-local ordered map after the measured candidates lost the cold resolver or
resource controls. See
[ADR-0064](design/0064-retain-ordered-resolver-symbol-index.md) and
[scientific validation 59](scientific/59-resolver-symbol-index-validation.md).

### Fuzzy resolver candidate

A **fuzzy candidate** is a name admitted by an explicitly selected edit metric
and bound, such as Levenshtein distance two. It is not an exact identifier and
does not authorize a `Call::target` or call edge. libcpg exposes no fuzzy
resolver API; an exact miss remains unresolved. A separate advisory interface
would need to expose its metric, work/candidate caps, ordering, and
completeness. See
[`usage/32-exact-call-resolution.md`](usage/32-exact-call-resolution.md).

### Structural clone class

A deterministically ordered set of at least two stable `NodeId` values that
share one final projection-level [WL](#weisfeilerlehman-wl-refinement) label.
It is a bounded topological candidate class, not proof of exact isomorphism,
source equality, or behavioral equivalence. See
[`components/graph/wl-analysis.md`](components/graph/wl-analysis.md).

### Function clone type

A relation between function-rooted program regions. **Type-1** preserves
retained syntax after ignoring comments and locations; **Type-2** abstracts
identifier and literal payloads; **Type-3** admits statement changes measured
by multiset similarity; **Type-4** compares program-dependence structure.
libcpg classifies pairs by the strongest established relation and groups
same-type pair edges into clone classes. This is distinct from a projection
[structural clone class](#structural-clone-class), which groups individual
vertices by one WL label. See
[`theory/23-function-clone-detection.md`](theory/23-function-clone-detection.md).

### Static Single Assignment (SSA)

An IR form in which every variable is assigned exactly once, with $`\phi`$-functions merging values at control-flow joins (Cytron et al. [[3]](#references)). `libcpg` deliberately does **not** use SSA for its DFG (it uses [AST-ordered reaching definitions](#ast-ordered-reaching-definitions)); SSA is defined here to make that contrast precise. See [`design/0003-ast-ordered-reaching-defs.md`](design/0003-ast-ordered-reaching-defs.md).

### Stable Dependencies Principle (SDP)

The architectural rule that dependencies should point toward stability
[[36]](#references). For a module dependency $`X\to Y`$, libcpg reports an SDP
violation exactly when $`I(X)<I(Y)`$: the more stable source depends on the less
stable target. Equal instability is compliant. A report aggregates module
pairs while retaining a concrete projection-edge witness; it is advisory
because projection and module boundaries are caller policy. See [Martin
package metrics](#martin-package-metrics).

### Weisfeiler–Lehman (WL) refinement

An iterative graph-coloring procedure that replaces each vertex label with a
digest of its current label and the multiset of neighboring labels. libcpg's
projection API starts from directed in/out degree, uses a fixed mixer and at
most 16 rounds, and returns [structural clone classes](#structural-clone-class).
The separate `GraphSimilarity` metric starts from CPG node kinds and compares
three-round label histograms. See
[`theory/11-weisfeiler-lehman-structural-clones.md`](theory/11-weisfeiler-lehman-structural-clones.md).

### WL refinement buffer

A call-local, one-dimensional `Vec<u64>` row aligned with one
`GraphProjection`'s dense node order. Projection-level WL alternates a current
and next label matrix, clearing and refilling the next rows while retaining
their capacities between the at-most-16 rounds. The canonical-vocabulary and
incident-neighbor vectors follow the same lifetime. These are explicit
heap-resident state-machine buffers, not a dense adjacency matrix, global
cache, thread-local high-water mark, or serialized result. See
[`design/0061-reusable-wl-refinement-buffers.md`](design/0061-reusable-wl-refinement-buffers.md).

### Strong update / Weak update

When a reaching-definitions sweep processes a definition of $`x`$ in **straight-line** context it performs a **strong update** — it [kills](#kill--gen-data-flow) prior definitions of $`x`$ and gens the new one. Inside a **conditional region** (where the definition may or may not execute) it performs a **weak update** — it adds the new definition without killing the old, since both may reach later uses. See [`theory/03-data-flow-and-reaching-definitions.md`](theory/03-data-flow-and-reaching-definitions.md).

### Strongly-connected component (SCC)

A maximal set of vertices in a directed graph where every vertex can reach every other vertex. `libcpg` computes exact SCC partitions for per-function CFGs and the resolved function call graph; cyclic CFG components identify loop regions, while cyclic call-graph components identify direct or mutual recursion. Contracting each SCC produces a directed acyclic condensation graph (CLRS [[13]](#references)). See [`components/graph/scc-analysis.md`](components/graph/scc-analysis.md) and [condensation analytics](components/graph/condensation-analysis.md).

### Subgraph

A graph formed from a subset of another graph's nodes and the edges among them. `libcpg` extracts subgraphs (`subgraph`, `function_cfg`, `function_dfg`) and matches pattern subgraphs against the CPG. See [Subgraph isomorphism](#isomorphism--subgraph-isomorphism).

### Taint catalog / Taint witness

A **taint catalog** is caller-owned policy that classifies represented call
paths as open source, sink, sanitizer, or external-model classes. A **taint
witness** is the canonical ordered CPG-node path supporting one candidate
source-to-sink flow. `libcpg` propagates these facts through typed AST/DFG
relations and interprocedural summaries; it does not infer policy by parsing
source text. Findings are advisory, and only a complete report licenses an
absence claim. This is a classic CPG application (Yamaguchi et al.
[[1]](#references)). See [Theory 27](theory/27-interprocedural-taint-and-nullflow.md)
and the [component guide](components/graph/taint-analysis.md).

### Maybe-null flow

An advisory flow from a null literal, uninitialized variable, or typed
nullable return to a represented dereference. The maybe-null domain uses the
same bounded summary and witness machinery as taint, but its origins and sinks
come from typed CPG evidence rather than catalog policy. See
[`security/02-taint-analysis.md`](security/02-taint-analysis.md).

### Terminal set (VF2)

In [VF2](#vf2), the sets of candidate nodes adjacent to the current partial mapping (the "fringe"), used to generate the next candidate pair and to test feasibility. `libcpg`'s matcher restores terminal sets exactly on backtracking (the "pop-order fix"). See [`theory/05-subgraph-isomorphism-vf2.md`](theory/05-subgraph-isomorphism-vf2.md).

### Topological order

A linear ordering of a [DAG's](#directed-acyclic-graph-dag) vertices in which
every edge's source precedes its target. `libcpg` uses Kahn's algorithm with a
minimum-component-id ready heap, making condensation levels and critical-path
tie breaks independent of edge insertion order [[24]](#references). See
[`components/graph/condensation-analysis.md`](components/graph/condensation-analysis.md).

### Tree-sitter

The incremental parser generator `libcpg` uses to turn source text into a concrete syntax tree. `ParserRegistry` holds the feature-gated grammars for the 16 built-in languages; [Mode B](#mode-b--build_from_tree) accepts a tree the caller parsed with its own grammar. See [`architecture/language-frontends.md`](architecture/language-frontends.md).

### Very-busy expression / Anticipability

An expression is **very busy**, or **anticipable**, after a program point when
every path to a modeled exit evaluates it before defining one of its operands.
It is a backward [Must analysis](#may-analysis--must-analysis) with intersection
at branches and supplies evidence for [PRE](#partial-redundancy-elimination-pre).
See [`components/graph/expression-analysis.md`](components/graph/expression-analysis.md).

### Visibility fan-in (VFI) / Visibility fan-out (VFO)

For the non-reflexive transitive closure $`R`$ of a directed graph,
$`VFO(i)=\sum_jR_{ij}`$ counts distinct other vertices reachable from $`i`$,
while $`VFI(i)=\sum_jR_{ji}`$ counts distinct other vertices that can reach
$`i`$. Their totals are equal because both count reachable ordered pairs.
They supply [propagation cost](#propagation-cost) and median
[core–periphery](#coreperiphery-classification--cyclic-core) evidence. See
[`theory/10-design-structure-matrix-analysis.md`](theory/10-design-structure-matrix-analysis.md).

### VF2

The subgraph-isomorphism algorithm of Cordella, Foggia, Sansone, and Vento [[4]](#references): a depth-first state-space search that grows a partial node mapping, pruning with feasibility rules over node kinds and incident edges, and backtracking when stuck. `libcpg`'s `Vf2Matcher` uses dense arrays, [Fenwick](#fenwick-tree)-ordered sets, flat undo logs, and a cursor-only heap pushdown automaton so backtracking restores the mapping and [terminal sets](#terminal-set-vf2) exactly without depth-dependent native recursion. Worst-case cost is $`O(N!\,N)`$ but pruning makes it practical. See [`theory/05-subgraph-isomorphism-vf2.md`](theory/05-subgraph-isomorphism-vf2.md).

### Weisfeiler-Lehman kernel / label refinement

A graph-similarity method (Weisfeiler & Leman [[10a]](#references); graph kernels by Shervashidze et al. [[10b]](#references)) that iteratively refines each node's label to a hash of its own label plus the sorted multiset of neighbour labels; the histogram of labels after $`k`$ iterations becomes a feature vector for comparison. `libcpg` uses 3 iterations in the `WeisfeilerLehman` [similarity metric](#similarity-metric). See [`theory/06-graph-similarity.md`](theory/06-graph-similarity.md).

### Widening / Narrowing

Abstract-interpretation convergence operators introduced by Cousot and Cousot
[[20]](#references). **Widening** accelerates an ascending chain to a safe
post-fixpoint; **narrowing** descends from it toward a more precise state.
`solve` widens only at `LoopForest` headers and bounds narrowing by configured
passes. `value_ranges` is the built-in infinite-height instance. See the
[data-flow framework guide](components/graph/dataflow-framework.md#solver-and-convergence)
and [interval guide](components/graph/interval-analysis.md).

### Master Theorem

A closed-form for divide-and-conquer recurrences of the shape

```math
T(n) = a \, T(n/b) + f(n), \qquad a \ge 1,\ b > 1
```

used by the complexity analyzer to classify recursive functions (e.g. $`a = b = 2`$, $`f(n) = O(n)`$ gives $`O(n \log n)`$). Stated in CLRS [[13]](#references). See [`components/algorithms/complexity.md`](components/algorithms/complexity.md).

### Martin package metrics

An architectural metric suite proposed by Robert C. Martin [[36]](#references).
Given [afferent/efferent coupling](#afferent-coupling--efferent-coupling),
**instability** is $`I=C_e/(C_a+C_e)`$ with an isolated module defined as zero.
**Abstractness** is $`A=N_a/N_t`$, where $`N_a`$ is abstract classes plus
traits and $`N_t`$ is all class/struct/enum/trait declarations. The proposed
**main sequence** is $`A+I=1`$; **distance** is $`D=|A+I-1|`$. These values are
exact for a supplied dependency projection and explicit module assignment, but
their architectural interpretation is advisory. See
[`theory/18-martin-package-metrics.md`](theory/18-martin-package-metrics.md).

### Reflexion model

A comparison between a declared high-level architecture and dependencies
observed in a low-level implementation, introduced by Murphy, Notkin, and
Sullivan [[37]](#references). A **convergence** is observed and permitted, a
**divergence** is observed but forbidden, and an **absence** is permitted but
not observed. `libcpg` maps opaque paths to layers by first textual prefix
match and reports ordered evidence; “reflexion” is unrelated to runtime
reflection. See [`theory/19-reflexion-model-conformance.md`](theory/19-reflexion-model-conformance.md).

### Architectural layer / Layering violation

An **architectural layer** is a bottom-first integer assigned to one SCC
component by reversing its source-oriented condensation longest-path level.
Larger values are higher layers. A healthy cross-component dependency descends
exactly one layer; a **skip-layer** edge descends more than one, and an
**upward** violation is lateral or ascending. A **cyclic-component** violation
identifies an SCC whose internal cyclic edges were contracted. See
[`components/graph/layering-analysis.md`](components/graph/layering-analysis.md).

### Architectural smell

An **architectural smell** is a dependency pattern that warrants human review
because it can accompany architectural erosion; it is evidence, not proof of a
defect. `architecture_smells` reports four Arcan-family categories [[38]](#references):
a **cyclic dependency** is a strongly connected component with at least two
nodes; a **hub-like dependency** is simultaneously a high fan-in and high
fan-out population outlier; an **unstable dependency** is a source module whose
configured share of outgoing dependencies points toward modules with greater
[Martin instability](#martin-package-metrics) [[39]](#references); and a
**god component** is a high-side outlier in assigned projection-node count.
Findings remain advisory and carry stable nodes plus numerical evidence. See
[`theory/21-advisory-architectural-smells.md`](theory/21-advisory-architectural-smells.md).

### API identity / Peer population / Support fraction

An **API identity** is libcpg's canonical identity for one called interface: a
complete sorted represented target set when resolution evidence exists, or a
typed named callee path for an external/unresolved target. A **peer population**
is a finite set of complete same-identity call occurrences selected globally or
by owner-function call-graph distance. Its exact **support fraction** is the
integer pair $`n/p`$, where $`n`$ peers carry a behavior and $`p`$ includes
every comparable occurrence, including the deviation. Thresholds are decided
by integer cross multiplication, never floating-point rounding. See
[`theory/35-population-anomaly-inference.md`](theory/35-population-anomaly-inference.md).

### Missing-check analysis / Check fingerprint

**Missing-check analysis** compares checks controlling a security-sensitive API
site with checks used by its peer population, following the comparative
condition lineage of Chucky and CRIX [[52]](#references), [[53]](#references).
A **check fingerprint** is libcpg's flat postorder semantic tape for one guard.
It abstracts local identifier/declaration spelling while retaining operators,
literals, members, called APIs, nominal types, imports, attributes, macros, and
opaque frontend kinds. A supported fingerprint absent from a site is advisory
evidence, not proof of a defect. See
[`components/graph/population-anomaly-detectors.md`](components/graph/population-anomaly-detectors.md).

### Implicit belief / Deviant behavior / Return-check belief

An **implicit belief** is a high-support represented convention inferred from a
program population, following Engler et al. [[54]](#references). **Deviant
behavior** is a call occurrence that lacks an inferred later reachable API or
an inferred return-value check. A **return-check belief** is supported when a
configured fraction of one API's call results reaches a represented guard over
forward DFG/PDG value evidence. These relations are exact for the modeled graph
and policy but remain advisory about semantic correctness or security impact.
See [`theory/35-population-anomaly-inference.md`](theory/35-population-anomaly-inference.md).

### Inconsistent clone / Path feasibility

An **inconsistent clone** is libcpg's advisory finding for a typed semantic
divergence inside an exact low-edit Type-2 or Type-3 function pair. The
divergence is either an aligned identifier that violates a dominant exact
rename convention or an aligned guard operator/literal payload that differs.
Low edit distance alone is only candidate evidence. This follows the
inconsistent-change motivation studied by Juergens et al. [[55]](#references).

**Path feasibility** asks whether a represented Boolean CFG outcome has any
abstract program state. libcpg combines sparse conditional constant propagation
(SCCP) executable edges with path-partitioned integer intervals. An
**infeasible branch** has no represented executable outcome; a **redundant
condition** has exactly one possible Boolean value; a **contradictory
condition** has no true interval alternative and retains predicate evidence.
See
[`theory/36-clone-consistency-and-abstract-feasibility.md`](theory/36-clone-consistency-and-abstract-feasibility.md).

### Pushdown automaton (PDA) / explicit continuation machine

A **pushdown automaton** (PDA) is a finite-control machine augmented by a stack.
In libcpg's stack-safety engineering, the term also names an equivalent
implementation technique: replace mutually recursive calls with a closed enum
of continuation/frame states stored in a heap `Vec`, then dispatch those states
in a loop. The logical stack remains, but source depth no longer determines
native call depth. Frames must retain every value that a suspended recursive
call would need after its child returns; omitting such state changes semantics.
Property-value and expression-key formatting, validation, persistence, and CPG
extraction use this technique. See [ADR-0040](design/0040-flat-property-value-arena.md)
and [ADR-0041](design/0041-shared-flat-expression-facts.md), grounded in
Reynolds's defunctionalization lineage [[47]](#references).

### Structural expression arena / CEK1

A **structural expression arena** is the shared postorder node and ordered
operand storage behind `ExprKey` and `ExprUniverse`. Nested operands store a
preceding stable node id; exact bottom-up structural classes deduplicate equal
roots without relying on hash collision assumptions. **CEK1** is the canonical
version-1 byte representation of one logical expression tree. In-memory arena
sharing is expanded at this boundary so every accepted byte string has one
strict tree interpretation. See
[ADR-0041](design/0041-shared-flat-expression-facts.md).

---

## References

1. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and Discovering Vulnerabilities with Code Property Graphs.* 2014 IEEE Symposium on Security and Privacy. DOI: [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44)
2. Ferrante, J., Ottenstein, K. J., Warren, J. D. (1987). *The Program Dependence Graph and Its Use in Optimization.* ACM TOPLAS 9(3). DOI: [10.1145/24039.24041](https://doi.org/10.1145/24039.24041)
3. Cytron, R., Ferrante, J., Rosen, B. K., Wegman, M. N., Zadeck, F. K. (1991). *Efficiently Computing Static Single Assignment Form and the Control Dependence Graph.* ACM TOPLAS 13(4). DOI: [10.1145/115372.115320](https://doi.org/10.1145/115372.115320)
4. Cordella, L. P., Foggia, P., Sansone, C., Vento, M. (2004). *A (Sub)graph Isomorphism Algorithm for Matching Large Graphs.* IEEE TPAMI 26(10). DOI: [10.1109/TPAMI.2004.75](https://doi.org/10.1109/TPAMI.2004.75)
5. *(reserved)*
6. Kildall, G. A. (1973). *A Unified Approach to Global Program Optimization.* POPL '73. DOI: [10.1145/512927.512945](https://doi.org/10.1145/512927.512945)
7. McCabe, T. J. (1976). *A Complexity Measure.* IEEE Transactions on Software Engineering SE-2(4). DOI: [10.1109/TSE.1976.233837](https://doi.org/10.1109/TSE.1976.233837)
8. Weiser, M. (1984). *Program Slicing.* IEEE Transactions on Software Engineering SE-10(4). DOI: [10.1109/TSE.1984.5010248](https://doi.org/10.1109/TSE.1984.5010248) (originally ICSE '81).
9. Scarselli, F., Gori, M., Tsoi, A. C., Hagenbuchner, M., Monfardini, G. (2009). *The Graph Neural Network Model.* IEEE Transactions on Neural Networks 20(1). DOI: [10.1109/TNN.2008.2005605](https://doi.org/10.1109/TNN.2008.2005605)
10. Weisfeiler-Lehman: (**10a**) Weisfeiler, B., Leman, A. (1968). *The reduction of a graph to canonical form and the algebra which appears therein.* Nauchno-Technicheskaya Informatsia 2(9) (no DOI). (**10b**) Shervashidze, N., Schweitzer, P., van Leeuwen, E. J., Mehlhorn, K., Borgwardt, K. M. (2011). *Weisfeiler-Lehman Graph Kernels.* JMLR 12. Open access: <https://jmlr.org/papers/v12/shervashidze11a.html>
11. Gamma, E., Helm, R., Johnson, R., Vlissides, J. (1994). *Design Patterns: Elements of Reusable Object-Oriented Software.* Addison-Wesley. ISBN 978-0201633610 (no DOI).
12. Zhou, Y., Liu, S., Siow, J., Du, X., Liu, Y. (2019). *Devign: Effective Vulnerability Identification by Learning Comprehensive Program Semantics via Graph Neural Networks.* NeurIPS 2019. arXiv:[1909.03496](https://arxiv.org/abs/1909.03496) (no DOI).
13. Cormen, T. H., Leiserson, C. E., Rivest, R. L., Stein, C. (2009). *Introduction to Algorithms* (3rd ed.). MIT Press. ISBN 978-0262033848 (no DOI). *(Master Theorem.)*
14. Aho, A. V., Lam, M. S., Sethi, R., Ullman, J. D. (2006). *Compilers: Principles, Techniques, and Tools* (2nd ed.). Addison-Wesley. ISBN 978-0321486813 (no DOI). *(Reaching definitions, liveness, data-flow analysis.)*
15. Chidamber, S. R., Kemerer, C. F. (1994). *A Metrics Suite for Object Oriented Design.* IEEE Transactions on Software Engineering 20(6). DOI: [10.1109/32.295895](https://doi.org/10.1109/32.295895) *(LCOM/CBO, used by `PatternMetrics`.)*
16. Aho, A. V., Sethi, R., Ullman, J. D. (1986). *Compilers: Principles, Techniques, and Tools.* Addison-Wesley. ISBN 0-201-10088-6 (no DOI). *(Natural loops.)*
17. Havlak, P. (1997). *Nesting of Reducible and Irreducible Loops.* ACM TOPLAS 19(4), 557–567. DOI: [10.1145/262004.262005](https://doi.org/10.1145/262004.262005)
18. Ramalingam, G. (1999). *Identifying Loops in Almost Linear Time.* ACM TOPLAS 21(2), 175–188. DOI: [10.1145/316686.316687](https://doi.org/10.1145/316686.316687)
19. Kam, J. B., Ullman, J. D. (1977). *Monotone Data Flow Analysis Frameworks.* Acta Informatica 7, 305–317. DOI: [10.1007/BF00290339](https://doi.org/10.1007/BF00290339)
20. Cousot, P., Cousot, R. (1977). *Abstract Interpretation: A Unified Lattice Model for Static Analysis of Programs by Construction or Approximation of Fixpoints.* POPL '77, 238–252. DOI: [10.1145/512950.512973](https://doi.org/10.1145/512950.512973)
21. Morel, E., Renvoise, C. (1979). *Global Optimization by Suppression of Partial Redundancies.* Communications of the ACM 22(2), 96–103. DOI: [10.1145/359060.359069](https://doi.org/10.1145/359060.359069)
22. Wegman, M. N., Zadeck, F. K. (1991). *Constant Propagation with Conditional Branches.* ACM Transactions on Programming Languages and Systems 13(2), 181–210. DOI: [10.1145/103135.103136](https://doi.org/10.1145/103135.103136)
23. Miné, A. (2006). *The Octagon Abstract Domain.* Higher-Order and Symbolic Computation 19, 31–100. DOI: [10.1007/s10990-006-8609-1](https://doi.org/10.1007/s10990-006-8609-1)
24. Kahn, A. B. (1962). *Topological Sorting of Large Networks.* Communications of the ACM 5(11), 558–562. DOI: [10.1145/368996.369025](https://doi.org/10.1145/368996.369025)
25. Coffman, E. G., Jr., Graham, R. L. (1972). *Optimal Scheduling for Two-Processor Systems.* Acta Informatica 1, 200–213. DOI: [10.1007/BF00288685](https://doi.org/10.1007/BF00288685)
26. MacCormack, A., Rusnak, J., Baldwin, C. Y. (2006). *Exploring the Structure of Complex Software Designs: An Empirical Study of Open Source and Proprietary Code.* Management Science 52(7), 1015–1030. DOI: [10.1287/mnsc.1060.0552](https://doi.org/10.1287/mnsc.1060.0552)
27. Baldwin, C. Y., MacCormack, A., Rusnak, J. (2014). *Hidden Structure: Using Network Methods to Map System Architecture.* Research Policy 43(8), 1381–1397. DOI: [10.1016/j.respol.2014.05.004](https://doi.org/10.1016/j.respol.2014.05.004)
28. Centrality foundations: (**28a**) Brin, S., Page, L. (1998). *The Anatomy of a Large-Scale Hypertextual Web Search Engine.* DOI: [10.1016/S0169-7552(98)00110-X](https://doi.org/10.1016/S0169-7552(98)00110-X). (**28b**) Brandes, U. (2001). *A Faster Algorithm for Betweenness Centrality.* DOI: [10.1080/0022250X.2001.9990249](https://doi.org/10.1080/0022250X.2001.9990249). (**28c**) Bonacich, P. (1987). *Power and Centrality: A Family of Measures.* DOI: [10.1086/228631](https://doi.org/10.1086/228631). (**28d**) Katz, L. (1953). *A New Status Index Derived from Sociometric Analysis.* DOI: [10.1007/BF02289026](https://doi.org/10.1007/BF02289026). (**28e**) Sabidussi, G. (1966). *The Centrality Index of a Graph.* DOI: [10.1007/BF02289527](https://doi.org/10.1007/BF02289527). (**28f**) Kleinberg, J. M. (1999). *Authoritative Sources in a Hyperlinked Environment.* DOI: [10.1145/324133.324140](https://doi.org/10.1145/324133.324140). (**28g**) Burt, R. S. (1992). *Structural Holes: The Social Structure of Competition.* Harvard University Press. ISBN 978-0-674-84371-4.
29. Blondel, V. D., Guillaume, J.-L., Lambiotte, R., Lefebvre, E. (2008). *Fast Unfolding of Communities in Large Networks.* Journal of Statistical Mechanics: Theory and Experiment, P10008. DOI: [10.1088/1742-5468/2008/10/P10008](https://doi.org/10.1088/1742-5468/2008/10/P10008)
30. Traag, V. A., Waltman, L., van Eck, N. J. (2019). *From Louvain to Leiden: Guaranteeing Well-Connected Communities.* Scientific Reports 9, 5233. DOI: [10.1038/s41598-019-41695-z](https://doi.org/10.1038/s41598-019-41695-z)
31. Cohesion and robustness foundations: (**31a**) Seidman, S. B. (1983). *Network Structure and Minimum Degree.* DOI: [10.1016/0378-8733(83)90028-X](https://doi.org/10.1016/0378-8733(83)90028-X). (**31b**) Batagelj, V., Zaveršnik, M. (2003). *An O(m) Algorithm for Cores Decomposition of Networks.* [arXiv:cs/0310049](https://arxiv.org/abs/cs/0310049). (**31c**) Cohen, J. (2008). *Trusses: Cohesive Subgraphs for Social Network Analysis.* NSA Technical Report (no DOI assigned). (**31d**) Hopcroft, J., Tarjan, R. (1973). *Algorithm 447: Efficient Algorithms for Graph Manipulation.* DOI: [10.1145/362248.362272](https://doi.org/10.1145/362248.362272). (**31e**) Tarjan, R. (1972). *Depth-First Search and Linear Graph Algorithms.* DOI: [10.1137/0201010](https://doi.org/10.1137/0201010). (**31f**) Stoer, M., Wagner, F. (1997). *A Simple Min-Cut Algorithm.* DOI: [10.1145/263867.263872](https://doi.org/10.1145/263867.263872). (**31g**) Newman, M. E. J. (2003). *Mixing Patterns in Networks.* DOI: [10.1103/PhysRevE.67.026126](https://doi.org/10.1103/PhysRevE.67.026126). (**31h**) Holme, P., Kim, B. J., Yoon, C. N., Han, S. K. (2002). *Attack Vulnerability of Complex Networks.* DOI: [10.1103/PhysRevE.65.056109](https://doi.org/10.1103/PhysRevE.65.056109).
32. Spectral foundations: (**32a**) Fiedler, M. (1973). *Algebraic Connectivity of Graphs.* DOI: [10.21136/CMJ.1973.101168](https://doi.org/10.21136/CMJ.1973.101168). (**32b**) Shi, J., Malik, J. (2000). *Normalized Cuts and Image Segmentation.* DOI: [10.1109/34.868688](https://doi.org/10.1109/34.868688).
33. Johnson, D. B. (1975). *Finding All the Elementary Circuits of a Directed Graph.* SIAM Journal on Computing 4(1), 77–84. DOI: [10.1137/0204007](https://doi.org/10.1137/0204007).
34. Directed-motif foundations: (**34a**) Holland, P. W., Leinhardt, S. (1970). *A Method for Detecting Structure in Sociometric Data.* American Journal of Sociology 76(3), 411–432. DOI: [10.1086/224954](https://doi.org/10.1086/224954). (**34b**) Batagelj, V., Mrvar, A. (2001). *A subquadratic triad census algorithm for large sparse networks with small maximum degree.* DOI: [10.1016/S0378-8733(01)00035-1](https://doi.org/10.1016/S0378-8733(01)00035-1). (**34c**) Milo, R. et al. (2002). *Network Motifs: Simple Building Blocks of Complex Networks.* DOI: [10.1126/science.298.5594.824](https://doi.org/10.1126/science.298.5594.824).
35. Graph-edit assignment foundations: (**35a**) Riesen, K., Bunke, H. (2009). *Approximate Graph Edit Distance Computation by Means of Bipartite Graph Matching.* DOI: [10.1016/j.imavis.2008.04.004](https://doi.org/10.1016/j.imavis.2008.04.004). (**35b**) Kuhn, H. W. (1955). *The Hungarian Method for the Assignment Problem.* DOI: [10.1002/nav.3800020109](https://doi.org/10.1002/nav.3800020109).
36. Martin package-metric foundations: (**36a**) Martin, R. C. (1994). *OO Design Quality Metrics: An Analysis of Dependencies.* Object Mentor technical report. [Archived report PDF](https://condor.depaul.edu/dmumaugh/OOT/Design-Principles/oodmetrc.pdf). No DOI assigned. (**36b**) Martin, R. C. (2003). *Agile Software Development: Principles, Patterns, and Practices.* Prentice Hall/Pearson. ISBN 978-0-13-597444-5. [Publisher record](https://www.informit.com/store/agile-software-development-principles-patterns-and-9780135974445).
37. Murphy, G. C., Notkin, D., Sullivan, K. J. (2001). *Software Reflexion Models: Bridging the Gap between Design and Implementation.* IEEE Transactions on Software Engineering 27(4), 364–380. DOI: [10.1109/32.917525](https://doi.org/10.1109/32.917525).
38. Fontana, F. A., Pigazzini, I., Roveda, R., Tamburri, D. A., Zanoni, M., Di Nitto, E. (2017). *Arcan: A Tool for Architectural Smells Detection.* 2017 IEEE International Conference on Software Architecture Workshops, 282–285. DOI: [10.1109/ICSAW.2017.16](https://doi.org/10.1109/ICSAW.2017.16).
39. Fontana, F. A., Pigazzini, I., Roveda, R., Zanoni, M. (2016). *Automatic Detection of Instability Architectural Smells.* 2016 IEEE International Conference on Software Maintenance and Evolution, 433–437. DOI: [10.1109/ICSME.2016.33](https://doi.org/10.1109/ICSME.2016.33).
40. Code-metric foundations: (**40a**) Chidamber, S. R., Kemerer, C. F. (1991). *Towards a Metrics Suite for Object Oriented Design.* DOI: [10.1145/117954.117970](https://doi.org/10.1145/117954.117970). (**40b**) Li, W., Henry, S. (1993). *Object-oriented metrics that predict maintainability.* DOI: [10.1016/0164-1212(93)90077-B](https://doi.org/10.1016/0164-1212(93)90077-B). (**40c**) Hitz, M., Montazeri, B. (1995). *Measuring Coupling and Cohesion in Object-Oriented Systems.* No DOI assigned. (**40d**) Henderson-Sellers, B. (1996). *Object-Oriented Metrics: Measures of Complexity.* ISBN 978-0-13-239872-5 (no DOI). (**40e**) Halstead, M. H. (1977). *Elements of Software Science.* ISBN 0-444-00205-7 (no DOI). (**40f**) Campbell, G. A. (2018). *Cognitive Complexity: An Overview and Evaluation.* DOI: [10.1145/3194164.3194186](https://doi.org/10.1145/3194164.3194186). (**40g**) Oman, P., Hagemeister, J. (1992). *Metrics for assessing a software system's maintainability.* DOI: [10.1109/ICSM.1992.242525](https://doi.org/10.1109/ICSM.1992.242525). (**40h**) Coleman, D., Ash, D., Lowther, B., Oman, P. (1994). *Using metrics to evaluate software system maintainability.* DOI: [10.1109/2.303623](https://doi.org/10.1109/2.303623).
41. Interprocedural distributive-data-flow foundations: (**41a**) Reps, T., Horwitz, S., Sagiv, M. (1995). *Precise Interprocedural Dataflow Analysis via Graph Reachability.* DOI: [10.1145/199448.199462](https://doi.org/10.1145/199448.199462). (**41b**) Sagiv, M., Reps, T., Horwitz, S. (1996). *Precise Interprocedural Dataflow Analysis with Applications to Constant Propagation.* DOI: [10.1016/0304-3975(96)00072-2](https://doi.org/10.1016/0304-3975(96)00072-2).
42. Horwitz, S., Reps, T., Binkley, D. (1990). *Interprocedural Slicing Using Dependence Graphs.* ACM Transactions on Programming Languages and Systems 12(1), 26–60. DOI: [10.1145/77606.77608](https://doi.org/10.1145/77606.77608).
43. Meredith, L. G., Radestock, M. (2005). *A Reflective Higher-order Calculus.* Electronic Notes in Theoretical Computer Science 141(5), 49–67. DOI: [10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016).
44. Kobayashi, N. (2006). *A New Type System for Deadlock-Free Processes.* CONCUR 2006, LNCS 4137, 233–247. DOI: [10.1007/11817949_16](https://doi.org/10.1007/11817949_16).
45. Naumovich, G., Avrunin, G. S. (1998). *A Conservative Data Flow Algorithm for Detecting All Pairs of Statements That May Happen in Parallel.* Proceedings of the 6th ACM SIGSOFT International Symposium on Foundations of Software Engineering, 24–34. DOI: [10.1145/288195.288213](https://doi.org/10.1145/288195.288213).
46. Honda, K., Vasconcelos, V. T., Kubo, M. (1998). *Language Primitives and Type Discipline for Structured Communication-Based Programming.* ESOP 1998. DOI: [10.1007/BFb0053567](https://doi.org/10.1007/BFb0053567).
47. Reynolds, J. C. (1972). *Definitional Interpreters for Higher-Order Programming Languages.* DOI: [10.1145/800194.805852](https://doi.org/10.1145/800194.805852).
48. Strom, R. E., Yemini, S. (1986). *Typestate: A Programming Language Concept for Enhancing Software Reliability.* DOI: [10.1109/TSE.1986.6312929](https://doi.org/10.1109/TSE.1986.6312929).
49. Fink, S. J., Yahav, E., Dor, N., Ramalingam, G., Geay, E. (2008). *Effective Typestate Verification in the Presence of Aliasing.* DOI: [10.1145/1348250.1348255](https://doi.org/10.1145/1348250.1348255).
50. Shadab, N., Gharat, P., Ernst, M. D., Kellogg, M., Lahiri, S. K., Lal, A., Sridharan, M. (2025). *Lightweight and Modular Resource Leak Checking (Extended Version).* DOI: [10.1007/s10009-025-00804-2](https://doi.org/10.1007/s10009-025-00804-2).
51. Sinha, S., Harrold, M. J. (2000). *Analysis and Testing of Programs with Exception Handling Constructs.* DOI: [10.1109/32.877846](https://doi.org/10.1109/32.877846).
52. Yamaguchi, F., Wressnegger, C., Gascon, H., Rieck, K. (2013). *Chucky: Exposing Missing Checks in Source Code for Vulnerability Discovery.* DOI: [10.1145/2508859.2516665](https://doi.org/10.1145/2508859.2516665).
53. Lu, K., Pakki, A., Wu, Q. (2019). *Detecting Missing-Check Bugs via Semantic- and Context-Aware Criticalness and Constraints Inferences.* [USENIX Security 2019](https://www.usenix.org/conference/usenixsecurity19/presentation/lu) (no DOI assigned in the proceedings record).
54. Engler, D., Chen, D. Y., Hallem, S., Chou, A., Chelf, B. (2001). *Bugs as Deviant Behavior: A General Approach to Inferring Errors in Systems Code.* DOI: [10.1145/502034.502041](https://doi.org/10.1145/502034.502041).
55. Juergens, E., Deissenboeck, F., Hummel, B., Wagner, S. (2009). *Do Code Clones Matter?* DOI: [10.1109/ICSE.2009.5070547](https://doi.org/10.1109/ICSE.2009.5070547).
56. Nielebock, S., Blockhaus, P., Krüger, J., Ortmeier, F. (2024). *ASAP-Repair: API-Specific Automated Program Repair Based on API Usage Graphs.* DOI: [10.1145/3643788.3648011](https://doi.org/10.1145/3643788.3648011).
57. Song, Y., Gao, X., Li, W., Chin, W.-N., Roychoudhury, A. (2024). *ProveNFix: Temporal Property-Guided Program Repair.* DOI: [10.1145/3643737](https://doi.org/10.1145/3643737).
58. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and Discovering Vulnerabilities with Code Property Graphs.* DOI: [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44).
59. Li, P., Yao, S., Korich, J. S., Luo, C., Yu, J., Cao, Y., Yang, J. (2025). *Neuro-symbolic Static Analysis with LLM-generated Vulnerability Patterns.* DOI: [10.48550/arXiv.2504.16057](https://doi.org/10.48550/arXiv.2504.16057).
60. Lekssays, A., Mouhcine, H., Tran, K., Yu, T., Khalil, I. (2025). *LLMxCPG: Context-Aware Vulnerability Detection Through Code Property Graph-Guided Large Language Models.* DOI: [10.48550/arXiv.2507.16585](https://doi.org/10.48550/arXiv.2507.16585).
61. Meli, M., McNiece, M. R., Reaves, B. (2019). *How Bad Can It Git? Characterizing Secret Leakage in Public GitHub Repositories.* DOI: [10.14722/ndss.2019.23418](https://doi.org/10.14722/ndss.2019.23418).
62. Shannon, C. E. (1948). *A Mathematical Theory of Communication.* Part I DOI: [10.1002/j.1538-7305.1948.tb01338.x](https://doi.org/10.1002/j.1538-7305.1948.tb01338.x); Part II DOI: [10.1002/j.1538-7305.1948.tb00917.x](https://doi.org/10.1002/j.1538-7305.1948.tb00917.x).
63. Yamaguchi, F., Lindner, F., Rieck, K. (2011). *Vulnerability Extrapolation: Assisted Discovery of Vulnerabilities Using Machine Learning.* [USENIX WOOT '11](https://www.usenix.org/conference/woot11/vulnerability-extrapolation-assisted-discovery-vulnerabilities-using-machine) (no DOI advertised in the proceedings record).
64. Yamaguchi, F., Lottmann, M., Rieck, K. (2012). *Generalized Vulnerability Extrapolation using Abstract Syntax Trees.* DOI: [10.1145/2420950.2421003](https://doi.org/10.1145/2420950.2421003).
65. Blackshear, S., Gorogiannis, N., O'Hearn, P. W., Sergey, I. (2018). *RacerD: Compositional Static Race Detection.* DOI: [10.1145/3276514](https://doi.org/10.1145/3276514).
66. Engler, D., Ashcraft, K. (2003). *RacerX: Effective, Static Detection of Race Conditions and Deadlocks.* DOI: [10.1145/945445.945468](https://doi.org/10.1145/945445.945468).
67. Alon, U., Zilberstein, M., Levy, O., Yahav, E. (2019). *code2vec: Learning Distributed Representations of Code.* DOI: [10.1145/3290353](https://doi.org/10.1145/3290353).
68. Hin, D., Kan, A., Chen, H., Babar, M. A. (2022). *LineVD: Statement-level Vulnerability Detection using Graph Neural Networks.* DOI: [10.1145/3524842.3527949](https://doi.org/10.1145/3524842.3527949).
69. Li, Y., Wang, S., Nguyen, T. N. (2021). *Vulnerability Detection with Fine-grained Interpretations.* DOI: [10.1145/3468264.3468597](https://doi.org/10.1145/3468264.3468597).
