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
| `dominator_analysis` | Direct dominance and function post-dominance over 256/2,048-stage diamond ladders | Exact nodes/edges, well-formed CSR, complete idom/ipdom relations, one exit, and two control dependences per stage |
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

### T9.1 H02 — complete copy-sized node-kind tags

**Hypothesis.** CFG and DFG hot paths copied payload-bearing `CpgNodeKind`
values before mutating the graph. Replacing variant-only clones with a complete
one-byte tag should reduce heap allocations by 5–20% and improve construction
time without weakening the stack-safe pushdown machines.

**Controlled change.** `CpgNodeKindTag` classifies all 45 variants one-to-one;
`CpgNodeKind::tag` and `CodePropertyGraph::node_tag` expose it without
allocation. The iterative CFG machine copies this tag. The iterative reaching-
definitions machine borrows the kind and copies only the `Arc<str>` binder it
must retain in a later heap frame. Its fixed-bounded recursive oracle still
clones the full enum independently. An all-variant property establishes that
tag equality is exactly Rust discriminant equality, and the shared generator
now reaches every variant.

**Controlled result against the pre-H02 H03 tree.** Criterion measured 20
samples after a one-second warm-up on performance-governed CPUs 0–1:

| Case | Before H02 | H02 | Median time change | Throughput change |
| --- | ---: | ---: | ---: | ---: |
| construction, 1,000-node linear AST | 82.493 µs | 81.414 µs | -1.28% | +1.30% |
| construction, 10,000-node linear AST | 1.003 ms | 1.000 ms | -0.79% (within noise threshold) | +0.80% |
| construction, 49-line parser corpus | 345.420 µs | 336.240 µs | -2.65% | +2.72% |
| construction, 514-line parser corpus | 6.384 ms | 6.152 ms | -3.66% | +3.80% |

Against immutable `main`, the final H02 tree was neutral on both synthetic
linear cases and improved the 49-line and 514-line parser cases by 6.49% and
8.83%, respectively. Headless DHAT measured exactly the same 174,636
allocations and 51,793,797 allocated bytes before and after H02, refuting the
allocation mechanism. The same profile reduced retired instructions by 1.65%
and cycles by 2.71%; resident-set size fell 5.46%, while cache and translation-
lookaside-buffer counters had enough run-to-run variance that no causal claim
is made from them. The hypothesis is accepted for reduced data movement and
runtime, with its predicted allocation reduction explicitly rejected.

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

### T9.1 H04 — boxed cold node payloads

**Hypothesis.** `CpgNode` was 592 bytes because its closed `CpgNodeKind` union
stored a complete `MethodSignature` inline and several uncommon fields stored a
complete `TypeInfo` inline. Moving those cold, variable-size payloads behind
one heap pointer should reduce arena copy traffic and peak memory enough to
outweigh the added allocations and pointer indirections.

**Controlled change.** The experiment had two separately measured steps. H04a
boxed only `Function.signature`. H04b also boxed the node-owned type payloads:
`Parameter.param_type`, `Variable.var_type`, `Field.field_type`,
`TypeAnnotation.type_info`, and `CpgNode.allocation_type`. A function's
parameter and return types remain together inside its single boxed
`MethodSignature`; they do not incur one allocation per parameter. Existing
constructors still accept `TypeInfo` by value and perform the storage decision
internally. Borrowing uses `Deref` or `Option::as_deref`, and Serde preserves
the existing JSON representation without an object-wrapper layer.

The correctness oracles include an exact JSON example for both boxed payload
families, an all-variant node-kind serialization property, the independent
recursive tree-builder oracle on fixed-bounded inputs, and the existing deep
builder suites on a 256 KiB thread stack. A 64-bit layout regression asserts a
168-byte upper bound rather than a feature-dependent exact value; the node was
160 bytes in the all-features debug test and 168 bytes in the release
language-feature profile. Rust layout is not a stable application binary
interface, so the bound protects the intended compactness without asserting an
unsupported byte-for-byte contract.

**Controlled construction result against pre-H04 H02.** Criterion measured 20
samples after a one-second warm-up on performance-governed CPUs 0–1:

