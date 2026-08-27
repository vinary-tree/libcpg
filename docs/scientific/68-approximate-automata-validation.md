# Scientific validation 68 -- Approximate automata and structural indexing

## Claims under test

The production claim has five parts:

1. all four edit methods agree with independent small-domain oracles on their
   supported semantics;
2. typed complete weighted relations obey extensional identity and
   associativity;
3. the two-level structural index preserves token equality, reversible
   vocabulary lookup, duplicate candidate multiplicity, deterministic order,
   and explicit incompleteness;
4. no input depth maps to native call depth across construction, query,
   evidence lifecycle, serialization, or destruction; and
5. the selected one-shot/reusable split follows measured workload evidence.

The validation does not claim that approximate distance proves program
equivalence, that every generalized cost is a metric, that incomplete
observations compose associatively, or that a directed acyclic word graph
(DAWG) beats a direct scan for every corpus.

## Independent semantic oracles

`tests/approximate_matching_properties.rs` exhausts bounded binary strings and
compares public standard distance with an independent two-row dynamic-program
oracle. It checks direct/composed parity, all limit outcomes, invalid weights,
epsilon and zero-cost paths, insertion-order determinism, normalization policy,
thread-count invariance, score normalization, and increased-budget behavior.

`tests/approximate_token_index.rs` compares indexed standard search with an
independent token-unit oracle over exhaustive and generated corpora. A
bijective-renaming property replaces every term by an arbitrary unique name and
requires identical candidate IDs and costs. Separate examples cover OSA,
unrestricted Damerau, generalized merge/split, repeated unknown terms,
different unknown terms, wrong tape domains, duplicate stable IDs, identical
candidate sequences, and every build/search cap.

The relation suite enumerates complete finite denotations and compares
composition with a direct join oracle. It tests left identity, right identity,
and associativity by input/output tape and tropical weight, never internal
state shape. Limited observations retain an explicit completion reason and are
excluded from category-law claims.

## End-to-end composition example

`tests/approximate_automata_pipeline.rs` executes the complete retained chain:

```text
duallity lexical aliases
    -> libcpg marker-typed lling relation composition
    -> complete normalized API-event map
    -> libdictenstein term <-> u64 vocabulary
    -> libdictenstein sequence<u64> DAWG
    -> liblevenshtein bounded neighborhood
    -> exact API temporal confirmation
```

Two raw spellings normalize to the same API event; two candidates normalize to
the same sequence and both survive indexing. The approximate result remains
advisory until the fixture checks the original typed event order.

## Deep stack and resource experiments

The public stack gate runs worker threads with 256 KiB native stacks. It covers
20,000 and 100,000-depth immutable projections, a 100,000-candidate canonical
search/rank/clone/debug/serialization/drop lifecycle, and a 100,000-token
vocabulary/DAWG build/query/clone/debug/drop lifecycle. The debug index gate
completed in about 140 seconds; the release gate completed in about 13 seconds.

All memory-intensive compilation, tests, and benchmarks run headlessly in a
user `systemd-run` scope with `MemoryMax=4G`, `MemorySwapMax=0`,
`TasksMax=128`, and `CARGO_BUILD_JOBS=1`. No GUI profiler participates.

## Structural performance evidence

The retained Criterion fixture uses five-unit structural sequences at 256 and
4,096 candidates, with exact and one-substitution queries. Every timed engine
is checked for the same best stable candidate before measurement.

| Candidates | Case | Independent scan | Reused index | One-shot adapter |
|---:|---|---:|---:|---:|
| 256 | exact | 22.86 us | 130.94 us | 267.92 us |
| 256 | noisy | 21.24 us | 122.34 us | 280.44 us |
| 4,096 | exact | 367.09 us | 2.045 ms | 4.558 ms |
| 4,096 | noisy | 326.52 us | 2.014 ms | 4.471 ms |

The independent scan is a correctness/performance control, not production
code. A prior automatic build-and-query implementation took about 0.724 ms at
256 candidates and 12.5 ms at 4,096 candidates. The root correction is an API
ownership decision: one-shot calls use the pair adapter; repeated queries own
and reuse `ApproximateTokenIndex`.

At 4,096 candidates, building the index performed 113,882 allocations totaling
10,696,683 bytes. Reused exact/noisy queries performed 19,967/19,832
allocations totaling 1,949,058/1,943,826 bytes. The implementation's
conservative algorithm-accounted peak was 28,664,336 bytes. These values are
regression baselines, not universal complexity constants.

## Gate matrix

| Command family | Evidence |
|---|---|
| `cargo test --features approximate-matching --test approximate_matching_properties` | exhaustive oracle, laws, limits, determinism |
| `cargo test --features approximate-matching --test approximate_token_index` | two dictionaries, all methods, renaming property, caps |
| `cargo test --features approximate-matching --test approximate_automata_pipeline` | four-library composition and exact confirmation |
| `cargo test --features approximate-matching --test approximate_matching_stack_safety` | 20,000/100,000 depth on 256 KiB stacks |
| `cargo bench --bench approximate_matching --features approximate-matching` | quality, latency, allocations, engine selection |
| `cargo clippy --all-targets --features approximate-matching -- -D warnings` | warning-free feature target surface |
| `./scripts/verify.sh` and `pgmcp bug-gate` | terminal repository and policy gates |

## References

1. Schulz, K. U., Mihov, S. (2002). *Fast String Correction with Levenshtein
   Automata.* DOI:
   [10.1007/s10032-002-0082-8](https://doi.org/10.1007/s10032-002-0082-8).
2. Damerau, F. J. (1964). *A Technique for Computer Detection and Correction of
   Spelling Errors.* DOI:
   [10.1145/363958.363994](https://doi.org/10.1145/363958.363994).
3. Mohri, M. (1997). *Finite-State Transducers in Language and Speech
   Processing.* Computational Linguistics 23(2), 269--311.
   [ACL Anthology J97-2003](https://aclanthology.org/J97-2003/).
4. Knuth, D. E. (1984). *Literate Programming.* DOI:
   [10.1093/comjnl/27.2.97](https://doi.org/10.1093/comjnl/27.2.97).
