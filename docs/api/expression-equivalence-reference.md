# Expression-equivalence API reference

The expression-equivalence API is feature-free. Cargo feature `serde` adds
serialization for reports and proofs, but deliberately does not add proof
deserialization.

## Analyze candidates

Lower each root through the live typed expression-IR boundary first:

```rust,ignore
use libcpg::analysis::{
    analyze_expression_equivalence, ExpressionEquivalenceCandidate,
    ExpressionEquivalenceLimits,
};

let candidates = [
    ExpressionEquivalenceCandidate::new(left_root, function_id, &left_ir),
    ExpressionEquivalenceCandidate::new(right_root, function_id, &right_ir),
];
let report = analyze_expression_equivalence(
    module_id,
    &profile,
    &candidates,
    ExpressionEquivalenceLimits::default(),
)?;
```

`analysis_domain` is a caller-selected function, module, revision, or program
identity used to qualify the report. `binding_context` is the exact lexical
namespace used by the exact pass. They are intentionally separate.

## Interpret completion before absence

| Completion | Meaning |
|---|---|
| `Complete` | both exact and alpha passes reached fixed points with complete proofs and pairs |
| `UnsupportedInputs` | supported candidates completed, but at least one input was explicitly excluded |
| `CandidateLimited` | the request exceeded the candidate ceiling before graph construction |
| `InputNodeLimited` | admitted flat IR nodes exceeded the request ceiling |
| `NodeLimited` | Replete refused the next e-node or child edge |
| `WorkLimited` | adapter or Replete logical work was exhausted |
| `MatchLimited` | further rewrite matches existed beyond the cap |
| `IterationLimited` | the next fixed-point iteration could not begin |
| `PairLimited` | `pairs` is a deterministic positive prefix |
| `ProofLimited` | semantic equality may exist, but provenance or proof construction is incomplete |

Inspect `exact_completion` when asking about same-context equality and
`alpha_completion` when asking about qualified cross-context clones. Absence
from a limited pass is inconclusive.

## Interpret pairs

`ExpressionEquivalenceKind::Exact` means equality in the configured typed
equational model and one binding context. It does not mean whole-program
behavioral equivalence.

`AlphaRenamedType4Clone` means equality after a reported bijective renaming of
resolved bindings from distinct contexts. `alpha_bindings` contains canonical
slots and both concrete `BindingIdentity` values. This is semantic Type-4 clone
evidence, not a replacement for `pattern::CloneType::Type4`.

Every pair is canonical: `left < right`. Pairs are sorted by source IDs and
kind, contain no duplicates, repeat the analysis domain and both binding
contexts, and carry the pass completion that qualified them.

## Replay a proof

```rust,ignore
for pair in &report.pairs {
    pair.proof.replay()?;
    assert!(pair.proof.encoded_len() > 0);
}
```

Replay validates Replete's identity, symmetry, transitivity, and congruence
nodes, then delegates named atomic laws to libcpg's typed verifier. AC leaves
and nested closed evaluation are processed with explicit heap worklists.

## Unsupported candidates

| Reason | Required response |
|---|---|
| `DuplicateSource` | deduplicate stable source roots |
| `ProfileMismatch` | lower and analyze with the same profile |
| `IncompleteLowering(completion)` | inspect and resolve the lowering outcome |
| `PersistedArtifact` | re-lower against live CPG and frontend facts |
| `RootMismatch` | make the candidate source equal the IR root source |
| `NonPureEvidence` | retain identity evidence; do not request algebraic equality |

Unsupported candidates never enter either e-graph.

## Limits

`ExpressionEquivalenceLimits` bounds candidates, input nodes, graph nodes and
children, arity, exact key bytes, parent links, logical work, matches,
iterations, public pairs, proof terms and edges, proof depth, evidence bytes,
and encoded proof bytes. Candidate, input, pair, match, and iteration fields
also have public hard ceilings named `MAX_EQUIVALENCE_*`.

Graph and byte limits that Replete requires structurally must be positive.
Operational caps such as work, matches, iterations, pairs, and proof-node
counts may be zero to request a typed exhaustion result.

## Persistence

With `serde`, reports and proofs implement `Serialize`. Proofs do not implement
`DeserializeOwned`, because persisted bytes cannot recreate live rewrite
authority. The operator dictionary serializes as a canonical ordered sequence,
so JSON map-key restrictions do not affect the format.

See [Usage 43](../usage/43-expression-equivalence.md),
[Security 33](../security/33-expression-equivalence-boundary.md), and
[ADR-0075](../design/0075-proof-producing-expression-equivalence.md).
