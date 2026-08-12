# Graph cohesion and structural robustness

Graph cohesion is not one scalar. Vertex persistence, triangle-supported
edges, resistance to single failures, weighted separation, endpoint mixing,
and response to a removal sequence expose different structural scales. This
chapter relates those definitions and explains why libcpg keeps them as a
family rather than collapsing them into one “health score.” The concrete API
is in [cohesion and structural robustness](../components/graph/cohesion-analysis.md).

## One graph, several scales

Let $`G=(V,E)`$ be the simple undirected topology derived from a projection.
The analyses move from local to global:

| Scale | Object | Constraint |
|---|---|---|
| vertex | k-core | every retained vertex has enough retained neighbors |
| edge | k-truss | every retained edge participates in enough retained triangles |
| single failure | articulation/bridge | deleting one object increases component count |
| robust region | two-edge-connected component | no one retained edge separates the region |
| global weighted seam | minimum cut | total crossing weight is smallest |
| graph-wide mixing | assortativity | endpoint degrees are correlated |
| dynamic experiment | attack trace | largest component evolves under a chosen order |

These views complement each other. A tree can have important high-degree hubs
but no 3-truss. A clique has high core and truss values but can be joined to
another clique by one bridge. A graph can have no bridge yet possess a small
multi-edge weighted cut.

## Nested vertex cohesion: k-cores

The $`k`$-core $`G_k`$ is produced by repeatedly removing vertices whose
current degree is below $`k`$. The result is unique regardless of valid removal
order, and the cores are nested:

```math
G_{k+1}\subseteq G_k.
```

This nesting makes coreness a persistence measure. A vertex with
$`c(v)=k`$ survives every threshold through $`k`$ but not $`k+1`$. It says
nothing about triangles: a regular bipartite graph can have high coreness and
zero triangle support.

The bin-sort decomposition computes all thresholds in one pass. Its central
invariant is that a processed vertex's current degree is final, while an
unprocessed neighbor is decremented only when its degree is larger. This is
why the final mutable degree array equals the core-number array.

## Nested edge cohesion: k-trusses

For a subgraph $`H`$, define edge support

```math
\operatorname{supp}_H(u,v)=|N_H(u)\cap N_H(v)|.
```

A $`k`$-truss requires

```math
\operatorname{supp}_H(e)\ge k-2
```

for every retained edge $`e`$. The subtraction by two makes every edge a
2-truss edge, every triangle edge a 3-truss edge, and every edge of a complete
graph on $`q`$ vertices a $`q`$-truss edge.

Support must be recomputed relative to the surviving subgraph. Removing one
edge destroys every triangle containing it, which can make companion edges
fall below the same threshold. A decomposition therefore uses fixed-point
peeling, not a one-time triangle count. The trusses are nested just like cores:

```math
T_{k+1}\subseteq T_k.
```

K-truss is stricter than k-core because it requires redundant three-cycles.
That is useful for collaboration or dependency clusters, but it deliberately
does not call a tree-shaped pipeline cohesive.

## Low links as certificates of single failure

Depth-first search (DFS) gives every vertex a discovery time $`d(v)`$. Its
low-link value $`\ell(v)`$ is the smallest discovery time reachable by moving
down zero or more DFS-tree edges and then across at most one back edge.

Consider tree edge $`(v,w)`$. If $`\ell(w)>d(v)`$, the subtree rooted at
$`w`$ has no route to $`v`$ or an ancestor except that tree edge; $`(v,w)`$ is
a bridge. If $`v`$ is not a root and $`\ell(w)\ge d(v)`$, the subtree cannot
escape above $`v`$ without passing through $`v`$; $`v`$ is an articulation
vertex. A root needs at least two DFS children because separate root subtrees
cannot have a cross edge without one becoming part of the other's traversal.

The inequalities are certificates, not heuristics. Deleting all certified
bridges partitions the graph into maximal regions in which every two vertices
have two edge-disjoint paths. These are the two-edge-connected components.

## Local bridges versus global cuts

A bridge is a one-edge unweighted cut. Global minimum cut generalizes this to
weighted sets of edges. For a nontrivial vertex subset $`S`$,

```math
\delta(S)=\{(u,v)\in E\mid u\in S,\ v\notin S\},
```

and

```math
\lambda(G)=\min_{\varnothing\ne S\subsetneq V}
\sum_{e\in\delta(S)}w(e).
```

Stoer–Wagner finds $`\lambda(G)`$ without enumerating $`2^{|V|}`$ subsets.
One phase grows a set by repeatedly selecting the outside supervertex with
maximum accumulated connection to it. The last two selected supervertices
form an $`s`$–$`t`$ cut whose value is the final accumulated connection. The
algorithm records that phase cut, merges $`s`$ and $`t`$, and repeats. The
minimum over phases is a global minimum cut.

