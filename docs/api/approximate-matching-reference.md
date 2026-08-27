# Approximate-matching API reference

Enable Cargo feature `approximate-matching`. Add `serde` when reports must be
serialized. The live `ApproximateTokenIndex` is intentionally not serializable;
its canonical reports are.

## Records and tapes

```rust,ignore
pub enum ApproximateTape {
    Text(Arc<str>),
    Tokens(Vec<Arc<str>>),
}

pub struct ApproximateRecord {
    pub id: ApproximateId,
    pub tape: Arc<ApproximateTape>,
}
```

Construct tapes with `ApproximateTape::text` or `ApproximateTape::tokens` and
records with `ApproximateRecord::new`. `ApproximateId` is the public total-order
tie-breaker and must be unique within one candidate corpus.

## Methods

| `ApproximateMethod` | Unit | Cost semantics |
|---|---|---|
| `StandardLevenshtein` | Unicode scalar or typed token | insertion, deletion, substitution |
| `OptimalStringAlignment` | Unicode scalar or typed token | standard operations plus non-overlapping adjacent transposition |
| `DamerauLevenshtein` | Unicode scalar or typed token | unrestricted adjacent transposition within the supported bound |
| `GeneralizedMergeSplit` | Unicode scalar or typed token | rational-cost generalized merge/split operation set |

The method determines `ApproximateScoreSemantics`. Evidence also records the
tropical min-plus semiring, exact rational raw/normalized costs, configured
distance bound, and method identity.

## Configuration and limits

```rust,ignore
let config = libcpg::ApproximateConfig {
    method: libcpg::ApproximateMethod::StandardLevenshtein,
    max_distance: 2,
    top_k: 32,
    limits: libcpg::ApproximateLimits {
        max_candidates: 10_000,
        max_input_units: 4_096,
        max_pair_cells: 2_000_000,
        max_work_units: 20_000_000,
        max_state_count: 2_000_000,
        max_memory_bytes: 128 * 1024 * 1024,
        max_unique_tokens: 50_000,
    },
};
```

Every limit must be nonzero. Checked arithmetic converts an unrepresentable
size into `ApproximateError`; it is never allowed to wrap into a smaller budget.

## One-shot search

```rust,ignore
pub fn approximate_candidates(
    query: &ApproximateRecord,
    candidates: &[ApproximateRecord],
    consumer: ApproximateConsumer,
    config: &ApproximateConfig,
    cancellation: &ApproximateCancellation,
) -> Result<ApproximateReport, ApproximateError>;
```

All tapes in one call must have the same domain (`Text` or `Tokens`). Mixed
tapes return `TapeDomainMismatch`. Cooperative cancellation is observed before
each candidate and at bounded operational boundaries.

## Reusable token index

```rust,ignore
let index = libcpg::ApproximateTokenIndex::build(&candidates, &config.limits)?;
let build_state = index.completion();
let census = index.counters();
let optional_id = index.token_id("CALL");
let optional_term = optional_id.and_then(|id| index.token(id));
let report = index.search(
    &query,
    libcpg::ApproximateConsumer::ApiSequenceNeighbor,
    &config,
    &libcpg::ApproximateCancellation::default(),
)?;
# Ok::<(), libcpg::ApproximateError>(())
```

`build_state` must be examined when a service needs a complete corpus. Search
also preserves it in `report.completion`; the caller cannot accidentally turn
a partial index into complete negative evidence.

## Report interpretation

`ApproximateReport` contains canonical `evidence`, one `completion` value,
shared replayable `provenance`, and resource `counters`. Each evidence record
names an `ExactConfirmation` predicate selected by its
`ApproximateConsumer`. A caller may display advisory navigation results, but it
must execute that exact predicate before making a semantic claim.

See [Usage 41](../usage/41-bounded-approximate-matching.md) for complete examples
and [Security 31](../security/31-approximate-evidence-boundary.md) for the trust
boundary.
