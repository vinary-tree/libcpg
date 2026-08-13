# ADR 0026 — derive architectural layers from condensation longest paths

- Status: Accepted
- Date: 2026-08-12

## Context

pgmcp's recommendation surface needs a reusable engine that proposes a stable
layer order and identifies cycle, skip-layer, and upward evidence. `libcpg`
already computes deterministic source-oriented longest-path levels over a
validated SCC condensation. Re-running community detection or inventing a
second module graph would produce a different identity domain and weaken
traceability.

The architectural convention numbers low layers from zero and high layers with
larger integers; the condensation metric numbers sources from zero. Public,
serializable structs also mean the function must remain total when callers pair
stale metrics with a decomposition.

## Decision

Expose `infer_layering(&SccDecomposition, &CondensationMetrics) ->
LayeringReport`. Validate the decomposition through the existing condensation
engine, validate only the metric fields consumed by layering, and reverse
source levels with `layer = height - 1 - source_level`.

Treat exactly-one-layer downward edges as healthy. Emit deterministic
`SkipLayer` records for longer descents and `Upward` records for lateral or
ascending edges. Emit one `CyclicComponent` record per cyclic SCC. The latter
is explicitly a component-level witness: SCC condensation no longer contains
enough information to name or count internal edges, and the library must not
fabricate them.

Return `complete` and `capped` state rather than panicking on malformed public
values. Keep the API feature-free and out of graph.

## Consequences

- Layer ids align with pgmcp's low-to-high architectural convention while
  preserving the exact grouping of condensation longest-path levels.
- Every unflagged condensation edge descends exactly one layer.
- The engine composes directly with module, call, CFG, or other SCC projections;
  pgmcp selects the module projection at its application boundary.
- Cyclic evidence is honest about contraction: it identifies the exact SCC but
  not an unavailable original edge.
- Stale bounded metrics remain diagnostically useful; impossible allocations,
  malformed graphs, and inconsistent dimensions are rejected.
- The shared validator costs one additional linear condensation pass.

## Alternatives considered

**Use Coffman–Graham output directly.** Rejected for this report because width-
bounded scheduling may add layers and move independent components. The task is
to preserve longest-path layering; CG remains available separately through
`coffman_graham`.

**Return concrete edges inside cyclic SCCs.** Rejected because
`SccDecomposition` intentionally retains only cross-component edges. Expanding
all node pairs would invent nonexistent edges, while selecting an arbitrary pair
would misstate evidence.

**Re-run Louvain and bucket communities by instability.** Rejected because it
is heuristic, changes the vertex identity domain, and duplicates existing
community and Martin-metric responsibilities.

**Reject every non-monotone level vector before classification.** Rejected
because it would make the specified upward category unreachable for public
deserialized inputs. Shape-consistent values are classified, while the direct
condensation pipeline remains strictly monotone.

## Validation

An exact four-component oracle proves reversal, cycle/skip witnesses, and the
one-step invariant. A stale-level oracle covers lateral and upward edges. A
256-case generated DAG property requires deterministic equality and proves
that every unflagged edge descends exactly one layer. Hostile tests cover
out-of-domain endpoints, incomplete/capped reports, impossible levels,
duplicate edges, and invalid multi-node SCC flags. Public and serde integration
tests protect the crate-root boundary. See [Scientific validation
21](../scientific/21-layering-validation.md).

## Reference

1. Coffman, E. G., Jr., & Graham, R. L. (1972). Optimal scheduling for two-
   processor systems. *Acta Informatica, 1*, 200–213. DOI:
   [10.1007/BF00288685](https://doi.org/10.1007/BF00288685).
