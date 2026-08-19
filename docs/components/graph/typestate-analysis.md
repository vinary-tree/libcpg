# Component — Deterministic typestate analysis

## Responsibility

`analysis::typestate` checks caller-supplied finite-state object properties over
typed CPG evidence. It owns property validation, event classification,
allocation-site object flow, intraprocedural CFG propagation, canonical
witnesses, and explicit completeness evidence. It owns no parser, points-to
analysis, graph mutation, runtime verifier, or vulnerability verdict.

![Typestate component flow from policy and CPG to qualified findings](../../diagrams/typestate-analysis.svg)

## Feature boundary

The complete API is available with `default = []`. It consumes language-neutral
node and edge kinds. Enabling a `lang-*` feature is necessary only when the
caller wants libcpg to parse that source language internally.

## Public surface

| API/type | Contract |
|---|---|
| `PropertyFsm` | closed states/events, partial transitions, allocation seeds, accepting states, and absorbing error policy |
| `PropertyFsm::validate` | deterministic set of every structural policy issue |
| `analyze_typestate(cpg, fsm, caps)` | complete qualified report surface |
| `check_typestate(cpg, fsm, caps)` | convenience projection of positive findings only |
| `TypestateAnalysis` | findings, terminal states, processed scope, work, caps, and structural issues |
| `TypestateViolation` | allocation object, violating site, prior state, event, witness, and cap qualifier |
| `TypestateCapEvent` | function, work, fact, or call-depth boundary |
| `TypestateIssue` | invalid policy or incomplete CPG/event evidence |

All named states and events use `Arc<str>` aliases (`PropertyState` and
`PropertyEvent`). Public maps and sets are ordered. With `serde`, the FSM and
report structures serialize deterministically; transition/allocation maps use
ordered entry sequences so tuple keys remain representable in JSON.

## Internal flow

1. Validate the entire `PropertyFsm`; stop before graph work on any issue.
2. Enumerate functions and calls through ordered heap collections.
3. Reconstruct event paths from typed callee AST evidence, then represented call
   targets; qualify unresolved, ambiguous, or depth-capped evidence.
4. Seed one `AllocationSeed` per allocation call and propagate its object id
   through intrafunction DFG/reference/alias evidence.
5. Reuse `function_cfg_view` for the canonical function-local CFG projection.
6. Place source-ordered events at their nearest enclosing CFG point.
7. Propagate `(object, state)` may-facts through an ordered heap worklist,
   retaining first canonical predecessor evidence.
8. Emit one finding per `(object, site, from, event)` and accumulate terminal
   states at every exit-like point.
9. Mark all findings capped if any resource boundary makes the report partial.

## Invariants

1. Every reported object is an allocation call in the input CPG.
2. Every reported site and witness node exists in the input CPG.
3. The error state is absorbing and cannot be accepting.
4. Missing ordinary transitions enter error; non-alphabet calls are identity
   transfers.
5. Branch joins retain the union of possible states.
6. Event and fact order is independent of CPG edge insertion order.
7. Every graph-depth traversal is iterative and heap-backed.
8. CPG node and edge counts are unchanged.
9. `is_complete()` is false for every cap or structural/evidence issue.
10. `object_is_accepting` is meaningful as a clean claim only on a complete
    report.

## Bounds and failure semantics

| `IfdsCaps` field | Typestate use |
|---|---|
| `max_functions` | rejects a graph with too many represented functions before analysis |
| `max_facts_per_node` | bounds object-flow identities and CFG object/state facts |
| `max_worklist_steps` | shared budget across object flow and CFG propagation |
| `max_call_depth` | bounds typed callee-path reconstruction |
| `max_scc_iters` | not applicable; the finite fact worklist admits each fact once per point |

Caps set `capped` and retain typed `TypestateCapEvent` evidence. Malformed
policy or graph evidence remains in `issues`. Neither condition silently becomes
an empty clean result.

## Tests

Fourteen feature-free unit/property tests cover property validation, legal and
forbidden sequences, error absorption, joins, aliases, call targets, missing
evidence, all applicable caps, independent generated oracles, insertion-order
determinism, and cap monotonicity. Conditional serde coverage round-trips policy
and report exactly. Public integration checks immutability, the Rust parser's
real CFG/DFG pipeline, and CFG depth 100,000 on a 256 KiB native stack.
