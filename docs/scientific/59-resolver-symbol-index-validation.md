# Scientific validation 59 -- resolver symbol-index evaluation

This record evaluates H14B: whether DAT, DynamicDawg, prefix traversal, fuzzy
automata, or their SIMD kernels improve libcpg's exact evidence-bounded call
resolver enough to justify a production dependency. A measured rejection is a
successful result when all semantic oracles run and the prototype is removed.

## 1. Falsifiable hypotheses

| ID | Claim | Failure evidence |
|---|---|---|
| H1 | Every candidate exact result equals an independent ordered-map scan, including duplicate candidate vectors. | Any missing, extra, reordered, or differently classified candidate. |
| H2 | Prefix candidates equal a full `starts_with` scan and preserve canonical `NodeId` order. | Any checksum/order mismatch or omitted prefix. |
| H3 | Distance-two top-64 candidates equal an independent bounded-distance scan. | Any term, distance, or rank differs. |
| H4 | A retained index improves cold whole-resolver work, not lookup alone. | Construction plus representative lookup is neutral/adverse. |
| H5 | Ownership, RSS, allocation, native stack, and destruction remain bounded for 100,000 symbols and deep names. | Unbounded cache, stack overflow, or the resource ceiling is reached. |
| H6 | A registry dependency is correct, publishable, licensed compatibly, and available at the evaluated API version. | Oracle failure, path-only API, incompatible release/toolchain, or license failure. |
| H7 | SIMD attribution names the actual production operation. | A speedup is assigned to an unused helper or an algorithmic index gain is called a vector gain. |
| H8 | Fuzzy evidence cannot change exact resolver topology without a public semantic contract. | An absent exact name gains a target or call edge. |

## 2. Experimental controls

The fixture deterministically generates 100,000 names of the form
`scope::resolver_symbol_042042`. Every 257th bucket contains two `NodeId`-like
values, exercising duplicate candidate payloads. A fixed linear-congruential
sequence produces exact and absent queries. Each timed arm runs its oracle
before measurement, alternates arm order, uses an odd replicate count, and
reports the median.

| Campaign | Replicates | Work per replicate | Oracle |
|---|---:|---|---|
| ordered map versus DAT | 51 | build; eight complete exact passes; eight complete absent passes | equal rolling candidate checksum; all misses zero |
| prefix | 51 | 50 selective, 25 broad, and 25 absent prefixes | independent full scan, flattened/sorted candidate checksum |
| fuzzy | 51 | one distance-two, top-64 near-miss query | independent bounded standard-distance scan, sorted `(distance, term)` vector |
| DynamicDawg | 11 | paired build; eight exact passes; eight absent passes | same ordered-map checksum |
| direct distance SIMD | 51 | equalized dynamic-programming work at 32, 128, and 512 bytes | scalar/SIMD distance checksum equality |
| deep lifecycle | one per backend/depth | construct, exact lookup/traversal, destroy on a 256 KiB worker stack | exact value and completion marker |

Every potentially memory-intensive command ran under
`systemd-run --user --scope -p MemoryMax=4G -p MemorySwapMax=0`. This ceiling
makes a dependency failure local and observable. Build and evidence storage
remained under the disk-backed worktree `target`, never `/tmp`.

## 3. Resolver baseline and causal profile

The committed `resolve_calls` benchmark covers the standalone symbol table and
complete exact, unresolved, ambiguous, and depth-64 resolver paths at 256 and
4,096 symbols. Before timing it checks the complete report and graph topology.

| Baseline case | Median |
|---|---:|
| symbol table, 256 / 4,096 symbols | 182.71 us / 4.2454 ms |
| exact resolver, 256 / 4,096 symbols | 349.40 us / 8.2835 ms |
| unresolved resolver, 256 / 4,096 symbols | 330.75 us / 6.8791 ms |
| ambiguous resolver, 256 / 4,096 symbols | 388.98 us / 7.8379 ms |
| exact resolver, 1,024 symbols at depth 64 | 3.0133 ms |

