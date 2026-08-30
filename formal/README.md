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
./scripts/verify-expression-equivalence-formal.sh all
~~~

The bounded TLC exploration complements rather than replaces the universal
Rocq results. Rust example, property, parser-backed, corruption, cap,
serialization, and 100,000-level small-stack tests validate the implementation
refinement.
