# Condensation-analytics validation

This scientific ledger records the falsifiable claims and independent
experiments for `condensation_metrics` and `coffman_graham`. The subject is a
strongly connected component (SCC) condensation directed acyclic graph (DAG),
not the parser or SCC algorithm itself. SCC partition correctness has its own
petgraph differential oracle; this campaign decides the new metrics and
schedule built on that partition.

## 1. Hypotheses

| ID | Falsifiable hypothesis | Deciding evidence |
|---|---|---|
| H1 | Source-oriented levels, height, width, and SCC-size-weighted critical-path weight equal exhaustive small-DAG results. | fixed weighted branch; 128 generated DAGs; exhaustive path enumerator |
| H2 | A CG result contains every component once, never exceeds $`w`$, and sends every precedence edge to a strictly lower layer. | fixed width-two DAG; generated invariant checks |
| H3 | Width one is serial-optimal, width two is optimal, and widths three/four satisfy the $`2-2/w`$ height bound. | exhaustive subset-schedule dynamic program on every generated DAG |
| H4 | Reversing edge storage, adding duplicates, or changing representative order without changing component ids changes no metric or layer. | two-form equality in every generated case; fixed representative-order case |
| H5 | Empty, capped, width-zero, cyclic, self-edge, out-of-domain, empty-component, and inconsistent membership/id inputs terminate without panic or false completeness. | fixed boundary/malformed examples; ten-shape CPG corruption campaign |
| H6 | The root API is feature-free and composes with `call_graph_sccs`; a real Rust tree-sitter/resolution/SCC pipeline produces the expected levels. | default and `lang-rust` public integration tests |
| H7 | `serde` preserves every option, metric, and CG layer field exactly. | JSON round-trip example under `serde` |
| H8 | Every Criterion fixture proves exact height, width, critical path, CG coverage, and precedence before timing. | eight benchmark smoke cases |
| H9 | Neither entry point changes CPG node or edge counts. | both public integration pipelines |

No hypothesis is inferred from another. For example, satisfying the width cap
does not decide lexicographic determinism, path weight, approximation height,
malformed-input behavior, or read-only operation.

## 2. Independent weighted-path oracle

Production uses canonical Kahn order and a one-pass dynamic program. The test
oracle instead enumerates every path from every component on generated DAGs.
For path $`p=(v_0,\ldots,v_k)`$, it computes

```math
weight(p)=\sum_{i=0}^{k}|C_{v_i}|.
```

The maximum enumerated weight must equal `critical_path_weight`. The reported
path is then checked separately:

1. every consecutive pair occurs in the condensation edge set;
2. every component id is in range; and
3. the independently summed SCC sizes equal the report.

This separation catches a report that has the right scalar by accident but an
invalid witness. Generated component sizes range from one through four, so the
oracle distinguishes edge count from SCC-member weight.

## 3. Independent optimal-schedule oracle

For $`n\le8`$ components, a bit mask represents completed unit tasks. Given
state $`S`$, a component is ready exactly when it is absent from $`S`$ and all
its predecessor bits are present. The oracle enumerates every non-empty subset
of ready tasks of cardinality at most $`w`$ and computes the minimum layer count
to the all-complete state.

```text
OPTIMAL_HEIGHT(dag, w):
    distance[empty] := 0
    every other distance := infinity

    for each reachable completed-set S:
        ready := unscheduled vertices whose predecessors are subsets of S
        for every non-empty batch B contained in ready with |B| <= w:
            distance[S union B] := min(distance[S union B], distance[S] + 1)

    return distance[all vertices]
```

This state-space dynamic program does not use CG labels, reverse placement, or
the production available-layer set. It therefore decides schedule height
independently.

For $`w=1`$, the test requires equality with the serial optimum. For $`w\ge2`$,
the floating-point-free form of the Coffman–Graham bound is

```math
wH_{CG}\le(2w-2)H_{opt}.
```

At $`w=2`$, both sides reduce to $`2H`$, so the same assertion proves exact
optimality. The fixed eight-component example additionally asserts the exact
width-two optimum.

## 4. Generated DAG campaign

`metrics_and_layering_match_small_graph_oracles` runs 128 cases. A case draws:

- component count $`n\in[1,8]`$;
- arbitrary candidate pairs in $`[0,8]^2`$, retaining only in-domain pairs
  with source id less than target id;
- SCC member counts in $`[1,4]`$; and
- width $`w\in[1,4]`$.

The source-less-than-target filter generates a broad DAG family without using
the production acyclicity check. Edges are sorted/deduplicated for the oracle
form. A second form reverses that storage and appends all original edges again.
The property requires exact `CondensationMetrics` equality and exact
`CgLayering` equality between forms.

Within every case, the test also checks:

- exhaustive maximum path weight;
- witness edges and witness weight;
- strict source-level increase on every edge;
- one occurrence of every component across CG layers;
- sorted inner layer vectors;
- `layer_of` agreement with `layers`;
- occupancy at most `width_limit`;
- strict downward CG precedence; and
- the exact/approximate height equations from Section 3.

The property completed without a shrunk counterexample. If a future regression
shrinks, its minimal input belongs in the committed proptest regression file
before the defect is considered fixed.

## 5. Fixed example matrix

