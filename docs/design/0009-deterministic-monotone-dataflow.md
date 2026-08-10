# 0009 — Deterministic monotone data-flow solver

## Status

**Accepted.** Implemented by `analysis::dataflow` over the `BlockCfg` selected
in ADR-0008.

## Context

Liveness, available expressions, sparse constant propagation, and interval
analysis need the same direction-polymorphic fixed-point machinery. Encoding a
private worklist in each pass would duplicate convergence bugs, resource
limits, ordering policy, edge semantics, and widening behavior. Reusing the
DFG builder's AST-ordered sweep would be incorrect: that pass materializes
nested identifier def-use evidence and deliberately is not a CFG fixed point.

The shared engine must remain available with `default = []`, preserve the
source CPG, produce repeatable states, support finite and infinite-height
domains, expose enough convergence evidence to diagnose hostile input, and
permit node-level queries without splitting the id-preserving blocks.

## Decision

**Provide a generic Kildall worklist over immutable `BlockCfg`, with an explicit
`Lattice` and `DataflowAnalysis` contract, deterministic RPO/post-order
priorities, loop-header-only widening, bounded narrowing, and a hard per-block
visit cap.**

Canonical `FactUniverse<T>` values supply stable dense ids. `BitSetDomain`
implements union/subset for may analyses; `MustBits` implements
intersection/reverse-subset for must analyses. Shared variable and expression
fact builders encode exact syntax classifications once. `DataflowSolution`
stores block boundary states and visit counts, and replays a caller-provided
node transfer for `state_at` queries.

The formal equations, API, pseudocode, fact rules, complexity, and validation
oracles are in the [component guide](../components/graph/dataflow-framework.md).

## Consequences

### Positive

- **One convergence contract.** Forward/backward propagation, boundary
  conditions, typed edge transfer, widening, narrowing, and caps are tested in
  one place.
- **Deterministic output.** Canonical facts and block priorities make states
  independent of insertion order; alternate worklist policies are property-test
  oracles rather than public nondeterminism.
- **Overlay identity survives.** Transfers inspect original `NodeId` members;
  solving never rewrites the CPG.
- **Finite and infinite domains.** Dense bit sets need no widening, while
  interval-like clients can widen only at proven loop headers and narrow under
  a fixed budget.
- **Reusable fact classification.** Variable binding/use and structural
  expression rules cannot silently diverge across later analyses.

### Negative

- **Client laws are trusted.** Rust's trait system cannot prove lattice laws or
  transfer monotonicity; a bad client can obtain a meaningless result, although
  the visit cap still terminates worklist execution.
- **Block and node transfers must agree.** `state_at` deliberately accepts the
  node-level counterpart as a closure; disagreement gives a wrong intra-block
  state.
- **Dense states scale with the universe.** A large fact universe is copied per
  block. Sparse or symbolic domains remain possible through the trait but are
  not built in.
- **Narrowing is budgeted.** A configured number of deterministic sweeps may
  stop above the most precise post-widening result.

## Alternatives considered

### One solver per analysis

Rejected because duplicated scheduling and convergence logic would make
termination and determinism analysis-specific and harder to validate.

### Use the AST-ordered DFG sweep as the universal engine

Rejected because it is designed to reach nested expression uses and applies a
bounded structural loop approximation. It cannot implement arbitrary CFG
confluence, backward analysis, edge transfer, or widening.

### Expose a petgraph algorithm directly

Rejected because petgraph does not supply this domain/transfer contract, and
native node indices would discard the stable `NodeId` and typed-edge guarantees
of `BlockCfg`.

### FIFO as the public scheduling contract

Rejected because deterministic RPO/post-order priorities generally converge
with fewer unproductive visits and state the intended control-flow order. FIFO
and LIFO remain independent test policies that must reach the same fixed point.

### Widen every block

Rejected because widening away from cyclic headers sacrifices precision
without solving a convergence problem. `LoopForest` provides exact natural and
conservative irreducible header evidence.

## References

- Kildall, G. A. (1973). *A Unified Approach to Global Program Optimization.*
  POPL '73. DOI:
  [10.1145/512927.512945](https://doi.org/10.1145/512927.512945)
- Kam, J. B., Ullman, J. D. (1977). *Monotone Data Flow Analysis Frameworks.*
  Acta Informatica 7. DOI:
  [10.1007/BF00290339](https://doi.org/10.1007/BF00290339)
- Cousot, P., Cousot, R. (1977). *Abstract Interpretation: A Unified Lattice
  Model for Static Analysis of Programs by Construction or Approximation of
  Fixpoints.* POPL '77. DOI:
  [10.1145/512950.512973](https://doi.org/10.1145/512950.512973)
