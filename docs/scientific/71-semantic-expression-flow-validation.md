# Scientific validation 71 -- Semantic expression flow

## Claims under test

The validation campaign tests that:

1. exact replayed equality remains distinct from availability,
   anticipability, dominance, and kill freedom;
2. resolved definition identity, not name spelling, controls kills;
3. every CSE candidate has a dominating producer and no intervening kill;
4. every PRE candidate is very busy at a common dominating placement and has
   proof edges spanning its occurrences;
5. all incomplete states remain distinguishable from complete absence;
6. semantic diff implements proof/difference/limit/unknown precedence and
   rejects contradictory evidence;
7. old and new revision ids may overlap without namespace confusion;
8. outputs are deterministic and both CPGs plus legacy structural diff remain
   unchanged; and
9. logical depth does not consume native call-stack depth.

## Formal triangulation

[`SemanticExpressionFlow.tla`](../../formal/tla/SemanticExpressionFlow.tla)
enumerates proof, equality, availability, anticipability, dominance, kill,
completion, evaluator, and four-way reporting combinations. TLC generated and
checked 391,168 distinct reachable states at depth 2 with no error.

[`SemanticExpressionFlow.v`](../../formal/rocq/SemanticExpressionFlow.v) proves
forward and backward path laws, must-fact soundness, CSE/PRE prerequisites,
diff precedence, contradiction rejection, stable public identity, and CPG
immutability. Twenty printed theorem groups close under the global context with
no axioms or admissions.

[`semantic_expression_flow.smt2`](../../formal/z3/semantic_expression_flow.smt2)
independently refutes twenty-four forbidden states. They include equality used
as availability, availability without a dominating producer, an intervening
kill, PRE without anticipability, an unreplayed positive, unequal values hidden
by a limit, proof/evaluator contradiction, leaked private revision slots, and
CPG mutation.

## Example, property, and oracle campaigns

Eleven focused unit/property tests cover:

- resolved binding kills versus same-spelling shadowing;
- a diamond whose must-available class has no dominating producer;
- unknown-call barriers;
- all sixteen generation/kill combinations of a diamond against an
  independently enumerated path oracle;
- loops and exact visit-limit reporting;
- an exceptional exit that blocks PRE while normal-path CSE remains valid;
- candidate insertion-order determinism;
- all four semantic-diff statuses;
- 32 generated unequal closed-literal comparisons;
- 32 generated alpha-renaming cases whose bijection proofs replay; and
- a 100,000-node complete CFG-flow lifecycle on a 256 KiB native stack.

Nine integration tests build real tree-sitter CPGs for Rust, C, C++, Go, Java,
Python, JavaScript, TypeScript, and Ruby. Rust, C, C++, Go, Java, Python, and
Ruby prove `1 + 2` equivalent to `3` and replay the proof. JavaScript and
TypeScript return `Unknown` because their strict IEEE profiles do not authorize
the integer fold; the independent evaluator does not invent a host-language
rule. Each test runs structural `cpg_diff` before and after comparison and
checks identical structural output, node counts, and edge counts.

The campaign also reruns all 24 upstream expression-equivalence tests,
including two 100,000-depth small-stack lifecycles, and checks all targets and
features with warnings denied.

## Stack-safety evidence

The 100,000-node test constructs, solves, reports, clones, formats, serializes,
deserializes, and drops the full flow report on a 256 KiB worker stack. The
production dataflow solver, class traversal, kill-path search, proof replay,
and closed evaluator all use explicit heap state. The test is not ignored in
release mode.

This demonstrates constant native-stack use for that lifecycle. It does not
remove the need for heap, work, and process-memory limits.

## Performance baseline

`benches/semantic_expression_flow.rs` separates proof-plus-flow reuse scaling
from cross-revision constant-fold comparison. Criterion `--quick` smoke
intervals on this host were:

| Workload | 95% quick interval | Midpoint throughput |
|---|---:|---:|
| 2 linear equal occurrences | 14.199--14.243 us | 140.77 Kelem/s |
| 8 linear equal occurrences | 72.472--72.779 us | 110.29 Kelem/s |
| 32 linear equal occurrences | 913.98--922.22 us | 34.761 Kelem/s |
| cross-revision `1 + 2` versus `3` | 44.327--44.697 us | not applicable |

The first family includes expression proof construction/replay, semantic-class
construction, both must fixed points, dominance/path checks, and report
materialization. Its declining throughput includes the growing public exact
pair set inherited from expression equivalence; it is not presented as a
linear-complexity claim.

Measurements ran headlessly under `systemd-run --user --scope` with a 4 GiB
`MemoryMax`, `MemorySwapMax=0`, and one Cargo build job. No heaptrack or other
profiler GUI was opened. These quick intervals establish local regression
fixtures, not portable performance guarantees.

## Interpretation limits

Passing this campaign supports the represented equality and must-flow
implications. It does not prove frontend completeness, arbitrary alias safety,
whole-program behavior, transformation correctness, or profitability. PRE and
CSE remain advisory. `ProvenDifferent` covers exact unequal closed values, not
all possible inequivalent open programs. A complete `Unknown` is a valid
answer, especially when the semantic profile deliberately excludes a law.

MeTTaIL was not modified, built, formatted, branched, or committed. Its
permission-gated E3 refactor is not implementation evidence for this task.

## References

1. Kildall, G. A. (1973). *A Unified Approach to Global Program Optimization.*
   DOI: [10.1145/512927.512945](https://doi.org/10.1145/512927.512945).
2. Morel, E., Renvoise, C. (1979). *Global Optimization by Suppression of
   Partial Redundancies.* DOI:
   [10.1145/359060.359069](https://doi.org/10.1145/359060.359069).
3. Willsey, M. et al. (2021). *egg: Fast and Extensible Equality Saturation.*
   DOI: [10.1145/3434304](https://doi.org/10.1145/3434304).
