# ADR-0014: Deterministic condensation-DAG analytics

## Status

Accepted.

## Context

`SccDecomposition` already records strongly connected components (SCCs), stable
component membership, and the edges of the contracted directed acyclic graph
(DAG). Consumers still had to reimplement topological order, architectural
depth, weighted critical paths, and width-bounded layering. Reimplementation
would duplicate validation, invite insertion-order-dependent ties, and tempt a
consumer to reconstruct a parallel graph whose identity can drift from SCC
component ids.

The public SCC fields also permit downstream construction. Values produced by
`libcpg` are canonical DAGs, but an external caller can supply duplicate edges,
an invalid endpoint, an empty component, or even a cycle. The aggregate metrics
entry point has an intentionally total signature, while direct
Coffman–Graham scheduling has a fallible signature. Both paths need bounded,
explicit behavior for malformed and adversarial input.

Two layer conventions are useful and easy to confuse:

- longest-path levels grow from sources and express dependency depth; and
- scheduling layers grow from sinks and require precedence edges to point
  strictly downward.

The design must make the orientation visible rather than silently choosing one.

## Decision

Add a feature-free `analysis::condensation` module with two root-exported entry
points:

```rust
pub fn condensation_metrics(
    decomposition: &SccDecomposition,
    options: &CondensationOptions,
) -> CondensationMetrics;

pub fn coffman_graham(
    decomposition: &SccDecomposition,
    width: u32,
) -> Result<CgLayering, AnalysisError>;
```

Both derive canonical adjacency from
`SccDecomposition::condensation_projection`, then map its stable
representatives back to SCC component ids through `component_by_node`. They do
not create a petgraph value, clone CPG nodes, or mutate any overlay.

### Canonical normalization

Validate component-position identity, non-empty membership, exact agreement
between component lists and `component_by_node`, edge endpoints, and absence of
self-edges. Build the existing payload-free condensation `GraphProjection`,
map its dense representatives to component ids, and retain sorted forward and
reverse adjacency vectors. A completed Kahn pass proves acyclicity. Duplicate
public edges are accepted because the projection normalizes them to the same
condensation relation; structural contradictions are rejected.

### Longest paths

Use Kahn's topological algorithm with a minimum-component-id heap [[1]](#references).
Compute source-oriented levels and SCC-member-weighted paths in one forward
dynamic program. Path-weight ties select the smaller immediate predecessor;
terminal ties select the smaller terminal component. Return `height` and
maximum level occupancy as `width`.

### Coffman–Graham layering

Use the predecessor-oriented transpose of Coffman and Graham's lexicographic
labeling [[2]](#references):

1. source-ready components enter a min-heap;
2. a key is the descending sequence of immediate-predecessor labels;
3. keys compare lexicographically, with component id as the final tie-break;
4. components are placed in decreasing label order; and
5. each enters the lowest non-full layer strictly above all successors.

Expose sink layer zero and document the invariant

```math
(u,v)\in E_c \Longrightarrow layer(u)>layer(v).
```

Sort component ids inside each returned layer. Preserve the requested
`width_limit` separately from the actual `width()`.

### Resource and error policy

Set `MAX_CONDENSATION_COMPONENTS` to $`10^6`$. `CondensationOptions` may lower
but not raise this limit.

- `condensation_metrics` remains total. Cap breach returns empty evidence with
  `complete == false` and `capped == true`; malformed structure returns empty,
  uncapped evidence. An invalid optional width preserves computed core metrics
  while setting `complete == false` and omitting the layering.
- Direct `coffman_graham` returns `AnalysisError::InvalidInput` for width zero
  or malformed structure and `AnalysisError::BudgetExceeded` above the hard
  cap.

All traversals are iterative. Layer and dense indices remain `u32` because the
hard cap is below that domain.

### Feature and serialization policy

The algorithms use only the existing feature-free SCC substrate, standard
collections, and `AnalysisError`; no Cargo capability feature is introduced.
Public options and reports derive serde only under the existing `serde`
feature.

## Alternatives considered

### Treat projection dense ids as component ids

Rejected. `SccDecomposition::condensation_projection` sorts stable `NodeId`
representatives, while public downstream-built decompositions can store valid
components in another id order. The implementation reuses the projection but
maps each representative through `component_by_node`; it never assumes dense
projection identity equals SCC identity.

### Use petgraph's arbitrary topological order

Rejected. A correct but insertion-sensitive order would make equal path and
layer choices unstable across semantically identical graph storage. The
minimum-id heap makes the tie contract executable and testable.

### Return only one layer orientation

Rejected. Source depth and sink-first scheduling answer different questions.
Negating or reversing an undocumented index in a consumer is error-prone. The
two vectors have distinct names, invariants, figures, and examples.

### Greedy breadth packing without lexicographic labels

Rejected. It can satisfy a width bound while losing the Coffman–Graham
two-processor optimum and general height guarantee. The independent exhaustive
schedule oracle would expose that substitution.

### Panic on malformed public structs

Rejected. `SccDecomposition` fields are public by campaign contract, and these
analyses are reasonable boundaries for untrusted serialized or downstream-built
values. Structural failure must be data, not process termination.

### Add a new dependency or feature

Rejected. Binary heaps, ordered sets, dense vectors, and deterministic sorting
are sufficient. A capability flag would weaken the feature-free graph-analysis
surface without buying correctness.

## Consequences

### Positive

- Call-graph and CFG consumers share one canonical condensation metric engine.
- Component weights retain recursive/cyclic cluster size in critical paths.
- Width, precedence, determinism, cap, and approximation claims have independent
  property oracles.
- T2.13 layering inference and later condensation consumers can reuse the
  result rather than implementing another scheduler.
- The CPG and SCC decomposition remain immutable.

### Costs

- Lexicographic heap comparisons can inspect long common key prefixes; the
  documentation reports that cost instead of claiming a linear CG kernel.
- A total metrics signature needs explicit `complete` and `capped` fields.
- The opposite level orientations require sustained documentation discipline.

### Compatibility

This is an additive, feature-free API. No existing graph shape, serialized CPG,
SCC result, feature set, or algorithm-detector result changes. `AnalysisError`
gains the general `InvalidInput` variant for structurally invalid analysis
arguments.

## Validation

The implementation is accepted only with:

- fixed empty, weighted-branch, malformed, cap, width-zero, and serde examples;
- a 128-case DAG property campaign with exhaustive weighted-path and optimal
  unit-schedule oracles;
- duplicate/reversed edge-storage determinism;
- feature-free and parser-backed public integration pipelines;
- an oracle-guarded Criterion target at two graph sizes and shapes; and
- repository fmt, clippy, rustdoc, documentation, feature-matrix, coverage, and
  staged bug-gate checks.

The detailed falsifiable hypotheses live in
[Scientific validation 09](../scientific/09-condensation-validation.md).

## References

1. Kahn, A. B. (1962). *Topological sorting of large networks.*
   Communications of the ACM 5(11), 558. DOI:
   [10.1145/368996.369025](https://doi.org/10.1145/368996.369025)
2. Coffman, E. G., Jr., Graham, R. L. (1972). *Optimal scheduling for
   two-processor systems.* Acta Informatica 1, 200–213. DOI:
   [10.1007/BF00288685](https://doi.org/10.1007/BF00288685)
