# Theory 44 — Deterministic CPG representations for external ML

A machine-learning (ML) representation is useful only when its coordinates
have stable meanings. For a Code Property Graph (CPG), that means more than
turning nodes into consecutive integers: node rows must retain typed syntax,
edges must retain overlay semantics, line-level evidence must stay aligned with
source locations, and path-based contexts must remain reconstructable.

This chapter defines the two feature-free representations implemented by
`export_pyg` and `ast_paths`. **PyG** abbreviates PyTorch Geometric, the common
consumer convention that motivates an edge-index table; libcpg does not depend
on PyG or PyTorch. **AST**, **CFG**, **DFG**, and **PDG** mean Abstract Syntax
Tree, Control-Flow Graph, Data-Flow Graph, and Program Dependence Graph.

![Canonical CPG evidence exported for external ML consumers](../diagrams/ml-export-pipeline.svg)

## 1. Canonical graph coordinates

Let $`G=(V,E,kappa_V,kappa_E,ho)`$ be a CPG, where:

- $`V`$ is the finite node set;
- $`E`$ is the edge multiset, so parallel typed edges remain distinct;
- $`\kappa_V:V\to K_V`$ assigns one of 45 node kinds;
- $`\kappa_E:E\to K_E`$ assigns one of 60 edge relations; and
- $`\rho:V\to R`$ assigns a source range.

Let $`\operatorname{id}(v)`$ be the stable `NodeId` of node $`v`. Canonical
dense order is the unique ascending sequence $`(v_0,\ldots,v_{n-1})`$ such
that:

```math
\operatorname{id}(v_i)<\operatorname{id}(v_j)\quad\text{whenever }i<j.
```

The dense map $`d(v_i)=i`$ is therefore independent of petgraph insertion
order. `PygExport::node_ids[i]`, `source_ranges[i]`, and `node_features[i]`
all describe the same $`v_i`$. A model prediction at row $`i`$ maps back to
source through this alignment, not through an unstable internal graph index.

Every edge $`e=(u,v)`$ becomes:

```math
\bigl(d(u),d(v),c_E(\kappa_E(e))\bigr),
```

where $`c_E:K_E\to\{0,\ldots,59\}`$ is the frozen relation-class map. Tuples
are ordered lexicographically, but equal tuples are retained. Thus two typed
parallel relations with equal endpoints and class contribute two rows rather
than collapsing into one.

## 2. The 73-column node schema

For node $`v_i`$, the exported row $`x_i\in\mathbb{R}^{73}`$ concatenates six
families:

```math
x_i=
\left[
\operatorname{onehot}_{45}(\kappa_V(v_i)),
\deg^-_{11}(v_i),
\deg^+_{11}(v_i),
h(v_i),
k(v_i),
\lambda(v_i),
p(v_i),
p^R(v_i),
s(v_i)
\right].
```

The symbols are defined before interpretation:

- $`\operatorname{onehot}_{45}`$ is the exact 45-kind indicator vector;
- $`\deg^-_{11}`$ and $`\deg^+_{11}`$ are incoming and outgoing degree vectors
  for AST, CFG, DFG, PDG, call, type, reference, scope, import, Heap, and
  NameFlow layers;
- $`h(v)`$ is the represented AST depth, or zero when unknown;
- $`k(v)\in\{0,1\}`$ is the depth-known bit;
- $`\lambda(v)`$ is represented AST loop-nesting depth;
- $`p(v)`$ and $`p^R(v)`$ are PageRank on forward and reversed topology; and
- $`s(v)\in\{0,1\}`$ marks membership in one optional complete backward PDG
  slice.

The equation has 45 kind columns, 22 layer-degree columns, and six scalar
columns. `PYG_NODE_FEATURE_NAMES` is the ordered schema registry, so consumers
should resolve indices by name rather than repeat numeric offsets.

### 2.1 Typed degrees

The eleven degree layers deliberately coarsen relations without erasing the
typed edge table. For layer $`j`$ with relation subset $`K_E^j`$:

```math
\deg_j^+(v)=
\left|\{e\in E\mid \operatorname{src}(e)=v\land
\kappa_E(e)\in K_E^j\}\right|,
```

and incoming degree is defined symmetrically. Because $`E`$ is a multiset,
parallel edges contribute to degree multiplicity exactly as they contribute to
`edge_index`.

