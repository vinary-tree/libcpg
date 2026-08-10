# 0008 — Id-preserving basic-block CFG view

## Status

**Accepted.** Implemented by `analysis::basic_blocks`; the older
`BasicBlockIdentifier` remains available with an API pointer to this view.

## Context

The CFG overlay intentionally retains AST-granular CPG nodes. Downstream
data-flow analyses nevertheless need the classic maximal straight-line blocks,
typed block-to-block edges, entry and exit blocks, and stable traversal orders.
Replacing CPG nodes with block nodes would invalidate public `NodeId` values and
disconnect the CFG from AST, DFG, and PDG evidence. The prior
`BasicBlockIdentifier` returned only an unordered leader map and could not
express those relationships.

The replacement must be feature-free, deterministic under insertion-order
permutations, stack-safe, total on supported CFG-only graphs, and read-only.

## Decision

**Construct a separate immutable `BlockCfg` whose blocks retain original
`NodeId` membership, whose dense `BlockId` values follow sorted leaders, and
whose typed edges are a lossless projection of every non-internal CFG edge.**

Leaders are the entry, non-`Sequential` targets, all successors of a
multi-successor node, and merge points. Blocks grow only across unique
`Sequential` predecessor/successor relations. Iterative sorted-successor DFS
precomputes post-order and reverse post-order. `BasicBlockIdentifier::identify`
retains its historical AST-descendant scope—including emitted unreachable CFG
fragments—but sorts its leader worklist and directs new callers to `block_cfg`.

The exact invariants, pseudocode, API, complexity, and validation oracles are in
the [basic-block component guide](../components/graph/basic-block-cfg.md).

## Consequences

### Positive

- **Overlay identity survives.** Every block member is an existing `NodeId`, so
  AST, DFG, PDG, source-range, and property lookups remain valid.
- **Deterministic analysis substrate.** Sorted leaders, dense ids, canonical
  typed edges, and stable traversal orders can feed fixed-point analyses.
- **No topology loss.** Only adjacent internal `Sequential` steps disappear;
  all other typed CFG relations become block edges.
- **No source mutation.** Multiple block views and other analyses can coexist
  without rewriting or cloning the CPG.
- **Migration path.** Existing leader-map callers retain source and historical
  unreachable-fragment compatibility.

### Negative

- **Duplicated topology metadata.** The view stores membership, typed edges,
  adjacency indexes, and traversal orders in addition to the CPG.
- **Reachability boundary.** Unreachable CFG islands are deliberately absent;
  callers needing them must use the reachability report separately.
- **Compatibility information loss.** `BasicBlockIdentifier` cannot expose
  errors, deterministic map iteration, edge kinds, exits, or orders without a
  breaking signature change.
- **Dense-id ceiling.** More than $`2^{32}`$ blocks cannot fit in `BlockId`.

## Alternatives considered

### Rewrite the CPG into block nodes

Rejected because it would destroy stable node identity and the shared overlay
model established by ADR-0001. It would also make basic-block analysis a
destructive phase-ordering constraint.

### Clone a compacted CPG

Rejected because a second `CodePropertyGraph` would blur whether ids and
properties refer to original program points, duplicate unrelated overlay
state, and still need a membership side table.

### Extend only the leader-keyed hash map

Rejected because hash-map iteration is not a deterministic graph order and the
shape cannot naturally carry typed edges, entry/exit blocks, or predecessor and
successor accessors.

### Fuse every one-successor chain

Rejected because a target with multiple predecessors is a control-flow entry,
and a non-`Sequential` edge represents a semantic boundary even when it is the
only outgoing relation.

## References

- Aho, A. V., Sethi, R., Ullman, J. D. (1986). *Compilers: Principles,
  Techniques, and Tools.* Addison-Wesley. ISBN 0-201-10088-6. This publication
  has no DOI; its ISBN is the stable bibliographic identifier.
