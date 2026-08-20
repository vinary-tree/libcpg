# Component — Clone-consistency and path-feasibility detectors

## Responsibility

`analysis::bugs::{clone_bugs,infeasible}` owns two feature-free, read-only
advisory analyses. I8 diagnoses typed divergences inside exact low-edit
Type-2/3 clone pairs. I9 combines sparse conditional constant propagation
(SCCP) and interval evidence to diagnose infeasible branch outcomes, redundant
conditions, and integer contradictions. Parsing, normalization, clone
classification, CFG/DFG construction, SCCP, and interval fixed points remain in
their existing providers.

![Typed clone and feasibility evidence flows into qualified immutable reports](../../diagrams/clone-feasibility-advisory-pipeline.svg)

## Feature boundary

Both analyzers compile with `default = []`. A `lang-*` feature is needed only
when libcpg itself parses source. `serde` adds configuration/report codecs
without changing analysis semantics. Neither analyzer adds, removes, or changes
graph nodes, edges, or properties.

## Source map

| File | Ownership |
|---|---|
| `src/analysis/bugs/clone_bugs.rs` | I8 policy, AST tapes/alignment, typed divergence evidence, reports/issues |
| `src/pattern/clones.rs` | exact internal Type-2/3 pair relation consumed by I8 |
| `src/analysis/bugs/infeasible.rs` | I9 condition arena, Boolean pushdown machine, SCCP/interval evidence, reports/issues |
| `src/analysis/dataflow/conditions.rs` | exact shared typed guard-root discovery |
| `src/analysis/dataflow/sccp.rs` | constant cells and executable/dead conditional edges |
| `src/analysis/dataflow/intervals.rs` | guard-local node environments and normalized comparison regions |

## Public surface

| API/type | Contract |
|---|---|
| `analyze_inconsistent_clones` | qualified I8 report or invalid-policy error |
| `inconsistent_clone_bugs` | positive-only I8 finding projection |
| `CloneBugConfig` | clone/GED policy, exact edit/rename thresholds, AST/alignment caps |
| `CloneBugAnalysis` | findings, exact evaluated pair evidence, issues, `is_complete()` |
| `InconsistentCloneBug` | pair, clone/GED/edit evidence, paired witness nodes, typed divergence |
| `ClonePairEvidence` | exact Type-2/3 pair admission and low-edit decision |
| `InconsistentCloneKind` | identifier-rename anomaly or guard-payload divergence |
| `analyze_infeasible_paths` | qualified I9 report or invalid-policy error |
| `infeasible_paths` | positive-only I9 finding projection |
| `InfeasiblePathConfig` | condition-node, path-state, and machine-step caps |
| `InfeasiblePathAnalysis` | findings, analyzed functions/conditions, issues, `is_complete()` |
| `InfeasiblePathFinding` | function/source/guard, kind, proof substrate, entry ranges, contradictions |
| `InfeasiblePathKind` | infeasible branch, redundant condition, or contradictory condition |
| `FeasibilityProofSource` | SCCP, intervals, or both |

## I8 pipeline

1. Enumerate exact Type-2/3 pairs in stable function-id order.
2. Construct bounded flat typed AST tapes with an iterative enter/exit machine.
3. Align equal shapes linearly; otherwise allocate a checked bounded edit
   matrix and reconstruct a canonical alignment.
4. Compute bounded pair-local typed GED evidence and reject capped surrogates.
5. Admit pairs whose exact structural edit fraction meets policy.
6. Infer exact dominant rename mappings in each direction and emit violations.
7. Compare aligned nodes within represented guards and emit exact operator or
   literal payload divergences.

## I9 pipeline

1. Enumerate functions and reachable CFG sources with conditional outcomes.
2. Discover one exact typed guard root and the interval environment immediately
   before the source.
3. Reuse SCCP for complete-guard constants and executable/dead CFG edges.
4. Build a bounded flat condition arena and query true/false outcomes through
   explicit heap continuations.