### 2.2 AST depth with explicit uncertainty

Duplicate `AstChild` edges with the same endpoints are first normalized as one
structural parent relation. Let $`P(v)`$ be the remaining parent set. Roots
have $`|P(v)|=0`$. A root-seeded breadth-first search assigns:

```math
h(r)=0,\qquad
h(v)=h(u)+1\quad\text{when }P(v)=\{u\}\text{ and }h(u)\text{ is known}.
```

The known predicate is $`k(v)=1`$ exactly for those assigned nodes. A
multiple-parent node, a cycle without a root, and every descendant blocked by
such ambiguity has $`k(v)=0`$. The numeric zero in $`h(v)`$ is then only a
storage value; it must be interpreted with $`k(v)`$. This two-column encoding
prevents an unknown node from looking like a genuine root.

Loop depth $`\lambda(v)`$ is accumulated along the same known chain, adding
one when the current node kind is `While`, `For`, or `Loop`.

### 2.3 Global topology centrality

PageRank uses an unweighted directed projection of the entire CPG. Repeated
endpoint pairs are merged in this projection, because typed multiplicity is
already represented elsewhere. With damping $`\alpha`$, node count $`n`$, and
predecessor set $`\operatorname{pred}(v)`$, one iteration has the familiar
shape:

```math
p_{t+1}(v)=\frac{1-\alpha}{n}
+\alpha\sum_{u\in\operatorname{pred}(v)}
\frac{p_t(u)}{\deg^+(u)},
```

with dangling mass handled by the shared centrality implementation. Reverse
PageRank applies the same process after reversing the topology. The export
retains `pagerank_converged` and `reverse_pagerank_converged`; a score does not
erase its numerical convergence status.

## 3. Complete PDG-slice localization

For criterion $`q\in V`$, let $`E_D`$ be only `ControlDependence` and
`DataDependence` edges. The backward slice is the least set $`S_q`$ satisfying:

```math
q\in S_q,
\qquad
(u,v)\in E_D\land v\in S_q\Longrightarrow u\in S_q.
```

The membership feature is $`s(v)=1`$ exactly when $`v\in S_q`$. Importantly,
libcpg computes $`S_q`$ with a graph-sized internal limit and only then checks
the caller's `max_slice_nodes`. If $`|S_q|`$ exceeds that cap, the operation
fails. It never emits $`S'_q\subset S_q`$ as though omitted dependencies were
negative labels.

## 4. Exact AST path contexts

