# Rholang NameFlow and level analysis

`analysis::concurrency::name_flow`, available with feature `rholang`, infers
fresh-name provenance, optionally materializes a typed NameFlow overlay, and
reports cyclic obligation/capability constraints. It consumes normalized CPG
metadata and never parses Rholang source.

![The component turns normalized fresh-name and message evidence into qualified immutable reports or a validated optional overlay.](../../diagrams/name-flow-overlay-boundary.svg)

## Public surface

| API or type | Contract |
|---|---|
| `analyze_name_flow` | qualified origin fixed point with default caps |
| `analyze_name_flow_with_caps` | same computation with caller-selected bounds |
| `materialize_name_flow_overlay` | transactional, idempotent append over existing nodes |
| `analyze_deadlock_freedom` | qualified NameFlow, canonical levels, cyclic channels, and witnesses |
| `analyze_deadlock_freedom_with_caps` | same computation with caller-selected bounds |
| `deadlock_freedom` | convenience projection to cyclic witnesses; empty is not proof |
| `NameFlowAnalysis` | origins, typed relations, match count, work count, issues, completeness |
| `DeadlockFreedomAnalysis` | NameFlow report, levels, cyclic channels, witnesses, issues |
| `NameFlowCaps` | origin, relation, communication, dependency, and aggregate-work bounds |
| `NameFlowOverlayError` | unknown endpoint detected before any graph mutation |

## Evidence contract

| Meaning | Required normalized evidence |
|---|---|
| fresh identity | `name_operation == FreshBinder` beneath a `Restriction` |
| direct reference | unique identifier definition, reaching definition, Reference, or typed DFG evidence |
| quote/drop | `name_operation == Quote` or `Drop` on an AST ancestor |
| channel use | `MsgKind` event and exact channel operand definition |
| carried name | send payload with a resolved fresh origin |
| scope extrusion | matched send/receive channel origin and positional payload/binder pair |
| process order | source-ordered events under one represented process root |

Conflicting definition evidence, a binder without a restriction, an unresolved
channel, mismatched payload arity, or an exhausted cap is retained as a typed
issue. No issue is silently converted to “no flow” or “deadlock-free.”

## Overlay relations

`Creates`, `References`, `Quotes`, `Drops`, `Carries`, `Receives`, and
`Communicates` all originate at the stable fresh binder. Query materialized
edges with `name_flow_edges`, `name_flow_successors`,
`name_flow_predecessors`, or `CpgEdgeKind::is_name_flow`. Reports stay outside
the graph until the caller opts into materialization.

## Determinism and stack safety

- `NodeId`, source-range, relation, issue, channel, and witness order is
  canonical;
- the least fixed point uses a duplicate-suppressing heap queue;
- payload watchers and transfers wake only affected events;
- channel cycles use the shared iterative SCC decomposition;
- acyclic levels use an explicit topological worklist; and
- every AST ancestor walk and report/drop path is iterative.

The native call depth is constant with respect to AST depth, payload-chain
length, number of processes, and SCC depth. Caps bound retained heap state and
work but do not replace process-level resident-memory limits.

## Related documentation

- [Theory](../../theory/52-rholang-name-flow-and-deadlock-freedom.md)
- [ADR-0069](../../design/0069-stack-safe-rholang-name-flow.md)
- [Usage](../../usage/37-rholang-name-flow-deadlock.md)
- [Security boundary](../../security/27-rholang-name-flow-boundary.md)
- [Validation](../../scientific/64-rholang-name-flow-validation.md)
