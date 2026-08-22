# Scientific validation 52 — Dense stack-safe VF2 state

## Claims under test

The H08 hypothesis is that deterministic VF2 search can replace hash-owned
candidate state with dense graph-aligned arrays and cursor-only heap frames,
substantially reducing search time and allocation without changing the exact
embedding sequence or moving logical depth onto the native stack.

The experiment tests six claims:

1. strict and relaxed results equal an independent exhaustive injection oracle
   on bounded labelled directed multigraphs;
2. self-loops, direction, non-induced extra target edges, parallel-edge
   existential semantics, disconnected patterns, and injectivity are retained;
3. insertion permutations of either graph retain the exact result sequence and
   every positive-cap prefix;
4. dense ordered-set operations equal an independent ordered-set model;
5. VF2-only timing and allocation improve against committed H07; and
6. a depth-100,000 public match completes with 256 KiB of native stack.

It does not claim a better asymptotic worst case, a deadline for a no-match
search, induced-subgraph semantics, or VF2++/VF3 candidate ordering.

## Experimental design

The controlled predecessor is H07 commit `783eb18`, which supplied the lazy
exact-edge projection cache. Its `pattern_vf2` fixture constructs a four-node
directed path and searches a 256- or 4,096-node target path with a one-result
cap. The oracle requires exactly one complete four-node mapping.

Profiling initially repeated fixture construction. That mixed workload was
rejected because graph construction dominated the samples and did not measure
the candidate-state hypothesis. The corrected `pattern_vf2` profile constructs
both graphs and the matcher once, then repeats only `find_matches`; the aggregate
`all` workload deliberately retains end-to-end construction semantics.

Criterion used one-second warm-up, twenty samples, optimized feature-free
binaries, and H07's saved baseline. Corpus controls used the all-language
fixture from validation 51 with ten samples and the same parser/result oracles.
All profilers ran headlessly; no heaptrack or graphical frontend was opened.

## Controlled timing result

The final fully lazy implementation reports:

| Target size | H08 in H07 comparison | Change from H07 | H08 in `main` comparison | Change from immutable `main` |
|---:|---:|---:|---:|---:|
| 256 nodes | 949 ns | -79.89% | 940 ns | -80.26% |
| 4,096 nodes | 1.977 µs | -96.91% | 1.999 µs | -96.83% |

Both independently sampled comparisons are statistically significant under
Criterion's bootstrap test. Their slightly different H08 central estimates are
ordinary run-to-run variation. The larger relative change is expected: H07 repeatedly collected and
sorted a target candidate set whose size grew with the target, while H08
advances one Fenwick cursor and stops after the first retained embedding.

## Allocation, peak, and hardware-counter result

The search-only driver repeats the 4,096-node one-match workload 10,000 times.
H07 and H08 use identical retained fixtures and emit checksum `10000`.

| Measure | H07 | H08 | Change |
|---|---:|---:|---:|
| DHAT total allocated bytes | 1,084,191,021 | 788,440,397 | -27.28% |
| DHAT allocation blocks | 360,080 | 220,088 | -38.88% |
| DHAT peak live bytes | 1,281,059 | 1,204,139 | -6.00% |
| Massif page-accounting peak | 11,292,672 | 11,268,096 | -0.22% |
| wall time | 0.66 s | 0.02 s | -96.97% |
| cycles | 2,709,311,184 | 89,014,438 | -96.71% |
| retired instructions | 9,310,578,136 | 238,295,653 | -97.44% |
| cache references | 64,777,356 | 22,392,952 | -65.43% |
| cache misses | 1,266,444 | 192,717 | -84.78% |
| page faults | 580 | 567 | -2.24% |
| dTLB loads | 499,454 | 9,280 | -98.14% |
| dTLB load misses | 12,551 | 1,796 | -85.69% |
| maximum RSS | 4,400 KiB | 4,324 KiB | -1.73% |

DHAT, Massif, and process RSS all report lower peaks in the final rerun. RSS is
still a coarse startup-sensitive sample at this scale, so the evidence supports
lower allocation traffic and instruction work without claiming that every
operating-system memory sample must decrease on another run.

The H07 sampled profile attributed 41.58% of recorded samples to hash-set
collection, 28% to sorting, 8% to vector collection, and 7.33% to candidate-pair
construction. These disappear as dominant paths in the dense implementation.

## End-to-end corpus control

The fully lazy node metadata is important because most corpus cases never run
VF2. Against H07:

