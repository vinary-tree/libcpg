# ADR-0074: Admit rewrites through a typed pure-expression IR

- **Status:** Accepted
- **Date:** 2026-08-30
- **Scope:** language semantic profiles, pure-expression lowering, rewrite
  admission, persistence authority, stack and resource safety

## Context

The Code Property Graph (CPG) records normalized expression categories and
ordered Abstract Syntax Tree (AST) children, but an operator spelling alone is
not a language-semantic contract. For example, addition may wrap, trap, become
undefined, use arbitrary-precision integers, or obey strict IEEE 754 floating
point. Calls, memory reads, exceptions, volatile access, short-circuiting, and
evaluation order can also make an algebraic identity observably wrong.

libcpg needs a language-neutral boundary from typed frontend evidence to later
equality-saturation work. That boundary must not become another parser, must not
mutate the CPG, and must never turn missing knowledge into an optimization or a
negative claim. The equational engine, Replete, is a standalone crate and is
not owned by libcpg.

![Typed facts and profiles reach certification only through complete, pure, live lowering evidence.](../diagrams/expression-ir-trust-boundary.svg)

*Figure -- expression semantic trust boundary. Source:
[`expression-ir-trust-boundary.puml`](../diagrams/expression-ir-trust-boundary.puml).*

## Decision

1. Frontends supply an `ExpressionFactTable`. Each fact names a closed typed
   operation, ordered source operands, a result type, and explicit effects.
2. A `SemanticProfile` declares ten dimensions: effects, evaluation order,
   integers, floating point, exceptions, short-circuit behavior, aliasing,
   calls, volatile/atomic access, and binding/scope identity.
3. `lower_expression_ir` validates facts against immutable CPG categories,
   literal identities, ordered AST topology, call/binding resolution, types,
   and language identity. It never examines source text or operator strings.
4. Lowering is an explicit `Enter`/`Exit` heap machine. It emits a flat
   postorder arena in which every operand identifier precedes its parent and a
   shared source expression is emitted once.
5. Every resource boundary is exact and typed: `WorkLimited`, `NodeLimited`,
   `OperandLimited`, or `Malformed`. Only `Complete` exposes a root eligible for
   certification.
6. Nodes carry `Pure`, `IdentityOnly`, or `Unknown` evidence. Effects and
   unsupported calls/memory accesses propagate conservatively through parents.
7. A `RewriteCertificate` names exactly one law. It is constructible only after
   checking complete lowering, live provenance, a total profile, pure subtree
   evidence, profile permission, and the law's type/value preconditions.
8. Serialization retains inspectable structure but omits live provenance.
   Deserialization produces `PersistedIdentityOnly`; it can never mint a
   certificate. Certificates implement serialization for audit output but do
   not implement deserialization.
9. Built-in profiles are provided only for Rust, C, C++, Go, Java, Python,
   JavaScript, TypeScript, and Ruby. Every other language receives an explicit
   unknown profile.

## Alternatives rejected

### Infer semantics from source spelling

Rejected because the same spelling has different types, effects, overflow,
dispatch, and evaluation rules across languages and contexts. It would also
duplicate the existing frontend parsers.

### Send raw CPG expressions directly to an e-graph

Rejected because congruence closure preserves asserted equalities; it does not
prove those equalities. An unsound rewrite remains unsound when efficiently
represented.

### Treat deserialized artifacts as trusted

Rejected because bytes cannot re-establish the originating CPG, frontend fact
derivation, or live validation. Cryptographic signing could authenticate an
issuer, but would still not recreate the semantic proof performed by this
process.

### Encode incompleteness as an empty result

Rejected because an empty limited result would be indistinguishable from a
complete negative result. Completion is therefore an explicit sum type.

## Consequences

- Consumers receive deterministic, typed, source-linked rewrite evidence.
- New frontends must define every profile dimension and emit exact facts before
  receiving rewrite authority.
- Strict IEEE 754 arithmetic remains conservative; finite reassociation is a
  separate explicit mode whose literals reject NaN, infinities, and negative
  zero.
- Persistence is suitable for inspection, caching, and replay diagnostics, but
  reauthorization requires fresh lowering against live CPG and frontend facts.
- Replete can remain generic and standalone: libcpg owns program evidence,
  while Replete owns congruence, saturation, and extraction.

## Verification obligations

The executable model and proof ledger are in
[`t4-1-expression-ir-formal-plan.md`](../verification/t4-1-expression-ir-formal-plan.md).
TLC checks the bounded operational machine and persistence transition; Rocq
proves the universal admission implications; Z3 independently refutes the
forbidden Boolean, integer, shift, and floating-point boundary states.

Production acceptance additionally requires example, property, parser-backed,
malformed-input, exact-cap, serde, and 100,000-level 256 KiB-stack campaigns.

## References

1. Tate, R., Stepp, M., Tatlock, Z., Lerner, S. (2009). *Equality Saturation: A
   New Approach to Optimization Phases.* POPL. DOI:
   [10.1145/1480881.1480915](https://doi.org/10.1145/1480881.1480915).
2. Willsey, M. et al. (2021). *egg: Fast and Extensible Equality Saturation.*
   POPL. DOI: [10.1145/3434304](https://doi.org/10.1145/3434304).
3. Nelson, G., Oppen, D. C. (1980). *Fast Decision Procedures Based on
   Congruence Closure.* JACM. DOI:
   [10.1145/322186.322198](https://doi.org/10.1145/322186.322198).
4. Leroy, X. (2009). *Formal Verification of a Realistic Compiler.* CACM. DOI:
   [10.1145/1538788.1538814](https://doi.org/10.1145/1538788.1538814).
