# ADR-0033: Compositional interprocedural effect summaries

- **Status:** Accepted
- **Date:** 2026-08-13
- **Scope:** feature-free `analysis::effects`

## Context

Taint, System Dependence Graph (SDG), and optimization clients need to know
whether a represented function reads or writes observable state, returns an
argument, or returns one constant. Re-running whole-body analysis at every
call duplicates work and fails on recursion. A source-text classifier would
also violate libcpg's language-agnostic evidence boundary.

T3.2 already supplies validated call-graph SCC scheduling and bounded monotone
fixed points. T3.5 must instantiate that framework without weakening its
determinism or completeness contracts.

## Decision

1. Represent observable storage as `Global`, `Param`, or `Field` places.
2. Build direct effects only from typed CPG node/edge evidence.
3. Compose resolved callees in deterministic `NodeId` order; map callee
   parameter places through the shared call-argument normalizer.
4. Treat unresolved/external calls and represented throw/await/yield effects
   conservatively as `Impure`.
5. Reuse SCCP's checked scalar fold functions for local and interprocedural
   constant returns.
6. Store constants in a lawful bottom/constant/overdefined coordinate. Preserve
   the ergonomic `Option<ConstValue>` plus an explicit overdefined bit.
7. Offer `analyze_effects` as the completeness-preserving API and retain the
   task-specified `effect_summaries` map projection as a documented convenience.
8. Make domain-local caps explicit in each summary and in the aggregate result.

## Rejected alternatives

### Parse source or match language spellings

Rejected because it duplicates frontend parsers, disagrees across languages,
and can fabricate evidence absent from the CPG.

### Use `Option<ConstValue>` as both bottom and overdefined

Rejected because it is not a lattice. After two constants conflict, a later
join could incorrectly revive a constant.

### Classify every parameter mutation as globally impure

Rejected because the callee does not know what actual place its caller passes.
Delayed parameter remapping is both more precise and compositional.

### Return only a map

Rejected as the sole API because caps and malformed SCC evidence would be
lost. The map remains only as a convenience projection.

## Consequences

- Recursive effects converge through the existing generic solver.
- Callers can distinguish reads, writes, constants, parameter-return flow,
  truncation, and incomplete results.
- No graph nodes or edges are created or modified.
- Field/index precision is limited by existing typed DFG/reference evidence.
- The 64-bit return-dependence mask has an explicit truncation indicator.
- T3.3 taint and T3.4 SDG can refine external/call summaries without acquiring
  a second interprocedural scheduler.

## Verification

Example tests cover pure leaves, globals, fields, parameter mutation,
unresolved and impure calls, scalar folding, disagreement, recursion,
malformed SCCs, caps, and width boundaries. Three 256-case properties check
the product-lattice laws, exact transitive-purity reachability under edge-order
permutation, and iteration-cap monotonicity. Public feature-free integration
and serde round-trip tests exercise the exported contract.

## References

1. Reps, Horwitz, Sagiv (1995), DOI
   [10.1145/199448.199462](https://doi.org/10.1145/199448.199462).
2. Wegman, Zadeck (1991), DOI
   [10.1145/103135.103136](https://doi.org/10.1145/103135.103136).
