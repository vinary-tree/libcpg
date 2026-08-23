# Scientific validation 57 -- bounded function-analysis cache

This record evaluates H13: whether exact caller-owned per-function plan reuse
accelerates repeated CPG construction without changing graph results,
weakening dependency semantics, or consuming input-shaped native stack. The
committed predecessor is H12 commit
`089aeb3db716ccde23c7dd791002170eca7d96bc`. Measurements were collected on
2026-08-22/23 under the repository methodology.

## 1. Falsifiable hypotheses

| ID | Claim | Failure evidence |
|---|---|---|
| H1 | Cold, hit, edited, and evicted cached builds equal ordinary fresh graphs exactly. | Any node, edge, root, entry, exit, language, or serialized byte differs. |
| H2 | Digest collisions cannot cause a false hit. | Distinct canonical bytes in one forced bucket return the wrong plan. |
| H3 | Every cache dependency is keyed or conservatively bypassed. | External caller, preceding lexical binding, nested function, receiver field, or crossing PDG edge reuses stale output. |
| H4 | Rebuilt graph identities never leak through cached plans. | Replay emits a foreign or out-of-range `NodeId`. |
| H5 | Entry and byte bounds hold under churn with deterministic LRU behavior. | Occupancy exceeds either limit or repeated order changes the victim. |
| H6 | Serial/parallel PDG reuse and language/Serde domains are deterministic. | Worker count, language reuse, insertion order, or serialization changes output. |
| H7 | Keying, replay, eviction, clear, and drop are stack-safe. | A 100,000-deep or wide case overflows a 256 KiB worker stack. |
| H8 | All-hit and edit-local workloads improve; cold overhead remains small and disabled is neutral. | Retired instructions or controlled timing contradict the stated boundaries. |

## 2. Correctness experiments

Seven unit tests cover forced digest collisions, schema-version separation,
dual-bound deterministic LRU, malformed cyclic ASTs, invalid local ordinals,
PDG insertion-order-independent keys, and equality between the optimized
one-pass source-context scan and independent per-function graph walks.

Nine public integration tests cover:

- cold, repeated-hit, rename, type, call, import, global, struct, and field edit
  examples;
- low-hit churn and eviction;
- zero-capacity exact passthrough;
- all conservative DFG dependency bypass classes;
- serial and parallel PDG equivalence;
- Rust/Python cache-domain separation;
- deterministic Serde bytes;
- 32 generated edit and permutation cases against a fresh oracle; and
- 100,000-deep plus 100,000-wide replay, eviction, clear, and drop on a
  256 KiB worker stack.

The final focused results were:

| Gate | Result |
|---|---|
| `cargo test --all-features --lib builder::incremental` | 7 passed; 1,267 filtered |
| `cargo test --all-features --test incremental_analysis_cache` | 9 passed |
| `cargo clippy --all-targets --all-features -- -D warnings` | passed |

## 3. Immutable H12 control

`git archive` materialized the exact H12 commit under disk-backed
`target/verification`. One external harness compiled H12, then changed only its
path dependency to H13 while retaining identical source and lockfile hashes:

| Input | SHA-256 |
|---|---|
| harness `main.rs` | `4f6d2ad5e6dd51a01476a1f4d776e3e2343094c400e881dd63e9bc0f53d7083a` |
| harness `Cargo.lock` | `d144bf70b9174385b3b6cd9872b703974ac72b66b10601c8b119e0f23d691c4c` |

Each binary built the same 64-function, 32-statement edited corpus 100 times
on CPU 0. Checksums were both 8,093,500.

| Measure | H12 | H13 ordinary path | Change |
|---|---:|---:|---:|
| retired instructions | 47,455,981,663 | 47,555,957,577 | +0.211% |
| three-run elapsed | 4.7444 s | 4.5508 s | -4.08% |
| one-run maximum RSS | 19,500 KiB | 19,444 KiB | -56 KiB |

Instructions are the primary cross-run control because clock frequency and
cache misses varied between campaigns. The +0.211% instruction movement is
small, while elapsed and RSS do not support an ordinary-path regression.

## 4. Criterion timing matrix

Criterion used 10 samples, one-second warm-up, five-second target time, and the
same 64-function corpus. Batched edit cases exclude cache seeding from the
timed closure.

| Workload | Median | Fresh control | Interpretation |
|---|---:|---:|---|
| fresh edit-one corpus | 52.752 ms | self | ordinary control |
| cold empty cache | 51.125 ms | 52.752 ms | overlapping/no supported cold regression |
| all 64 unchanged | 41.733 ms | 52.752 ms | 20.89% faster |
| edit one of 64 | 41.366 ms | 52.752 ms | 21.58% faster |
| shared dependency edit | 1.7979 ms | 1.7194 ms | 4.57% cost for exact DFG bypass on tiny functions |
| half-function churn | 45.982 ms | 54.009 ms | 14.86% faster |
| bounded eviction | 50.500 ms | 52.752 ms | 4.27% faster |
| disabled cache | 50.448 ms | 52.752 ms | ordinary path; run-order noise |

