# System dependence summary edges and slicing

`analysis::sdg` composes three existing feature-free providers—resolved call
topology, interprocedural DFG edges, and effect summaries—with the PDG. It adds
one labeled overlay and exposes deterministic backward and forward slicing.

![End-to-end SDG overlay construction](../../diagrams/sdg-summary-overlay.svg)

## Public surface

| Item | Semantics |
|---|---|
| `SUMMARY_EDGE_LABEL` | stable string `summary` |
| `SliceCaps::new(max_nodes, max_call_depth)` | explicit traversal resource limits |
| `build_summary_edges(&mut cpg, &effects)` | append labeled actual-to-call edges and return addition count |
| `interprocedural_backward_slice(&cpg, criterion, &caps)` | ordered HRB backward slice |
| `interprocedural_forward_slice(&cpg, criterion, &caps)` | ordered direction-dual forward slice |

All entry points compile with `default = []`. `SliceCaps` gains serialization
only under `serde`.

## Input contract

1. Resolve calls so each represented call has `CallSite`, `StaticCall`, or
   `DynamicCall` topology.
2. Extract DFG after resolution so actual-to-formal `Parameter` and
   return-expression-to-call `ReturnValue` edges exist.
3. Build each function's PDG with `PdgBuilder`.
4. Compute `BTreeMap<NodeId, EffectSummary>` with `effect_summaries` or provide
   summaries obeying the same mask/cap contract.
5. Call `build_summary_edges`, then slice.

The summary builder tolerates absent summaries, missing endpoints, duplicate
call-edge kinds, and several resolved callees. It never adds nodes.

## Internal traversal seam

The intraprocedural and SDG APIs share `builder::pdg::slice_phase`. An edge mask
always admits PDG edges and independently controls `Parameter` and
`ReturnValue`. Each queue item is `(NodeId, call_depth)`; a `BTreeMap` stores
the best depth for every phase node, and a `BTreeSet` accumulates the public
result.

This reuse gives the legacy PDG slicers stable neighbor order while keeping
their `FxHashSet<NodeId>` return type compatible. The SDG API exposes ordered
sets at its new public boundary.

## Two-phase state transition

![Backward and forward phase masks](../../diagrams/sdg-two-pass-slicing.svg)

| Direction | Phase one (ascend) | Phase two (descend) |
|---|---|---|
| backward / incoming | PDG + summary + `Parameter`; no `ReturnValue` | PDG + summary + `ReturnValue`; no `Parameter` |
| forward / outgoing | PDG + summary + `ReturnValue`; no `Parameter` | PDG + summary + `Parameter`; no `ReturnValue` |

Phase two is seeded with every phase-one node and its smallest accumulated call
depth. This is necessary: a caller dependence found through a summary can
itself contain a call whose callee contributes to the slice.

## Labeled uniqueness

`connect_unique_labeled` compares endpoints, edge kind, and label. This permits
an unlabeled `DataDependence` and a `DataDependence(label="summary")` to coexist
without duplication or mutation. `build_summary_edges` pre-sorts and
deduplicates its candidates so multi-edge call topology and multi-target masks
produce one edge per semantic endpoint pair.

## Bounds and failure behavior

- `max_nodes == 0`: empty result;
- unknown criterion: empty result;
- `max_call_depth == 0`: only PDG/summary reachability;
- missing callee summary: no summary edge for that target;
- capped callee summary: every actual argument receives a summary edge; and
- mask-truncated callee summary: unrepresentable argument positions receive
  summary edges.

No public SDG function panics on malformed graph references. Construction
returns an addition count rather than an error because absent edges are an
ordinary partial-evidence condition.

## Related material

- [Theory 28](../../theory/28-system-dependence-and-slicing.md)
- [ADR-0035](../../design/0035-system-dependence-summary-slicing.md)
- [Usage 11](../../usage/11-interprocedural-slicing.md)
- [Scientific validation 30](../../scientific/30-sdg-slicing-validation.md)
- [Security boundary](../../security/03-system-dependence-slicing.md)

## References

1. Horwitz, S., Reps, T., Binkley, D. (1990). *Interprocedural Slicing Using
   Dependence Graphs.* DOI:
   [10.1145/77606.77608](https://doi.org/10.1145/77606.77608).
2. Ferrante, J., Ottenstein, K. J., Warren, J. D. (1987). *The Program
   Dependence Graph and Its Use in Optimization.* DOI:
   [10.1145/24039.24041](https://doi.org/10.1145/24039.24041).
