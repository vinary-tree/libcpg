# Scientific validation 53 — Borrowed-CSR dominator optimization

## Claims under test

The H09 hypothesis is that the existing deterministic, stack-safe
Cooper–Harvey–Kennedy (CHK) engine can consume borrowed compressed-sparse-row
(CSR) adjacency and a synthetic-exit overlay, substantially reducing dominance
and Program Dependence Graph (PDG) cost without changing any represented
dominance, post-dominance, control-dependence, ordering, error, or stack-safety
contract.

This claim has four independently falsifiable parts:

1. immediate dominators and constant-time dominance intervals equal independent
   graph-theoretic oracles;
2. dominance frontiers implement the strict-dominance definition, including
   self-loops and backedges into the distinguished root;
3. post-dominators, infinite-region attachment, and PDG control edges equal the
   former petgraph-based production relation under storage permutations; and
4. controlled direct, consumer, allocation, and hardware-counter measurements
   improve against the immediately preceding H08 checkpoint while graph-depth
   work remains on the heap.

## Experimental design

### Fixtures and baselines

`benches/dominator_analysis.rs` constructs an oracle-checked chain of diamonds.
Each stage contributes two branch arms and one join, so $`s`$ stages contain
$`3s+1`$ vertices and $`4s`$ directed edges. It times two public operations at
256 and 2,048 stages:

- `dominator_tree` over an immutable feature-free `GraphProjection`; and
- `post_dominator_tree` over the equivalent hand-built function CFG.

The fixture is constructed and validated before timing. The headless
`dominator_analysis` workload retains its projection and CPG across 100
repetitions, isolating analysis rather than graph construction. The existing
parser-backed `pdg_construction` target is the consumer control.

The saved `t9-2-h09-h08` baseline is commit `f251bff`, the last stable H08
checkpoint before any H09 production edit. Immutable `main` predates this
epic's public unified dominator API, so a same-API direct-main baseline does not
exist; inventing one would not be a valid comparison. The parser-backed PDG
control is therefore reported against H08 as the isolated predecessor, while
the optimization ledger retains the older cumulative `main` measurements.

### Controlled environment

The measurements use the repository protocol: AMD Ryzen Threadripper PRO
5975WX, CPUs 0 and 1 on one L3/CCD, `performance` governor, Rust 1.97.1,
Criterion 0.5.1, perf, Valgrind 3.25.1 DHAT/Massif, and disk-backed artifacts
under `target/verification`. No heaptrack process or graphical profiler was
started. Intel VTune command-line collectors were unavailable under the host
policy; perf used its headless DWARF fallback.

## Correctness experiments

### Vertex-removal oracle

For each root-reachable node $`n`$, the independent oracle removes candidate
$`d`$ and performs an iterative reachability traversal. It defines

```math
d\operatorname{\ dom}n
\iff d=n\lor n\notin\operatorname{Reach}(G-d,r).
```

It derives the immediate dominator as the unique strict dominator dominated by
every other strict dominator, then derives each frontier directly from the
published definition rather than from a CHK fixed point. Production `idom`,
`dominates`, and every frontier row must equal this oracle.

The deterministic exhaustive campaign enumerates all loop-free edge subsets
for one through four vertices: $`1+2^2+2^6+2^{12}=4,165`$ rooted directed
graphs. A 256-case generated campaign extends this to ten vertices and includes
self-loops and repeated input edges. A named irreducible fixture includes
multiple backedges and an unreachable competing entry.

The oracle initially refuted production on a one-node root self-loop. The
failure was not discarded: the frontier walk was corrected to model the root's
implicit external predecessor, and a named root-self-loop/backedge regression
plus its minimized proptest seed now pin the result.

### Differential and permutation oracles

- 256 generated graphs compare immediate dominators with
  `petgraph::algo::dominators::simple_fast` and require byte-identical results
  under reversed node/edge input.
- 4,164 exit-reachable CFG edge subsets through four vertices compare control
  dependence with the former production petgraph algorithm.
- 256 generated CFGs compare post-dominance and emitted PDG control edges with
  that oracle.
- 256 generated CFGs require complete `PostDominatorTree` equality under
  opposite node and edge insertion orders.
- Parsed Rust scope and PDG tests prove the direct AST-scope computation retains
  the previous function boundary and public consumer relation.

### Stack experiment

`tests/dominator_stack_safety.rs` exercises four adversarial 100,000-node
shapes inside a thread with a 256 KiB native stack: a deep projection path, a
wide projection star, a function CFG containing one real exit beside a deep
non-terminating cycle, and a wide function CFG with 99,999 real exits. It
computes and drops both public result trees, checks exact immediate relations,
checks the wide dominator child row, and checks the virtual post-dominator
roots for both the infinite and wide-exit regions. A pass corroborates that
function-scope ordering, reverse postorder, exit reachability, CHK, frontier
construction and ordering, synthetic-exit overlay construction, Euler
intervals, result conversion, and destruction do not make graph depth or
fan-out native call depth.

