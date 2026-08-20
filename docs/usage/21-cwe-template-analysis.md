# Usage 21 — Declarative CWE-template analysis

This guide runs the built-in Common Weakness Enumeration (CWE) registry,
interprets qualified advisory results, specializes catalog roles, and validates
the public integration and benchmark surfaces. The analysis itself is
feature-free; language features are needed only when libcpg also constructs the
CPG from source.

## 1. Run the executable example

From the repository root:

```bash
cargo run --no-default-features --example cwe_templates
```

The example constructs typed Abstract Syntax Tree (AST) and Data Flow Graph
(DFG) evidence for untrusted request data reaching `sqlx::query`. It calls the
ten-template public registry and prints one advisory CWE-89 candidate. The
example neither parses source text nor mutates the graph.

An output resembles:

```text
advisory CWE-89 candidate at NodeId(3), confidence 0.800, witness [NodeId(1), NodeId(5), NodeId(3)]
```

Node identifiers depend on construction order. The confidence value is an
evidence score, not a probability or severity.

## 2. Analyze an existing CPG

```rust,ignore
use libcpg::{
    analyze_builtin_cwes, CallGraphResolution, CodePropertyGraph,
    CweAnalysisConfig,
};

fn inspect(cpg: &CodePropertyGraph, resolution: &CallGraphResolution) {
    let report = analyze_builtin_cwes(cpg, resolution, &CweAnalysisConfig::default())
        .expect("the built-in templates and default resource policy are valid");

    for candidate in &report.candidates {
        println!(
            "CWE-{} {} at {:?}: {:?}",
            candidate.cwe, candidate.title, candidate.site, candidate.evidence
        );
        for node in &candidate.witness {
            assert!(cpg.node(*node).is_some(), "witness belongs to this CPG");
        }
    }

    if !report.is_complete() {
        eprintln!("CWE-layer issues: {:#?}", report.issues);
        eprintln!("taint qualification: {:#?}", report.taint_analysis);
        eprintln!("null-flow qualification: {:#?}", report.null_analysis);
        eprintln!("typestate qualification: {:#?}", report.typestate_analyses);
    }
}
```

Use the same `CallGraphResolution` produced for the analyzed program whenever
possible. The default resolution is appropriate for graphs whose represented
DFG/reference edges already provide the required local flow, but it cannot
invent missing interprocedural identities.

## 3. Select a subset

For a faster or policy-specific scan, select templates by stable identity or
CWE number and call `analyze_cwe_templates`:

```rust,ignore
use libcpg::{
    analyze_cwe_templates, builtin_cwe_templates, CallGraphResolution,
    CodePropertyGraph, CweAnalysisConfig,
};

fn inspect_injection_families(cpg: &CodePropertyGraph) {
    let templates: Vec<_> = builtin_cwe_templates()
        .into_iter()
        .filter(|template| matches!(template.cwe, 22 | 78 | 79 | 89 | 502))
        .collect();
    let report = analyze_cwe_templates(
        cpg,
        &CallGraphResolution::default(),
        &templates,
        &CweAnalysisConfig::default(),
    )
    .expect("the selected shipped templates are valid");
    println!("{} candidate(s)", report.candidates.len());
}
```

The selected taint-backed templates still share one solver run.

## 4. Specialize project APIs

The registry returns owned data. Extend roles with reviewed typed callee-path
patterns:

```rust,ignore
use libcpg::{builtin_cwe_templates, Pattern};

let mut sql = builtin_cwe_templates()
    .into_iter()
    .find(|template| template.cwe == 89)
    .expect("the built-in SQL template exists");
sql.source
    .call_patterns
    .push(Pattern::Exact("web::Request::untrusted_body".into()));
sql.sanitizer
    .call_patterns
    .push(Pattern::Exact("database::Prepared::bind".into()));
sql.sink
    .call_patterns
    .push(Pattern::Exact("database::RawQuery::execute".into()));
```

