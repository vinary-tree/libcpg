# Centrality and structural importance

Centrality measures turn graph position into a score, but they do not define
one universal meaning of “important.” This chapter develops the intuitions
behind libcpg's centrality family and explains how those abstractions apply to
program graphs.

The concrete API and option contracts are documented in
[projection centrality analysis](../components/graph/centrality-analysis.md).

## A relation comes before a score

A centrality computation starts with a graph $`G=(V,E)`$. In a Code Property
Graph (CPG), the choice of $`E`$ is semantic:

- a call-graph edge means one function may call another;
- a control-flow edge means execution may continue between program points;
- a data-flow edge means a value may reach another node;
- a module edge means one architectural unit depends on another.

The same formula answers a different question on each relation. PageRank over
a call graph may surface widely called functions; PageRank over a reversed
module-dependency graph may surface foundational dependencies. A score cannot
repair missing call targets or an incorrectly selected overlay.

## Three intuitions

The family can be organized around three intuitions.

### Endorsement and recursive prestige

PageRank, eigenvector centrality, Katz centrality, and Hyperlink-Induced Topic
Search (HITS) use the scores of neighbors to define a vertex's score.

PageRank is a conservative random walk. A vertex shares its current mass among
outgoing neighbors; teleportation and dangling-node redistribution preserve a
probability distribution. Personalized PageRank changes only the restart
distribution, making relevance relative to selected seeds. Reverse PageRank
changes only edge orientation.

Eigenvector centrality uses the dominant eigenvector of a symmetric adjacency
matrix. Katz centrality adds a baseline and attenuates walks by length, so every
vertex can have a positive score even when it is not in the dominant spectral
component. HITS separates two roles: hubs point to authorities, while
authorities are pointed to by hubs.

### Mediation and unavoidable structure

Betweenness counts how much shortest-path traffic a vertex or edge mediates.
This makes it sensitive to bridges between regions. It is not a flow-capacity
model: all shortest paths divide one unit of credit, edge weights are ignored,
and longer alternative paths receive no credit.

Dominance asks a stricter, rooted question. A vertex $`a`$ dominates $`b`$ when
every directed path from the root to $`b`$ contains $`a`$. Betweenness can be
high because many shortest paths cross a vertex; dominance requires every
path, regardless of length, to cross it. libcpg exposes the two analyses over
the same projection substrate without pretending they are interchangeable.

### Distance and neighborhood redundancy

Closeness rewards a small total shortest-path distance. Its
Wasserman–Faust factor discounts vertices that reach only a small component.
Harmonic centrality sums reciprocal distances, so unreachable pairs
contribute zero naturally.

Burt's constraint is local rather than all-pairs. If a vertex's contacts also
reach each other, its neighborhood is redundant and its constraint is high. A
vertex connecting otherwise separated contacts can have lower constraint and
act as a broker across structural holes.

## Worked topology: an outward star

Consider a directed outward star with center 0 and leaves 1, 2, and 3:

```text
             1
             ^
             |
       2 <-- 0 --> 3
```

- HITS gives vertex 0 hub score 1 and equal authority score
  $`1/\sqrt{3}`$ to each leaf.
- Undirected eigenvector centrality gives the center score $`1/\sqrt{2}`$ and
  every leaf score $`1/\sqrt{6}`$.
- Undirected closeness gives the center the shortest mean distance.
- Burt constraint gives the center $`3(1/3)^2=1/3`$ and each leaf 1 because a
  leaf invests all normalized relation weight in one contact.
- Directed vertex betweenness is zero: no directed path continues from a leaf
  through the center to another leaf.

The example shows why reporting “the centrality” without its measure and edge
orientation is incomplete.

## Fixed points, convergence, and periodicity

An iterative score starts from a vector $`x_0`$ and repeatedly applies a map
$`F`$ until the maximum component change is below a tolerance:

```math
\lVert F(x_t)-x_t\rVert_\infty < \varepsilon.
```

