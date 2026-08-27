# Usage 41 -- Bounded approximate matching

Enable feature `approximate-matching`. These APIs consume caller-owned immutable
records; they do not require a language grammar or mutate a Code Property Graph
(CPG).

## Run a one-shot text search

```rust,ignore
use libcpg::{
    approximate_candidates, ApproximateCancellation, ApproximateConfig,
    ApproximateConsumer, ApproximateId, ApproximateRecord, ApproximateTape,
};

let query = ApproximateRecord::new(
    ApproximateId::External { corpus: 1, item: 0 },
    ApproximateTape::text("read_flie"),
);
let candidates = [
    ApproximateRecord::new(
        ApproximateId::External { corpus: 1, item: 1 },
        ApproximateTape::text("read_file"),
    ),
    ApproximateRecord::new(
        ApproximateId::External { corpus: 1, item: 2 },
        ApproximateTape::text("write_file"),
    ),
];
let report = approximate_candidates(
    &query,
    &candidates,
    ApproximateConsumer::DeveloperNavigation,
    &ApproximateConfig::default(),
    &ApproximateCancellation::default(),
)?;
for evidence in report.evidence {
    println!("{:?}: {}/{}", evidence.candidate,
             evidence.raw_cost.numerator, evidence.raw_cost.denominator);
}
# Ok::<(), libcpg::ApproximateError>(())
```

This ranks spellings. It does not resolve a symbol; the consumer's recorded
confirmation is still an exact symbol lookup or user selection.

## Reuse a structural-sequence index

Use one token per language structure. Tokens need not be characters and may
contain arbitrary Unicode text.

```rust,ignore
use libcpg::{
    ApproximateCancellation, ApproximateCompletion, ApproximateConfig,
    ApproximateConsumer, ApproximateId, ApproximateRecord, ApproximateTape,
    ApproximateTokenIndex,
};

let sequence = |item, terms: &[&str]| ApproximateRecord::new(
    ApproximateId::External { corpus: 7, item },
    ApproximateTape::tokens(terms.iter().copied()),
);
let candidates = vec![
    sequence(1, &["OPEN", "READ", "CLOSE"]),
    sequence(2, &["OPEN", "WRITE", "CLOSE"]),
    sequence(3, &["OPEN", "READ", "CLOSE"]),
];
let query = sequence(0, &["OPEN", "READ"]);
let config = ApproximateConfig::default();
let index = ApproximateTokenIndex::build(&candidates, &config.limits)?;
assert_eq!(index.completion(), &ApproximateCompletion::Complete);
assert_eq!(index.counters().indexed_candidates, 3);
assert_eq!(index.counters().indexed_sequences, 2);

let open_id = index.token_id("OPEN").expect("indexed term");
assert_eq!(index.token(open_id).as_deref(), Some("OPEN"));
let report = index.search(
    &query,
    ApproximateConsumer::ApiSequenceNeighbor,
    &config,
    &ApproximateCancellation::default(),
)?;
assert_eq!(report.completion, ApproximateCompletion::Complete);
# Ok::<(), libcpg::ApproximateError>(())
```

Candidates 1 and 3 share a token sequence but remain two results because the
second dictionary stores every stable candidate slot for a sequence.

## Compose typed rewrites before indexing

For a finite lexical normalization pipeline, convert duallity rules into a
typed libcpg relation, compose it with a structural relation, and require a
complete observation before building sequences:

```rust,ignore
use libcpg::{
    ApiTapeDomain, ApproximateCancellation, ApproximateLimits,
    FiniteRewriteRule, NormalizedTapeDomain, SymbolTapeDomain,
    TypedFiniteRelation,
};

let limits = ApproximateLimits::default();
let lexical_rules = [
    duallity::RewriteRule::with_cost("fs.open", "file.open", 0.0)?,
    duallity::RewriteRule::with_cost("std.open", "file.open", 0.0)?,
];
let lexical =
    TypedFiniteRelation::<SymbolTapeDomain, NormalizedTapeDomain>
        ::from_duallity_rules(&lexical_rules, &limits)?;
let structural =
    TypedFiniteRelation::<NormalizedTapeDomain, ApiTapeDomain>::from_rules(
        &[FiniteRewriteRule::new("file.open", "OPEN", 0)],
        &limits,
    )?;
let complete = lexical
    .compose(&structural, &limits)?
    .observe(&ApproximateCancellation::default())?
    .into_complete()
    .expect("do not treat a limited relation as complete");
assert_eq!(complete.paths().len(), 2);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Reassociation is sound for complete extensional relations. It is not promised
for truncated observations.

## Interpret limits safely

Always branch on `report.completion`. `Complete` plus an empty evidence vector
is a complete no-match. `Incomplete(reasons)` plus an empty vector is unknown;
retry only under an explicitly revised policy, not by silently increasing
resources. Persist `provenance`, limits, counters, and the exact-confirmation
identity with any evidence sent across a process boundary.

## Related documentation

- [Component contract](../components/graph/approximate-matching.md)
- [API reference](../api/approximate-matching-reference.md)
- [Theory 56](../theory/56-weighted-relations-and-approximate-matching.md)
- [ADR-0073](../design/0073-bounded-composable-approximate-evidence.md)
- [Security boundary](../security/31-approximate-evidence-boundary.md)
- [Scientific validation](../scientific/68-approximate-automata-validation.md)
