# Usage 22 — Cryptographic API-misuse analysis

This guide runs the feature-free built-in policy, interprets findings, extends
rules, configures caps, and validates the public surfaces.

## 1. Run the example

```bash
cargo run --no-default-features --example crypto_misuse
```

The example builds a tiny typed CPG containing a literal key passed to
`Cipher::with_key`. It prints a complete report with one advisory
`HardcodedKey` finding. The literal value is not copied into the finding.

## 2. Analyze an existing CPG

```rust,ignore
use libcpg::{
    analyze_builtin_crypto, CallGraphResolution, CodePropertyGraph,
    CryptoAnalysisConfig,
};

fn inspect(cpg: &CodePropertyGraph, resolution: &CallGraphResolution) {
    let report = analyze_builtin_crypto(
        cpg,
        resolution,
        &CryptoAnalysisConfig::default(),
    );

    for finding in &report.findings {
        println!(
            "{:?} at {:?}, rule {}, argument {:?}",
            finding.kind, finding.site, finding.rule, finding.argument_index
        );
    }

    if !report.is_complete() {
        eprintln!("crypto issues: {:#?}", report.issues);
        eprintln!("typestate qualification: {:#?}", report.typestate);
        eprintln!("taint qualification: {:#?}", report.taint);
    }
}
```

Pass the call resolution produced for the same program. The default empty
resolution is useful only when typed callee syntax and local graph relations
already contain sufficient evidence.

## 3. Interpret evidence

| Variant | Meaning |
|---|---|
| `CallPath` | a reconstructed or resolved callee matched a direct weak-hash rule |
| `LiteralOrigins` | typed literal nodes reach a governed algorithm, IV, or nonce argument |
| `Taint` | a categorical literal source flows through AST/DFG/summary evidence to a key argument |
| `Typestate` | the configured property observed a forbidden object-event transition |

`confidence_milli` is a deterministic taint evidence score in `0..=1000`; it
is not probability or severity.

## 4. Extend the policy

```rust,ignore
use libcpg::{
    analyze_crypto_misuse, builtin_crypto_rules, CryptoArgumentRule, Pattern,
};

let mut rules = builtin_crypto_rules();
rules.key_parameters.push(
    CryptoArgumentRule::new("project-key-loader")
        .with_call(Pattern::Exact("project::Cipher::from_key".into()))
        .with_argument(0),
);
rules.secure_random_sources.push(
    Pattern::Exact("project::entropy::nonce".into()),
);
```

Then pass `&rules` to `analyze_crypto_misuse`. Add both a misuse fixture and a
clean fixture for every policy extension. Prefer exact paths. Broad substring
patterns expand the admitted API family and may increase false positives.

## 5. Configure resource policy

```rust,ignore
use libcpg::CryptoAnalysisConfig;

let config = CryptoAnalysisConfig {
    max_calls: 50_000,
    max_origin_nodes_per_argument: 200_000,
    ..CryptoAnalysisConfig::default()
};
```

Values are clamped by `MAX_CRYPTO_CALLS` and
`MAX_CRYPTO_ORIGIN_NODES`. Smaller values are valid service budgets, but a cap
event makes the report incomplete. Configure `ifds_caps` separately for the
delegated taint and typestate engines.

## 6. Handle incomplete reports

| Issue/evidence | Action |
|---|---|
| `CallsCapped` | inspect call population and approve a bounded increase if safe |
| `OriginNodesCapped` | inspect alias/DFG fan-in and repair malformed cycles or raise the explicit bound |
| `CalleePathCapped` | repair cyclic/deep callee syntax or raise the delegated path depth deliberately |
| `MissingCallPattern` | add at least one reviewed path alternative |
| `MissingArgumentIndex` | specify the semantic parameter position |
| `EmptyPattern` | replace the matcher with a nonempty exact/segment/substring pattern |
| delegated issue | inspect `report.typestate` or `report.taint`; do not discard it |

Do not increase a cap simply to remove an issue. First establish whether the
graph and rule are valid and whether the larger work envelope is acceptable.

## 7. Serialize policy and reports

Enable `serde`:

```bash
cargo test --no-default-features --features serde \
  analysis::security::crypto::tests::rules_and_analysis_round_trip_canonically
```

Serialize the aggregate `CryptoAnalysis`, not `findings` alone. Apply source
repository access control to the output even though literal values are omitted.

## 8. Focused validation

```bash
# Feature-free unit, property, and 100,000-depth small-stack tests
cargo test --no-default-features analysis::security::crypto

# Public API
cargo test --no-default-features --test crypto_misuse_integration

# Rust Tree-sitter pipeline
cargo test --no-default-features --features lang-rust \
  --test crypto_misuse_integration

# Optional serialization
cargo test --no-default-features --features serde \
  analysis::security::crypto
```

See the [component contract](../components/graph/crypto-misuse-analysis.md),
[Theory 39](../theory/39-cryptographic-api-misuse.md),
[Security 14](../security/14-cryptographic-api-misuse.md), and
[Scientific validation 44](../scientific/44-cryptographic-api-misuse-validation.md).
