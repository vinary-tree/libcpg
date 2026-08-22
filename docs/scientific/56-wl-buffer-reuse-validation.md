# Scientific validation 56 -- reusable WL refinement buffers

This record evaluates H12: whether call-local buffer reuse improves
projection-level Weisfeiler--Lehman (WL) refinement without changing any
observable result. The committed predecessor is H11 commit `ab6556a`. Results
were collected on 2026-08-22 under the controlled environment documented in
the [optimization ledger](../engineering/optimization-ledger.md).

## 1. Falsifiable hypotheses

| ID | Claim | Failure evidence |
|---|---|---|
| H1 | Reused buffers return exactly the committed predecessor's labels for valid inputs and round counts. | Any exact or generated vector differs from an independent fresh-allocation oracle. |
| H2 | Empty, singleton, path, ring, disconnected, repeated-label, parallel-edge, and self-loop examples retain documented results. | Any fixture differs in labels, classes, cardinality, or ordering. |
| H3 | Repetition and edge-insertion permutation are deterministic. | Repeated bytes or duplicate/reversed-edge projections differ. |
| H4 | Joint-vocabulary similarity is reflexive, symmetric, finite, and in the unit interval. | Self-similarity differs from one, pair order changes bits, or a score lies outside $`[0,1]`$. |
| H5 | Malformed public projections fail before indexed traversal and iteration requests remain capped at 16. | Panic, partial aligned result, or above-cap output differs from round 16. |
| H6 | Parser-built call projections compose without mutating the CPG. | Parser case fails, result misaligns, or graph shape changes. |
| H7 | Graph depth and width do not consume proportional native stack. | A 100,000-node path or star overflows a 256 KiB worker stack. |
| H8 | Reuse reduces isolated WL allocation traffic and does not regress representative timing. | DHAT blocks do not fall or an isolated/corpus Criterion case exceeds the rejection boundary. |
| H9 | `FxHasher` may be retained only if it preserves public numeric compatibility as well as performance. | It changes raw digest ordering or the fixed public mixer contract. |

## 2. Correctness experiments

The focused suite contains 13 unit tests. An independent oracle copies the H11
fresh-allocation algorithm and separately spells its mixer rather than calling
production helpers. It compares exact outputs for zero, one, four, 16, and
hostile `usize::MAX` requests across empty, singleton, self-loop, path, ring,
and disconnected fixtures.

Two `proptest` campaigns run 256 generated cases each:

1. isomorphic sparse-identifier relabeling compares single-graph and joint-
   vocabulary production output against the predecessor oracle, reverses edge
   enumeration, duplicates edges, and compares logical clone partitions; and
2. independently generated graph pairs test repeated output, insertion
   permutation, self-similarity one, bit-exact symmetry, finite values, and the
   closed unit interval.

Exact pre-existing tests additionally cover two directed triangles, outward
stars, weights, out-of-universe endpoints, source/sink seeds, malformed CSR,
clone thresholds/order, fixed mixer vectors, and iteration caps. The public
integration suite now includes a parser-built four-function Rust call path.

The all-feature results were:

| Gate | Result |
|---|---|
| `cargo test --all-features analysis::wl::tests --lib` | 13 passed; 1,254 filtered; 1,267-test library census |
| `cargo test --all-features --test wl_integration` | 2 passed, including parser construction |
| `cargo test --all-features --test wl_stack_safety` | 1 passed: 100,000-node path and star on 256 KiB |

## 3. Separable timing experiment

Criterion used the committed H11 baseline, CPUs 0--1, performance governor,
one-second warm-up, three-second measurement, and 20 samples. The retained
clear-and-push buffers improve every isolated case:

| Case | H11 | H12 | Criterion central change |
|---|---:|---:|---:|
| labels / 1,024-node ring | 53.517 µs | 52.794 µs | -1.3724% |
| clone classes / 1,024-node ring | 56.744 µs | 55.890 µs | -1.5133% |
| labels / 8,192-node ring | 425.66 µs | 420.20 µs | -1.2583% |
| clone classes / 8,192-node ring | 451.51 µs | 444.34 µs | -1.6455% |

An earlier safe zero-filled prototype was rejected: labels and clone classes at
8,192 nodes regressed 1.3097% and 1.2644% respectively. This negative result
identified redundant initialization writes; replacing them with
capacity-retaining clear-and-push produced the retained result.

The 10-sample end-to-end corpus controls include parsing, CPG construction,
call resolution, projection, strongly-connected components, and WL:

| Corpus | H11 | H12 | Interpretation |
|---|---:|---:|---|
| Rust, about 50 lines | 382.11 µs | 386.05 µs | +1.0354%, within noise threshold |
| Rust, about 500 lines | 7.0556 ms | 7.0630 ms | +0.1115%, within noise threshold |
| Rust, about 5,000 lines | 99.142 ms | 98.933 ms | -0.2458%, no change detected |
| Python, JavaScript, Go, C | 1.0974 ms | 1.1017 ms | +0.3840%, within noise threshold |