## Performance results

### Criterion: direct analysis

| Public operation / stages | H08 central time | H09 central time | Criterion change |
|---|---:|---:|---:|
| dominators / 256 | 309.97 µs | 258.32 µs | -16.75% |
| post-dominators / 256 | 509.03 µs | 437.02 µs | -15.28% |
| dominators / 2,048 | 2.6160 ms | 2.2259 ms | -15.06% |
| post-dominators / 2,048 | 4.2222 ms | 3.7292 ms | -11.71% |

All four changes have $`p<0.05`$ in Criterion's comparison and are classified
as improvements. The smaller post-dominator gain is expected because its timed
public boundary still constructs the canonical function projection and public
maps.

### Criterion: parser-backed PDG consumer

| Corpus | H08 central time | H09 central time | Criterion change |
|---|---:|---:|---:|
| Rust small (49 lines) | 57.992 µs | 40.804 µs | -29.76% |
| Rust medium (514 lines) | 3.1532 ms | 2.3207 ms | -27.09% |

This larger end-to-end effect includes removal of the discarded first CFG
projection and tree-allocated control-edge sorting.

### Headless retained-fixture profile

| Counter, 100 direct + post-dominator repetitions | H08 | H09 | Change |
|---|---:|---:|---:|
| elapsed time | 0.7090 s | 0.6342 s | -10.55% |
| DHAT allocated bytes | 709,998,188 | 574,877,164 | -19.03% |
| DHAT allocation blocks | 8,499,332 | 5,808,729 | -31.66% |
| DHAT peak live bytes | 5,386,036 | 4,853,419 | -9.89% |
| Massif page peak | 16,191,488 | 15,372,288 | -5.06% |
| maximum RSS | 9,072 KiB | 8,236 KiB | -9.22% |
| cycles | 2,844,591,088 | 2,458,702,778 | -13.57% |
| retired instructions | 8,545,780,156 | 7,188,471,820 | -15.88% |
| cache references | 105,992,891 | 75,172,548 | -29.08% |
| cache misses | 14,474,220 | 12,475,538 | -13.81% |
| dTLB loads | 1,650,466 | 1,359,584 | -17.62% |
| dTLB load misses | 277,323 | 285,212 | +2.84% |
| page faults | 22,365 | 33,755 | +50.93% |

Page faults and data translation lookaside buffer (dTLB) load misses are the
two adverse counters. They increased despite lower live memory, RSS,
allocation volume, TLB loads, and elapsed time; these startup- and allocator-
sensitive observations are reported rather than reclassified. No claim is
made that every counter or unrelated workload must improve.

## Interpretation and falsification boundary

The evidence supports the representation hypothesis on the named diamond
ladder and parser corpora. It does not prove that CHK beats Lengauer–Tarjan on
every graph, that every CFG converges in few passes, or that public `BTreeMap`
ownership is free. CHK retains its conservative $`O(VE)`$ bound.

The optimization would be refuted by any vertex-removal, petgraph,
post-dominator, PDG, or insertion-order mismatch; by native-stack failure at
100,000 nodes; by a non-improving isolated benchmark; or by moving an edge copy
behind a cache whose mutation semantics are not proved. Those boundaries remain
executable.

## Reproduction

```sh
cargo test --lib analysis::dominators::tests --no-default-features
cargo test --test dominator_stack_safety --no-default-features
cargo test --lib builder::pdg::tests --features lang-rust

cargo bench --bench dominator_analysis -- --baseline t9-2-h09-h08
cargo bench --bench pdg_construction --features lang-rust -- \
  --baseline t9-2-h09-h08

LIBCPG_PROFILE_REPETITIONS=100 \
LIBCPG_PROFILE_OUTPUT_DIR=target/verification/dominator-final \
./scripts/profile.sh dominator_analysis
```

Profiler and Criterion captures are regenerable ignored artifacts. This page
and the [optimization ledger](../engineering/optimization-ledger.md) are the
reviewed persistent record.

## References

1. Cooper, K. D., Harvey, T. J., Kennedy, K. (2001). *A Simple, Fast Dominance
   Algorithm.* Rice University Computer Science Technical Report TR-06-33870.
2. Cytron, R., Ferrante, J., Rosen, B. K., Wegman, M. N., Zadeck, F. K. (1991).
   *Efficiently Computing Static Single Assignment Form and the Control
   Dependence Graph.* ACM TOPLAS 13(4), 451–490. DOI:
   [10.1145/115372.115320](https://doi.org/10.1145/115372.115320).
3. Lengauer, T., Tarjan, R. E. (1979). *A Fast Algorithm for Finding Dominators
   in a Flowgraph.* ACM TOPLAS 1(1), 121–141. DOI:
   [10.1145/357062.357071](https://doi.org/10.1145/357062.357071).
