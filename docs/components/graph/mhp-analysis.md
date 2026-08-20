# Component — May-happen-in-parallel analysis

## Responsibility

`analysis::concurrency::mhp` transforms typed parallel-composition AST regions
into one immutable, bounded relation between execution sites in different
operands. It owns no parser, scheduler, memory-conflict model, or graph
mutation.

![MHP component flow](../../diagrams/mhp-par-operands.svg)

## Public surface

| API/type | Contract |
|---|---|
| `may_happen_in_parallel(&CodePropertyGraph)` | deterministic feature-free MHP relation |
| `MhpPair` | one canonical unordered pair with `first < second` |
| `MhpRelation` | sorted deduplicated pairs plus explicit `capped` state |
| `MhpRelation::contains` | symmetric membership; self-pairs are false |
| `MhpRelation::partners` | stable iterator over all paired sites |
| `MAX_MHP_PAIRS` | one-million-pair materialization ceiling |

## Internal flow

1. Select `Block` nodes carrying `MsgKind::Spawn` in stable `NodeId` order.
2. Treat each direct AST child as one operand root.
3. Walk each operand with a cycle guard, stopping at nested functions.
4. Retain statement nodes and the explicit execution-bearing node kinds.
5. Form cross-products only between distinct operands.
6. Canonicalize into a `BTreeSet`; stop visibly at the pair cap.

## Invariants

1. Every pair satisfies `first < second` and both ids exist in the input CPG.
2. Logical membership is symmetric and never reflexive.
3. Sites from one operand are not related by that parallel block.
4. Sequential blocks contribute no relation.
5. Nested function bodies do not leak into an enclosing operand.
6. Node/edge insertion order cannot change the result.
7. A capped relation contains only valid pairs but may omit later pairs.
8. Analysis never changes graph nodes or edges.

## Dependencies

| Dependency | Purpose |
|---|---|
| `CpgNodeKind::Block` | structural parallel-region boundary |
| `MsgKind::Spawn` | language-neutral frontend semantic marker |
| AST overlay | direct operands and subtree membership |
| `CpgNode::is_statement` plus explicit node kinds | execution-site vocabulary |
| ordered sets | canonical deduplication and deterministic output |

No CFG, DFG, SCC, source parser, or grammar dependency is required.

## Failure and uncertainty behavior

Absent typed metadata or AST evidence produces no invented pair. Malformed AST
cycles terminate through visited sets. Nested or shared malformed subtrees may
be revisited for another parallel region, but output remains deduplicated.
Reaching `MAX_MHP_PAIRS` returns immediately with `capped = true` rather than
allocating an unbounded quadratic relation.

## Tests

Unit examples cover positive/negative operand semantics, direct roots, nesting,
function boundaries, cap behavior, query helpers, and immutability. A 128-case
property checks an independent Cartesian-product oracle plus insertion-order
determinism. Public integration covers the feature-free crate root and a real
Rholang Mode-B parse tree; serde coverage round-trips the public relation.

MHP remains a reusable substrate. The current downstream consumers are the
[race and atomicity candidate detectors](concurrency-bug-detection.md), which
add exact access identity and lock evidence without changing this relation.
