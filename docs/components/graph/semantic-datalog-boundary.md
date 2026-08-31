# Semantic Datalog evidence boundary

This component copies libcpg's proof-qualified semantic expression report into
canonical owned Datalog extensional-database (EDB) relations. It is a one-way
adapter, not another reasoning engine.

![The semantic bridge validates proofs and stable identities before producing owned relations and explicit completion.](../../diagrams/semantic-datalog-responsibility-boundary.svg)

*Figure -- Replete, libcpg, and caller-owned Datalog responsibilities. Source:
[`semantic-datalog-responsibility-boundary.puml`](../../diagrams/semantic-datalog-responsibility-boundary.puml).*

## Inputs and outputs

The immutable input is `SemanticExpressionFlowReport`: exact replayable
expression proofs, deterministic semantic classes, must-availability,
must-anticipability, advisory common-subexpression elimination (CSE) and
partial-redundancy elimination (PRE) candidates, and typed completion.

`SemanticDatalogFacts` owns seven canonical relations:

| Relation | Arity | Meaning |
|---|---:|---|
| `semantic_class(function, representative, occurrence)` | 3 | Stable source membership; self-membership is reflexive identity. |
| `semantic_proof(domain, left, right, proof_pair)` | 4 | Replayed exact proof edge and source-report index. |
| `semantic_available(function, node, class)` | 3 | Class available immediately before a CFG point. |
| `semantic_anticipable(function, node, class)` | 3 | Class very busy immediately after a CFG point. |
| `semantic_cse_candidate(function, class, producer, use, proof_pair)` | 5 | Exact-proof and flow-qualified reuse candidate. |
| `semantic_pre_candidate(function, class, placement, occurrence)` | 4 | Occurrence governed by a proof-spanned PRE placement. |
| `semantic_completion(function, dimension, completion)` | 3 | Completion for equality, both flow directions, CSE, and PRE. |

Identifiers are stable libcpg `NodeId` values except `proof_pair`, a checked
address into the still-live source report. The owned tuple is not a proof: a
consumer needing proof authority must retain the report and replay that proof.

## Validation pipeline

1. Validate nonzero limits beneath public hard ceilings.
2. Require classes, members, point facts, and PRE members to be strictly
   ordered and duplicate-free.
3. Require one class per occurrence and the smallest occurrence as its
   representative.
4. Replay applicable exact proofs and build an explicit heap adjacency map.
5. Iteratively require those edges to connect each nontrivial class.
6. Match CSE proof indices to their exact producer/use endpoints.
7. Require PRE proof indices to remain inside and span the whole class.
8. Copy stable rows and five completion dimensions into owned ordered sets.

Malformed proof, index, class, point, or size evidence returns
`SemanticDatalogError`; no partial bundle escapes.

## Completion rule

Semantic completion preserves every precise upstream limiting cause.
`DatalogEvaluationCompletion` separately represents the caller's fixed-point
outcome. For semantic dimension $`d`$:

```math
\operatorname{CanConcludeAbsence}(d)
=
\operatorname{Complete}_{\mathrm{semantic}}(d)
\land
\operatorname{Complete}_{\mathrm{Datalog}}.
```

Positive rows remain valid when later work is limited.

## Ownership and dependency rules

- `analysis::expression_equivalence` may use Replete but not Datalog types.
- `analysis::datalog` may inspect the CPG but not Replete or semantic proofs.
- `analysis::semantic_datalog` may inspect public semantic reports but not
  Replete internals or the CPG.
- Replete depends on neither libcpg nor a Datalog runtime.
- Callers own fact-file I/O, Datalog rules, evaluation, provenance, and
  process limits.

`scripts/verify-reasoning-boundaries.sh` enforces these Cargo-tree, source, and
shared-mutable-state constraints.

## Resource and stack model

Defaults are 8,388,608 rows and 268,435,456 rendered bytes. Hard ceilings are
67,108,864 rows and 1,073,741,824 bytes. Checked arithmetic rejects overflow;
rendering calculates encoded length before allocating returned file buffers.

Class connectivity uses an explicit heap `Vec` worklist. Other passes are flat
loops over vectors and ordered maps/sets, so native stack depth does not grow
with report size. External memory, swap, CPU, and time limits remain necessary
for untrusted large reports.

See the [API reference](../../api/semantic-datalog-reference.md),
[Usage 45](../../usage/45-semantic-datalog-export.md),
[ADR-0077](../../design/0077-owned-semantic-datalog-evidence.md), and
[Security 35](../../security/35-semantic-datalog-boundary.md).
