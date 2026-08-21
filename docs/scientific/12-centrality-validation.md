# Scientific validation 12: centrality analysis

This document states the claims, oracles, generated properties, and benchmark
protocol for libcpg's projection centrality family. It distinguishes evidence
already checked by the repository from interpretations the implementation does
not establish.

## Scope and hypotheses

The subject is `src/analysis/centrality.rs` and its public crate-root exports.
The validation hypotheses are:

- **H1 — alignment:** every successful scalar result has exactly one score per
  projection node in `graph.nodes` order;
- **H2 — fixed examples:** PageRank, betweenness, spectral, distance, HITS, and
  Burt outputs match analytically derived small-graph values;
- **H3 — differential agreement:** PageRank agrees with petgraph on a directed
  ring, and the shared projection dominator implementation agrees with
  `petgraph::algo::dominators::simple_fast` under generated graphs;
- **H4 — Brandes correctness:** directed vertex betweenness agrees with a naive
  enumeration of all simple source-to-target paths on fixed and generated
  graphs, and layer rescaling remains finite beyond the `f64` path-count range;
- **H5 — composition:** summing two disjoint normalized
  `accumulate_for_sources` results reproduces all-source Brandes scores;
- **H6 — determinism:** identical calls return bit-identical results, and an
  isomorphic reversal of stable identifiers returns corresponding scores
  within `1e-9`;
- **H7 — total boundaries:** empty, disconnected, malformed, unknown-node,
  invalid-option, zero-iteration, and over-budget inputs terminate without
  panic and expose the documented state;
- **H8 — immutability:** the public pipeline leaves its input projection byte-
  for-byte equal to a pre-analysis clone; and
- **H9 — benchmark validity:** every timed path, ring, and clustered case runs
  a deterministic oracle before Criterion begins measurement.

## Example-based oracles

| Fixture | Expected evidence |
|---|---|
| directed 4-ring | PageRank is exactly `[0.25; 4]`, sums to one, and agrees with petgraph within `1e-8` |
| directed graph and transpose | reverse PageRank score vector equals PageRank of `graph.transpose()` |
| personalized 3-path | seed mass is L1-normalized; duplicate seeds combine; all-zero distinct seeds become uniform |
| directed 6-node diamond/path graph | Brandes scores match naive all-simple-path shortest-path fractions |
| 1,100-layer binary-width directed graph | vertex and edge Brandes remain finite after the number of shortest paths exceeds the ordinary `f64` range; symmetric layer vertices remain equal |
| undirected 3-path | both canonical edge-betweenness values equal 2 |
| 4-star | eigenvector center is $`1/\sqrt{2}`$, leaves are $`1/\sqrt{6}`$; Katz preserves leaf symmetry |
| undirected 3-path | closeness is `[2/3, 1, 2/3]`; harmonic is `[3/4, 1, 3/4]` |
| outward 4-star | HITS hub vector is `[1,0,0,0]`; leaf authority scores are $`1/\sqrt{3}`$ |
| undirected 3-path | Burt constraint is `[1, 1/2, 1]` |
| undirected triangle | every Burt constraint equals $`9/8`$ |

The naive betweenness oracle recursively enumerates simple paths rather than
using Brandes dependency accumulation. For each ordered source/target pair it
selects the shortest enumerated length and divides credit by the number of
shortest paths. The oracle is intentionally limited to a small fixture because
simple-path enumeration is exponential. The deep layered fixture exercises a
different boundary: its independent symmetry/finiteness oracle would fail if
raw path counts reached infinity and dependency accumulation formed `inf/inf`.

## Property-based campaign

The centrality module generates 128 directed topologies with one to six
vertices. Self-loops are excluded from this property so the isomorphic
relabeling relation is easy to inspect; dedicated examples test other boundary
behavior. Each topology is rebuilt with the stable identifiers reversed and
offset into a disjoint numeric range.

For PageRank, personalized PageRank, reverse PageRank, eigenvector, Katz,
closeness, harmonic, both HITS vectors, Burt constraint, vertex betweenness,
and edge betweenness, the property checks:

1. two calls on the original projection return exactly equal structs;
2. reversed vertex and edge input order returns the exact same structs;
3. the original and relabeled node or canonical-edge sets map bijectively;
4. corresponding scores differ by at most `1e-9`; and
5. every fallible function stays inside the default work budget.

The crate's existing projection properties independently permute edge input,
add duplicate edges, and establish canonical CSR construction. Dominator
properties cover 256 generated directed graphs against petgraph's
`simple_fast` oracle.

## Integration and robustness

