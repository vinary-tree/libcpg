# Usage 20 — Mine API-usage and temporal protocols

Use `analyze_api_usage_mining` when you need both learned API conventions and
explicit finite-state lifecycle checks. The analyzer is feature-free; grammar
features are needed only when libcpg itself parses source.

## 1. Run the executable feature-free example

```text
cargo run --no-default-features --example api_usage_protocols
```

The example builds a typed CPG through public APIs. Nineteen of twenty `lock`
sites reach `unlock` and carry DFG object-flow evidence; the twentieth omits it.
The program asserts an exact 19/20 pattern, one usage deviation, and one
non-accepting temporal exit before printing them.

## 2. Analyze parser-built Rust

Enable `lang-rust` for construction:

```rust
use libcpg::{
    analyze_api_usage_mining, ApiUsageMiningConfig, CpgBuilder, Language,
    NamedTemporalProperty, PopulationThreshold, PropertyFsm,
    TreeSitterCpgBuilder,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = r#"
        struct Guard;
        fn lock() -> Guard { Guard }
        fn unlock(_guard: Guard) {}

        fn complete() {
            let guard = lock();
            unlock(guard);
        }

        fn incomplete() {
            let _guard = lock();
        }
    "#;
    let cpg = TreeSitterCpgBuilder::new().build(source, Language::Rust)?;
    let property = NamedTemporalProperty::new(
        "release-after-acquire",
        PropertyFsm::new("error")
            .with_allocation("lock", "acquired")
            .with_event("unlock")
            .with_transition("acquired", "unlock", "released")
            .with_accepting("released"),
    );
    let config = ApiUsageMiningConfig {
        minimum_support: PopulationThreshold::new(3, 4),
        minimum_population: 2,
        ..ApiUsageMiningConfig::default()
    };
    let report = analyze_api_usage_mining(&cpg, &[property], &config)?;

    if !report.is_complete() {
        eprintln!("incomplete usage evidence: {:?}", report.issues);
        for temporal in &report.temporal_analyses {
            eprintln!(
                "property {} complete: {}",
                temporal.property,
                temporal.is_complete()
            );
        }
    }
    for pattern in &report.patterns {
        println!(
            "pattern {:?} for {:?}: {}/{}",
            pattern.kind,
            pattern.api,
            pattern.support.supporting,
            pattern.support.population,
        );
    }
    for finding in &report.findings {
        println!("usage advisory at {:?}: {:?}", finding.site, finding.kind);
    }
    for finding in &report.temporal_findings {
        println!(
            "temporal advisory {} at {:?}: {:?}",
            finding.property, finding.site, finding.kind
        );
    }
    Ok(())
}
```

With only two `lock` occurrences, a 3/4 threshold cannot infer the one-of-two
call-order fragment. The explicit temporal property can still flag the omitted
release because temporal policy is independent from population frequency. Use
a larger corpus when mining conventions.

## 3. Define a temporal property

`PropertyFsm` is deterministic. Define every event in its alphabet, map
allocation events to initial states, add legal transitions, and mark every
acceptable exit state:

```rust
use libcpg::{NamedTemporalProperty, PropertyFsm};

let transaction = NamedTemporalProperty::new(
    "finish-after-begin",
    PropertyFsm::new("error")
        .with_allocation("begin", "active")
        .with_event("commit")
        .with_event("rollback")
        .with_transition("active", "commit", "finished")
        .with_transition("active", "rollback", "finished")
        .with_accepting("finished"),
);
```

A call to `commit` in state `finished` has no legal transition and becomes
`ForbiddenTransition`. An exit in state `active` becomes `NonAcceptingExit`.
The property name must be non-empty and unique in one analysis request.

## 4. Choose support and population

`PopulationThreshold::new(s, d)` represents exact threshold $`s/d`$. T6.5
requires:

```math
0 < s \leq d
\quad\land\quad
2s > d
```

The second condition makes it a strict majority. `minimum_population` must be
at least two. For a 95% policy use:

```rust
use libcpg::{ApiUsageMiningConfig, PopulationThreshold};

let config = ApiUsageMiningConfig {
    minimum_support: PopulationThreshold::new(19, 20),
    minimum_population: 20,
    ..ApiUsageMiningConfig::default()
};
```

