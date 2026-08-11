# Validating interval value-range analysis

This page records the evidence for libcpg's feature-free
[`value_ranges`](../components/graph/interval-analysis.md) analysis. The
validation separates the production abstract interpreter from its oracles:
concrete execution checks soundness, algebraic properties check the domain,
the generic solver exposes convergence evidence, and a parser-backed example
checks the complete source-to-report path.

## Hypotheses

The campaign evaluates seven falsifiable hypotheses:

1. **Concrete soundness.** Every value observed by the generated straight-line
   and bounded-loop interpreters belongs to the reported interval.
2. **Join algebra.** Interval hull and pointwise environment join are
   associative, commutative, and idempotent.
3. **Widening coverage.** Widening contains both its previous state and incoming
   candidate.
4. **Narrowing descent.** `Interval::narrow` and every solver narrowing sweep
   remain below the corresponding widened state.
5. **Bounded convergence.** Generated loopy CFGs terminate below 64 visits per
   block when widening is restricted to loop headers.
6. **Determinism.** Stable node identities with reversed node and CFG-edge
   insertion produce identical public reports.
7. **Non-interference.** Analysis does not change CPG node or edge counts.

## Concrete mini-interpreter

The straight-line generator chooses one `i8` initial value and up to 13
addition/subtraction statements with `i8` operands. The independent oracle
evaluates those statements directly in `i128`; it does not call interval
transfer code. The CPG fixture represents the same program with variable,
assignment, identifier, literal, AST, CFG, and `DefUse` evidence.

For every generated definition $`d`$ with concrete value $`v_d`$, the deciding
assertion is:

```math
v_d \in INTERVAL[d,x].
```

The bounded operand/count choices make concrete arithmetic exact, so a failure
cannot be dismissed as oracle overflow.

The counting-loop generator chooses an `i8` start, a positive distance below
32, and a positive step below 9. It concretely records each loop-header value
until the exit condition becomes true. Production analysis receives a cyclic
CFG with an explicit `LoopBack` edge. Every recorded value must belong to the
abstract header state, and every per-block visit count must remain below 64.

## Algebraic property experiments

`arb_interval` generates finite, lower-unbounded, upper-unbounded, and top
intervals. Each property runs 96 cases.

### Hull

For intervals $`A`$, $`B`$, and $`C`$:

```math
A \sqcup A = A,
\qquad
A \sqcup B = B \sqcup A,
\qquad
(A \sqcup B) \sqcup C = A \sqcup (B \sqcup C).
```

The environment experiment lifts the same laws through sorted variable-id
maps. This tests the exact `Lattice` implementation consumed by the solver.

### Widening and narrowing

For previous interval $`A`$ and candidate $`B`$:

```math
A \sqsubseteq A \nabla B
\qquad\land\qquad
B \sqsubseteq A \nabla B.
```

The public narrowing operator must satisfy:

```math
NARROW(A,B) \sqsubseteq A.
```

A fixed loop example then solves the same graph twice: once with zero
narrowing passes and once with two. Every narrowed `before` and `after` state
must be below its widened-only counterpart. This cross-checks domain order,
solver sweep policy, and loop-header widening together.

## Branch and parser experiments

A hand-built public-API graph starts an integer parameter at top and branches
on `x < 10`. The true target must report $`[-\infty,9]`$ and the false target
$`[10,+\infty]`$. Node and edge counts are sampled before analysis and must be
unchanged afterward.

The parser-backed Rust integration uses real source:

```rust
fn classify(x: i64) -> i64 {
    if x < 5 { return 1; } else { return 2; }
}
```

`TreeSitterCpgBuilder` performs parsing and constructs AST/CFG evidence. The
production interval analysis consumes only those semantic CPG nodes. The true
and false CFG targets must report $`[-\infty,4]`$ and
$`[5,+\infty]`$, respectively. This experiment guards the interface between
the language frontend, block projection, variable facts, comparison transfer,
and public report.

## Cross-analysis oracle

The generated straight-line CPG also runs through sparse conditional constant
propagation (SCCP). Where SCCP proves `ConstValue::Int(c)` for a definition, the
interval state for that definition and variable must contain $`c`$:

```math
SCCP[d] = Constant(c) \quad\Longrightarrow\quad c \in INTERVAL[d,x].
```

SCCP and interval analysis use different domains and drivers: SCCP uses sparse
AST/DFG value cells plus executable CFG edges, while intervals use dense block
environments plus widening. Agreement therefore catches mistakes that a
self-derived expected value could share.

## Adversarial and boundary experiments

Fixed examples cover:

- invalid and non-function selectors with exact `AnalysisError` variants;
- an AST cycle, which must terminate through the expression visited stack;
- a call on an assignment right-hand side, which must become top;
- empty intersections, which keep the path at environment bottom;
- duplicate/out-of-order environment pairs, which sort and hull canonically;
- extended endpoints, singleton membership, meet, narrowing, and partial order;
- rejection of the empty $`[+\infty,+\infty]`$ and
  $`[-\infty,-\infty]`$ encodings, with overflowing shifts and negation
  conservatively returning top;
- all true/false forms of `<`, `<=`, `>`, and `>=`, including comparisons with
  the literal on the left;
- simple, compound, unsupported, incomplete, and cyclic expression shapes; and
- Serde round trips for every public result/domain layer under `serde`.

The clean maximal-matrix coverage runs measure the interval module at
**99.46 % line**, **99.08 % region**, **100 % function**, and **95.95 % branch
coverage**. These figures came from the complete 672-test matrix that predated
the condensation suite, not a focused subset; the current census is in
[Testing](../engineering/02-testing.md).

## Reproduction

The focused experiments are:

```text
cargo test --lib analysis::dataflow::intervals
cargo test --test intervals_integration
cargo test --test intervals_integration --features lang-rust
```

The repository gate additionally runs the feature-free, maximal, all-feature,
Clippy, rustdoc, documentation, diagram, and diff-check matrices through
`./scripts/verify.sh`.

## Interpretation and limits

A passing campaign corroborates soundness for the generated mini-languages and
the hand-built/parser examples. It does not prove full Rust or cross-language
integer semantics. Unsupported operators deliberately become top, so the
analysis favors soundness over precision. Performance conclusions are also out
of scope: visit counts establish bounded convergence, not production latency.

The octagon citation documents the intended relational extension, not a claim
that libcpg currently tracks two-variable constraints.

## References

1. Cousot, P., Cousot, R. (1977). *Abstract Interpretation: A Unified Lattice
   Model for Static Analysis of Programs by Construction or Approximation of
   Fixpoints.* POPL '77. DOI:
   [10.1145/512950.512973](https://doi.org/10.1145/512950.512973)
2. Miné, A. (2006). *The Octagon Abstract Domain.* Higher-Order and Symbolic
   Computation 19, 31–100. DOI:
   [10.1007/s10990-006-8609-1](https://doi.org/10.1007/s10990-006-8609-1)