Headless symbolized profiles use 32,768 symbols and four repetitions. The
complete exact workload measured 434 ms and 165,804 KiB RSS. Named samples
were distributed across `build_symbol_table` (15.56%), `resolve_calls`
(14.85%), candidate insertion (4.35%), `connect_unique` (3.93%), and
`connect_labeled_unique` (3.78%). Exact name lookup was not a standalone
hotspot. Unresolved and ambiguous controls reached 396.98 ms / 145,728 KiB and
541.50 ms / 218,304 KiB, respectively.

An exploratory fixture originally used `connect_unique` while manufacturing
the input graph, creating a quadratic uniqueness scan before the timed
resolver. Replacing fixture construction with lawful direct `connect` removed
that confound. Production topology emission still uses unique insertion.

## 4. Exact and prefix results

All current 4.0.0-release-candidate checksums matched.

| Operation | `BTreeMap` | DAT | Relative DAT result |
|---|---:|---:|---:|
| build | 3.506618 ms | 22.169225 ms | +530.6% |
| eight exact passes | 170.204030 ms | 80.322936 ms | -52.8% |
| eight absent passes | 15.511112 ms | 7.788819 ms | -49.8% |
| peak RSS | 42,164 KiB | 53,380 KiB | +26.6% |
| DHAT total allocation | 40,243,623 B | 73,080,165 B | +81.6% |
| DHAT peak live bytes | 29,926,304 B | 36,550,148 B | +22.1% |

One cold build plus one exact pass derives from the separately measured terms:
24.78 ms for the ordered control and 32.21 ms for DAT. DAT lookup amortizes its
build only after about 1.66 complete 100,000-query passes over one retained
index. libcpg has neither that usage shape nor an index invalidation epoch.

The prefix oracle checksum was `9307069498928516957` for all arms:

| Prefix algorithm | Median | Relative to full scan |
|---|---:|---:|
| ordered-map full scan | 39.921061 ms | control |
| ordered-map lower-bound range | 0.368700 ms | -99.08% |
| DAT prefix zipper | 3.206443 ms | -91.97% |

The DAT zipper is about 8.70 times slower than the exact ordered range already
available from `BTreeMap`. The range also emits lexical order without a
post-sort.

## 5. DynamicDawg result

The 100,000-symbol paired campaign completed within the 4 GiB scope at 78,312
KiB peak RSS and zero swap. All checksums equaled the ordered oracle.

| Operation | `BTreeMap` median | DynamicDawg median | Relative result |
|---|---:|---:|---:|
| build | 3.396718 ms | 9.314642 ms | 2.74 times slower |
| eight exact passes | 171.482943 ms | 331.429943 ms | 1.93 times slower |
| eight absent passes | 14.668481 ms | 31.471017 ms | 2.15 times slower |

The candidate loses all resolver-relevant operations; no amortization model can
make it preferable for this fixture.

## 6. Fuzzy result and semantic falsification

For query `scope::resolvor_symbol_042042`, maximum distance two, and top 64,
the independent bounded scan and ordered automaton returned the same complete
vector and checksum `1816965315520952722`.

| Algorithm | Median |
|---|---:|
| bounded scan over 100,000 terms | 32.965629 ms |
| ordered dictionary/automaton intersection | 37.441 us |

The roughly 880-fold gain validates the indexed automaton algorithm for an
authorized fuzzy workload. H8 nevertheless falsifies production adoption:
`ResolveConfig` contains no edit metric, distance/work/candidate completeness
contract, or advisory fuzzy result. Substituting this result for an absent
exact spelling would invent topology.

## 7. SIMD attribution

The direct scalar/SIMD experiment preserved every distance checksum.

| ASCII length | Scalar median | SIMD median | SIMD result |
|---:|---:|---:|---:|
| 32 | 3.256206 ms | 4.043419 ms | +24.14% |
| 128 | 2.604089 ms | 2.653744 ms | +1.84% |
| 512 | 2.488190 ms | 2.583711 ms | +3.87% |