Prefer `Pattern::Exact` for stable project APIs. A broad substring is useful
for a reviewed framework family, but it expands the admission relation and can
raise false positives. Version custom templates beside the project security
policy and test vulnerable plus sanitized fixtures.

## 5. Interpret evidence variants

| Evidence | Review |
|---|---|
| `Taint` | inspect origin/sink classes, argument position, full retained taint report, and witness path |
| `NullDereference` | inspect null origin kind, parameter position, path feasibility, and full null report |
| `Typestate` | inspect object identity, state before event, event kind, CFG path, and retained property report |
| `IntegerOverflow` | compare abstract result interval with exact represented type domain and language overflow mode |
| `OutOfBoundsWrite` | compare index interval with represented length and verify array-origin evidence |
| `HardcodedCredential` | inspect the protected source node in the CPG; the report intentionally excludes literal text |

`candidate.source` may be absent for sink-local evidence. Every witness is a
canonical represented path or structural provenance vector, not necessarily a
single feasible runtime trace.

## 6. Handle incomplete reports

Never discard `issues`. Common actions are:

| Issue | Action |
|---|---|
| `StructuralMatchCap` | review template breadth and target multiplicity; approve a bounded increase only if operationally safe |
| `MissingFunction` | repair frontend containment or scope evidence |
| `MissingIntervalContext` | ensure the expression belongs to a represented reachable CFG node |
| `MissingIntegerType` | supply a represented fixed-width type |
| `AmbiguousIntegerType` | repair contradictory type evidence; do not pick one arbitrarily |
| `UnsupportedIntegerType` | map or model the exact language type before interpreting overflow |
| `MissingArrayLength` | preserve array type/origin evidence in the CPG |
| `AmbiguousArrayLength` | resolve contradictory aliases/origins rather than selecting one length |
| `ArrayOriginCap` | inspect origin fan-out and select an explicit safe bound |
| `IntervalAnalysis` | inspect the retained solver reason and underlying graph |

Also inspect delegated `issues` and cap events in the taint, null, and typestate
reports. `report.is_complete()` performs the aggregate check.

## 7. Configure resource policy

```rust,ignore
use libcpg::CweAnalysisConfig;

let config = CweAnalysisConfig {
    max_templates: 32,
    max_patterns_per_template: 8,
    max_matches_per_pattern: 20_000,
    max_array_origin_nodes: 200_000,
    ..CweAnalysisConfig::default()
};
```

All values must be nonzero and must not exceed compile-time hard ceilings.
Smaller values are useful for service-level budgets, but a cap event makes the
report incomplete. Larger values increase heap/time exposure; they do not
change the native-stack guarantee.

## 8. Serialize reports

Enable `serde` and use a format with `Arc` support through serde's `rc` feature:

```bash
cargo test --no-default-features --features serde \
  analysis::security::cwe::tests::templates_config_and_qualified_report_round_trip
```

The report intentionally preserves candidates, structural matches, issues, and
delegated analyses. Do not export only `candidates`, because consumers would
lose completeness evidence. Protect serialized reports as source-analysis
metadata.

## 9. Run focused validation

```bash
# Feature-free public pipeline
cargo test --no-default-features --test cwe_templates_integration

# Real Rust parser pipeline plus feature-free public pipeline
cargo test --no-default-features --features lang-rust \
  --test cwe_templates_integration

# Unit examples, property campaign, serde, and small-stack oracle
cargo test --no-default-features analysis::security::cwe::tests
cargo test --no-default-features --features serde \
  analysis::security::cwe::tests

# Correctness-guarded performance measurement
cargo bench --no-default-features --bench cwe_templates
```

Every benchmark corpus asserts exact completeness and candidate counts before
timing. Benchmark output is a local performance observation, not a correctness
test or cross-machine service-level objective.

See the [component contract](../components/graph/cwe-template-analysis.md),
[Theory 38](../theory/38-declarative-cwe-query-semantics.md),
[Security 13](../security/13-cwe-template-analysis.md), and
[Scientific validation 43](../scientific/43-cwe-template-validation.md).
