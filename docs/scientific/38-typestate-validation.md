# Scientific validation 38 — Finite-state typestate analysis

This ledger tests whether a caller-defined finite-state property is evaluated
deterministically over represented allocation, data-flow, call, and CFG
evidence. It does not test runtime feasibility or claim a whole-program
alias/points-to proof.

## 1. Hypotheses

| ID | Hypothesis | Falsifying observation |
|---|---|---|
| H1 | `open → use → close` is clean and terminally accepting | a violation appears or `closed` is absent at exit |
| H2 | `open → close → use` reports exactly the closed-state `use` with allocation/site witness | the event is missed, duplicated, or assigned another prior state |
| H3 | the error state absorbs follow-on events | later events on the same erroneous path emit duplicate findings or leave error |
| H4 | branch joins implement may-union | either the legal open state or forbidden closed-state path disappears |
| H5 | DFG/reference/alias evidence preserves allocation identity without name matching | an alias loses the object or spelling alone creates one |
| H6 | malformed policy/evidence and every applicable cap prevent a clean claim | an invalid, unresolved, ambiguous, unplaced, or capped run is `is_complete()` |
| H7 | equivalent edge insertion orders produce byte-equivalent ordered values | reversed CFG/DFG insertion changes report order or content |
| H8 | increasing only the work budget cannot turn a complete result incomplete | a larger cap regresses completeness or changes a formerly complete answer |
| H9 | the public Rust parser pipeline supplies real allocation/argument flow | a double close is missed without hand-added edges |
| H10 | graph depth does not consume proportional native stack | depth 100,000 overflows a 256 KiB worker stack |
| H11 | analysis is read-only and serialization preserves evidence | graph counts change or serde round-trip differs |

## 2. Independent linear oracle

For 256 generated cases, a Boolean vector independently chooses `close` or
`use` after one `open`. The oracle stores a single Boolean `open` flag. A close
when false or a use when false is the first forbidden index; otherwise close
sets the flag false and use preserves it. The oracle never calls the production
property transfer.

The production report must be complete, contain exactly zero or one finding,
and point to the oracle's first forbidden call. This checks ordinary transfer
and absorbing-error behavior across vectors up to 47 events.

## 3. Metamorphic properties

Two further 256-case properties test relations between executions rather than
single examples:

1. **Insertion permutation.** Build equivalent CPGs with forward and reversed
   CFG/DFG edge insertion. Their complete `TypestateAnalysis` values must be
   equal.
2. **Budget monotonicity.** For work budgets $`b_1\le b_2`$, if the report at
   $`b_1`$ is complete, the report at $`b_2`$ must also be complete with equal
   violations and terminal states.

These properties target deterministic scheduling and explicit cap semantics,
not only event-transition examples.

## 4. Example-based matrix

| Family | Examples |
|---|---|
| positive | use-before-close, correct terminal acceptance, known non-alphabet call |
| negative | use-after-close, repeated events after error, branch-specific invalid path |
| identity | distinct allocation-site key, variable definition, DFG alias, call argument |
| call evidence | typed callee identifier and represented target without callee syntax |
| policy robustness | empty names, open vocabulary, nonabsorbing error, allocation/transition conflict |
| graph robustness | unresolved call, missing CFG, missing point, ambiguous parent/target surfaces |
| resource bounds | exact function, worklist, fact-per-node, and callee-depth evidence |
| compatibility | `serde` policy/report round-trip and `default = []` public API |
| integration | immutable hand graph, parser-built Rust double close, small-stack depth gate |

## 5. Stack-safety experiment

The integration gate constructs a function CFG with 100,000 sequential block
points between `open` and `close`. Analysis runs on a worker configured with a
256 KiB native stack. Success requires a complete, clean report with terminal
state `closed`. The test exercises function scope, canonical projection, object
flow, fact propagation, predecessor retention, and teardown through the public
crate-root API.

This experiment supports stack usage independent of CFG depth for this path. It
does not bound heap memory; `IfdsCaps` and the complexity model cover that
separate resource.

## 6. Release matrix

| Configuration | Evidence |
|---|---|
| `--no-default-features` | 14 unit/property tests and two public integration gates |
| `--features serde` | exact policy and report round-trip plus the deep public gate |
| `--features lang-rust` | parser-built CFG/DFG double-close integration |
| strict Clippy | all feature-free targets with warnings denied |
| rustdoc and links | public API docs plus indexed DOI-linked documentation |

## 7. Interpretation limits

Passing the suite supports conformance to the documented finite
allocation-site may-analysis. It cannot establish branch feasibility, dynamic
dispatch completeness, field/container points-to precision, runtime instance
identity, temporal concurrency ordering, or vulnerability impact.

## References

1. Strom, R. E., Yemini, S. (1986). *Typestate: A Programming Language Concept
   for Enhancing Software Reliability.* DOI:
   [10.1109/TSE.1986.6312929](https://doi.org/10.1109/TSE.1986.6312929).
2. Fink, S. J., Yahav, E., Dor, N., Ramalingam, G., Geay, E. (2008). *Effective
   Typestate Verification in the Presence of Aliasing.* DOI:
   [10.1145/1348250.1348255](https://doi.org/10.1145/1348250.1348255).
