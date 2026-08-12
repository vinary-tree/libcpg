# Scientific validation — bipartite graph-edit upper bound

This record separates the mathematical guarantees tested by the suite from
the heuristic role of node-star assignment.

## Claims under test

1. The in-crate Hungarian solver minimizes every supplied finite square cost
   matrix and resolves equal costs deterministically.
2. Every uncapped value is the cost of a concrete edit script and is therefore
   no smaller than exact graph-edit distance (GED).
3. The public bound is symmetric, deterministic, normalized to $`[0,1]`$, and
   zero for identical graphs.
4. Node tags, complete directed edge kinds, self-loops, and parallel
   multiplicity have their documented edit semantics.
5. Invalid numeric options fail explicitly, while budget fallback is visibly
   marked with `capped = true`.
6. Whole-CPG Weisfeiler–Lehman similarity uses fixed cross-release hash
   constants rather than standard-library randomized/versioned hashing.

## Independent assignment oracle

A 128-case property test generates square matrices through order seven with
decimal costs. The production primal-dual Hungarian result is compared with a
recursive enumeration of every column permutation. The oracle also checks
that the output is itself a permutation. A named all-zero matrix requires the
identity assignment, exercising the smallest-column tie rule.

## Independent exact-GED oracle

A second 128-case property generates pairs of typed directed graphs through
six nodes from arbitrary label and adjacency masks. The oracle enumerates
every partial injective mapping: each source node may be deleted or mapped to
one unused target node. At each complete mapping it independently prices
unmatched nodes, unequal node tags, and directed-edge multiset differences.

If $`d^*`$ is the minimum oracle value and $`U`$ is the production result, each
case checks

```math
d^* \le U \le U_0,
```

where $`U_0`$ is the delete-all/insert-all script. It also checks the reverse
orientation, a repeated call, the uncapped marker, and the normalized range.
This oracle validates the bound claim; it does not claim that bipartite
assignment always finds exact GED.

## Example and integration evidence

- empty and identical graphs return exact zero;
- a fixture with one relabel and three typed edge edits costs `2.5`;
- reversed graph storage and sparse `NodeId` values do not change results;
- configured and hard caps return the documented legacy value with
  `capped = true`;
- NaN, infinity, negative costs, normalization overflow, and assignment-entry
  overflow all return `AnalysisError::InvalidInput`;
- serde options and results round-trip when the feature is enabled;
- a feature-free public integration ties `GraphEdit` exactly to
  `1 - GedBound::normalized`;
- parsed Rust functions with `if` versus `while` structure have nonzero GED
  without mutating either CPG; and
- malformed CPG variants remain total and reflexively zero.

## Stable WL migration evidence

The legacy similarity helper formerly used `DefaultHasher`. A named regression
now asserts all six exact 64-bit histogram keys and counts produced by a
two-node graph over three rounds. The combiner is shared with projection WL,
whose own fixed-vector tests validate its SplitMix-style constants.

## Benchmark validity

The Criterion target executes six cases: identical paths, fully relabeled
isolates, and paths whose edge kind changes, each at 16 and 48 nodes. Before
timing, every fixture asserts its exact expected script cost:

```math
U_{\mathrm{identical}}=0, \qquad
U_{\mathrm{relabel}}=n, \qquad
U_{\mathrm{retype}}=n-1.
```

The benchmark measures this implementation's scaling; it is not evidence that
the heuristic correspondence is optimal on arbitrary graphs.

## Reproducibility

The focused evidence is feature-free and maximal-matrix compatible:

```bash
cargo test --no-default-features pattern::ged
cargo test --all-features pattern::ged
cargo test --all-features pattern::similarity
cargo test --no-default-features --test ged_integration
cargo test --all-features --test ged_integration
cargo test --all-features --test robustness
cargo bench --no-default-features --bench ged -- --test
```

Repository coverage is measured after the semantic oracles pass and recorded
in the [testing guide](../engineering/02-testing.md). In the clean maximal
matrix, `src/pattern/ged.rs` measures **99.83%** line, **99.28%** region,
**100.00%** function, and **93.33%** branch coverage. The corresponding
repository totals are **98.17%** line, **98.08%** region, **98.39%** function,
and **86.71%** branch coverage. Coverage demonstrates that implementation paths
ran; exhaustive assignment and exact-GED comparison provide the stronger
correctness evidence.

See the [theory chapter](../theory/06-graph-similarity.md), [component
guide](../components/patterns/ged-analysis.md), and
[ADR 0023](../design/0023-deterministic-bipartite-ged.md).

## References

1. Riesen, K., Bunke, H. (2009). *Approximate Graph Edit Distance Computation by Means of Bipartite Graph Matching.* Image and Vision Computing 27(7), 950–959. DOI: [10.1016/j.imavis.2008.04.004](https://doi.org/10.1016/j.imavis.2008.04.004)
2. Kuhn, H. W. (1955). *The Hungarian Method for the Assignment Problem.* Naval Research Logistics Quarterly 2(1–2), 83–97. DOI: [10.1002/nav.3800020109](https://doi.org/10.1002/nav.3800020109)