Path-based code models represent a function through pairs of terminal tokens
and the syntax path joining them. code2vec popularized this representation
[[1]](#references). libcpg retains more evidence than the minimal learned
triple so that callers can verify and transform it.

For validated function $`f`$, let $`V_f`$ contain $`f`$ and its AST
descendants, stopping before nested functions. Let $`T_f`$ contain scoped nodes
other than $`f`$ with no `AstChild` successor. These are the **terminals** for
this export; they need not be lexical tokens in a particular grammar.

For terminal $`a`$, the parent chain is admitted only when every step has one
unique parent and reaches $`f`$ without repetition. A missing parent, multiple
parents, or a cycle suppresses the chain. For admitted distinct terminals
$`a,b`$, let $`\operatorname{lca}(a,b)`$ be their lowest common ancestor.
The exact path is:

```math
\pi(a,b)=
[a,\ldots,\operatorname{lca}(a,b),\ldots,b].
```

`PathContext::path` stores every node of $`\pi`; `ancestor_index` identifies
the lowest common ancestor; and `node_kinds` stores one aligned class per path
node. Therefore every adjacent pair can be checked as one `AstChild` edge in
either direction.

![Exact terminal-to-terminal AST path reconstruction](../diagrams/ast-path-context.svg)

### 4.1 Source order and path width

Children are ordered by `(range.start, range.end, range.start_line,
range.start_col, NodeId)`. Let $`i_a`$ and $`i_b`$ be the child-slot positions
of the two branches immediately beneath the lowest common ancestor. The path
width is:

```math
w(a,b)=|i_a-i_b|.
```

The length is the edge count $`|\pi(a,b)|-1`$. A context is retained only when
both values fit the caller's filters. Terminals themselves are processed in
source order, and unordered pairs use that order, making token direction
deterministic.

### 4.2 Token evidence

The terminal token uses the first represented source of meaning:

1. non-empty exact `CpgNode::text`;
2. the node's semantic name;
3. a represented operator or literal value; or
4. the stable node-kind schema name.

This is a presentation fallback, not parsing. The exact terminal `NodeId`,
source range through the CPG, and node-kind class remain authoritative.

## 5. Literate algorithms

The graph export narrates alignment before numerical construction:

```text
CanonicalGraphExport(CPG, optional criterion):
    Place every NodeId in a min-order heap; drain it into node_ids.
    Build stable-id -> dense-id and aligned source-range tables.
    Scan every typed edge once:
        Increment its coarse input/output degree columns.
        Place its complete dense typed tuple in a min-order heap.
    Traverse normalized AstChild roots with a breadth-first queue.
    Run canonical forward and reverse PageRank projections.
    If requested, compute the complete backward PDG slice and enforce its cap.
    Fill one 73-column row for each dense node.
    Return aligned rows, edges, ids, ranges, slice evidence, and convergence.
```

Path extraction makes native call depth independent of AST height:

```text
CanonicalAstPaths(CPG, function, limits):
    Validate the function and every compile-time ceiling.
    Collect its AST scope with an explicit worklist.
    Build normalized parent/child vectors and source-ordered child slots.
    Place every terminal in a source-order heap; enforce the terminal cap.
    For each terminal:
        Walk toward the function in a heap vector.
        Use generation marks to reject repetition without clearing per chain.
        Suppress missing or ambiguous chains; enforce aggregate ancestor steps.
    Compute the exact terminal-pair count; enforce its cap before enumeration.
    For each unordered admitted pair:
        Compare chain suffixes to find the lowest common ancestor.
        Apply length and width filters.
        Enforce context and retained-node caps before appending.
        Materialize the exact upward/downward path and aligned evidence.
    Return the complete retained context vector.
```

No phase uses native recursion over graph or source depth.

## 6. Complexity and resource policy

Let $`n=|V|`$, $`m=|E|`$, $`t=|T_f|`$, $`H`$ be maximum admitted parent
height, $`c`$ be emitted context count, and $`L`$ be aggregate emitted path
nodes. Heap canonicalization costs $`O((n+m)\log(n+m))`$ in the current simple
form. Degree and AST work are linear after ordering. PageRank costs
$`O(I(n+m))`$ for $`I`$ bounded iterations on the normalized topology.

Path chain construction costs $`O(tH)`$ in the deepest case. Pair examination
costs $`O(t^2)`$, while retained output storage costs $`O(L)`. The public caps
bound terminals, pair checks, contexts, total retained nodes, and aggregate
ancestor steps. Compile-time ceilings prevent a caller from disabling those
allocation guards with arbitrarily large options.

## 7. Interpretation limits

`PygExport` is evidence, not a model. Node features do not contain learned
weights, data-set labels, or a claim that PageRank predicts vulnerability.
Global centrality can leak whole-program context if training and evaluation
splits are made after export. Source ranges and exact tokens may reveal source
content. Consumers own normalization, batching, train/test isolation, model
calibration, and privacy controls.

`PathContext` is a represented-syntax view, not proof of parser validity. A
malformed terminal chain is suppressed. A clean empty result can also arise
from length/width filters. A cap violation is distinguishable because it is an
error, but malformed suppression is not counted in the current result type.

LineVD and IVDetect motivate structural and line-aligned features, but libcpg
does not claim to reproduce their training data, architectures, accuracy, or
vulnerability labels [[2]](#references), [[3]](#references). Presence means
only that a deterministic representation was constructed from the supplied
CPG.

## References

1. Alon, U., Zilberstein, M., Levy, O., Yahav, E. (2019). *code2vec: Learning
   Distributed Representations of Code.* DOI:
   [10.1145/3290353](https://doi.org/10.1145/3290353).
2. Hin, D., Kan, A., Chen, H., Babar, M. A. (2022). *LineVD: Statement-level
   Vulnerability Detection using Graph Neural Networks.* DOI:
   [10.1145/3524842.3527949](https://doi.org/10.1145/3524842.3527949).
3. Li, Y., Wang, S., Nguyen, T. N. (2021). *Vulnerability Detection with Fine-
   grained Interpretations.* DOI:
   [10.1145/3468264.3468597](https://doi.org/10.1145/3468264.3468597).
4. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and
   Discovering Vulnerabilities with Code Property Graphs.* DOI:
   [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44).
