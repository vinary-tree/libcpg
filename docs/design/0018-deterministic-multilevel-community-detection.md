# ADR-0018: Deterministic multi-level community detection

- **Status:** Accepted
- **Date:** 2026-08-11
- **Decision owners:** libcpg maintainers
- **Scope:** feature-free projection analysis

## Context

pgmcp contains a single-level Louvain local-moving kernel and a separate
connected-component refinement. The kernel stores assignments and candidate
weights in hash maps. Hash iteration can decide equal modularity gains, so the
same graph can receive a different partition across runs. It also omits the
aggregation phase that defines multi-level Louvain.

libcpg requires deterministic public boundaries, stable identifiers, explicit
caps, malformed-input hardening, feature-free graph kernels, and node-aligned
results. Community detection must preserve these contracts without cloning or
mutating the CPG.

## Decision

libcpg will:

1. expose `louvain(&GraphProjection, LouvainOptions)` as a feature-free,
   fallible multi-level analysis;
2. forget edge direction, deterministically sum reciprocal weights, count
   self-loops once as edge mass and twice as weighted degree, and reject
   negative or non-finite weights;
3. perform local-moving sweeps in ascending dense vertex order;
4. accumulate neighbor-community weight into a reusable dense vector and scan
   touched community identifiers in ascending order;
5. resolve exact score ties toward the smallest community identifier;
6. aggregate each partition into a new weighted graph, preserving total edge
   weight and sorting weights before compensated summation;
7. clamp work to 50 passes per level and 10 aggregation levels;
8. return `CommunityAssignment` vectors aligned with sorted projection nodes;
9. renumber communities by their smallest stable member;
10. recompute reported generalized modularity on the original graph rather
    than reusing a local gain accumulator;
11. expose `try_refine_connected` as the checked connected-component split and
    `refine_connected` as a total downstream convenience boundary;
12. retain the resolution in `CommunityKind` so refinement can recompute the
    exact score of its partition; and
13. describe the refinement as Leiden-style connectivity refinement, never as
    a complete Leiden implementation.

## Rationale

Dense vectors remove hash-order dependence and avoid allocating a map per
vertex. Canonical dense traversal makes floating-point reduction order part of
the contract. Smallest-identifier tie breaking makes equal-gain decisions
reviewable and reproducible.

Aggregation is not an optimization detail: it enables whole communities to
move after vertex-level local moving has stopped. Omitting it changes the
algorithm and can miss a better coarser partition. Recomputing modularity from
the final original-node assignment provides a direct consistency boundary and
supports an independent test oracle.

The connected refinement is separate because its objective differs from
modularity maximization. A caller can inspect raw Louvain output, request the
connectivity guarantee, or use the checked boundary for untrusted serialized
assignments.

## Alternatives considered

### Port pgmcp's hash-map kernel verbatim

Rejected. Candidate iteration can change exact tie outcomes, result maps are
not aligned with stable CPG identifiers, and the implementation is not
multi-level Louvain.

### Randomize vertex order and retain the best run

Rejected for the core API. It would require a seed and run budget, make exact
reproduction conditional on a random-number implementation, and multiply
work. A future experimental wrapper can run deterministic seeded permutations
without weakening the base kernel.

### Depend on an external clustering crate

Rejected. The core must remain feature-free and petgraph-major-version
independent. The required kernel fits the existing CSR substrate, and the
in-crate implementation makes weight, cap, and tie semantics explicit.

### Claim full Leiden guarantees

Rejected. Splitting disconnected induced subgraphs guarantees ordinary
connectivity only. Full Leiden has a different refinement and aggregation
protocol with stronger theorems. Conflating the two would overstate the
implementation.

### Silently ignore invalid weights

Rejected. Clamping or dropping negative/non-finite weights changes the
modularity measure while returning an apparently valid partition. Typed
rejection preserves the evidence boundary.

## Consequences

### Positive

- The same projection and options produce byte-identical assignments.
- Multi-level aggregation is available without a new dependency.
- Results are directly joinable to all projection-aligned analyses.
- Connected refinement has an explicit, independently testable guarantee.
- Caps bound every heuristic repetition dimension.

### Trade-offs

- The canonical vertex order selects one of potentially many local optima.
- Dense candidate storage uses $`O(n)`$ memory per active level.
- The connected refinement is not full Leiden and may lower modularity.
- Directed dependency meaning is intentionally reduced to undirected
  association for this algorithm.

## Validation

The decision is guarded by analytic planted-clique and complete-graph
fixtures, an independent modularity implementation, a ring-of-cliques
multi-level witness, aggregation weight conservation, generated storage-order
permutations, generated connectedness checks, malformed numeric/CSR cases,
public API integration, a parsed Rust call-graph experiment, and the shared
corruption campaign. See
[community validation](../scientific/13-community-validation.md).

## References

- Blondel et al. 2008.
  [DOI: 10.1088/1742-5468/2008/10/P10008](https://doi.org/10.1088/1742-5468/2008/10/P10008).
- Traag, Waltman, and van Eck 2019.
  [DOI: 10.1038/s41598-019-41695-z](https://doi.org/10.1038/s41598-019-41695-z).
