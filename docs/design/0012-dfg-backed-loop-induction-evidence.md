# ADR-0012: DFG-backed loop-induction evidence

## Status

Accepted.

## Context

The natural-loop forest from [ADR-0007](0007-dominator-natural-loop-forest.md)
identifies headers, latches, bodies, nesting, and exits, but it does not explain
which computations remain constant across iterations or which variables move
by a regular step. Those facts are needed by later optimization, bound, and
cost analyses. Aho, Sethi, and Ullman §9.5 gives the classical reaching-
definitions formulation for loop invariants and induction variables [[1]](#references).

`libcpg` already has the necessary semantic evidence: the parser-independent
AST node vocabulary, a typed CFG, `DefUse` DFG edges, canonical `VarFacts`, a
validated `LoopForest`, and constant-time dominance queries. The decision must
preserve [ADR-0001](0001-unified-overlay-graph.md): analysis evidence must not
masquerade as source topology or require a second parser/intermediate
representation.

The CPG is language-neutral. It cannot prove every operator non-trapping,
resolve machine-width overflow, or model volatile and concurrent effects.
Therefore induction recognition and transformation authorization must be
separate contracts.

## Decision

Add a feature-free `loop_induction(cpg, function, forest)` analysis that
validates every supplied forest reference against the selected function and
returns one immutable, canonically ordered `InductionAnalysis`.

For each loop, form a finite function-scoped region from its CFG body and AST
descendants. Compute the least loop-invariant fixed point from non-empty
`DefUse` predecessor sets: a use is invariant only when all reaching
definitions are outside the region or already invariant. Never infer a missing
DFG relation.

Recognize an explicit basic induction variable only when one supported additive
recurrence is the variable's unique in-region definition and its update
dominates every latch. Recognize semantic `For` ranges as implicit increasing
step-one variables. Recognize one level of affine derived variables over a
basic IV, using checked integer coefficient arithmetic. Leave symbolic or
overflowing steps unknown.

Emit counted-trip evidence only for reducible loops whose range or exit guard
compares an IV against an invariant operand in a direction compatible with its
step. Do not compute a numeric trip count or claim termination.

Expose a conservative hoist-candidate subset only after structural kind,
dominance, unique-definition, and dominated-use checks. Keep that evidence
advisory and out of graph.

## Consequences

### Positive

- Reaching-definition evidence, not variable spelling alone, decides
  invariance.
- Unique, latch-dominating recurrence checks reject conditional and
  multiply-updated false positives.
- Semantic ranges let parser-backed Rust `for i in 0..x` participate without
  adding a Rust dependency to the analysis.
- Checked steps and explicit `Unknown` outcomes preserve conservative behavior
  across language integer models.
- Canonical ordered collections make reports independent of graph insertion
  order and reproducible across runs.
- Iterative, visited-set-bounded AST walks terminate on cyclic malformed graphs
  and are stack-safe for deeply nested regions.
- Out-of-graph results compose with later cost analyses without mutating source
  evidence or invalidating graph identifiers.

### Negative

- The fixed point rescans candidate subtrees and may be quadratic in the number
  of candidates per loop; nested regions intentionally overlap.
- Only additive basic recurrences and one-level affine derived expressions are
  recognized. Nonlinear, pointer, iterator, and recursively derived patterns
  remain absent.
- `Counted` does not mean “terminating”: an `!=` loop can step over its bound,
  and language-specific wrapping can defeat a directional argument.
- Hoist candidates still require a language-aware transformation policy for
  traps, overflow, aliasing, volatile/atomic access, and concurrency.
- A caller-supplied stale or foreign forest is rejected rather than silently
  repaired.

## Alternatives considered

### Parse recurrence source text in the analysis

Rejected. The language frontends already use tree-sitter and normalize syntax
into semantic CPG nodes. A source scanner would duplicate grammar behavior,
break Mode B and feature-free operation, and disagree with existing AST/DFG
identity. The Rust mapper instead preserves range and binder semantics, and the
analysis consumes those nodes.

### Match names without `DefUse` evidence

Rejected because shadowing, multiple reaching definitions, and missing edges
would create unsound invariance claims. Non-empty DFG evidence is an explicit
proof obligation.

### Accept any syntactically additive update

Rejected because an update in only one branch, or two updates to the same
variable, does not define one per-iteration step. Uniqueness and domination of
every latch are required.

### Compute exact trip counts

Rejected for this layer. Exact counts require initial-value relations,
strict/inclusive endpoint arithmetic, divisibility, machine-width and overflow
semantics, and proof that no other exit intervenes. This API records the
smaller fact it can justify: a direction-compatible IV/bound comparison.
Downstream runtime/size analysis can combine that evidence with stronger
domains, as illustrated by Brockschmidt et al. [[2]](#references).

### Mutate the CPG with invariant or induction edges

Rejected because these are derived, potentially policy-sensitive facts rather
than parsed program relations. Immutable reports preserve the unified graph as
the source of evidence and permit multiple analyses with different policies.

## References

1. Aho, A. V., Sethi, R., Ullman, J. D. (1986). *Compilers: Principles,
   Techniques, and Tools*, §9.5, “Loops in Flow Graphs.” Addison-Wesley.
   ISBN 0-201-10088-6 (no DOI).
2. Brockschmidt, M., Emmes, F., Falke, S., Fuhs, C., Giesl, J. (2014).
   *Alternating Runtime and Size Complexity Analysis of Integer Programs.*
   TACAS 2014, pp. 140–155. DOI:
   [10.1007/978-3-642-54862-8_10](https://doi.org/10.1007/978-3-642-54862-8_10)
