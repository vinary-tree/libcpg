# ADR 0023 — Use deterministic bounded bipartite GED

- **Status:** Accepted
- **Date:** 2026-08-12
- **Decision owners:** `libcpg` maintainers

## Context

The existing `SimilarityMetric::GraphEdit` name described a weighted blend of
node-count difference, edge-count difference, and node-kind Jaccard overlap.
That surrogate did not construct an edit mapping, did not distinguish edge
kinds or wiring, and could score topologically different graphs too highly.

The replacement must remain feature-free, deterministic, symmetric, bounded
for untrusted CPGs, configurable for scientific use, and compatible with the
existing `GraphSimilarity` facade. It must also expose when a hard resource
limit prevents genuine GED computation.

## Decision

1. Add `ged_upper_bound`, `GedOptions`, and `GedBound` under the always-on
   `pattern` module.
2. Build a Riesen–Bunke-style node-star matrix with substitution,
   insertion/deletion dummy, and degree-difference costs.
3. Solve the square assignment with an in-crate primal-dual Hungarian
   implementation. Ascending scans and strict improvement fix ties.
4. Ignore the assignment objective as the final distance. Evaluate the
   induced mapping as a concrete edit script over `NodeKindTag` labels and
   typed directed-edge multisets.
5. Evaluate both orientations and the delete-all/insert-all script, returning
   their minimum. This preserves the concrete-script upper-bound proof while
   enforcing symmetry and normalized boundedness.
6. Reject non-finite or negative costs and non-finite derived arithmetic.
7. Cap each input at 512 nodes, honoring a smaller caller limit. Above the
   effective limit, return the old surrogate with `capped = true`; never call
   that compatibility value a proven upper bound.
8. Define `SimilarityMetric::GraphEdit` as one minus the default normalized
   uncapped bound. Retain structural/label weights only for the capped legacy
   path.
9. Replace `DefaultHasher` in the whole-CPG WL similarity helper with the same
   fixed SplitMix-style combiner used by projection WL.

## Alternatives considered

### Keep the count/Jaccard surrogate

Rejected. Linear cost was attractive, but the result did not measure an edit
script and the API name overstated its semantics.

### Compute exact GED

Rejected for the general CPG boundary. Exact correspondence search is
combinatorial and conflicts with predictable operation on ordinary parsed
graphs. Tests still use an exhaustive solver through six nodes as an
independent oracle.

### Return the Hungarian star objective directly

Rejected. Local star costs approximate incident structure and need not equal
the cost of a globally consistent typed-edge edit script. Pricing the concrete
script provides the upper-bound guarantee.

### Use a third-party assignment crate

Rejected. The required primal-dual kernel is small, feature-free, and fully
covered against brute-force permutations through order seven. An additional
dependency would not reduce the public algorithmic surface or verification
obligation.

### Fail when the hard limit is exceeded

Rejected for the `GraphSimilarity` compatibility surface, which historically
returned a total score. The explicit `capped` bit lets direct callers require
true bound evidence while the facade remains total.

## Consequences

Within the budget, `GraphEdit` now responds to node tags, directed wiring,
complete edge kinds, self-loops, and parallel multiplicity. The result is
deterministic, symmetric, in $`[0,1]`$ after normalization, and backed by a
concrete edit script. The admitted path uses cubic time and quadratic memory
in the combined node count; the 512-node per-input ceiling bounds both.

Callers that require scientific GED evidence must inspect `capped`. Legacy
weight builders no longer change uncapped `GraphEdit` results; real costs live
in `GedOptions`.

## Verification obligations

- Hungarian results equal exhaustive permutation minima through order seven;
- generated bounds through six nodes are deterministic, symmetric, normalized,
  and no smaller than an exhaustive exact-GED oracle;
- named examples cover empty/identical graphs, relabels, edge-kind changes,
  parallel edges, sparse stable ids, caps, invalid numbers, and overflow;
- public integrations cover feature-free construction and parsed Rust CPGs;
- malformed-CPG robustness exercises a direct reflexive GED call;
- benchmark families assert exact oracles before timing; and
- maximal and feature-free matrices, rustdoc, docs, lints, and staged bug-gate
  validation all pass.

See the [component guide](../components/patterns/ged-analysis.md),
[theory](../theory/06-graph-similarity.md), and
[scientific validation](../scientific/18-ged-validation.md).
