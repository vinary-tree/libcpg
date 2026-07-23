# Theory 01 — The Code Property Graph: Formal Model

> **Where this sits.** [Theory 00](00-overview.md) motivated merging the syntax,
> control, and data views onto one node set. This chapter makes that precise: it
> defines the [Code Property Graph](../GLOSSARY.md#code-property-graph-cpg) as a
> single typed graph, defines each overlay ([AST](../GLOSSARY.md#abstract-syntax-tree-ast),
> [CFG](../GLOSSARY.md#control-flow-graph-cfg), [DFG](../GLOSSARY.md#data-flow-graph-dfg),
> [PDG](../GLOSSARY.md#program-dependence-graph-pdg)) as an **edge-induced subgraph**,
> catalogues the node-kind and edge-kind alphabets, and shows how queries compose
> across layers. Notation follows the [Glossary conventions](../GLOSSARY.md#notation-conventions).

## 1. Definition

A Code Property Graph — the representation introduced by Yamaguchi et al.
[[1]](#references) — is a **directed, edge-typed, vertex-labelled multigraph**

```math
G = (V,\ E,\ \kappa,\ \tau)
```

where

- $`V`$ is a finite set of **vertices**, one per AST node of the analysed
  program;
- $`E`$ is a finite set of directed **edges**; each $`e \in E`$ has a
  source $`\mathrm{src}(e) \in V`$ and a target $`\mathrm{tgt}(e) \in V`$
  (it is a multigraph — two vertices may be joined by several edges of different
  types, e.g. an AST edge *and* a CFG edge);
- $`\kappa : V \to K`$ is the **node-kind labelling**, assigning every vertex
  a kind from the alphabet $`K`$ of 45 [`CpgNodeKind`](../GLOSSARY.md#node-kind--edge-kind)
  variants;
- $`\tau : E \to T`$ is the **edge-kind labelling**, assigning every edge a
  kind from the alphabet $`T`$ of [`CpgEdgeKind`](../GLOSSARY.md#node-kind--edge-kind)
  variants.

The essential design commitment is that **$`V`$ is shared by every layer**.
There is exactly one vertex per program element; the AST, CFG, DFG, and PDG differ
only in *which edges they contribute to $`E`$*. This is what makes "the same
statement, seen three ways" a single object rather than three cross-referenced ones.

In `libcpg` this graph is the type `CodePropertyGraph`, which wraps a
[`petgraph`](../GLOSSARY.md#petgraph) `DiGraph<CpgNode, CpgEdge>`. A `CpgNode`
carries its identity `id: NodeId`, its label `kind: CpgNodeKind`, its
[`SourceRange`](../GLOSSARY.md#code-property-graph-cpg), and its AST `children` /
`parent`; a `CpgEdge` carries `id: EdgeId`, `source`, `target`, and
`kind: CpgEdgeKind`. Storage and memory layout are described in
[`architecture/overview.md`](../architecture/overview.md) and
[`components/graph/overview.md`](../components/graph/overview.md); this page stays at
the level of the model.

## 2. Overlays as edge-induced subgraphs

Partition the edge alphabet $`T`$ into families and define, for each family
$`F \subseteq T`$, the **edge subset**

```math
E_F \;=\; \{\, e \in E : \tau(e) \in F \,\}
```

and the corresponding **overlay** $`G_F = (V, E_F)`$ — the same vertices,
restricted to the edges of that family. `libcpg` exposes exactly this partition
through predicate methods on `CpgEdgeKind` (`is_ast`, `is_cfg`, `is_dfg`, `is_pdg`,
`is_call`, `is_type`), so an overlay is recovered by *filtering* the one edge set:

```math
\begin{aligned}
E_{\mathrm{AST}} &= \{\, e : \texttt{is\_ast}(\tau(e)) \,\} & &\text{(tree structure)}\\
E_{\mathrm{CFG}} &= \{\, e : \texttt{is\_cfg}(\tau(e)) \,\} & &\text{(execution order)}\\
E_{\mathrm{DFG}} &= \{\, e : \texttt{is\_dfg}(\tau(e)) \,\} & &\text{(value flow)}\\
E_{\mathrm{PDG}} &= \{\, e : \texttt{is\_pdg}(\tau(e)) \,\} & &\text{(dependence)}
\end{aligned}
```

Three properties follow directly and are worth stating because analyses rely on
them:

1. **Shared vertices.** Every overlay is a graph on the *whole* of $`V`$. A
   vertex with no CFG edge is still present in $`G_{\mathrm{CFG}}`$ as an
   isolated vertex; it is simply not part of the control flow.
2. **Disjoint edge families.** $`E_{\mathrm{AST}}`$,
   $`E_{\mathrm{CFG}}`$, $`E_{\mathrm{DFG}}`$, $`E_{\mathrm{PDG}}`$,
   and the call/type/reference/scope/import families are pairwise disjoint — each
   edge has exactly one kind — so the overlays never double-count an edge.
3. **The AST is the base layer.** $`E_{\mathrm{AST}}`$ is always present (the
   builder produces it first); the other families are overlaid on top. The CFG and
   DFG are built by their extractors; the PDG — the dependence overlay of
   Ferrante–Ottenstein–Warren [[2]](#references) — is added on demand
   (see [Theory 04](04-program-dependence-and-slicing.md)).

![The AST, CFG, and DFG overlays over one shared node set](../diagrams/cpg-overlay.svg)

*Figure — the AST (base), CFG, and DFG as edge subsets over a single vertex set; the PDG overlays two further dependence families on the same nodes. Source: [`diagrams/cpg-overlay.dot`](../diagrams/cpg-overlay.dot).*

## 3. The node-kind alphabet $`K`$

A vertex's kind is what every query, pattern, and heuristic dispatches on, so the
alphabet is deliberately expressive: 45 [`CpgNodeKind`](../GLOSSARY.md#node-kind--edge-kind)
variants, a mix of **unit** variants (`Root`, `If`, `Return`, `Await`, …) and
**data-carrying** variants that attach structured payloads (e.g.
`Function { signature: MethodSignature }`, `Call { target: Option<NodeId>, is_method: bool }`,
`Identifier { name, definition }`, `Literal { kind: LiteralKind }`). They fall into
seven groups:

| Group | Count | Representative variants |
|---|---|---|
| Structural | 7 | `Root`, `Module`, `Class`, `Struct`, `Enum`, `Trait`, `Impl` |
| Function-level | 3 | `Function`, `Parameter`, `Block` |
| Variable | 2 | `Variable`, `Field` |
| Statement | 14 | `Return`, `If`, `Else`, `While`, `For`, `Loop`, `Match`, `MatchArm`, `Break`, `Continue`, `Throw`, `Try`, `Catch`, `Finally` |
| Expression | 11 | `BinaryOp`, `UnaryOp`, `Assignment`, `Call`, `MemberAccess`, `IndexAccess`, `Identifier`, `Literal`, `Lambda`, `Await`, `Yield` |
| Type | 2 | `TypeAnnotation`, `GenericParam` |
| Special | 6 | `Comment`, `Import`, `Attribute`, `Macro`, `Error`, `Unknown` |

Being *language-agnostic* is the point: a tree-sitter grammar for any of the 16
built-in languages — or a caller-supplied Rholang/MeTTa grammar via
[Mode B](../GLOSSARY.md#mode-b--build_from_tree) — is mapped onto this fixed
vocabulary, so downstream analyses are written once against $`K`$ rather than
per language. The full variant-by-variant reference lives in
[`components/graph/nodes.md`](../components/graph/nodes.md) and
[`api/graph-reference.md`](../api/graph-reference.md).

![The 45 CpgNodeKind variants grouped by category](../diagrams/node-kind-taxonomy.svg)

*Figure — the 45 `CpgNodeKind` variants organised by group. Source: [`diagrams/node-kind-taxonomy.puml`](../diagrams/node-kind-taxonomy.puml).*

## 4. The edge-kind alphabet $`T`$

Edges are typed so that overlays are addressable. `CpgEdgeKind` groups into the
families below; the CFG and DFG families are *wrappers* around finer sub-alphabets
(`ControlFlow(CfgEdgeKind)` with 14 inner kinds — [Theory 02](02-control-flow-and-complexity.md);
`DataFlow(DfgEdgeKind)` with 13 — [Theory 03](03-data-flow-and-reaching-definitions.md)):

| Family | Predicate | Kinds |
|---|---|---|
| AST | `is_ast` | `AstChild`, `AstParent`, `AstNextSibling`, `AstPrevSibling` |
| CFG | `is_cfg` | `ControlFlow(CfgEdgeKind)` — 14 inner variants |
| DFG | `is_dfg` | `DataFlow(DfgEdgeKind)` — 13 inner variants |
| PDG | `is_pdg` | `ControlDependence`, `DataDependence` |
| Call graph | `is_call` | `StaticCall`, `DynamicCall`, `CallSite` |
| Type | `is_type` | `TypeOf`, `Inherits`, `Implements`, `GenericInstance` |
| Reference | — | `Reference`, `Definition`, `Declaration` |
| Scope | — | `EnclosingScope`, `ContainedIn` |
| Import | — | `Imports`, `Exports` |

The bidirectional AST edges (`AstChild`/`AstParent`, `AstNextSibling`/`AstPrevSibling`)
plus each `CpgNode`'s explicit `parent` pointer and `children` list let the tree be
walked in either direction and let **source order be recovered** even though
`petgraph` does not itself order incident edges — the mechanism is detailed in
[`architecture/overview.md`](../architecture/overview.md) and
[`components/graph/edges.md`](../components/graph/edges.md).

![CpgEdgeKind grouped into AST, CFG, DFG, PDG and further families](../diagrams/edge-kind-taxonomy.svg)

*Figure — the `CpgEdgeKind` alphabet grouped into AST / CFG / DFG / PDG / call / type / reference / scope / import families. Source: [`diagrams/edge-kind-taxonomy.puml`](../diagrams/edge-kind-taxonomy.puml).*

## 5. How queries compose across layers

Because overlays share $`V`$, a cross-layer query is a **composition of edge
relations**, filtered by node kind. Writing $`u \xrightarrow{F} v`$ for "there
is an edge $`e \in E_F`$ with $`\mathrm{src}(e)=u`$,
$`\mathrm{tgt}(e)=v`$", a [taint](../GLOSSARY.md#taint-analysis) query is the
existence of a data-flow path between two vertices selected by their kind:

```math
\mathrm{tainted}(s, t) \;\equiv\; \kappa(s) \in \mathit{Sources} \;\wedge\; \kappa(t) \in \mathit{Sinks} \;\wedge\; s \xrightarrow{\ \mathrm{DFG}\ ^{+}} t
```

where $`\xrightarrow{\ \mathrm{DFG}\ ^{+}}`$ is the transitive closure of the
DFG edge relation. A *guarded*-taint refinement additionally asks the PDG whether a
sanitising branch dominates the path — the same nodes, a different overlay. The
point of the model is that this is one traversal over one graph, not a join across
three.

### Literate procedure — a cross-layer walk

```text
Query: does definition d reach a call argument on some execution path?
Inputs: CPG G; definition node d.

 1. uses ← every u with  d ─DFG→ u          (data overlay: who reads d)
 2. for each u in uses:
 3.     a ← nearest AST ancestor of u whose kind is Call   (syntax overlay)
 4.     if a exists and u is an argument child of a:
 5.         report (d reaches call a via u)
 6. // to also require reachability, intersect with cfg-reachable(entry, a)
```

Step 1 reads $`E_{\mathrm{DFG}}`$, step 3 reads $`E_{\mathrm{AST}}`$, and
the optional step 6 reads $`E_{\mathrm{CFG}}`$ — all indexing into the same
vertices. The corresponding real API restricts each hop to its overlay:

```rust
use libcpg::{CodePropertyGraph, CpgNodeKind, NodeId};

/// Every `Call` node that a definition `d` flows into, via the DFG overlay,
/// where the flowed-to identifier is a child (argument) of the call.
fn calls_reached_by(cpg: &CodePropertyGraph, d: NodeId) -> Vec<NodeId> {
    let mut hits = Vec::new();
    // DFG overlay: uses of the definition (DefUse / ReachingDef edges).
    for use_site in cpg.uses_of_definition(d) {
        // AST overlay: climb to the enclosing node.
        if let Some(parent) = cpg.ast_parent(use_site) {
            if let Some(node) = cpg.node(parent) {
                if matches!(node.kind, CpgNodeKind::Call { .. }) {
                    hits.push(parent);
                }
            }
        }
    }
    hits
}
```

`uses_of_definition` traverses only DFG edges; `ast_parent` and `node` read the AST
overlay and the node label — three layers, one graph, no side tables. The same
composition underlies subgraph matching ([Theory 05](05-subgraph-isomorphism-vf2.md)),
where a whole *pattern graph* over $`(V, E)`$ is matched against the CPG, and
GNN message passing ([Theory 09](09-graph-neural-networks.md)), where a node's
neighbourhood is taken across the AST, CFG, and DFG overlays at once.

### Constructing the model directly

The model is small enough to build by hand — useful for tests and for
understanding, and available with **no feature flags** (the default feature set is
empty). The first node added becomes the graph [root](../GLOSSARY.md#code-property-graph-cpg);
`add_node` assigns each vertex a fresh `NodeId` and returns it; `connect` adds a
typed edge:

```rust
use libcpg::{
    CodePropertyGraph, CpgNode, CpgNodeKind, CpgEdgeKind, CfgEdgeKind,
    Language, NodeId, SourceRange,
};

let mut cpg = CodePropertyGraph::new(Language::Rust);

// Two vertices in the shared set V.
let root = cpg.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::Root, SourceRange::default()));
let ret  = cpg.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::Return, SourceRange::default()));

// One AST edge and one CFG edge over the SAME two vertices (a multigraph).
cpg.connect(root, ret, CpgEdgeKind::AstChild);
cpg.connect(root, ret, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential));

assert_eq!(cpg.node_count(), 2); // vertices counted once…
assert_eq!(cpg.edge_count(), 2); // …edges counted per typed overlay
```

The two edges join the *same* pair of vertices with different kinds — the concrete
realisation of the "one node set, many overlays" model. From here, running the
`CfgExtractor` and `DfgExtractor` populates the control and data overlays
automatically, and `PdgBuilder` adds the dependence overlay; those construction
stages are the subject of the next three chapters and of
[`architecture/data-flow.md`](../architecture/data-flow.md).

## References

1. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and Discovering Vulnerabilities with Code Property Graphs.* 2014 IEEE Symposium on Security and Privacy. DOI: [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44)
2. Ferrante, J., Ottenstein, K. J., Warren, J. D. (1987). *The Program Dependence Graph and Its Use in Optimization.* ACM TOPLAS 9(3). DOI: [10.1145/24039.24041](https://doi.org/10.1145/24039.24041)
