# ADR-0019: Deterministic bounded cohesion analysis

- **Status:** Accepted
- **Date:** 2026-08-11
- **Decision owners:** libcpg maintainers
- **Scope:** feature-free projection analysis

## Context

pgmcp contains useful graph-cohesion and connectivity kernels, but their
public results use hash maps and sets, several tie decisions depend on hash
iteration, k-core uses a re-bucketing implementation instead of the cited
linear algorithm, k-truss repeatedly searches all supports, and minimum cut
has no library-owned size boundary. The kernels also consume petgraph values
directly rather than libcpg's stable projection seam.

libcpg needs one language-independent family that is deterministic at public
boundaries, validates caller-mutable CSR fields, stays feature-free, preserves
stable `NodeId` alignment, avoids recursive traversal, and rejects work or
numeric states outside explicit limits.

## Decision

libcpg will:

1. expose k-core, k-truss, cut-structure, two-edge-component, global-min-cut,
   assortativity, and attack-simulation functions over `&GraphProjection`;
2. derive one canonical simple undirected topology for every unweighted
   operation by forgetting direction, deduplicating reciprocal/parallel edges,
   and ignoring self-loops;
3. keep minimum-cut weight semantics separate: reciprocal directed weights are
   deterministically summed, loops ignored, and negative, non-finite, or
   overflowing weights rejected;
4. implement Batagelj–Zaversnik k-core peeling with dense `bin`, `position`,
   and element arrays;
5. implement k-truss with canonical edge identifiers, initial triangle
   support, true bin updates, and smaller-neighborhood support maintenance;
6. cap k-truss at 200,000 simple edges before triangle work;
7. implement Hopcroft–Tarjan low links iteratively over dense arrays;
8. derive two-edge-connected components by deleting certified bridges and
   breadth-first traversal in dense order;
9. implement Stoer–Wagner with a 1,024-node cap, smallest-dense-id equal-
   connection ties, checked weight accumulation, and canonical cut-side output;
10. define assortativity as Newman's undirected endpoint-degree coefficient,
    returning zero when variance is absent;
11. accept an explicit distinct `NodeId` removal order for attack simulation,
    cap `max_steps` at 10,000, report intentional prefix truncation, and compute
    area from the graph's actual initial largest component;
12. return only projection-aligned vectors, sorted stable identifiers,
    canonical edges, or canonical component lists; and
13. leave the input projection and source CPG immutable.

## Rationale

One shared topology prevents subtle disagreements: a reciprocal call pair must
not count as two edges for k-core but one edge for bridge detection. Separating
weighted minimum cut avoids interpreting malformed weights in topological
analyses while still preserving a strict numeric evidence boundary where
weights matter.

Dense bin arrays implement the intended peeling algorithms and make equal-bin
initial order deterministic. Iterative low-link traversal removes recursion
depth as an input-controlled resource. Canonical output rules make non-unique
representations reproducible without claiming that one minimum-cut side has
greater semantic importance.

The attack kernel accepts policy rather than inventing it. A caller may rank by
degree, betweenness, architecture priority, or a seeded experiment, but the
result records exactly the supplied sequence.

## Alternatives considered

### Port pgmcp's petgraph and hash-map implementations verbatim

Rejected. It would couple the API to a graph dependency, retain avoidable
iteration-order nondeterminism, and preserve slower peeling behavior that the
campaign explicitly exists to improve.

### Treat directed edges separately

Rejected for this family. Classical core, truss, articulation, bridge,
Stoer–Wagner, and the selected assortativity definitions are undirected.
Directed analogues answer different questions and should have explicit names
rather than silently change these contracts.

### Use recursive DFS for low links

Rejected. A valid adversarial path can be tens of thousands of vertices deep;
recursion would make process-stack capacity an undocumented graph-size limit.

### Materialize every triangle for k-truss

Rejected. A graph below the edge cap can still contain tens of millions of
triangles. On-demand active-triangle discovery bounds storage by vertices and
edges instead of triangle count.

### Clamp negative minimum-cut weights to zero

Rejected. Clamping changes the optimization problem while returning an
apparently authoritative cut. Typed rejection preserves the caller's ability
to correct projection weights.

### Hide an attack-order heuristic in the analysis

Rejected. Different order policies are different experiments. An explicit
order keeps the causal assumption visible and lets other analysis results feed
the experiment without creating a dependency cycle.

## Consequences

### Positive

- All public structures have stable ordering and optional serde support.
- K-core and connectivity remain linear in projection size.
- K-truss avoids an unbounded triangle materialization.
- Deep paths cannot overflow the process stack in low-link analysis.
- Minimum-cut memory and cubic work are bounded before matrix allocation.
- Public integration can compose call, module, CFG, DFG, or external
  projections without feature gates.

### Trade-offs

- Direction is intentionally discarded for this family.
- K-truss still has superlinear worst-case work and therefore needs its edge
  ceiling.
- Stoer–Wagner's dense matrix is appropriate only below its node ceiling.
- Assortativity compresses a graph into one global correlation.
- Attack simulation recomputes connected components after each removal; its
  explicit step ceiling bounds the resulting repeated traversal.

## Validation

Independent tests use naïve threshold peeling for every k-core, fixed-point
triangle peeling for every k-truss, brute vertex and edge removal for
articulation and bridge truth, exhaustive bipartitions for minimum-cut weight,
and a separate breadth-first search for every attack step. Analytic clique,
triangle, bridge, star, weighted-seam, empty, malformed, overflow, cap, serde,
deep-path, public projection, and parsed Rust call-graph cases cover boundaries.
The Criterion target repeats analytic oracles before timing. See
[cohesion validation](../scientific/14-cohesion-validation.md).

## References

- Seidman 1983.
  [DOI: 10.1016/0378-8733(83)90028-X](https://doi.org/10.1016/0378-8733(83)90028-X).
- Batagelj and Zaveršnik 2003.
  [arXiv:cs/0310049](https://arxiv.org/abs/cs/0310049).
- Cohen 2008, National Security Agency Technical Report. No DOI assigned.
- Hopcroft and Tarjan 1973.
  [DOI: 10.1145/362248.362272](https://doi.org/10.1145/362248.362272).
- Tarjan 1972.
  [DOI: 10.1137/0201010](https://doi.org/10.1137/0201010).
- Stoer and Wagner 1997.
  [DOI: 10.1145/263867.263872](https://doi.org/10.1145/263867.263872).
- Newman 2003.
  [DOI: 10.1103/PhysRevE.67.026126](https://doi.org/10.1103/PhysRevE.67.026126).
- Holme et al. 2002.
  [DOI: 10.1103/PhysRevE.65.056109](https://doi.org/10.1103/PhysRevE.65.056109).