| Case | Before H04 | H04 | Median time change | Throughput change |
| --- | ---: | ---: | ---: | ---: |
| construction, 1,000-node linear AST | 81.664 µs | 66.613 µs | -18.41% | +22.56% |
| construction, 10,000-node linear AST | 959.160 µs | 864.570 µs | -9.92% | +11.01% |
| construction, 49-line parser corpus | 333.660 µs | 332.930 µs | -0.27% (within noise threshold) | +0.27% |
| construction, 514-line parser corpus | 6.098 ms | 6.036 ms | -1.03% (within noise threshold) | +1.04% |

H04a alone reduced the node to 288 bytes, total DHAT bytes by 24.81%, and peak
live DHAT bytes by 36.97%. The completed H04b node is 168 bytes in the release
profile, a 71.62% inline-size reduction. Headless DHAT measured 33,906,117
allocated bytes, down 34.54%, and a 1,687,524-byte peak, down 51.46%. Massif's
page-accounting peak fell 12.26%, from 13,492,224 to 11,837,440 bytes.

The storage trade-off is explicit: boxing created 175,416 allocations, 780
more than the 174,636 pre-H04 count (+0.45%), and perf's data translation
lookaside buffer (dTLB) load misses rose from 24,120 to 31,819 (+31.92%). In the
same aggregate profile, cycles fell 2.19%, instructions fell 0.66%, page faults
fell 37.20%, dTLB loads fell 6.50%, and elapsed time fell 3.99%. Cache-reference
and cache-miss samples moved downward but had 8.05% and 13.12% variation, so
they are directional evidence only. Peak RSS fell 9.98%, but process-level RSS
also contains measurement noise and is secondary to the exact layout and DHAT
results.

Against immutable `main`, the final tree improved the 1,000-node and
10,000-node synthetic cases by 17.66% and 16.52%, respectively, and the
49-line and 514-line parser cases by 7.10% and 10.11%. The hypothesis is
accepted: substantially less inline and peak memory accompanied a net runtime
improvement, while the documented allocation-count and dTLB-miss costs remain
visible for future decisions.

### T9.1 H05 — cold metadata and removal of an unread edge map

**Hypothesis.** Every node stored an empty 32-byte `FxHashMap` header, every
edge stored a 24-byte `String` option even though most edges were unlabeled,
and `CodePropertyGraph` maintained an edge-id map that no query read. Lazy
property allocation, a boxed string slice for present labels, and removal of
the unread map should reduce node, edge, and graph storage without changing
semantics or persistence.

**Controlled change.** H05a changed
`CpgNode.properties` to `Option<Box<FxHashMap<...>>>` and `CpgEdge.label` to
`Option<Box<str>>`. `None` is the canonical empty property state and
`with_property` allocates once on first insertion. A custom field adapter
preserves the existing Serde map shape, including `{}` for empty properties;
`Box<str>` preserves the existing string shape. H05b then removed
`edge_index_map`. A repository-wide search proved that its only occurrences in
production were declaration, capacity reservation, insertion, and cloning—no
reader, lookup API, or analysis depended on it. Edge ids remain in edge
weights, and `next_edge_id` still preserves explicit-id reconstruction.

The oracles cover exact empty and populated property JSON, the canonical CPV1
property bytes, 256-case lazy-map equivalence against an eager `FxHashMap`, a
256-case arbitrary-label JSON round trip, explicit edge-id reconstruction,
whole-graph serialization properties, an edge-sorted AST oracle, and the
independent source-versus-tree builder property. Layout regressions use upper
bounds because Rust layout is not a stable application binary interface.

**Controlled construction result against committed H04.** Criterion measured
20 samples after a one-second warm-up on performance-governed CPUs 0–1. The
change column is Criterion's central comparison estimate; confidence intervals
crossing the configured noise threshold are reported as no change.

| Case | Before H05 | H05 | Time change | Throughput change |
| --- | ---: | ---: | ---: | ---: |
| construction, 1,000-node linear AST | 66.464 µs | 54.567 µs | -17.79% | +21.64% |
| construction, 10,000-node linear AST | 834.800 µs | 644.450 µs | -22.60% | +29.20% |
| construction, 49-line parser corpus | 336.210 µs | 335.920 µs | -0.12% (no change detected) | +0.12% |
| construction, 514-line parser corpus | 6.091 ms | 6.093 ms | -0.68% (no change detected) | +0.68% |