These cases target the standard-distance dynamic-programming kernel. They do
not generalize to every operation in `liblevenshtein`. Source tracing shows
that production transducer state reduction invokes SIMD for four through eight
positions; `find_edge_label_simd` appears only in its definition, examples,
and tests. The ordered fuzzy gain is primarily dictionary/automaton
intersection, not the unused edge helper or the direct distance kernel.

## 8. Version and correctness audit

The current local siblings identify as `libdictenstein` and `liblevenshtein`
4.0.0-rc.1 and require Rust 1.95. The registry offered
`libdictenstein` 0.2.0 and `liblevenshtein` 0.9.1, so the evaluated current API
was unavailable as a version-only dependency. Both current crates use the
Apache-2.0 license; license compatibility alone is insufficient for adoption.

The registry DAT failed the exact oracle: the ordered checksum was
`40556706704`, while the published DAT returned `135270024` in every
replicate. Current source is exact. Upstream commit `86e658c` introduced
`StaticDATBuilder`, which accumulates an ordinary trie and assigns each known
sibling set a collision-free BASE. The published incremental builder relocated
children after conflicts while preserving only local array state, so later
collisions could disconnect descendant/value addressing. libcpg cannot depend
on the incorrect release or a local unreleased path.

## 9. Native-stack and heap boundary

DAT and DynamicDawg each passed a 4,096-segment construction, lookup, and drop
lifecycle on a 256 KiB worker stack at approximately 23 MiB RSS. Source audit
confirms iterative builders and traversal, plus an explicit `Vec` in the
DynamicDawg `Drop` implementation.

The transducer also retained constant native call depth, but its generated
transition cache has a quadratic heap shape. Let $`q`$ be query units,
$`s=2^{\lceil\log_2(q+1)\rceil}`$ the dense row stride, $`g`$ the number of
generated positional states, and $`z`$ the bytes per target. Its storage is

```math
M_{\mathrm{dense}} = gsz.
```

Along an exact path, $`g=O(q)`$ and $`s=O(q)`$, hence
$`M_{\mathrm{dense}}=O(q^2)`$. The measured geometric campaign confirms the
model:

| Repeated `nested::` segments | Approximate query bytes | Peak RSS | Result |
|---:|---:|---:|---|
| 64 | 527 | 23,048 KiB | pass |
| 256 | 2,063 | 70,592 KiB | pass |
| 512 | 4,111 | 267,752 KiB | pass |
| 1,024 | 8,207 | 1,055,492 KiB | pass |
| 2,048 | 16,399 | at least 4 GiB | killed by cgroup ceiling |

![The measured geometric campaign distinguishes constant native stack from quadratic transducer heap retention.](../diagrams/resolver-index-resource-boundary.svg)

The failure is not described as a stack overflow. It is a precise resource
contract failure caused by `DenseGeneratedTargets::push_row`, which eagerly
appends a power-of-two-width row for each generated state.

## 10. Conclusion

H1--H3 establish current-candidate correctness; H4 rejects DAT and
DynamicDawg for the actual resolver lifecycle; H5 rejects fuzzy transducer
admission for untrusted long names; H6 rejects the available registry boundary;
H7 keeps SIMD claims operation-specific; and H8 preserves exact topology.

The production outcome is therefore the unchanged ordered exact index, plus
committed oracle-guarded resolver benchmarks and causal headless profiles. No
dependency, feature, unsafe block, fuzzy edge, prototype, or path reference
remains. ADR-0064 records the decision and Security 22 records its trust and
availability boundary.

## References

1. Aoe, J. (1989). *An Efficient Digital Search Algorithm by Using a
   Double-Array Structure.* IEEE Transactions on Software Engineering 15(9),
   1066--1077. DOI:
   [10.1109/32.31365](https://doi.org/10.1109/32.31365).
2. Schulz, K. U., Mihov, S. (2002). *Fast String Correction with Levenshtein
   Automata.* International Journal on Document Analysis and Recognition 5,
   67--85. DOI:
   [10.1007/s10032-002-0082-8](https://doi.org/10.1007/s10032-002-0082-8).
