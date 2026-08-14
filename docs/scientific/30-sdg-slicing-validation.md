# Scientific validation 30 — SDG summary edges and slicing

This page states falsifiable claims for `analysis::sdg`, names independent or
hand-computed oracles, and distinguishes graph-algorithm correctness from the
completeness of the input CPG.

## 1. Hypotheses

| Hypothesis | Discriminating experiment | Oracle |
|---|---|---|
| One returning parameter emits one summary edge | two-argument call, mask bit zero | exact labeled endpoint pair |
| Labels do not overwrite ordinary PDG evidence | unlabeled dependence already has the same endpoints | both labeled and unlabeled edges remain |
| Multi-target calls use may-union semantics | two callees select different arguments | exactly two summary edges |
| Construction is idempotent and append-only | run builder twice | second count zero; node count unchanged |
| Missing summaries invent no relation | resolved call with empty map | graph shape unchanged |
| Capped summaries remain conservative | zero mask with `capped=true` | every argument connected |
| Fixed-width overflow is visible in topology | 65 arguments and truncated mask | argument 64 connected |
| Backward slicing enters the intended callee | criterion at first call result | formal, body, and return included |
| Backward slicing rejects wrong context | two calls share one callee | second argument/call excluded |
| Forward slicing is the direction dual | criterion at first actual argument | callee included; second call excluded |
| Caps are total | zero nodes, zero depth, missing criterion | exact bounded/empty sets |
| Serialization preserves caps | JSON round trip under `serde` | structural equality |

## 2. Hand-computed two-caller oracle

The central fixture has actual arguments $`a_1`$ and $`a_2`$, one formal
$`p`$, callee body $`b`$, return value $`r`$, and calls $`c_1`$ and $`c_2`$:

```math
a_1\to p,\quad a_2\to p,\quad p\to b\to r,\quad
r\to c_1,\quad r\to c_2,
```

with summary edges $`a_1\to c_1`$ and $`a_2\to c_2`$. The exact backward
slice of $`c_1`$ under the two-pass rules is

```math
\{a_1,p,b,r,c_1\}.
```

It excludes $`a_2`$ and $`c_2`$. The forward slice of $`a_1`$ has the same
five-node set. These assertions discriminate the two-pass implementation from
ordinary unrestricted reachability, which would include the sibling context.

## 3. Property-based campaigns

Three 256-case properties generate typed dependence multigraphs:

1. reversing edge insertion order leaves both complete `BTreeSet` slices
   byte-for-byte equal and never exceeds `max_nodes`;
2. increasing `max_nodes` yields a superset, while increasing call depth with
   enough node capacity also yields a superset; and
3. on PDG-only generated graphs, the legacy `backward_slice` result is a subset
   of the interprocedural result when the graph fits under the cap.

The generator includes parallel and self edges across `DataDependence`,
`ControlDependence`, `Parameter`, and `ReturnValue`, so determinism is tested
against insertion order rather than only repeated calls on one adjacency
layout.

## 4. Public integration boundary

`tests/sdg_integration.rs` imports only crate-root exports. It constructs a
resolved identity function, computes its real `EffectSummary`, builds the
callee PDG, materializes one summary edge, proves second-run idempotency, and
runs both public interprocedural slice directions. This checks the intended
cross-module pipeline:

```text
call SCCs -> effect_summaries -> PdgBuilder
          -> build_summary_edges -> backward/forward SDG slice
```

## 5. Coverage matrix

| Dimension | Evidence |
|---|---|
| normal | selective summary and both slice directions |
| negative | missing summary and sibling call context |
| boundary | zero caps, unknown node, capped summary, 65 arguments |
| mutation | node preservation, append-only edges, labeled coexistence |
| idempotency | exact second-run zero additions |
| dynamic call | may-union across two resolved callees |
| deterministic | reverse-insertion generated equality |
| monotonic | node and depth cap properties |
| regression | existing intraprocedural slice subset property |
| integration | effect/PDG/summary/slice crate-root pipeline |
| serialization | `SliceCaps` round trip |

## 6. Complexity and resource interpretation

The tests assert cardinality bounds, but do not treat a bounded result as a
complete slice. `max_nodes` selects a stable traversal prefix.
`max_call_depth` limits boundary crossings on any retained path. Summary-edge
construction is proportional to represented resolved-call arguments plus
deduplication; slicing is proportional to reached nodes and inspected admitted
edges, with deterministic frontier sorting.

## 7. Reproduction

```sh
cargo test --no-default-features --lib sdg
cargo test --no-default-features --test sdg_integration
cargo test --no-default-features --features serde --lib sdg
cargo clippy --no-default-features --all-targets -- -D warnings
```

## References

1. Horwitz, S., Reps, T., Binkley, D. (1990). *Interprocedural Slicing Using
   Dependence Graphs.* DOI:
   [10.1145/77606.77608](https://doi.org/10.1145/77606.77608).
2. Ferrante, J., Ottenstein, K. J., Warren, J. D. (1987). *The Program
   Dependence Graph and Its Use in Optimization.* DOI:
   [10.1145/24039.24041](https://doi.org/10.1145/24039.24041).
