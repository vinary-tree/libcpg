# Complexity-evidence validation

This page records the falsifiable claims and executable experiments behind
`ComplexityAnalyzer::estimate_time_complexity_with_evidence`. The method
combines a validated Control Flow Graph (CFG), dominator-derived loop forest,
and Data Flow Graph (DFG)-backed induction report into an advisory Big-O class.
The tests therefore decide both the positive evidence path and the exact
compatibility boundary where the original Abstract Syntax Tree (AST) heuristic
must be returned unchanged.

## 1. Hypotheses

| ID | Falsifiable hypothesis | Deciding evidence |
|---|---|---|
| H1 | A fully counted reducible nest of depth $`d`$ maps to exactly one polynomial degree and confidence `1.0`. | depth-one-through-five example; 128-case generated depth campaign |
| H2 | The justification names every loop header, IV, comparison, and bound that decides H1. | complete-node-evidence assertions; public nested-loop integration |
| H3 | An unknown trip cannot exceed the legacy loop confidence, and an irreducible region becomes one linear factor at confidence `0.3`. | `while true` and two-entry-cycle counterexamples |
| H4 | No CFG edge, absent selector, empty forest, or rejected downstream evidence changes the legacy class, confidence bits, or justification. | fixed fallback examples; 128 generated well-formed CFG-free CPGs; parser loop-free corpus |
| H5 | Loop and recursion composition keeps the asymptotically worse class without erasing subordinate evidence. | counted-loop plus recursive-call example |
| H6 | Equivalent semantic CPGs stored in opposite node/edge orders return byte-identical justifications and equal classes/confidences. | storage-permutation property |
| H7 | The public API and `DefaultAlgorithmDetector` consume the evidence path, remain read-only, and accept real tree-sitter Rust ranges. | public integration and parsed sorting experiment |
| H8 | The feature boundary is correct: providers stay feature-free, the consumer requires only `algorithm-detection`, and no manifest dependency changes. | feature-only public build/test and manifest diff gate |
| H9 | Every benchmark fixture satisfies its class, confidence, header/comparison/bound, and non-mutation oracle before timing. | `complexity_evidence/*` Criterion preflight |

The hypotheses are deliberately independent. Seeing `Quadratic` for one
program does not decide determinism (H6), provenance (H2), fallback identity
(H4), or the feature contract (H8).

## 2. Example-based experiments

Seven unit tests isolate the consumer from parser behavior:

| Experiment | Discriminating condition |
|---|---|
| `counted_nests_use_exact_forest_depth_and_complete_node_evidence` | Enumerates depths one through five and requires exact class, confidence `1.0`, every header/bound/IV, stronger confidence than the heuristic, and graph non-mutation. |
| `unknown_trip_keeps_the_legacy_heuristic_confidence` | A natural `while true` cycle has no supported IV/bound relation; it must remain `Linear` at exactly `0.6` and name the unknown trip. |
| `irreducible_region_is_one_low_confidence_linear_factor` | A two-entry strongly connected region has no natural header and must receive the `0.3` floor. |
| `cfg_free_and_invalid_inputs_return_the_exact_legacy_estimate` | An AST-only loop and an absent selector must preserve all three legacy fields exactly. |
| `dominant_recursion_retains_subordinate_loop_evidence` | A recursive call ties/dominates the counted loop class and must retain the bound in subordinate evidence. |
| `evidence_reports_are_deterministic_under_storage_permutations` | Generated depth and storage order must not alter any result field. |
| `every_cfg_free_generated_function_falls_back_exactly` | Arbitrary well-formed AST-only CPGs must preserve the legacy result bit-for-bit. |

The public integration file adds three downstream-user experiments:

1. A feature-only, hand-built nested semantic range proves the method is
   exported under `algorithm-detection` alone and leaves node/edge counts
   unchanged.
2. A real Rust loop-free corpus requires exact equality with the legacy method.
3. A real nested Rust range sort requires `DefaultAlgorithmDetector` to attach
   `Quadratic`, confidence `1.0`, and both `i`/`j` counted-trip records.

The third test uses the normal tree-sitter frontend. It does not grant the
analyzer permission to parse: the assertion succeeds only because the frontend
has already normalized range/binder syntax into semantic nodes and built typed
CFG/DFG evidence.

## 3. Property-based experiments

Both campaigns run 128 generated cases.

### Storage-permutation determinism

`evidence_reports_are_deterministic_under_storage_permutations` generates a
nesting depth in $`[1,8]`$ and a storage direction. It builds semantically
identical CPGs with stable `NodeId`s while reversing petgraph node and CFG-edge
insertion. It then requires equality of:

```math
(\text{class},\ \operatorname{bits}(\text{confidence}),\ \text{justification}).
```

This is stronger than set equality. The justification exposes ordering, so any
provider or consumer that accidentally follows adjacency insertion order
refutes the property.

### Generated fallback identity

