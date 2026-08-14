# Security boundary for deadlock-candidate analysis

Deadlocks are availability and denial-of-service concerns, but a static cyclic
SCC is evidence, not an incident verdict. This page defines the trust boundary
for `communication_deadlocks` and the safe way to consume its witnesses.

![Process wait-for evidence and conservative edge suppression](../diagrams/wait-for-deadlock.svg)

## 1. Assets and actors

| Actor or asset | Security role |
|---|---|
| language frontend / semantic adapter | supplies `MsgKind` and `LockKind` evidence |
| DFG/call resolver | supplies resource identities and resolved callees |
| analyzed repository | untrusted, possibly malformed input |
| deadlock analyzer | constructs immutable advisory projections |
| SCC engine | classifies cyclic components deterministically |
| reviewer or downstream tool | confirms reachability, external actors, and operational policy |

## 2. Threats and controls

| Threat | Control | Residual risk |
|---|---|---|
| attacker chooses misleading resource names | identity requires a resolved `Variable` `NodeId` | incorrect upstream links can still misidentify |
| analyzer guesses common lock method names | typed `LockKind` only; no source/name allowlist | absent adapter annotations cause false negatives |
| an unblocked producer is ignored | a wait edge requires every known producer to be initially blocked | unknown external producers remain invisible |
| shared reads are called deadlock | all-read lock SCCs are suppressed | non-standard lock implementations may differ |
| unknown release leaves a phantom held lock | unresolved release clears held state | clearing can hide a later real order edge |
| recursive calls cause nontermination | summaries are monotone over a finite resource universe | large call/resource products can still cost time |
| malformed AST cycles recurse forever | scope/resource traversals use visited sets | the existing DFG AST walk has its own guarded traversal contract |
| candidate is presented as proven exploitability | types, details, and docs consistently say advisory candidate | downstream wording can still overstate it |
| stale serialized graph invents semantics | absent `lock_kind` defaults to `None` | stale data suppresses findings until rebuilt |

## 3. False-negative conditions

Missing message/lock metadata, unresolved identifiers, missing call targets,
external producers, runtime-created resources, reflection, callbacks not
represented by the call graph, condition-variable protocols, and unresolved
release operations can suppress a finding. A clean result is meaningful only
relative to evidence completeness.

## 4. False-positive conditions

A cyclic SCC can contain an unreachable acquisition, mutually exclusive branch,
guarded receive, impossible pattern match, or call path that cannot execute while
the represented lock remains held. Interprocedural summaries intentionally use
may-acquire union. Review the precise sites and surrounding CFG before assigning
severity.

## 5. Safe operational workflow

1. Retain `kind`, `processes`, `resources`, and every `DeadlockWitness`.
2. Verify source ranges against the same CPG revision.
3. For communication cycles, check external producers and message guards.
4. For lock cycles, confirm lock lifetime, mode, reentrancy, and call reachability.
5. Combine with CFG/SDG slicing and runtime telemetry when available.
6. Treat reports as source-sensitive security data.

## 6. Non-goals

The analysis does not prove deadlock freedom, enumerate schedules, model fairness,
interpret lock implementation internals, infer condition-variable predicates,
or assign CWE severity. It supplies reproducible review candidates.

For the orthogonal question of which sites may execute concurrently, see the
[MHP security boundary](06-mhp-analysis.md).

## References

1. Kobayashi, N. (2006). *A New Type System for Deadlock-Free Processes.* DOI:
   [10.1007/11817949_16](https://doi.org/10.1007/11817949_16).
