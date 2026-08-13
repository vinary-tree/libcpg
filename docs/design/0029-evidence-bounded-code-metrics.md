# ADR-0029 — Evidence-Bounded Code Metrics

- **Status:** Accepted
- **Date:** 2026-08-13
- **Decision owners:** `libcpg` maintainers
- **Scope:** feature-free `analysis::metrics`

## Context

The CPG now contains resolved call targets and real member-to-field
`Reference`, `FieldRead`, and `FieldWrite` topology. Earlier pattern metrics
matched member spelling against field names, which could conflate unrelated
receivers and could not expose LCOM1–5, the CK suite, Halstead counts, cognitive
complexity, or maintainability index through the stable analysis API.

The new API must work without grammar features or retained source, remain
deterministic under petgraph storage order, preserve the evidence behind class
cohesion, and align its numeric scoring kernel with pgmcp. It must also define
degenerate and malformed cases rather than returning non-finite values or
walking inheritance cycles indefinitely.

## Decision

### Preserve method-field witnesses

Expose `MethodFieldGraph` before scalar LCOM values. A retained access requires
both a declaration `Reference` and a typed field read/write touching the access.
Store method, access, field, and read/write flags. Store resolved internal method
calls separately for LCOM4. Do not fall back to member-name matching.

Associate a sibling `Impl(T)` with a nominal type only if the exact name `T`
identifies one declaration in the same root/module scope. Never borrow fields
from implementation blocks or cross a nested nominal/function declaration.

### Publish one explicit metric vocabulary

- WMC sums semantic cyclomatic complexity for direct methods.
- DIT is longest inheritance-component depth; NOC is immediate child count.
- CBO counts distinct external nominal owners supported by call/type edges.
- RFC counts own methods plus distinct direct resolved callees.
- LCOM1 is disjoint pair count; LCOM2 is floored disjoint-minus-sharing count;
  LCOM3 is field-sharing component count; LCOM4 adds internal calls; LCOM5 is
  the Henderson-Sellers normalization.
- Function metrics classify `CpgNodeKind`, not source tokens. Nested functions
  are separate scopes.
- Maintainability uses pgmcp's raw SEI formula and clamps directly to
  $`[0,100]`.

`CkOptions` includes classes always, structs by default, enums by caller choice,
and `Implements` inheritance by caller choice. Traits remain endpoints but do
not emit concrete class rows.

### Make boundaries total and deterministic

Use iterative finite traversals and ordered sets/maps. Sort all public vectors
by `NodeId`. Collapse inheritance SCCs before computing depth. Use saturating
integer counts and defined zero-denominator cases. A missing function is an
`AnalysisError`; an invalid nominal selector yields an empty evidence graph.

![Both metric families consume normalized CPG evidence and retain their policy boundaries.](../diagrams/code-metrics-pipeline.svg)

*Figure — accepted data flow. Source:
[`code-metrics-pipeline.puml`](../diagrams/code-metrics-pipeline.puml).*

## Literate algorithm

```text
for each selected nominal type C in stable ID order:
    G = method_field_graph(C)
    weight each method with semantic cyclomatic complexity
    condense inheritance cycles and read C's longest root distance
    collect immediate children, resolved call owners, and direct callees
    derive five named LCOM variants from G
    emit one ClassMetrics row

for one requested function f:
    reject missing or non-function selectors
    walk f's semantic AST scope, stopping at nested functions
    count decisions, nesting increments, operator keys, and operand keys
    merge comment source-line intervals clipped to f
    evaluate Halstead and MI formulae with explicit empty cases
```

## Consequences

- Cohesion values are traceable to concrete access nodes and declarations.
- Call-dependent CBO, RFC, and LCOM4 improve with resolver coverage without an
  API change.
- Results work under `default = []`; parser features are only needed to build a
  CPG from source.
- Semantic Halstead counts are stable across frontends that produce the same CPG
  shape, but intentionally differ from token-level counts that include syntax
  discarded by the CPG.
- The full CK implementation favors a simple repeated ownership scan. It is
  polynomial and finite but not yet benchmarked; an indexed optimization must
  preserve exact output and be measurement-driven.
- All metric values are advisory quality signals. The implementation does not
  label a class defective or automatically recommend refactoring.

## Alternatives rejected

### Reuse pattern-detector metrics

Rejected because that module is feature-gated, aggregates values across classes,
matches field spelling, and does not expose a real method-field graph or the
specified variants.

### Reparse retained source

Rejected because source may not be retained, parser availability is feature
dependent, and a second parser/classifier would drift from the CPG that other
analyses consume.

### Infer unresolved member and call targets

Rejected because a plausible name is not declaration evidence. False
method-field or class-coupling edges would make exact scalar arithmetic describe
the wrong graph.

### Follow inheritance walks directly

Rejected because malformed or generated CPGs can contain cycles. SCC
condensation supplies a deterministic finite quotient and preserves acyclic
depth semantics.

### Return only scalar metrics

Rejected because users could not audit LCOM or distinguish missing resolution
from a truly field-free method.

## Validation and citations

[Scientific validation 24](../scientific/24-code-metrics-validation.md) records
exact CK/LCOM, formula, parser, serde, negative, and insertion-order experiments.
The definitions use McCabe
([DOI 10.1109/TSE.1976.233837](https://doi.org/10.1109/TSE.1976.233837)),
Chidamber–Kemerer
([1991 DOI](https://doi.org/10.1145/117954.117970),
[1994 DOI](https://doi.org/10.1109/32.295895)), Li–Henry
([DOI 10.1016/0164-1212(93)90077-B](https://doi.org/10.1016/0164-1212(93)90077-B)),
Campbell
([DOI 10.1145/3194164.3194186](https://doi.org/10.1145/3194164.3194186)),
Oman–Hagemeister
([DOI 10.1109/ICSM.1992.242525](https://doi.org/10.1109/ICSM.1992.242525)),
and Coleman et al.
([DOI 10.1109/2.303623](https://doi.org/10.1109/2.303623)). Hitz–Montazeri,
Henderson-Sellers, and Halstead are cited through their no-DOI paper/book
records in the [component reference list](../components/graph/code-metrics.md#references).
