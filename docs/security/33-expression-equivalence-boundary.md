# Security boundary -- Expression-equivalence evidence

## Security claim

libcpg reports a positive expression-equivalence pair only when both operands
come from live, complete, pure, profile-matched typed expression IR, Replete
places them in one e-class, provenance is complete, and an independent
stack-safe replay accepts every proof step. The report does not establish
whole-program observational equivalence, frontend correctness, rewrite
profitability, or authorization to modify another repository.

## Trust boundaries

| Input or artifact | Treatment |
|---|---|
| immutable CPG identity and topology | live structural evidence |
| `ExpressionIr` | admitted only with live provenance and complete lowering |
| semantic profile and exact type | mandatory policy namespace |
| binding context | mandatory namespace for exact equivalence |
| alpha-renaming bijection | explicit qualification, never implicit identity |
| Replete e-class equality | candidate evidence, not sufficient by itself |
| Replete provenance tape | untrusted until complete and independently replayed |
| serialized report bytes | audit evidence only; never live rewrite authority |
| Datalog fact, clone class, or approximate match | independent advisory evidence; never equality by conversion |
| MeTTaIL source or worktree | outside this campaign's write authority |

The decisive path is illustrated below.

![Live typed evidence passes through bounded saturation and independent proof replay before a positive pair can be emitted.](../diagrams/expression-equivalence-pipeline.svg)

*Figure -- expression-equivalence evidence boundary. Source:
[`expression-equivalence-pipeline.puml`](../diagrams/expression-equivalence-pipeline.puml).*

## Hazards and controls

### Cross-scope equality

Two identically spelled variables can denote different definitions. Exact
operators therefore include resolved binding identity and the caller-supplied
binding-context namespace. Alpha equivalence uses a separate graph and reports
the total bijection that qualified the pair. A non-bijective mapping rejects
the alpha claim.

### Unsound algebra

Rules are compiled only from the explicit semantic profile and exact result
type. Strict floating-point semantics, effects, unknown types, opaque
operations, unresolved calls, mutable loads, and incomplete lowering prevent
admission or rule use. Associative-commutative (AC) replay compares sorted leaf
multisets, preserving multiplicity; it cannot confuse `a + a + b` with
`a + b + b`.

### Proof confusion

E-class membership is necessary but not sufficient. Structural proof nodes are
checked against their endpoints. Each atomic law is validated by libcpg rather
than trusted by label. Constant-fold replay evaluates the entire closed source
term with explicit `Enter` and `Exit` frames and requires an exactly typed
literal target. Unknown labels, malformed arity, open terms, mixed namespaces,
and exhausted proof limits reject the pair.

### Collision and serialization hazards

Canonical operator bytes encode discriminants, namespaces, identities,
literals, and complete type descriptors. Equality is not inferred from an
unverified hash collision. Stable serialization is useful for comparison and
audit, but deserializing bytes does not recreate live CPG provenance or grant
rewrite authority.

### Incomplete negative results

Candidate, input, e-node, work, match, iteration, pair, and proof limits have
typed completion states. A limited pass means “not completely explored,” not
“not equivalent.” Consumers must retain both exact and alpha completion values
with any emitted prefix.

### Denial of service

All traversals and replay machines are heap-backed, so expression depth does
not consume native stack depth. Heap safety still requires finite caller caps.
For untrusted workloads, combine `ExpressionEquivalenceLimits` with process
resident-memory, no-swap, task, and time limits.

## Operational checklist

1. Lower against the current immutable CPG and an explicit semantic profile.
2. Reject incomplete, identity-only, effectful, profile-mismatched, or stale IR.
3. Supply the exact binding-context namespace for each candidate.
4. Configure all typed resource caps for the deployment.
5. Inspect exact and alpha completion independently.
6. Require a replayed proof for every positive pair.
7. Preserve alpha-bijection qualification rather than collapsing it into exact
   equality.
8. Treat serialized reports, approximate matches, Datalog facts, and clone
   groups as non-authoritative inputs to separate decisions.
9. Revalidate after any frontend, CPG, semantic-profile, Replete-rule, or type
   boundary changes.
10. Keep standalone extraction work independent; do not interpret this API as
    permission to change MeTTaIL.

See [ADR-0075](../design/0075-proof-producing-expression-equivalence.md), the
[component guide](../components/graph/expression-equivalence.md), and the
[formal verification plan](../verification/t4-2-expression-equivalence-formal-plan.md).

## Reference

Nelson, G., Oppen, D. C. (1980). *Fast Decision Procedures Based on Congruence
Closure.* DOI:
[10.1145/322186.322198](https://doi.org/10.1145/322186.322198).
