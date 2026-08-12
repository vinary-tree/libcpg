# ADR 0021: Deterministic bounded simple-cycle enumeration

- **Status:** Accepted
- **Date:** 2026-08-12
- **Scope:** `src/analysis/cycles.rs`

## Context

SCC decomposition answers whether a directed region is cyclic, but consumers
also need concrete witnesses: recursive call chains, cyclic module dependencies,
and control-flow cycles. The number of elementary cycles can be exponential,
so an unbounded `Vec<Vec<NodeId>>` API is not a safe library primitive.

The existing `GraphProjection` already supplies sorted stable nodes and forward
and reverse CSR. `projection_sccs` already supplies deterministic iterative
Tarjan decomposition. A new analysis should reuse those contracts, remain
available with `default = []`, and distinguish a complete answer from a bounded
prefix.

## Decision

Adopt a bounded iterative DFS variant with the following public contract:

1. `simple_cycles(&GraphProjection, &CycleOptions)` validates public CSR fields.
2. `max_length` defaults to 10 and is clamped to 32.
3. `max_cycles` defaults to 10,000 and is clamped to 100,000.
4. Search is restricted to SCCs already classified as cyclic.
5. Cycle lengths, starts, and CSR successors are traversed in ascending order.
6. Paths admit only vertices greater than their start; rotations are normalized
   to the smallest `NodeId` and guarded by a `BTreeSet`.
7. Output is sorted by length and then lexicographically.
8. On the first unique over-cap cycle, stop inside DFS, return the retained
   prefix, and expose `truncated`, `dropped_cycles = 1`, and the omitted cycle.
9. Use explicit, reusable path/frame/membership buffers; never recurse on graph
   depth and never allocate a graph-sized buffer for each start.

The DOI-verified Johnson paper is cited as the classical elementary-cycle
basis. Documentation states explicitly that this implementation is a bounded
DFS variant, not Johnson's blocked-set algorithm.

## Rationale

SCC reuse avoids algorithm duplication and proves that skipped regions cannot
contain a cycle. Stable dense order plus sorted CSR removes hash iteration from
the traversal. Processing length first makes a truncated result a canonical
prefix rather than an arbitrary deterministic subset.

The first omitted cycle is more useful than a bare boolean: callers can verify
the boundary and present a concrete explanation. The omitted count is labelled
a lower bound because computing the exact suffix size would defeat the cap.

## Alternatives considered

### Return an error when the count cap is exceeded

Rejected. Every retained cycle remains valid evidence, and callers commonly
prefer a bounded witness set. A typed partial result communicates more than an
error while remaining honest about incompleteness.

### Continue searching to count every omitted cycle

Rejected. Exact suffix counting has the same exponential search problem as
complete enumeration and would make `max_cycles` only a memory cap, not a work
boundary.

### Port Johnson's complete blocked-set implementation exactly

Rejected for this bounded API. It offers stronger unbounded enumeration
asymptotics but adds substantially more state and does not by itself define
stable prefix or truncation semantics. The simpler SCC-pruned DFS satisfies the
explicit small-length query while remaining auditable.

### Use `petgraph` cycle enumeration

Rejected. The feature-free analysis already has canonical CSR and SCC
substrates; converting to another graph representation would add allocation,
ordering reconciliation, and a second source of topology semantics.

### Deduplicate with a hash set

Rejected. Hash iteration and seeding are unnecessary nondeterminism risks.
`BTreeSet<Vec<u32>>` provides ordered, reproducible keys.

## Consequences

Positive consequences:

- complete results and bounded prefixes are distinguishable;
- self-loops, reciprocal cycles, and opposite orientations have explicit
  semantics;
- malformed serialized projections fail before indexed traversal;
- the same node/edge set produces byte-stable serde output;
- acyclic graphs pay only validation and SCC costs; and
- stack and retained-output resources have hard public ceilings.

Trade-offs:

- enumeration is still exponential on dense SCCs;
- the dropped count is a proven lower bound, not the exact suffix size; and
- repeated exact-length search favors canonical prefix order over Johnson's
  unbounded output-sensitive complexity.

## Verification

The implementation is checked against an independent exhaustive permutation
oracle on generated directed graphs of up to eight vertices. Tests cover every
edge in every cycle, simple membership, rotation uniqueness, direction
preservation, sorted prefixes under random count caps, reversed insertion
order, malformed public CSR, serde, parsed mutual recursion, a 25,000-node
ring, and oracle-guarded acyclic/sparse/dense benchmarks.

## Reference

- Johnson, D. B. (1975). *Finding All the Elementary Circuits of a Directed
  Graph.* SIAM Journal on Computing 4(1), 77–84. DOI:
  [10.1137/0204007](https://doi.org/10.1137/0204007).
