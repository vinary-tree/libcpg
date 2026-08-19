# Theory 00 — What a Code Property Graph Is, and Why Unify the Views

> **The theory pillar.** This is the entry point to `libcpg`'s foundations. It
> motivates the central idea — merge syntax, control, and data views of a program
> onto **one shared node set** — and maps out the pages that formalise each layer.
> Every term used here is defined once in the [Glossary](../GLOSSARY.md); this page
> links to it rather than redefining. Mathematical notation follows the Glossary's
> [notation conventions](../GLOSSARY.md#notation-conventions): a graph is
> $`G = (V, E)`$ with vertex set $`V`$ and typed edge set $`E`$,
> and $`N`$/$`E`$ denote node/edge **counts** when a scalar is needed.

## 1. Three questions, historically three tools

To reason about a program, an analysis usually needs to answer three different
kinds of question:

- **Syntax** — *"what is written?"* Answered by the
  [Abstract Syntax Tree (AST)](../GLOSSARY.md#abstract-syntax-tree-ast): the tree a
  parser produces, whose interior nodes are language constructs and whose children
  are their syntactic parts.
- **Control** — *"in what order can it run?"* Answered by the
  [Control Flow Graph (CFG)](../GLOSSARY.md#control-flow-graph-cfg): a graph whose
  edges encode the possible execution order between statements.
- **Data** — *"how do values move?"* Answered by the
  [Data Flow Graph (DFG)](../GLOSSARY.md#data-flow-graph-dfg): a graph whose edges
  connect the point that **defines** a value to the points that **use** it.

Classically these are three *separate* data structures, built and queried by three
*separate* passes. A question that spans them — *"can an attacker-controlled value
flow into a buffer size that a surrounding `if` fails to bound?"* — then has to be
stitched together by hand across the AST (to find the syntactic shape), the CFG (to
find the guard), and the DFG (to follow the value). The correspondence between "the
same statement" in three graphs is exactly the accounting that goes wrong.

![Three separate program views versus one unified Code Property Graph](../diagrams/cpg-vs-traditional.svg)

*Figure — three separate views (AST, CFG, DFG) versus one unified CPG over a single node set. Source: [`diagrams/cpg-vs-traditional.dot`](../diagrams/cpg-vs-traditional.dot).*

## 2. The idea: one node set, many typed overlays

A [Code Property Graph (CPG)](../GLOSSARY.md#code-property-graph-cpg) removes the
stitching by construction. Yamaguchi, Golde, Arp, and Rieck [[1]](#references)
observed that the three views do not need three node sets — they only need three
**edge sets over the same nodes**. Build the AST once; then *overlay* control-flow
edges and data-flow edges onto the very same AST nodes. The result is a single
directed graph in which:

- every node is an AST node (a function, an `if`, an identifier, a literal, …), and
- every edge is **typed** — it is an AST edge, or a CFG edge, or a DFG edge, or a
  dependence edge — so a query can restrict itself to one layer or freely compose
  across layers.

Because the node set is shared, "the same statement in the control view and the
data view" is *literally the same node*. Cross-cutting questions become ordinary
graph traversals that hop between edge types, with no side table to keep consistent.
`libcpg` adds a fourth overlay on demand — the
[Program Dependence Graph (PDG)](../GLOSSARY.md#program-dependence-graph-pdg) of
[control](../GLOSSARY.md#control-dependence)- and
[data](../GLOSSARY.md#data-dependence)-dependence edges — which turns
[program slicing](../GLOSSARY.md#program-slicing) into a single reachability query
(see [Theory 04](04-program-dependence-and-slicing.md)).

![AST, CFG, and DFG overlays sharing one node set in a Code Property Graph](../diagrams/cpg-overlay.svg)

*Figure — the AST, CFG, and DFG overlays drawn on one shared node set; each layer is an edge subset over the same vertices. Source: [`diagrams/cpg-overlay.dot`](../diagrams/cpg-overlay.dot).*

## 3. Why unify: the vulnerability-query motivation

The CPG was introduced specifically to *express and discover vulnerabilities as
graph queries* [[1]](#references). A large class of bugs is not visible in any one
view:

- A [taint](../GLOSSARY.md#taint-analysis) bug needs the **DFG** (untrusted value
  reaches a sink) *and* the **CFG/PDG** (no guard on the path sanitises it).
- A missing-check bug needs the **AST** (the call shape) *and* the **control**
  structure around it.
- An injection needs to follow **data** from a source node, classified by its
  **node kind**, to a sink node, classified likewise.

On a unified graph these are single traversals that alternate edge types. The same
substrate then supports higher-level analyses that all reduce to graph operations:
[subgraph-isomorphism](../GLOSSARY.md#isomorphism--subgraph-isomorphism) pattern
detection ([Theory 05](05-subgraph-isomorphism-vf2.md)),
[design-pattern](../GLOSSARY.md#design-pattern) recognition
([Theory 07](07-design-pattern-detection.md)),
[complexity-class](../GLOSSARY.md#complexity-class--big-o) estimation
([Theory 08](08-algorithm-and-complexity-analysis.md)), and
[graph-neural-network](../GLOSSARY.md#graph-neural-network-gnn) embeddings
([Theory 09](09-graph-neural-networks.md)), and
[Design Structure Matrix](../GLOSSARY.md#design-structure-matrix-dsm)
visibility/core analysis ([Theory 10](10-design-structure-matrix-analysis.md)), and
[Weisfeiler–Lehman](../GLOSSARY.md#weisfeilerleman-wl-refinement)
structural-clone refinement
([Theory 11](11-weisfeiler-lehman-structural-clones.md)), and deterministic
[centrality](../GLOSSARY.md#centrality) over projection topology
([Theory 12](12-centrality-and-structural-importance.md)), and deterministic
[community detection](../GLOSSARY.md#community-detection) by generalized
modularity ([Theory 13](13-community-structure-and-modularity.md)), and
[cohesion and structural robustness](../GLOSSARY.md#cohesion--structural-robustness)
from core/truss persistence, low links, weighted cuts, assortativity, and attack
traces ([Theory 14](14-graph-cohesion-and-robustness.md)), and
[algebraic connectivity](../GLOSSARY.md#algebraic-connectivity--fiedler-vector)
with deterministic Fiedler bisection
([Theory 15](15-spectral-connectivity-and-bisection.md)), and bounded
[elementary-cycle](../GLOSSARY.md#elementary-cycle--simple-cycle) enumeration
with canonical rotations and explicit truncation
([Theory 16](16-bounded-simple-cycle-enumeration.md)), and a complete
[directed-triad motif census](../GLOSSARY.md#directed-triad--motif-census)
with selected bounded four-node graphlets
([Theory 17](17-directed-motif-census.md)), and
[Martin package metrics](../GLOSSARY.md#martin-package-metrics) over explicit
architectural module assignments
([Theory 18](18-martin-package-metrics.md)), and pure
[reflexion-model conformance](../GLOSSARY.md#reflexion-model) over a declared
path-to-layer policy ([Theory 19](19-reflexion-model-conformance.md)), and
[architectural layering](../GLOSSARY.md#architectural-layer--layering-violation)
from condensation longest-path levels
([Theory 20](20-longest-path-architectural-layering.md)), and
[architectural smells](../GLOSSARY.md#architectural-smell) from SCC, degree,
Martin-instability, and assigned component-size evidence
([Theory 21](21-advisory-architectural-smells.md)), and
[object-oriented and function metrics](../GLOSSARY.md#code-metrics)
from resolved field/call and normalized node-kind evidence
([Theory 22](22-object-oriented-and-function-metrics.md)), and deterministic
[multi-file program composition](../GLOSSARY.md#program-cpg--file-scope)
with evidence-bounded import linkage
([Theory 24](24-program-composition.md)), and bounded deterministic
[IFDS/IDE](../GLOSSARY.md#ifds--ide) procedure-summary fixed points over the
resolved call graph ([Theory 25](25-ifds-ide-summary-solving.md)), and
[effect summaries](../GLOSSARY.md#effect-summary--place--purity) as a lawful
compositional instance of that framework
([Theory 26](26-interprocedural-effect-lattice.md)), and data-driven advisory
[taint and maybe-null summaries](../GLOSSARY.md#taint-catalog--taint-witness)
with canonical interprocedural evidence
([Theory 27](27-interprocedural-taint-and-nullflow.md)). Each
graph analysis consumes the same
`CodePropertyGraph`; none needs its own bespoke intermediate representation.

## 4. The four overlays at a glance

| Overlay | Question | Edge family | Built by | Detailed in |
|---|---|---|---|---|
| [AST](../GLOSSARY.md#abstract-syntax-tree-ast) | what is written | `AstChild` / `AstParent` / `AstNextSibling` / `AstPrevSibling` | the builder (always) | [Theory 01](01-code-property-graphs.md) |
| [CFG](../GLOSSARY.md#control-flow-graph-cfg) | run order | `ControlFlow(CfgEdgeKind)` — 14 kinds | `CfgExtractor` | [Theory 02](02-control-flow-and-complexity.md) |
| [DFG](../GLOSSARY.md#data-flow-graph-dfg) | value flow | `DataFlow(DfgEdgeKind)` — 13 kinds | `DfgExtractor` | [Theory 03](03-data-flow-and-reaching-definitions.md) |
| [PDG](../GLOSSARY.md#program-dependence-graph-pdg) | what depends on what | `ControlDependence` / `DataDependence` | `PdgBuilder` (on demand) | [Theory 04](04-program-dependence-and-slicing.md) |

All four are subsets of one edge set over one vertex set. The
[node-kind](../GLOSSARY.md#node-kind--edge-kind) and
[edge-kind](../GLOSSARY.md#node-kind--edge-kind) alphabets that make the overlays
addressable are the subject of [Theory 01](01-code-property-graphs.md); the concrete
`petgraph`-backed storage is described in
[`architecture/overview.md`](../architecture/overview.md) and
[`components/graph/overview.md`](../components/graph/overview.md).

## 5. A first taste in code

With a language grammar enabled, the whole unified graph — AST plus CFG plus DFG —
comes from one call, and every overlay is queried through the same
`CodePropertyGraph` value. (`libcpg`'s default feature set is empty, so a
per-language `lang-*` feature is required for the internal-parse `build`; the
feature-free construction paths are covered in [Theory 01](01-code-property-graphs.md)
and [`usage/01-building-cpgs.md`](../usage/01-building-cpgs.md).)

```rust
// requires: features = ["lang-rust"]
use libcpg::{CpgBuilder, TreeSitterCpgBuilder, Language, Result};

fn first_taste() -> Result<()> {
    let source = "fn max(a: i32, b: i32) -> i32 { if a > b { a } else { b } }";

    // One call builds the AST and overlays the CFG and DFG onto its nodes.
    let builder = TreeSitterCpgBuilder::new();
    let cpg = builder.build(source, Language::Rust)?;

    // One value, many views:
    println!("nodes (shared by every overlay): {}", cpg.node_count());
    println!("edges (all overlays combined):   {}", cpg.edge_count());

    // A control-flow metric read straight off the CFG overlay (Theory 02):
    println!("cyclomatic complexity: {}", cpg.cyclomatic_complexity());

    // The AST overlay still tells us the syntactic shape:
    for function in cpg.functions() {
        println!("function: {:?}", function.name());
    }
    Ok(())
}
```

`node_count` counts vertices once no matter how many overlays touch them;
`edge_count` counts every typed edge across all overlays. The `functions()` query
reads the AST layer, while `cyclomatic_complexity()` reads the CFG layer — of the
same graph. Errors are the crate's own [`libcpg::Error`](../api/graph-reference.md)
(there is no `CpgError`).

## 6. Map of the theory pillar

This overview sits above thirty-two formal chapters. Read them in order for a ground-up
development, or jump to the layer you need:

1. **[01 — Code Property Graphs](01-code-property-graphs.md).** The formal model:
   one vertex set, typed edge overlays as edge-induced subgraphs, and the node-kind
   / edge-kind taxonomies that make queries addressable.
2. **[02 — Control Flow and Complexity](02-control-flow-and-complexity.md).** CFG
   theory, [basic blocks](../GLOSSARY.md#basic-block), the 14
   [`CfgEdgeKind`](../GLOSSARY.md#control-flow-graph-cfg) semantics, and McCabe
   [cyclomatic complexity](../GLOSSARY.md#cyclomatic-complexity).
3. **[03 — Data Flow and Reaching Definitions](03-data-flow-and-reaching-definitions.md).**
   Data-flow lattices and fixed points, [reaching definitions](../GLOSSARY.md#reaching-definition),
   and `libcpg`'s [AST-ordered sweep](../GLOSSARY.md#ast-ordered-reaching-definitions).
4. **[04 — Program Dependence and Slicing](04-program-dependence-and-slicing.md).**
   [Dominators](../GLOSSARY.md#dominator--post-dominator), the
   [reverse dominance frontier](../GLOSSARY.md#dominance-frontier--reverse-dominance-frontier),
   the PDG, and Weiser [slicing](../GLOSSARY.md#program-slicing).
5. **[05 — Subgraph Isomorphism and VF2](05-subgraph-isomorphism-vf2.md).**
6. **[06 — Graph Similarity](06-graph-similarity.md).**
7. **[07 — Design-Pattern Detection](07-design-pattern-detection.md).**
8. **[08 — Algorithm and Complexity Analysis](08-algorithm-and-complexity-analysis.md).**
9. **[09 — Graph Neural Networks](09-graph-neural-networks.md).**
10. **[10 — Design Structure Matrices, visibility, and architectural cores](10-design-structure-matrix-analysis.md).**
    Transitive visibility, propagation cost, median quadrants, and the largest
    multi-vertex SCC as distinct architecture evidence.
11. **[11 — Weisfeiler–Lehman structural clones](11-weisfeiler-lehman-structural-clones.md).**
12. **[12 — Centrality and structural importance](12-centrality-and-structural-importance.md).**
13. **[13 — Community structure and modularity](13-community-structure-and-modularity.md).**
14. **[14 — Graph cohesion and robustness](14-graph-cohesion-and-robustness.md).**
15. **[15 — Spectral connectivity and bisection](15-spectral-connectivity-and-bisection.md).**
16. **[16 — Bounded simple-cycle enumeration](16-bounded-simple-cycle-enumeration.md).**
17. **[17 — Directed motif census](17-directed-motif-census.md).**
18. **[18 — Martin package metrics](18-martin-package-metrics.md).**
19. **[19 — Reflexion-model conformance](19-reflexion-model-conformance.md).**
20. **[20 — Longest-path architectural layering](20-longest-path-architectural-layering.md).**
21. **[21 — Advisory architectural smells](21-advisory-architectural-smells.md).**
22. **[22 — Object-oriented and function metrics](22-object-oriented-and-function-metrics.md).**
23. **[23 — Function clone detection](23-function-clone-detection.md).**
    Ordered AST fingerprints, multiset similarity, and bounded PDG WL/VF2
    verification define the four function-clone relations.
24. **[24 — Multi-file program composition](24-program-composition.md).**
    Injective identifier translation, ordered file scopes, and conservative
    alias evidence define the unified program graph and import topology.
25. **[25 — IFDS/IDE summary fixed points](25-ifds-ide-summary-solving.md).**
    A summary lattice, callee-first condensation order, and bounded recursive
    Kleene iteration define the generic interprocedural analysis engine.
26. **[26 — Interprocedural effect lattice](26-interprocedural-effect-lattice.md).**
    Typed storage places, a worst-purity chain, return-dependency bits, and a
    flat scalar lattice compose direct and callee evidence without reparsing.
27. **[27 — Interprocedural taint and maybe-null summaries](27-interprocedural-taint-and-nullflow.md).**
    Open catalog classes, origin-sensitive facts, typed value propagation, and
    canonical witnesses define advisory source-to-sink and null-dereference
    evidence without source-text parsing.
28. **[28 — System dependence and context-sensitive slicing](28-system-dependence-and-slicing.md).**
    Effect-derived summary edges and direction-aware two-pass traversal define
    deterministic bounded slices without unrealizable sibling-call paths.
29. **[29 — Channel topology and linear consumption](29-channel-topology-and-linearity.md).**
    Frontend-normalized message events, definition-resolved channel identities,
    and structural producer/consumer predicates define deterministic advisory
    orphan, blocked, and contention evidence without source reparsing.
30. **[30 — Cyclic wait and lock order](30-cyclic-wait-and-lock-order.md).**
    Initially blocked process relations, typed held-lock order, finite
    interprocedural acquisition summaries, and shared SCC decomposition define
    statement-precise advisory deadlock candidates.
31. **[31 — May-happen-in-parallel relations](31-may-happen-in-parallel.md).**
    Typed parallel operand subtrees, explicit execution-site boundaries,
    canonical symmetric pairs, and a visible output cap define immutable
    structural concurrency evidence.
32. **[32 — Advisory session-protocol duality](32-session-protocol-duality.md).**
    Definition-resolved channel events, ordered shallow payload types, and a
    three-valued binary linear-duality rule define auditable Rholang protocol
    sketches without claiming a complete session type system.
33. **[33 — Finite-state typestate properties](33-finite-state-typestate-properties.md).**
    Caller-defined property automata, allocation-site identity, typed object
    flow, and CFG may-facts define deterministic advisory forbidden-transition
    evidence with explicit completeness bounds.
34. **[34 — Advisory resource, definition, and exception analysis](34-advisory-bug-detection.md).**
    Non-accepting typestate terminals, empty scoped reaching definitions,
    finite exception-type summaries, lexical handler dispatch, uncaught root
    propagation, and dead-handler evidence.

The later chapters define deterministic projection structure, architecture
metrics, inspectable evidence, and their interpretation limits.

For how these ideas are realised in code, cross the bridge to the architecture
pillar ([`architecture/overview.md`](../architecture/overview.md),
[`architecture/data-flow.md`](../architecture/data-flow.md)) and the component
references ([`components/graph/overview.md`](../components/graph/overview.md)).

## References

1. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and Discovering Vulnerabilities with Code Property Graphs.* 2014 IEEE Symposium on Security and Privacy. DOI: [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44)
