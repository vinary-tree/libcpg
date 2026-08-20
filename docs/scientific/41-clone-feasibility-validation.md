# Scientific validation 41 — Clone and feasibility advisories

This ledger validates I8 inconsistent-clone and I9 infeasible-path results
against independent exact oracles. It establishes conformance to represented
CPG evidence and configured bounds, not real-world defect precision.

![The validation targets both independent advisory evidence paths](../diagrams/clone-feasibility-advisory-pipeline.svg)

## 1. Hypotheses

| ID | Hypothesis | Falsifying observation |
|---|---|---|
| H1 | one anomalous destination among ten aligned renames produces one exact `9/10` finding | another count/support/witness is reported |
| H2 | aligned clone guard payloads distinguish exact operators and literals | a flip is missed or equal payload is reported |
| H3 | exact Type-3 pair evidence is evaluated once | a pair disappears or is duplicated through class closure |
| H4 | $`x>5\land x<3`$ has no true interval alternative | a true state survives or predicate order changes |
| H5 | $`x<3\lor x\geq3`$ has no false alternative but is not contradictory | false survives or contradiction is emitted |
| H6 | `if true` has one dead false edge and one redundant-true guard | SCCP/interval evidence disagrees |
| H7 | equality/disequality regions and signed extrema are exact | excluded constants return or arithmetic overflows |
| H8 | same-block transfers reach the interval state immediately before the guard | a preceding exact assignment is omitted |
| H9 | caps, malformed nodes, cycles, and unsupported operators make reports visibly incomplete | evidence is silently classified or loops |
| H10 | insertion permutations and repeated runs produce equal ordered reports | any report value/order changes |
| H11 | serde preserves every public policy and report value | round trip changes evidence |
| H12 | represented depth does not consume proportional native stack | 100,000 nodes overflow 64 KiB |
| H13 | analyzers are read-only | node or edge counts change |

## 2. Independent I8 oracles

The rename fixture creates ten aligned pairs without calling the production
mapping inference. A generated anomaly count $`q`$ gives independent dominant
support $`(10-q)/10`$. The oracle decides the exact threshold through widened
integer cross multiplication and predicts the anomalous node-id prefix.

Guard fixtures vary the two typed payloads while keeping shape fixed. Type-3
fixtures insert one node into one of two four-item bodies; the oracle predicts
one edit over a six-entry larger tape and exactly one pair relation. Generated
small tapes compare production alignment with an independently enumerated edit
distance oracle. Reverse insertion order must leave all reports equal.

## 3. Independent I9 oracle

The property generator builds bounded formulas over one integer variable and
supported comparisons. For every integer in an independent finite domain, a
direct evaluator computes the concrete truth set. Production true/false
alternatives are compared with that exhaustive set for 256 generated cases.
The concrete evaluator does not call condition arenas, interval regions, SCCP,
or production transfer functions.

Separate examples cover correlations beyond isolated comparison constants:

| Condition | Expected result |
|---|---|
| `x > 5 && x < 3` | true edge infeasible; redundant false; contradiction with two predicates |
| `x < 3 || x >= 3` | false edge infeasible; redundant true; no contradiction |
| `true` | false edge infeasible; redundant true; no interval contradiction |
| prior `x = 7`, then `x > 5` | false edge infeasible; entry interval is exactly `[7,7]` |

## 4. Boundary matrix

| Family | Examples |
|---|---|
| I8 admission | Type-2 rename, Type-2 guard flip, Type-3 insertion, non-low-edit exclusion |
| I8 evidence | exact support boundary, directionality, equal guards, consistent rename, GED evidence |
| I8 qualification | missing/cyclic/shared AST, node cap, alignment cap/allocation, GED cap, invalid policy |
| I9 Boolean semantics | literals, negation, conjunction/disjunction short circuit, reversed comparisons |
| I9 regions | strict/inclusive inequalities, equality, disequality, integer extrema, unsupported operators |
| I9 proof composition | SCCP-only, interval-only, combined proof source, unreachable-source exclusion |
| I9 qualification | missing guard/node/state, AST cycle, unsupported node, node/state/step cap, invalid policy |
| common contracts | projection equality, determinism, graph immutability, serde |
| public/frontends | combined public API and parser-built Rust CPG |
| stack safety | 100,000 AST nodes on a 64 KiB worker stack |

## 5. Metamorphic properties

- Reordering equivalent node and non-AST edge insertions preserves reports.
- Repeating an analyzer preserves byte-equivalent ordered evidence under serde.
- Renaming all consistently aligned identifiers produces no anomaly.
- Increasing anomalous rename count can only reduce dominant exact support.
- Negating a condition exchanges requested true and false partitions.
- Swapping comparison operands and reversing the operator preserves regions.
- Adding an unrelated function does not change an existing condition's proof.
- Analysis preserves node/edge counts and input payloads.

## 6. Stack-safety experiment

An I8 test creates a 100,000-node AST tape and exercises traversal/alignment
teardown on a 64 KiB worker stack. An I9 test creates 100,000 nested negations
and proves the expected outcome on the same native-stack size. Success requires
the exact report and clean worker teardown. These experiments exercise the flat
tapes, condition arena, explicit continuations, visited/active sets, witness
lifecycle, ordering, and destruction. Heap and total-work consumption remain
bounded separately by public caps.

## 7. Executed gates

| Gate | Evidence |
|---|---|
| feature-free I8 unit/property | 10 examples plus 128 alignment/insertion property cases |
| feature-free I9 unit/property | 10 examples plus 256 exhaustive integer-oracle cases |
| serde | 11 I9 cases and public I8/report round trips |
| affected dataflow suite | 87 SCCP, interval, lattice, property, and stack tests |
| public integration | exact manual witnesses and graph immutability |
| parser integration | Rust clone flip, contradiction, and constant redundant guard |
| runnable example | exact one-clone and three-feasibility runtime assertions |
| Criterion | both analyzers at 8 and 32 functions; each workload executed once |
| strict Clippy | no-default all targets with warnings denied |

## 8. Benchmark interpretation

`advisory_bugs` supplies controlled regression workloads, not universal timing
claims. `inconsistent_clones` includes pair discovery, tape/alignment/GED, and
finding construction. `infeasible_paths` includes per-function SCCP, interval
state derivation, condition evaluation, and report construction. Fixture size,
compiler, target, and Criterion environment must accompany any reported time.

## 9. Interpretation limits

The oracles establish exact implementation semantics for finite represented
inputs. They do not establish that clone divergence implies a fault, that
frontends capture every runtime path, that intervals decide arbitrary symbolic
conditions, or that a reported dead branch is safe to delete.

## References

1. Juergens, E., Deissenboeck, F., Hummel, B., Wagner, S. (2009). *Do Code
   Clones Matter?* DOI:
   [10.1109/ICSE.2009.5070547](https://doi.org/10.1109/ICSE.2009.5070547).
2. Wegman, M. N., Zadeck, F. K. (1991). *Constant Propagation with Conditional
   Branches.* DOI:
   [10.1145/103135.103136](https://doi.org/10.1145/103135.103136).
3. Cousot, P., Cousot, R. (1977). *Abstract Interpretation.* DOI:
   [10.1145/512950.512973](https://doi.org/10.1145/512950.512973).
