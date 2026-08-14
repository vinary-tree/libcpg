# ADR-0039: Infer advisory session sketches from typed CPG evidence

- **Status:** Accepted
- **Date:** 2026-08-14
- **Scope:** `analysis::concurrency::session` under `feature = "rholang"`

## Context

The concurrency layer already exposes normalized message operations and
definition-resolved channel identities. The epic requires a behavioral-type-
inspired check that can flag incompatible endpoint sequences without claiming a
complete Rholang type system. The result must remain deterministic, auditable,
read-only, and honest when evidence is absent or the topology is not binary and
linear.

## Decision

1. Compile `session.rs` and its public API only with the `rholang` feature.
2. Reuse `extract_events` and the exact resolved channel-operand identifier;
   never parse source or compare channel names.
3. Group resolved send/receive actions first by `ChannelId`, then by top-level
   process, and order actions by stable event-site `NodeId`.
4. Infer shallow payload vectors from event-local literals, explicit types, and
   typed definitions. Preserve compound or absent evidence as `Unknown`.
5. Check ordinary binary linear duality only for exactly two processes, equal
   trace lengths, opposite directions, equal payload arities, and equal known
   payload types.
6. Return `Nonconformant` for proven mismatches, `Inconclusive` for unknown
   types, persistent/peeking receives, or non-binary participation, and
   `Conformant` only for an exact known dual. Proven mismatch dominates
   uncertainty.
7. Retain exact `MsgKind`, process ids, event sites, source ranges, and stable
   issue kinds so every verdict is auditable.
8. Return immutable out-of-graph structures and derive serialization only when
   the existing `serde` feature is also enabled.

## Rejected alternatives

### Hand-roll a Rholang parser in the analyzer

Rejected. The tree-sitter frontend already supplies structured nodes and typed
message metadata. A second parser would disagree on recovery, comments,
quoting, and grammar evolution.

### Pair channels by textual spelling

Rejected. Names can be shadowed and reused. Resolved definition identity is the
only accepted channel key.

### Treat every channel as binary

Rejected. One or more than two participating processes may denote local,
shared, or multiparty behavior. Binary duality cannot classify those safely, so
the result is explicitly inconclusive.

### Treat persistent receive and peek as ordinary receive

Rejected. One remains installed and the other does not consume. Both violate
the linear one-action correspondence assumed by the checker.

### Guess types from source text or identifier names

Rejected. Untyped and compound evidence becomes `Unknown`; absence of evidence
is not evidence of a type.

### Materialize protocol edges in the CPG

Rejected. Sketches are derived advisory findings. Keeping them out of graph
prevents mutation and avoids presenting an incomplete behavioral abstraction as
source topology.

## Consequences

### Positive

- Results are deterministic and contain only input-backed evidence.
- A three-valued verdict separates proven incompatibility from uncertainty.
- The public structures are suitable for CLI, JSON, and review tooling.
- The analyzer remains independent of source syntax despite its Rholang gate.

### Negative

- Top-level process grouping cannot distinguish dynamic endpoint instances.
- Shallow types do not model subtyping, dependent types, recursion, priorities,
  polymorphism, or global/multiparty protocols.
- Event-site order is a stable structural sketch, not an execution trace under
  all Rholang reductions.
- Missing frontend typing commonly yields an inconclusive result.

## Verification

Example tests cover exact duals, direction/order, trace length, payload arity,
known type, unknown type, non-linear receive, endpoint cardinality, all literal
categories, typed identifier definitions, AST cycles, nested event boundaries,
immutability, and serialization. A 128-case generated property constructs exact
duals and reverses node/edge insertion order. Public integration includes a
typed two-process oracle and a real Rholang Mode-B tree.

## References

1. Honda, K., Vasconcelos, V. T., Kubo, M. (1998). *Language Primitives and
   Type Discipline for Structured Communication-Based Programming.* DOI:
   [10.1007/BFb0053567](https://doi.org/10.1007/BFb0053567).
2. Caires, L., Pfenning, F. (2010). *Session Types as Intuitionistic Linear
   Propositions.* DOI:
   [10.1007/978-3-642-15375-4_16](https://doi.org/10.1007/978-3-642-15375-4_16).
