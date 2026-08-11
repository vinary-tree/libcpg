# Loop-induction analysis validation

This page records the falsifiable claims and executable experiments behind
`loop_induction`. The implementation reports loop invariants, induction
variables (IVs), and counted-trip evidence from existing semantic Abstract
Syntax Tree (AST), Control Flow Graph (CFG), and `DefUse` Data Flow Graph (DFG)
relations. The validation therefore tests both positive recognition and the
conservative boundary where evidence must remain absent or unknown.

## 1. Hypotheses

| ID | Falsifiable hypothesis | Deciding evidence |
|---|---|---|
| H1 | Every invariant use has a non-empty `DefUse` predecessor set, and every predecessor is outside the loop region or already invariant. | `assert_analysis_contracts`; parser acceptance oracle |
| H2 | Every reported IV update belongs to its own loop; explicit basic updates are unique in-region definitions that dominate every latch. | generated fixture contract; duplicate and two-latch counterexamples |
| H3 | Exact steps and directions agree with checked integer arithmetic; overflow and symbolic coefficients do not become invented constants. | `checked_direction_and_comparison_oracles_match_integer_semantics`; overflow examples |
| H4 | One-level affine derived steps equal coefficient times basic step when that product is representable. | 192-case generated coefficient/step campaign |
| H5 | Counted evidence uses only invariant bounds and direction-compatible comparisons; stationary, unknown, incompatible, and irreducible cases remain `Unknown`. | comparison oracle; subtraction/irreducible examples |
| H6 | `hoistable` is a subset of invariant nodes and rejects duplicate definitions or non-dominated observations/uses. | hoist fixtures and shared contract assertions |
| H7 | Equivalent CPGs built in opposite node/edge insertion orders produce identical forests and induction reports. | 192-case insertion-permutation property |
| H8 | The analysis is read-only, rejects foreign forest references, terminates on cyclic malformed ASTs, and is native-stack safe on deep regions. | public integration, malformed harness, 25,000-node stress test |
| H9 | A real Rust `for i in 0..x` build exposes semantic range/binder nodes and reports `i`, step one, a body-contained update, and counted evidence. | `parser_generated_rust_range_loop_satisfies_the_acceptance_oracle` |

Each claim can fail independently. For example, testing only that `i` appears
would not decide H1, H2, H5, or H9; those tests inspect the proof relations and
loop placement as well as the name.

## 2. Example-based experiments

The unit fixtures construct semantic CPGs directly, so they isolate analysis
semantics from a particular grammar:

| Experiment | Discriminating condition |
|---|---|
| `explicit_loop_reports_invariants_basic_and_derived_ivs_and_counted_bound` | Simultaneously checks an invariant assignment, basic step three, derived coefficient two/step six, external initialization, and a counted guard. |
| `semantic_range_loop_reports_implicit_step_and_counted_evidence` | Requires range binding `i` to become an implicit step-one IV without a language feature. |
| `nested_range_loops_keep_induction_and_bounds_loop_local` | Two nested ranges must attach each IV and bound to the correct stable loop id. |
| `recurrence_requires_an_invariant_operand_unique_definition_and_all_latches` | Mutates one fixture three ways: non-invariant step, duplicate definition, and an added latch that bypasses the update; all must reject `i`. |
| `subtraction_unknown_coefficients_and_overflow_remain_conservative` | Checks negative direction, incompatible guard rejection, symbolic coefficient, and checked multiplication overflow. |
| `update_shapes_directions_and_checked_arithmetic_cover_supported_forms` | Enumerates compound/simple update shapes, zero direction, and negation overflow. |
| `affine_recognition_and_malformed_semantic_shapes_cover_every_operator_family` | Exercises supported affine operators and malformed/missing operands without relying on parser behavior. |
| `unknown_and_irreducible_loops_do_not_claim_counted_trip_evidence` | Prevents structural cycles from being mislabeled as counted loops. |
| `malformed_selectors_foreign_forests_and_ast_cycles_are_bounded` | Decides error identity and termination for absent nodes, foreign forest members, and cyclic AST relations. |
| `twenty_five_thousand_node_loop_region_is_stack_safe` | Makes recursive region traversal fail on ordinary native stack limits while the iterative implementation completes. |
| `all_public_result_layers_round_trip_through_serde` | Serializes and reconstructs the complete public report when `serde` is enabled. |

The public integration file adds two feature-free downstream-user tests: one
for semantic ranges and graph non-mutation, and one for a compound basic IV and
direction-compatible guard. These import only crate-root APIs, catching missing
exports or accidental feature gates that unit tests inside the module cannot.

## 3. Property-based experiments

Both `proptest` campaigns run 192 generated cases.

### Insertion-order determinism and contracts

`analysis_is_insertion_order_deterministic_and_satisfies_contracts` generates a
nonzero step in $`[-32,32]`$, an affine coefficient in $`[-16,16]`$, and either
guard operand order. For each case it:

1. computes a loop forest and induction report;
2. rebuilds the same nodes and typed edges in reverse insertion order;
3. requires the two forests and reports to be exactly equal;
4. checks the basic step and checked derived product; and
5. independently traverses every reported invariant use and verifies the H1
   reaching-definition predicate.

The permutation matters because `petgraph` adjacency iteration reflects
insertion order. Exact result equality is stronger than comparing set sizes and
detects unstable tie-breaking.

