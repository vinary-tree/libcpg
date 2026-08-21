# Optimization Ledger

This ledger is the durable evidence boundary for `libcpg` performance work. It
records the pre-optimization baseline established by epic task T9.0, the
workloads that produced it, the ranked hot paths, and the experiments that the
evidence permits next. It does **not** claim that one machine predicts another,
that one fixture predicts arbitrary programs, or that a profiler sample proves
an optimization correct.

The baseline commit is `621bbd5c5bbdbce66771165c5eb15af583269b58` on
`feature/libcpg-ax-epic`. No production source under `src/` changed while this
baseline was established.

## Terms and interpretation

Criterion reports a point estimate of the arithmetic mean, $`\mu`$, and a
point estimate of the sample standard deviation, $`\sigma`$. Every timing below
is written as $`\mu \pm \sigma`$. It is a measurement of the named fixture on
the recorded host, not an asymptotic bound. `Throughput::Elements` is always the
number of CPG or projection nodes processed by the timed operation.

**Resident set size (RSS)** is the maximum physical-memory footprint reported
by `/usr/bin/time -v`. **DHAT** is Valgrind's Dynamic Heap Analysis Tool; its
"blocks" count is the number of heap allocations over one complete workload
process, including fixture construction performed by that profile driver.
**Massif** is Valgrind's heap/page profiler. **LBR** means a processor's Last
Branch Record call-chain facility; **DWARF** is the debug-unwind fallback used
when LBR cannot open a sampling event.

## Controlled environment

| Variable | Recorded value |
| --- | --- |
| Local measurement date | 2026-08-20, America/New_York |
| Processor | AMD Ryzen Threadripper PRO 5975WX, Zen 3, 32 physical cores |
| Affinity | CPUs 0 and 1; distinct physical cores, same L3/CCD id 0 |
| Cache | 1 MiB L1d, 1 MiB L1i, 16 MiB L2, 128 MiB L3 total |
| Memory | 131,724,488 KiB installed |
| Governor | `performance` on the selected CPUs |
| Kernel | Linux 7.1.6-arch1-1, x86-64 |
| Rust | `rustc 1.97.1 (8bab26f4f 2026-07-14)`, LLVM 22.1.6 |
| Cargo | `cargo 1.97.1 (c980f4866 2026-06-30)` |
| Criterion | 0.5.1 |
| Profilers | perf from the host kernel tools; Valgrind 3.25.1; Intel VTune Profiler 2025.8.0 CLI |
| Cargo features | `lang-rust,lang-python,lang-javascript,lang-go,lang-c` |

CPU 0 and CPU 1 both report L3 cache id 0 through `lscpu -e`; the workload is
single-threaded today, but the two-core mask leaves one same-CCD core available
for future internal parallelism without moving the process across an L3
boundary.

## Workloads and correctness oracles

Every Criterion target checks its result before entering the timed loop. The
shared fixture integration test also exercises the generators with examples and
property-based tests.

| Target | Timed work | Pre-timing oracle |
| --- | --- | --- |
| `cpg_construction` | Arena construction at 1,000/10,000 nodes; parser-backed Rust at about 50/500 lines | Exact synthetic node/edge/adjacency counts; parsed graphs contain nodes and edges |
| `traversal_primitives` | `node`, `ast_children`, `cfg_successors`, `dfg_successors`, and `edges_between` at 1,024/8,192 nodes | Exact three-overlay adjacency and edge multiplicity for every path step |
| `dfg_extraction` | DFG extraction on parser-built, DFG-free Rust templates | Input has no DFG edges; output has represented data-flow evidence |
| `pdg_construction` | PDGs for every function in parser-built Rust templates | Functions exist, input has no PDG edges, output has dependence evidence |
| `scc_analysis` | Path, ring, and eight-node clustered call graphs at 1,000/10,000 functions | Exact partition node and component counts |
| `pattern_vf2` | Bounded VF2 matching of a four-node path into 256/4,096 nodes | Exactly one retained match |
| `similarity_wl` | Four-round WL labels and clone classes on 1,024/8,192-node rings | Exact label cardinality, ring symmetry, and one canonical clone class |
| `analyze_corpus` | Parse/build, call resolution, call projection, SCC, and WL over four corpus classes | Nonempty graph/function evidence and exact repeated-run summary equality |

The committed real-source corpus is:

| Class | Source |
| --- | --- |
| Rust, about 50 lines | `src/algorithms/families/searching.rs` (49 lines) |
| Rust, about 500 lines | `src/analysis/bugs/deviant.rs` (514 lines) |
| Rust, about 5,000 lines | `src/builder/node_mapper.rs` plus `src/analysis/scc.rs` (5,173 lines, parsed as two real modules) |
| Cross-language | `benches/corpus/service.py`, `.js`, `.go`, and `.c` |

