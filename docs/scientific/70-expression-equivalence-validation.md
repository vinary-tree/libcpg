# Scientific validation 70 -- Expression equivalence

## Claims under test

The validation campaign tests that:

1. only live, complete, pure, profile-matched expression roots enter an
   e-graph;
2. exact equality remains inside one binding context, while alpha evidence is
   explicitly qualified by a bijection;
3. every positive pair has an independently replayable sound proof;
4. all limited or unsupported states remain distinguishable from complete
   absence;
5. public pairs and serialized reports are deterministic under candidate
   insertion permutations;
6. AC normalization preserves leaf multiplicity and nested folds reach a fixed
   point; and
7. expression and proof depth do not consume native call-stack depth.

## Formal triangulation

[`ExpressionEquivalence.tla`](../../formal/tla/ExpressionEquivalence.tla)
enumerates modes, boundaries, forced stops, work/node/match/iteration/proof
caps, proof availability, fixed-point completion, and deterministic output.
TLC generated 8,616 states, found 8,608 distinct states, reached depth 6, and
reported no error.

[`ExpressionEquivalence.v`](../../formal/rocq/ExpressionEquivalence.v) proves
equivalence laws, boundary confinement, alpha qualification, replay soundness,
monotone knowledge, cap conservation, sorted output extension, AC permutation
preservation, and nested closed-fold evaluation. Nineteen printed theorem
groups are closed under the global context with no axioms or admissions.

[`expression_equivalence.smt2`](../../formal/z3/expression_equivalence.smt2)
independently refutes 22 forbidden states, including unsaturated absence,
unreplayable positives, AC authorization/multiplicity violations, and accepted
closed folds with an unevaluable source or mismatched literal.

## Example, property, and adversarial campaigns

The focused suite contains 24 tests:

- exact identity, commutativity, associativity, integer/Boolean identities,
  and constant folding;
- a nested two-stage fold with wrong-target and open-source proof corruption;
- all six permutations and both binary associations of three AC operands,
  producing and replaying all 66 exact pairs;
- the multiplicity counterexample $`a+a+b\not\equiv a+b+b`$;
- alpha bijection and non-bijective repeated-binding rejection;
- candidate, input, graph, work, match, iteration, pair, and proof caps;
- duplicate source, profile mismatch, incomplete lowering, persisted artifact,
  root mismatch, and effectful-root exclusion;
- exact transitivity, canonical pair order, irrelevant-candidate invariance,
  deterministic pair prefixes, and byte-identical serialized reports;
- every canonical operator discriminant family, malformed bytes, and generated
  arbitrary-precision integer round trips; and
- generated commutativity/insertion-order and checked-`i8` evaluator oracles.

Nine integration tests construct actual tree-sitter CPGs for Rust, C, C++, Go,
Java, Python, JavaScript, TypeScript, and Ruby. They lower typed facts, produce
and replay exact evidence, and verify that CPG node/edge counts are unchanged.

## Stack-safety evidence

Two independent gates run on 256 KiB worker stacks:

1. a 100,000-level pair completes IR preparation, graph construction,
   equivalence, proof construction/replay, clone, debug formatting,
   serialization, and drop; and
2. a manually built 100,000-level closed proof term exercises the new
   evaluator, full proof replay, formatting, and drop.

Both use flat arenas and explicit heap frames. Neither test is reduced or
skipped in release profiles.

## Performance baseline

`benches/expression_equivalence.rs` separates pair-output growth from
AC-normalization width. Criterion `--quick` smoke intervals on this host were:

| Workload | Median estimate |
|---|---:|
| 2 identical candidates | 6.8291 us |
| 8 identical candidates | 48.236 us |
| 32 identical candidates | 652.47 us |
| 4 AC operands | 236.26 us |
| 16 AC operands | 5.5132 ms |
| 64 AC operands | 196.93 ms |

These quick intervals prove benchmark operability and establish a local
regression baseline; they are not a cross-machine performance claim.

The direct quick binary completed in 0.24 seconds with maximum process resident
set size 43,076 KiB and no swaps. Headless Massif reported a peak total near
1.454 MiB for the 64-operand AC case. DHAT reported 238,630,252 allocated bytes
in 3,808,167 blocks, 1,424,796 bytes in 6,362 live blocks at global maximum,
and 561 bytes in three process-lifetime blocks at exit. Perf-stat captured
253.12 ms task clock, 2,794,582,388 instructions, and 6,740,506 cache misses for
the instrumented case.

VTune hardware sampling could not recognize the host processor. Software
sampling was then attempted and was denied by the host's nonzero Linux Yama
`ptrace_scope`. The campaign did not weaken that security control. Both
best-effort failures are retained in the verification capture.

All measurements ran headlessly under `systemd-run --user --scope` with a
4 GiB `MemoryMax`, `MemorySwapMax=0`, and one Cargo build job. No profiler GUI
was opened.

## Interpretation limits

Passing this campaign supports exact equality only in the represented typed
equational model. It does not prove whole-program observational equivalence,
frontend correctness, optimization profitability, or absence after an
incomplete pass. Structural clone classes, Datalog facts, dataflow results,
and security verdicts remain independent consumers.

## References

1. Leroy, X. (2009). *Formal Verification of a Realistic Compiler.* CACM. DOI:
   [10.1145/1538788.1538814](https://doi.org/10.1145/1538788.1538814).
2. Willsey, M. et al. (2021). *egg: Fast and Extensible Equality Saturation.*
   POPL. DOI: [10.1145/3434304](https://doi.org/10.1145/3434304).
3. Knuth, D. E. (1984). *Literate Programming.* DOI:
   [10.1093/comjnl/27.2.97](https://doi.org/10.1093/comjnl/27.2.97).
