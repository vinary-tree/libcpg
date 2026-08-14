# Security boundary for channel-topology findings

Channel-topology findings can identify dropped outputs, permanently producer-
less linear receives, and schedule-sensitive consumption. They are evidence for
defensive review, not deadlock, exploitability, availability, or correctness
verdicts.

![Channel extraction trust boundaries](../diagrams/channel-event-extraction.svg)

## 1. Assets and trust boundaries

| Actor or asset | Role |
|---|---|
| language frontend | translates concrete syntax to trusted `MsgKind` metadata |
| CPG builder | supplies AST scope, source ranges, and identity edges |
| analyzed repository | potentially malformed or adversarial input |
| event extractor | reads typed graph evidence and retains unresolved identities |
| channel classifier | groups events and emits immutable advisory findings |
| consumer/reviewer | combines evidence with reachability and application policy |

The frontend is the only component allowed to know grammar-specific operator
syntax. The analyzer trusts the graph representation but does not trust source
spelling as semantic identity.

## 2. Threats and controls

| Threat | Control | Residual risk |
|---|---|---|
| shadowed names are merged | channel key is the resolved `Variable` `NodeId` | imprecise upstream alias edges can still merge meanings |
| renamed uses are split | `definition`, `DefUse`, and `Reference` evidence establishes identity | missing evidence yields false negatives |
| analyzer reparses hostile source | analyzer reads typed CPG only | a buggy frontend can attach wrong metadata |
| nested process event leaks into parent | function-scoped traversal stops at nested functions | dynamic process values are not modeled |
| edge order changes findings | ordered maps/sets plus explicit site sorting | changed `NodeId` assignment legitimately changes presentation order |
| peek is treated as a consumer | `Peek` observes but never enters linear counts | pattern/guard failure can prevent observation at runtime |
| persistent receive is reported starved | only `Recv` produces `BlockedRecv` | the persistent body can still await an absent message operationally |
| finding is mistaken for a reachable race | report is documented as topology-only and advisory | downstream tools can overstate the result |
| old serialized graph silently invents kinds | missing `message_kind` deserializes to `None` | old data can suppress findings until rebuilt |

## 3. False-negative conditions

A missing finding can result from absent or incorrect frontend metadata,
unresolved definitions, unsupported aliasing or channel extrusion, events
outside top-level functions, external processes, dynamically generated code,
or stale serialized graphs. An unresolved event remains visible through
`extract_events`, but it is excluded from `channel_report` because there is no
sound channel key.

## 4. False-positive conditions

The classifier does not require event reachability or simultaneous enablement.
Two producer sites in mutually exclusive branches can therefore trigger
contention. A represented send may be dead code; an external receiver may make
an orphan finding harmless; and a guard or failed pattern can prevent a receive
from matching. These are expected consequences of the topology abstraction.

## 5. Safe handling

- Display supporting process ids, event sites, and source ranges with each
  result.
- Inspect unresolved `ChannelEvent` values before treating a clean report as
  meaningful.
- Combine `BlockedRecv` with CFG reachability and
  [`communication_deadlocks`](05-deadlock-analysis.md) cyclic-wait evidence.
- Confirm whether non-determinism is a protocol feature before escalating F4.
- Protect reports like source code: event locations and channel structure can
  reveal application architecture.
- Rebuild rather than patch serialized graphs when frontend normalization or
  def-use construction changes.

## 6. Non-goals

This topology analysis does not enumerate Petri-net markings, prove fairness, simulate
message queues, evaluate Rholang patterns/guards, infer protocol/session types,
or prove deadlock freedom. The separate F3 analysis reports structural cyclic
candidates but still does not prove runtime deadlock.

## References

1. Meredith, L. G., Radestock, M. (2005). *A Reflective Higher-order Calculus.*
   DOI:
   [10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016).
