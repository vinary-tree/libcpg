# ADR-0076: Compose expression proofs with semantic flow

- **Status:** Accepted
- **Date:** 2026-08-30
- **Scope:** semantic expression classes, must-availability, must-anticipability,
  advisory CSE/PRE candidates, and cross-revision semantic diff

## Context

A Code Property Graph (CPG) is libcpg's shared typed graph of syntax, control
flow, data flow, calls, and related program facts. ADR-0075 can prove that two
pure typed expressions are equal in one modeled semantic profile. Equality
alone cannot show that a computed value reaches a later program point, that a
definition was not overwritten, or that moving a computation is safe.

An **e-class** is a set of terms connected by replayable equality evidence.
**Availability** is the forward *must* fact that an expression has been
computed on every incoming control-flow path without a later kill.
**Anticipability**, also called **very busy**, is the backward *must* fact that
every outgoing path evaluates an expression before a kill or exit.
**Dominance** means every entry-to-use path passes through a node. A **kill** is
a represented definition or effect that can invalidate a value. A
`BindingIdentity` is the resolved definition node plus scope; spelling alone is
not binding identity.

Common-subexpression elimination (CSE) reuses an earlier equal computation.
Partial-redundancy elimination (PRE) considers placing a computation so that a
later partially redundant set can be reused. This API only identifies
candidates; it performs neither transformation.

![Replayed equality and two must-dataflow directions meet only at the advisory candidate boundary.](../diagrams/semantic-expression-flow-pipeline.svg)

*Figure -- proof-qualified semantic expression flow. Source:
[`semantic-expression-flow-pipeline.puml`](../diagrams/semantic-expression-flow-pipeline.puml).*

## Decision

1. Exact, independently replayed ADR-0075 proofs form deterministic semantic
   classes. Alpha-renamed clone evidence never forms a same-program value
   class.
2. Every class records exact resolved `BindingIdentity` dependencies. A
   represented definition kills only its dependent classes. An unresolved
   assignment, unmodeled call, `await`, or `yield` conservatively kills all
   classes; a separately admitted pure call is not such a barrier.
3. libcpg's iterative Kildall solver computes forward must-availability and
   backward must-anticipability over the basic-block control-flow graph.
4. CSE requires an exact proof, complete availability, one dominating
   producer, and an explicit check that no intervening kill lies on a
   producer-to-use path.
5. PRE requires a semantic class with at least two occurrences, complete
   anticipability, a nearest common dominator that dominates every occurrence,
   and proof edges spanning the class.
6. All results are advisory and additive. The CPG is borrowed immutably, and
   legacy structural `CpgDiff` and `ChangeKind` behavior is unchanged.
7. Cross-revision comparison treats old and new `NodeId` values as coordinates
   in independent graphs. Private comparison slots disambiguate them and never
   escape into public output.
8. Semantic diff has four outcomes. A replayed proof yields
   `ProvenEquivalent`; unequal exact closed values yield `ProvenDifferent`; an
   undecided incomplete comparison yields `Limited`; and an undecided complete
   comparison yields `Unknown`.
9. A proof/evaluator contradiction is a typed error. Decisive evidence takes
   precedence over an unrelated later limit.
10. Flow visits, candidate/path work, equivalence work, and closed-evaluation
    work have separate typed bounds. All depth-sensitive work uses heap arenas
    or explicit queues.

For class $`C`$ and point $`p`$, availability is the conjunction over every
entry path $`\pi`$:

```math
\operatorname{Avail}(C,p)
  \iff
  \bigwedge_{\pi\in\operatorname{Paths}(\operatorname{entry},p)}
  \operatorname{GeneratedAfterLastKill}(C,\pi,p).
```

A CSE candidate therefore obeys:

```math
\operatorname{CSE}(x,y)\Longrightarrow
\operatorname{ExactProof}(x,y)\land
\operatorname{Avail}([x],y)\land
\operatorname{Dom}(x,y)\land
\operatorname{NoInterveningKill}(x,y)\land
\operatorname{CompleteFlow}.
```

Availability cannot replace dominance. If both arms of a diamond generate an
equal expression, the class can be available at the join although neither arm
occurrence dominates it.

## Alternatives rejected

### Treat one e-class as sufficient optimization authority

Rejected because an e-graph contains equations, not control-flow paths,
definitions, effects, dominance, or placement safety.

### Encode equal expressions as one textual key

Rejected because identifier spelling can cross scopes and because a hash or
normal form cannot replace a replayed typed proof.

### Fold semantic status into structural `ChangeKind`

Rejected because structural alignment and semantic evidence have different
proof and completion boundaries. They must remain separately inspectable.

### Merge old and new CPG node namespaces

Rejected because two valid revisions commonly assign the same numeric
`NodeId` to unrelated nodes. Public results preserve each revision's original
coordinate system.

### Modify MeTTaIL as part of extraction

Rejected. Replete is an independent top-level crate. MeTTaIL is an immutable
read-only provenance source and is outside this campaign's write authority;
its pgmcp E3 task remains permission-gated and unimplemented.

## Consequences

- Positive candidates contain their proof references and stable source ids.
- A limited positive remains valid, but absence is meaningful only after the
  relevant equality and dataflow dimensions complete.
- Conservative barriers may omit profitable candidates; they cannot invent
  unsafe ones inside the represented model.
- JavaScript and TypeScript integer folds can remain `Unknown` under their
  strict IEEE profiles. That is a deliberate semantic boundary, not a parser
  failure.
- Native-stack use stays constant with graph or expression depth; finite heap
  work still requires explicit limits.

## Verification

The [formal plan](../verification/t4-3-semantic-expression-flow-formal-plan.md)
defines the pre-implementation TLA+, Rocq, and Z3 contract. Example, exhaustive
small-graph, property, parser-backed, immutability, and 100,000-node
small-stack evidence is cataloged in
[Scientific validation 71](../scientific/71-semantic-expression-flow-validation.md).

## References

1. Kildall, G. A. (1973). *A Unified Approach to Global Program Optimization.*
   DOI: [10.1145/512927.512945](https://doi.org/10.1145/512927.512945).
2. Morel, E., Renvoise, C. (1979). *Global Optimization by Suppression of
   Partial Redundancies.* DOI:
   [10.1145/359060.359069](https://doi.org/10.1145/359060.359069).
3. Tate, R., Stepp, M., Tatlock, Z., Lerner, S. (2009). *Equality Saturation: A
   New Approach to Optimization Phases.* DOI:
   [10.1145/1480881.1480915](https://doi.org/10.1145/1480881.1480915).