The displayed `time` interval and the change bootstrap are distinct Criterion
estimators; near zero, their point signs can therefore differ. The confidence
interval and Criterion verdict, not that sign, determine whether a change is
claimed.

H05a alone changed the release language-feature layouts from 168 to 144 bytes
per `CpgNode` (-14.29%) and from 40 to 32 bytes per `CpgEdge` (-20.00%). It
reduced total DHAT bytes by 4.00% and peak live DHAT bytes by 8.04% with exactly
the same 175,416 allocations. Its 10,000-node construction case improved
8.41%; the other three cases were within the noise threshold or had no detected
change.

After H05b, `CodePropertyGraph` fell from 216 to 184 bytes (-14.81%). Headless
DHAT fell from 33,906,117 to 31,484,597 total bytes (-7.14%) and from 1,687,524
to 1,445,372 peak live bytes (-14.35%). It recorded 175,406 allocations, ten
fewer than H04 because the ten profile repetitions no longer allocate an edge
map. Massif's page-accounting peak fell 2.32%, from 11,837,440 to 11,563,008
bytes.

The three-run perf profile was neutral in cycles (+0.05%), reduced retired
instructions by 0.81%, and increased elapsed time by 1.82%; the latter conflicts
with the longer Criterion construction result and is treated as process noise,
not hidden. Page faults fell 9.40% and dTLB loads fell 32.55%. dTLB misses fell
15.54%, but their 10.17–20.85% run variation prevents a causal claim. Peak RSS
rose 4.86% and is likewise secondary to the exact layout and DHAT evidence.

Against immutable `main`, the complete T9.1 tree improved the 1,000-node and
10,000-node constructors by 32.73% and 35.03%, respectively, and the 49-line
and 514-line parser cases by 6.98% and 6.88%. H05 is accepted: the common empty
and unlabeled representations are materially smaller, the unread structure is
gone, the synthetic construction paths are faster, and neither parser path
regressed under the controlled comparison.

### T9.2 H07 — lazy deterministic exact-edge CSR cache

**Hypothesis.** CFG/DFG neighbor and endpoint queries repeatedly filtered
petgraph adjacency and collected fresh vectors. One lazily built cache of exact
edge indices, with deterministic forward/reverse overlay rows and source/target
rows, should make warm reads proportional to the selected row and eliminate
their result allocations. It must preserve exact parallel edges, invalidate on
mutation, remain absent from serialization, and keep graph depth off the native
stack.

**Controlled change.** `CodePropertyGraph` now owns a
`OnceLock<Box<CpgProjectionCache>>`. A counting/scatter builder creates three
CSR edge-index orders, sorts only nontrivial rows, and retains aligned target ids
for endpoint partitions. `CpgEdges`, `CfgNeighbors`, and `DfgNeighbors` are
borrowed exact-size/double-ended iterators. Stable projection, SCC, PDG, VF2,
and statistics readers use the cache. Mutation-oriented call resolution, DFG,
and uniqueness checks use crate-private native petgraph adjacency so a read does
not trigger “build global cache, append one edge, invalidate” inside a loop.

The semantic oracles compare every cached accessor with direct edge filtering
over 256 generated graphs and compare logical sequences across 256 insertion
permutations. Parser integration repeats the full oracle after Rust call
resolution and PDG construction. Exact examples cover parallel edges, unknown
ids, cursor behavior, mutation invalidation, clone, and serde omission. A
100,000-node CFG path plus a 99,999-edge DFG row builds, traverses, and drops on
a 256 KiB native stack.

**Isolated warm-read result against committed H05.** Criterion's central
time-change estimate is shown; node and AST child reads are controls outside
the new cache.

| Case | 1,024 nodes | 8,192 nodes |
|---|---:|---:|
| `node` control | +1.07% (noise threshold) | +2.17% |
| `ast_children` control | +3.86% | -2.74% |
| `cfg_successors` | -33.11% | -18.13% |
| `dfg_successors` | -31.09% | -14.03% |
| `edges_between` | -45.10% | -45.77% |

