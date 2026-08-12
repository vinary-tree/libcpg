# Community structure and modularity

A community partition is a model of mesoscale graph structure: it lies between
individual vertices and one graph-wide summary. This chapter explains the
mathematical intuition behind libcpg's multi-level Louvain analysis and the
separate connected refinement. The concrete API is documented in
[deterministic community detection](../components/graph/community-detection.md).

## Dense relative to what?

Calling a group “dense” needs a baseline. A group containing high-degree
vertices will contain many internal edges even under random wiring. Modularity
compares observed internal weight with a **configuration-model null model**
that preserves expected weighted degree.

For a weighted undirected graph, let $`m`$ be total edge weight and $`k_i`$ the
weighted degree of vertex $`i`$. The null model assigns expected adjacency

```math
\mathbb{E}[A_{ij}]=\frac{k_i k_j}{2m}.
```

Given community labels $`c_i`$ and resolution $`\gamma`$, generalized
modularity is

```math
Q_\gamma=\frac{1}{2m}\sum_{i,j}
\left(A_{ij}-\gamma\frac{k_i k_j}{2m}\right)
\mathbf{1}[c_i=c_j].
```

Here $`A`$ is the symmetric weighted adjacency matrix and
$`\mathbf{1}[P]`$ is one when proposition $`P`$ is true and zero otherwise.
Grouping the double sum by community yields the numerically cheaper formula
used for final reporting:

```math
Q_\gamma=\sum_C\left[
\frac{L_C}{m}-\gamma\left(\frac{D_C}{2m}\right)^2
\right].
```

$`L_C`$ is observed internal edge weight and $`D_C`$ is total weighted degree
in community $`C`$.

## Resolution as a scale control

The resolution $`\gamma`$ controls the null-model penalty.

- $`\gamma=1`$ is standard modularity.
- Increasing $`\gamma`$ generally favors smaller groups.
- Decreasing $`\gamma`$ generally favors larger groups.
- $`\gamma=0`$ rewards only internal weight and removes the degree-based
  penalty.

This parameter does not eliminate the modularity resolution limit. It changes
the scale at which merging is attractive, and comparisons are meaningful only
when projection, weights, and resolution are held constant.

## Why local moving works

Suppose vertex $`i`$ has been temporarily removed from its current community.
For candidate community $`C`$, let $`k_{i,C}`$ be incident weight from $`i`$
to vertices in $`C`$, and let $`D_C`$ be the candidate's current total degree.
Terms independent of $`C`$ cancel, leaving a candidate score proportional to

```math
k_{i,C}-\gamma\frac{k_iD_C}{2m}.
```

The first term favors observed attachment; the second charges for attachment
expected from degrees alone. Louvain repeatedly chooses the best neighboring
community until a complete pass makes no move. This reaches a local optimum
for the fixed vertex order, not necessarily the global maximum of $`Q`$.

## Why aggregation is essential

Local moving can reach a state where no single vertex should move, even though
two whole groups should merge. Louvain contracts every first-level community
into one vertex:

- edge weights between two groups are summed;
- edges internal to one group become a self-loop; and
- total graph weight is preserved.

Local moving on the aggregate graph can then move whole groups. Repeating
local moving and aggregation produces a hierarchy of progressively coarser
partitions. libcpg's ring-of-cliques regression demonstrates a graph whose
multi-level result strictly coarsens and does not score below the one-level
result.

## Determinism changes the experiment

The original method often randomizes vertex order, and different orders can
reach different local optima. libcpg instead defines a reproducible experiment:

1. stable identifiers determine dense order;
2. compressed-sparse-row neighbors are sorted;
3. candidate communities are scanned in ascending identifier order;
4. exact score ties choose the smallest identifier; and
5. result identifiers are renumbered by smallest member.

The output is therefore repeatable for the same canonical projection. It is
one deterministic local optimum, not a distribution over randomized runs.

## Connectivity is a separate invariant

High modularity does not imply that each induced community is connected.
Repeated Louvain aggregation can retain a label whose original vertices no
longer form one connected subgraph. A downstream architecture view should not
present such a label as one navigable subsystem.

libcpg's connected refinement computes connected components inside every
assigned community. Splitting continues until every output group is internally
connected. This operation can lower $`Q`$: the topology invariant is the goal,
not score maximization.

The refinement is inspired by the defect analysis in Traag, Waltman, and van
Eck, but it is deliberately narrower than full Leiden. Full Leiden interleaves
local moving, probabilistic refinement, and aggregation and proves stronger
$`\gamma`$-connectivity and subset-optimality properties. libcpg claims only
ordinary connectedness for this API.

## Program-graph interpretation

On a module dependency graph, a community is a candidate subsystem with more
internal dependency weight than the null model predicts. On a call graph, it
is a candidate collaboration region. Useful follow-up questions include:

- Does a connected community align with an intended package boundary?
- Are high-betweenness edges the seams between communities?
- Does a strongly connected component force otherwise separate communities
  together?
- Does the partition remain stable under a justified projection change?

Community output remains advisory. It cannot establish ownership, runtime
coupling, semantic cohesion, or an architectural rule without corroborating
evidence.

## References

- Blondel et al., “Fast unfolding of communities in large networks,” 2008.
  [DOI: 10.1088/1742-5468/2008/10/P10008](https://doi.org/10.1088/1742-5468/2008/10/P10008).
- Traag, Waltman, and van Eck, “From Louvain to Leiden: guaranteeing
  well-connected communities,” 2019.
  [DOI: 10.1038/s41598-019-41695-z](https://doi.org/10.1038/s41598-019-41695-z).
