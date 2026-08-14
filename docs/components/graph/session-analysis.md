# Component — Advisory session-protocol analysis

## Responsibility

`analysis::concurrency::session` turns resolved Rholang channel events into
ordered per-process endpoint sketches and checks ordinary binary linear duality.
It owns no parser, graph mutation, runtime scheduler, global protocol, or
deadlock-freedom proof.

![Session analysis component flow](../../diagrams/session-duality.svg)

## Feature boundary

The module and every public type below require `feature = "rholang"`. The
analyzer still reads only language-neutral `MsgKind`, AST, type, def-use, and
reference evidence. The gate reflects which current frontend supplies the
required behavioral metadata, not permission to inspect source text.

## Public surface

| API/type | Contract |
|---|---|
| `session_sketches(&CodePropertyGraph)` | sorted immutable sketch for every resolved channel with events |
| `SessionSketch` | channel, ordered endpoints, verdict, and issues |
| `SessionEndpoint` | top-level process plus ordered actions |
| `SessionAction` | direction, exact `MsgKind`, shallow payload vector, site, and range |
| `SessionPayloadType` | literal category, explicit named type, or `Unknown` |
| `SessionConformance` | `Conformant`, `Nonconformant`, or `Inconclusive` |
| `SessionIssue` / `SessionIssueKind` | stable evidence for mismatch or uncertainty |

## Internal flow

1. Reuse ordered `extract_events` output.
2. Retain resolved send and receive events; exclude channel-less operations.
3. Exclude the exact channel identifier while scanning each event-local AST.
4. Infer shallow payload types in source order with nested-event/function and
   cycle boundaries.
5. Group by channel and process in ordered maps; sort actions by site id.
6. Compare exactly two endpoint vectors position by position.
7. Let a proven mismatch dominate uncertainty; otherwise return the strongest
   supported three-valued verdict.

## Invariants

1. Channel identity is a resolved `Variable` node, never a spelling.
2. Every process and site id in output exists in the input CPG.
3. Output channel, process, action, payload, and issue order is deterministic.
4. `Conformant` implies exactly two equal-length linear endpoint vectors with
   opposite directions and equal known payload types.
5. `Nonconformant` implies at least one concrete mismatch issue.
6. `Inconclusive` never hides a concrete mismatch.
7. Nested event and function syntax cannot become an outer event's payload.
8. Node and edge counts are unchanged.

## Evidence and uncertainty

Primitive `TypeInfo` names are normalized to the same categories as literals;
custom type names remain explicit. Untyped parameters, untyped identifiers, and
compound expressions become `Unknown`. A non-binary endpoint count produces
one `EndpointCardinality` issue with all observed process and site evidence.
Persistent and peeking receives produce `NonLinearReceive` because their
matching cardinality differs from ordinary consuming receive.

## Tests

Ten focused unit/property tests cover exact and mismatched traces, unknown and
non-linear evidence, cardinality, payload categories, definition-backed types,
malformed AST termination, immutability, insertion-order determinism, and serde
when enabled. Two integration tests exercise the crate-root API, a typed binary
oracle, and the real Rholang frontend.
