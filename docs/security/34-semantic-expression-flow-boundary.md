# Security boundary -- Semantic expression flow

## Security claim

libcpg emits a CSE or PRE candidate only after exact expression proofs replay,
the relevant deterministic must-dataflow reaches a fixed point, resolved kills
are applied, and dominance/path premises hold. Cross-revision status is based
on a replayed proof or two exact unequal closed values, never on source ids,
structural alignment, missing evidence, or approximate similarity.

The report is advisory. It does not prove whole-program observational
equivalence, source-rewrite validity, exception-motion safety outside the
represented CFG, profitability, frontend correctness, or permission to modify
another repository.

## Trust boundaries

| Input or artifact | Treatment |
|---|---|
| immutable CPG and selected function CFG | live structural and path evidence |
| live complete pure `ExpressionIr` | eligible only through its semantic profile and provenance |
| exact Replete proof | candidate equality evidence, independently replayed |
| alpha-renamed proof | qualified clone/diff evidence, never same-execution value identity |
| `BindingIdentity` | authoritative represented definition and scope for kills |
| identifier spelling | display data; never a kill or equality key by itself |
| available/very-busy fixed point | path fact only; never an equality proof |
| dominance and no-kill search | placement premise only; never semantic equality |
| old/new numeric `NodeId` equality | meaningless across independent revision coordinates |
| exact closed evaluator result | decisive difference only when both typed values exist and differ |
| limit or unsupported reason | explicit incompleteness; never a negative proof |
| structural diff, Datalog fact, or approximate match | independent evidence; never converted into semantic status |
| MeTTaIL repository or worktree | immutable and outside this campaign's write authority |

## Hazards and controls

### Equality/path confusion

An e-class records equations, not executions. The adapter therefore keeps
equality, availability, anticipability, dominance, and kills in separate typed
structures and requires all relevant premises at the final candidate boundary.

### False producer at a control-flow join

A must-available class can have different producers on different predecessor
paths. CSE additionally requires one reported occurrence to dominate the use
and explicitly rejects an intervening represented kill.

### Shadowing and alias uncertainty

Kills use resolved definition identities. Same-spelling bindings in different
scopes do not interact. An unresolved assignment is not assumed independent;
it kills every class. Unknown calls and suspension points are conservative
barriers unless the live expression boundary supplies explicit purity.

### Cross-revision identity collision

Old and new CPGs allocate `NodeId` independently. Private comparison slots keep
their terms apart even when numeric ids match. The slots never escape and the
original ids remain labeled as old and new.

### Incomplete-result escalation

Positive evidence remains valid when unrelated later work is capped, but
absence is conclusive only for a complete report. `Limited` and `Unknown` are
distinct: limited evidence stopped early; unknown evidence completed without a
decisive witness.

### Proof/evaluator disagreement

The proof path and closed evaluator are independent checks. If a replayed
equality proof coexists with unequal exact typed values, analysis returns
`ContradictoryEvidence`; it does not choose a convenient answer.

### Denial of service and stack exhaustion

Expression, proof, dataflow, evaluation, and path searches use flat arenas or
explicit heap work queues. This prevents input depth from consuming native
call-stack depth. Heap and CPU use remain finite only under configured limits.
For untrusted workloads, combine API limits with process memory, swap, task,
and time limits.

## Operational checklist

1. Build expression facts from the current immutable CPG and explicit profile.
2. Reject stale, incomplete, effectful, unresolved, or profile-mismatched IR.
3. Keep exact and alpha proof meanings separate.
4. Preserve resolved `BindingIdentity` values and conservative barriers.
5. Inspect equality, forward, backward, and candidate-work completion.
6. Replay every referenced proof before consuming a candidate.
7. Treat CSE/PRE output as advisory input to a separately verified transformer.
8. Keep old and new revision coordinates separate even when ids match.
9. Preserve all four semantic-diff statuses and typed limitations.
10. Fail closed on contradictory evidence or malformed indices.
11. Revalidate after frontend, CFG, binding, effect, profile, Replete, or
    dataflow changes.
12. Do not edit MeTTaIL; its E3 task remains permission-gated.

See [ADR-0076](../design/0076-proof-qualified-semantic-expression-flow.md), the
[component guide](../components/graph/semantic-expression-flow.md), and the
[formal plan](../verification/t4-3-semantic-expression-flow-formal-plan.md).

## References

1. Kildall, G. A. (1973). *A Unified Approach to Global Program Optimization.*
   DOI: [10.1145/512927.512945](https://doi.org/10.1145/512927.512945).
2. Nelson, G., Oppen, D. C. (1980). *Fast Decision Procedures Based on
   Congruence Closure.* DOI:
   [10.1145/322186.322198](https://doi.org/10.1145/322186.322198).
