# Security boundary for session-protocol sketches

Protocol mismatches can cause lost messages, indefinite waits, or inconsistent
state, but `session_sketches` is an advisory structural check. A conformant
sketch is not proof of runtime progress or security; a nonconformant sketch is
evidence for review, not a vulnerability classification.

![Session evidence, duality decision, and explicit uncertainty](../diagrams/session-duality.svg)

## 1. Assets and actors

| Actor or asset | Security role |
|---|---|
| Rholang frontend | normalizes message kinds and builds AST/def-use evidence |
| analyzed repository | untrusted input that may contain malformed, large, or adversarial syntax |
| session analyzer | reads CPG evidence and emits immutable advisory sketches |
| downstream policy | decides whether mismatch or uncertainty blocks deployment |
| reviewer | confirms runtime endpoints, reductions, types, and operational impact |

## 2. Threats and controls

| Threat | Control | Residual risk |
|---|---|---|
| source text mimics send/receive syntax in comments or strings | analyzer never parses source; it requires `MsgKind` | faulty frontend metadata can still misclassify an event |
| shadowed channel names are conflated | channel key is a resolved definition node | missing def-use evidence omits an event from sketches |
| channel operand is counted as payload | exact resolved operand identifier is excluded | malformed frontend ordering can resolve the wrong identifier upstream |
| nested event payload leaks into its parent | nested message and function nodes are traversal boundaries | unusual frontend flattening may lose a boundary before analysis |
| AST cycle causes nontermination | each event-local walk has a visited set | a very large acyclic subtree still costs linear work |
| unknown payload is guessed compatible | `UnknownPayloadType` forces `Inconclusive` absent a harder mismatch | downstream tools may ignore the qualifier |
| persistent or peeking receive is treated linearly | `NonLinearReceive` forces `Inconclusive` absent a harder mismatch | richer service protocols are not checked |
| shared/multiparty channel is forced into a binary model | endpoint counts other than two are inconclusive | dynamic instances inside two process definitions remain conflated |
| conformant sketch is called deadlock-free | API and docs explicitly separate duality from progress/deadlock freedom | consumers can overstate advisory results |
| derived evidence becomes stale graph topology | results are out of graph and read-only | callers may cache across graph revisions |

## 3. Safe consumption

1. Preserve `Conformant`, `Nonconformant`, and `Inconclusive` as distinct policy
   states; never treat inconclusive as conformant.
2. Display issue kinds, process ids, event sites, and source ranges beside any
   protocol finding.
3. Recompute sketches after changing the CPG or frontend configuration.
4. Confirm that the two process definitions represent the intended runtime
   endpoint instances.
5. Combine with channel topology, communication-deadlock, reachability, and
   application authorization evidence before assigning security impact.
6. Use a dedicated session type checker or model checker for recursive,
   priority, progress, or deadlock-freedom claims.

## 4. False positives and false negatives

Stable event-site order can differ from runtime interleavings, so a structural
mismatch may not be exercised. Conversely, unresolved channels and missing
types suppress checks or make them inconclusive. Dynamic fresh-name instances,
delegation, recursion, branching, and multiparty behavior are outside the
sketch and can hide a real incompatibility.

## 5. Non-goals

The analyzer does not authenticate peers, enforce capabilities, inspect message
values, infer global protocols, prove subject reduction, prove progress, prove
deadlock freedom, or assign a Common Weakness Enumeration (CWE) identifier.

## References

1. Caires, L., Pfenning, F. (2010). *Session Types as Intuitionistic Linear
   Propositions.* DOI:
   [10.1007/978-3-642-15375-4_16](https://doi.org/10.1007/978-3-642-15375-4_16).
2. Jaramillo, J. C., Pérez, J. A. (2025). *Contrasting Deadlock-Free Session
   Processes.* DOI:
   [10.4230/LIPIcs.ECOOP.2025.17](https://doi.org/10.4230/LIPIcs.ECOOP.2025.17).
