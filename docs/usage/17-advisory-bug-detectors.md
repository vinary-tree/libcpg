# Usage 17 — Review advisory bug-detector evidence

All three detector families are feature-free. Build or supply a CPG, keep the
qualified report, and inspect convenience projections only for positive
searches. The analyzers do not parse source strings and never mutate the graph.

For missing-check, call-order, and return-check populations, use the separate
[population anomaly guide](18-population-anomaly-detectors.md).
For inconsistent clones, infeasible branches, redundant guards, and integer
contradictions, use the separate
[clone/feasibility guide](19-clone-feasibility-advisories.md).

![Three detector paths and their completeness boundaries](../diagrams/bug-detector-pipeline.svg)

## 1. Define a resource policy and find leaks

This example treats `open` as allocation, `close` as release, and only `closed`
as acceptable at a represented function exit:

```rust
use libcpg::PropertyFsm;

fn resource_property() -> PropertyFsm {
    PropertyFsm::new("error")
        .with_allocation("open", "open")
        .with_event("close")
        .with_transition("open", "close", "closed")
        .with_accepting("closed")
}
```

Analyze with the full report:

```rust
use libcpg::{analyze_resource_leaks, CodePropertyGraph, IfdsCaps, PropertyFsm};

fn review_leaks(cpg: &CodePropertyGraph, property: &PropertyFsm) {
    let report = analyze_resource_leaks(cpg, property, &IfdsCaps::default());

    for leak in &report.leaks {
        println!(
            "object {:?} may exit at {:?} in {} via {:?}",
            leak.object, leak.exit, leak.state, leak.witness
        );
    }

    if !report.is_complete() {
        eprintln!(
            "partial leak evidence: {:?}; typestate issues: {:?}",
            report.issues, report.typestate.issues
        );
    }
}
```

`NoAcceptingState` means the policy did not define release success, so the
detector returns no leak projection and refuses a complete verdict. A typestate
forbidden transition and a leak are separate: the former is an invalid event;
the latter is a legal but non-accepting state at an exit.

## 2. Find executable uses without an in-scope definition

```rust
use libcpg::{analyze_uninitialized_uses, CodePropertyGraph};

fn review_uses(cpg: &CodePropertyGraph) {
    let report = analyze_uninitialized_uses(cpg);
    for finding in &report.uses {
        println!(
            "{} at {:?} has no definition in function {:?}; outside: {:?}",
            finding.variable,
            finding.use_site,
            finding.function,
            finding.out_of_scope_definitions
        );
    }
}
```

The detector examines the shared binder-aware variable fact universe. It does
not report an unbound function name, method member name, declaration name, or
unbound signature type as a variable read. A bound function-valued variable is
still an ordinary use and is checked.

`out_of_scope_definitions` is evidence, not initialization: it lists represented
definitions that reach the use syntactically but belong to another function.

## 3. Review exception summaries and handlers

```rust
use libcpg::{analyze_exception_flow, CodePropertyGraph, IfdsCaps};

fn review_exceptions(cpg: &CodePropertyGraph) {
    let report = analyze_exception_flow(cpg, &IfdsCaps::default());

    for finding in &report.uncaught {
        println!(
            "{:?} escapes entry {:?} from {:?} via {:?}",
            finding.exception, finding.entry, finding.origin, finding.witness
        );
    }
    for finding in &report.dead_catches {
        println!(
            "catch {:?} in {:?} is unreachable for {:?}; catch_all={}",
            finding.catch, finding.function, finding.accepted, finding.catch_all
        );
    }

    if !report.is_complete() {
        eprintln!(
            "partial exception evidence: caps={:?}, issues={:?}",
            report.cap_events, report.issues
        );
    }
}
```

`summaries` maps every processed function to its possible escaping exception
types. `reached_catches` maps each reached catch node to dispatched types.
`uncaught` is emitted only at functions in source components of the represented
call graph; `dead_catches` covers every represented handler no origin reaches.

![Exception propagation and handler selection](../diagrams/exception-flow-summary.svg)

## 4. Interpret type and call evidence

| Evidence | Interpretation |
|---|---|
| `Named("E")` | represented nominal exception type `E` |
| `Unknown` | a throw exists but its nominal type is absent |
| exact type match | typed catch accepts the thrown type |
| represented inheritance path | catch of a supertype accepts a represented subtype |
| no accepted type | catch-all, including `Unknown` |
| one represented call target | callee escaping summary is propagated |
| zero represented targets | no callee summary can be propagated |
| multiple represented targets | `AmbiguousCall` makes the report incomplete |
| no typed `Catch` entry edge | `MissingCatchEdge` makes handler topology incomplete |

An unknown type is retained as positive evidence and also produces
`UnknownThrowType`; it can be handled only by a catch-all. External behavior not
represented as a function target is outside the analysis.

## 5. Tune explicit bounds

```rust
use libcpg::IfdsCaps;

let caps = IfdsCaps {
    max_functions: 200_000,
    max_facts_per_node: 8_192,
    max_worklist_steps: 2_000_000,
    max_scc_iters: 1_024,
    ..IfdsCaps::default()
};
```

Resource-leak analysis also consumes `max_call_depth` while typestate rebuilds
typed callee event paths. Exception flow does not consume that field: call
propagation uses finite summaries over SCCs instead of recursive call-path
expansion. Raising a cap grants more work; it does not repair missing type,
call, CFG, DFG, or AST evidence.

## 6. Choose report APIs for policy decisions

| Need | API |
|---|---|
| review positive leak candidates | `resource_leaks` |
| assert no represented leak | `analyze_resource_leaks` and `is_complete()` |
| review uninitialized-use candidates | `uninitialized_uses` |
| retain processed-function evidence | `analyze_uninitialized_uses` |
| review escaping exception candidates | `uncaught_exceptions` |
| review dead handlers | `dead_catches` |
| assert no represented exception finding | `analyze_exception_flow` and `is_complete()` |

Never turn an empty convenience vector into a clean verdict without the report
that qualifies it.

## 7. Serialize evidence

With the `serde` feature, policies and all detector reports implement
`Serialize` and `Deserialize`:

```rust
// requires: features = ["serde"]
use libcpg::{analyze_exception_flow, CodePropertyGraph, IfdsCaps};

fn exception_json(cpg: &CodePropertyGraph) -> serde_json::Result<String> {
    serde_json::to_string_pretty(&analyze_exception_flow(cpg, &IfdsCaps::default()))
}
```

Ordered collections preserve deterministic values across a round trip.

See [Theory 34](../theory/34-advisory-bug-detection.md),
[ADR-0044](../design/0044-evidence-composed-bug-detectors.md), the
[component guide](../components/graph/bug-detectors.md), the
[validation ledger](../scientific/39-bug-detector-validation.md), and the
[security boundary](../security/09-bug-detectors.md).
