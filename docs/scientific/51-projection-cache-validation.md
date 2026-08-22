# Scientific validation 51 — Lazy deterministic exact-edge projection cache

## Claims under test

The H07 hypothesis is that one lazy, deterministic exact-edge index on a Code
Property Graph (CPG) can make repeated overlay and endpoint traversal
allocation-free without changing graph semantics, public ordering, mutation
behavior, persistence shape, or stack safety.

“Exact edge” means one represented `CpgEdge`, including direction, typed
sub-kind, optional label, stable `EdgeId`, and parallel multiplicity. It is not
the merged endpoint relation stored by the public `GraphProjection`. A
**compressed sparse row** (CSR) index stores row offsets and contiguous raw edge
indices. The cache has forward, reverse, and source-by-target orders so each
query can select one contiguous interval.

The experiment tests five claims:

1. every public cached result equals direct filtering of the authoritative edge
   set;
2. logical order is independent of node/edge insertion order;
3. public mutation invalidates every derived raw index before reuse;
4. warm CFG, DFG, and endpoint reads reduce time and allocation; and
5. construction, high-degree row sorting, lookup, invalidation, and destruction
   remain independent of graph depth on the native stack.

It does not claim that a cold first lookup is free or that retained cache memory
is zero. Both costs are measured explicitly.

## Experimental design

The controlled predecessor is committed H05 (`6e15a0d`): cold node/edge
metadata was compact and the unread edge-id map had been removed, but CPG edge
queries still traversed petgraph directly. Criterion's immutable `main`
baseline measures the complete T9.1-plus-H07 branch outcome. Both comparisons
use the same checked fixtures, one-second warm-up, Criterion bootstrap
analysis, performance-governed CPUs 0–1, and optimized all-feature binaries.
The general protocol is defined in
[scientific validation 04](04-measurement-methodology.md).

The traversal fixture contains one AST, one CFG, and one DFG edge between every
adjacent node pair. Each benchmark performs one query for every node (or
adjacent pair) at 1,024 and 8,192 nodes. Before timing, the fixture oracle checks
all expected layers and endpoints. The corpus benchmark reparses and analyzes
committed approximately 50-, 500-, and 5,000-line Rust programs plus Python,
JavaScript, Go, and C programs; it checks node/edge/function counts, call SCCs,
projection WL labels, and a repeated deterministic summary before timing.

## Isolated H07 result against committed H05

The table reports Criterion's central time-change estimate. `node` and
`ast_children` do not use the new edge cache and serve as controls.

| Operation | 1,024 nodes | 8,192 nodes | Interpretation |
|---|---:|---:|---|
| `node` | +1.07% (noise threshold) | +2.17% | unchanged control surface |
| `ast_children` | +3.86% | -2.74% | maintained H03 child index, not H07 |
| `cfg_successors` | -33.11% | -18.13% | warm layer row; zero result allocation |
| `dfg_successors` | -31.09% | -14.03% | warm layer row; zero result allocation |
| `edges_between` | -45.10% | -45.77% | target-grouped row and binary partitions |

The isolated end-to-end corpus comparison exposes cold cache construction:

| Corpus | H07 time | Change from H05 | Criterion verdict |
|---|---:|---:|---|
| Rust, approximately 50 lines | 397.75 µs | +7.58% | regression |
| Rust, approximately 500 lines | 7.106 ms | +0.56% | no change detected |
| Rust, approximately 5,000 lines | 109.87 ms | -0.37% | no change detected |
| Python + JavaScript + Go + C | 1.102 ms | +4.25% | regression |

These cases construct a new graph and then request call projection/SCC evidence,
so they pay one global cache build. The small cases have too little later work
to amortize it. This is a measured lazy-cache trade-off, not hidden as warm-read
performance.

## Complete branch result against immutable `main`

The full branch includes the preceding T9.1 layout and child-index work. Against
`main`, the targeted hot reads improve in every size class:

| Operation | 1,024-node time | Change | 8,192-node time | Change |
|---|---:|---:|---:|---:|
| `node` control | 2.641 µs | +2.37% | 23.943 µs | +0.36% (noise threshold) |
| `ast_children` | 3.518 µs | -88.95% | 39.574 µs | -85.36% |
| `cfg_successors` | 18.343 µs | -37.71% | 187.51 µs | -18.33% |
| `dfg_successors` | 17.987 µs | -30.83% | 180.37 µs | -18.93% |
| `edges_between` | 9.720 µs | -45.71% | 91.423 µs | -44.99% |

The complete corpus boundary also improves or remains within noise:

| Corpus | Branch time | Change from `main` | Criterion verdict |
|---|---:|---:|---|
| Rust, approximately 50 lines | 383.05 µs | -6.74% | improvement |
| Rust, approximately 500 lines | 7.114 ms | -8.54% | improvement |
| Rust, approximately 5,000 lines | 112.04 ms | -2.63% | noise threshold |
| Python + JavaScript + Go + C | 1.083 ms | -11.01% | improvement |

The isolated and cumulative controls answer different questions. H05-to-H07
shows the cache's cold cost; `main`-to-branch establishes that the shipped
campaign does not regress the end-to-end boundary.

## Allocation, peak memory, and counter experiment

`scripts/profile.sh traversal_primitives` ran ten repetitions headlessly. Its
workload constructs the 8,192-node graph and performs one mixed traversal per
repetition; unlike Criterion, it includes cold construction. DHAT measures
allocation traffic and live heap, Massif includes page accounting, `/usr/bin/time`
reports process RSS, and `perf stat -r 3` reports hardware counters.

| Measure | H05 | H07 | Change |
|---|---:|---:|---:|
| DHAT total allocated bytes | 77,000,155 | 75,364,395 | -2.12% |
| DHAT allocation blocks | 737,604 | 246,274 | -66.61% |
| DHAT peak live bytes | 2,999,920 | 4,310,480 | +43.69% |
| Massif page-accounting peak | 13,053,952 | 14,323,712 | +9.73% |
| maximum RSS | 5,776 KiB | 7,056 KiB | +22.16% |
| cycles | 99,396,514 | 105,750,416 | +6.39% |
| retired instructions | 248,637,765 | 338,333,466 | +36.07% |
| cache references | 4,641,231 | 9,186,947 | +97.94% |
| cache misses | 346,828 | 773,162 | +122.92% |
| page faults | 6,431 | 9,473 | +47.30% |
| dTLB loads | 167,270 | 209,649 | +25.34% |
| dTLB load misses | 20,900 | 29,410 | +40.72% |

The allocation-count reduction validates the zero-allocation warm accessors:
the predecessor allocated vectors on every CFG, DFG, and endpoint query. The
peak and counter increases falsify a stronger “free index” claim. They arise
because this profile constructs three deterministic edge orders and performs
only one read sweep before dropping the graph. For repeated reads, Criterion
shows the benefit after construction. Cache references/misses and dTLB misses
are reported without a causal claim; run-to-run variation and multiplexing make
them secondary evidence.

The `CodePropertyGraph` language-feature layout grows from 184 to 200 bytes for
the `OnceLock<Box<_>>`, while remaining below the 216-byte pre-H05 graph. The
cache's edge arrays account for the retained live-heap increase; edge payloads
themselves are not copied.

Perf recording used the headless DWARF fallback because last-branch recording
was unavailable. Its sampled hot paths include cache construction, row sorting,
and layer partitioning, consistent with the cold workload. Intel VTune ran only
through its command-line interface and could not collect because host
`ptrace_scope` forbids it. No GUI profiler or heaptrack process was started.

## Semantic and determinism oracles

One 256-case property builds bounded graphs with AST, CFG, DFG, PDG, call, and
other edges, including parallel endpoints. For every generated node and every
source/target pair it compares:

- cached outgoing and incoming exact edges with direct edge filtering;
- cached CFG/DFG successors and predecessors with direct typed filtering;
- each crate-private overlay iterator with the corresponding edge-kind
  predicate; and
- `edges_between` with the exact directed endpoint multiset.

