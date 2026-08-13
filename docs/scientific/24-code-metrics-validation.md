# Scientific Validation 24 — CK, LCOM, and Function Metrics

This page records the falsifiable evidence for the metric definitions selected
by [ADR-0029](../design/0029-evidence-bounded-code-metrics.md). The subject is
not whether one scalar predicts a future defect; it is whether `libcpg`
computes its documented scalar exactly from the represented CPG evidence.

## Hypotheses

| ID | Hypothesis | Refuting observation |
|---|---|---|
| H1 | Method-field links require a declaration reference and typed field read/write at the same access. | Spelling alone creates a link, or valid resolved evidence is omitted. |
| H2 | The hand class produces the documented WMC, DIT, NOC, CBO, RFC, and LCOM1–5 values. | Any exact integer differs or LCOM5 differs beyond floating tolerance. |
| H3 | Halstead and MI derived values equal independent formula evaluation from primitive counts. | A count or formula result differs. |
| H4 | Cognitive and cyclomatic scores obey the pgmcp-compatible structured rules and exclude nested functions. | Nest depth, logical flow, recursion, or nested-scope events are miscounted. |
| H5 | Equivalent fixed-ID graphs are invariant under reversed node and edge insertion. | Any `ClassMetrics` field differs. |
| H6 | Parser-built Rust sibling implementations attach to their uniquely scoped struct and consume real T2.15 field edges. | Methods/fields are absent, guessed, or LCOM differs from the fixture oracle. |
| H7 | Hostile selectors, inheritance cycles, and arithmetic extremes terminate with documented finite outcomes. | Panic, unbounded traversal, non-finite output, or undocumented partial result. |

![The experiments independently pin both evidence inputs and scalar outputs.](../diagrams/code-metrics-pipeline.svg)

*Figure — validation surface. Source:
[`code-metrics-pipeline.puml`](../diagrams/code-metrics-pipeline.puml).*

## Experiments

### Exact class oracle

`hand_class_has_exact_ck_and_lcom_suite` builds four classes. Class `C` has
three methods and two fields:

```text
first  -> x; calls D::external
second -> x; calls C::third
third  -> y
```

`first` contains one `If`, `second` one `While`, and `third` no decision. `C`
inherits `Base`; `Child` inherits `C`. Every access has an explicit `Reference`
and typed field DFG edge. The exact oracle is:

| Metric | Expected | Derivation |
|---|---:|---|
| WMC | 5 | $`2+2+1`$ |
| DIT | 1 | `C -> Base` |
| NOC | 1 | immediate child `Child` |
| CBO | 3 | `Base`, `Child`, and external owner `D` |
| RFC | 4 | three own methods plus `D::external` |
| LCOM1 | 2 | pairs `(first, third)` and `(second, third)` |
| LCOM2 | 1 | two disjoint minus one sharing pair |
| LCOM3 | 2 | `{first, second}` and `{third}` |
| LCOM4 | 1 | `second -> third` joins the two components |
| LCOM5 | 0.75 | $`(3-(2+1)/2)/(3-1)`$ |

The same test pins root/child DIT and NOC. The inheritance-cycle experiment
adds `A -> B` and `B -> A` and requires both depths to remain zero after SCC
condensation while both immediate-child counts remain one.

### Resolution boundary

`method_field_graph_requires_reference_and_typed_access_evidence` first creates
a same-name access with a valid field `Reference` but no `FieldRead` or
`FieldWrite`; it must remain absent. Adding a typed read makes the exact
`(method, access, field, reads=true, writes=false)` row appear. This is a
discriminating counterexample against name-only or reference-only algorithms.

`uniquely_scoped_impl_methods_attach_to_struct_but_ambiguous_names_do_not`
builds `Root -> Module -> Struct(S)` beside `Impl(S)`. The method and its field
access attach to the struct. Adding a second same-scope `Struct(S)` makes the
implementation ambiguous and both candidate graphs must exclude it.

### Halstead and maintainability formula oracle

`function_metrics_match_halstead_cognitive_and_mi_oracles` supplies two `+`
nodes, one assignment, one return, repeated `x`, two integer values, three
overlapping comment-line spans, and one disjoint span. The independent
primitive oracle is:

```text
distinct operators = 3     total operators = 4
distinct operands  = 3     total operands  = 5
vocabulary         = 6     length           = 9
logical lines      = 10    merged comments  = 4
```

The test independently evaluates:

