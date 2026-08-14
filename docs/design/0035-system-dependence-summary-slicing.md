# ADR-0035: Overlay SDG summary edges and two-pass slicing

- **Status:** Accepted
- **Date:** 2026-08-14
- **Scope:** feature-free `analysis::sdg` and shared PDG traversal

## Context

Intraprocedural PDG slicing cannot cross resolved function boundaries.
Unrestricted reachability over actual-to-formal and return-to-call DFG edges is
not a sufficient extension: when several call sites share one callee, a walk
can enter through one call context and leave through another.

T3.5 already computes `EffectSummary::param_to_return`. The graph vocabulary
already contains `Parameter`, `ReturnValue`, and `DataDependence`; introducing
a parallel graph or another public edge kind would violate the unified-overlay
design.

## Decision

1. Represent the System Dependence Graph (SDG) as typed edges over the existing
   CPG node set, never as a parallel graph.
2. Retain `Parameter` and `ReturnValue` as the interprocedural spine emitted by
   DFG extraction after call resolution.
3. Materialize caller-local summary relations as `DataDependence` edges with
   the stable label `summary`.
4. Derive summary endpoints from the union of resolved callees'
   `param_to_return` masks.
5. Treat capped effect summaries as depending on every actual argument, and
   treat parameter positions beyond a truncated `u64` mask conservatively.
6. Make labeled edge identity include endpoints, kind, and label so an
   unlabeled data dependence is neither overwritten nor mistaken for a
   summary edge.
7. Implement backward slicing in two passes: exclude `ReturnValue` while
   ascending; then exclude `Parameter` while descending.
8. Implement forward slicing as the direction dual: exclude `Parameter` while
   ascending; then exclude `ReturnValue` while descending.
9. Share one deterministic masked traversal with the existing PDG slicers.
10. Require callers to provide `SliceCaps { max_nodes, max_call_depth }`; return
    an ordered `BTreeSet<NodeId>` and the empty set for an unknown criterion.

## Rejected alternatives

### A separate SDG object

Rejected. Node translation would duplicate identity, increase memory, and
break the invariant that analyses compose through one graph.

### A new `Summary` edge enum variant

Rejected. Summary is a semantic role of data dependence. The label preserves
that role without expanding the stable edge-kind vocabulary.

### Mutate an existing unlabeled data-dependence edge

Rejected. The same endpoints can carry both an ordinary dependence and a
summary dependence. Relabeling would destroy existing evidence and violate
append-only construction.

### One unrestricted reachability pass

Rejected. It admits unrealizable paths between sibling call sites sharing a
callee, the context problem the HRB construction was designed to solve
[[1]](#references).

### Store a full call stack in each work item

Rejected for this API. It increases state from node/depth pairs to potentially
many call strings. HRB summary edges plus two edge masks supply the intended
context discipline with bounded linear state.

### Return an unordered hash set

Rejected. Public analysis results must be stable across graph insertion order
and process hash seeds.

## Consequences

### Positive

- Existing PDG clients keep their API and receive deterministic neighbor
  selection as a shared improvement.
- Summary-edge construction is append-only, labeled, and idempotent.
- Backward and forward slices cross calls without switching sibling call
  contexts.
- The feature-free surface can operate on parsed, merged, deserialized, or
  caller-built CPGs.
- Node-count and call-depth resource limits are explicit at every public SDG
  slice invocation.

### Negative

- The `u64` effect mask needs conservative widening for signatures wider than
  64 formals.
- A plain `BTreeSet` result does not carry a completeness flag; consumers must
  know the caps they supplied and preserve upstream effect-summary cap state.
- Precision remains bounded by call resolution, DFG/PDG evidence, and effect
  summary precision.

## Verification

Named examples cover selective and multi-target summary construction,
unlabeled-edge coexistence, idempotency, append-only shape, missing summaries,
capped and 65-argument boundaries, backward and forward call crossing,
wrong-call-site exclusion, unknown criteria, zero node/depth caps, and serde.
Generated tests permute edge insertion, compare repeated backward and forward
results, prove cap monotonicity, and require the existing intraprocedural slice
to remain a subset when the full graph fits. A crate-root integration test
composes effect analysis, PDG construction, summary construction, and both
slice directions.

## References

1. Horwitz, S., Reps, T., Binkley, D. (1990). *Interprocedural Slicing Using
   Dependence Graphs.* DOI:
   [10.1145/77606.77608](https://doi.org/10.1145/77606.77608).
2. Ferrante, J., Ottenstein, K. J., Warren, J. D. (1987). *The Program
   Dependence Graph and Its Use in Optimization.* DOI:
   [10.1145/24039.24041](https://doi.org/10.1145/24039.24041).
