# Usage 23 — Secret flow and missing access guards

This guide runs both feature-free security-policy analyses, interprets their
evidence, extends policy, and handles incomplete reports.

## 1. Run the executable example

```bash
cargo run --no-default-features --example security_policies
```

The example reports one high-entropy literal reaching authentication and one
unguarded mutation among five peers. Output contains node IDs and evidence
metadata, not the literal contents.

## 2. Analyze secret flows

```rust,ignore
use libcpg::{
    analyze_builtin_secrets, CallGraphResolution, CodePropertyGraph,
    SecretAnalysisConfig,
};

fn inspect_secrets(cpg: &CodePropertyGraph, resolution: &CallGraphResolution) {
    let report = analyze_builtin_secrets(
        cpg,
        resolution,
        &SecretAnalysisConfig::default(),
    );

    for finding in &report.findings {
        println!(
            "{:?}: {:?} -> {:?}, argument {}",
            finding.family,
            finding.source,
            finding.sink,
            finding.argument_index,
        );
    }
    if !report.is_complete() {
        eprintln!("secret qualification issues: {:#?}", report.issues);
        eprintln!("taint qualification: {:#?}", report.taint);
    }
}
```

Pass call-resolution evidence for the same program. An empty resolution is
appropriate only when typed callee syntax and graph relations already identify
the required calls.

## 3. Analyze missing authN/authZ guards

The generic default requires four-of-five support and compares a bounded
call-graph neighborhood. Route collections are often easier to interpret as a
whole-program population:

```rust,ignore
use libcpg::{
    analyze_builtin_authz, AuthzAnalysisConfig, CodePropertyGraph, PeerScope,
    PopulationThreshold,
};

fn inspect_access(cpg: &CodePropertyGraph) -> Result<(), libcpg::AnalysisError> {
    let report = analyze_builtin_authz(
        cpg,
        &AuthzAnalysisConfig {
            minimum_support: PopulationThreshold::new(4, 5),
            minimum_population: 5,
            peer_scope: PeerScope::WholeProgram,
            ..AuthzAnalysisConfig::default()
        },
    )?;

    for finding in &report.findings {
        println!(
            "missing access guard at {:?}; peers {:?}",
            finding.site, finding.peer_sites
        );
    }
    if !report.is_complete() {
        eprintln!("access-policy issues: {:#?}", report.issues);
    }
    Ok(())
}
```

Build PDG control-dependence edges before analysis when guards come from parsed
conditionals. `PdgBuilder::build` is on demand.

## 4. Extend secret policy

```rust,ignore
use libcpg::{
    builtin_secret_rules, Pattern, SecretSinkFamily, SecretSinkRule,
};

let mut rules = builtin_secret_rules();
rules.sinks.push(
    SecretSinkRule::new("project-token", SecretSinkFamily::Authentication)
        .with_call(Pattern::Exact("project::session::accept_token".into()))
        .with_argument(0),
);
rules
    .name_hints
    .push(Pattern::Segment("service_credential".into()));
```

Pass `&rules` to `analyze_secret_flows`. Add positive and clean corpus cases for
every new API family. Broad `Substring` patterns can increase false positives.

## 5. Extend access policy

```rust,ignore
use libcpg::{
    builtin_authz_rules, AuthzMutationRule, Pattern,
};

let mut rules = builtin_authz_rules();
rules.mutations.push(
    AuthzMutationRule::new("project-admin-mutation")
        .with_call(Pattern::Exact("project::admin::rotate_owner".into())),
);
rules
    .guard_patterns
    .push(Pattern::Exact("project::policy::require_owner".into()));
```

Pass `&rules` to `analyze_missing_authz`. Comparable sites must share exact
`ApiIdentity`; a mutation family selects candidates but does not merge distinct
APIs into one population.

## 6. Interpret evidence safely

| Evidence | Meaning | Does not establish |
|---|---|---|
| `HighEntropy` | typed string length/distribution crossed configured thresholds | live credential, cryptographic randomness, exploitability |
| `NameHint` | bounded typed binding context matched a policy pattern | that the string is secret or reaches a sink by itself |
| secret witness | shared taint engine found represented source-to-argument flow | runtime feasibility on every path |
| `expected_guards` | peer-supported normalized access checks are absent | authoritative application policy or complete mediation |
| `peer_support` | exact rational convention among admitted comparable sites | probability, severity, or correctness of the convention |

## 7. Configure and handle bounds

`max_literals` and `max_name_context_nodes_per_literal` are clamped by
`MAX_SECRET_LITERALS` and `MAX_SECRET_NAME_CONTEXT_NODES`. Secret propagation
also uses `ifds_caps`. AuthZ fingerprint and callee traversal use
`max_check_nodes` and `max_callee_depth` through the shared missing-check engine.

Never increase a cap merely to suppress an issue. First check for malformed
cycles, unexpected graph fan-in, missing parser evidence, or an overbroad rule.
Require `is_complete()` before interpreting an empty finding vector.

## 8. Serialization and focused validation

Serialize aggregate `SecretAnalysis` or `AuthzAnalysis`, never findings alone.
Protect the serialized report as source-analysis metadata even though secret
values are omitted.

```bash
# Unit, property, cap, and 100,000-node small-stack tests
cargo test --no-default-features analysis::security::secrets
cargo test --no-default-features analysis::security::authz

# Public API and executable example
cargo test --no-default-features --test security_policy_integration
cargo run --no-default-features --example security_policies

# Rust parser + CFG/DFG/PDG composition
cargo test --no-default-features --features lang-rust \
  --test security_policy_integration

# Optional policy/report serialization
cargo test --no-default-features --features serde \
  analysis::security::secrets
cargo test --no-default-features --features serde \
  analysis::security::authz
```

See the [component contract](../components/graph/security-policy-analysis.md),
[Theory 40](../theory/40-secret-flow-and-access-policy-inference.md),
[Security 15](../security/15-secret-and-access-policy-analysis.md), and
[Scientific validation 45](../scientific/45-security-policy-validation.md).
