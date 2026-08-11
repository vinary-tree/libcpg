# ADR-0017: Deterministic bounded projection centrality

- **Status:** Accepted
- **Date:** 2026-08-11
- **Decision owners:** libcpg maintainers
- **Scope:** feature-free graph analysis

## Context

The analysis-expansion campaign requires pgmcp's centrality algorithms in
libcpg so both projects can share one graph-analysis substrate. The source
implementations use `HashMap<NodeIndex, f64>` and build per-algorithm adjacency
maps. Those structures are appropriate prototypes but do not satisfy libcpg's
public determinism, stable-identifier, malformed-input, feature-free, and
resource-cap contracts.

The family mixes directed and undirected semantics, fixed-point and
all-sources traversal kernels, scalar node and canonical edge results, and
weighted and unweighted measures. A single opaque `centrality(graph, name)`
function would hide material differences and make option validation ambiguous.

## Decision

libcpg will:

1. expose named functions for PageRank, reverse PageRank, personalized
   PageRank, vertex and edge betweenness, eigenvector, Katz, closeness,
   harmonic, HITS, and Burt constraint;
2. return node vectors aligned with sorted `GraphProjection::nodes`, with an
   explicit `CentralityKind`, iteration/source count, and convergence state;
3. expose canonical sorted edge-betweenness pairs separately and coupled HITS
   hub/authority results together;
4. traverse dense vertices and sorted forward/reverse CSR arrays, without
   kernel-owned thread pools;
5. sort floating contribution collections before reduction and use canonical
   dense traversal order for dependency accumulation;
6. clamp iterative requests to `MAX_CENTRALITY_ITERATIONS = 10_000`;
7. require explicit work budgets for Brandes, distance, edge-betweenness, and
   Burt kernels, with a default ceiling of two billion modeled operations;
8. expose `accumulate_for_sources` as the deterministic serial Brandes kernel
   for ordered external parallelism;
9. validate the complete public projection contract before indexed traversal;
10. retain directed semantics for PageRank, vertex Brandes, HITS, and
    dominators, while using documented undirected projections for the other
    measures;
11. clamp negative spectral/Burt edge weights to zero and reject non-finite
    matrix weights, preserving finite nonnegative-matrix semantics;
12. rescale shortest-path weights by one common factor per breadth-first layer,
    preserving Brandes predecessor ratios without path-count overflow;
13. reject personalized restart weights whose grouped or total sum overflows;
14. use shifted power iteration on $`A+I`$ for eigenvector centrality; and
15. reject Katz attenuation outside the sufficient row-sum convergence bound
    by returning `converged == false` without iteration.

## Rationale

Named APIs make direction, weighting, normalization, and failure behavior
reviewable at the call site. Dense aligned vectors eliminate repeated
`NodeIndex` maps and survive beyond petgraph's local identifier lifetime.
Forward and reverse CSR allow reverse PageRank without allocating a transpose.

The iteration ceiling prevents hostile `usize::MAX` requests. Work budgets
prevent all-sources kernels from starting computations whose modeled cost is
already unacceptable. Returning an error before partial traversal is safer
than returning an apparently complete truncated ranking.

Adding the identity to the eigenvector iteration preserves eigenvectors while
avoiding period-two behavior on bipartite graphs. The Katz row-sum bound is
conservative, deterministic, and linear to inspect; computing the exact
spectral radius solely for option validation would duplicate the kernel and
introduce another convergence problem.

Brandes dependency accumulation consumes ratios, not absolute shortest-path
counts. Layer-wise rescaling preserves every ratio because all predecessors of
a vertex belong to the same preceding breadth-first layer. This avoids the
`inf / inf` failure mode of an exponentially branching but budget-compliant
graph without approximating the dependency formula.

## Alternatives considered

### Keep pgmcp's `HashMap<NodeIndex, f64>` outputs

Rejected. Hash iteration can reorder floating reductions; `NodeIndex` is not a
stable CPG identity; and each algorithm would rebuild a topology already held
in `GraphProjection`.

### Put every score in one enum-returning function

Rejected. Personalized seeds, Katz attenuation, HITS paired vectors, edge
keys, and Brandes normalization have materially different types and contracts.
A broad dispatcher would move errors from the type system to runtime.

### Parallelize inside every kernel

Rejected. An internal pool would obscure reduction order, compete with caller
schedulers, and add a feature/dependency to the core. The serial per-source
Brandes kernel is the stable parallelization seam.

### Iterate eigenvector centrality on plain adjacency

Rejected. A connected bipartite graph can have dominant eigenvalues of equal
magnitude and opposite sign, making plain power iteration oscillate even
though the desired nonnegative eigenvector exists.

### Compute the exact Katz spectral radius first

Rejected. It adds an iterative pre-analysis and could itself fail to converge.
The maximum weighted row sum is a sufficient convergence bound and is already
available from deterministic adjacency construction.

### Return partial all-sources results when a budget expires

Rejected. A source-prefix ranking is order-sensitive and can be mistaken for a
complete centrality vector. The implementation predicts modeled work and
rejects before traversal.

## Consequences

- The entire family is available with `default = []` and composes with every
  CPG or external projection.
- Score arrays are directly zip-compatible with `GraphProjection::nodes`.
- Edge betweenness uses undirected canonical pairs; vertex betweenness remains
  directed.
- Reciprocal weighted edges add in undirected spectral/Burt views. Negative
  weights have no influence. Self-loops affect spectral measures but not
  shortest-path or Burt neighborhoods.
- Invalid scalar numerical options return non-converged aligned results;
  malformed projection or budget failures use `AnalysisError` where the
  function signature is fallible.
- A non-converged eigenvector or Katz result is inspectable but must not be
  presented as a completed ranking without its convergence metadata.
- Future pgmcp rewiring can replace duplicate algorithms while preserving an
  ordered-parallel Brandes path.

## Verification

The decision is locked by fixed ring/path/star/triangle/disconnected fixtures,
petgraph PageRank and dominator differentials, a naive all-simple-path Brandes
oracle, source-partition composition, malformed and resource-cap examples,
feature-free public integration, corruption testing, a 1,100-layer
path-count-overflow regression, a 128-case generated input-permutation/
relabeling campaign over every measure, and oracle-guarded path/ring/clustered
benchmarks. See
[Scientific validation 12](../scientific/12-centrality-validation.md).

## References

- Brandes, U. (2001). *A Faster Algorithm for Betweenness Centrality.*
  [DOI 10.1080/0022250X.2001.9990249](https://doi.org/10.1080/0022250X.2001.9990249).
- Berkhin, P. (2005). *A Survey on PageRank Computing.*
  [DOI 10.1080/15427951.2005.10129098](https://doi.org/10.1080/15427951.2005.10129098).
- Bonacich, P. (1987). *Power and Centrality: A Family of Measures.*
  [DOI 10.1086/228631](https://doi.org/10.1086/228631).
- Katz, L. (1953). *A New Status Index Derived from Sociometric Analysis.*
  [DOI 10.1007/BF02289026](https://doi.org/10.1007/BF02289026).
