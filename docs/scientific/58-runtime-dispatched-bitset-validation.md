# Scientific validation 58 -- runtime-dispatched bit-set kernels

This record evaluates H14A: whether runtime-dispatched AVX2 union and
intersection improve wide dense data-flow joins without changing lattice
semantics, short-width behavior, portability, allocation, or native-stack
bounds. The committed predecessor is H13 commit
`5a81e2963301b62788193edd2fb7b5841b826016`.

## 1. Falsifiable hypotheses

| ID | Claim | Failure evidence |
|---|---|---|
| H1 | Selected union and intersection equal the scalar word oracle for every width, pattern, tail, and change flag. | Any word or returned Boolean differs. |
| H2 | The dispatcher never executes AVX2 without runtime support and every non-x86 target selects scalar. | Illegal instruction, unsupported forced selection, or non-x86 AVX symbol/codegen. |
| H3 | The SIMD boundary performs no out-of-range, uninitialized, aliased, or misaligned-invalid access. | Miri or AddressSanitizer failure, or an unsatisfied documented precondition. |
| H4 | Public liveness, available-expression, and very-busy-expression results remain exact. | Any scalar/selected analysis report differs. |
| H5 | Width and concurrency do not consume input-shaped native stack. | A 100,000-word or 100,000-node case overflows a 256 KiB worker stack. |
| H6 | A measured crossover excludes widths where dispatch or tails outweigh vector work. | A retained width regresses reproducibly or an excluded width wins reproducibly. |
| H7 | Primitive improvement remains visible under matched headless profiles without hidden allocation. | Checksums, DHAT counts, or primary counters contradict the claim. |
| H8 | End-to-end claims are limited by the fraction of analysis time spent in bit-set kernels. | Documentation generalizes primitive speedup to workloads dominated by other stages. |

## 2. Correctness campaigns

The production scalar implementation is the executable differential oracle.
Test-only dispatch selection is thread-local and restored by a `Drop` guard,
including during unwinding. It refuses to force AVX2 on unsupported hardware.

| Campaign | Coverage | Result |
|---|---|---|
| Generated word vectors | arbitrary pairs from zero through 260 words; selected union/intersection/subset and change flags compared with scalar | passed |
| Exact boundaries | 0, 1, 63, 64, 65, 255, 256, 257, 511, 512, 513, 767, 768, 769, 1,023, 1,024, 1,025, 8,191, 8,192, 8,193, and 100,000 bits; alternating and sparse patterns | passed |
| Concurrency | eight workers, 100,000-bit domains, 64 repetitions each | passed on 256 KiB stacks |
| Public liveness | parser-built Rust CFG/DFG plus chain, diamond, loop, 100,000-node deep, and 100,000-variable wide cases | scalar and selected reports equal |
| Public Must analyses | 1,100 structural expressions through available and very-busy analysis | scalar and selected reports equal |
| Complete namespace | all data-flow unit and integration cases | 99 passed |
| Strict static gates | all targets and features | format and Clippy passed |

The generated campaign is property-based: vector contents, lengths, and
change/no-change outcomes vary independently. The boundary campaign is
example-based: it targets every dispatch, four-word chunk, partial-word bit
width, and scalar-tail transition explicitly. These methods are complementary;
random generation does not replace named unsafe-boundary cases.

## 3. Unsafe-code and target evidence

The AVX2 functions accept equal-length Rust slices and operate on the largest
four-word prefix. Unaligned loads and stores make `Vec<u64>` alignment
sufficient. A scalar suffix handles zero through three remaining words.

| Gate | Scope | Result |
|---|---|---|
| Miri | focused scalar dispatcher, boundary widths, and tails | passed in 120.17 s |
| native x86-64 AddressSanitizer | property differential through the selected AVX2 path | passed |
| `wasm32-unknown-unknown` check | no-default-feature library | passed; scalar-only cfg |
| aarch64 all-feature and no-default attempts | cross compilation | blocked before libcpg by missing host `aarch64-linux-gnu-gcc`; tree-sitter C also requires it |

The aarch64 attempt is an external toolchain limitation, not passing execution
evidence. Compile-time cfg removes x86 imports, target-feature functions, and
runtime detection from non-x86 builds; the wasm build independently checks
that boundary. No recursive call path was introduced.

## 4. Controlled environment and predecessor

Measurements ran 2026-08-23 on an AMD Ryzen Threadripper PRO 5975WX under the
`performance` governor with CPUs 0 and 1 pinned. The toolchain was Rust 1.97.1
and LLVM 22.1.6. Criterion used 30 samples, one-second warm-up, and two-second
measurement for the dedicated primitive target. Profiles were headless; no
heaptrack or graphical frontend ran.

The committed H13 scalar implementation is the control. Disassembly showed
that LLVM already auto-vectorized union and intersection into two-lane SSE2
batches, so H14A is compared with a vectorized compiler baseline rather than a
naive one-word loop. Subset retained a scalar early exit.

## 5. Primitive baseline and crossover

The dedicated `bitset_domain` target measures union, intersection, and a true
subset control at 17 bit widths. Fixture construction and exact result checks
run outside the timed closure. Representative H13 scalar medians were:

| Bit width | Words | Union | Intersection |
|---:|---:|---:|---:|
| 0 | 0 | 2.919 ns | 2.957 ns |
| 1 | 1 | 13.699 ns | 13.640 ns |
| 256 | 4 | 15.411 ns | 15.383 ns |
| 1,024 | 16 | 18.062 ns | 18.177 ns |
| 8,192 | 128 | 51.836 ns | 49.493 ns |
| 65,536 | 1,024 | 311.06 ns | 363.21 ns |

