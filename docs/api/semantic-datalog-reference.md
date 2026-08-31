# Semantic Datalog API reference

This feature-free API converts a borrowed proof-qualified semantic flow report
into owned canonical Datalog extensional-database (EDB) facts. Cargo feature
`serde` adds deterministic serialization; facts cannot be deserialized into
equality-proof authority.

## Export

```rust,ignore
use libcpg::analysis::{export_semantic_datalog, SemanticDatalogLimits};

let facts = export_semantic_datalog(
    &semantic_flow_report,
    SemanticDatalogLimits::default(),
)?;
```

The function validates source identity, replays exact proof edges, validates
CSE/PRE proof references, copies completion, and retains no report reference.

## Inspect typed rows

`SemanticDatalogFacts` exposes immutable slices through `class_members()`,
`proof_edges()`, `available()`, `anticipable()`, `cse_candidates()`,
`pre_candidates()`, and `completions()`. `fact_count()` returns their total;
`limits()` returns the policy used to construct and render the bundle.

Fact structs are copyable, totally ordered, hashable, and serializable when
`serde` is enabled. Bundle fields remain private so callers cannot forge a
bundle that appears boundary-validated.

## Interpret combined completion

```rust,ignore
use libcpg::analysis::{
    DatalogEvaluationCompletion, SemanticDatalogDimension,
};

let completion = facts.query_completion(
    SemanticDatalogDimension::CseCandidates,
    DatalogEvaluationCompletion::Complete,
);

if completion.can_conclude_absence() {
    // Both prerequisite fixed points completed.
}
```

`SemanticDatalogDimension` distinguishes exact equality, availability,
anticipability, CSE candidates, and PRE candidates.
`SemanticDatalogCompletion` preserves all semantic limiting causes.
`DatalogEvaluationCompletion` preserves work, iteration, memory, time,
cancellation, and unknown engine outcomes. Only `Complete`/`Complete` makes
`is_complete()` or `can_conclude_absence()` true.

## Render Soufflé buffers

```rust,ignore
for file in facts.souffle_files()? {
    // The caller chooses an approved directory and atomic write policy.
    write_atomically(approved_directory, file.relation, &file.contents)?;
}
```

Rendering performs no filesystem access. Numeric fields are decimal unsigned
integers. Completion and dimension fields are double-quoted RFC 4180 symbols.
The caller declares matching Soufflé relations with
`rfc4180=true, delimiter="\t"`.

## Limits

| Field | Default | Hard ceiling | Enforcement |
|---|---:|---:|---|
| `max_facts` | 8,388,608 | `MAX_SEMANTIC_DATALOG_FACTS` = 67,108,864 | before large construction and after canonicalization |
| `max_rendered_bytes` | 268,435,456 | `MAX_SEMANTIC_DATALOG_BYTES` = 1,073,741,824 | checked encoded-length pass before rendering |

Zero and above-ceiling values return `InvalidLimit`. Excess rows return
`FactLimit`; excess bytes return `RenderedByteLimit`; checked arithmetic
overflow returns `SizeOverflow`.

## Error contract

`SemanticDatalogError::Proof` preserves replay failure. `Invariant` rejects
malformed classes, cross-class proof edges, unknown point classes, mismatched
CSE proof endpoints, non-spanning PRE proofs, and proof index overflow. Errors
never return a partial bundle.

See the [component contract](../components/graph/semantic-datalog-boundary.md),
[Usage 45](../usage/45-semantic-datalog-export.md), and
[ADR-0077](../design/0077-owned-semantic-datalog-evidence.md).
