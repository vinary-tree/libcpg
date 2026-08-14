# Usage 10 — Running taint and maybe-null analysis

This guide starts with an already constructed Code Property Graph (CPG),
materializes evidence-bounded call topology, runs the built-in generic taint
catalog, and interprets complete versus capped output. The analysis APIs are
feature-free; parsing source still requires the matching language feature or a
caller-supplied Mode-B tree.

## 1. Resolve calls before interprocedural analysis

```rust
use libcpg::{
    analyze_taint_report, resolve_calls, CodePropertyGraph, IfdsCaps,
    ResolveConfig, TaintAnalysis, TaintCatalog,
};

fn scan(mut cpg: CodePropertyGraph) -> TaintAnalysis {
    let resolution = resolve_calls(&mut cpg, &ResolveConfig::default());
    analyze_taint_report(
        &cpg,
        &resolution,
        &TaintCatalog::builtin_generic(),
        &IfdsCaps::default(),
    )
}
```

`resolve_calls` records every processed call and emits topology only when one
target has supported evidence. Taint analysis then composes represented callees
through summaries and applies catalog external models to opaque calls.

Ensure the CPG has DFG evidence before scanning. Parsed construction normally
runs `DfgExtractor`; a hand-built or externally loaded graph must supply the
intended `DefUse`, `ReachingDef`, `Parameter`, `ReturnValue`, and dereference
relations itself.

## 2. Require completeness for negative claims

```rust
use libcpg::TaintAnalysis;

fn represented_flow_absent(analysis: &TaintAnalysis) -> bool {
    analysis.is_complete() && analysis.flows.is_empty()
}
```

An empty vector from `analyze_taint` loses the distinction between “complete
and empty” and “capped before a flow was retained.” Use `analyze_taint_report`
whenever absence matters.

Even a complete empty report is relative to the current CPG and catalog. It is
not a general security proof.

## 3. Read and rank findings

```rust
use libcpg::TaintAnalysis;

fn print_findings(analysis: &TaintAnalysis) {
    for flow in &analysis.flows {
        println!(
            "{} -> {} at {:?}; CWE={:?}; confidence={:.2}; capped={}",
            flow.source_class,
            flow.sink_class,
            flow.sink,
            flow.cwe,
            flow.confidence,
            flow.capped,
        );
        println!("witness: {:?}", flow.witness);
    }
}
```

The witness begins at `source` and ends at `sink`. `param_index` identifies the
sink/callee argument position when known. Confidence ranks the weakest catalog,
external-model, or call-resolution evidence on the path; it is not a
probability.

## 4. Replace the catalog

```rust
use libcpg::{CatalogRule, ExternalModel, Pattern, TaintCatalog};

fn catalog() -> TaintCatalog {
    TaintCatalog {
        sources: vec![CatalogRule {
            pattern: Pattern::Exact("queue::receive".into()),
            class: "queue_message".into(),
            cwe: None,
        }],
        sinks: vec![CatalogRule {
            pattern: Pattern::Exact("template::evaluate".into()),
            class: "template_eval".into(),
            cwe: Some(95),
        }],
        sanitizers: vec![CatalogRule {
            pattern: Pattern::Exact("template::escape".into()),
            class: "template_escape".into(),
            cwe: None,
        }],
        externals: vec![(
            Pattern::Exact("vendor::identity".into()),
            ExternalModel::Passthrough,
        )],
    }
}
```

Use `TaintCatalog::empty()` when building a policy from scratch. To extend the
generic policy, create `TaintCatalog::builtin_generic()` and append rules. If
several rules produce the same class/CWE pair, the most specific match supplies
confidence. A sanitizer match takes precedence over all other roles.

## 5. Model opaque calls deliberately

- `Passthrough` preserves argument taint in the result and is the default.
- `Havoc` drops argument taint; use it only with evidence that the result is
  independent.
- `Source` creates a project-defined ambient class.
- `Sink` records every tainted argument with the supplied class and CWE.
- `Sanitizer` kills argument taint at the result.

Prefer exact external patterns. A broad sanitizer substring can hide real
flows; a broad sink substring can create excessive review noise.

## 6. Run maybe-null analysis

```rust
use libcpg::{
    analyze_null_flows_report, CallGraphResolution, CodePropertyGraph,
    IfdsCaps, NullAnalysis,
};

fn null_scan(
    cpg: &CodePropertyGraph,
    resolution: &CallGraphResolution,
) -> NullAnalysis {
    analyze_null_flows_report(cpg, resolution, &IfdsCaps::default())
}
```

`NullFlow::origin_kind` distinguishes an explicit null literal, an
uninitialized variable, and a typed nullable return. The sink is a represented
DFG dereference or unary `*`. As with taint, use `is_complete()` before
interpreting an empty result.

## 7. Choose caps for a service

```rust
use libcpg::IfdsCaps;

fn service_caps() -> IfdsCaps {
    IfdsCaps {
        max_scc_iters: 64,
        max_facts_per_node: 2_048,
        max_worklist_steps: 500_000,
        max_call_depth: 96,
        max_functions: 50_000,
    }
}
```

`max_facts_per_node` bounds distinct origin-sensitive facts, not just class
names. `max_worklist_steps` also bounds local typed transfer. `max_call_depth`
bounds callee-path reconstruction. If any bound is reached, inspect
`cap_events`, `domain_capped_functions`, and per-flow `capped` before raising
the limit under controlled resources.

## 8. Review checklist

For each candidate:

1. confirm deployment control of the source;
2. inspect the typed witness and call-resolution confidence;
3. verify the sink argument is actually security-sensitive;
4. check whether control-flow feasibility or aliases refine the path;
5. validate any sanitizer semantics independently;
6. interpret the CWE as categorization, not proof; and
7. retain cap and issue evidence in reports and user interfaces.

## 9. Reproduce the focused validation

```sh
cargo test --no-default-features --lib taint
cargo test --no-default-features --test taint_integration
cargo test --no-default-features --features serde --lib taint
cargo bench --bench ifds_taint -- --test
```

See [Scientific validation 29](../scientific/29-taint-nullflow-validation.md)
for the exact examples, generated oracles, and benchmark shapes.

## References

1. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and
   Discovering Vulnerabilities with Code Property Graphs.* DOI:
   [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44).
2. Reps, T., Horwitz, S., Sagiv, M. (1995). *Precise Interprocedural Dataflow
   Analysis via Graph Reachability.* DOI:
   [10.1145/199448.199462](https://doi.org/10.1145/199448.199462).
