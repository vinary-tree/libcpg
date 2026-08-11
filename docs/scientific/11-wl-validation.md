# Scientific validation 11 — WL labels and structural clone classes

This record treats `wl_labels` and `structural_clone_classes` as falsifiable
algorithms. It separates exact examples, fixed hash vectors, a generated
isomorphism experiment, malformed-input behavior, public integration, and
benchmark oracles. A passing experiment supports only the claim its assertions
can distinguish.

## 1. Hypotheses

| ID | Falsifiable claim | Deciding evidence |
|---|---|---|
| H1 | Two directed triangles receive one common final label and one six-member clone class. | Exact two-triangle fixture after two rounds. |
| H2 | An outward-star center differs from its three leaves, while all leaves remain one ordered clone class. | Exact star fixture after one round. |
| H3 | Any isomorphic stable-identifier relabeling preserves the final label multiset and logical clone partition. | 256 generated graphs, independent relabeling, and logical-id normalization. |
| H4 | Reversing edge enumeration and adding duplicate copies before canonical projection construction cannot alter H3. | The second form in every generated case. |
| H5 | Requested rounds above 16 are observationally identical to exactly 16 rounds. | Boundary examples using 17 and `usize::MAX`. |
| H6 | The stable mixer matches the fixed port constants and wrapping sequence. | Three exact hexadecimal input/output vectors. |
| H7 | Direction affects the seed, weights do not affect labels, and out-of-universe endpoints are excluded. | Source/sink, weighted-path, and external-endpoint examples. |
| H8 | Malformed public projection state cannot panic or yield an aligned false-valid label vector. | Unit corruption and ten-family public robustness campaign. |
| H9 | Clone members/classes obey the documented threshold and total ordering. | Mixed 3/2/2-member fixture with thresholds 0, 3, and 9. |
| H10 | Crate-root APIs compose from a public CPG through call projection without mutation or optional features. | Feature-free six-function integration test. |
| H11 | Benchmark timing cannot silently measure an invalid or wrongly shaped result. | Exact ring/star label and class oracles executed before every timed group. |

These hypotheses are complementary. Isomorphism invariance does not establish
that non-isomorphic graphs differ; a correct partition does not establish the
specified numeric mixer; and termination on malformed input does not establish
valid-input semantics.

## 2. Generated isomorphism experiment

The property campaign runs 256 cases. Each case draws:

- zero through nine logical vertices;
- up to 79 candidate directed pairs, retaining only in-range endpoints;
- a requested round count in $`[0,23]`$;
- a clone threshold in $`[0,11]`; and
- one random ordering key per logical vertex.

The first projection uses `NodeId(logical)`. The second sorts logical vertices
by `(ordering_key, logical)`, assigns sparse identifiers `100 + 3 * rank`,
reverses edge enumeration, and supplies every edge twice. The constructor
normalizes duplicates. This mapping is a known bijection, so the test compares
label multisets directly and maps each returned stable identifier back to its
logical vertex before comparing clone partitions.

The property also repeats `wl_labels` on the original projection and requires
byte-identical output. A counterexample can therefore expose relabeling,
storage-order, duplicate-edge, threshold, iteration-clamp, or repeated-run
nondeterminism.

The test is not an oracle for distinguishing non-isomorphic graphs. WL's known
incompleteness makes such a claim false; the experiment correctly tests the
necessary invariance direction only.

## 3. Exact fixtures and boundaries

| Experiment | Defect it can expose |
|---|---|
| Two disjoint directed triangles | inconsistent equal-neighborhood refinement or unstable grouping. |
| Outward star | lost directed-degree seed or failure to retain leaf symmetry. |
| Source to sink at zero rounds | accidental direction erasure before refinement. |
| Weighted path plus external endpoints | weight leakage or failure of projection-universe closure. |
| Empty and malformed CSR | unchecked indexing or ambiguous partial output. |
| Exactly/above 16 rounds | wrong clamp boundary or arithmetic overflow on a hostile request. |
| Classes of sizes 3, 2, and 2 | incorrect minimum floor, member ordering, size ordering, or stable-id tie break. |
| Three mixer vectors | constant, operation-order, or wrapping-semantics drift. |

