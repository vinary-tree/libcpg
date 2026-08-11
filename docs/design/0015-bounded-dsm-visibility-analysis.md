# ADR 0015 — Bounded DSM visibility over deterministic projections

## Status

**Accepted.** Implemented by `src/analysis/dsm.rs` and the generalized
`projection_sccs` entry point in `src/analysis/scc.rs`.

## Context

Architecture analysis needs more than direct in-degree and out-degree. A
dependency can propagate through several intermediate components, so
[Design Structure Matrix
(DSM)](../GLOSSARY.md#design-structure-matrix-dsm) research measures indirect
visibility and its density, commonly called [propagation
cost](../GLOSSARY.md#propagation-cost) [[1]](#references). Directed-network
architecture work also treats the largest cyclic group as distinct core
evidence [[2]](#references).

libcpg already has the necessary feature-free substrates:

- [`GraphProjection`](../components/graph/projections.md) provides canonical
  stable-node identity and forward/reverse compressed-sparse-row (CSR)
  adjacency without cloning CPG payloads;
- the iterative Tarjan SCC engine provides deterministic component membership,
  self-cycle detection, and condensation edges; and
- the out-of-graph analysis convention keeps derived reports immutable and
  avoids contaminating CPG topology with analysis conclusions.

The pgmcp source being ported performed one breadth-first search (BFS) per
vertex and contained its own SCC routine. Its public signatures are total:
`analyze_dsm(&GraphProjection) -> DsmAnalysis` and
`classify_core_periphery(&GraphProjection, &DsmAnalysis) ->
CorePeripheryResult`. The port must preserve its chain/cycle/hub fixture
semantics and median quadrants while satisfying libcpg's deterministic,
feature-free, malformed-input, stable-identity, and explicit-cap contracts.

The all-sources search costs $`O(n(n+m))`$ for $`n`$ vertices and $`m`$ edges.
Without a hard bound, a small sparse-looking public input can still trigger
quadratic work. Public `GraphProjection` fields and serde also mean callers can
construct inconsistent CSR arrays or combine an analysis with a different
same-sized graph.

## Decision

### 1. Consume `GraphProjection`, not parsers or CPG payloads

Both DSM entry points operate only on a payload-free `GraphProjection`. They do
not parse source, inspect syntax, infer calls, or mutate a CPG. Callers choose
the semantic edge relation by constructing a call, module, control-flow,
data-flow, program-dependence, condensation, or external projection first.

### 2. Compute visibility by one forward BFS per source

The implementation reuses one visited vector and queue. It marks the source
visited before traversal, so self-visibility is excluded even when a cycle
returns to the source. Each newly reached target increments exactly one source
VFO count and one target VFI count. Integer reachable-pair totals are converted
to `f64` only for the final normalization by $`n^2`$.

This computes the required row and column sums without allocating the
$`n\times n`$ closure matrix.

### 3. Preserve median quadrant policy and separate cyclic-core evidence

VFI and VFO medians define inclusive high thresholds. The four combinations
map to `Core`, `Shared`, `Control`, and `Peripheral`. This is an explicit
comparative policy rather than a claim that median thresholds reproduce every
classification rule in the literature.

The largest SCC containing at least two vertices is reported separately as
`cyclic_core`. A singleton self-loop is excluded from this multi-component
architectural group. Equal-size candidates choose the component with the
smallest stable `NodeId`.

![The four median visibility quadrants and the separate largest multi-vertex SCC.](../diagrams/dsm-core-periphery.svg)

*Figure — two related but non-equivalent core signals. Source:
[`diagrams/dsm-core-periphery.puml`](../diagrams/dsm-core-periphery.puml).*

### 4. Reuse one generalized SCC engine

Add `projection_sccs(&GraphProjection) -> SccDecomposition` to the existing SCC
module. Known call-graph and control-flow projection kinds retain their
specialized `SccProjection` provenance; every other direct projection records
`SccProjection::Projection(kind)`. The wrapper clones canonical dense adjacency
and invokes the same iterative Tarjan `decompose` function used by the existing
CPG-specific APIs.

DSM classification must call this entry point. It must not copy pgmcp's local
Tarjan implementation or label an arbitrary graph as a call graph.

### 5. Validate public projection structure and exact provenance

Before BFS, validate:

- strictly ascending stable nodes;
- forward and reverse offset shape and monotonicity;
- in-range, strictly ascending adjacency targets;
- optional weight alignment;
- equal forward/reverse edge counts; and
- every forward edge's presence in the matching reverse slice.

`DsmAnalysis` retains the exact stable node universe. Classification requires
that universe, the node count, both vector lengths, completion status,
per-vertex count range, equal fan-in/fan-out totals, finite propagation cost,
and agreement between the scalar and total count to match the supplied
projection. An analysis from a different graph with the same cardinality or a
caller-forged inconsistent result is therefore rejected.

### 6. Make the total failure state explicit

Set `MAX_DSM_NODES = 50_000`. An oversized input is rejected before structural
validation and traversal. It returns `complete == false`, `capped == true`,
the input node universe, and empty derived vectors. A malformed projection or
mismatched result returns `complete == false`, `capped == false`. No partial
visibility vector or partial class vector is exposed.

This status-bearing result preserves the specified total signatures without a
panic, silent truncation, sentinel count, or breaking `Result` conversion.

### 7. Keep topology binary and results deterministic

Edge weights do not affect visibility. Duplicate edges have already been
merged by `GraphProjection`; sorted stable nodes and adjacency determine dense
order. Median sorting, SCC order, cyclic-core ties, and stable lookup are fully
specified. All result types support serde behind the existing `serde` feature.

## Consequences

### Positive

- Exact indirect-coupling metrics are available with `default = []` for every
  projection kind and for caller-supplied graphs.
- The $`O(n^2)`$ closure matrix is never materialized.
- One SCC implementation defines cycle behavior across CFG, call-graph,
  condensation, loop, and DSM analyses.
- Stable-node provenance prevents accidental cross-graph composition.
- Malformed public CSR and excessive node counts are observable without panic
  or partial-result ambiguity.
- Deterministic ordering makes reports cacheable, serializable, and suitable
  for exact property assertions.

### Negative

- $`O(n(n+m))`$ time remains expensive well below the hard cap; the cap is a
  safety ceiling, not a latency promise.
- Validation checks reverse agreement with binary searches, adding up to
  $`O(m\log d_{max})`$ before the dominant BFS phase.
- The median labels can be counterintuitive for degenerate distributions; an
  all-zero graph labels every vertex `Core` under inclusive zero thresholds.
- Retaining stable node provenance adds one $`O(n)`$ vector to each result.
- `projection_sccs` extends the public SCC provenance enum with an additive
  variant that exhaustive downstream matches must handle.

## Alternatives considered

### Materialize the transitive-closure matrix

Rejected. It simplifies row/column summation but adds $`O(n^2)`$ memory, even
though the API returns only two vectors and a scalar. At the public cap, dense
storage is disproportionate and unnecessary.

### Use Floyd–Warshall in production

Rejected. Its $`O(n^3)`$ time and $`O(n^2)`$ storage are unsuitable for the
sparse graph substrate. It remains valuable as an independent small-graph test
oracle.

### Copy pgmcp's DSM-local Tarjan implementation

Rejected. Two SCC engines would create competing singleton-cycle, tie-order,
stack-safety, and determinism contracts. Generalizing the proven libcpg entry
point is smaller and preserves one source of truth.

### Call `decompose` with `SccProjection::CallGraph` for every input

Rejected. It would compute correct components but publish false provenance for
module, data-flow, program-dependence, condensation, and external graphs.

### Return `Result` from the two public functions

Rejected for this port. It breaks the required total signatures and the
near-verbatim pgmcp API. Explicit `complete` and `capped` fields distinguish all
bounded outcomes while preserving composition.

### Panic or silently truncate above 50,000 vertices

Rejected. A panic is hostile to library consumers; truncation changes global
reachability and can produce plausible but false metrics. Empty derived vectors
plus explicit status cannot be mistaken for a complete calculation.

### Classify from direct degree instead of transitive visibility

Rejected. Direct degree answers a different question and misses indirect
architectural coupling, the defining purpose of the DSM visibility method.

## References

1. MacCormack, A., Rusnak, J., Baldwin, C. Y. (2006). *Exploring the Structure
   of Complex Software Designs: An Empirical Study of Open Source and
   Proprietary Code.* Management Science 52(7), 1015–1030. DOI:
   [10.1287/mnsc.1060.0552](https://doi.org/10.1287/mnsc.1060.0552)
2. Baldwin, C. Y., MacCormack, A., Rusnak, J. (2014). *Hidden Structure: Using
   Network Methods to Map System Architecture.* Research Policy 43(8),
   1381–1397. DOI:
   [10.1016/j.respol.2014.05.004](https://doi.org/10.1016/j.respol.2014.05.004)