The shared case intentionally changes one global constant read by every
function. CFG plans remain local, but all DFG plans recompute. It is an adverse
soundness control, not an expected high-hit workload.

## 5. Headless instruction, allocation, and memory evidence

Every profiler was command-line-only. No heaptrack or graphical frontend ran.
`perf stat -r 3` pinned the release driver to CPU 0. Fresh, cold, hit, and
disabled use 100 repetitions; two-build edit/churn/eviction scenarios use 20.

| Workload | Instructions per scenario | Relative meaning |
|---|---:|---|
| fresh | 473,283,441 | control |
| cold | 479,538,701 | +1.322% |
| all-hit | 392,340,251 | -17.102% |
| edit-one (seed + edit) | 870,068,029 | two-build scenario |
| shared (seed + edit) | 40,654,678 | small-function two-build scenario |
| half-churn (seed + edit) | 922,398,407 | two-build scenario |
| eviction (seed + edit) | 959,196,085 | two-build scenario |
| disabled | 473,283,527 | +0.000018% |

DHAT ran one scenario and reports allocation traffic:

| Workload | Total bytes | Blocks | Peak live bytes / blocks |
|---|---:|---:|---:|
| fresh | 36,200,614 | 191,950 | 15,226,899 / 42,465 |
| cold | 37,630,032 | 193,453 | 16,158,060 / 43,065 |
| all-hit (seed + hit) | 70,273,073 | 324,235 | 16,187,193 / 43,108 |
| edit-one | 70,373,767 | 325,300 | 16,205,013 / 43,137 |
| shared | 3,901,340 | 28,649 | 962,636 / 2,541 |
| half-churn | 72,932,614 | 358,187 | 16,741,094 / 43,981 |
| eviction | 74,597,001 | 386,708 | 15,567,568 / 42,687 |
| disabled | 36,200,617 | 191,950 | 15,226,902 / 42,465 |

Cold insertion adds 1,429,418 allocated bytes and 1,503 blocks. Disabled is
block-identical to fresh; its three-byte difference is diagnostic output.

Massif `--pages-as-heap=yes` measured peaks of 26,767,360 B fresh and disabled,
27,648,000 B cold, 31,920,128 B all-hit, 31,924,224 B edit-one,
11,497,472 B shared, 33,058,816 B half-churn, and 31,461,376 B eviction.
The seed-plus-replay cases intentionally keep cache entries alive while the new
graph is constructed.

## 6. Resident key share

The cache reports an additive estimated breakdown. After one normal cold
64-function build:

| Resident class | Bytes | Share |
|---|---:|---:|
| canonical keys | 177,670 | 37.98% |
| CFG/DFG plans | 284,036 | 60.71% |
| entry records | 6,144 | 1.31% |
| total | 467,850 | 100% |

The byte limit covers this sum. Allocator and balanced-tree private overhead
remain outside the estimate; the independent entry limit bounds their growth.

## 7. Stack and security interpretation

The deep/wide test runs parsing, exact key formation, cold insertion, hit
replay, deterministic one-entry eviction, graph drop, iterative cache clear,
and cache drop on a 256 KiB stack. It establishes constant native call depth
for those paths, not constant heap use.

Full key comparison makes digest collisions harmless to integrity. Exact
function source bytes remain in memory, so caches must be scoped to one
trusted caller/session and cleared at the lifecycle boundary. Conservative
bypasses preserve ordinary semantics but do not strengthen the underlying
analysis beyond its documented evidence.

## 8. Reproduction

```text
cargo test --all-features --lib builder::incremental
cargo test --all-features --test incremental_analysis_cache
cargo bench --bench reanalyze --features lang-rust -- --noplot
cargo build --release --example profile_workloads \
  --features lang-rust,lang-python,lang-javascript,lang-go,lang-c
LIBCPG_PROFILE_REPETITIONS=100 \
  perf stat -r 3 -e cycles:u,instructions:u,cache-references:u,cache-misses:u \
  target/release/examples/profile_workloads reanalyze_all_hit
```

Validation artifacts are captured below `target/verification`, inspected, and
deleted by exact path after the stable commit. Nothing is written to `/tmp`.

## References

1. Michie, D. (1968). *Memo Functions and Machine Learning.* Nature 218,
   19--22. DOI:
   [10.1038/218019a0](https://doi.org/10.1038/218019a0).
2. Mattson, R. L., Gecsei, J., Slutz, D. R., and Traiger, I. L. (1970).
   *Evaluation Techniques for Storage Hierarchies.* IBM Systems Journal 9(2),
   78--117. DOI:
   [10.1147/sj.92.0078](https://doi.org/10.1147/sj.92.0078).
