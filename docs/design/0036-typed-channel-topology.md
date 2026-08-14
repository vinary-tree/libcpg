# ADR-0036: Preserve typed message events and report channel topology out of graph

- **Status:** Accepted
- **Date:** 2026-08-14
- **Scope:** feature-free `graph::MsgKind` metadata and `analysis::concurrency`

## Context

The Rholang frontend previously normalized sends and all receive forms to
`CpgNodeKind::Call`. That preserved call-like CFG/DFG behavior but erased the
difference between linear `<-`, persistent `<=`, and non-consuming `<<-`.
Recovering the distinction inside an analyzer would require reparsing source or
matching grammar-specific strings, neither of which is a sound feature-free
analysis boundary.

Channel spelling is also insufficient identity. Shadowing can give equal names
different meanings, while reference/def-use evidence can give different use
spellings one definition. The CPG already has stable `NodeId` values and typed
`Reference`/`DefUse` relations.

## Decision

1. Add the language-neutral `MsgKind` enumeration with `Send`, `SendSync`,
   `Recv`, `RecvPersistent`, `Peek`, `Select`, and `Spawn`.
2. Store optional normalized semantics in `CpgNode::message_kind`, independent
   of `CpgNodeKind`. A send remains a call-like expression, a selection remains
   `Match`, and parallel composition remains `Block`.
3. Assign message metadata only in the language frontend. The concurrency
   analyzer does not inspect grammar names, operator glyphs, or source text.
4. Define `ChannelId = NodeId`, restricted to a resolved `Variable` definition.
5. Resolve each event only inside its own AST subtree through
   `Identifier::definition`, `DefUse`/`ReachingDef`, or `Reference` evidence.
6. Retain unresolved channel-using events with `channel = None`; never fall back
   to spelling equality.
7. Use top-level `Function` nodes as process roots and exclude nested function
   bodies from the enclosing process.
8. Return a `BTreeMap<NodeId, Vec<ChannelEvent>>`, sorting both processes and
   event sites by `NodeId`.
9. Keep `channel_report` read-only and findings out of graph. Report
   `OrphanSend`, `BlockedRecv`, and `NondeterministicConsumption` with sorted
   process/site evidence.
10. Treat `SendSync` as a producer, `Peek` as a non-consuming observer, and
    `RecvPersistent` as excluded from linear starvation.
11. Count producer and linear-receiver sites, not only distinct processes, for
    contention because two sites in one process can still race to match.
12. Serialize the new metadata and report types only under the existing
    `serde` feature; add no feature or dependency.

## Rejected alternatives

### Parse source in `analysis::concurrency`

Rejected. It duplicates the parser, is fragile under whitespace/comments and
grammar evolution, and makes feature-free analysis language-specific.

### Infer receive kind from `CpgNodeKind::Call`

Rejected. The normalization is intentionally many-to-one; no graph fact
distinguishes `<-`, `<=`, and `<<-` after that loss.

### Replace `Call`, `Match`, and `Block` node kinds

Rejected. Existing CFG/DFG consumers rely on those structural kinds. Orthogonal
metadata preserves concurrency semantics without regressing established graph
roles.

### Use channel strings as identity

Rejected. Name equality is unsound under shadowing and fails to exploit the
already represented semantic links.

### Add send-to-receive edges to the CPG

Rejected for this task. Matching is an advisory, graph-wide relation with
partial evidence. Returning findings out of graph avoids turning a candidate
schedule into authoritative topology and preserves append-only graph semantics.

### Enumerate Petri-net markings

Rejected. F1/F4 need only polynomial structural tallies. Runtime reachability
and cyclic-wait reasoning are separate analyses with different completeness and
resource contracts.

## Consequences

### Positive

- The real Rholang grammar drives semantic classification once, at the frontend
  boundary.
- The feature-free core works on parsed, merged, deserialized, or hand-built
  graphs.
- Stable definition identity survives spelling reuse and avoids guessed links.
- Event and finding order is reproducible across edge insertion order and hash
  seeds.
- Existing node-kind behavior remains compatible.

### Negative

- A caller-built CPG must attach `MsgKind` metadata explicitly.
- Old serialized nodes deserialize with `message_kind = None` and therefore
  contribute no events until rebuilt or annotated.
- Incomplete DFG/reference evidence produces unresolved events and can suppress
  channel findings.
- Structural contention does not prove that competing sites are simultaneously
  reachable.

## Verification

Named examples cover blocked receives, orphan sends, matched channels,
persistent non-starvation, peek observation, two-producer contention, two-linear-
receiver contention, both `DefUse` and `Reference` identity, unresolved events,
nested functions, and read-only/out-of-graph behavior. Generated tests reverse
edge insertion and vary producer/receiver counts. Public integration tests use
the crate-root API, and a Mode-B test parses real Rholang syntax with the
upstream Tree-sitter grammar and requires all seven normalized kinds.

## References

1. Meredith, L. G., Radestock, M. (2005). *A Reflective Higher-order Calculus.*
   DOI:
   [10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016).
