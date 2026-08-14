# ADR-0032 — Deterministic bounded IFDS/IDE procedure summaries

- **Status:** Accepted
- **Date:** 2026-08-13
- **Decision owners:** `libcpg` maintainers
- **Scope:** feature-free `analysis::ifds`

## Context

Interprocedural taint, effects, typestate, and related domains need one shared
summary scheduler. Reimplementing recursion handling inside each analysis risks
different call-resolution rules, hash-dependent output, and missing resource
boundaries. The CPG already exposes an exact deterministic call-graph SCC
decomposition, but its component vector is ordered by stable identity rather
than dependency direction. Public SCC fields also permit callers to supply a
stale or malformed value.

The intraprocedural `analysis::dataflow::Lattice` is coupled to block-CFG
transfer. Interprocedural finite-fact and environment domains need a smaller
summary algebra without requiring a block graph or a new feature dependency.

## Decision

Add a feature-free `analysis::ifds` module with a dedicated `Lattice`,
`SummaryDomain`, `SummaryCtx`, `CalleeRef`, `IfdsCaps`, and ordered
`IfdsOutput`.

Validate the supplied decomposition structurally and require exact equality
with a fresh `call_graph_sccs(cpg)` result. Interpret condensation edges as
caller-to-callee dependencies and use deterministic sink-first Kahn removal.
Evaluate acyclic singleton components once. Initialize recursive summaries to
bottom and apply ascending-`NodeId` sequential Kleene rounds until stable or
`max_scc_iters` is reached.

Check each candidate's executable lattice contract before updating. Retain
seeds and public output in ordered collections. Bound functions, retained facts,
condensation/summary work, recursive rounds, and default callee-label depth.
Return partial output with typed evidence rather than panic or silently claim
completeness.

Keep the trait accessible as `analysis::ifds::Lattice` and re-export it at the
crate root as `IfdsLattice`. This preserves the established root name
`Lattice` for the intraprocedural framework.

![The accepted design validates current call topology, schedules callees first, and distinguishes complete from partial output.](../diagrams/ifds-summary-fixpoint.svg)

*Figure — accepted IFDS/IDE summary architecture. Source:
[`ifds-summary-fixpoint.puml`](../diagrams/ifds-summary-fixpoint.puml).*

## Consequences

- Taint, effects, typestate, and future domains share one recursion and cap
  contract.
- No database identifier or pgmcp storage type crosses the libcpg API.
- A resolved call remains a stable `NodeId`; external and unresolved calls are
  explicit values.
- Reordering edge insertions cannot perturb public output or floating hash
  iteration.
- Stale SCC values are rejected, adding one fresh call-graph decomposition per
  solve.
- Runtime lattice checks catch local contract contradictions but cannot prove a
  domain distributive or monotone.
- A cap produces a monotone bottom-based prefix. Consumers must check
  `is_complete()` before a negative verdict.
- `SummaryDomain::summarize` remains an extension point; the framework cannot
  preempt or resource-bound arbitrary client code beyond passing `IfdsCaps`.

## Alternatives considered

### Port the pgmcp taint implementation unchanged

Rejected because it carries analysis-specific facts, database-facing
identities, hash collections, and a warning-only recursion ceiling. libcpg
needs a reusable, deterministic, feature-free core.

### Recursively evaluate callees on demand

Rejected because native recursion risks stack overflow, mutual recursion
requires an implicit cycle detector anyway, and traversal order would be harder
to make canonical. The existing iterative SCC substrate already gives the
correct unit of convergence.

### Iterate every function globally

Rejected because acyclic functions need one evaluation when callees are ready.
Global rounds repeat settled regions and make one recursive component's cap
affect unrelated components.

### Trust any supplied `SccDecomposition`

Rejected because public fields permit stale membership, direction, cyclic flags,
or condensation edges. A plausible but old decomposition can silently evaluate
callers before newly resolved callees.

### Return `Result` on every cap

Rejected because monotone partial summaries and precise cap evidence are useful
to interactive analysis. `IfdsOutput::is_complete` exposes the one safe
completeness predicate while retaining partial data. Structural and domain-law
issues use the same ordered evidence channel.

### Reuse the intraprocedural `Lattice` directly

Rejected because that framework also requires block-CFG direction, boundary,
and transfer operations. A small summary lattice makes IFDS/IDE domains
independent of CFG representation while preserving familiar algebra.

## Validation

[Scientific validation 27](../scientific/27-ifds-ide-validation.md) records
hand-computed acyclic and recursive oracles, exhaustive small-graph
reachability comparison, insertion-order determinism, semilattice laws,
cap-monotonicity properties, malformed inputs, public integration, serde, and
benchmark oracles.

## References

1. Reps, T., Horwitz, S., Sagiv, M. (1995). *Precise Interprocedural Dataflow
   Analysis via Graph Reachability.* DOI:
   [10.1145/199448.199462](https://doi.org/10.1145/199448.199462).
2. Sagiv, M., Reps, T., Horwitz, S. (1996). *Precise Interprocedural Dataflow
   Analysis with Applications to Constant Propagation.* DOI:
   [10.1016/0304-3975(96)00072-2](https://doi.org/10.1016/0304-3975(96)00072-2).
