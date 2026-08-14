# ADR-0038: Derive MHP from typed parallel operand subtrees

- **Status:** Accepted
- **Date:** 2026-08-14
- **Scope:** feature-free `analysis::concurrency::mhp`

## Context

Race-oriented analyses need a deterministic may-happen-in-parallel (MHP)
relation, but libcpg must remain language-neutral and feature-free. ADR-0036
already established `MsgKind::Spawn` as typed parallel-composition metadata.
The implementation therefore needs to convert that evidence into statement
pairs without parsing source, importing a grammar, or mutating the CPG.

A real Rholang Mode-B oracle exposed an important frontend variation: a `par`
block can own direct process roots such as `Call` nodes, while synthetic and
other frontend fixtures may insert wrapper `Block` nodes. Operand discovery
must honor the typed AST boundary rather than require either shape.

## Decision

1. Recognize a parallel region only when a `Block` has
   `message_kind == Some(MsgKind::Spawn)`.
2. Treat every direct AST child of that block as one operand root, whether the
   child is a process node or a wrapper block.
3. Collect statement nodes plus `Variable`, `Assignment`, `Call`, `Await`, and
   `Yield` as execution sites; exclude identifiers, literals, and structural
   nodes.
4. Stop operand traversal at nested `Function` nodes so independently invoked
   definitions do not inherit the enclosing execution context.
5. Relate execution sites only across distinct direct operands. Compose nested
   parallel regions by taking the union of their independently derived pairs.
6. Store each symmetric pair once in ascending `NodeId` order, globally sort
   and deduplicate the relation, and provide order-insensitive membership.
7. Bound output with `MAX_MHP_PAIRS`; set `capped` whenever later valid pairs
   are omitted.
8. Return the relation out of graph and derive serde support only under the
   existing `serde` feature.

## Rejected alternatives

### Parse `|`, `par`, or other source spellings

Rejected. Syntax is language-specific, comments and strings are ambiguous, and
Mode B already supplies typed tree-sitter structure. The analysis consumes the
normalized semantic field established by ADR-0036.

### Require operand wrapper blocks

Rejected. The real Rholang parser produces direct send-call children for the
accepted parallel fixture. Wrapper-only logic silently returned an empty
relation and was caught by the integration oracle.

### Relate every AST node

Rejected. Identifiers, literals, and structural blocks are not separately
scheduled execution sites. Including them would inflate a quadratic relation
with evidence that downstream race analyses cannot use.

### Traverse nested function bodies

Rejected. A nested declaration does not execute its body at declaration time.
Crossing that boundary would assert parallelism for sites requiring a separate
call.

### Store both directed orientations

Rejected. MHP is symmetric. Canonical unordered storage halves redundant
materialization and gives deterministic equality, while `contains` preserves
the natural symmetric query.

### Materialize MHP edges in the CPG

Rejected. The relation is advisory, may be capped, and depends on normalized
concurrency evidence. Keeping it immutable and out of graph avoids stale or
authoritative-looking overlay edges.

## Consequences

### Positive

- The feature-free analysis is independent of every grammar crate.
- Direct and wrapped operand shapes have identical semantics.
- Canonical ordering supports reproducible serialization and testing.
- A hard, visible bound protects callers from unbounded quadratic output.
- The result composes naturally with future conflicting-access analysis.

### Negative

- Missing or incorrect frontend `Spawn` metadata directly affects soundness.
- Structural MHP can include unreachable or guard-incompatible sites.
- A capped relation is incomplete and downstream consumers must check it.
- The chosen execution-site vocabulary must evolve when the graph model gains
  a new execution-bearing node kind.

## Verification

Example tests cover different operands, same-operand and sequential negatives,
direct process roots, nested parallel composition, nested-function exclusion,
cap boundaries, symmetric queries, immutability, and serde. A 128-case
generated property compares the result to an independent Cartesian-product
oracle while reversing node and edge insertion order. Public integration tests
exercise both a feature-free hand-built graph and a real Rholang Mode-B tree.

## References

1. Naumovich, G., Avrunin, G. S. (1998). *A Conservative Data Flow Algorithm
   for Detecting All Pairs of Statements That May Happen in Parallel.* DOI:
   [10.1145/288195.288213](https://doi.org/10.1145/288195.288213).
