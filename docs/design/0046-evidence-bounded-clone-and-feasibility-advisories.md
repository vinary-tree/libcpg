# ADR-0046: Compose bounded clone and feasibility evidence

- **Status:** Accepted
- **Date:** 2026-08-19
- **Scope:** feature-free `analysis::bugs::{clone_bugs,infeasible}`

## Context

Two defect families require more evidence than a local syntax match. An
inconsistent clone is suspicious because a small change diverges from the
corresponding code in a near-duplicate. An infeasible branch is suspicious
because represented value facts prove that one Boolean outcome has no state.
Juergens et al. found that inconsistent changes to clones can indicate faults
[[1]](#references). Sparse conditional constant propagation (SCCP) determines
constant values and executable edges [[2]](#references), while interval
abstract interpretation can prove bound contradictions that no constant value
alone exposes [[3]](#references).

libcpg already provides exact Type-2/3 clone pairs, typed graph-edit-distance
(GED) evidence, SCCP, interval analysis, typed Abstract Syntax Tree (AST)
operators, and Control Flow Graph (CFG) edge outcomes. The advisory layer must
compose those substrates without reparsing source, mutating the CPG, hiding
incomplete evidence, or turning source nesting into native recursion.

![Both advisory paths compose typed graph evidence into qualified findings](../diagrams/clone-feasibility-advisory-pipeline.svg)

## Decision

1. I8 consumes the exact internal Type-2/3 pair relation. It does not recover
   pairs from transitive clone classes, because class closure can contain a
   member pair that never met the detector's pairwise threshold.
2. Each pair is represented by deterministic typed preorder AST tapes. A
   same-shape pair aligns linearly; a different-shape pair uses a bounded
   heap-resident Wagner–Fischer matrix. Missing nodes, cycles, shared nodes,
   node caps, allocation failure, and matrix caps are typed issues.
3. A pair is eligible only when normalized alignment edits divided by the
   larger tape size satisfy an exact rational threshold. Pair-local bounded GED
   remains evidence; a capped surrogate disqualifies the pair visibly.
4. I8 reports only typed semantic divergences:
   - an identifier pair that violates a dominant aligned rename mapping; or
   - an aligned binary/unary operator or literal inside represented branch
     guards whose exact payload differs.
5. Rename consistency is exact. For dominant mapping support $`n/p`$ and
   configured threshold $`s/d`$, accept only when $`nd\geq sp`$. Floating-point
   clone similarity is retained as upstream evidence but never decides rename
   support.
6. I9 discovers a condition only from a typed conditional CFG source or the
   first AST child of a recognized control wrapper. It does not scan arbitrary
   descendants for something comparison-shaped.
7. SCCP supplies constant guard values and dead conditional edges. Interval
   analysis supplies the exact named-variable environment immediately before
   each guard, including earlier statements in the same basic block.
8. I9 evaluates requested true and false outcomes with a heap-resident Boolean
   pushdown machine. It represents short-circuit conjunction/disjunction as
   exact path partitions and intersects normalized comparison regions with the
   entry environment.
9. Equality denotes one singleton region. Disequality denotes the two disjoint
   regions on either side. Empty regions at signed-integer extrema are proofs,
   not unsupported syntax.
10. A represented conditional edge is infeasible when SCCP marks it dead or
    every interval alternative for its Boolean outcome is empty. A complete
    condition is redundant when exactly one Boolean outcome is possible. A
    contradiction is reported only when every true alternative is empty and
    at least one interval predicate witness exists.
11. Every finding records its function, condition/source, proof substrate, SCCP
    value when present, entry intervals, and interval contradictions. Positive-
    only projections exist for review; absence claims require the qualified
    report and `is_complete()`.
12. All input-sized traversals, alignments, Boolean evaluations, witness
    lifecycles, and report ordering are iterative and heap-backed. Explicit
    node, state, step, pair, GED, and alignment caps terminate hostile inputs.

![The feasibility evaluator defunctionalizes Boolean recursion into explicit heap state](../diagrams/boolean-condition-pushdown-machine.svg)

## Literate algorithms

The I8 algorithm keeps candidate admission separate from semantic diagnosis:

```text
procedure FIND-INCONSISTENT-CLONES(CPG, policy)
  pairs := exact Type-2/3 pair relation in stable function order
  for each pair (L, R)
    left, right := bounded iterative typed AST tapes
    alignment := linear same-shape alignment or bounded edit matrix
    ged := bounded typed AST graph-edit upper bound
    if evidence is complete and exact_edit_fraction(alignment) is admitted
      infer dominant aligned identifier mappings
      emit every mapping violation with exact support
      emit every aligned guard operator/literal payload divergence
  return ordered findings, evaluated pair evidence, and typed issues
```

The I9 algorithm asks the same machine for both Boolean outcomes:

```text
procedure FIND-INFEASIBLE-PATHS(CPG, policy)
  for each function and reachable conditional CFG source in stable order
    condition := exact typed guard root
    constants := SCCP(function)
    entry := interval state immediately before source
    true_alternatives := EVALUATE(condition, true, entry)
    false_alternatives := EVALUATE(condition, false, entry)
    emit each represented edge proved dead by SCCP or empty alternatives
    if exactly one outcome is possible, emit redundant condition
    if true alternatives are empty with interval predicates, emit contradiction
  return ordered findings, analyzed conditions, and typed issues
```

## Rejected alternatives

### Hand-roll a source parser

Rejected. The frontends already own language grammar and syntax normalization.
Reparsing retained text would duplicate grammars, confuse strings/comments with
syntax, and lose resolved AST/CFG/DFG evidence.

### Diagnose transitive clone classes

Rejected. Clone-class connectivity is not pairwise equivalence. The exact pair
relation preserves the threshold evidence that admitted a comparison.

### Report every low-edit pair

Rejected. Low edit distance is a candidate filter, not a defect. I8 requires a
concrete rename-convention violation or guard-payload divergence.

### Collapse disequality to one convex interval

Rejected. $`x\ne c`$ is the union $`(-\infty,c-1]\cup[c+1,+\infty)`$ over
integers. Its convex hull would incorrectly admit $`c`$ and destroy proofs.

### Recursively evaluate the Boolean AST

Rejected. Deep generated expressions would consume proportional native stack
during evaluation and destruction. Explicit frames preserve the recursive
equations while fixing native call depth.

### Infer a contradiction from a constant `false`

Rejected. A false literal proves a redundant condition and dead true edge but
does not supply an interval contradiction. The contradiction category requires
predicate evidence such as $`x>5\land x<3`$.

## Consequences

### Positive

- One anomalous rename among ten aligned occurrences retains exact `9/10`
  support and concrete paired nodes.
- A flipped clone guard retains the exact left/right typed payloads.
- `x > 5 && x < 3` yields the dead true edge, redundant-false guard, and one
  contradiction with both predicates in evaluation order.
- `if true` yields the dead false edge and redundant-true guard without a false
  interval-contradiction label.
- Hand-built and parser-built graphs use the same public, feature-free analysis
  code. The analyzers are deterministic and read-only.
- One hundred thousand nested condition nodes execute on a 64 KiB native stack.

### Negative

- Clone divergence and infeasibility remain advisory. A deliberate clone
  change or unreachable defensive branch can be correct.
- I8 precision is bounded by clone classification, AST normalization,
  alignment, and guard representation. I9 precision is bounded by CFG, DFG,
  SCCP, interval, integer, and supported-operator evidence.
- Intervals forget relations not expressible through path-local intersections.
  Unsupported syntax becomes an issue rather than an unsound conclusion.
- Explicit caps bound work but can make a report incomplete; callers must not
  discard that qualification.

## Verification

Example tests pin the exact one-of-ten rename anomaly, guard flip, Type-3
single-pair behavior, contradiction, tautology, constant guard, same-block
interval state, every issue/cap, serde, and graph immutability. Generated tests
compare rename and Boolean outcomes with independent exact oracles and permute
insertion order. Parser integration builds Rust CPGs for both advisory paths.
Dedicated small-stack tests evaluate 100,000-node ASTs. Criterion workloads run
both analyzers at 8 and 32 functions.

## References

1. Juergens, E., Deissenboeck, F., Hummel, B., Wagner, S. (2009). *Do Code
   Clones Matter?* 31st International Conference on Software Engineering,
   485–495. DOI:
   [10.1109/ICSE.2009.5070547](https://doi.org/10.1109/ICSE.2009.5070547).
2. Wegman, M. N., Zadeck, F. K. (1991). *Constant Propagation with Conditional
   Branches.* ACM TOPLAS 13(2), 181–210. DOI:
   [10.1145/103135.103136](https://doi.org/10.1145/103135.103136).
3. Cousot, P., Cousot, R. (1977). *Abstract Interpretation: A Unified Lattice
   Model for Static Analysis of Programs by Construction or Approximation of
   Fixpoints.* POPL, 238–252. DOI:
   [10.1145/512950.512973](https://doi.org/10.1145/512950.512973).