The isolated corpus comparison measures the cold first build: approximately
50-line Rust regressed 7.58%, 500-line and 5,000-line Rust had no detected
change (+0.56% and -0.37%), and the four-language bundle regressed 4.25%.
That cost is retained as a documented falsification boundary rather than
relabelled as warm traversal.

**Complete result against immutable `main`.** The cumulative branch improves
CFG reads 37.71%/18.33%, DFG reads 30.83%/18.93%, and endpoint lookup
45.71%/44.99% at 1,024/8,192 nodes. AST child reads, supplied by H03, improve
88.95%/85.36%. End-to-end analysis improves 6.74% on small Rust, 8.54% on
medium Rust, and 11.01% on the cross-language corpus; the large Rust result
(-2.63%) remains inside the configured noise threshold.

**Headless allocation and peak result against H05.** Ten cold construction-plus-
traversal repetitions produced the following complete trade-off:

| Measure | H05 | H07 | Change |
|---|---:|---:|---:|
| DHAT bytes | 77,000,155 | 75,364,395 | -2.12% |
| DHAT blocks | 737,604 | 246,274 | -66.61% |
| DHAT peak live bytes | 2,999,920 | 4,310,480 | +43.69% |
| Massif page peak | 13,053,952 | 14,323,712 | +9.73% |
| maximum RSS | 5,776 KiB | 7,056 KiB | +22.16% |
| cycles | 99,396,514 | 105,750,416 | +6.39% |
| retired instructions | 248,637,765 | 338,333,466 | +36.07% |

The cold profile performs only one read sweep, so cache construction dominates
its counters while Criterion measures repeated warm access. Allocation blocks
fall by two thirds, validating the target, but retained peak memory rises. The
graph header grows from 184 to 200 bytes and remains below the pre-H05 216-byte
layout. VTune's headless collectors were unavailable under host `ptrace_scope`;
perf used its headless DWARF fallback. H07 is accepted because its exact warm
read and allocation objectives pass, the cumulative corpus boundary does not
regress, and the cold/peak costs are explicit. Full method and counter tables
are in [validation 51](../scientific/51-projection-cache-validation.md).

### T9.2 H08 — dense stack-safe VF2 state

H07's corrected search-only profile made the next bottleneck unambiguous: hash-set
collection accounted for 41.58% of samples, sorting for 28%, vector collection for 8%,
and candidate-pair construction for 7.33%. H08 maps stable graph ids into a lazy dense
domain, replaces mapping and fringe hash tables with arrays and Fenwick-ordered sets,
stores terminal restoration in flat delta logs, and gives each search-PDA frame only a
pattern index and target cursor. A necessary distinct directed-neighbour check rejects
impossible candidates without changing non-induced or parallel-edge semantics.

**Isolated result against committed H07.** The corrected Criterion workload retains the
fixture and measures only capped matching:

| Target | H07 | H08 | Central change |
|---:|---:|---:|---:|
| 256 nodes | 4.726 µs | 0.949 µs | -79.89% |
| 4,096 nodes | 63.429 µs | 1.977 µs | -96.91% |

Against immutable `main`, an independent final run reports 0.940 µs and 1.999 µs,
improvements of 80.26% and 96.83%. The small central-time variation between runs is
expected; the exact one-match oracle and canonical result prefix are unchanged.

**Headless search-only result against H07.** Ten thousand retained-fixture searches
reduce DHAT bytes 27.28%, allocation blocks 38.88%, cycles 96.71%, retired instructions
97.44%, cache references 65.43%, cache misses 84.78%, and dTLB misses 85.69%. DHAT and
Massif peak live memory fall 6.00% and 0.22%; maximum RSS falls from 4,400 to 4,324 KiB
(1.73%). RSS remains a coarse startup-sensitive sample, so no stronger generalization is
made. No GUI or heaptrack process was used.