| Experiment | Discriminating condition |
|---|---|
| `empty_decomposition_has_zero_metrics_and_empty_layering` | Empty input returns complete zero metrics and a valid empty positive-width CG schedule. |
| `weighted_critical_path_and_longest_levels_are_exact` | Component weights $`(2,1,3,1,2)`$ choose a weight-eight branch rather than the equal-edge-count lighter branch; reversed representative order proves projection-dense ids do not replace component ids. |
| `coffman_graham_is_width_bounded_and_strictly_downward` | An eight-component DAG checks all layer invariants and exact width-two optimal height. |
| `caps_and_invalid_inputs_are_reported_without_panics` | Caller/hard caps, a clamped oversized option, zero optional/direct width, a cycle, self-edge, empty component, mismatched id/membership, extra membership, and invalid endpoint select bounded/error paths. |
| `reports_round_trip_through_json` | Options and a report with optional CG layers survive exact serde JSON round trips. |

These cases exercise every public structural validation class directly rather
than inferring robustness from SCC-engine-produced decompositions.

## 6. Public integration experiments

`tests/condensation_integration.rs` imports only crate-root API.

The feature-free test builds

```text
first <-> second -> middle -> {left_sink, right_sink}
```

through public CPG operations. It requires the recursive pair to become one
weight-two SCC, levels `(0, 1, 2, 2)`, critical path `(0, 1, 2)` with weight
four, and CG layers `({2,3}, {1}, {0})` in sink-first order. CPG node and edge
counts must remain unchanged.

The `lang-rust` test parses four real Rust functions, enables the existing
evidence-bounded call resolver, computes call-graph SCCs, and then runs the new
metrics. It identifies components through `component_by_node`, not assumed
source ids, and requires root level zero, both middle calls at level one, and
the shared leaf at level two. This is end-to-end parser integration without
putting parsing logic in the analytics module.

## 7. Malformed input and caps

The normal SCC constructors prove a DAG, non-empty components, and canonical
ids. The new boundary repeats the checks because public fields and serde can
introduce external values.

`condensation_metrics` cannot return an error by contract, so failure is
observable through `complete` and `capped`. It never returns partial path or
layer vectors that could be mistaken for a whole-graph result. Direct
`coffman_graham` distinguishes invalid input from `BudgetExceeded`.

The hard component limit is $`10^6`$. The cap test uses the caller-selectable
lower bound to decide the same branch with three components, avoiding a
million-allocation unit fixture. The public constant is asserted separately to
prevent silent policy drift.

## 8. Benchmark experiment

`benches/condensation.rs` is a `harness = false` Criterion target. It uses two
deterministic families at 1,024 and 8,192 components:

- a single path, whose height and critical weight equal component count and
  whose width is one; and
- eight independent equal-length chains, whose height is component count
  divided by eight and whose width is eight.

Each family runs once for core metrics and once with `w = 8` CG layering, for
eight smoke cases. Before any timed closure, the harness checks complete/cap
flags, exact height/width/path length/path weight, optional-result presence,
CG height/width, per-layer capacity, and strict downward precedence.

The timed closure receives an already-built `SccDecomposition`; it measures
only the public analytics call. `black_box` covers the decomposition, options,
and returned report.

## 9. Coverage result

Clean stable and nightly maximal-matrix runs measure
`analysis/condensation.rs` at **97.29 % line**, **98.11 % region**, **97.37 %
function**, and **92.50 % branch coverage**. The module therefore clears the
epic thresholds of 90 % lines and 80 % branches. These figures include the
full feature matrix, both public integrations, the whole-CPG corruption
campaign, and the generated/exhaustive unit oracles; they are not extrapolated
from a focused run.

## 10. Reproduction

```text
# Feature-free unit/property and public integration evidence:
cargo test --no-default-features analysis::condensation::tests
cargo test --no-default-features --test condensation_integration

# Serialization and real-parser pipelines:
cargo test --no-default-features --features serde \
  analysis::condensation::tests
cargo test --no-default-features --features lang-rust \
  --test condensation_integration

# Oracle-guarded benchmark smoke:
cargo bench --no-default-features --bench condensation -- --test

# Repository-wide feature, lint, rustdoc, link, diagram, and coverage gates:
./scripts/verify.sh
cargo cov-text
cargo +nightly cov-branch
```

Validation commands are captured once to retained logs for diagnosis and the
temporary logs are removed after the stable commit.

## 11. Interpretation boundary

The experiments establish deterministic graph metrics and unit-task schedule
properties for the supplied condensation. They do not establish parser
correctness, call-target completeness, real execution time, parallel safety,
equal task cost, or optimal scheduling when components have heterogeneous
durations. The weighted critical path uses SCC member count by explicit
contract; it is structural evidence, not a performance model.

## References

1. Kahn, A. B. (1962). *Topological sorting of large networks.*
   Communications of the ACM 5(11), 558. DOI:
   [10.1145/368996.369025](https://doi.org/10.1145/368996.369025)
2. Coffman, E. G., Jr., Graham, R. L. (1972). *Optimal scheduling for
   two-processor systems.* Acta Informatica 1, 200–213. DOI:
   [10.1007/BF00288685](https://doi.org/10.1007/BF00288685)
