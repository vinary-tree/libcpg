# Scientific validation 72 -- Semantic Datalog boundary

## Claims under test

The campaign tests that:

1. exported proof edges replay and nontrivial classes are connected by replayed
   exact evidence;
2. stable source identities, not engine-local identifiers, define public rows;
3. CSE/PRE proof indices match endpoints and span their class;
4. positives survive honest limits while absence requires both fixed points;
5. canonical output is independent of candidate insertion order;
6. facts are owned and ordinary CPG export plus the CPG remain unchanged;
7. malformed reports and resource excess fail closed with typed errors;
8. serialization is deterministic but cannot recreate proof authority;
9. Replete, Datalog, and CPG responsibilities remain acyclic; and
10. large lifecycles do not consume size-dependent native stack.

## Formal triangulation

[`ResponsibilityBoundary.tla`](../../formal/tla/ResponsibilityBoundary.tla)
varies replay, equality and Datalog completion, stable/canonical identity, and
every forbidden architecture request. TLC generates and checks 2,048 distinct
reachable states at depth 2 with every invariant true.

[`ResponsibilityBoundary.v`](../../formal/rocq/ResponsibilityBoundary.v) proves
positive/negative evidence implications, exact completion, rejection of
shared state, reverse dependencies, embedded runtimes, tuple authority and
internal IDs, canonical source pairs, and immutable export. Sixteen printed
theorem groups close with no axioms or admissions.

[`responsibility_boundary.smt2`](../../formal/z3/responsibility_boundary.smt2)
independently refutes twenty unsafe publication, absence, completion,
dependency, authority, identity-leak, and mutation states. Every query is
`unsat`; none is `sat` or `unknown`.

Run all formal evidence with:

```bash
./scripts/verify-responsibility-boundary-formal.sh
```

## Rust refinement evidence

Eleven focused unit/property tests cover:

- canonical class, proof, availability, anticipability, CSE, PRE, and
  completion export;
- independent replay of every exported proof index;
- forward/reverse candidate-order byte equivalence;
- preservation of positives under equality work limitation;
- every equality, dataflow, candidate-work, and caller-evaluation limit while
  retaining replayed positive evidence;
- owned use after dropping the source report, with identical clone, rendering,
  and serialization;
- reversed members, missing CSE proof, disconnected class, disconnected PRE
  proof, unknown point class, duplicate proof/candidate/point evidence, and
  inconsistent completion corruption;
- zero, fact-count, and exact encoded-byte limit rejection;
- a generated property over all 91 semantic/Datalog completion pairs, where
  conclusive absence is true for exactly one pair; and
- exact canonical completion relation bytes; and
- 100,000 singleton rows exported, cloned, rendered, serialized, and dropped
  on a 256 KiB worker stack.

The parser-backed integration test builds a real Rust tree-sitter CPG, lowers
two equal typed expressions, runs Replete-backed equality and libcpg semantic
flow, exports semantic facts, and replays the indexed proof. It exports the
ordinary CPG EDB before and after and checks identical facts plus unchanged
node/edge counts and graph representation.

The compile-fail rustdoc proves `SemanticDatalogFacts` has no conversion to
`ExpressionEquivalenceProof`. Private bundle fields and proof constructors
provide the stronger API boundary.

## Architecture gate

`scripts/verify-reasoning-boundaries.sh` checks Cargo trees and source modules.
It requires one Replete package in libcpg; rejects egg, egglog, and Soufflé
runtimes; rejects a Replete-to-libcpg reverse dependency; separates ordinary
Datalog from equality; keeps the bridge free of Replete/CPG internals; and
rejects shared mutable bridge state.

Structural checks complement behavior tests; neither alone establishes the
ownership claim.

## Stack and resource interpretation

The 100,000-row test demonstrates constant native-stack use for export, clone,
rendering, serialization, and destruction. It does not prove bounded heap use
without limits. Fact/byte caps and checked arithmetic provide the in-process
bound; campaign commands additionally run under `systemd-run --user --scope`
with `MemoryMax=4G`, `MemorySwapMax=0`, and one Cargo build job.

No heaptrack or graphical profiler is opened. Verification output is captured
under `target/verification` and cleaned after inspection.

## Interpretation limits

This evidence validates the formal responsibility boundary. It does not
validate arbitrary caller rules, external engine correctness, frontend
completeness, whole-program transformation soundness, or profitability.
Representative self-membership follows equality identity; non-reflexive
connectivity requires replayed evidence.

Replete is standalone and unpublished. libcpg consumes exact package version
`0.1.0` at canonical Git revision
`907fc4b953e7e13beb237e8ce369629a15eb4cb8`; registry publication remains a
separate operation, not an adapter change. MeTTaIL was not modified, built,
formatted, branched, or committed.

## References

1. Nelson, G., Oppen, D. C. (1980). *Fast Decision Procedures Based on
   Congruence Closure.* DOI:
   [10.1145/322186.322198](https://doi.org/10.1145/322186.322198).
2. Tate, R., Stepp, M., Tatlock, Z., Lerner, S. (2009). *Equality Saturation: A
   New Approach to Optimization Phases.* DOI:
   [10.1145/1480881.1480915](https://doi.org/10.1145/1480881.1480915).
3. Jordan, H., Scholz, B., Subotić, P. (2016). *Soufflé: On Synthesis of
   Program Analyzers.* DOI:
   [10.1007/978-3-319-41540-6_23](https://doi.org/10.1007/978-3-319-41540-6_23).
