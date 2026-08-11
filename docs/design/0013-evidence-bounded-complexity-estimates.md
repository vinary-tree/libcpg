# ADR-0013: Evidence-bounded complexity estimates

## Status

Accepted.

## Context

The original `ComplexityAnalyzer` inferred loop cost from Abstract Syntax Tree
(AST) nesting and a small counted-loop heuristic. That behavior is useful for
AST-only and client-built Code Property Graphs (CPGs), but syntax alone cannot
distinguish a natural loop from an arbitrary nested node shape, identify an
irreducible cyclic region, or name the definition and bound that justify a
counted trip.

The feature-free analysis substrate now provides stronger evidence: a
function-scoped Control Flow Graph (CFG), dominance-qualified `LoopForest`, and
Data Flow Graph (DFG)-backed `InductionAnalysis`. The `algorithm-detection`
consumer can compose those providers without adding a grammar dependency or
mutating the graph. The decision must also preserve the existing public
`estimate_time_complexity` behavior for callers that rely on AST-only input.

No language-neutral static analysis can infer an exact input-size recurrence
from every loop. `TripEvidence::Counted` proves only a supported induction
variable/bound relation; it does not prove termination, input dependence,
machine-width safety, or the absence of another exit. Irreducible control flow
has no single natural-loop nesting interpretation. Confidence must reflect
these evidence boundaries rather than conceal them.

## Decision

Add
`ComplexityAnalyzer::estimate_time_complexity_with_evidence(cpg, function)`
behind the existing `algorithm-detection` feature. Keep
`estimate_time_complexity` unchanged as both public compatibility surface and
the exact total fallback.

The new method first computes that fallback. It then accepts graph evidence
only when the centralized intraprocedural CFG scope is valid and contains an
edge, `loop_forest` succeeds with a non-empty forest, and `loop_induction`
validates the supplied forest. Any failure returns the already-computed
fallback without changing its class, confidence bits, or justification.

For validated evidence, map maximum reducible-loop depth $`d`$ to one
polynomial degree: `Linear`, `Quadratic`, `Cubic`, or `Polynomial(d)`. If every
cyclic region is irreducible, represent the region conservatively as one linear
factor. Assign confidence `1.0` only when every reducible trip is counted; cap
unknown trips at the original loop-heuristic confidence; assign any
irreducible result the conservative `0.3` floor.

Produce a deterministic justification naming every stable loop id, header,
depth, induction variable, comparison node, and bound node. When recursion is
also present, retain the asymptotically worse class but append the subordinate
evidence. Keep the result immutable and out of graph.

Switch `DefaultAlgorithmDetector` to the evidence-aware method. Preserve its
existing AST loop/recursion patterns for family signatures and for fallback;
the switch changes the complexity consumer, not the family taxonomy.

## Consequences

### Positive

- The default complexity estimate is grounded in executable CFG cycles and
  DFG-backed trip evidence rather than AST nesting alone.
- Exact legacy fallback preserves AST-only, invalid-selector, loop-free, and
  partially constructed graph behavior.
- Fully counted reducible nests receive full confidence and cite the precise
  headers, variables, comparisons, and bounds that decide the result.
- Unknown trips and irreducible regions remain visible instead of being
  upgraded to invented certainty.
- Canonical provider reports make results independent of graph storage and
  edge insertion order.
- The consumer adds no parser, grammar feature, graph mutation, serialized
  type, or Cargo dependency.
- Recursion composition does not erase subordinate loop evidence.

### Negative

- Evidence-aware calls pay for CFG projection, loop analysis, induction
  analysis, and legacy fallback computation; repeated callers should account
  for this cost at their own orchestration layer.
- Maximum natural-loop depth is still a structural $`O(n^d)`$ model. A loop
  bounded by a constant or logarithm may be over-estimated.
- `Counted` is not an exact trip count or termination proof, even at confidence
  `1.0`; confidence describes evidence completeness for this model.
- A mixed function containing one irreducible region lowers the whole loop
  estimate to the irreducible confidence floor.
- Recursion remains the pre-existing name/operator heuristic and may dominate
  a stronger loop estimate.

## Alternatives considered

### Replace the original method in place

Rejected. Existing callers may construct AST-only graphs or assert the legacy
justification and confidence. An additive method makes the stronger contract
explicit while preserving source and behavioral compatibility.

### Parse source text or introduce a complexity-specific parser

Rejected. Tree-sitter frontends already normalize syntax into semantic CPG
nodes. A second parser would disagree with Mode B, duplicate grammar behavior,
break feature-free providers, and lose stable node identity. This consumer
reads the existing CFG/DFG evidence only.

### Treat every AST loop as counted evidence

Rejected. Syntactic nesting does not prove reachability, natural-loop
structure, a unique induction recurrence, or an invariant bound. The legacy
method retains that heuristic boundary; the new method must not relabel it as
proof.

### Compute symbolic trip counts and solve recurrences

Rejected for this API. That requires initial-value relations, endpoint
arithmetic, path-sensitive exits, language overflow semantics, and a recurrence
solver. The current providers justify a smaller, well-tested fact: a
direction-compatible IV/bound comparison. Runtime and size analyses can add
stronger domains, as in Brockschmidt et al. [[1]](#references).

### Mutate the CPG with complexity edges or annotations

Rejected. Complexity is derived and policy-sensitive, not parsed topology.
Out-of-graph reports preserve the CPG as source evidence and allow different
consumers to apply different confidence policies.

### Average confidence across loops

Rejected. An average can hide one unknown or irreducible region behind several
strong loops. The minimum-evidence policy keeps the weakest structural
obligation visible.

## References

1. Brockschmidt, M., Emmes, F., Falke, S., Fuhs, C., Giesl, J. (2014).
   *Alternating Runtime and Size Complexity Analysis of Integer Programs.*
   TACAS 2014, pp. 140–155. DOI:
   [10.1007/978-3-642-54862-8_10](https://doi.org/10.1007/978-3-642-54862-8_10)
2. Cormen, T. H., Leiserson, C. E., Rivest, R. L., Stein, C. (2009).
   *Introduction to Algorithms* (3rd ed.), Chapter 4. MIT Press.
   ISBN 978-0262033848 (no DOI).