**End-to-end control and rejected variants.** Fully lazy node metadata improves small
Rust 4.83%, medium Rust 4.12%, and the cross-language corpus 1.84% against H07; large
Rust improves 3.33% within Criterion's noise threshold. An eager degree view regressed
small/cross cases by about 4.2%; making degrees lazy but copying ids eagerly still showed
medium/large regressions. Those variants were rejected at their root. Candidate reordering
and raw-degree cuts were also rejected because they would change canonical capped prefixes
or the established existential multiedge relation. Full counters, properties, stack gate,
and falsification boundaries are in
[validation 52](../scientific/52-dense-vf2-validation.md).

### T9.2 H09 — borrowed-CSR CHK and PDG boundary

The H08 headless profile attributed only 3.61% of sampled cycles to the CHK
fixed point. The dominant costs were edge-row cloning into `Vec<Vec<u32>>`,
materializing and reversing a post-dominator graph, per-join frontier vectors
and tree sets, public map construction, and a discarded first function CFG
projection. H09 retains bounded deterministic CHK and changes the ownership
boundary: direct analysis borrows both CSR directions, post-dominance swaps them
behind a virtual-exit row/bit-mask overlay, frontier rows are flat and
heap-sorted, and function scope produces one projection.

**Direct Criterion result against committed H08:**

| Target | H08 | H09 | Criterion change |
|---|---:|---:|---:|
| dominators / 256 stages | 309.97 µs | 258.32 µs | -16.75% |
| post-dominators / 256 stages | 509.03 µs | 437.02 µs | -15.28% |
| dominators / 2,048 stages | 2.6160 ms | 2.2259 ms | -15.06% |
| post-dominators / 2,048 stages | 4.2222 ms | 3.7292 ms | -11.71% |

**Parser-backed PDG consumer result against committed H08:**

| Corpus | H08 | H09 | Criterion change |
|---|---:|---:|---:|
| Rust small (49 lines) | 57.992 µs | 40.804 µs | -29.76% |
| Rust medium (514 lines) | 3.1532 ms | 2.3207 ms | -27.09% |

**Headless retained-fixture profile, 100 repetitions:** allocated bytes fall
19.03%, allocation blocks 31.66%, peak live bytes 9.89%, Massif page peak
5.06%, RSS 9.22%, cycles 13.57%, retired instructions 15.88%, cache references
29.08%, cache misses 13.81%, and dTLB loads 17.62%. dTLB misses rise 2.84% and
page faults rise 50.93% despite the lower allocation/live-memory counters;
both are retained as adverse startup-sensitive observations. VTune CLI
collectors were unavailable; perf used the headless DWARF fallback. No GUI or
heaptrack process was used.

Replacing CHK with Lengauer–Tarjan was rejected because the measured fixed
point was not the bottleneck. Weakening the independent frontier oracle was
also rejected: it exposed and now guards the root self-loop/backedge semantics.
Full exhaustive/property evidence, stack gate, counters, and falsification
boundaries are in
[validation 53](../scientific/53-dominator-optimization-validation.md).

### T9.3 H10 — deterministic parallel plans and ordered emission

H10 separates per-function CFG/DFG/PDG discovery from graph mutation and maps
independent plans through indexed Rayon iterators. One serial emitter applies
plans in canonical function order, preserving exact node/edge identities,
idempotency, and Serde bytes. Ordered VF2 batches use the same indexed reduction
without graph mutation. The serial PDG path was also corrected to snapshot
existing edges once instead of rescanning the complete graph per function.

On the controlled 512-function fixture, the pipeline improves from 144.27 ms
serial to 12.35 ms at 32 workers (11.68×); the 128-pattern VF2 batch improves
from 1.1150 ms to 178.71 µs (6.24×). A single worker is 0.4% slower than serial,
which supports the serial default and explicit opt-in. The headless 32-worker
profile improves wall time 4.95× while RSS falls 2.3%; DHAT bytes rise 0.63%
and peak live bytes rise 1.51%, recording the concurrency trade rather than
hiding it. Complete affinity, counter, property, and depth-100,000 evidence is
in [validation 54](../scientific/54-parallel-analysis-validation.md).

### T9.3 H11 — build-local mapped-string canonicalization

