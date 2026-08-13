# ADR-0030 — Bounded Function Clone Detection

- **Status:** Accepted
- **Date:** 2026-08-13
- **Decision owners:** `libcpg` maintainers
- **Scope:** feature-free `pattern::detect_clones`

## Context

The crate already contains a fixed WL combiner, function-scoped PDG
projections, and a corrected relaxed VF2 implementation. Function clone
detection should compose those components rather than introduce another parser,
graph model, or isomorphism engine. It must classify Type-1 through Type-4,
remain reproducible under stable-identifier relabeling, and make exponential
work visible to callers.

## Decision

Use ordered bottom-up AST Merkle fingerprints for Type-1 and Type-2, a
kind-and-name multiset Jaccard relation for Type-3, and PDG WL cosine followed
by the existing relaxed VF2 matcher for Type-4. Share `stable_wl_mix` with the
WL analysis instead of using `DefaultHasher`.

Classify each function pair by the strongest applicable relation. Require an
equal feature bag before Type-4 so reordered independent statements reach the
semantic verifier while inserted statements remain Type-3. Group same-type
pair edges into deterministic connected components.

Reject invalid option values. Return `BudgetExceeded` before omitting any
required Type-3 comparison, any PDG above the caller's at-most-300-node limit,
or any VF2 invocation above its budget. Configure VF2 for relaxed node/edge
compatibility and `with_max_matches(1)` because verification is existential.

![The accepted design reuses AST, PDG, WL, and VF2 components behind explicit budget gates.](../diagrams/clone-detection-pipeline.svg)

*Figure — accepted clone-evidence pipeline. Source:
[`clone-detection-pipeline.puml`](../diagrams/clone-detection-pipeline.puml).*

## Literate algorithm

```text
extract stable function roots
for each function:
    iteratively fingerprint its ordered non-comment AST twice
    count its kind-and-name feature bag

for each stable pair:
    accept equal exact fingerprints as Type-1
    otherwise accept equal abstract fingerprints as Type-2
    otherwise apply exact size-ratio buckets and consume candidate-pair budget
    if bags are equal:
        construct bounded PDGs and compare WL-label cosine
        consume VF2 budget and verify a relaxed full-size embedding
    otherwise accept Jaccard above threshold as Type-3

group pair edges by type and emit canonical connected components
```

## Consequences

- The API works with `default = []` and any caller-built CPG.
- Results depend on CPG evidence, not parser availability or retained source.
- Node IDs and hash-map iteration never influence fingerprints or scores.
- Comment changes do not prevent Type-1; nested functions form separate units.
- Type-4 precision depends on PDG completeness and relaxed VF2 semantics.
- The hard limits preserve termination, while typed errors prevent partial
  findings from being mistaken for a complete census.
- An oracle-guarded Criterion target exercises Type-1 equality, Type-3
  insertion, and Type-4 reordering at two corpus sizes. Hard caps remain the
  safety contract; benchmark samples are measurements, not semantic evidence.

## Alternatives rejected

### Reparse source or hand-roll a language parser

Rejected because it would be feature-dependent, duplicate frontend logic, and
diverge from the CPG evidence used by every other analysis.

### Use `DefaultHasher`

Rejected because its algorithm is not a stable cross-release contract. The
existing fixed combiner already has determinism and permutation tests.

### Treat WL equality as Type-4 proof

Rejected because one-dimensional WL is not a complete graph-isomorphism test.
WL is a prefilter; VF2 is the verifier.

### Run VF2 on complete CPG subgraphs

Rejected because AST order and unrelated CFG/DFG edges would contaminate a
PDG-semantic comparison. The verifier receives a pure PDG topology copy.

### Return partial classes at a cap

Rejected because a caller could mistake incomplete output for absence of
clones. Typed failure keeps completeness observable.

## Validation

[Scientific validation 25](../scientific/25-function-clone-validation.md)
records example, integration, property, serde, malformed-input, and resource
experiments. The taxonomy and graph-based rationale are grounded in the
Bellon, Baxter, Zou, and Komondoor–Horwitz studies cited by
[Theory 23](../theory/23-function-clone-detection.md#references).