Support is calculated only over complete sites with the same anchor
`ApiIdentity`. A population below `minimum_population` produces no pattern but
is not an incomplete analysis.

## 5. Read usage graphs

Each `ApiUsageGraph` is centered on one call. Inspect:

- `anchor`, `function`, and `api` for source identity;
- `nodes` for anchor/following action, data shape, and control context;
- `edges` for receiver, positional parameter, selection, and order roles;
- `is_acyclic()` before accepting a deserialized or externally constructed
  graph.

![The public graph retains source nodes and typed centered relations](../diagrams/centered-api-usage-graph.svg)

`Order` means CFG-reachable later, not guaranteed on every runtime path.

## 6. Read patterns and usage deviations

`patterns` retains exact support and every supporting site. `findings` retains
the deviating site/function/API, deviation kind, exact peer support, supporting
sites, and observed witness nodes.

| Pattern kind | Possible deviation |
|---|---|
| `FollowingCall` | `MissingFollowingCall` |
| `ArgumentCount` | observed count differs |
| `Argument` | position absent or shape differs |
| `Receiver` | receiver absent or shape differs |
| `Control` | expected direct control context absent |

The positive-only `api_usage_misuses` helper deliberately omits graphs,
patterns, temporal evidence, and issues. Use it only for advisory review, never
for proving absence.

## 7. Handle completeness

```rust
# use libcpg::ApiUsageMiningAnalysis;
fn require_complete(
    report: &ApiUsageMiningAnalysis,
) -> Result<(), &'static str> {
    if report.is_complete() {
        Ok(())
    } else {
        Err("API-usage or temporal evidence is incomplete")
    }
}
```

Review `issues` for mining exclusions and `temporal_analyses[*].resource` for
delegated typestate issues/caps. Do not increase a cap blindly; determine why
the corpus needs more work and retain process-level resource limits.

## 8. Tune resource policy

```rust
use libcpg::{ApiUsageMiningConfig, IfdsCaps};

let config = ApiUsageMiningConfig {
    max_call_sites: 150_000,
    max_cfg_nodes_per_site: 250_000,
    max_graph_nodes: 20_000,
    max_graph_edges: 200_000,
    max_callee_depth: 512,
    max_temporal_properties: 256,
    typestate_caps: IfdsCaps::default(),
    ..ApiUsageMiningConfig::default()
};
```

Every field is validated against documented hard ceilings. Global call overflow
stops mining atomically; CFG/AUG overflow excludes only the affected site;
temporal caps remain visible in the delegated report.

## 9. Serialize and transport

Enable `serde` to serialize the complete config/report. Preserve
`ApiUsageMiningAnalysis`, not only finding vectors, so exact support,
supporting-site provenance, centered graphs, issues, and delegated temporal
qualification survive transport.

## 10. Interpret advisories

- A frequent fragment is a peer convention, not an API contract.
- A temporal finding is relative to the caller FSM and represented object flow.
- CFG order is may-reachability.
- Allocation-site identity may merge runtime instances.
- The analyzer never authorizes or performs a repair.

See [Theory 37](../theory/37-api-usage-graphs-and-temporal-properties.md),
[ADR-0047](../design/0047-evidence-bounded-api-usage-and-temporal-mining.md),
the [component guide](../components/graph/api-usage-protocol-mining.md), the
[validation ledger](../scientific/42-api-usage-temporal-validation.md), and the
[security boundary](../security/12-api-usage-protocol-mining.md).

## References

1. Nielebock, S., Blockhaus, P., Krüger, J., Ortmeier, F. (2024).
   *ASAP-Repair: API-Specific Automated Program Repair Based on API Usage
   Graphs.* DOI:
   [10.1145/3643788.3648011](https://doi.org/10.1145/3643788.3648011).
2. Song, Y., Gao, X., Li, W., Chin, W.-N., Roychoudhury, A. (2024).
   *ProveNFix: Temporal Property-Guided Program Repair.* DOI:
   [10.1145/3643737](https://doi.org/10.1145/3643737).