`every_cfg_free_generated_function_falls_back_exactly` draws from the shared
well-formed CPG generator, selects its function, and compares the new and
legacy methods. Because the generated graphs contain rich AST structure but no
CFG overlay, the experiment detects any attempt to partially use evidence or
rewrite the compatibility result.

## 4. Confidence oracle

Let $`I`$ mean at least one irreducible region, $`U`$ mean at least one unknown
trip, and $`c_h`$ be the clamped legacy loop confidence. The independent test
oracle is:

```text
CONFIDENCE(I, U, c_h)
  if I: return 0.3
  if U: return c_h
  return 1.0
```

The three fixed graph families select each branch. In particular, the unknown
case asserts `to_bits()` equality with the legacy confidence rather than a
tolerant floating-point comparison.

## 5. Integration, malformed input, and read-only behavior

`tests/complexity_evidence_integration.rs` imports only public paths. The
feature-only case catches accidental parser gating; the `lang-rust` cases catch
frontend semantic regressions. Every evidence fixture records graph node and
edge counts before analysis and requires equality afterward.

The repository malformed-input harness exercises
`DefaultAlgorithmDetector::detect` across fixed and generated corruptions under
`algorithm-detection`. Because the detector now calls the evidence-aware
method, those tests cover dangling relations, cycles, bodyless functions, and
invalid semantic shapes through the public consumer. Provider validation and
visited-set bounds must yield a result or fallback without panic or mutation.

## 6. Benchmark experiment

The existing `induction_analysis` Criterion target includes a gated
`complexity_evidence` group at loop-fixture sizes 64, 512, and 2,048. Before
each timed closure it requires:

- `Linear` class and confidence `1.0`;
- the stable loop header, comparison, and bound node in the justification;
- the named induction variable; and
- unchanged node and edge counts.

Fixture, forest, and expected-result construction occur outside the timed
closure. The target remains feature-free for its original `loop_induction`
group; enable `algorithm-detection` to add the complexity consumer:

```text
cargo bench --no-default-features --features algorithm-detection \
  --bench induction_analysis -- --test
```

Timing evidence is local to these single-loop fixtures and sizes. It is not a
whole-corpus or symbolic-complexity claim.

## 7. Clean maximal-matrix coverage result

The stable `cargo cov-text` and nightly `cargo +nightly cov-branch` experiments
both ran the then-complete `full + rholang + metta` matrix: 634 library tests
and all 38 integration/robustness cases. Those recorded runs predate the
condensation suite; the current census is in
[Testing](../engineering/02-testing.md). For
`algorithms/detection/complexity.rs`, they measured:

| Metric | Covered | Epic threshold |
|---|---:|---:|
| Lines | **97.36 %** | 90 % |
| Regions | **97.32 %** | — |
| Functions | **100.00 %** | — |
| Branches | **88.46 %** | 80 % |

The whole repository measured 97.94 % lines, 97.87 % regions, 98.09 %
functions, and 84.27 % branches in the same clean runs. Both named epic
thresholds are therefore decided by the maximal matrix rather than a focused
diagnostic run.

## 8. Reproduction

```text
# Feature-gated unit and property evidence:
cargo test --no-default-features --features algorithm-detection \
  --lib algorithms::detection

# Public feature-only and real-parser evidence:
cargo test --no-default-features --features algorithm-detection \
  --test complexity_evidence_integration
cargo test --no-default-features --features "algorithm-detection lang-rust" \
  --test complexity_evidence_integration

# Malformed public-consumer matrix:
cargo test --no-default-features --features algorithm-detection \
  --test robustness

# Oracle-guarded benchmark smoke:
cargo bench --no-default-features --features algorithm-detection \
  --bench induction_analysis -- --test

# Repository-wide feature, lint, rustdoc, link, and diagram gates:
./scripts/verify.sh

# Maximal line/region and nightly branch measurements:
cargo cov-text
cargo +nightly cov-branch
```

## 9. Interpretation boundary

The experiments corroborate the implemented model: canonical natural-loop
depth, supported DFG-backed trip evidence, the confidence precedence, total
fallback identity, and public detector integration. They do not prove that a
counted loop runs exactly $`n`$ times, that the function terminates, that an
irreducible region is mathematically linear, or that a family label is correct.
Those claims require stronger symbolic size, path, and language semantics.

McCabe's cyclomatic measure is also outside this claim. It counts independent
CFG paths [[2]](#references), whereas this experiment evaluates an advisory
input-size growth class.

## References

1. Cormen, T. H., Leiserson, C. E., Rivest, R. L., Stein, C. (2009).
   *Introduction to Algorithms* (3rd ed.), Chapter 4. MIT Press.
   ISBN 978-0262033848 (no DOI).
2. McCabe, T. J. (1976). *A Complexity Measure.* IEEE Transactions on Software
   Engineering SE-2(4), 308–320. DOI:
   [10.1109/TSE.1976.233837](https://doi.org/10.1109/TSE.1976.233837)
