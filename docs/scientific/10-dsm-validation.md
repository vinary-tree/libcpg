# Scientific validation 10 — DSM visibility and core–periphery

This record treats [`analyze_dsm`](../components/graph/dsm-analysis.md) and
`classify_core_periphery` as falsifiable algorithms. It separates fixed
regression examples, an independent transitive-closure oracle, generated
storage permutations, malformed-input experiments, public integration, and
serialization. A passing example is evidence only for the hypothesis it
actually discriminates.

The research interpretation follows MacCormack, Rusnak, and Baldwin's DSM
dependency metrics [[1]](#references) and Baldwin, MacCormack, and Rusnak's
directed-network/cyclic-core methodology [[2]](#references). The median
quadrant boundary is libcpg's explicit port policy and is tested as such; this
record does not attribute that exact threshold policy to either paper.

## 1. Hypotheses

| ID | Falsifiable claim | Deciding evidence |
|---|---|---|
| H1 | For every valid in-budget projection, VFO and VFI equal the row and column sums of the non-reflexive transitive closure. | 256 generated graphs versus independent Floyd–Warshall closure. |
| H2 | Propagation cost equals reachable ordered pairs divided by $`n^2`$, with zero for an empty graph and no diagonal contribution from cycles or self-loops. | generated scalar oracle plus chain, empty, cycle, and self-loop examples. |
| H3 | The total VFO count equals the total VFI count. | assertion in every generated case. |
| H4 | Median thresholds and inclusive comparisons assign exactly one documented quadrant to every complete-result vertex. | hub, all-symmetric cycle, two-vertex path, and generated deterministic comparisons. |
| H5 | `cyclic_core` is the largest SCC with at least two members; equal sizes choose the smallest stable representative; singleton self-loops are excluded. | independent mutual-reachability partition, fixed cycle/tie/self-loop examples. |
| H6 | Reversing edge enumeration and adding duplicate copies cannot change any DSM or classification field. | exact two-form equality in every generated case. |
| H7 | An out-of-universe edge, malformed CSR, mismatched reverse CSR, mismatched result vector, impossible count, non-finite/inconsistent scalar, or different same-sized node universe cannot cause traversal, panic, or false completeness. | fixed boundary examples and public robustness harness. |
| H8 | An input above 50,000 vertices returns no partial derived vectors and sets `complete = false`, `capped = true`. | public-constant oversized projection and lower-cap decision-path example. |
| H9 | Crate-root APIs compose from CPG to call projection to DSM to shared SCC without mutating the CPG. | feature-free hand-built public integration and parser-backed Rust integration. |
| H10 | serde preserves node provenance, visibility vectors, cost, thresholds, classes, cyclic core, and status exactly. | JSON round-trip example under `serde`. |

These claims are independent. A correct propagation-cost scalar could conceal
swapped per-node vectors; a correct SCC size could conceal the wrong tie
component; and a terminating malformed-input case could still falsely mark a
partial report complete. Each dimension therefore has its own assertion.

## 2. Independent closure oracle

Production performs one breadth-first search (BFS) per source over canonical
compressed-sparse-row (CSR) adjacency. The property oracle instead constructs
a dense boolean matrix and applies the Floyd–Warshall recurrence:

```math
R^{(k)}_{ij}=R^{(k-1)}_{ij}\lor
\left(R^{(k-1)}_{ik}\land R^{(k-1)}_{kj}\right).
```

After every intermediate vertex has been admitted, the oracle clears the
diagonal. It computes row sums, column sums, and their normalized total. Because
the oracle uses a different representation and dynamic-programming schedule,
a defect in the BFS queue or visited reset does not automatically reproduce in
the expected value.

The independent cyclic-core oracle uses the same completed closure, not
production Tarjan. For the first unassigned vertex $`u`$, it forms

```math
C(u)=\{u\}\cup\{v\ne u\mid R_{uv}\land R_{vu}\}.
```

It marks that equivalence class assigned, retains classes of cardinality at
least two, and keeps the first class only when a later class is strictly larger.
Generated `NodeId` values equal dense order, so this first-on-tie rule is the
smallest-stable-representative contract.

## 3. Generated experiment

`dsm_matches_transitive_closure_and_is_storage_order_invariant` runs 256 cases.
A case draws:

- $`n\in[0,9]`$ vertices;
- zero through 79 candidate directed endpoint pairs in $`[0,9]^2`$; and
- only pairs whose two endpoints are below $`n`$.

The first production projection receives the retained sequence. A second
projection receives all retained edges, a reversed copy of them, and reversed
enumeration. `GraphProjection::from_edges` canonicalizes the duplicate
multiset, but its optional weights can differ after merging; DSM deliberately
ignores weights. The property requires exact equality of `DsmAnalysis` and
`CorePeripheryResult` across forms.

For each form, the experiment independently requires:

1. exact VFO vector equality with closure row sums;
2. exact VFI vector equality with closure column sums;
3. equal VFO and VFI totals;
4. propagation cost within one machine epsilon of the independently normalized
   integer pair count;
5. exact largest multi-vertex mutual-reachability class; and
6. complete deterministic classification equality.

The generated state space includes empty graphs, disconnected vertices,
self-loops, duplicate edges, directed acyclic graphs, multiple SCCs, and fully
mutually reachable regions. A future shrunk counterexample belongs in the
committed proptest regression ledger before a discovered defect is considered
closed.

## 4. Ported and boundary fixture matrix

The four pgmcp source fixtures are ported with the same topology and numeric
oracles:

| Experiment | Expected result |
|---|---|
| `linear_chain_propagation_and_visibility` | `0 -> 1 -> 2 -> 3` gives VFO `[3,2,1,0]`, VFI `[0,1,2,3]`, and cost `0.375`. |
| `disconnected_graph_has_zero_propagation` | Three isolates give zero vectors and zero cost. |
| `fully_connected_cycle_has_max_propagation` | The directed 3-cycle gives both vectors `[2,2,2]`, cost $`6/9`$, all median classes `Core`, and cyclic core `[0,1,2]`. |
| `classifies_shared_and_control_roles` | `0,1 -> 2 -> 3,4` makes vertex 2 `Core`, vertices 3 and 4 `Shared`, and reports no cyclic group. |

Additional examples discriminate contracts the source fixture did not cover:

| Experiment | Defect it can expose |
|---|---|
| Empty graph and singleton self-loop | divide-by-zero, false self-visibility, or treating one self-loop as a multi-component cyclic core. |
| Equal two-cycle groups over sparse stable IDs | unstable SCC tie selection or confusion between stable and dense identifiers. |
| Out-of-universe endpoints | traversal of an edge that `GraphProjection` must discard. |
| Public cap and lower-cap equality/overflow cases | wrong `>`/`>=` boundary or partial-vector leakage. |
| Malformed offsets and reverse targets | unchecked public CSR indexing or forward/reverse inconsistency. |
| Truncated VFO, impossible/imbalanced counts, non-finite scalar, and different same-sized node universe | overflow, scalar/count inconsistency, length-only alignment, or cross-graph result composition. |
| Stable labels and lookup helpers | drift in serialized text labels, `class_of`, or stable cyclic-core mapping. |
| serde round trip | omitted provenance, enum, floating-point, vector, or status field. |

## 5. Public integration experiments

`tests/dsm_integration.rs` imports only the crate-root API.

The feature-free experiment builds five function nodes with public CPG
operations and call edges

```text
left_control, right_control -> hub -> left_shared, right_shared
```

It then runs `call_graph_projection`, `analyze_dsm`,
`classify_core_periphery`, and `projection_sccs`. It requires vectors
`[3,3,2,0,0]` and `[0,0,2,3,3]`, cost `0.32`, the exact five quadrant labels,
an empty cyclic core, acyclic SCC output with `CallGraph` provenance, and
unchanged CPG node/edge counts.

The `lang-rust` experiment parses

```rust
fn left() { right(); }
fn right() { left(); }
fn root() { left(); }
```

with the real tree-sitter Rust frontend and evidence-bounded call resolution.
It maps named functions to projection-dense identifiers, requires propagation
cost $`4/9`$, requires `{left,right}` as the cyclic core, and requires `left`
and `right` to be median `Core` while `root` is `Control`. It also proves the
analysis leaves the parser-built CPG unchanged.

This is end-to-end parser integration, but parsing logic remains outside DSM:
the analytics receive only the resulting call `GraphProjection`.

## 6. Malformed-input campaign

The unit boundary validates public projection nodes, CSR offsets/targets,
weight alignment, reverse agreement, result completion, exact stable node
universe, vector lengths, per-node count bounds, fan-in/fan-out total equality,
and finite scalar/count consistency. Every invalid form yields empty derived
vectors with `complete == false`; only a node-cap breach sets `capped`.

The repository-wide `tests/robustness.rs` campaign additionally injects ten CPG
corruption families: dangling parents, children, call targets, and definitions;
self and two-node parent cycles; an AST-edge cycle; a bodyless function;
inconsistent child bookkeeping; and an edge without pointers. Each case builds
the call projection and requires shared SCC, DSM, and classification to
terminate with in-domain results. This checks integration robustness rather
than merely replaying a hand-built valid projection.

## 7. Feature and serialization boundaries

All implementation, fixed examples, the property campaign, malformed-CPG
integration, and the hand-built public integration compile with
`--no-default-features`. The parser experiment alone requires `lang-rust`.
Serialization derives and the JSON round trip alone require `serde`.

`benches/dsm.rs` measures visibility and classification separately over paths
and rings at 256 and 1,024 vertices. Before Criterion enters either timed loop,
the fixture checks exact VFO/VFI patterns, propagation cost, median classes,
completion flags, and cyclic-core membership. A smoke run is therefore a
correctness check as well as a harness check. Timing remains evidence for only
those named shapes and sizes; it is not a latency guarantee at the hard cap.

## 8. Coverage result

A clean maximal-matrix `cargo cov-text` run measured `analysis/dsm.rs` at
**99.77% line**, **99.42% region**, and **97.62% function** coverage. A clean
nightly `cargo +nightly cov-branch` run measured **90.38% branch** coverage.
Both runs executed all **679 library tests** and the **45 public integration and
robustness tests**, including both DSM integration cases; these are not
focused-test extrapolations. The module therefore clears the epic's 90% line
and 80% branch thresholds.

## 9. Reproduction

```text
# Feature-free examples, malformed inputs, and 256 generated cases:
cargo test --no-default-features analysis::dsm::tests

# Public feature-free and real-parser pipelines:
cargo test --no-default-features --test dsm_integration
cargo test --no-default-features --features lang-rust --test dsm_integration

# Serialization and shared SCC regressions:
cargo test --no-default-features --features serde \
  analysis::dsm::tests::results_round_trip_through_json
cargo test --no-default-features analysis::scc::tests

# Whole malformed-CPG campaign and repository gates:
cargo test --no-default-features --test robustness
./scripts/verify.sh

# Oracle-guarded benchmark smoke and measurement:
cargo bench --bench dsm --no-default-features -- --test
cargo bench --bench dsm --no-default-features

# Coverage (stable lines/regions/functions; nightly branches):
cargo cov-text
cargo +nightly cov-branch
```

Validation commands are captured to diagnostic logs. Temporary logs are
removed after the stable commit.

## 10. Interpretation boundary

These experiments establish exact closure counts and cyclic groups for the
supplied directed projection, deterministic ordering, bounded failure states,
and read-only integration. They do not establish parser completeness,
call-resolution completeness, causal code-change propagation, modularity
quality, runtime scalability at the hard cap, or the organizational meaning of
a quadrant label. Those claims require separate evidence and compatible graph
semantics.

## References

1. MacCormack, A., Rusnak, J., Baldwin, C. Y. (2006). *Exploring the Structure
   of Complex Software Designs: An Empirical Study of Open Source and
   Proprietary Code.* Management Science 52(7), 1015–1030. DOI:
   [10.1287/mnsc.1060.0552](https://doi.org/10.1287/mnsc.1060.0552)
2. Baldwin, C. Y., MacCormack, A., Rusnak, J. (2014). *Hidden Structure: Using
   Network Methods to Map System Architecture.* Research Policy 43(8),
   1381–1397. DOI:
   [10.1016/j.respol.2014.05.004](https://doi.org/10.1016/j.respol.2014.05.004)