**Hypothesis.** Parser mapping appeared to allocate one `Arc<str>` per name,
type, operator, literal, import, or grammar-kind occurrence. A temporary
content-keyed pool should reduce retained ownership without changing public
types, bytes, order, cloning, Serde, or stack bounds. Candidate interning and
hasher choices had to win measured workloads and avoid global mutable state.

**Inventory and controlled change.** Three real Rust modules produce 28,510
mapped string occurrences, 1,810 distinct values, and initially 28,510 distinct
allocations. `CpgStringPool` now visits one completed mapped node before graph
insertion, uses borrowed `&str` lookup in `FxHashSet<Arc<str>>`, and replaces a
duplicate with a clone of the canonical handle. The fixed-depth match plus flat
parameter/generic loops introduces no recursive call edge; the pool drops at
the AST/CFG boundary.

| Retained measure, three graphs alive | Before H11 | H11 | Change |
|---|---:|---:|---:|
| mapped string allocations | 28,510 | 2,226 | -92.2% |
| mapped string payload bytes | 295,920 | 37,767 | -87.2% |

The 416 remaining duplicate allocations are equal values in different
build-local pools. Each individual file has exactly one allocation per distinct
content; no process-global table is involved.

**Controlled Criterion result against committed H10:**

| Case | H10 | H11 | Central change |
|---|---:|---:|---:|
| parser Rust, 49 lines | 334.22 µs | 326.98 µs | -1.85% |
| parser Rust, 514 lines | 6.0021 ms | 5.9759 ms | -0.39% |
| 1,000-node non-parser control | 53.878 µs | 54.382 µs | +1.23% |
| 10,000-node non-parser control | 638.69 µs | 638.01 µs | +0.03% |

Both parser cases improve. The small non-parser movement executes none of H11
and remains a noise control.

**Headless ten-repetition profile against committed H10:**

| Measure | Construction change | Whole-corpus change |
|---|---:|---:|
| DHAT total bytes | +0.56% | +0.19% |
| DHAT total blocks | +0.05% | +0.01% |
| DHAT peak live bytes | -3.68% | -2.93% |
| DHAT peak live blocks | -26.44% | -20.97% |
| maximum RSS | -1.94% | -2.33% |
| Massif page peak | -1.11% | -2.94% |

Total traffic stays flat because the mapper still creates each candidate before
releasing a duplicate; live ownership is the target and falls materially.
Perf elapsed is +1.24%/+0.19% on the short construction/whole-corpus controls,
while retired instructions are -1.27%/-0.08%; Criterion is the timing decision
boundary. High-variation cache/TLB observations are retained in validation 55
without a causal claim.

**Hasher and dependency result.** A temporary 30-sample Criterion target modeled
the observed 28,510/1,810 string distribution and a separate 100,000-key `u32`
map. Fx completed in 266.76 µs versus AHash's 309.83 µs for strings (13.9%
faster), and 1.7035 ms versus 2.0340 ms for integer keys (16.2% faster). H11
retains Fx, removes rejected `ahash`, removes unused `string_cache`, and deletes
the temporary benchmark/probe after recording their results.

The 256 generated ownership cases, Unicode/229,376-byte examples, public clone
and Serde-byte oracles, and depth-100,000/256 KiB parser gate are detailed in
[validation 55](../scientific/55-string-storage-validation.md). ADR-0060 records
the rejected global atom, mutex, whole-graph sweep, and AHash alternatives.

### T9.3 H12 -- reusable deterministic WL buffers

**Hypothesis.** Four-round projection Weisfeiler--Lehman (WL) refinement
allocated a new label matrix and canonical vocabulary at every round. Reusing
call-local capacities should reduce allocation traffic and isolated latency
while preserving the fixed public numeric labels, malformed behavior, and
constant native call depth. `FxHasher` was a separate candidate only if it
could meet that compatibility boundary.

**Controlled change.** Nonzero-round calls now allocate one capacity-matched
next-label row per graph and one total-node canonical vocabulary. Every round
clears and refills them, then swaps current and next matrices. Zero-round calls
return their seed matrix before scratch allocation. The incident vector remains
call-local and reused. There is no global/thread-local cache, graph mutation,
unsafe initialization, or recursive traversal.

**Criterion against committed H11:**

