# Usage 18 — Review population anomaly evidence

I6 missing-check and I7 deviant-behavior analysis are feature-free. Supply a
normalized CPG, configure an exact population policy, retain the qualified
report, and use source ranges associated with reported node identifiers when
presenting evidence. Neither analyzer parses source strings or mutates the
graph.

A complete feature-free runnable version of both exact fixtures is available as
[`examples/population_anomalies.rs`](../../examples/population_anomalies.rs):

```sh
cargo run --example population_anomalies --no-default-features
```

![Typed evidence, exact beliefs, and review findings](../diagrams/population-anomaly-pipeline.svg)

## 1. Configure sensitive APIs and find missing checks

The catalog is the security-policy boundary. This example admits exact calls
to `authorize`, labels them `access-control`, and compares every represented
same-API use:

```rust
use libcpg::{
    CatalogRule, MissingCheckConfig, Pattern, PeerScope, PopulationThreshold,
    TaintCatalog,
};

fn access_check_policy() -> MissingCheckConfig {
    let mut catalog = TaintCatalog::empty();
    catalog.sinks.push(CatalogRule {
        pattern: Pattern::Exact("authorize".into()),
        class: "access-control".into(),
        cwe: Some(862),
    });

    MissingCheckConfig {
        catalog,
        sensitive_classes: ["access-control".into()].into_iter().collect(),
        minimum_support: PopulationThreshold::new(4, 5),
        minimum_population: 5,
        peer_scope: PeerScope::WholeProgram,
        ..MissingCheckConfig::default()
    }
}
```

Run the qualified analyzer and preserve exact fractions:

```rust
use libcpg::{analyze_missing_checks, CodePropertyGraph, MissingCheckConfig};

fn review_missing_checks(
    cpg: &CodePropertyGraph,
    config: &MissingCheckConfig,
) -> Result<(), libcpg::AnalysisError> {
    let report = analyze_missing_checks(cpg, config)?;
    for finding in &report.findings {
        println!(
            "{} at {:?}: {} expected checks across {:?}",
            finding.api.name(),
            finding.site,
            finding.expected_checks.len(),
            finding.peer_sites,
        );
        for (check, support) in &finding.peer_support {
            println!(
                "  {} / {} peers; fingerprint nodes={}; guards={:?}",
                support.supporting,
                support.population,
                check.nodes.len(),
                finding.check_witnesses[check],
            );
        }
    }
    if !report.is_complete() {
        eprintln!("partial missing-check evidence: {:?}", report.issues);
    }
    Ok(())
}
```

`sensitive_classes` empty means all matched sink classes. The built-in generic
catalog is the default, but security review should normally provide an
application-specific catalog.

## 2. Choose a peer scope

`PeerScope::WholeProgram` is appropriate when all uses of a resolved API share
one contract. For context-sensitive populations:

```rust
use libcpg::PeerScope;

let nearby = PeerScope::CallGraph { max_distance: 2 };
```

Distance is measured on an undirected view of the represented call graph
between owner functions. Radius zero admits only sites in the same function;
radius one admits directly connected callers/callees; radius two can connect
callers through a shared represented callee. It is structural context, not a
proof that peers have equal preconditions.

## 3. Infer call-order and return-check beliefs

```rust
use libcpg::{
    analyze_deviant_behavior, CodePropertyGraph, DeviantBehaviorConfig,
    DeviantBehaviorKind, PopulationThreshold,
};

fn review_deviations(cpg: &CodePropertyGraph) -> Result<(), libcpg::AnalysisError> {
    let config = DeviantBehaviorConfig {
        minimum_support: PopulationThreshold::new(9, 10),
        minimum_population: 10,
        ..DeviantBehaviorConfig::default()
    };
    let report = analyze_deviant_behavior(cpg, &config)?;

    for finding in &report.findings {
        let behavior = match &finding.kind {
            DeviantBehaviorKind::MissingFollowingCall { expected } => {
                format!("missing later call to {}", expected.name())
            }
            DeviantBehaviorKind::UncheckedReturn => "unchecked return value".to_owned(),
        };
        println!(
            "{} at {:?}: {}; support={}/{}; examples={:?}",
            finding.api.name(),
            finding.site,
            behavior,
            finding.peer_support.supporting,
            finding.peer_support.population,
            finding.supporting_sites,
        );
    }

    if !report.is_complete() {
        eprintln!("partial deviant evidence: {:?}", report.issues);
    }
    Ok(())
}
```

Inspect `call_order_beliefs` and `return_check_beliefs` even when no occurrence
deviates; they record what the population actually established and include
supporting sites and guards.

## 4. Interpret thresholds exactly

`PopulationThreshold::new(4, 5)` means at least four fifths, not a floating-
point approximation. The deviating occurrence remains in the denominator:

| Population | Exact support | Meets `4/5`? | Meets `9/10`? |
|---|---:|---:|---:|
| four support, one deviation | `4/5` | yes | no |
| nine support, one deviation | `9/10` | yes | yes |
| eight support, two deviations | `8/10` | yes | no |

`ratio()` is safe for display but never needed for a policy decision.

## 5. Tune evidence bounds

```rust
use libcpg::{DeviantBehaviorConfig, MissingCheckConfig};

let missing = MissingCheckConfig {
    max_check_nodes: 200_000,
    max_callee_depth: 512,
    ..MissingCheckConfig::default()
};
let deviant = DeviantBehaviorConfig {
    max_return_evidence_nodes: 200_000,
    max_cfg_nodes_per_call: 200_000,
    max_callee_depth: 512,
    ..DeviantBehaviorConfig::default()
};
```

Raising a cap permits more represented work. It does not repair missing call,
CFG, DFG, PDG, AST, binding, or frontend evidence. A reached cap produces a
typed issue and makes `is_complete()` false.

## 6. Choose the report API

| Need | API |
|---|---|
| review positive missing checks | `missing_checks` |
| assert no represented missing check | `analyze_missing_checks` plus `is_complete()` |
| review positive deviations | `deviant_behaviors` |
| inspect inferred beliefs or assert no represented deviation | `analyze_deviant_behavior` plus `is_complete()` |

An empty convenience vector is not a clean verdict because it discards issues
and the inferred-belief context.

## 7. Serialize evidence

With `serde`, configs and reports support deterministic JSON-compatible
serialization. Typed-key maps are encoded as ordered key/value sequences:

```rust
// requires: features = ["serde"]
use libcpg::{analyze_missing_checks, CodePropertyGraph, MissingCheckConfig};

fn missing_check_json(
    cpg: &CodePropertyGraph,
    config: &MissingCheckConfig,
) -> Result<String, Box<dyn std::error::Error>> {
    let report = analyze_missing_checks(cpg, config)?;
    Ok(serde_json::to_string_pretty(&report)?)
}
```

See [Theory 35](../theory/35-population-anomaly-inference.md),
[ADR-0045](../design/0045-exact-population-anomaly-detectors.md), the
[component guide](../components/graph/population-anomaly-detectors.md), the
[validation ledger](../scientific/40-population-anomaly-validation.md), and the
[security boundary](../security/10-population-anomaly-detectors.md).