Two candidate thresholds were tested rather than inferred:

| Candidate | Evidence | Decision |
|---|---|---|
| four words | exact four-word union improved 2.92% and intersection 2.08% (the latter within noise); five-word union regressed 12.75% and intersection 9.74% | reject because one vector plus scalar tail is adverse |
| eight words | 511/512-bit cases improved about 13--15%; 513-bit tail cases were within noise; 767--769-bit cases improved about 12--23% | retain |

At larger widths, repeated controlled comparisons produced these ranges:

| Bit width | Union change | Intersection change |
|---:|---:|---:|
| 1,024 | -5.3% to -6.3% | -4.6% to -5.7% |
| 8,192 | -29.5% to -32.6% | -21.9% to -24.6% |
| 65,536 | -32% to -38% | -30% to -31% |

Negative change means faster. The threshold is expressed in storage words,
not logical bits: eight words equal 512 stored bits. A domain whose bit width
is not a multiple of 64 still masks unused logical bits through the existing
representation invariant.

## 6. Matched headless primitive profile

The scalar and selected binaries executed the same `bitset_primitives`
workload with identical checksum SHA-256 values. DHAT was byte-for-byte equal:
28,327 allocated bytes in 17 blocks, a 26,165-byte/7-block peak, and 544
bytes/1 block live at exit. Reads and writes were also identical.

| Measure | Scalar | AVX2 selected | Change |
|---|---:|---:|---:|
| elapsed | 0.934740747 s | 0.486038945 s | -48.0% |
| cycles | 3,954,848,216 | 2,054,858,196 | -48.0% |
| retired instructions | 21,720,722,213 | 7,951,624,980 | -63.39% |
| maximum RSS | 4,156 KiB | 4,164 KiB | +8 KiB |

The selected call graph attributed 49.36% of samples to intersection and
47.16% to union, so 96.52% landed in the intended kernels. The eight-KiB RSS
difference is a coarse process measurement and is not presented as an
allocation change; exact DHAT counts are equal.

## 7. End-to-end boundary

Existing public liveness families compared with H13 as follows:

| Family | Central change | Interpretation |
|---|---:|---|
| chain / 512 nodes | -2.25% | modest improvement |
| chain / 4,096 nodes | +1.28% | within noise |
| diamond / 64 rungs | +0.05% | unchanged |
| diamond / 512 rungs | +0.98% | within noise |
| nested loops / 128 nodes | -1.05% | within noise |
| nested loops / 1,024 nodes | +0.69% | within noise |

A deliberately wide selected-kernel liveness profile took 0.616766 s,
2,419,051,789 cycles, and 5,016,502,961 instructions with 5,476 KiB maximum
RSS. DHAT reported 329,419,824 allocated bytes in 1,446,957 blocks, peaking at
2,615,722 bytes in 8,925 blocks. `variable_facts` accounted for 19.12% of
samples, `BTreeMap` insertion for 9.18%, and `BinaryHeap::pop` for 8.85%; each
AVX2 function was below the 0.5% reporting threshold.

This is the expected whole-system boundary. Accelerating one kernel changes
total time in proportion to that kernel's original share, as formalized by
Amdahl's law. The primitive win is real, while fact discovery and ordered
worklist operations dominate this selected end-to-end workload.

## 8. Reproduction

```text
./scripts/bench.sh bitset_domain
cargo test --all-features analysis::dataflow
cargo clippy --all-targets --all-features -- -D warnings

LIBCPG_PROFILE_CPUS=0,1 \
LIBCPG_PROFILE_REPETITIONS=100 \
./scripts/profile.sh bitset_primitives

LIBCPG_PROFILE_CPUS=0,1 \
LIBCPG_PROFILE_REPETITIONS=1 \
./scripts/profile.sh dataflow_liveness_wide
```

The profile script builds and invokes only command-line tools. Captures live
under `target/verification`, are inspected after each gate, and are deleted by
exact path after the stable checkpoint. Nothing is built or profiled in
`/tmp`, which may be a memory-backed filesystem.

## 9. Interpretation and limits

H1--H5 pass on the available targets and sanitizers. H6 passes for the retained
eight-word crossover and explicitly fails the rejected four-word policy. H7
passes for the matched primitive profile. H8 is honored by reporting neutral
end-to-end families and the fact-construction hot path alongside the primitive
gain.

These results do not predict ARM NEON, SVE, AVX-512, other microarchitectures,
or sparse domains. Such candidates require their own direct-load design,
differential oracle, exact tails, sanitizer/target gates, and measured
crossover. They also do not justify vectorizing an early-exit predicate merely
because a vector instruction exists.

## References

1. Intel Corporation. *Intel Intrinsics Guide.*
   [Official reference](https://www.intel.com/content/www/us/en/docs/intrinsics-guide/index.html)
   (no DOI).
2. The Rust Project. *`std::arch`.*
   [Official reference](https://doc.rust-lang.org/std/arch/index.html)
   (no DOI).
3. Amdahl, G. M. (1967). *Validity of the Single Processor Approach to
   Achieving Large Scale Computing Capabilities.* AFIPS Spring Joint Computer
   Conference, 483--485. DOI:
   [10.1145/1465482.1465560](https://doi.org/10.1145/1465482.1465560).
4. Georges, A., Buytaert, D., and Eeckhout, L. (2007). *Statistically
   Rigorous Java Performance Evaluation.* OOPSLA '07, 57--76. DOI:
   [10.1145/1297027.1297033](https://doi.org/10.1145/1297027.1297033).