A separate 256-case property constructs the same logical graph under different
node and edge insertion orders and requires identical stable logical sequences.
Example tests pin unknown-node empty views, forward/backward iterator cursors,
parallel edge-id order, all six layer classes, cache invalidation after node and
edge changes, graph cloning, serde omission, and legacy input without a cache
field.

The public parser integration repeats the direct-filter oracle after Rust
parsing, call resolution, and PDG construction. This crosses the actual
mutation-to-analysis boundary and checks every public cached accessor rather
than only a synthetic shape. Existing projection, SCC, PDG, and VF2 oracle
suites remain unchanged and consume the cache through production paths.

## Root-cause experiments and rejected variants

Instrumentation during development found more than 64 cache rebuilds at a
19,691-node/49,000-edge boundary. Backtraces identified mutation-oriented call
resolution and DFG parameter/return construction reading cached global
adjacency immediately before adding edges. Moving those exact checks to
crate-private native petgraph adjacency removed all cache builds from the
mutation phase. This repairs the phase-boundary root cause; it does not disable
the cache for stable SCC/projection consumers.

Two alternatives were measured and rejected:

- dereferencing edge payloads inside endpoint binary-search comparisons made
  the hot `edges_between` path materially slower, so aligned target ids are
  retained; and
- a whole-edge owner-index scan for call SCCs regressed the 10,000-node path by
  about 12.5%, so stable SCC reads use the shared cache.

Skipping row sorting when degree is zero or one is a semantics-preserving
construction improvement, not a special-case shortcut: such rows have exactly
one possible order.

## Stack-safety experiment

An exact example constructs 100,000 nodes, a 99,999-edge CFG path, and a
99,999-edge DFG star from the root. A worker with a 256 KiB native stack:

1. builds all forward, reverse, and endpoint CSR orders;
2. binary-partitions the high-degree DFG row;
3. walks the full CFG path through cached successor views;
4. resolves an endpoint slice; and
5. drops the graph and cache before the worker returns.

The test completes in approximately 0.21 seconds on the recorded host. The
high-degree star specifically exercises library sorting at depth 99,999; the
long path exercises graph depth. Neither dimension consumes recursive native
frames.

## Feature and gate matrix

| Command family | Evidence |
|---|---|
| focused `graph::cpg` tests | exact examples, 512 property campaigns, mutation, serde, and depth/high-degree stack gate |
| `cargo test --all-features --test integration` | parser/call/PDG direct-filter oracle |
| `cargo test --all-features analysis::scc --lib` | cached stable call/CFG SCC consumers |
| `cargo test --all-features analysis::projection --lib` | owned projection over cached exact edges |
| `cargo test --all-features pattern::vf2 --lib` | exact endpoint feasibility over cached rows |
| `cargo bench --all-features --bench traversal_primitives -- --baseline main` | warm read time with checked fixture |
| `cargo bench --all-features --bench analyze_corpus -- --baseline main` | complete parser/analysis boundary |
| `scripts/profile.sh traversal_primitives` | headless allocation, peak, counter, and hot-path evidence |
| `cargo clippy --all-targets --all-features -- -D warnings` | warning-free complete target surface |
| `./scripts/verify.sh` | format, feature matrices, tests, examples, rustdoc, docs, and diff hygiene |

## Falsification boundaries

The tests establish equality for public graph mutation paths and finite graphs
admitted by `u32` index conversion. They do not bless direct corruption of
private petgraph storage, promise stable Rust struct layout, or make
`GraphProjection` preserve parallel payloads. A caller that performs only one
small cached query can observe cold-build overhead and retained memory; lazy
construction avoids charging callers that never request such a view, but it
cannot eliminate the cost once requested. Applications admitting hostile
hand-built graphs must still bound aggregate nodes and edges before triggering
the cache.

## References

1. Barrett, R., Berry, M., Chan, T. F., Demmel, J., Donato, J., Dongarra, J.,
   Eijkhout, V., Pozo, R., Romine, C., and van der Vorst, H. (1994).
   *Templates for the Solution of Linear Systems: Building Blocks for Iterative
   Methods* (2nd ed.). SIAM. DOI:
   [10.1137/1.9781611971538](https://doi.org/10.1137/1.9781611971538).