```math
V=9\log_2 6
```

```math
D=\frac{3}{2}\frac{5}{3}=2.5
```

and the full raw-then-clamped pgmcp MI expression. It compares volume,
difficulty, effort, estimated bugs, and MI within $`10^{-12}`$. This
cross-checks `pgmcp/src/parsing/complexity.rs` without sharing production code.

### Control-nesting oracle

`nested_control_flow_logical_sequences_and_recursion_score_cognitively` builds
an `If` containing a `While`, then `&&`, `Break`, and a resolved recursive call.
A nested function contains another `If` that must be excluded. Cyclomatic is
$`1+3=4`$; cognitive is $`1+2+1+1+1=6`$. This distinguishes path count from
nest-weighted complexity and tests the nested-function boundary.

`semantic_operator_and_operand_taxonomy_is_complete` places every supported
statement/expression operator kind and every literal/typed/symbol operand kind
in one semantic function. Exact distinct and total counts prove that the
language-independent classifier is exhaustive over its documented vocabulary;
the fixture also exercises multi-arm matching and zero-line MI normalization.

### Property-based determinism and arithmetic totality

`ck_and_lcom_are_deterministic_under_insertion_order` generates between one and
five methods, one and four fields, and a Boolean method-field incidence mask.
It constructs equivalent fixed-ID CPGs with forward and reverse insertion,
then requires complete `ClassMetrics` equality. Each generated result must also
satisfy:

```math
\operatorname{LCOM}_2\le\operatorname{LCOM}_1
```

```math
\operatorname{LCOM}_4\le\operatorname{LCOM}_3
```

```math
0\le\operatorname{LCOM}_5\le1
```

`pgmcp_mi_formula_is_total_and_bounded` generates finite volume, arbitrary
`u32` complexity/line/comment counts, and requires a finite MI in $`[0,100]`$.
The crate's proptest configuration runs these blocks with the repository's
standard case count.

### Public and parser integration

`tests/metrics_integration.rs` contains two end-to-end cases:

- `public_feature_free_metrics_api_consumes_resolved_field_evidence` constructs
  a CPG through only public APIs, invokes all three metric functions, verifies
  the read witness and scalar row, and proves graph non-mutation.
- `parsed_rust_impl_exposes_real_ck_lcom_and_function_metrics` parses a real
  Rust `Counter` with fields `value` and `limit` plus sibling methods `bump` and
  `reset`. It requires two methods, two fields, real `Reference` edges for every
  retained access, one LCOM component, exact LCOM1 and LCOM5, nontrivial
  cyclomatic/cognitive/Halstead values, and bounded MI.

With feature `serde`, `public_metrics_round_trip_through_serde` round-trips
options, method-field evidence, class reports, and function reports.

## Result and validity boundary

The focused feature-free suite passes eleven unit/property experiments; the
serde suite adds the public report round-trip; and both public integration
cases pass in their respective feature matrices. These results corroborate
H1–H7 for the documented CPG semantics.

The maximal coverage experiment measures `analysis/metrics.rs` at **98.11 %**
line, **98.00 %** region, **100 %** function, and **80.00 %** branch coverage,
meeting the epic's 90 % line and 80 % branch thresholds.

They do not validate a causal relationship between a metric and human
maintenance effort, defect density, or design quality. They also do not claim
token-level Halstead equivalence: the classifier intentionally operates on
semantic CPG nodes. Resolver false negatives remain visible as absent evidence
and can lower observed CBO, RFC, or LCOM connectivity. See the complete
[component interpretation boundary](../components/graph/code-metrics.md#interpretation-boundary).

## Sources checked

The formulas and terminology are tied to the cited sources, including McCabe
([DOI](https://doi.org/10.1109/TSE.1976.233837)), Chidamber–Kemerer
([1991 DOI](https://doi.org/10.1145/117954.117970),
[1994 DOI](https://doi.org/10.1109/32.295895)), Li–Henry
([DOI](https://doi.org/10.1016/0164-1212(93)90077-B)), Campbell
([DOI](https://doi.org/10.1145/3194164.3194186)), Oman–Hagemeister
([DOI](https://doi.org/10.1109/ICSM.1992.242525)), and Coleman et al.
([DOI](https://doi.org/10.1109/2.303623)). The no-DOI Hitz–Montazeri paper and
the Henderson-Sellers/Halstead books are identified by stable paper/ISBN records
in the [component references](../components/graph/code-metrics.md#references).
