# Glossary

This glossary is the **canonical definition** of every term, acronym, and symbol used across the `libcpg` documentation. Each term is defined once here; other pages link back to it rather than redefining. Entries are alphabetical.

## Notation conventions

Mathematical expressions use GitHub math spans: inline as ``$`…`$`` and display as fenced ` ```math ` blocks. A CPG is written as the graph $`G = (V, E)`$ with vertex set $`V`$ (nodes) and edge set $`E`$ (typed edges). $`N`$ and $`E`$ denote node and edge **counts** when a scalar is needed (e.g. in complexity formulae). Citations resolve to DOIs where they exist; see [References](#references).

---

### Abstract Syntax Tree (AST)

The tree that a parser produces from source code: interior nodes are language constructs (functions, `if`, expressions) and children are their syntactic parts. In `libcpg` the AST is the **base layer** of the [Code Property Graph](#code-property-graph-cpg) — every other layer overlays edges on the *same* AST nodes. AST edges are `AstChild` / `AstParent` / `AstNextSibling` / `AstPrevSibling`. See [`architecture/graph-data-model.md`](architecture/graph-data-model.md).

### Aggregation (GNN)

The step in a [Graph Neural Network](#graph-neural-network-gnn) that combines a node's neighbour vectors into one vector. `libcpg`'s `AggregationMethod` enumerates `Mean` (the default and the only one used in message passing), `Sum`, `Max`, `Attention`, and `Hierarchical`; the latter two are **reserved placeholders**, not yet wired. See [Message passing](#message-passing).

### Algorithm family

A structural category of algorithm (sorting, searching, graph traversal, dynamic programming, …) that the `algorithm-detection` feature recognises from a function's control-flow and recursion shape via **heuristics** (e.g. "has a midpoint calculation", "has a visited set", "has a memoization table"). Exposed as `AlgorithmFamily`. Detection is heuristic, not a proof of identity. See [`components/algorithms/families.md`](components/algorithms/families.md).

### AST-ordered reaching definitions

`libcpg`'s [data-flow](#data-flow-graph-dfg) analysis strategy: a single **flow-sensitive** sweep over AST nodes *in source order*, maintaining an environment `ReachingEnv` mapping each variable name to the set of definitions currently reaching it. It applies a [strong update](#strong-update--weak-update) in straight-line context and a weak update inside conditional regions, and sweeps loop bodies twice for loop-carried dependencies. It is **not** [SSA](#static-single-assignment-ssa) and **not** classic CFG-fixed-point propagation; it is chosen for simplicity and for threading definitions into deeply nested expression uses. See [`theory/03-data-flow-and-reaching-definitions.md`](theory/03-data-flow-and-reaching-definitions.md) and [`design/0003-ast-ordered-reaching-defs.md`](design/0003-ast-ordered-reaching-defs.md).

### Backward slice / Forward slice

A [program slice](#program-slicing). The **backward** slice of a node $`s`$ is the set of nodes that can affect $`s`$ (its transitive [PDG](#program-dependence-graph-pdg) predecessors); the **forward** slice is the set of nodes that $`s`$ can affect (its transitive successors). `libcpg` computes them as bounded breadth-first traversals — `backward_slice(&cpg, s, max_nodes)` and `forward_slice(&cpg, s, max_nodes)` — after [`ControlDependence`](#control-dependence) and [`DataDependence`](#data-dependence) edges have been added. The `max_nodes` argument caps the result. Introduced by Weiser [[8]](#references). See [`usage/04-program-slicing.md`](usage/04-program-slicing.md).

### Basic block

A maximal straight-line run of statements with a single entry and single exit — no branches in except at the top, none out except at the bottom. `BasicBlockIdentifier` groups CFG nodes into basic blocks. Basic blocks are the classic unit of control-flow analysis; `libcpg` operates at AST-node granularity but can identify block leaders. See [`components/builder/cfg.md`](components/builder/cfg.md).

### Call graph

The overlay whose edges connect call sites to the functions they invoke: `StaticCall` (resolved statically), `DynamicCall` (virtual/indirect), and `CallSite`. Queried through `call_sites`, `callees`, and `callers`. See [`components/graph/edges.md`](components/graph/edges.md).

### Code Property Graph (CPG)

A single directed graph that merges several program views onto **one shared node set**: the [AST](#abstract-syntax-tree-ast), the [CFG](#control-flow-graph-cfg), the [DFG](#data-flow-graph-dfg), and — on demand — the [PDG](#program-dependence-graph-pdg). Introduced by Yamaguchi et al. [[1]](#references) to express vulnerability queries that need syntax, control, and data flow simultaneously. In `libcpg` the CPG is the type `CodePropertyGraph`, backed by a [petgraph](#petgraph) `DiGraph<CpgNode, CpgEdge>`. See [`theory/01-code-property-graphs.md`](theory/01-code-property-graphs.md).

### Complexity class / Big-O

An asymptotic growth category for a function's running time. `libcpg`'s `ComplexityClass` ladder is, from cheapest to most expensive, `Constant` $`O(1)`$, `Logarithmic` $`O(\log n)`$, `Linear` $`O(n)`$, `Linearithmic` $`O(n \log n)`$, `Quadratic` $`O(n^2)`$, `Cubic` $`O(n^3)`$, `Polynomial(k)` $`O(n^k)`$, `Exponential` $`O(2^n)`$, `Factorial` $`O(n!)`$, and `Unknown`. See [`components/algorithms/complexity.md`](components/algorithms/complexity.md).

### Confidence (pattern match)

A score in $`[0, 1]`$ that a detected [design pattern](#design-pattern) attaches to each `PatternMatch`, measuring how completely the candidate subgraph fills the pattern's template. `GofPatternDetector` keeps only matches at or above `min_confidence` (default `0.7`). See [Gang of Four](#gang-of-four-gof).

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

### Cyclomatic complexity

McCabe's structural complexity metric [[7]](#references): the number of linearly independent paths through a function's CFG. `libcpg` computes `cyclomatic_complexity()` as

```math
M = E - N + 2
```

where $`E`$ is the number of CFG edges and $`N`$ the number of CFG nodes (for a single connected component with one entry and one exit). See [`theory/02-control-flow-and-complexity.md`](theory/02-control-flow-and-complexity.md).

### Data dependence

A [PDG](#program-dependence-graph-pdg) edge kind (`DataDependence`): node $`u`$ (a use) is data-dependent on node $`d`$ (a definition) when $`d`$ defines a value that $`u`$ reads and the definition can reach the use. `libcpg` derives these by re-projecting DFG `DefUse`/`ReachingDef` edges within a function. See [`theory/04-program-dependence-and-slicing.md`](theory/04-program-dependence-and-slicing.md).

### Data Flow Graph (DFG)

The overlay whose edges track how values move from definitions to uses, typed by `DfgEdgeKind` (13 variants: `DefUse`, `UseDef`, `ReachingDef`, `DataDependency`, `Parameter`, `ReturnValue`, `FieldRead`, `FieldWrite`, `IndexRead`, `IndexWrite`, `Alias`, `Dereference`, `AddressOf`). Wrapped as `CpgEdgeKind::DataFlow(DfgEdgeKind)`. Built by `DfgExtractor` using [AST-ordered reaching definitions](#ast-ordered-reaching-definitions). See [`components/builder/dfg.md`](components/builder/dfg.md).

### Def-use chain / Definition / Use

A **definition** is a program point that assigns a variable a value; a **use** is a point that reads it. A **def-use chain** links a definition to every use it reaches. `libcpg` models these with `Definition` / `DefinitionKind`, `Use` / `UseKind`, and `DefUseChain`, built by `build_def_use_chains`. See [`components/builder/dfg.md`](components/builder/dfg.md).

### Design pattern

A reusable solution to a recurring design problem. `libcpg` detects the 23 [Gang-of-Four](#gang-of-four-gof) patterns structurally, by matching each pattern's template graph against the CPG with a relaxed [VF2](#vf2) matcher. See [`components/patterns/gang-of-four.md`](components/patterns/gang-of-four.md).

### Dominator / Post-dominator

Node $`d`$ **dominates** node $`n`$ if every path from entry to $`n`$ passes through $`d`$; $`p`$ **post-dominates** $`n`$ if every path from $`n`$ to exit passes through $`p`$. Post-dominators are dominators computed on the *reversed* CFG. `libcpg` uses `petgraph`'s `dominators::simple_fast` for this. Prerequisite for [control dependence](#control-dependence). See [`theory/04-program-dependence-and-slicing.md`](theory/04-program-dependence-and-slicing.md).

### Dominance frontier / Reverse dominance frontier

The **dominance frontier** of node $`d`$ is the set of nodes where $`d`$'s dominance "stops" — the join points just beyond $`d`$'s dominated region (Cytron et al. [[3]](#references)). Computing it on the *reversed* CFG (the **reverse dominance frontier**) yields exactly the [control-dependence](#control-dependence) relation. See [`theory/04-program-dependence-and-slicing.md`](theory/04-program-dependence-and-slicing.md).

### DPML (Design-Pattern Markup Language)

`libcpg`'s small YAML/TOML schema for declaring a pattern as roles (`DpmlRole`) and constraints (`DpmlConstraint`), loaded into a `DpmlTemplate` and compiled to a [`PatternTemplate`](#pattern-template). Malformed input yields `DpmlError`. Lets users add pattern templates without writing Rust. See [`components/patterns/dpml.md`](components/patterns/dpml.md).

### Embedding

A dense real-valued vector that summarises a node (`NodeEmbedding`) or a subgraph (`SubgraphEmbedding`) so that structurally/semantically similar code lands nearby in vector space. Produced by the [GNN](#graph-neural-network-gnn) and compared with [cosine similarity](#cosine-similarity). See [`components/gnn/embeddings.md`](components/gnn/embeddings.md).

### Feature flag (cargo)

A compile-time switch declared in `Cargo.toml` that gates optional code and dependencies. `libcpg`'s default set is **empty** (`default = []`): language grammars, pattern/algorithm detection, serde, and the GNN are each opt-in. Key flags: `lang-*` (16 grammars), `design-patterns`, `algorithm-detection`, `serde`, `gnn`, `ml-linfa`/`ml-rules`, the Mode-B toggles `rholang`/`metta`, and the umbrella `full`. See [`engineering/01-build-and-features.md`](engineering/01-build-and-features.md).

### Feature vector (classification)

The fixed-length numeric summary of a candidate subgraph (12 fields) that `PatternClassifier` scores to label a [design pattern](#design-pattern) — an alternative to template matching. `ClassificationMode` selects rule-based, ML (`ml-linfa`), or hybrid scoring. Not to be confused with a graph feature vector used by the `Cosine` [similarity metric](#similarity-metric). See [`components/patterns/classification.md`](components/patterns/classification.md).

### Gang of Four (GoF)

The four authors of *Design Patterns* [[11]](#references) and, by metonymy, the 23 patterns catalogued there, grouped into `GofCategory::Creational` (5), `Structural` (7), and `Behavioral` (11). `libcpg` names them with the `GofPattern` enum — note the variant is `FactoryMethod` (never `Factory`). See [`components/patterns/gang-of-four.md`](components/patterns/gang-of-four.md).

### Graph edit distance

The minimum number of node/edge insertions, deletions, and relabellings that turn one graph into another — a similarity measure `libcpg` approximates in the `GraphEdit` [similarity metric](#similarity-metric). Exact graph edit distance is NP-hard, so an approximate blend is used. See [`theory/06-graph-similarity.md`](theory/06-graph-similarity.md).

### Graph Neural Network (GNN)

A neural network that computes node representations by repeatedly [aggregating](#aggregation-gnn) information from neighbours ([message passing](#message-passing)), pioneered for graphs by Scarselli et al. [[9]](#references) and applied to vulnerability detection by Devign (Zhou et al. [[12]](#references)). `libcpg`'s `CpgGnn` (feature `gnn`) owns a CPG and produces [embeddings](#embedding). See [`components/gnn/overview.md`](components/gnn/overview.md).

### Idempotent

An operation that has the same effect whether applied once or many times. `CfgExtractor::extract`, `DfgExtractor::extract`, and `PdgBuilder::build` are idempotent: re-running them does not duplicate edges. This lets construction stages be re-applied safely.

### Isomorphism / Subgraph isomorphism

A **graph isomorphism** is a bijection between two graphs' nodes that preserves edges. **Subgraph isomorphism** asks whether a (small) *pattern* graph is isomorphic to some subgraph of a (large) *target* graph — the core question in pattern detection. It is NP-complete in general; `libcpg` solves it with [VF2](#vf2). See [`theory/05-subgraph-isomorphism-vf2.md`](theory/05-subgraph-isomorphism-vf2.md).

### Jaccard similarity

The size of the intersection over the size of the union of two sets:

```math
J(A, B) = \frac{|A \cap B|}{|A \cup B|}
```

`libcpg`'s default [similarity metric](#similarity-metric) applies it to the multisets of node kinds of two graphs. See [`theory/06-graph-similarity.md`](theory/06-graph-similarity.md).

### Kill / Gen (data-flow)

In [reaching-definitions](#reaching-definition) analysis, processing a definition of variable $`x`$ **generates** (`gen`) the new definition and **kills** (`kill`) prior definitions of $`x`$. A [strong update](#strong-update--weak-update) does both; a weak update only generates. See [`theory/03-data-flow-and-reaching-definitions.md`](theory/03-data-flow-and-reaching-definitions.md).

### Lattice (data-flow)

The algebraic structure — a partially ordered set with meet/join — over which classical data-flow analyses iterate to a fixed point, formalised by Kildall [[6]](#references). `libcpg`'s reaching-definitions environment is a map into the powerset lattice of definitions; the theory page frames the AST-ordered sweep against this backdrop. See [`theory/03-data-flow-and-reaching-definitions.md`](theory/03-data-flow-and-reaching-definitions.md).

### Message passing

One round of GNN computation: every node updates its vector from its neighbours' vectors. `libcpg` uses mean aggregation with a ReLU nonlinearity over AST, CFG, and DFG neighbourhoods:

```math
h_v^{(k)} = \mathrm{ReLU}\!\left( \mathrm{mean}\left( \{ h_u^{(k-1)} : u \in \mathcal{N}(v) \} \cup \{ h_v^{(k-1)} \} \right) \right)
```

where $`\mathcal{N}(v)`$ is $`v`$'s neighbourhood across the three overlays and $`k`$ indexes the layer (up to `num_layers`). See [`components/gnn/message-passing.md`](components/gnn/message-passing.md).

### MeTTa

A language for the [F1R3FLY.io](#rholang) / Hyperon ecosystem based on rewriting over symbolic [S-expressions](#s-expression). `libcpg` maps MeTTa to CPG nodes through [Mode B](#mode-b--build_from_tree) (`map_metta`): e.g. `(= (double $x) (* $x 2))` becomes a `Function` "double" with `$x` as a `Parameter` flowing to its use. See [`usage/06-f1r3fly-rholang-metta.md`](usage/06-f1r3fly-rholang-metta.md).

### Mode B / `build_from_tree`

The construction path where the **caller supplies an already-parsed** `tree_sitter::Tree` and `libcpg` builds the CPG from it: `TreeSitterCpgBuilder::build_from_tree(&tree, source, language)`. It needs no `lang-*` feature (the caller owns the grammar), skips the `max_file_size` check, and is the only path for [Rholang](#rholang) and [MeTTa](#metta). "Mode A" is the internal-parse path (`build`). See [`design/0002-mode-b-build-from-tree.md`](design/0002-mode-b-build-from-tree.md).

### Node kind / Edge kind

The type tag on a CPG node (`CpgNodeKind`, 45 variants — a mix of unit variants such as `Root`, `If`, `Return` and data-carrying variants such as `Function { signature }`, `Call { target, is_method }`) or edge (`CpgEdgeKind`). Kinds drive every query and every pattern/complexity heuristic. See [`components/graph/nodes.md`](components/graph/nodes.md) and [`components/graph/edges.md`](components/graph/edges.md).

### Pattern template

A declarative description of a pattern as node constraints (`NodeConstraint`) and edge constraints (`EdgeConstraint`) that `.to_pattern_graph()` compiles into a target graph for [VF2](#vf2) matching. `PatternTemplate` lives in the `pattern::` module; the [DPML](#dpml-design-pattern-markup-language) loader compiles YAML/TOML into one. See [`components/patterns/vf2-matching.md`](components/patterns/vf2-matching.md).

### petgraph

The Rust graph library `libcpg` builds on. `CodePropertyGraph` wraps a `petgraph::graph::DiGraph<CpgNode, CpgEdge>`, and PDG construction reuses `petgraph`'s dominator algorithm. Chosen for its mature graph algorithms and stable `NodeIndex`/`EdgeIndex` model. See [`design/0001-unified-overlay-graph.md`](design/0001-unified-overlay-graph.md).

### Program Dependence Graph (PDG)

The overlay of [control-dependence](#control-dependence) and [data-dependence](#data-dependence) edges, introduced by Ferrante–Ottenstein–Warren [[2]](#references). It is the substrate for [program slicing](#program-slicing). Added on demand by `PdgBuilder::build(&mut cpg, function)` (it is *not* built during initial construction). See [`theory/04-program-dependence-and-slicing.md`](theory/04-program-dependence-and-slicing.md).

### Program slicing

Reducing a program to just the statements that affect (or are affected by) a chosen point of interest, the **slicing criterion**. Introduced by Weiser [[8]](#references). `libcpg` computes [backward and forward slices](#backward-slice--forward-slice) over the PDG. See [`usage/04-program-slicing.md`](usage/04-program-slicing.md).

### Reaching definition

A definition of a variable that reaches a program point with no intervening redefinition on some path. The `ReachingDef` DFG edge connects such a definition to the use it reaches. Computed by [AST-ordered reaching definitions](#ast-ordered-reaching-definitions). See [`theory/03-data-flow-and-reaching-definitions.md`](theory/03-data-flow-and-reaching-definitions.md).

### ReLU

The **Rectified Linear Unit** activation $`\mathrm{ReLU}(x) = \max(0, x)`$, applied element-wise after each GNN [aggregation](#aggregation-gnn) to introduce nonlinearity. See [Message passing](#message-passing).

### Rholang

The concurrent process-calculus language of the [F1R3FLY.io](https://f1r3fly.io) / RChain ecosystem, based on the reflective higher-order **ρ-calculus** (a **process calculus**: a formalism for concurrent, communicating processes). `libcpg` maps Rholang onto CPG vocabulary through [Mode B](#mode-b--build_from_tree) (`map_rholang`): `contract` → `Function`, `x!(…)` send → `Call`, `new`-bound channel → `Variable`, a `rho:` URI → `Import`. See [`usage/06-f1r3fly-rholang-metta.md`](usage/06-f1r3fly-rholang-metta.md).

### S-expression

A symbolic expression: either an atom or a parenthesised list of S-expressions — the uniform syntax of Lisp-family and [MeTTa](#metta) code. `libcpg`'s MeTTa mapper dispatches on each list's head atom. See [`usage/06-f1r3fly-rholang-metta.md`](usage/06-f1r3fly-rholang-metta.md).

### Similarity metric

The strategy `GraphSimilarity` uses to score two graphs' likeness: `SimilarityMetric::Jaccard` (default), `Cosine`, `WeisfeilerLehman`, or `GraphEdit`. Scores blend a structural component (weight `0.7`) and a label component (weight `0.3`). See [`theory/06-graph-similarity.md`](theory/06-graph-similarity.md).

### Static Single Assignment (SSA)

An IR form in which every variable is assigned exactly once, with $`\phi`$-functions merging values at control-flow joins (Cytron et al. [[3]](#references)). `libcpg` deliberately does **not** use SSA for its DFG (it uses [AST-ordered reaching definitions](#ast-ordered-reaching-definitions)); SSA is defined here to make that contrast precise. See [`design/0003-ast-ordered-reaching-defs.md`](design/0003-ast-ordered-reaching-defs.md).

### Strong update / Weak update

When a reaching-definitions sweep processes a definition of $`x`$ in **straight-line** context it performs a **strong update** — it [kills](#kill--gen-data-flow) prior definitions of $`x`$ and gens the new one. Inside a **conditional region** (where the definition may or may not execute) it performs a **weak update** — it adds the new definition without killing the old, since both may reach later uses. See [`theory/03-data-flow-and-reaching-definitions.md`](theory/03-data-flow-and-reaching-definitions.md).

### Subgraph

A graph formed from a subset of another graph's nodes and the edges among them. `libcpg` extracts subgraphs (`subgraph`, `function_cfg`, `function_dfg`) and matches pattern subgraphs against the CPG. See [Subgraph isomorphism](#isomorphism--subgraph-isomorphism).

### Taint analysis

A data-flow query that tracks whether values from *untrusted sources* can reach *sensitive sinks* without sanitisation — a classic CPG application (Yamaguchi et al. [[1]](#references)). Expressible in `libcpg` by traversing DFG/PDG edges from source nodes to sink nodes. See [`usage/02-querying-and-traversal.md`](usage/02-querying-and-traversal.md).

### Terminal set (VF2)

In [VF2](#vf2), the sets of candidate nodes adjacent to the current partial mapping (the "fringe"), used to generate the next candidate pair and to test feasibility. `libcpg`'s matcher restores terminal sets exactly on backtracking (the "pop-order fix"). See [`theory/05-subgraph-isomorphism-vf2.md`](theory/05-subgraph-isomorphism-vf2.md).

### Tree-sitter

The incremental parser generator `libcpg` uses to turn source text into a concrete syntax tree. `ParserRegistry` holds the feature-gated grammars for the 16 built-in languages; [Mode B](#mode-b--build_from_tree) accepts a tree the caller parsed with its own grammar. See [`architecture/language-frontends.md`](architecture/language-frontends.md).

### VF2

The subgraph-isomorphism algorithm of Cordella, Foggia, Sansone, and Vento [[4]](#references): a depth-first state-space search that grows a partial node mapping, pruning with feasibility rules over node kinds and incident edges, and backtracking when stuck. `libcpg`'s `Vf2Matcher` implements it with an explicit push-order stack so backtracking restores the mapping and [terminal sets](#terminal-set-vf2) exactly. Worst-case cost is $`O(N!\,N)`$ but pruning makes it practical. See [`theory/05-subgraph-isomorphism-vf2.md`](theory/05-subgraph-isomorphism-vf2.md).

### Weisfeiler-Lehman kernel / label refinement

A graph-similarity method (Weisfeiler & Leman [[10a]](#references); graph kernels by Shervashidze et al. [[10b]](#references)) that iteratively refines each node's label to a hash of its own label plus the sorted multiset of neighbour labels; the histogram of labels after $`k`$ iterations becomes a feature vector for comparison. `libcpg` uses 3 iterations in the `WeisfeilerLehman` [similarity metric](#similarity-metric). See [`theory/06-graph-similarity.md`](theory/06-graph-similarity.md).

### Master Theorem

A closed-form for divide-and-conquer recurrences of the shape

```math
T(n) = a \, T(n/b) + f(n), \qquad a \ge 1,\ b > 1
```

used by the complexity analyzer to classify recursive functions (e.g. $`a = b = 2`$, $`f(n) = O(n)`$ gives $`O(n \log n)`$). Stated in CLRS [[13]](#references). See [`components/algorithms/complexity.md`](components/algorithms/complexity.md).

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
14. Aho, A. V., Lam, M. S., Sethi, R., Ullman, J. D. (2006). *Compilers: Principles, Techniques, and Tools* (2nd ed.). Addison-Wesley. ISBN 978-0321486813 (no DOI). *(Reaching definitions, data-flow analysis.)*
15. Chidamber, S. R., Kemerer, C. F. (1994). *A Metrics Suite for Object Oriented Design.* IEEE Transactions on Software Engineering 20(6). DOI: [10.1109/32.295895](https://doi.org/10.1109/32.295895) *(LCOM/CBO, used by `PatternMetrics`.)*
