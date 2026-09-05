# Formal verification

The formal tree contains executable and deductive models for semantic boundaries
whose failure could turn an advisory libcpg result into an unsound compiler or
security claim. Formal artifacts specify the design before production code is
written; implementation tests then refine each model invariant.

## Expression intermediate representation

T4.1 uses three complementary checks:

- The TLA+ ExpressionIr model exhaustively explores a bounded iterative lowering
  machine over tree, shared-subgraph, and cyclic facts. It varies every node's
  evidence mode, profile completeness, rewrite permission, and exact work/node
  cap. TLC checks postorder references, honest completion, fail-closed limits,
  and certificate soundness.
- The Rocq ExpressionIr development proves for arbitrary profiles that a public
  rewrite certificate implies complete lowering, a fully specified semantic
  profile, a pure subtree, and permission for the named rewrite law. It also
  proves the flat-tape append and limit-precedence lemmas without admitted
  assumptions.
- The Z3 expression_ir model independently refutes every Boolean combination
  in which eligibility coexists with one missing prerequisite.

Run all three with:

~~~bash
./scripts/verify-expression-ir-formal.sh
~~~

The bounded TLA+ state space is not a proof about arbitrary Rust heap sizes.
The universally quantified Rocq theorems cover the certificate boundary; Rust
example, property, differential, malformed-input, and small-stack tests cover
the refinement from typed CPG facts to the implementation.

## Expression equivalence

T4.2 formally refines T4.1's admission boundary before using Replete:

- The TLA+ model enumerates exact and alpha modes, namespace boundaries,
  fixed-point completion, forced stops, distinct resource caps, proof
  availability, and deterministic positive prefixes.
- The Rocq development proves the admitted equality laws, alpha qualification,
  replay soundness, monotone knowledge, AC permutation/multiplicity
  preservation, and nested closed-term evaluation without admitted axioms.
- The Z3 model refutes 22 forbidden states, including unreplayable positives,
  unsaturated negative conclusions, illegal AC evidence, and mismatched or
  unevaluable closed folds.

Run all three with:

~~~bash
./scripts/verify-expression-equivalence-formal.sh
~~~

The bounded TLC exploration complements rather than replaces the universal
Rocq results. Rust example, property, parser-backed, corruption, cap,
serialization, and 100,000-level small-stack tests validate the implementation
refinement.

## Semantic expression flow

T4.3 composes exact expression evidence with available and very-busy
dataflow without confusing equality with a control-flow fact:

- The TLA+ model checks 391,168 reachable combinations of proof, completion,
  availability, dominance, kill, anticipability, evaluator, and reporting
  states.
- The Rocq development proves path-transfer laws, must-fact soundness,
  PRE/CSE prerequisites, four-way semantic-diff classification, evidence
  consistency, and CPG immutability.
- The Z3 model refutes twenty-four forbidden states, including availability
  without dominance, intervening kills, decisive evidence hidden by limits,
  proof/evaluator contradictions, and noncanonical public pairs.

Run all three with:

~~~bash
./scripts/verify-semantic-expression-flow-formal.sh
~~~

The implementation refinement uses deterministic heap work queues and flat
expression/proof tapes. It reports advisory candidates and additive semantic
diff evidence; it never mutates the CPG or changes the legacy structural-diff
taxonomy.

## Datalog, e-graph, and CPG responsibility boundary

T4.4 formally fixes the one-way evidence and dependency boundary before
semantic equality becomes a caller-owned Datalog EDB relation:

- The TLA+ model exhaustively varies proof replay, equality/Datalog completion,
  stable/canonical identity, and attempted shared-state, reverse-dependency,
  embedded-runtime, tuple-authority, and internal-ID violations.
- The Rocq development proves that exported positives require proofs, negatives
  require both fixed points, completion is honest, every architecture shortcut
  is rejected, public pairs are canonical, and owned export preserves its CPG
  and semantic-report inputs.
- The Z3 model refutes twenty forbidden publication, absence, completion,
  dependency, authority, identity-leak, and mutation states.

Run all three with:

~~~bash
./scripts/verify-responsibility-boundary-formal.sh
~~~

The intended implementation copies stable source identities and explicit
completion into owned canonical fact rows. It shares no mutable Replete or
Datalog engine state, exposes no engine-local IDs, and gives no Datalog tuple
the authority of an independently replayed equality proof.