The seeded synthetic generator covers a deep AST, a wide AST, many functions,
a complete directed CFG, and a complete directed call graph. It uses the stable
seed `0x6c69_6263_7067_0900`; the proptest varies both seed and node count and
requires two generated edge snapshots to be identical.

## Criterion `main` baseline

The following 34 cases were saved with `--save-baseline main` under
`taskset -c 0,1` and the `performance` governor.

| Target / case | $`\mu \pm \sigma`$ |
| --- | ---: |
| `cpg_construction/linear_ast_overlay/1000` | 82.170 ± 0.193 µs |
| `cpg_construction/linear_ast_overlay/10000` | 1.008605 ± 0.011270 ms |
| `cpg_construction/tree_sitter_rust/rust-small-49` | 359.249 ± 1.200 µs |
| `cpg_construction/tree_sitter_rust/rust-medium-514` | 6.733075 ± 0.010225 ms |
| `traversal_primitives/node/1024` | 2.595 ± 0.001 µs |
| `traversal_primitives/node/8192` | 23.859 ± 0.031 µs |
| `traversal_primitives/ast_children/1024` | 31.957 ± 0.036 µs |
| `traversal_primitives/ast_children/8192` | 266.440 ± 0.373 µs |
| `traversal_primitives/cfg_successors/1024` | 29.545 ± 0.019 µs |
| `traversal_primitives/cfg_successors/8192` | 230.414 ± 0.188 µs |
| `traversal_primitives/dfg_successors/1024` | 26.117 ± 0.026 µs |
| `traversal_primitives/dfg_successors/8192` | 221.636 ± 0.173 µs |
| `traversal_primitives/edges_between/1024` | 17.906 ± 0.009 µs |
| `traversal_primitives/edges_between/8192` | 166.457 ± 0.597 µs |
| `dfg_extraction/extract/rust-small-49` | 38.867 ± 4.557 µs |
| `dfg_extraction/extract/rust-medium-514` | 1.171197 ± 0.018707 ms |
| `pdg_construction/build_all_functions/rust-small-49` | 53.422 ± 4.643 µs |
| `pdg_construction/build_all_functions/rust-medium-514` | 1.657798 ± 0.049163 ms |
| `call_graph_sccs/path/1000` | 349.819 ± 0.873 µs |
| `call_graph_sccs/path/10000` | 4.170587 ± 0.024409 ms |
| `call_graph_sccs/ring/1000` | 228.837 ± 0.201 µs |
| `call_graph_sccs/ring/10000` | 2.719981 ± 0.003282 ms |
| `call_graph_sccs/clustered/1000` | 259.008 ± 0.696 µs |
| `call_graph_sccs/clustered/10000` | 2.951794 ± 0.004174 ms |
| `pattern_vf2/linear_subgraph/256` | 4.764 ± 0.069 µs |
| `pattern_vf2/linear_subgraph/4096` | 63.037 ± 0.040 µs |
| `similarity_wl/labels/1024` | 54.595 ± 0.017 µs |
| `similarity_wl/labels/8192` | 435.748 ± 0.454 µs |
| `similarity_wl/clone_classes/1024` | 57.729 ± 0.018 µs |
| `similarity_wl/clone_classes/8192` | 459.727 ± 1.462 µs |
| `analyze_corpus/end_to_end/rust_approximately_50_loc` | 411.125 ± 1.655 µs |
| `analyze_corpus/end_to_end/rust_approximately_500_loc` | 7.900072 ± 0.058424 ms |
| `analyze_corpus/end_to_end/rust_approximately_5000_loc` | 117.292081 ± 1.390410 ms |
| `analyze_corpus/end_to_end/python_javascript_go_c` | 1.215053 ± 0.003720 ms |

The relatively large variation on the small DFG and PDG cases is visible in
their standard deviations; those rows are retained honestly and should not be
used to justify a small optimization without a longer confirmatory run.

## Memory baseline

Each row is one execution of the named profile workload. DHAT totals include
the setup needed to construct that process's input; Criterion's timed DFG and
PDG loops use an untimed clone setup, so allocation totals and timed means answer
different questions.

| Workload | Peak RSS (KiB) | DHAT bytes | DHAT allocations |
| --- | ---: | ---: | ---: |
| `cpg_construction` | 6,856 | 8,217,688 | 23,849 |
| `traversal_primitives` | 10,136 | 16,942,267 | 90,169 |
| `dfg_extraction` | 6,624 | 8,217,686 | 23,849 |
| `pdg_construction` | 6,816 | 10,057,736 | 36,890 |
| `scc_analysis` | 5,100 | 3,998,691 | 1,914 |
| `pattern_vf2` | 6,324 | 5,863,362 | 106 |
| `similarity_wl` | 5,548 | 5,705,492 | 933 |
| `analyze_corpus` | 27,232 | 102,627,689 | 341,513 |

