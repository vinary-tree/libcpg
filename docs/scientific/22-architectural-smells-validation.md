# Architectural-smell validation

This chapter records the falsifiable claims, oracle design, generated tests,
coverage, and interpretation limits for `architecture_smells`. The analysis is
advisory, but its structural and numerical implementation is tested exactly.

## Claims and oracles

| Claim | Independent fixture | Required observation |
|---|---|---|
| Multi-node SCCs are cyclic smells. | Two-node reciprocal cycle with incoming/outgoing external edges. | One SCC finding, exact members, size `2`, internal-edge count `2`. |
| Hubs require high fan-in and fan-out. | Seven-node two-sided star. | Center alone; fan evidence `3`/`3`; both z-scores above two. |
| Less-stable dependency share follows Martin direction. | Stable middle module with one edge to a less-stable target. | One unstable finding; share `1`; source instability `0.25`. |
| God component uses module node-count z-score. | Module sizes `4,1,1,1,1`. | Four-node module alone; z-score above configured `1`. |
| Clean topology is not overreported. | Three-node chain and singleton self-loop. | No findings at default thresholds. |

Each fixture isolates one detector by choosing module boundaries that suppress
the other three. This prevents a test from merely checking that *some* smell
was returned.

## Property-based experiment

The 256-case generator selects up to 18 sparse stable node identifiers, up to
160 raw weighted edges, and five possible module buckets. It constructs the
same logical graph twice with reversed node, edge, and assignment input order.
For every case it asserts:

```text
findings(original order) = findings(reversed order)
every finding node belongs to the projection
every finding node vector is non-empty, sorted, and unique
every severity and evidence value is finite
the result vector obeys the documented canonical comparator
```

The generator includes empty graphs, self-loops, parallel edges with different
weights, dense SCCs, isolated vertices, singleton/multi-node modules, and
threshold-adjacent natural outcomes. Proptest shrinking produced an early
empty-graph counterexample to an eager test-fixture filter; the generator was
corrected to filter before indexing, and no regression seed was retained
because the minimized case is now part of the ordinary domain.

## Hostile public inputs

Example tests require empty output, without panic or mutation, for:

- malformed forward CSR;
- an SCC decomposition from a different projection;
- missing or out-of-range module assignment for a projected node;
- missing or duplicate Martin modules;
- non-finite and out-of-range instability, abstractness, and distance;
- non-finite, negative, or out-of-range option fields.

An invalid assignment entry outside the projection is explicitly inert. This
distinguishes an irrelevant whole-CPG mapping from missing evidence inside the
selected analysis boundary.

## Feature and serialization matrix

The root integration test constructs seven `Module` CPG nodes and six `Imports`
edges, then runs:

```text
CodePropertyGraph
  -> module_graph_projection
  -> projection_sccs
  -> martin_metrics
  -> architecture_smells
```

It passes with `--no-default-features`, proving the engine is feature-free and
read-only. The all-feature test verifies stable JSON kind and evidence names.
Unit serde tests round-trip every public type and reject malformed/unknown
evidence keys.

## Coverage checkpoint

After hostile-path strengthening, `analysis/smells.rs` records **100% line,
region, and function coverage** in the maximal stable run. The nightly branch
run records **96.97% branch coverage** (64 of 66 branch outcomes); the only two
uncovered outcomes are the deliberately failing sides of an internal
`debug_assert!` precondition for positive z-scores. Coverage is evidence of
executed structure, not proof that the smell definitions are universally
predictive.

## Threats to validity

- **Construct validity:** node count is a portable component-size proxy, not
  lines of code. A caller needing LOC-based policy must filter or enrich
  findings outside this API.
- **Threshold validity:** defaults are review heuristics, not universal defect
  boundaries. Sensitivity analysis should vary `SmellOptions`.
- **Projection validity:** results describe the selected dependency overlay;
  unresolved call edges or incomplete import extraction reduce recall.
- **Ground truth:** synthetic oracles prove implementation semantics, not that
  maintainers judge every emitted smell harmful.
- **Instability meaning:** Martin instability is coupling direction, not code
  churn or runtime failure probability.

The correct empirical follow-up is to compare findings against blinded
maintainer judgments and report precision/recall with confidence intervals,
stratified by projection type and project size.

## Reproduction

```bash
cargo test --all-features analysis::smells::tests
cargo test --no-default-features --test smells_integration
cargo test --all-features --test smells_integration
cargo cov-text
cargo +nightly cov-branch
./scripts/verify.sh
```

See [Theory 21](../theory/21-advisory-architectural-smells.md), [component
guide](../components/graph/architecture-smells.md), and [ADR
0027](../design/0027-validated-advisory-architectural-smells.md).

## References

1. Fontana, F. A. et al. (2017). *Arcan: A Tool for Architectural Smells
   Detection*. DOI:
   [10.1109/ICSAW.2017.16](https://doi.org/10.1109/ICSAW.2017.16).
2. Fontana, F. A. et al. (2016). *Automatic Detection of Instability
   Architectural Smells*. DOI:
   [10.1109/ICSME.2016.33](https://doi.org/10.1109/ICSME.2016.33).