| Case | H11 | H12 | Central change |
|---|---:|---:|---:|
| WL labels, 1,024-node ring | 53.517 µs | 52.794 µs | -1.37% |
| clone classes, 1,024-node ring | 56.744 µs | 55.890 µs | -1.51% |
| WL labels, 8,192-node ring | 425.66 µs | 420.20 µs | -1.26% |
| clone classes, 8,192-node ring | 451.51 µs | 444.34 µs | -1.65% |

A first safe zero-filled scratch prototype regressed the two 8,192-node cases
by 1.31% and 1.26%; it was removed. Capacity-retaining clear-and-push avoids
those redundant initialization writes and improves all isolated cases. Four
end-to-end parser/corpus controls remained within Criterion's noise threshold
or reported no change (-0.25% to +1.04%).

**Headless ten-repetition profile:**

| Measure | Isolated WL change | Whole-corpus change |
|---|---:|---:|
| DHAT total bytes | -124,320 (-0.237%) | -75,040 (-0.015%) |
| DHAT allocation blocks | -180 (-1.719%) | -540 (-0.019%) |
| DHAT peak live bytes/blocks | unchanged | unchanged |
| maximum RSS | -44 KiB (-0.802%) | -492 KiB (-2.076%) |
| Massif page peak | unchanged | +4,096 B (+0.014%) |

Nine allocations disappear from each four-round call; the isolated driver
performs 20 calls, exactly explaining its 180-block reduction. Perf-stat
elapsed time was -0.66%/+0.80% for WL/corpus, while instructions were
+0.13%/-0.02%; cache/TLB variation is not treated as causal. DWARF supplied the
headless call-graph fallback. VTune CLI was attempted without a GUI and rejected
by host `ptrace_scope` and AMD/driver availability.

**Hasher decision.** A removed 30-sample prototype measured 8,192 three-word
signatures at 32.169 µs with the fixed mixer and 8.315 µs with `FxHasher`.
Fx is rejected despite its 3.87x isolated speed because raw digest ordering
defines public sorted ranks. Changing it can change exact labels and violates
the cross-release contract.

Two 256-case generated campaigns, an independent fresh-allocation predecessor
oracle, exact topology/boundary cases, parser composition, and 100,000-node
path/star lifecycles on 256 KiB are detailed in
[validation 56](../scientific/56-wl-buffer-reuse-validation.md). ADR-0061
records the ownership proof and rejected zero-fill, hasher, global-cache, and
unsafe alternatives.

### T9.4 H13 -- bounded exact function-analysis cache

**Hypothesis.** Repeated whole-file builds paid the complete CFG/DFG/PDG
planning cost even when most functions were byte-identical. A caller-owned
cache of exact function-local plans should improve unchanged and locally edited
revisions without changing the ordinary API, graph bytes, determinism, or
native-stack bound.

**Controlled change.** The cache retains versioned canonical key bytes and
plans expressed only in function-local ordinals. Its FNV-1a digest selects a
bucket; full bytes decide identity, including under an injected constant-digest
collision. Represented external callers, preceding lexical bindings, nested
functions, nominal field ownership, or crossing PDG edges bypass reuse. A
deterministic LRU enforces caller-selected entry and estimated resident-byte
limits. Zero capacity executes the ordinary pipeline. All traversal and drop
paths use heap-owned worklists or flat containers.

**Criterion timing against the ordinary fresh path:**

| Revision case | Median | Change from 52.752 ms fresh |
|---|---:|---:|
| cold cache | 51.125 ms | within noise; no claimed improvement |
| exact hit | 41.733 ms | -20.89% |
| one edited function | 41.366 ms | -21.58% |
| half-cache churn | 45.982 ms | -14.86% |
| forced eviction | 50.500 ms | -4.27% |
| shared-dependency bypass | 1.7979 ms | +4.57% versus its 1.7194 ms fresh control |

The last row is an adverse small-case result and confirms that exact bypasses
are not presented as hits. A 100-build immutable-H12 control retired
47,455,981,663 instructions; current ordinary construction retired
47,555,957,577 (+0.211%) while elapsed time fell 4.08% and RSS fell 56 KiB.
With a zero-capacity cache, the 100-build instruction difference from the
ordinary path was 8,519 instructions (+0.000018%).