For the aggregate `all` driver, `/usr/bin/time -v` reported 39,348 KiB maximum
RSS. DHAT reported 161,603,561 bytes in 519,119 allocations, 31,134,984 bytes
live at its global maximum, and 696 bytes in two blocks at exit. Massif with
`--pages-as-heap=yes` reported a 45,539,328-byte peak. Massif also emitted its
documented `brk segment overflow` limitation, so the independent RSS value is
the primary resident-memory result; the Massif value is retained as a
page-allocation profile, not substituted for RSS.

The release-profile size snapshot was:

| Type | `size_of` (bytes) |
| --- | ---: |
| `CodePropertyGraph` | 216 |
| `CpgNode` | 592 |
| `CpgEdge` | 40 |
| `NodeId` | 4 |
| `GraphProjection` | 152 |

`size_of` measures the inline value only. It excludes heap allocations owned by
vectors, maps, graphs, strings, and reference-counted source text.

## Hardware counters and ranked hot paths

`perf stat -r 3` ran ten aggregate repetitions per trial. Values are trial
means; parenthesized percentages are perf's reported variation.

| Counter | Value |
| --- | ---: |
| Cycles | 6,451,940,144 (±1.13%) |
| Instructions | 13,376,920,965 (±0.15%) |
| Cache references | 353,697,818 (±0.62%) |
| Cache misses | 34,108,216 (±1.48%) |
| Page faults | 10,337 (±10.48%) |
| dTLB loads | 12,393,808 (±2.08%) |
| dTLB load misses | 860,249 (±8.85%) |
| Elapsed time | 1.547153 ± 0.020867 s |

The host PMU did not support perf's generic `LLC-loads` or `LLC-load-misses`
events; both were reported as `<not supported>`. No replacement value is
inferred.

LBR recording failed because `sys_perf_event_open` rejected
`cpu/cycles/Pu` with `EINVAL`. The required DWARF fallback captured 3,080
samples, 12.925 billion approximate event cycles, and zero lost samples. The
flat, no-children ranking is:

| Rank | Symbol or operation | Samples |
| ---: | --- | ---: |
| 1 | tree-sitter `ts_node_child_with_descendant` | 25.34% |
| 2 | `DfgExtractor::extract_function_dfg_indexed` | 5.16% |
| 3 | tree-sitter `ts_node_parent` | 3.28% |
| 4 | DFG `visit_reaching` | 3.06% |
| 5 | `ast_children` result-vector collection | 2.82% |
| 6 | tree-sitter `ts_node_start_byte` | 2.82% |
| 7 | tree-sitter `ts_node_child_count` | 2.79% |
| 8 | `resolve::build_symbol_table` | 2.43% |
| 9 | `CodePropertyGraph::ast_children` outside result collection | 2.00% |
| 10 | tree-sitter parser core `ts_parser_parse` | 1.57% |

This is an aggregate workload ranking: parser-backed corpus work receives more
wall time than the traversal microbenchmarks. It identifies where to design the
next isolated experiment; it is not a percentage breakdown for every target.

## VTune availability boundary

The VTune CLI was invoked headlessly; no GUI process was opened. Hotspots
hardware collection could not begin because `kernel.yama.ptrace_scope` is `1`,
and VTune requested `0`. Software-sampling Hotspots failed on the same policy.
Memory Access additionally reported that the AMD processor was not recognized
for that Intel analysis, architectural PMU version was unavailable to VTune,
and its sampling driver was not installed. Changing a host-wide `ptrace` policy
or installing a kernel driver is outside a library benchmark's authority, so
the exact failures are recorded instead. perf, DHAT, Massif, and RSS collection
all completed without such a change.

## Evidence-ranked optimization queue

No item in this queue has been implemented by T9.0. Each is a hypothesis that
requires one isolated change, the full correctness gates, and comparison with
the named `main` baseline.