| Corpus | H08 time | Change | Criterion verdict |
|---|---:|---:|---|
| Rust, approximately 50 lines | 383.57 µs | -4.83% | improvement |
| Rust, approximately 500 lines | 7.188 ms | -4.12% | improvement |
| Rust, approximately 5,000 lines | 111.29 ms | -3.33% | within noise threshold |
| Python + JavaScript + Go + C | 1.084 ms | -1.84% | improvement |

An eager distinct-degree prototype regressed the small and cross-language
cases by approximately 4.2%. Making only degrees lazy still produced noisy
medium/large regressions because node ids were copied eagerly. The accepted
design makes ids and degrees one fully lazy view. This isolates the VF2 cost at
its actual consumer instead of masking an eager charge elsewhere.

## Correctness and metamorphic oracles

One 128-case campaign enumerates every injection for random patterns of at most
four nodes and targets of at most eight nodes. An independent iterative oracle
checks node-kind compatibility, directed incident edges, self-loops, parallel
edges, and both strictness axes, then compares the exact set of mappings with
production VF2.

The earlier bounded recursive specification remains as a second differential
oracle. It is intentionally test-only and bounded; production remains an
explicit heap machine. Separate properties require:

- identical embedding sequences after both pattern and target insertion
  permutations;
- monotone canonical prefixes for positive result caps;
- relaxed results to contain strict results;
- unchanged results after adding a strictly incompatible isolated target node;
  and
- every `DenseSet` membership, population, first element, and successor query
  to equal an independent `BTreeSet` after generated insert/remove sequences.

Examples distinguish non-induced matching from induced matching: an extra
reverse incompatible target edge does not reject an otherwise valid mapping,
but reversing the required pattern edge does. A two-node disconnected pattern
into three compatible target nodes yields all six injections. Parallel pattern
edges preserve the established existential compatible-edge contract rather
than imposing target multiplicity.

## Integration and stack-safety boundary

The parser integration builds a Rust CPG, asks a strict matcher for its first
self-embedding twice, requires a complete mapping, identical result
order, and unchanged node/edge counts. This crosses parsing, lazy projection
construction, dense metadata, and capped search.

A worker with a 256 KiB native stack matches a public 100,000-node directed path.
Search frames, mapping arrays, Fenwick trees, terminal counts, and delta logs
are heap owned. Compact-frame tests require `Vf2SearchFrame` to remain at most
16 bytes. The gate proves constant native call depth for this fixture; it does
not bound heap or factorial search work on adversarial graphs.

## Feature and gate matrix

| Command family | Evidence |
|---|---|
| focused `pattern::vf2` tests | exact semantics, dense-set model, differential/metamorphic properties, compact frames |
| feature-free library suite | all graph and pattern regressions with no parser grammar |
| `cargo test --all-features --test integration` | parser-built deterministic strict self-match and graph immutability |
| `cargo bench --bench pattern_vf2` | H07 and immutable-`main` controlled comparisons |
| `cargo bench --all-features --bench analyze_corpus` | non-VF2 end-to-end control boundary |
| headless search-only profile | RSS, DHAT, Massif, perf counters, and hot paths |
| `cargo clippy --all-targets --all-features -- -D warnings` | warning-free complete target surface |
| `cargo doc --all-features --no-deps` | rustdoc surface |
| `./scripts/verify.sh` | format, feature matrices, tests, examples, rustdoc, docs, and diff hygiene |

## Falsification boundaries

Generated exact-oracle graphs are intentionally bounded so exhaustive
enumeration terminates. Criterion measures one constrained path and one result;
it does not predict dense symmetric or unsuccessful searches. A positive result
cap bounds output and stops at a canonical prefix, but cannot bound work before
the next match. Applications accepting hostile graphs must enforce graph-size
and outer time/work admission independently.

The distinct-degree prune counts unique directed neighbours, not compatible
edge labels or multiplicity. Stronger typed/fringe cardinality rules may be
valuable, but are not part of the validated claim until proved sound for
non-induced typed multigraphs.

## References

1. Cordella, L. P., Foggia, P., Sansone, C., and Vento, M. (2004). *A
   (Sub)Graph Isomorphism Algorithm for Matching Large Graphs.* IEEE
   Transactions on Pattern Analysis and Machine Intelligence 26(10). DOI:
   [10.1109/TPAMI.2004.75](https://doi.org/10.1109/TPAMI.2004.75).
2. Fenwick, P. M. (1994). *A New Data Structure for Cumulative Frequency
   Tables.* Software: Practice and Experience 24(3). DOI:
   [10.1002/spe.4380240306](https://doi.org/10.1002/spe.4380240306).
