# 0007 — Dominator-based natural-loop forest

## Status

**Accepted.** Implemented by `analysis::loops`; `CfgExtractor` also emits
`LoopExit` at its single block-continuation site.

## Context

A cyclic strongly-connected component (SCC) proves mutual reachability, but it
does not by itself identify a single-entry loop header, latches, natural body,
or lexical nesting. Syntax-only loop counting has the opposite weakness: it can
name parsed loop constructs but cannot describe cyclic CFGs introduced by
lowering or malformed/multiple-entry control flow. Optimization and structured
analysis need both exact natural-loop evidence and an honest representation of
irreducibility.

The API also has to be feature-free, deterministic under insertion-order
permutations, stack-safe on deep graphs, and compatible with the shared
reachable intraprocedural function scope used by post-dominance.

## Decision

**Classify natural back edges by dominance, derive bodies by reverse closure,
and represent non-dominating retreating cycles as explicit whole-SCC
irreducible regions.**

Back edges sharing a header are merged before body construction. Loop nesting
is strict body containment, with the smallest enclosing body as parent. A
sorted `LoopForest` records roots, parent/child links, depth, innermost node
membership, latches, entries, and exit edges.

The CFG builder retypes the existing continuation edge from `Sequential` to
`LoopExit` whenever that continuation leaves an AST loop scope. Retyping occurs
at one emission site, preserving edge count and cyclomatic complexity.

The exact mathematics, pseudocode, API contract, complexity, and validation
oracles are in the [loop-analysis component guide](../components/graph/loop-analysis.md).

## Consequences

### Positive

- **Evidence-backed headers.** A natural header dominates every latch tail.
- **Shared-header fidelity.** Multiple latches form one loop rather than
  duplicate overlapping results.
- **Visible irreducibility.** Whole-SCC over-approximation is marked, so a
  consumer cannot confuse it with a single-entry natural loop.
- **Determinism and depth safety.** Ordered results and iterative graph/forest
  walks make insertion order and native stack depth irrelevant.
- **Builder/analysis agreement.** `LoopExit` edges expose loop boundaries to
  basic-block identification while `LoopForest.exits` independently derives
  every body-crossing transition.

### Negative

- **Conservative irreducible bodies.** A whole SCC can include nodes not present
  in any one intuitive cycle; the `irreducible` flag is mandatory context.
- **Quadratic nesting comparison.** Strict containment compares loop bodies
  pairwise. This favors simple, auditable behavior over a more complex interval
  index; real functions normally contain far fewer loops than CFG nodes.
- **Reachability boundary.** Unreachable cyclic fragments are excluded by the
  shared `FunctionCfgView`, matching dominator and post-dominator semantics.

## Alternatives considered

### Treat every cyclic SCC as one loop

Rejected because nested reducible loops collapse into one outer cyclic SCC,
losing headers, latches, membership, and depth.

### Use syntax nodes as the authoritative loop forest

Rejected because syntax is not control-flow evidence and cannot model lowered,
hand-built, or irreducible CFG cycles.

### Force each retreating edge into a natural loop

Rejected because a retreating edge without a dominating head does not establish
a single-entry natural loop. Doing so would fabricate structure precisely where
the CFG proves irreducibility.

### Add a second `LoopExit` edge beside `Sequential`

Rejected because it changes edge counts, inflates cyclomatic complexity, and
creates parallel competing classifications for one continuation.

## References

- Aho, A. V., Sethi, R., Ullman, J. D. (1986). *Compilers: Principles,
  Techniques, and Tools.* Addison-Wesley. ISBN 0-201-10088-6 (no DOI).
- Havlak, P. (1997). *Nesting of Reducible and Irreducible Loops.* ACM TOPLAS
  19(4), 557–567. DOI:
  [10.1145/262004.262005](https://doi.org/10.1145/262004.262005).
- Ramalingam, G. (1999). *Identifying Loops in Almost Linear Time.* ACM TOPLAS
  21(2), 175–188. DOI:
  [10.1145/316686.316687](https://doi.org/10.1145/316686.316687).
