# Theory 17 — Directed motif census

Large program graphs can differ in globally visible ways while sharing the
same recurring local wiring patterns. A **network motif** is one such small
induced topology, counted without regard to the identities of its vertices.
Motif counts can describe local call, control, or dependence structure; they
are evidence about architecture, not proof of source-level intent.

## Dyads and the Davis–Leinhardt taxonomy

For two distinct vertices, a directed **dyad** is:

- **mutual** when both arc directions are present;
- **asymmetric** when exactly one direction is present; or
- **null** when neither direction is present.

An unordered set of three vertices contains exactly three dyads. If their
counts are `$M$`, `$A$`, and `$N$`, then

```math
M + A + N = 3.
```

The Davis–Leinhardt triad name concatenates these counts. A suffix distinguishes
non-isomorphic orientations having the same counts: `D` points **down** from a
distinguished source, `U` points **up** toward a distinguished sink, `C` is
cyclic, and `T` is transitive. The sixteen isomorphism classes are

```text
003  012  102  021D 021U 021C 111D 111U
030T 030C 201  120D 120U 120C 210  300
```

The distinction is structural. For example, `111D` has an asymmetric arc from
the third vertex into a mutual dyad, whereas `111U` reverses that asymmetric
arc. Similarly, `120D`, `120U`, and `120C` are three different orientations
of one mutual and two asymmetric dyads.

## Six-bit tricodes

Choose dense vertices `$a < b < c$` and let `$I(x,y)$` be one when arc
`$x \rightarrow y$` exists and zero otherwise. `libcpg` forms the tricode

```math
t = I(a,b)2^0 + I(b,a)2^1 + I(a,c)2^2 + I(c,a)2^3
  + I(b,c)2^4 + I(c,b)2^5.
```

There are `$2^6 = 64$` labeled tricodes but only sixteen unlabeled isomorphism
classes. A complete 64-entry table maps each tricode to its class. Vertex
renaming changes the code but not the mapped class.

## Census conservation

Every unordered vertex triple has exactly one tricode and exactly one class.
Consequently, for `$n$` vertices and class counts `$c_i$`,

```math
\sum_{i=0}^{15} c_i = {n \choose 3} = \frac{n(n-1)(n-2)}{6}.
```

The implementation enumerates triples directly. That gives an exact,
auditable conservation law and avoids the subtle boundary accounting required
by sparse subquadratic census algorithms. The configured 512-vertex ceiling
keeps this cubic enumeration bounded.

## Four-node graphlet predicates

The same induced-adjacency model defines two selected four-node graphlets.
For a subset `$S$` of four vertices:

1. it is a complete bidirected clique when, for all distinct `$u,v \in S$`,
   both `$u \rightarrow v$` and `$v \rightarrow u$` exist;
2. a vertex `$c \in S$` is an out-star center when it points to all other
   vertices and those three leaves have no arcs among themselves.

The out-star predicate does not constrain reverse spoke arcs
`$\ell \rightarrow c$`. This is a deliberate compatibility property of the
ported topology contract, not the conventional strictly induced directed-star
definition. Consumers requiring the stricter definition should post-filter or
define a separate graphlet family.

## Architecture interpretation

On a call graph, `030T` can indicate a direct caller-to-callee shortcut across
a two-step call chain, while `030C` indicates a local recursion cycle. `021D`
can expose a fan-out site and `021U` a convergence site. These interpretations
remain advisory:

- projections can omit unresolved calls or coalesce parallel evidence;
- one topology can arise from unrelated source-level causes;
- motif frequency alone does not establish design quality or intent; and
- comparisons should use the same projection kind and filtering policy.

## Computational model

Dense row bitsets answer an adjacency query in constant time. Enumerating all
triples costs `O(n³)`; enumerating all quadruples and testing a fixed number of
arcs costs `O(n⁴)`. The adjacency storage costs
`O(n ceil(n / 64))` words. This favors deterministic, bounded analyses of CPG
projections. For much larger sparse networks, Batagelj and Mrvar's sparse triad
census is the appropriate conceptual alternative.

The [component guide](../components/graph/motif-analysis.md) documents the API
and exact index meanings. [ADR 0022](../design/0022-deterministic-motif-census.md)
records why the bounded exhaustive algorithm is used, and the [validation
record](../scientific/17-motif-census-validation.md) separates tested claims
from architectural interpretation.

## References

- Holland, P. W., and Leinhardt, S. (1970). *A Method for Detecting Structure
  in Sociometric Data*. American Journal of Sociology 76(3), 411–432.
  [DOI 10.1086/224954](https://doi.org/10.1086/224954).
- Batagelj, V., and Mrvar, A. (2001). *A subquadratic triad census algorithm
  for large sparse networks with small maximum degree*. Social Networks 23(3),
  237–243.
  [DOI 10.1016/S0378-8733(01)00035-1](https://doi.org/10.1016/S0378-8733(01)00035-1).
- Milo, R. et al. (2002). *Network Motifs: Simple Building Blocks of Complex
  Networks*. Science 298(5594), 824–827.
  [DOI 10.1126/science.298.5594.824](https://doi.org/10.1126/science.298.5594.824).
