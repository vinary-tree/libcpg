# Security boundary for may-happen-in-parallel analysis

Concurrency defects can threaten integrity and availability, but a
may-happen-in-parallel (MHP) pair is only scheduling evidence. It is neither a
data race nor proof that both sites are reachable together. This page defines
the trust boundary for `may_happen_in_parallel` and the safe use of capped
relations.

![Typed parallel evidence and bounded canonical output](../diagrams/mhp-par-operands.svg)

## 1. Assets and actors

| Actor or asset | Security role |
|---|---|
| language frontend / semantic adapter | supplies trustworthy `MsgKind::Spawn` metadata and AST operand edges |
| analyzed repository | untrusted input that can maximize nesting, sites, and cross-products |
| MHP analyzer | reads typed structure and emits immutable advisory pairs |
| downstream race analysis | intersects MHP with resolved conflicting memory accesses and synchronization |
| reviewer | confirms reachability, scheduler semantics, and impact |

## 2. Threats and controls

| Threat | Control | Residual risk |
|---|---|---|
| attacker embeds `par` text in comments or names | analyzer never parses source; only typed `Spawn` metadata is accepted | a faulty frontend can still mis-annotate a block |
| parser uses direct roots instead of wrapper blocks | every direct AST child is an operand root | malformed AST grouping can still distort operands |
| quadratic pair explosion exhausts memory | `MAX_MHP_PAIRS` stops materialization and sets `capped` | traversal before the cap still consumes work; capped output is incomplete |
| AST cycle causes nontermination | each operand traversal has a visited set | overlapping malformed subtrees can produce conservative duplicate work |
| nested declaration body is treated as immediately concurrent | traversal stops at `Function` | dynamic callbacks require a richer call-aware model |
| advisory pair is labeled a vulnerability | API/docs distinguish MHP from conflicting access and race evidence | downstream tools can still overstate the result |
| absent pair is treated as proof of sequentiality | `capped` explicitly exposes truncation | missing upstream metadata is not mechanically distinguishable from true absence |
| stale serialized data invents parallelism | relation is derived out of graph and should be recomputed | callers may cache results across graph revisions |

## 3. Safe consumption

1. Reject or visibly qualify `relation.capped == true` when completeness matters.
2. Verify that each site still exists in the same CPG revision.
3. Intersect MHP with two accesses to the same resolved storage identity.
4. Require at least one write and account for happens-before/synchronization
   evidence before reporting a race candidate.
5. Confirm CFG reachability, guards, callbacks, and runtime scheduler behavior.
6. Preserve the words *may* and *candidate* in security output.

## 4. False negatives and false positives

Missing `Spawn` annotations, missing AST children, dynamic process creation,
nested callback execution, and the pair cap can suppress valid MHP evidence.
Unreachable branches, mutually exclusive guards, or runtime serialization can
leave structurally related sites unable to overlap. Therefore an empty complete
relation is evidence about the represented typed AST, not proof of global race
freedom.

## 5. Non-goals

The analysis does not resolve memory locations, infer conflicting accesses,
model mutexes or happens-before order, enumerate schedules, prove race freedom,
or assign a Common Weakness Enumeration (CWE) identifier. It supplies one
bounded input to such analyses.

## References

1. Naumovich, G., Avrunin, G. S. (1998). *A Conservative Data Flow Algorithm
   for Detecting All Pairs of Statements That May Happen in Parallel.* DOI:
   [10.1145/288195.288213](https://doi.org/10.1145/288195.288213).
