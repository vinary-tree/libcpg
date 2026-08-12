# Graph Similarity

> Theory pillar · file 06. The graded counterpart to exact [subgraph isomorphism](05-subgraph-isomorphism-vf2.md); the learned counterpart is [graph neural networks](09-graph-neural-networks.md).

[Subgraph isomorphism](../GLOSSARY.md#isomorphism--subgraph-isomorphism) ([file 05](05-subgraph-isomorphism-vf2.md)) answers a *yes/no* question: does this exact shape occur? Often the useful question is *how much*: how close are two functions, is this refactoring structurally equivalent to the original, which of a thousand snippets most resembles a known‑vulnerable one? For those, `libcpg` computes a **[similarity metric](../GLOSSARY.md#similarity-metric)** — a score in $`[0, 1]`$ where $`1`$ means "as alike as this metric can tell" and $`0`$ means "nothing in common". This page defines the four metrics `libcpg` ships, the mathematics behind each, and when to reach for which.

All of this lives in the always‑on `pattern` module, in the `GraphSimilarity` calculator.

```rust
use libcpg::pattern::{GraphSimilarity, SimilarityMetric};

let sim = GraphSimilarity::new()                       // metric = Jaccard by default
    .with_metric(SimilarityMetric::WeisfeilerLehman);  // pick a metric

let score: f64 = sim.similarity(&graph_a, &graph_b);   // in [0, 1]
```

`SimilarityMetric` selects the strategy; `similarity(&g1, &g2)` dispatches to it and returns the score. The default is `Jaccard`.

![Four similarity metrics side by side — Jaccard multiset overlap, cosine of feature vectors, Weisfeiler‑Lehman label histograms, and a bounded bipartite graph-edit upper bound — each mapping a pair of graphs to a score in the unit interval.](../diagrams/similarity-metrics.svg)

*Figure — the four `SimilarityMetric` strategies and the graph signal each one consumes. Source: [`diagrams/similarity-metrics.puml`](../diagrams/similarity-metrics.puml).*

---

## 1. Jaccard similarity (the default)

The [Jaccard index](../GLOSSARY.md#jaccard-similarity) is the size of the intersection over the size of the union of two sets:

```math
J(A, B) = \frac{|A \cap B|}{|A \cup B|}.
```

`libcpg` applies it not to raw sets but to the **multiset of node kinds** of each graph. Every node contributes its [`NodeKindTag`](../GLOSSARY.md#node-kind--edge-kind); let $`a_k`$ and $`b_k`$ be how many nodes of kind $`k`$ graphs $`A`$ and $`B`$ contain. The multiset generalisation of Jaccard is

```math
J(A, B) = \frac{\sum_k \min(a_k, b_k)}{\sum_k \max(a_k, b_k)},
```

which is exactly what the code computes: the intersection sums $`\min(a_k,b_k)`$ over the kinds present, and the union sums $`\max(a_k,b_k)`$ (adding the counts of kinds that appear only in $`B`$). Two empty graphs are defined to be identical ($`J = 1`$, avoiding $`0/0`$).

**What it captures and misses.** Jaccard on node‑kind multisets is a *bag‑of‑constructs* measure: it asks whether two programs are built from the same mix of ifs, calls, and assignments, in similar proportions. It is fast ($`O(|V_A| + |V_B|)`$) and permutation‑invariant, but it is **blind to wiring** — two graphs with identical node counts but completely different edges score $`1`$. That is a feature for a cheap first pass and a limitation the other metrics exist to address.

---

## 2. Cosine similarity of feature vectors

[Cosine similarity](../GLOSSARY.md#cosine-similarity) measures the angle between two vectors, ignoring their magnitudes:

```math
\cos(u, v) = \frac{u \cdot v}{\lVert u \rVert \, \lVert v \rVert} \in [-1, 1].
```

The `Cosine` metric summarises each *whole graph* as a fixed **9‑dimensional feature vector** and compares the two vectors. The features (each softly normalised toward $`[0,1]`$) are:

| # | Feature | Normalisation |
|---|---|---|
| 1 | node count | $`\min(N/1000,\,1)`$ |
| 2 | edge count | $`\min(E/2000,\,1)`$ |
| 3 | AST‑edge ratio | AST edges / total edges |
| 4 | CFG‑edge ratio | CFG edges / total edges |
| 5 | DFG‑edge ratio | DFG edges / total edges |
| 6 | function count | $`\min(\#\text{fns}/100,\,1)`$ |
| 7 | class count | $`\min(\#\text{classes}/50,\,1)`$ |
| 8 | AST depth | $`\min(\text{depth}/50,\,1)`$ |
| 9 | [cyclomatic complexity](../GLOSSARY.md#cyclomatic-complexity) | $`\min(M/100,\,1)`$ |

If either vector is all zeros the score is $`0`$; otherwise it is the cosine above. Because the vector blends *counts* (features 1–2, 6–7) with *shape ratios* (features 3–5, 8–9), cosine rewards graphs that have a similar **overall composition** — a comparable balance of syntax vs. control vs. data flow, similar depth, similar branch density — even at different absolute sizes. It is coarser than a node‑by‑node comparison but robust to scale, which makes it a good clustering key for "programs of the same character".

---

## 3. The Weisfeiler‑Lehman kernel

Jaccard sees node kinds; cosine sees aggregate ratios; neither sees **local structure**. The [Weisfeiler‑Lehman (WL) kernel](../GLOSSARY.md#weisfeiler-lehman-kernel--label-refinement) does, by *refining* each node's label to encode its growing neighbourhood, then comparing the resulting label distributions. It descends from the 1968 Weisfeiler‑Leman graph‑canonisation heuristic and was cast as a scalable graph kernel by Shervashidze et al. [[1]](#references). `libcpg` runs **3 refinement iterations**.

### Label refinement

Start every node with a label equal to (a hash of) its node‑kind tag. Then repeatedly rewrite each node's label to a hash of *its own label together with the sorted multiset of its neighbours' labels*:

```math
\ell^{(i)}(v) = \operatorname{hash}\Big(\ell^{(i-1)}(v),\ \operatorname{sort}\big(\{\, \ell^{(i-1)}(u) : u \in \mathcal{N}(v) \,\}\big)\Big).
```

Sorting makes the label **order‑independent** (a canonical fingerprint of the
neighbourhood); hashing compresses it back to a single token so the next round
is cheap. After $`i`$ rounds, a node's label fingerprints its entire $`i`$‑hop
neighbourhood. Equal labels are structural-candidate evidence, not a proof:
one-dimensional WL can fail to distinguish graphs, and finite hashes can
collide.

The hash combiner is the same fixed SplitMix-style wrapping sequence used by
projection WL. It does not use `DefaultHasher`, so its constants and output are
stable across Rust releases; an exact-vector regression fixes that contract.

In `libcpg` the neighbourhood $`\mathcal N(v)`$ is **undirected**: it pools the labels reachable along both outgoing and incoming edges, across all overlays. As literate pseudocode:

```text
Algorithm WL‑Histogram(G, iterations = 3):
  for each node v:  ℓ[v] ← hash( NodeKindTag(v) )        # round 0 labels
  hist ← empty multiset                                   # label → count
  repeat `iterations` times:
    ℓ' ← { }
    for each node v:
      nbr ← sort( [ ℓ[u] for u in out‑neighbours(v) ]     # both directions,
                ++ [ ℓ[w] for w in in‑neighbours(v)  ] )   #   all edge kinds
      ℓ'[v] ← hash( ℓ[v], nbr )
      hist[ ℓ'[v] ] += 1                                   # tally each refined label
    ℓ ← ℓ'
  for each node v:  hist[ ℓ[v] ] += 1                      # final round tallied once more
  return hist
```

The result is a **histogram** of every refined label the graph produced across the three rounds (the last round's labels are counted once more when the sweep ends — a harmless quirk applied identically to both graphs). The initial round‑0 labels are not tallied on their own; they enter only through the refinements they seed.

This whole-CPG similarity metric is distinct from the feature-free
[`wl_labels`](11-weisfeiler-lehman-structural-clones.md) projection API. The
projection API seeds directed degree, uses a fixed cross-release mixer, accepts
a bounded round count, returns final per-node labels, and groups structural
clone candidates inside one graph. Numeric labels from the two variants are
not interchangeable.

### Comparing two histograms

Two graphs are then scored by the **cosine of their label histograms**. Treat each distinct label as a coordinate; a label present in only one graph contributes $`0`$ to the dot product:

```math
\operatorname{sim}_{\text{WL}}(A, B) = \frac{\sum_\ell c^A_\ell\, c^B_\ell}{\sqrt{\sum_\ell (c^A_\ell)^2}\ \sqrt{\sum_\ell (c^B_\ell)^2}},
```

where $`c^A_\ell`$ is the count of label $`\ell`$ in graph $`A`$. Shared labels
provide local-structure evidence, so WL distinguishes graphs that Jaccard and
cosine may call identical at the cost of three refinement passes. `GraphEdit`
goes further by pricing typed wiring changes, at cubic assignment cost.

![Weisfeiler‑Lehman label refinement: node labels start as kind tags and, over successive rounds, absorb sorted neighbour labels, producing a histogram used as a graph fingerprint.](../diagrams/wl-kernel.svg)

*Figure — one graph's WL refinement over iterations 0→3 and the resulting label histogram fingerprint. Source: [`diagrams/wl-kernel.dot`](../diagrams/wl-kernel.dot).*

---

## 4. Bounded bipartite graph-edit distance

The **graph-edit distance (GED)** is the minimum cost of a script of node and
edge insertions, deletions, and substitutions that transforms one attributed
graph into another. Exact GED requires a global correspondence search and is
practical only for small graphs. Riesen and Bunke instead reduce local
node-star correspondences to a linear assignment problem, obtaining a fast
suboptimal edit script [[2]](#references). `libcpg` follows that bipartite
strategy, but evaluates the chosen correspondence as a concrete edit script so
the returned value is a genuine **upper bound** on exact GED.

Let $`G_1=(V_1,E_1)`$ and $`G_2=(V_2,E_2)`$. The default edit costs are
$`c_s=1`$ for substituting unequal `NodeKindTag` values, $`c_v=1`$ for a node
insertion or deletion, and $`c_e=0.5`$ for a typed directed-edge insertion or
deletion. Parallel edges count with multiplicity; self-loops and edge kinds
are preserved.

### Star-cost assignment

Stable `NodeId` order fixes rows and columns. For a source node $`u`$ and
target node $`v`$, the substitution entry is

```math
C(u,v) = c_s [\operatorname{tag}(u) \ne \operatorname{tag}(v)]
       + \frac{c_e}{2}\left|\deg(u)-\deg(v)\right|,
```

where $`[P]`$ is $`1`$ when predicate $`P`$ holds and $`0`$ otherwise, and
$`\deg`$ is total incoming plus outgoing degree. Dedicated dummy rows and
columns encode deletion and insertion:

```math
C(u,\epsilon)=c_v+\frac{c_e}{2}\deg(u), \qquad
C(\epsilon,v)=c_v+\frac{c_e}{2}\deg(v).
```

The resulting square matrix has side $`|V_1|+|V_2|`$. The deterministic
primal-dual Hungarian algorithm minimizes its assignment cost [[3]](#references).
Ascending column scans and strict improvements fix ties to the smallest
available column.

### From a local assignment to a global bound

The star objective is only a correspondence heuristic: it sees degree
differences, not the complete edge alignment. `libcpg` therefore discards that
objective value after assignment and prices the induced concrete script:

```text
Algorithm GED-Upper-Bound(G1, G2, options):
  validate that every edit cost is finite and nonnegative
  if either graph exceeds the effective node budget:
    return the explicitly flagged compatibility surrogate
  for each orientation (G1 → G2 and G2 → G1):
    order nodes by stable NodeId
    build the square node/substitution/dummy cost matrix
    mapping ← deterministic Hungarian assignment(matrix)
    node_cost ← unmatched nodes + unequal mapped NodeKindTag values
    edge_cost ← multiset difference of mapped typed directed edges
    script_cost ← node_cost + edge_cost
  return min(forward script, reverse script, delete-all/insert-all script)
```

Every retained candidate is an executable edit script, so if $`d^*`$ is exact
GED and $`U`$ is the returned uncapped value, then

```math
d^*(G_1,G_2) \le U(G_1,G_2) \le
U_0=(|V_1|+|V_2|)c_v+(|E_1|+|E_2|)c_e.
```

Running both orientations and retaining the cheaper script makes the result
symmetric. Division by the delete-all/insert-all cost $`U_0`$ gives
`GedBound::normalized` in $`[0,1]`$; when $`U_0=0`$, the normalized bound is
defined as $`0`$. `SimilarityMetric::GraphEdit` reports

```math
\operatorname{sim}_{\mathrm{GE}}(G_1,G_2)=1-\frac{U(G_1,G_2)}{U_0}.
```

![End-to-end GED flow from validated CPGs through the budget decision, star-cost matrix, deterministic Hungarian assignment, concrete edit-script evaluation, symmetric minimum, and normalized result.](../diagrams/ged-pipeline.svg)

*Figure — the bounded bipartite GED pipeline. Source: [`diagrams/ged-pipeline.puml`](../diagrams/ged-pipeline.puml).*

### Budget and compatibility fallback

Hungarian assignment is cubic and its matrix is quadratic. `GedOptions` admits
at most `min(max_nodes, MAX_GED_NODES)` nodes per input, where
`MAX_GED_NODES = 512`. Beyond that boundary, `ged_upper_bound` sets
`GedBound::capped = true` and returns the former size/Jaccard surrogate. A
capped value is deliberately documented as a compatibility distance, **not a
proven GED upper bound**.

`GraphSimilarity` preserves its `structural_weight` and `label_weight`
builders only for this capped fallback. Within the GED budget, those legacy
weights have no effect; callers tune real edit semantics with `GedOptions` and
call `ged_upper_bound` directly.

The admitted complexity, with $`N=|V_1|+|V_2|`$, is $`O(N^3+|E_1|+|E_2|)`$
time and $`O(N^2+|E_1|+|E_2|)`$ auxiliary space. The fallback is linear in
the input size.

---

## 5. Choosing a metric

| Metric | Signal it reads | Cost | Best for |
|---|---|---|---|
| `Jaccard` (default) | multiset of node kinds | $`O(V)`$ | quick "same mix of constructs?" triage |
| `Cosine` | 9‑D graph feature vector | $`O(V + E)`$ | scale‑robust clustering by overall composition |
| `WeisfeilerLehman` | 3‑round neighbourhood label histograms | $`O(3\,(V + E))`$ | structure‑aware comparison; distinguishing look‑alikes |
| `GraphEdit` | typed concrete edit script selected by bipartite assignment | cubic within 512 nodes; linear flagged fallback | interpretable structural change cost |

A practical progression: filter cheaply with `Jaccard` or `Cosine`, then confirm survivors with `WeisfeilerLehman` when local structure matters. When you need a *precise* embedding rather than a score, drop back to exact [VF2 matching](05-subgraph-isomorphism-vf2.md); when you want a *learned* notion of similarity trained to a task, use the [GNN embeddings](09-graph-neural-networks.md) and compare them with the same cosine formula of Section 2 (see [`components/gnn/embeddings.md`](../components/gnn/embeddings.md)).

### A worked comparison

Two functions that both consist of a loop, a comparison, and an assignment but wire them differently will score:

- **Jaccard ≈ 1** — identical node‑kind bags;
- **Cosine ≈ 1** — near‑identical feature vectors (same counts and ratios);
- **WL < 1** — the differing edges give different refined‑label histograms;
- **GraphEdit < 1** — the concrete script charges for the typed edges that must
  be removed and inserted.

That spread is the whole point: each metric is a different lens. The runnable
GED API is in the [component guide](../components/patterns/ged-analysis.md),
and the four-metric facade is in
[`components/patterns/vf2-matching.md`](../components/patterns/vf2-matching.md).

---

## References

1. Shervashidze, N., et al. (2011). *Weisfeiler-Lehman Graph Kernels.* JMLR 12. <https://jmlr.org/papers/v12/shervashidze11a.html> (no DOI). Orig.: Weisfeiler & Leman (1968).
2. Riesen, K., Bunke, H. (2009). *Approximate Graph Edit Distance Computation by Means of Bipartite Graph Matching.* Image and Vision Computing 27(7), 950–959. DOI: [10.1016/j.imavis.2008.04.004](https://doi.org/10.1016/j.imavis.2008.04.004)
3. Kuhn, H. W. (1955). *The Hungarian Method for the Assignment Problem.* Naval Research Logistics Quarterly 2(1–2), 83–97. DOI: [10.1002/nav.3800020109](https://doi.org/10.1002/nav.3800020109)