5. Normalize variable/constant comparisons, fork disjoint regions, intersect
   entry ranges, and retain predicates for every empty alternative.
6. Combine SCCP and interval evidence into per-edge infeasibility, complete-
   guard redundancy, and predicate-backed contradiction findings.

![The I9 continuation machine implements exact short-circuit partitions without native recursion](../../diagrams/boolean-condition-pushdown-machine.svg)

## Invariants

1. The input CPG is unchanged.
2. Every finding witness exists in the input graph and belongs to its recorded
   function evidence scope.
3. I8 evaluates exact pair relations once; transitive clone-class membership
   does not fabricate pairs.
4. Rename and edit thresholds use exact integer cross multiplication.
5. A low-edit pair without typed suspicious divergence is not a finding.
6. I9 excludes SCCP-unreachable condition sources from diagnosis.
7. Disequality remains two disjoint regions; convexification cannot reintroduce
   the excluded constant.
8. A contradiction requires interval predicate evidence.
9. Findings/issues are canonically ordered and insertion-order independent.
10. All input-shaped traversals and lifecycles are heap-backed and bounded.

## Bounds and failure semantics

| Bound | Observable result |
|---|---|
| clone `max_pairs` / Type-4 budgets | upstream `AnalysisError::BudgetExceeded` |
| `max_ast_nodes` | `CloneBugIssue::AstNodeCap`; affected pair is not classified |
| `max_alignment_cells` | alignment cap/allocation issue; affected pair is not classified |
| GED `max_nodes` | `GedCapped`; capped surrogate is excluded |
| `max_condition_nodes` | `ConditionNodeCap`; condition has no feasibility finding |
| `max_condition_states` | `ConditionStateCap`; path partition is incomplete |
| `max_condition_steps` | `ConditionStepCap`; evaluation stops visibly |

Malformed structural evidence is retained as an issue so valid findings from
other functions/pairs remain usable. Invalid zero or above-hard-ceiling policy
values return `AnalysisError::InvalidInput` before graph analysis.

## Complexity

For one clone pair with tape sizes $`n`$ and $`m`$, same-shape alignment is
$`O(n+m)`$; different-shape alignment is $`O(nm)`$ time and space within the
cell cap. GED has its documented bounded assignment cost. For one condition
with $`v`$ arena nodes, $`s`$ path states, and $`k`$ machine transitions, I9
cost is $`O(v+s+k)`$ modulo ordered environment operations and the configured
caps. Boolean disjunction can grow alternatives exponentially in syntax size;
the state/step caps make that risk explicit.

## Verification links

Unit examples cover exact rename and guard witnesses, contradictions,
tautologies, constants, same-block state, all caps/issues, serde, and graph
immutability. Property tests compare independent alignment/Boolean oracles and
insertion orders. Public integration includes a parser-built Rust CPG. A 64 KiB
worker stack evaluates 100,000 nested nodes. Criterion executes both analyzers
at 8 and 32 functions.

See [Theory 36](../../theory/36-clone-consistency-and-abstract-feasibility.md),
[ADR-0046](../../design/0046-evidence-bounded-clone-and-feasibility-advisories.md),
the [validation ledger](../../scientific/41-clone-feasibility-validation.md),
the [security boundary](../../security/11-clone-feasibility-detectors.md), and
the [usage guide](../../usage/19-clone-feasibility-advisories.md).

## References

1. Juergens, E., Deissenboeck, F., Hummel, B., Wagner, S. (2009). *Do Code
   Clones Matter?* DOI:
   [10.1109/ICSE.2009.5070547](https://doi.org/10.1109/ICSE.2009.5070547).
2. Wegman, M. N., Zadeck, F. K. (1991). *Constant Propagation with Conditional
   Branches.* DOI:
   [10.1145/103135.103136](https://doi.org/10.1145/103135.103136).
3. Cousot, P., Cousot, R. (1977). *Abstract Interpretation.* DOI:
   [10.1145/512950.512973](https://doi.org/10.1145/512950.512973).