The triangle and star topologies reproduce the pgmcp source fixtures. libcpg
adds exact output ordering and the public-boundary cases needed by its broader
projection contract.

## 4. Public integration experiment

`tests/wl_integration.rs` imports only crate-root items and runs with
`--no-default-features`. It builds six public function nodes and two directed
call triangles, then projects the call graph. It requires:

1. a well-formed six-node projection;
2. one identical final label across all six vertices;
3. one clone class containing exactly the six stable function identifiers;
4. an empty result when the requested class size is seven; and
5. unchanged CPG node and edge counts after every analysis.

This experiment exercises public construction, edge typing, projection,
validation, root exports, WL refinement, stable-id conversion, and
non-mutation. It does not involve parsing because WL consumes topology, not
source syntax.

## 5. Malformed-input campaign

`GraphProjection` fields are public for serialization and external adapters.
The focused unit test corrupts offsets so that the forward CSR is invalid and
requires both public functions to return empty vectors. The shared validator
also rejects unsorted/duplicate nodes, wrong offset lengths or endpoints,
descending offsets, out-of-range or unsorted targets, mismatched weights, and
forward/reverse disagreement.

`tests/robustness.rs` injects ten malformed CPG families and builds a call
projection from each. WL labels must either be empty or align exactly with the
projection; every clone member must map to an in-domain node, members must be
strictly ordered, and every returned class must contain at least two members.
The campaign establishes safe integration behavior, not meaningful clone
semantics for corrupt program graphs.

## 6. Benchmark experiment

`benches/wl.rs` supplies two Criterion groups:

| Group | Shapes and sizes | Pre-timing oracle |
|---|---|---|
| `wl_labels` | directed ring and outward star at 1,024 and 8,192 vertices; four rounds | exact vector length; one ring label; distinct center/leaf star labels and one common leaf label |
| `wl_clone_classes` | the same four inputs | exact full-ring class; exact sorted leaf-only star class |

The graph and expected results are constructed before `bench_function`; the
timed closure measures only the selected production operation. A smoke run
uses Criterion's `--test` mode to execute every oracle and operation once.

This benchmark establishes a reproducible measurement instrument. It does not
by itself establish a complexity class, a latency service-level objective, or
representativeness of a particular application corpus.

## 7. Coverage result

A clean maximal-matrix `cargo cov-text` run measured `analysis/wl.rs` at
**100.00% line**, **100.00% region**, and **100.00% function** coverage. A
clean nightly `cargo +nightly cov-branch` run measured **100.00% branch**
coverage. Both runs executed all **679 library tests** and the **45 public
integration and robustness tests**, including the WL public integration case;
these are not focused-test extrapolations. The module therefore clears the
epic's 90% line and 80% branch thresholds.

## 8. Reproduction

```text
# Fixed examples, exact vectors, and 256 generated cases:
cargo test --no-default-features analysis::wl::tests --lib

# Crate-root integration and malformed public-graph campaign:
cargo test --no-default-features --test wl_integration
cargo test --no-default-features --test robustness

# Oracle-guarded benchmark smoke and timed measurement:
cargo bench --bench wl --no-default-features -- --test
cargo bench --bench wl --no-default-features

# Whole repository and maximal coverage gates:
./scripts/verify.sh
cargo cov-text
cargo +nightly cov-branch
```

Validation output is captured to diagnostic logs and those temporary files are
removed after the stable commit.

## 9. Interpretation boundary

The evidence establishes deterministic bounded refinement for a supplied
valid projection, ordering/threshold behavior, safe malformed-input handling,
and public feature-free composition. It does not establish graph-isomorphism
completeness, absence of 64-bit collisions, source or behavioral clone
equivalence, parser completeness, or correctness of application-specific edge
semantics.

## References

- Shervashidze, N., Schweitzer, P., van Leeuwen, E. J., Mehlhorn, K., and
  Borgwardt, K. M. (2011). *Weisfeiler-Lehman Graph Kernels.* Journal of
  Machine Learning Research 12(77), 2539–2561.
  [JMLR article and PDF](https://jmlr.org/papers/v12/shervashidze11a.html).