The corpus boundary is dominated by parsing and graph construction. Its neutral
result neither hides nor overstates the isolated WL improvement.

## 4. Headless allocation and memory experiment

The repository profiler executed ten repetitions per workload. Dynamic Heap
Analysis Tool (DHAT), Massif, `/usr/bin/time -v`, and `perf stat -r 3` ran
without a graphical interface.

| Workload / measure | H11 | H12 | Change |
|---|---:|---:|---:|
| WL DHAT total bytes | 52,467,628 | 52,343,308 | -124,320 (-0.237%) |
| WL DHAT allocation blocks | 10,474 | 10,294 | -180 (-1.719%) |
| WL DHAT peak bytes / blocks | 1,978,825 / 299 | 1,978,825 / 299 | unchanged |
| WL maximum RSS | 5,484 KiB | 5,440 KiB | -44 KiB (-0.802%) |
| WL Massif page peak | 12,398,592 B | 12,398,592 B | unchanged |
| corpus DHAT total bytes | 504,120,085 | 504,045,045 | -75,040 (-0.015%) |
| corpus DHAT allocation blocks | 2,814,466 | 2,813,926 | -540 (-0.019%) |
| corpus DHAT peak bytes / blocks | 16,447,263 / 62,636 | 16,447,263 / 62,636 | unchanged |
| corpus maximum RSS | 23,696 KiB | 23,204 KiB | -492 KiB (-2.076%) |
| corpus Massif page peak | 30,031,872 B | 30,035,968 B | +4,096 B (+0.014%) |

The exact 180-block WL reduction equals nine avoided allocations for each of
20 four-round calls in the ten-repetition workload (`wl_labels` plus clone
classes). The corpus executes more projections and removes 540 blocks. Peak
live memory is expected to remain unchanged because current and next matrices
must coexist during a round.

Perf elapsed time changed from 0.068090 to 0.067639 seconds for WL and from
1.033464 to 1.041749 seconds for the corpus. Instructions changed by +0.125%
and -0.023% respectively; cache/TLB counters varied more than these effects and
are treated as secondary noise. Last Branch Record sampling was unavailable,
so the script used its DWARF call-graph fallback. Headless Intel VTune CLI was
attempted: Hotspots was blocked by host `ptrace_scope`, while Memory Access
rejected the AMD microarchitecture/driver configuration. No host policy was
changed and no GUI was opened.

## 5. Hasher prototype and compatibility decision

A temporary 30-sample Criterion target hashed 8,192 representative three-word
WL signatures. The fixed compatible mixer measured 32.169 microseconds;
`FxHasher` measured 8.315 microseconds, approximately 3.87 times as fast.

The prototype is rejected and fully removed. Canonical labels are sorted ranks
of raw digests, so changing digest ordering can change public numeric labels
even without a collision. The fixed mixer also has published exact vectors and
cross-release stability semantics. A faster but incompatible result does not
satisfy H9.

## 6. Stack and security interpretation

The 100,000-node test constructs both a path (deep topology) and an outward
star (wide topology) inside a 256 KiB worker. It performs four rounds, repeated
determinism checks, clone grouping, and explicit drops. The implementation uses
only heap vectors and bounded loops; it contains no recursive graph traversal.

The test establishes native-stack independence for those lifecycle shapes. It
does not cap heap consumption. A hostile-input service must admit projections
by node/edge/byte budgets, and equal WL labels must never authorize a code
rewrite or security decision without stronger semantic evidence.

## 7. Reproduction

```text
cargo test --all-features analysis::wl::tests --lib
cargo test --all-features --test wl_integration
cargo test --all-features --test wl_stack_safety
taskset -c 0,1 cargo bench --all-features --bench similarity_wl -- --baseline h12-before --noplot
taskset -c 0,1 cargo bench --all-features --bench analyze_corpus -- --baseline h12-before --noplot
LIBCPG_PROFILE_REPETITIONS=10 LIBCPG_PROFILE_OUTPUT_DIR=target/verification/h12-wl ./scripts/profile.sh similarity_wl
LIBCPG_PROFILE_REPETITIONS=10 LIBCPG_PROFILE_OUTPUT_DIR=target/verification/h12-corpus ./scripts/profile.sh analyze_corpus
```

Validation output is captured under `target/verification`, inspected, and
deleted exactly after the stable commit. The temporary hasher source, Cargo
registration, Criterion data, and executable are likewise removed.

## References

1. Shervashidze, N., Schweitzer, P., van Leeuwen, E. J., Mehlhorn, K. M., and
   Borgwardt, K. M. (2011). *Weisfeiler-Lehman Graph Kernels.* Journal of
   Machine Learning Research 12(77), 2539--2561.
   [JMLR article and PDF](https://jmlr.org/papers/v12/shervashidze11a.html).
2. Knuth, D. E. (1984). *Literate Programming.* The Computer Journal 27(2),
   97--111. DOI:
   [10.1093/comjnl/27.2.97](https://doi.org/10.1093/comjnl/27.2.97).