PageRank's teleportation makes the transition well behaved for ordinary
damping values. Spectral iteration has a different concern: on a bipartite
graph, the largest positive and negative adjacency eigenvalues can have equal
magnitude, causing plain power iteration to oscillate. libcpg iterates on
$`A+I`$ for eigenvector centrality. If $`Ax=\lambda x`$, then

```math
(A+I)x=(\lambda+1)x,
```

so eigenvectors are unchanged while the problematic sign symmetry is shifted.

Katz centrality uses a sufficient, inspectable convergence condition. If
$`\lVert A\rVert_\infty`$ is the maximum weighted row sum, libcpg requires

```math
\alpha\lVert A\rVert_\infty < 1.
```

This bound can reject an attenuation that a precise spectral-radius test would
accept, but it never requires an unbounded eigenvalue computation merely to
validate an option.

## Deterministic numerical reduction

Graph algorithms are mathematically order-independent but floating-point
addition is not associative. Summing contributions in hash-map iteration order
can change low bits between processes or versions. libcpg uses dense vectors,
sorted CSR adjacency, total-order sorting for contribution collections, and
canonical traversal order for dependency accumulations. Exact repeats are
therefore bit-identical for one projection.

An isomorphic stable-identifier relabeling can still change algorithm traversal
order in dependency-based computations such as Brandes. The validation
contract requires numerical equality within a stated tolerance under such a
relabeling, while exact equality is required for repeated calls on the same
projection.

Brandes also faces a magnitude problem: the number of shortest paths can grow
exponentially with breadth-first depth. Only ratios between predecessor path
weights are needed. libcpg divides all weights in one breadth-first layer by
the same positive maximum; this changes their common scale but preserves every
ratio used by dependency accumulation, preventing otherwise-valid deep graphs
from producing infinite path counts.

## Bounded computation as semantics

Iteration and traversal limits are part of the public meaning, not incidental
implementation settings. An iterative result records completed rounds and a
convergence bit. All-sources algorithms reject work that exceeds the caller's
budget before returning a partial score vector. This distinction prevents a
partial ranking from being mistaken for a completed one.

Brandes source accumulation is deliberately exposed. Independent workers can
compute disjoint, sorted source partitions, but floating partial vectors must
be combined in a fixed partition order to retain reproducibility.

## Interpretation boundaries

Centrality is most useful as one feature in a larger analysis:

- combine centrality with churn and defects to prioritize review;
- compare PageRank and reverse PageRank to distinguish dependency consumers
  from foundations;
- investigate high edge betweenness as a possible architectural seam;
- use dominators for mandatory control chokepoints rather than substituting a
  high betweenness threshold;
- treat low Burt constraint as brokerage evidence, not proof of desirable
  modularity.

Centrality does not prove runtime frequency, ownership, semantic correctness,
security, or causal importance. Those claims require corresponding dynamic or
semantic evidence.

## References

1. Brin, S., and Page, L. (1998). *The Anatomy of a Large-Scale Hypertextual
   Web Search Engine.*
   [DOI 10.1016/S0169-7552(98)00110-X](https://doi.org/10.1016/S0169-7552(98)00110-X).
2. Brandes, U. (2001). *A Faster Algorithm for Betweenness Centrality.*
   [DOI 10.1080/0022250X.2001.9990249](https://doi.org/10.1080/0022250X.2001.9990249).
3. Bonacich, P. (1987). *Power and Centrality: A Family of Measures.*
   [DOI 10.1086/228631](https://doi.org/10.1086/228631).
4. Katz, L. (1953). *A New Status Index Derived from Sociometric Analysis.*
   [DOI 10.1007/BF02289026](https://doi.org/10.1007/BF02289026).
5. Sabidussi, G. (1966). *The Centrality Index of a Graph.*
   [DOI 10.1007/BF02289527](https://doi.org/10.1007/BF02289527).
6. Kleinberg, J. M. (1999). *Authoritative Sources in a Hyperlinked
   Environment.*
   [DOI 10.1145/324133.324140](https://doi.org/10.1145/324133.324140).
7. Burt, R. S. (1992). *Structural Holes: The Social Structure of
   Competition.* Harvard University Press. ISBN 978-0-674-84371-4.
