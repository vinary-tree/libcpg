# Bounded simple-cycle enumeration

This chapter develops the mathematical object returned by
`libcpg::simple_cycles`, explains why strongly connected components are the
right search boundary, and distinguishes the crate's bounded deterministic DFS
from Johnson's complete elementary-circuit algorithm [[1]](#references).

## Directed graphs and elementary cycles

Let $`G = (V, E)`$ be a finite directed graph. A sequence
$`c = (v_0, v_1, \ldots, v_{k-1})`$ is an **elementary directed cycle** when:

```math
1 \le k,\qquad
(v_i, v_{(i+1)\bmod k}) \in E,\qquad
v_i \ne v_j\ \text{for}\ i \ne j.
```

The closing copy of $`v_0`$ is implicit. This definition includes a self-loop
when $`k = 1`$ and a reciprocal pair when $`k = 2`$. It is stricter than a
closed walk, which may repeat vertices, and different from an undirected cycle
basis, which forgets orientation and need not list every simple cycle.

## Equivalence under rotation

A cycle has no distinguished geometric starting point. Define rotation by

```math
\rho_j(c)_i = v_{(i+j)\bmod k}.
```

All $`\rho_j(c)`$ describe the same directed cycle. Because stable `NodeId`
values have a total order and vertices are distinct, each equivalence class has
exactly one rotation whose first vertex is the minimum:

```math
\operatorname{canon}(c)
= \rho_j(c)\quad\text{where}\quad
v_j = \min\{v_0,\ldots,v_{k-1}\}.
```

This canonicalization does not identify reversal. Reversal changes every edge
direction, so it is another directed cycle only if those reverse edges exist.

## Why SCCs are the exact pruning boundary

A strongly connected component (SCC) is a maximal vertex set in which every
vertex reaches every other. Every vertex on a directed cycle reaches its next
vertex around the cycle and eventually reaches all other cycle vertices.
Therefore all vertices of one directed cycle lie in one SCC.

Conversely, a multi-vertex SCC contains at least one directed cycle; a singleton
SCC contains a cycle exactly when it has a self-loop. It follows that searching
only `SccDecomposition::cyclic_components()` is complete for cycle enumeration:
no cycle crosses a condensation edge, because the condensation graph is a
directed acyclic graph.

## Canonical bounded search

For a chosen start vertex $`s`$, the implementation admits only internal path
vertices $`v > s`$. A discovered closing path therefore has $`s`$ as its unique
minimum and cannot be rediscovered from another rotation. Searches run in the
lexicographic product order

```math
(\text{cycle length},\ \text{start vertex},\ \text{successor sequence}).
```

Sorted CSR successors make iterative DFS lexicographic within one start. Thus
stopping after the first cycle beyond the count cap yields a deterministic
prefix of the globally sorted answer, plus that next cycle as a truncation
witness.

The length bound $`L`$ and result bound $`C_{max}`$ define the observable query:

```math
\mathcal{C}_{L}(G)
= \{\operatorname{canon}(c) \mid c\text{ is elementary and } |c|\le L\}.
```

If $`|\mathcal{C}_{L}(G)| \le C_{max}`$, the result is complete. Otherwise the
result retains the first $`C_{max}`$ members under `(length, lexicographic)`
order and returns member $`C_{max}+1`$ as evidence that the suffix exists.

## Relationship to Johnson's algorithm

Johnson's 1975 algorithm enumerates all elementary circuits with a blocked-set
discipline and achieves $`O((V+E)(C+1))`$ time for $`C`$ circuits
[[1]](#references). `libcpg` cites that work as the classical basis and shares
its SCC-centered intuition, but does not claim to implement Johnson's blocked
algorithm. The crate uses a simpler bounded DFS variant because its public
contract prioritizes:

- small, explicit cycle-length and result ceilings;
- canonical stable-identifier ordering;
- immediate first-omission evidence;
- reusable feature-free CSR/SCC infrastructure; and
- explicit stacks for native-stack safety.

This is a deliberate bounded-query design, not a replacement asymptotic result
for unbounded all-cycle enumeration.

## Worst-case growth

In a complete directed graph without self-loops, the number of length-$`k`$
cycles is

```math
\binom{n}{k}(k-1)! = \frac{n!}{k(n-k)!}.
```

Summing this expression shows why even modest dense graphs need a count cap.
SCC pruning makes acyclic regions cheap and the length cap prunes deep paths,
but neither removes the exponential output family. A truthful API must expose
partiality when it stops; silently returning a vector prefix would confuse
valid individual witnesses with a complete structural census.

## References

1. Johnson, D. B. (1975). *Finding All the Elementary Circuits of a Directed
   Graph.* SIAM Journal on Computing 4(1), 77–84. DOI:
   [10.1137/0204007](https://doi.org/10.1137/0204007).
