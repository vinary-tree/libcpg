# Use proof-qualified semantic expression flow

This guide begins with one immutable Code Property Graph (CPG), its selected
function, a frontend `SemanticProfile`, and live, complete, pure
`ExpressionIr` roots. Follow the expression-IR and expression-equivalence
guides first if those terms are unfamiliar.

## 1. Prepare exact candidates

Use the selected function as each candidate's binding context unless a more
precise context identity is required. Candidate roots must be reachable in the
function CFG.

```rust,ignore
use libcpg::analysis::{
    analyze_semantic_expression_flow, ExpressionEquivalenceCandidate,
    SemanticExpressionFlowLimits,
};

let candidates = expression_irs
    .iter()
    .map(|(source, ir)| {
        ExpressionEquivalenceCandidate::new(*source, function, ir)
    })
    .collect::<Vec<_>>();

let report = analyze_semantic_expression_flow(
    &cpg,
    function,
    function,
    &profile,
    &candidates,
    SemanticExpressionFlowLimits::default(),
)?;
```

The analysis groups candidates only through exact replayed proofs. Qualified
alpha-renaming remains clone evidence and does not become a reusable value in
the same execution.

## 2. Preserve completion and exclusions

```rust,ignore
if !report.completion.is_complete() {
    // Retain the exact completion and exclusions. Positive candidates remain
    // valid, but an absent candidate is not a negative conclusion.
}
```

An unsupported input, limited equality pass, forward/backward visit cap, and
candidate path-work cap describe different evidence gaps. Do not reduce them
to one Boolean.

## 3. Consume CSE evidence conservatively

```rust,ignore
for candidate in &report.cse_candidates {
    let pair = &report.equivalence.pairs[candidate.proof_pair];
    pair.proof.replay()?;
    assert_eq!(pair.kind, libcpg::analysis::ExpressionEquivalenceKind::Exact);
    // candidate.producer dominates candidate.use_node and no represented
    // intervening kill was found. A later optimizer must still validate its
    // own target-language and source-rewrite obligations.
}
```

Availability means a class was generated on every path. The additional
dominating-producer field matters: two diamond arms can make a class available
at their join without either arm being a reusable producer.

## 4. Consume PRE evidence conservatively

Each PRE entry identifies a semantic class, a nearest common-dominator
placement, dominated occurrences, and proof edges spanning them. It means the
expression is very busy at that represented placement under conservative
kills. It does not assert that insertion, deletion, exception motion, register
pressure, code size, or runtime profitability is acceptable.

Treat unknown calls, unresolved stores, `await`, and `yield` as barriers. Do
not delete these barriers in a consumer merely because an expression appears
pure by syntax.

## 5. Add a semantic view to structural diff

Run the existing structural `cpg_diff` independently. Then compare selected
old and new expression roots:

```rust,ignore
use libcpg::analysis::{
    analyze_semantic_expression_diff, ExpressionEquivalenceCandidate,
    SemanticExpressionDiffLimits, SemanticExpressionDiffStatus,
};

let semantic = analyze_semantic_expression_diff(
    &profile,
    ExpressionEquivalenceCandidate::new(old_root, old_function, &old_ir),
    ExpressionEquivalenceCandidate::new(new_root, new_function, &new_ir),
    SemanticExpressionDiffLimits::default(),
)?;

match semantic.status {
    SemanticExpressionDiffStatus::ProvenEquivalent => {
        semantic.proof.as_ref().expect("proof is present").replay()?;
    }
    SemanticExpressionDiffStatus::ProvenDifferent => {
        // Both exact closed values are present and unequal.
    }
    SemanticExpressionDiffStatus::Limited => {
        // Retry under an approved larger policy or preserve inconclusiveness.
    }
    SemanticExpressionDiffStatus::Unknown => {
        // Complete checked evidence decided neither relation.
    }
}
```

The revisions may both use `NodeId::new(0)` for their root. Numeric equality
across revisions has no semantic meaning; private slots isolate the coordinate
systems. The analysis neither merges nor mutates the CPGs.

Under strict IEEE JavaScript and TypeScript profiles, an integer-looking
`1 + 2` expression may not receive a native integer-fold certificate. A
comparison with literal `3` can therefore be `Unknown`. Preserve that result;
do not substitute host-language arithmetic.

## 6. Keep depth and resources bounded

Choose equality, block-visit, candidate-work, and evaluator-work limits from
deployment policy. Run untrusted large analyses inside external resident-memory,
swap, and time limits as well. Consumers should use the flat public reports and
must not recursively traverse expression or proof arenas.

The production path and its clone, formatting, serialization, and drop
lifecycle are tested at 100,000 nodes on a 256 KiB worker stack.

## Repository boundary

This workflow uses standalone Replete through libcpg. It does not modify,
build, format, branch, or commit MeTTaIL. MeTTaIL may be consulted only as an
immutable provenance oracle until its separately tracked E3 refactor receives
permission.

## Related documentation

- [API reference](../api/semantic-expression-flow-reference.md)
- [Component contract](../components/graph/semantic-expression-flow.md)
- [ADR-0076](../design/0076-proof-qualified-semantic-expression-flow.md)
- [Security boundary](../security/34-semantic-expression-flow-boundary.md)
- [Scientific validation](../scientific/71-semantic-expression-flow-validation.md)
- [Formal plan](../verification/t4-3-semantic-expression-flow-formal-plan.md)

## Reference

Morel, E., Renvoise, C. (1979). *Global Optimization by Suppression of Partial
Redundancies.* DOI:
[10.1145/359060.359069](https://doi.org/10.1145/359060.359069).
