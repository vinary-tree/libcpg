# ADR-0037: Derive deadlock candidates from typed evidence and shared SCCs

- **Status:** Accepted
- **Date:** 2026-08-14
- **Scope:** feature-free `analysis::concurrency::deadlock` and typed lock metadata

## Context

ADR-0036 preserved message semantics and channel identity without reparsing
source. T3.7 must deepen that topology into cyclic waits and also carry the
RacerD-style lock-order idea into libcpg. A private Tarjan implementation would
duplicate a verified graph primitive. Matching names such as `lock`, `mutex`,
or channel spellings would reintroduce language/API heuristics at the analysis
boundary.

The real Rholang oracle also exposed a DFG boundary defect: a contract nested
under `new` could not see lexically preceding outer channel definitions, while
blindly traversing sibling functions would leak their locals.

## Decision

1. Add `LockKind::{AcquireRead, AcquireWrite, Release}` and `LockMode` as
   optional metadata orthogonal to `CpgNodeKind::Call`.
2. Require lock and channel resources to resolve to `Variable` `NodeId` values;
   never fall back to spelling equality.
3. Define a process as initially blocked only when its first relevant message
   operation is a resolved linear receive.
4. Add `P -> Q` only when `Q` produces the awaited channel and every known
   producer of that channel is initially blocked. Preserve self-waits.
5. Build wait-for and lock-order graphs with `GraphProjection::from_edges` and
   reuse `projection_sccs` for cyclic-component recognition.
6. Return `DeadlockFinding` values out of graph with sorted processes/resources
   and `DeadlockWitness` values carrying exact sites and source ranges.
7. Model read/write acquisition modes and suppress SCCs supported only by
   shared-read ordering.
8. Compute resolved-callee reachable-acquisition summaries to a finite monotone
   fixed point and retain caller, callee, acquisition, and call-site evidence.
9. Treat an unresolved release conservatively by clearing the held stack; this
   prevents an unknown release from creating false lock-order edges.
10. Seed each function's DFG environment from lexically preceding outer
    definitions while stopping at sibling/nested `Function` bodies.
11. Preserve deterministic `NodeId` order throughout and serialize new public
    evidence only under the existing `serde` feature.

## Rejected alternatives

### Parse call text or maintain a lock-name allowlist

Rejected. Library and user-defined APIs can use arbitrary names, while the same
name can mean unrelated behavior. Semantic adapters attach `LockKind` after API
resolution.

### Implement another cycle detector in `deadlock.rs`

Rejected. `GraphProjection` already canonicalizes nodes/edges and
`projection_sccs` already handles multi-node and self cycles deterministically.

### Add wait-for or lock-order edges to the CPG

Rejected. These are advisory may-relations derived from incomplete evidence.
Materializing them as authoritative program edges would blur the source graph
and complicate invalidation.

### Add an edge when any blocked producer exists

Rejected. An additional unblocked producer can satisfy the receive and break
the cycle. Requiring all known producers to be initially blocked reduces a
specific avoidable false positive.

### Treat persistent receive or peek as linear initial wait

Rejected. The accepted F3 abstraction is built on consuming linear progress.
Persistent service and non-consuming observation have different operational
roles and remain outside this structural wait relation.

### Seed a function by traversing all outer syntax

Rejected. Later siblings are not lexically preceding, and sibling function
bodies own separate locals. The seed follows the ancestor path and visits only
preceding siblings, with `Function` as a hard environment boundary.

## Consequences

### Positive

- Both candidate families reuse the same tested SCC substrate.
- Witnesses are stable, source-locatable, and auditable.
- Recursive call summaries converge without a depth magic number.
- Channel and lock spellings can change without changing identity.
- Real nested Rholang contracts retain their surrounding `new` bindings.

### Negative

- Callers or semantic adapters must attach lock metadata.
- Missing targets/resources suppress findings.
- May-acquire summaries can report an infeasible branch or call path.
- Read/write modes do not encode upgrades, poisoning, reentrancy, or runtime
  lock implementation details.
- Cyclic structure is not a reachability or schedule proof.

## Verification

Named tests cover mutual receive, send-first, alternative unblocked producers,
self-wait, persistent/peek exclusion, direct and interprocedural A/B lock
cycles, release behavior, recursive summary convergence, shared-read
suppression, mixed-mode reporting, serde, source-range validity, and read-only
out-of-graph behavior. Generated tests reverse graph-edge insertion and create
process rings with optional progress breakers. Public tests exercise the crate
root and a real Rholang Mode-B tree. DFG regressions cover both outer capture and
sibling-function non-leakage.

## References

1. Kobayashi, N. (2006). *A New Type System for Deadlock-Free Processes.* DOI:
   [10.1007/11817949_16](https://doi.org/10.1007/11817949_16).
