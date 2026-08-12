# Validation of bounded simple-cycle enumeration

This page records the hypotheses, independent oracles, adversarial fixtures,
and integration evidence for `libcpg::simple_cycles`. The central scientific
question is not merely whether known cycles are found, but whether the bounded
result equals the mathematically defined canonical prefix for arbitrary small
directed graphs.

## Hypotheses

1. **Completeness within bounds.** With a nonbinding count cap, the result is
   exactly every elementary directed cycle whose length is within the effective
   length bound.
2. **Canonical uniqueness.** Cyclic rotations occur once with the smallest
   stable identifier first; reverse orientations remain distinct.
3. **Determinism.** Repeated runs and reversed edge insertion produce identical
   `CycleSet` values.
4. **Truthful truncation.** A binding count cap returns the sorted canonical
   prefix and the immediately following cycle as `first_dropped_cycle`.
5. **Safety.** Malformed public CSR is rejected, long graphs do not recurse on
   the native stack, and the input projection is unchanged.

## Independent exhaustive oracle

The property oracle does not call SCC analysis, production canonicalization, or
the production DFS. For each generated graph with at most eight vertices it:

1. chooses every candidate length;
2. chooses each possible minimum start;
3. generates every permutation of the remaining distinct vertices greater
   than that start;
4. verifies every internal edge and the closing edge directly in CSR; and
5. stores successful stable-identifier sequences in an independent `BTreeSet`.

The oracle result is then sorted by `(length, lexicographic sequence)` and
compared for exact equality. Directed adjacency comes from a generated 64-bit
mask, which covers every possible edge, including self-loops, for eight
vertices. Ninety-six cases exercise full equality; another ninety-six exercise
random count caps and exact prefix/first-omission behavior.

This oracle initially contained a defect: it checked the closing edge but not
each internal permutation edge. Proptest minimized two counterexamples. The
oracle was corrected to check every consecutive pair, and both seeds remain in
`proptest-regressions/analysis/cycles.txt` so the distinction between a vertex
permutation and a cycle stays regression-locked.

## Example-based fixtures

| Fixture | Expected observation |
|---|---|
| self-loop + reciprocal pair + triangle | canonical lengths one, two, and three in order |
| bidirectional triangle | two length-three orientations, not one undirected cycle |
| acyclic tail beside a cyclic SCC | only cycles inside the SCC |
| length cap two | the triangle is excluded without count truncation |
| count cap two | two retained cycles plus the triangle as first omitted witness |
| count cap zero | empty retained prefix plus the first admissible cycle |
| malformed target index | typed `InvalidInput`, no indexed panic |
| 33-node ring | excluded by the hard length ceiling of 32 |
| 25,000-node ring | stack-safe completion with no bounded cycle |

Every example additionally verifies that each returned sequence is nonempty,
contains distinct nodes, starts with its minimum, and has every required edge,
including its closing edge.

## Integration evidence

The feature-free integration builds an external projection with sparse stable
identifiers `10`, `30`, `70`, and `90`. It observes exactly a reciprocal pair
and a triangle, checks root-crate re-exports, repeats the analysis, and compares
the projection before and after.

With `lang-rust`, a parser-built call graph contains mutually recursive
`alpha`/`beta` functions plus an isolated `leaf`. Import resolution produces
exactly one two-function cycle. This crosses source parsing, call resolution,
projection construction, SCC pruning, cycle enumeration, and public output
without mutating the CPG.

The shared malformed-input robustness corpus now runs cycle enumeration over
every corrupt CPG fixture after constructing a valid call projection. It checks
node membership and all public bounds alongside the rest of the analysis
surface.

## Benchmark oracles

`benches/cycles.rs` contains six smokeable Criterion cases:

- acyclic paths at 1,024 and 8,192 nodes;
- disjoint directed triangles at 1,023 and 8,190 nodes; and
- complete directed graphs at 8 and 10 nodes with a binding 1,000-cycle cap.

Every case executes an oracle before timing. Complete cases assert the exact
cycle count and every closing edge. Dense cases assert a full retained prefix,
`truncated = true`, a dropped lower bound of one, and a concrete omitted
witness. The benchmarks therefore cannot report performance for an incorrect
answer.

## Falsification results and retained fixes

The 25,000-node ring exposed repeated graph-sized membership allocation and a
whole-array debug assertion inside each start search. Reusing one scratchpad and
using constant-time postconditions removed that accidental cost.

Dense benchmark review exposed a second boundary: applying `max_cycles` only
after a whole start/length search allowed the local discovery buffer to exceed
the public cap. The limit now enters the DFS itself as remaining capacity plus
one witness. These are retained architectural fixes, not benchmark-only
special cases.

## Measured coverage

The clean maximal-feature coverage matrix measures `analysis/cycles.rs` at
**100.00%** line coverage (460/460), **100.00%** function coverage (40/40),
**100.00%** region coverage (730/730), and **100.00%** branch coverage (50/50).
The stable line/region run and nightly branch run both execute the complete
repository suite; these are not focused-test figures.

Across the same matrix, the repository measures **98.10%** line, **98.33%**
function, and **98.02%** region coverage on stable. The nightly run measures
**86.30%** branch coverage repository-wide.

## Reference

- Johnson, D. B. (1975). *Finding All the Elementary Circuits of a Directed
  Graph.* SIAM Journal on Computing 4(1), 77–84. DOI:
  [10.1137/0204007](https://doi.org/10.1137/0204007).