`tests/centrality_integration.rs` imports only crate-root APIs in a
`--no-default-features` build. It executes every centrality family plus
projection dominators on one public external graph, verifies alignment and
finite scores, checks canonical edge order, and compares the graph with its
pre-analysis clone. Unknown nodes and caller-corrupted reverse CSR are rejected
without panic.

`tests/robustness.rs` now applies the full centrality family to call projections
derived from ten malformed-CPG classes and 64 generated corruption cases. The
robustness claim is termination and structural alignment, not meaningful
centrality for a semantically corrupt source graph.

## Benchmark design

`benches/centrality.rs` defines two Criterion groups:

- `centrality_iterative` measures PageRank and shifted eigenvector iteration on
  path, ring, and clustered projections at 256 and 1,024 vertices;
- `centrality_all_sources` measures directed Brandes and undirected closeness
  on the same three shapes at 128 and 256 vertices.

Before registration, the iterative oracle checks PageRank convergence, mass,
alignment, ring uniformity, eigenvector finiteness/L2 normalization, and HITS
alignment/finiteness. It does not require long-path eigenvector convergence:
the spectral gap narrows as the path grows, so convergence within a particular
caller cap is not a correctness invariant. The traversal oracle checks finite
complete results, path-center ordering, ring symmetry, canonical edge output,
cluster-bridge maximal edge betweenness, and finite Burt constraint.

### Reproduction

Smoke the benchmark oracles without collecting a performance distribution:

```bash
cargo bench --bench centrality --no-default-features -- --test
```

For measurements, pin a physical CPU and select the performance governor as
described in [measurement methodology](04-measurement-methodology.md):

```bash
sudo cpupower frequency-set --governor performance
mkdir -p target/verification
taskset -c 2 cargo bench --bench centrality --no-default-features 2>&1 \
  | tee target/verification/libcpg-centrality-bench.log
```

Record processor, governor, Rust version, commit, feature set, thermal state,
and sample configuration with any reported numbers, inspect the captured log,
and remove that exact file when the investigation is complete. The committed
benchmark contains no claimed speedup; it is a reproducible measurement
instrument for the later optimization tranche.

## Falsification criteria

Reject H1 if a valid nonempty scalar result is not aligned. Reject H2–H5 if an
oracle differs beyond its stated tolerance. Reject H6 if an exact repeat
changes bits or a relabeling exceeds `1e-9`. Reject H7 if a boundary panics,
indexes malformed CSR, silently exceeds a budget, or presents an invalid
option as converged. Reject H8 if the projection mutates. Reject H9 if a timed
benchmark can begin before its shape oracle passes.

## Current evidence and limitations

A clean maximal-matrix line/region/function coverage run and a separate clean
nightly branch-coverage run each executed **679 library tests** and **45 public
integration and robustness tests**, including both centrality integration
cases. `analysis/centrality.rs` measured **98.32% line**, **97.47% region**,
**100.00% function**, and **88.95% branch** coverage, above the campaign's 90%
line and 80% branch thresholds. These are whole-repository results, not a
focused-test extrapolation.

The complete default, maximal, and all-feature test matrices; example target;
Clippy with warnings denied; rustdoc with warnings denied; documentation,
diagram, math-delimiter, and diff checks all pass. The feature-free focused
suite passes all 14 centrality unit/property tests, both public integration
tests, and the malformed-input harness. All 24 path/ring/clustered benchmark
smoke cases pass their pre-timing oracles.

The campaign does not validate centrality against runtime profiles, prove call
resolution completeness, establish a universally correct projection choice,
or claim that a score causes defects or architectural risk. Floating scores
are only comparable when kind, projection provenance, orientation, weights,
options, and convergence state match.

## References

1. Brandes, U. (2001). *A Faster Algorithm for Betweenness Centrality.*
   [DOI 10.1080/0022250X.2001.9990249](https://doi.org/10.1080/0022250X.2001.9990249).
2. Brin, S., and Page, L. (1998). *The Anatomy of a Large-Scale Hypertextual
   Web Search Engine.*
   [DOI 10.1016/S0169-7552(98)00110-X](https://doi.org/10.1016/S0169-7552(98)00110-X).
3. Girvan, M., and Newman, M. E. J. (2002). *Community Structure in Social and
   Biological Networks.*
   [DOI 10.1073/pnas.122653799](https://doi.org/10.1073/pnas.122653799).
4. Marchiori, M., and Latora, V. (2000). *Harmony in the Small-World.*
   [DOI 10.1016/S0378-4371(00)00311-3](https://doi.org/10.1016/S0378-4371(00)00311-3).