| Priority | Evidence | Next controlled experiment |
| ---: | --- | --- |
| 1 | 25.34% in tree-sitter descendant traversal plus 3.28% parent lookup | Count repeated mapper ancestry/descendant queries by node kind; test a per-build, arena-indexed ancestry cache without changing mapper semantics. |
| 2 | 5.16% in indexed DFG extraction and 3.06% in reaching visitation | Profile DFG alone on deep, wide, and loop-heavy fixtures; attribute allocations by DHAT program point before changing its heap machine. |
| 3 | 2.82% in `ast_children` vector collection, another 2.00% in the method, and 90,169 traversal-driver allocations | **Accepted (T9.1 H03):** maintain the source-order child index during insertion and return a borrowed slice. See the controlled result below. |
| 4 | 2.43% in symbol-table construction | Separate call-resolution indexing from parser construction and measure whether one reusable immutable index removes repeated scans. |
| 5 | 1.41% in call-graph edge normalization plus visible hash-table rehash samples | Measure exact edge cardinalities and reserve requirements on sparse and dense call graphs before adding capacity hints. |

Stack safety is non-negotiable in every experiment: caches, iterators, and
capacity changes must preserve the explicit heap worklists, pushdown machines,
and bounded arenas already used by deep traversals. Performance profiles are
not stack-safety proofs; the dedicated 256 KiB-stack regression suites remain
the gate.

### T9.1 H03 — maintained AST child index

**Hypothesis.** The traversal profile attributed 4.82% of aggregate samples to
`ast_children` and its result-vector collection. Maintaining the already
modeled `CpgNode.children` index at `AstChild` insertion should remove the graph
scan, sort, and allocation without changing ascending-`EdgeId` source order.

**Controlled change.** `connect` now appends monotonic edges to the index in
constant time; explicit-id reconstruction rebuilds it in edge-id order.
`ast_children` returns `&[NodeId]`. Exact examples and property tests compare
that slice with an edge-sorted oracle, permute explicit-edge storage order, and
exercise malformed multi-parent graphs. Existing deep-AST tests continue to
run on a 256 KiB thread stack.

**Result against immutable `main`.** Criterion measured 20 samples after a
one-second warm-up on performance-governed CPUs 0–1:

| Case | `main` | H03 | Median time change | Throughput change |
| --- | ---: | ---: | ---: | ---: |
| `ast_children/1024` | 31.957 µs | 3.246 µs | -89.84% | +884.29% |
| `ast_children/8192` | 266.440 µs | 37.562 µs | -85.91% | +609.66% |
| construction, 1,000-node linear AST | 82.061 µs | 82.032 µs | -0.03% (no significant change) | +0.03% |
| construction, 10,000-node linear AST | 1.009 ms | 0.988 ms | -2.07% | +2.12% |
| construction, 49-line parser corpus | 359.249 µs | 345.500 µs | -3.91% | +4.07% |
| construction, 514-line parser corpus | 6.733 ms | 6.369 ms | -5.38% | +5.69% |

The headless DHAT traversal workload fell from 90,169 to 73,787 allocations
(-18.17%) and from 16,942,267 to 16,418,030 allocated bytes (-3.09%). Peak RSS
fell from 10,136 to 9,928 KiB (-2.05%). The first implementation scanned every
target's incoming edges to canonicalize its parent and regressed synthetic
construction; restricting that scan to explicit-id reconstruction or a
conflicting pre-populated parent removed the regression while preserving the
edge-order oracle. The hypothesis is accepted.

## Reproduce and compare

The benchmark wrapper defaults to CPUs 0 and 1 and stores logs under
`target/verification`; it never writes build or profile artifacts to `/tmp`.

```sh
features=lang-rust,lang-python,lang-javascript,lang-go,lang-c
for bench in cpg_construction traversal_primitives dfg_extraction \
  pdg_construction scc_analysis pattern_vf2 similarity_wl analyze_corpus
do
  taskset -c 0,1 cargo bench --bench "$bench" --features "$features" -- \
    --save-baseline main
done
```

For a complete headless profile capture:

```sh
LIBCPG_PROFILE_CPUS=0,1 \
LIBCPG_PROFILE_REPETITIONS=10 \
./scripts/profile.sh all
```

The script builds `target/release/examples/profile_workloads`, records RSS and
perf counters/call graphs, uses DHAT and Massif, and attempts VTune only through
its command-line interface. It reports an unavailable collector and continues
with the successful headless captures. Set `LIBCPG_PROFILE_OUTPUT_DIR` to retain
a named comparison directory under `target/verification`.

After an optimization, compare the same cases without overwriting `main`:

```sh
./scripts/bench.sh traversal_primitives --baseline main
LIBCPG_BENCH_FEATURES=lang-rust \
  ./scripts/bench.sh dfg_extraction --baseline main
```

Criterion baselines and raw profiler captures live under ignored `target/` and
may be regenerated. This ledger is the reviewed, committed record. See the
[measurement methodology](../scientific/04-measurement-methodology.md) for the
scientific interpretation and the [performance guide](03-performance.md) for
the broader complexity model.