**Allocation and residency boundary.** A one-scenario DHAT run measured
36,200,614 bytes in 191,950 blocks for fresh construction and 37,630,032 bytes
in 193,453 blocks for a cold insertion. Disabled mode had the same block count
as fresh. The normal 128-entry cache retained an estimated 467,850 bytes:
177,670 key bytes, 284,036 artifact bytes, and 6,144 metadata bytes. Forced
16-entry eviction retained 36,720 bytes. Massif page peaks were 26,767,360
bytes fresh/disabled and 27,648,000 bytes cold.

Exact examples, the 32-case generated revision campaign, Mode-A/Mode-B parser
composition, deterministic collision/eviction checks, and deep and wide
100,000-node runs on a 256 KiB stack are detailed in
[validation 57](../scientific/57-function-analysis-cache-validation.md).
ADR-0062 records the key inventory, dependency boundary, ownership model, and
rejected partial/global/persistent cache alternatives.

### T9.4 H14A -- runtime-dispatched dense bit-set kernels

**Hypothesis.** Wide liveness and expression fixed points repeatedly apply
contiguous word-wise union and intersection. Runtime-selected AVX2 should
reduce instruction work without changing lattice bits, change flags, public
configuration, non-x86 behavior, allocation, or native-stack depth. Short
widths and subset early exit were explicit falsification boundaries.

**Controlled change.** Union and intersection retain their scalar oracle and
select four-`u64` AVX2 chunks only for at least eight words on a runtime-capable
x86/x86-64 CPU. One `OnceLock` caches the feature decision. Unaligned direct
slice loads avoid staging copies; zero-to-three-word tails return to the scalar
loop. Non-x86 builds compile scalar selection only. Subset remains scalar.
The only unsafe functions have a documented target-feature, slice-validity,
complete-chunk, and scalar-tail contract.

**Crossover and rejected candidate.** A four-word minimum improved its exact
width but a five-word input regressed union 12.75% and intersection 9.74%, so
that policy was removed. The retained eight-word threshold improved the
511/512-bit cases about 13--15%, left the 513-bit tail within noise, and
improved the 767--769-bit cases about 12--23%. At 8,192 bits, union improved
29.5--32.6% and intersection 21.9--24.6%; at 65,536 bits the ranges were
32--38% and 30--31%.

**Matched headless primitive profile.** Scalar and selected executions had the
same checksum and exact DHAT result: 28,327 bytes in 17 blocks, with identical
reads/writes. Elapsed fell from 0.934740747 s to 0.486038945 s (-48.0%), cycles
from 3,954,848,216 to 2,054,858,196 (-48.0%), and retired instructions from
21,720,722,213 to 7,951,624,980 (-63.39%). RSS moved by +8 KiB. Union and
intersection accounted for 96.52% of samples.

**Whole-analysis boundary.** Six established liveness families ranged from
-2.25% to +1.28%, all neutral or modestly favorable. In a deliberately wide
case, `variable_facts`, `BTreeMap` insertion, and `BinaryHeap::pop` dominated;
the AVX2 functions were each below the 0.5% report threshold. The primitive
gain is retained without an unsupported whole-analysis speedup claim.

Arbitrary-vector differential properties, named tail/boundary examples,
parser-built May/Must analyses, Miri, native AddressSanitizer, wasm scalar
portability, and 100,000-wide/deep 256 KiB stack gates are detailed in
[validation 58](../scientific/58-runtime-dispatched-bitset-validation.md).
ADR-0063 records the dispatch and unsafe boundary. No GUI or heaptrack process
was used, and no build/profile artifact was placed under `/tmp`.

## Reproduce and compare

The benchmark wrapper defaults to CPUs 0 and 1 and stores logs under
`target/verification`; it never writes build or profile artifacts to `/tmp`.

```sh
features=lang-rust,lang-python,lang-javascript,lang-go,lang-c
for bench in cpg_construction traversal_primitives dfg_extraction \
  pdg_construction dominator_analysis scc_analysis pattern_vf2 similarity_wl \
  analyze_corpus
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