### Checked direction and comparison oracle

`checked_direction_and_comparison_oracles_match_integer_semantics` generates
the full `i64` step domain and each supported comparison. It derives direction
independently with `cmp(&0)`, computes the expected direction/operator relation
with a separate truth table, and requires comparison reversal to be an
involution:

```math
reverse(reverse(op)) = op.
```

This experiment covers extreme integer endpoints that a small list of examples
would be unlikely to select.

## 4. Parser-backed acceptance experiment

The `lang-rust` integration test parses exactly this source through the public
builder:

```rust
fn consume(_: usize) {}
fn counted(x: usize) {
    for i in 0..x {
        consume(i);
    }
}
```

It first asserts that the mapper produced a semantic `BinaryOp("..")` range and
a `Variable("i")` binder beneath the `For` header. It then requires:

- `i` has exact step one and increasing direction;
- its update node is the `For` header and lies in its loop body;
- its initialization evidence is exactly the semantic binder;
- its loop has `TripEvidence::Counted` naming `i`;
- every invariant use satisfies the DFG fixed-point contract;
- every hoist candidate belongs to the invariant set; and
- node and edge counts are unchanged by analysis.

This is also a parser-boundary test: if a Rust grammar upgrade stops preserving
range or binder semantics, the test fails at the semantic-node assertion rather
than encouraging the analysis to scan source text.

## 5. Malformed-input and resource experiments

`tests/robustness.rs` injects ten corruption classes, including dangling
parents/children/definitions, AST cycles, missing pointer/edge symmetry, and a
bodyless function. Both fixed and generated campaigns run `loop_forest` and
then `loop_induction` over the validated forest. They require termination,
absence of panics, real-node result references, equal loop/result cardinality,
and the hoist-subset law. An absent selector must return the exact
`AnalysisError::UnknownNode` value.

The dedicated 25,000-node example isolates stack safety from the mixed
corruption campaign. Together these experiments decide structural robustness;
they do not claim a semantically meaningful result for a corrupt graph.

## 6. Clean maximal-matrix coverage result

The stable `cargo cov-text` and nightly `cargo +nightly cov-branch` experiments
both ran the complete `full + rholang + metta` matrix: 627 library tests and all
35 integration/robustness cases. For `analysis/induction.rs`, they measured:

| Metric | Covered | Epic threshold |
|---|---:|---:|
| Lines | **97.67 %** | 90 % |
| Regions | **97.44 %** | — |
| Functions | **97.26 %** | — |
| Branches | **84.44 %** | 80 % |

The whole repository measured 97.95 % lines, 97.88 % regions, 98.08 %
functions, and 84.27 % branches in the same clean runs. Both named epic
thresholds are therefore decided by maximal-matrix evidence, not the earlier
focused diagnostic run.

The feature-free `induction_analysis` Criterion target supplies a separate
performance experiment on single counted loops with 64, 512, and 2,048
invariant definitions. Its pre-timing oracle decides H1, H2, H3, H5, H6, and
read-only behavior before Criterion measures `loop_induction`; fixture and
forest construction are outside the timed closure. Timing evidence remains
local to those named workloads and is not a correctness or whole-corpus claim.

## 7. Reproduction

```text
# Feature-free unit and public integration evidence:
cargo test --no-default-features --lib induction::tests
cargo test --no-default-features --test induction_integration

# Real Rust parser acceptance:
cargo test --no-default-features --features lang-rust \
    --test induction_integration

# Serialization contract:
cargo test --no-default-features --features serde \
    --lib induction::tests

# Repository-wide feature and malformed-input matrix:
./scripts/verify.sh

# Execute each induction benchmark workload once with its full oracle:
cargo bench --no-default-features --bench induction_analysis -- --test

# Controlled Criterion measurement and named baseline:
./scripts/bench.sh induction_analysis --save-baseline before

# Maximal line/region and nightly branch measurements:
cargo cov-text
cargo +nightly cov-branch
```

The engineering census and measured coverage are recorded in
[`engineering/02-testing.md`](../engineering/02-testing.md). A focused test run
is useful for diagnosis but is not substituted for the clean maximal-matrix
coverage gate.

## 8. Interpretation boundary

The experiments corroborate the implemented semantic model: supported
additive/affine shapes, explicit DFG evidence, natural-loop structure, and
language-neutral checked arithmetic. They do **not** prove exact trip counts,
termination, language-specific overflow behavior, or transformation safety.
`TripEvidence::Counted` and `hoistable` remain inputs to stricter downstream
policy. This distinction is important for runtime/size work, where structural
loop evidence is combined with stronger symbolic bounds [[2]](#references).

## References

1. Aho, A. V., Sethi, R., Ullman, J. D. (1986). *Compilers: Principles,
   Techniques, and Tools*, §9.5, “Loops in Flow Graphs.” Addison-Wesley.
   ISBN 0-201-10088-6 (no DOI).
2. Brockschmidt, M., Emmes, F., Falke, S., Fuhs, C., Giesl, J. (2014).
   *Alternating Runtime and Size Complexity Analysis of Integer Programs.*
   TACAS 2014, pp. 140–155. DOI:
   [10.1007/978-3-642-54862-8_10](https://doi.org/10.1007/978-3-642-54862-8_10)
