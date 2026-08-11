# Validating sparse conditional constant propagation

This page records how libcpg tests the claims made by
[`sccp`](../components/graph/sccp-analysis.md). The validation separates the
production implementation from its oracles: concrete interpretation checks
value proofs, graph partitions check control proofs, and algebraic properties
check the lattice on which convergence depends.

## Hypotheses

The test campaign evaluates five falsifiable hypotheses:

1. **Value soundness.** If SCCP reports `Constant(c)` for a definition in the
   generated straight-line language, concrete execution produces $`c`$.
2. **Conditional partition.** Executable conditional edges and dead edges are
   disjoint and their union is every scoped conditional edge.
3. **Refinement monotonicity.** SCCP-refined unreachable nodes are a superset of
   structurally unreachable nodes.
4. **Determinism.** Stable node identities with permuted node and non-AST edge
   insertion produce identical results.
5. **Non-interference.** Running SCCP does not change CPG node or edge counts.

These are stronger than checking a few expected maps: each hypothesis relates
SCCP to an independent semantic or structural model.

## Concrete mini-interpreter

The generated `StmtSpec` language starts with one literal `x` and applies up to
11 statements selected from addition, subtraction, and bitwise exclusive-or
with `i8` literal operands. Concrete execution uses `i64`, so this bounded
corpus cannot overflow.

The CPG fixture represents every statement as a definition whose right-hand
side contains a `DefUse`-linked read of the previous definition. The oracle
interprets the same `StmtSpec` vector without calling production folding code:

```text
INTERPRET(initial, statements):
    x := signed-64(initial)
    values := [x]
    for (operator, literal) in statements:
        if operator is +: x := x + literal
        if operator is -: x := x - literal
        if operator is ^: x := x xor literal
        append x to values
    return values
```

For each of 96 generated cases, every concrete value is compared with the
constant stored for its CPG definition node. This avoids a self-fulfilling
oracle: the interpreter neither reads `ConstLattice` nor invokes SCCP transfer
functions.

## Control-flow oracle

A hand-computed boolean diamond has one `ConditionalTrue` and one
`ConditionalFalse` edge. The chosen edge must be executable and the other dead;
their sets must be disjoint and exhaustive. Plain reachability initially marks
both targets reachable, while refinement moves the rejected target to the
unreachable partition.

Additional fixtures distinguish three implicit phi cases:

| Executable definitions | Expected use |
|---|---|
| `7`, `7` | `Constant(7)` |
| `7`, `9` | `Overdefined` |
| live `7`, dead `9` | `Constant(7)` |

The third row specifically falsifies implementations that join DFG
predecessors without checking path executability.

## Algebraic and metamorphic properties

Generated `ConstLattice` values verify:

```math
a \sqcup a = a
```

```math
a \sqcup b = b \sqcup a
```

```math
(a \sqcup b) \sqcup c = a \sqcup (b \sqcup c)
```

The insertion-order test is metamorphic: it constructs the same identified
diamond twice, reverses node insertion and every non-AST edge insertion, and
requires `SccpResult` equality. AST edges retain source order because operand
order is semantic rather than an irrelevant permutation.

## Boundary and integration evidence

Example tests cover exact float bits, strings, characters, booleans, null,
unsupported aggregate literals, calls, unresolved inputs, checked integer
overflow, division by zero, shifts, signed-minimum negation, the full supported
scalar-operator vocabulary, cross-kind equality conservatism, non-boolean loop
headers, and `Case`/`DefaultCase` edges. Empty functions,
unknown/non-function selectors, mismatched refinement functions, DefUse edges
with every invalid scope/value-endpoint combination, Serde round trips, and
malformed AST cycles cover API boundaries.

The parser-backed Rust integration uses:

```rust
fn choose() {
    let x = 1 + 2;
    if x == 3 {
        let selected = 4;
    } else {
        let discarded = 5;
    }
}
```

It asserts constants on the addition, the `x` definition, and the comparison,
then locates the false CFG edge whose target contains `discarded`. This test
crosses the real parser, node mapper, CFG extractor, DFG extractor, SCCP, and
public crate-root API.

That integration first failed because anonymous tree-sitter tokens were
retained as CPG operands: `+` became a third binary child and `if` became the
CFG condition. The generalized node-aware filter now drops anonymous syntax
tokens while preserving named rules. A dedicated grammar-level regression
pins both sides of that distinction.

## Reproduction

The focused and cross-layer checks can be reproduced with:

```bash
cargo test --no-default-features analysis::dataflow::sccp::tests
cargo test --no-default-features --test sccp_integration
cargo test --no-default-features --features lang-rust --test sccp_integration
```

Repository verification additionally runs all features, Clippy, rustdoc,
documentation rendering/link checks, stable line/region coverage, nightly
branch coverage, and the staged pgmcp bug gate.

The clean maximal-feature coverage runs execute 627 library tests, 21 general
integration tests, 5 robustness tests, 3 SCCP integration tests, 3 interval
integration tests, and 3 loop-induction integration tests. For
`analysis/dataflow/sccp.rs`, stable coverage reports **98.58 % lines**,
**98.67 % regions**, and **100 % functions**; the nightly branch run reports
**80.00 % branches**. These figures satisfy the epic's per-module 90 % line and
80 % branch thresholds and are not derived from a focused-test subset.

## References

1. Wegman, M. N., Zadeck, F. K. (1991). *Constant Propagation with Conditional
   Branches.* ACM TOPLAS 13(2), 181–210. DOI:
   [10.1145/103135.103136](https://doi.org/10.1145/103135.103136)
