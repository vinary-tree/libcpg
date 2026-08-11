# ADR-0016: Stable WL projection labels and structural clone classes

- **Status:** Accepted
- **Date:** 2026-08-11
- **Decision owners:** libcpg maintainers
- **Scope:** feature-free graph analysis

## Context

The architecture-analysis epic requires the pgmcp Weisfeiler–Lehman (WL)
structural-clone capability on libcpg's shared `GraphProjection` substrate. The
result must be deterministic across node/edge insertion order and Rust
releases, safe for caller-constructed public projection values, bounded under
hostile iteration requests, and usable without parser or optional-language
features.

The source algorithm used a fixed SplitMix-style combiner but assigned compact
labels in vertex traversal order. That order does not preserve numeric label
multisets under an isomorphic stable-identifier relabeling. libcpg's public
contract explicitly requires that stronger determinism property.

## Decision

libcpg will:

1. expose `wl_labels(&GraphProjection, usize) -> Vec<u64>` and
   `structural_clone_classes(&GraphProjection, usize, usize) -> Vec<Vec<NodeId>>`;
2. seed labels from the ordered pair `(in_degree, out_degree)`;
3. refine against the sorted union of incoming and outgoing neighbor-label
   multisets, ignoring edge weights;
4. retain the source mix constants and wrapping operation sequence;
5. canonicalize every round by the sorted rank of all distinct raw labels,
   never by first encounter;
6. clamp requests to `MAX_WL_ITERATIONS = 16`;
7. validate the entire public projection contract before indexed traversal and
   return an empty result for malformed input; and
8. sort clone members by stable `NodeId`, then classes by descending size and
   smallest member, treating `min_class < 2` as two.

`GraphProjection::is_well_formed` becomes the shared validation boundary for
WL and DSM. It checks stable-node order, both compressed-sparse-row (CSR)
shapes, target bounds/order, weight alignment, and forward/reverse equality.

## Rationale

Directed-degree seeds cheaply preserve source/sink distinctions. A combined
incident multiset matches the ported kernel and remains order-independent.
The fixed mixer avoids an implementation-defined standard-library hash.
Sorted-rank canonicalization adds $`O(n\log n)`$ work per round but makes the
numeric result invariant under stable-identifier relabeling. A 16-round cap
provides an explicit work bound while retaining neighborhoods far deeper than
the three rounds used by libcpg's separate whole-graph similarity metric.

Returning empty vectors for malformed projections follows the existing total
analysis boundary: callers cannot confuse partial labels with valid aligned
labels because a nonempty well-formed graph must return exactly `node_count`
labels. Clone classes expose stable identifiers so results survive outside the
projection's local dense-index lifetime.

## Alternatives considered

### Use `DefaultHasher`

Rejected. Its algorithm is not promised stable across Rust versions, making
persisted regression fixtures and cross-process reproduction unreliable.

### Assign canonical labels on first encounter

Rejected. It is deterministic for one sorted `NodeId` universe but not under
an isomorphic relabeling that changes which signature appears first.

### Preserve separate incoming and outgoing multisets at every round

Not selected for this port. It is a valid directed-WL variant, but it changes
the pgmcp kernel's equivalence relation. The directed-degree seed already
preserves the first-order orientation distinction and remains part of every
later signature.

### Run until convergence with no ceiling

Rejected for the public API. Convergence is finite, but the caller could force
work proportional to graph size when bounded neighborhood evidence is the
required result. An explicit clamped round count is reproducible and safe.

### Treat equal labels as exact clones

Rejected. One-dimensional WL is not a complete isomorphism test and the
implementation uses a finite digest. The API deliberately calls them
structural clone classes and the documentation requires exact downstream
matching when proof is needed.

## Consequences

- The API is feature-free and composes with every built-in or external
  projection.
- Results are deterministic under node relabeling, edge order, and duplicate
  input edges normalized by `from_edges`.
- Edge kind, weight, node kind, name, and source text do not affect labels.
- Reciprocal edges and self-loops contribute through both forward and reverse
  adjacency, preserving incident multiplicity.
- Numeric labels are stable tokens for this implementation but are not global
  semantic identifiers or cryptographic hashes.
- Candidate classes can contain false positives and require semantic or exact
  matching when a stronger claim is needed.

## Verification

The decision is locked by fixed triangle/star examples, exact mixer vectors,
iteration-bound and malformed-input tests, a 256-case isomorphic-relabeling
property with reordered/duplicated edges, feature-free public integration, the
repository corruption campaign, and oracle-guarded Criterion benchmarks. See
[Scientific validation 11](../scientific/11-wl-validation.md).

## References

- Shervashidze, N., Schweitzer, P., van Leeuwen, E. J., Mehlhorn, K., and
  Borgwardt, K. M. (2011). *Weisfeiler-Lehman Graph Kernels.* Journal of
  Machine Learning Research 12(77), 2539–2561.
  [JMLR article and PDF](https://jmlr.org/papers/v12/shervashidze11a.html).
