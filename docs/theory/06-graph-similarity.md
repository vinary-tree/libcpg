# Graph Similarity

> Theory pillar · file 06. The graded counterpart to exact [subgraph isomorphism](05-subgraph-isomorphism-vf2.md); the learned counterpart is [graph neural networks](09-graph-neural-networks.md).

[Subgraph isomorphism](../GLOSSARY.md#isomorphism--subgraph-isomorphism) ([file 05](05-subgraph-isomorphism-vf2.md)) answers a *yes/no* question: does this exact shape occur? Often the useful question is *how much*: how close are two functions, is this refactoring structurally equivalent to the original, which of a thousand snippets most resembles a known‑vulnerable one? For those, `libcpg` computes a **[similarity metric](../GLOSSARY.md#similarity-metric)** — a score in `` $`[0, 1]`$ `` where `` $`1`$ `` means "as alike as this metric can tell" and `` $`0`$ `` means "nothing in common". This page defines the four metrics `libcpg` ships, the mathematics behind each, and when to reach for which.

All of this lives in the always‑on `pattern` module, in the `GraphSimilarity` calculator.

```rust
use libcpg::pattern::{GraphSimilarity, SimilarityMetric};

let sim = GraphSimilarity::new()                       // metric = Jaccard by default
    .with_metric(SimilarityMetric::WeisfeilerLehman)   // pick a metric
    .with_structural_weight(0.7)                        // used only by GraphEdit (Section 5)
    .with_label_weight(0.3);

let score: f64 = sim.similarity(&graph_a, &graph_b);   // in [0, 1]
```

`SimilarityMetric` selects the strategy; `similarity(&g1, &g2)` dispatches to it and returns the score. The default is `Jaccard`.

![Four similarity metrics side by side — Jaccard multiset overlap, cosine of feature vectors, Weisfeiler‑Lehman label histograms, and approximate graph‑edit distance — each mapping a pair of graphs to a score in [0,1].](../diagrams/similarity-metrics.svg)

*Figure — the four `SimilarityMetric` strategies and the graph signal each one consumes. Source: [`diagrams/similarity-metrics.puml`](../diagrams/similarity-metrics.puml).*

---

## 1. Jaccard similarity (the default)

The [Jaccard index](../GLOSSARY.md#jaccard-similarity) is the size of the intersection over the size of the union of two sets:

```math
J(A, B) = \frac{|A \cap B|}{|A \cup B|}.
```

`libcpg` applies it not to raw sets but to the **multiset of node kinds** of each graph. Every node contributes its [`NodeKindTag`](../GLOSSARY.md#node-kind--edge-kind); let `` $`a_k`$ `` and `` $`b_k`$ `` be how many nodes of kind `` $`k`$ `` graphs `` $`A`$ `` and `` $`B`$ `` contain. The multiset generalisation of Jaccard is

```math
J(A, B) = \frac{\sum_k \min(a_k, b_k)}{\sum_k \max(a_k, b_k)},
```

which is exactly what the code computes: the intersection sums `` $`\min(a_k,b_k)`$ `` over the kinds present, and the union sums `` $`\max(a_k,b_k)`$ `` (adding the counts of kinds that appear only in `` $`B`$ ``). Two empty graphs are defined to be identical (`` $`J = 1`$ ``, avoiding `` $`0/0`$ ``).

**What it captures and misses.** Jaccard on node‑kind multisets is a *bag‑of‑constructs* measure: it asks whether two programs are built from the same mix of ifs, calls, and assignments, in similar proportions. It is fast (`` $`O(|V_A| + |V_B|)`$ ``) and permutation‑invariant, but it is **blind to wiring** — two graphs with identical node counts but completely different edges score `` $`1`$ ``. That is a feature for a cheap first pass and a limitation the other metrics exist to address.

---

## 2. Cosine similarity of feature vectors

[Cosine similarity](../GLOSSARY.md#cosine-similarity) measures the angle between two vectors, ignoring their magnitudes:

```math
\cos(u, v) = \frac{u \cdot v}{\lVert u \rVert \, \lVert v \rVert} \in [-1, 1].
```

The `Cosine` metric summarises each *whole graph* as a fixed **9‑dimensional feature vector** and compares the two vectors. The features (each softly normalised toward `` $`[0,1]`$ ``) are:

| # | Feature | Normalisation |
|---|---|---|
| 1 | node count | `` $`\min(N/1000,\,1)`$ `` |
| 2 | edge count | `` $`\min(E/2000,\,1)`$ `` |
| 3 | AST‑edge ratio | AST edges / total edges |
| 4 | CFG‑edge ratio | CFG edges / total edges |
| 5 | DFG‑edge ratio | DFG edges / total edges |
| 6 | function count | `` $`\min(\#\text{fns}/100,\,1)`$ `` |
| 7 | class count | `` $`\min(\#\text{classes}/50,\,1)`$ `` |
| 8 | AST depth | `` $`\min(\text{depth}/50,\,1)`$ `` |
| 9 | [cyclomatic complexity](../GLOSSARY.md#cyclomatic-complexity) | `` $`\min(M/100,\,1)`$ `` |

If either vector is all zeros the score is `` $`0`$ ``; otherwise it is the cosine above. Because the vector blends *counts* (features 1–2, 6–7) with *shape ratios* (features 3–5, 8–9), cosine rewards graphs that have a similar **overall composition** — a comparable balance of syntax vs. control vs. data flow, similar depth, similar branch density — even at different absolute sizes. It is coarser than a node‑by‑node comparison but robust to scale, which makes it a good clustering key for "programs of the same character".

---

## 3. The Weisfeiler‑Lehman kernel

Jaccard sees node kinds; cosine sees aggregate ratios; neither sees **local structure**. The [Weisfeiler‑Lehman (WL) kernel](../GLOSSARY.md#weisfeiler-lehman-kernel--label-refinement) does, by *refining* each node's label to encode its growing neighbourhood, then comparing the resulting label distributions. It descends from the 1968 Weisfeiler‑Leman graph‑canonisation heuristic and was cast as a scalable graph kernel by Shervashidze et al. [[1]](#references). `libcpg` runs **3 refinement iterations**.

### Label refinement

Start every node with a label equal to (a hash of) its node‑kind tag. Then repeatedly rewrite each node's label to a hash of *its own label together with the sorted multiset of its neighbours' labels*:

```math
\ell^{(i)}(v) = \operatorname{hash}\Big(\ell^{(i-1)}(v),\ \operatorname{sort}\big(\{\, \ell^{(i-1)}(u) : u \in \mathcal{N}(v) \,\}\big)\Big).
```

Sorting makes the label **order‑independent** (a canonical fingerprint of the neighbourhood); hashing compresses it back to a single token so the next round is cheap. After `` $`i`$ `` rounds, a node's label is a fingerprint of its entire `` $`i`$ ``‑hop neighbourhood — so two nodes share a label at round `` $`i`$ `` only if their `` $`i`$ ``‑hop surroundings are structurally identical.

In `libcpg` the neighbourhood `` $`\mathcal N(v)`$ `` is **undirected**: it pools the labels reachable along both outgoing and incoming edges, across all overlays. As literate pseudocode:

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

### Comparing two histograms

Two graphs are then scored by the **cosine of their label histograms**. Treat each distinct label as a coordinate; a label present in only one graph contributes `` $`0`$ `` to the dot product:

```math
\operatorname{sim}_{\text{WL}}(A, B) = \frac{\sum_\ell c^A_\ell\, c^B_\ell}{\sqrt{\sum_\ell (c^A_\ell)^2}\ \sqrt{\sum_\ell (c^B_\ell)^2}},
```

where `` $`c^A_\ell`$ `` is the count of label `` $`\ell`$ `` in graph `` $`A`$ ``. Because a shared label certifies a shared local structure, WL is the most **structure‑aware** of the four metrics: it distinguishes graphs that Jaccard and cosine would call identical, at the cost of the three refinement passes.

![Weisfeiler‑Lehman label refinement: node labels start as kind tags and, over successive rounds, absorb sorted neighbour labels, producing a histogram used as a graph fingerprint.](../diagrams/wl-kernel.svg)

*Figure — one graph's WL refinement over iterations 0→3 and the resulting label histogram fingerprint. Source: [`diagrams/wl-kernel.dot`](../diagrams/wl-kernel.dot).*

---

## 4. Approximate graph‑edit distance

The [graph‑edit distance](../GLOSSARY.md#graph-edit-distance) is the minimum number of node/edge insertions, deletions, and relabellings that turn one graph into the other — an intuitive dissimilarity, but **NP‑hard** to compute exactly. `libcpg`'s `GraphEdit` metric therefore uses a cheap surrogate built from size differences and node‑kind overlap.

Let `` $`N_1, N_2`$ `` be node counts and `` $`E_1, E_2`$ `` edge counts. Define per‑axis similarities from the normalised differences

```math
\text{node\_sim} = 1 - \frac{|N_1 - N_2|}{\max(N_1, N_2)}, \qquad
\text{edge\_sim} = 1 - \frac{|E_1 - E_2|}{\max(E_1, E_2, 1)},
```

and combine a **structural** term (a 60/40 blend of node‑ and edge‑size agreement) with a **label** term (the Jaccard node‑kind score of Section 1):

```math
\operatorname{sim}_{\text{GE}}(A,B) = w_{\text{struct}} \big(0.6\,\text{node\_sim} + 0.4\,\text{edge\_sim}\big) + w_{\text{label}}\, J(A, B).
```

This is a fast approximation, not a true edit distance: it reads graph *size* and *composition*, not the specific rewrites that would align them. It is useful as a forgiving "roughly the same magnitude and make‑up" score.

---

## 5. Structural vs. label weighting

`GraphSimilarity` carries two weights, `structural_weight` (default `` $`0.7`$ ``) and `label_weight` (default `` $`0.3`$ ``). It is important to be precise about their reach: **only the `GraphEdit` metric consumes them** — they are the `` $`w_{\text{struct}}`$ `` and `` $`w_{\text{label}}`$ `` of the equation in Section 4. `Jaccard`, `Cosine`, and `WeisfeilerLehman` are single‑signal metrics and ignore the weights entirely. So the `0.7 / 0.3` split expresses one design choice — *when blending, trust graph structure roughly twice as much as the node‑kind bag* — and it takes effect only when you select `SimilarityMetric::GraphEdit`.

---

## 6. Choosing a metric

| Metric | Signal it reads | Cost | Best for |
|---|---|---|---|
| `Jaccard` (default) | multiset of node kinds | `` $`O(V)`$ `` | quick "same mix of constructs?" triage |
| `Cosine` | 9‑D graph feature vector | `` $`O(V + E)`$ `` | scale‑robust clustering by overall composition |
| `WeisfeilerLehman` | 3‑round neighbourhood label histograms | `` $`O(3\,(V + E))`$ `` | structure‑aware comparison; distinguishing look‑alikes |
| `GraphEdit` | size deltas + Jaccard, weighted | `` $`O(V + E)`$ `` | forgiving "roughly the same graph" score |

A practical progression: filter cheaply with `Jaccard` or `Cosine`, then confirm survivors with `WeisfeilerLehman` when local structure matters. When you need a *precise* embedding rather than a score, drop back to exact [VF2 matching](05-subgraph-isomorphism-vf2.md); when you want a *learned* notion of similarity trained to a task, use the [GNN embeddings](09-graph-neural-networks.md) and compare them with the same cosine formula of Section 2 (see [`components/gnn/embeddings.md`](../components/gnn/embeddings.md)).

### A worked comparison

Two functions that both consist of a loop, a comparison, and an assignment but wire them differently will score:

- **Jaccard ≈ 1** — identical node‑kind bags;
- **Cosine ≈ 1** — near‑identical feature vectors (same counts and ratios);
- **WL < 1** — the differing edges give different refined‑label histograms.

That spread is the whole point: each metric is a different lens, and the WL disagreement is precisely the structural information the cheaper metrics discard. The runnable API and further examples live in [`components/patterns/vf2-matching.md`](../components/patterns/vf2-matching.md).

---

## References

1. Shervashidze, N., et al. (2011). *Weisfeiler-Lehman Graph Kernels.* JMLR 12. <https://jmlr.org/papers/v12/shervashidze11a.html> (no DOI). Orig.: Weisfeiler & Leman (1968).
