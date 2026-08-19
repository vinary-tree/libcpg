# Usage 16 — Check a finite-state object protocol

Typestate analysis is available with `default = []`. Define a finite-state
property, build or supply a CPG with CFG and DFG evidence, and inspect the
qualified `TypestateAnalysis`. The analyzer never parses source text itself.

## 1. Define the property

This policy allows zero or more `use` events while a resource is open and one
`close`. A `use` or second `close` after closure has no ordinary transition, so
it enters the absorbing error state.

```rust
use libcpg::PropertyFsm;

fn resource_property() -> PropertyFsm {
    PropertyFsm::new("error")
        .with_allocation("open", "open")
        .with_event("use")
        .with_event("close")
        .with_transition("open", "use", "open")
        .with_transition("open", "close", "closed")
        .with_accepting("closed")
}
```

![The resource property used by the example](../diagrams/typestate-property-fsm.svg)

Builder methods insert their state/event vocabulary automatically. If a caller
constructs or deserializes public fields directly, `validate` reports every
closed-vocabulary, allocation, accepting-state, and absorbing-error issue.

## 2. Analyze and qualify the result

```rust
use libcpg::{analyze_typestate, CodePropertyGraph, IfdsCaps, PropertyFsm};

fn review(cpg: &CodePropertyGraph, fsm: &PropertyFsm) {
    let report = analyze_typestate(cpg, fsm, &IfdsCaps::default());

    for finding in &report.violations {
        println!(
            "object {:?}: {:?} from {} via {} (witness {:?})",
            finding.object,
            finding.site,
            finding.from,
            finding.event,
            finding.witness
        );
    }

    if !report.is_complete() {
        eprintln!("partial typestate evidence: {:?} {:?}", report.cap_events, report.issues);
    } else if report.violations.is_empty() {
        println!("no forbidden transition in the represented evidence");
    }
}
```

`check_typestate` returns only the findings vector. It is convenient for
positive searches, but it cannot distinguish “clean and complete” from “empty
because evidence was incomplete.” Enforcement code should retain the report.

## 3. Interpret fields

| Field | Meaning |
|---|---|
| `violations` | canonical forbidden-transition candidates |
| `terminal_states` | possible states at represented function exits, keyed by allocation call |
| `processed_functions` | functions with analyzed allocation objects |
| `processed_objects` | admitted allocation call-site identities |
| `worklist_steps` | object-flow and CFG fact work consumed |
| `capped` / `cap_events` | typed resource boundary evidence |
| `issues` | policy, event, AST-parent, CFG, or placement incompleteness |

Each finding carries the property state immediately before the event. Its
`witness` begins with the allocation object and retains canonical CFG/event
evidence to the violating site. Node identifiers are from the input CPG.

## 4. Check terminal acceptance

Forbidden transitions and terminal policy are distinct. A resource may remain
legally open throughout a function but still violate a caller's release policy.

```rust
use libcpg::{CodePropertyGraph, IfdsCaps, NodeId, PropertyFsm, analyze_typestate};

fn accepting_at_exit(
    cpg: &CodePropertyGraph,
    fsm: &PropertyFsm,
    allocation: NodeId,
) -> Option<bool> {
    let report = analyze_typestate(cpg, fsm, &IfdsCaps::default());
    report
        .is_complete()
        .then(|| report.object_is_accepting(allocation, fsm))
        .flatten()
}
```

`None` means either this wrapper refused an incomplete report or the object had
no represented terminal state. `Some(true)` means every represented terminal
state is accepting; `Some(false)` means at least one is not.

## 5. Tune explicit bounds

```rust
use libcpg::IfdsCaps;

let caps = IfdsCaps {
    max_facts_per_node: 8_192,
    max_worklist_steps: 2_000_000,
    max_call_depth: 256,
    max_functions: 200_000,
    ..IfdsCaps::default()
};
```

`max_scc_iters` belongs to recursive procedure-summary solving and is not used
by this finite intraprocedural fact worklist. Raising a cap increases resource
permission; it does not repair missing CFG, DFG, target, or AST evidence.

## 6. Serialize policy and evidence

With `serde`, both the policy and report implement `Serialize` and
`Deserialize`:

```rust
// requires: features = ["serde"]
use libcpg::{CodePropertyGraph, IfdsCaps, PropertyFsm, analyze_typestate};

fn report_json(cpg: &CodePropertyGraph, fsm: &PropertyFsm) -> serde_json::Result<String> {
    serde_json::to_string_pretty(&analyze_typestate(cpg, fsm, &IfdsCaps::default()))
}
```

The ordered-map representation rejects duplicate serialized keys and preserves
deterministic values across a round trip.

See [Theory 33](../theory/33-finite-state-typestate-properties.md),
[ADR-0043](../design/0043-deterministic-typestate-analysis.md), the
[component guide](../components/graph/typestate-analysis.md), the
[validation ledger](../scientific/38-typestate-validation.md), and the
[security boundary](../security/08-typestate-analysis.md).