Nonnegative weights are essential to the proof. Negative weights would make
“adding connection” cease to be monotone and would give ambiguous architectural
meaning, so libcpg rejects them rather than silently clamps them.

## Assortative and disassortative mixing

Assortativity measures whether edges join vertices with similar degrees. It is
a Pearson correlation over edge endpoints:

- $`r>0`$: high-degree tends to join high-degree and low to low;
- $`r<0`$: high-degree tends to join low-degree; and
- $`r=0`$: no linear endpoint-degree correlation, or no endpoint-degree
  variance under libcpg's convention.

A star is maximally disassortative, $`r=-1`$, because every edge joins its one
high-degree center to a degree-one leaf. A regular graph has no degree variance,
so the coefficient's denominator is zero and libcpg returns zero rather than a
not-a-number value.

Assortativity is global. Two subgraphs with opposite mixing patterns can cancel,
so local core, truss, and cut evidence should accompany it.

## Robustness as an experiment

Static cut structure describes one failure. Attack simulation describes a
specified sequence. After each deletion, the largest connected component
$`L_i`$ summarizes how much topology remains mutually reachable. Normalizing
both axes produces a comparable area under the curve (AUC):

```math
R=\frac{1}{s}\sum_{i=1}^{s}
\frac{L_{i-1}+L_i}{2|V|}.
```

$`R`$ is conditional on the removal order and processed prefix. Degree-first,
betweenness-first, community-bridge-first, and random orders are different
experiments. libcpg accepts the order explicitly so selection policy remains a
reviewable upstream decision rather than hidden inside the kernel.

## Determinism is part of the definition

Several mathematical answers are non-unique:

- equal-degree vertices can be peeled in either order;
- equal-support edges can be peeled in either order;
- DFS forests can choose different roots or neighbors;
- multiple global cuts can share one minimum weight; and
- breadth-first traversal can discover component members in many orders.

libcpg fixes dense node order, canonical edge order, sorted adjacency,
smallest-identifier ties, smallest-member component labels, and canonical cut
sides. This does not change scalar core, truss, cut-weight, or assortativity
answers. It makes equally valid representations byte-stable and therefore
testable, cacheable, and suitable for scientific comparison.

## Program-analysis interpretation

On a module or call projection:

- high coreness suggests participation in a mutually sustained dependency
  region;
- high trussness identifies dependencies embedded in multiple triangular
  collaborations;
- articulation functions or bridge calls identify single topological seams;
- two-edge-connected components identify regions robust to any one call-edge
  deletion;
- a light weighted cut suggests a candidate separation boundary;
- assortativity describes whether hubs connect to hubs or leaves; and
- attack traces compare explicit maintenance or failure scenarios.

These are structural observations. Runtime dispatch, reflection, generated
code, unresolved calls, and projection weighting can change every conclusion.
Use results as evidence for review, never as an automatic authorization or
deletion verdict.

## References

- Seidman, “Network Structure and Minimum Degree,” 1983.
  [DOI: 10.1016/0378-8733(83)90028-X](https://doi.org/10.1016/0378-8733(83)90028-X).
- Batagelj and Zaveršnik, “An O(m) Algorithm for Cores Decomposition of
  Networks,” 2003. [arXiv:cs/0310049](https://arxiv.org/abs/cs/0310049).
- Cohen, “Trusses: Cohesive Subgraphs for Social Network Analysis,” National
  Security Agency Technical Report, 2008. No DOI assigned.
- Hopcroft and Tarjan, “Algorithm 447: Efficient Algorithms for Graph
  Manipulation,” 1973.
  [DOI: 10.1145/362248.362272](https://doi.org/10.1145/362248.362272).
- Tarjan, “Depth-First Search and Linear Graph Algorithms,” 1972.
  [DOI: 10.1137/0201010](https://doi.org/10.1137/0201010).
- Stoer and Wagner, “A Simple Min-Cut Algorithm,” 1997.
  [DOI: 10.1145/263867.263872](https://doi.org/10.1145/263867.263872).
- Newman, “Mixing Patterns in Networks,” 2003.
  [DOI: 10.1103/PhysRevE.67.026126](https://doi.org/10.1103/PhysRevE.67.026126).
- Holme et al., “Attack Vulnerability of Complex Networks,” 2002.
  [DOI: 10.1103/PhysRevE.65.056109](https://doi.org/10.1103/PhysRevE.65.056109).
