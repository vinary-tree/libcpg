# Scientific validation — directed motif census

This record distinguishes algorithmic claims supported by tests from possible
architectural interpretations of motif frequency.

## Claims under test

1. Every unordered triple is assigned to exactly one of the sixteen
   Davis–Leinhardt classes.
2. Directionally distinct `111D`/`111U` and
   `120D`/`120U`/`120C` classes remain distinct.
3. The two selected graphlet predicates implement their documented topology
   contract exactly.
4. Results do not depend on edge insertion order, weights, self-loops, or
   repeated evaluation.
5. Invalid CSR and computations beyond either resource budget fail explicitly
   rather than returning partial evidence.

## Independent triad oracle

The production classifier uses a 64-entry table. Its test oracle does not
duplicate that table. Instead, the test suite stores one representative
six-bit graph for each class, generates all six permutations of three vertex
labels, and asks whether each of the 64 possible labeled tricodes is isomorphic
to a representative. Every production table entry must equal that independently
identified class.

The following named fixtures additionally make orientation failures legible:

| Classes | Evidence |
|---|---|
| `003`, `012`, `102` | empty, one-arc, and mutual-pair examples |
| `021D`, `021U`, `021C` | fan-out, fan-in, and directed-chain examples |
| `111D`, `111U` | the asymmetric arc points into or out of the mutual dyad |
| `030T`, `030C` | transitive triangle and directed cycle |
| `201` | two mutual dyads sharing one vertex |
| `120D`, `120U`, `120C` | down, up, and cyclic orientations around a mutual dyad |
| `210`, `300` | five-arc and complete bidirected examples |

## Generated-graph oracle

A 128-case property test generates directed graphs through eight vertices from
arbitrary 64-bit masks, including possible self-loops. An independent nested-
combination implementation classifies every triple by the permutation oracle
and every quadruple by direct edge predicates. It must equal the complete
production `MotifCensus`.

For every generated graph, the test also checks

```math
\sum_i \mathtt{triads}[i] = {n \choose 3},
```

then repeats the census and rebuilds the same projection with reversed edge
insertion order. All three results must be byte-for-byte equal.

## Example and integration evidence

- A bidirected four-clique produces four `300` triads and one clique graphlet.
- A four-node out-star produces three `021D` triads, one `003` leaf triple,
  and one out-star graphlet.
- Reciprocal spoke arcs leave the source-compatible out-star count unchanged.
- Empty graphs, one/two-node graphs, weights including infinity and NaN, and
  self-loops preserve the documented topology semantics.
- Smaller caller budgets, the 50/512 hard limits, and malformed CSR each return
  the expected typed error.
- A feature-free external projection with sparse `NodeId` values produces
  three `012` triads and one `030T` triad without mutation.
- A parsed Rust call DAG `alpha → beta`, `alpha → gamma`, `beta → gamma`
  produces one `030T` triad and leaves the CPG unchanged.
- The general malformed-input robustness harness now composes motif analysis
  with call-graph projection and checks the conservation law.

## Benchmark validity

The Criterion target times six admitted cases: directed paths and out-stars at
24/50 vertices, and complete bidirected graphs at 16/30 vertices. Before a
single sample is timed, each family checks exact deterministic oracles:

- all families conserve `C(n,3)` triads;
- complete graphs contain `C(n,3)` `300` triads and `C(n,4)` clique graphlets;
- an out-star with `$\ell$` leaves contains `C(\ell,3)` `003` triads,
  `C(\ell,2)` `021D` triads, and `C(\ell,3)` out-star graphlets; and
- directed paths contain neither selected four-node graphlet.

These benchmarks measure implementation behavior; they do not establish a
universal performance comparison with sparse census algorithms.

## Defects exposed during validation

The exhaustive class evidence made the upstream `111D`/`111U` and
`120D`/`120U`/`120C` collapses impossible to preserve. Benchmark smoke testing
also exposed a defective `choose(n, 2)` benchmark oracle before timing; the
production out-star result was correct. Bibliographic verification corrected
the Holland–Leinhardt article pages to 411–432.

## Coverage and reproducibility

The motif module is exercised in the feature-free and maximal feature
matrices, the two public integrations, the robustness suite, and six benchmark
smoke cases. On the same source revision, clean maximal-matrix `cargo cov-text`
and nightly `cargo +nightly cov-branch` runs measured **100.00% line**, **100.00%
region**, **100.00% function**, and **100.00% branch** coverage for
`analysis/motifs.rs`. Repository-wide totals were 98.12% line, 98.05% region,
98.36% function on stable and 86.44% branch on nightly. These measurements are
evidence of exercised implementation paths, not a replacement for the
independent semantic oracles above. The repository-wide protocol is in the
[testing guide](../engineering/02-testing.md).

Reproduce the focused evidence with:

```bash
cargo test --no-default-features analysis::motifs
cargo test --all-features analysis::motifs
cargo test --all-features --test motifs_integration
cargo test --all-features --test robustness
cargo bench --no-default-features --bench motifs -- --test
```

See the [theory chapter](../theory/17-directed-motif-census.md), [component
guide](../components/graph/motif-analysis.md), and
[ADR 0022](../design/0022-deterministic-motif-census.md).
