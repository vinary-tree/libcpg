# Semantic expression-flow API reference

The feature-free API provides proof-qualified CSE/PRE evidence and
cross-revision semantic comparison. Cargo feature `serde` adds report
serialization; it does not turn persisted proof bytes into live authority.

## Analyze one function

```rust,ignore
use libcpg::analysis::{
    analyze_semantic_expression_flow, ExpressionEquivalenceCandidate,
    SemanticExpressionFlowLimits,
};

let candidates = roots
    .iter()
    .zip(&expression_irs)
    .map(|(&source, ir)| {
        ExpressionEquivalenceCandidate::new(source, function, ir)
    })
    .collect::<Vec<_>>();

let report = analyze_semantic_expression_flow(
    &cpg,
    function,
    analysis_domain,
    &profile,
    &candidates,
    SemanticExpressionFlowLimits::default(),
)?;
```

`function` selects the basic-block CFG. `analysis_domain` qualifies the nested
equality report; it does not replace the candidate binding context.

## Interpret completion first

| Completion | Meaning |
|---|---|
| `Complete` | equality, both must fixed points, and candidate checks completed |
| `UnsupportedInputs` | admitted inputs completed, but at least one candidate was explicitly excluded |
| `EquivalenceLimited(reason)` | exact equality/provenance/pair construction was limited |
| `AvailableDataflowLimited` | the forward visit cap prevented a fixed point |
| `VeryBusyDataflowLimited` | the backward visit cap prevented a fixed point |
| `CandidateWorkLimited` | dominance/path qualification exhausted its work cap |

Positive entries remain proof-qualified when aggregate completion is limited.
An absent CSE or PRE candidate is conclusive only when `completion.is_complete()`.
`available_completion` and `very_busy_completion` expose the two dataflow
dimensions separately.

## Read classes and candidates

`classes` are ordered by their smallest stable source id. `occurrences` and
resolved `dependencies` are canonical. `available_in[node]` contains class
representatives available immediately before `node`; `busy_out[node]`
contains representatives very busy immediately after it.

For each `SemanticCseCandidate`, `producer` dominates `use_node`, and
`proof_pair` indexes an exact independently replayable pair in
`report.equivalence.pairs`. For each `SemanticPreCandidate`, `placement` is the
nearest common dominator accepted by the must-anticipability check, and
`proof_pairs` spans `occurrences`. Neither type instructs a source rewrite.

`excluded` distinguishes equality-boundary reasons from a source outside the
reachable selected CFG. `metrics` reports semantic classes, occurrences,
resolved dependencies, charged candidate work, and emitted candidate counts.

## Configure independent limits

`SemanticExpressionFlowLimits` contains:

- `equivalence`, the complete `ExpressionEquivalenceLimits` policy;
- `max_visits_per_block`, a nonzero cap for each flow direction; and
- `max_candidate_work`, a nonzero path/report budget no greater than
  `MAX_SEMANTIC_FLOW_WORK`.

Do not collapse these into one budget: each completion value identifies a
different missing-evidence boundary.

## Compare two revisions

```rust,ignore
use libcpg::analysis::{
    analyze_semantic_expression_diff, ExpressionEquivalenceCandidate,
    SemanticExpressionDiffLimits, SemanticExpressionDiffStatus,
};

let diff = analyze_semantic_expression_diff(
    &profile,
    ExpressionEquivalenceCandidate::new(old_root, old_context, &old_ir),
    ExpressionEquivalenceCandidate::new(new_root, new_context, &new_ir),
    SemanticExpressionDiffLimits::default(),
)?;

match diff.status {
    SemanticExpressionDiffStatus::ProvenEquivalent => diff
        .proof
        .as_ref()
        .expect("equivalence has a proof")
        .replay()?,
    SemanticExpressionDiffStatus::ProvenDifferent => {
        assert_ne!(diff.old_value, diff.new_value);
    }
    SemanticExpressionDiffStatus::Limited => {
        assert!(!diff.limitations.is_empty());
    }
    SemanticExpressionDiffStatus::Unknown => {
        assert!(diff.limitations.is_empty());
    }
}
```

`old_root` and `new_root` belong to independent coordinate systems and may be
numerically equal. The public report preserves both original ids. A
`ProvenDifferent` result requires two exact unequal closed typed values; lack
of an equality proof is never enough.

`SemanticExpressionDiffLimits` separates equality limits from a nonzero
per-revision iterative evaluation cap, bounded by
`MAX_SEMANTIC_DIFF_EVALUATION_WORK`.

## Error contract

Invalid limits fail before analysis. Structural analysis and proof failures
remain typed. `ContradictoryEvidence` means a replayed equality proof disagreed
with exact closed evaluation; consumers must fail closed and report the
internal inconsistency.

See [Usage 44](../usage/44-semantic-expression-flow.md),
[Security 34](../security/34-semantic-expression-flow-boundary.md), and
[ADR-0076](../design/0076-proof-qualified-semantic-expression-flow.md).
