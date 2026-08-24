# Usage 37 -- Rholang NameFlow and deadlock-freedom evidence

Enable feature `rholang`, build the Code Property Graph (CPG) from a
caller-owned Rholang Tree-sitter tree through Mode B, and retain the qualified
analysis whenever absence matters.

## Analyze fresh-name flow

```rust,ignore
use libcpg::{analyze_name_flow, CodePropertyGraph, NodeId};

fn print_origins(cpg: &CodePropertyGraph, node: NodeId) {
    let report = analyze_name_flow(cpg);
    println!("fresh origins: {:?}", report.origins_of(node));
    if !report.complete {
        eprintln!("partial NameFlow evidence: {:?}", report.issues);
    }
}
```

The example uses `ignore` because callers supply a constructed Rholang CPG.
Origins are fresh-binder `NodeId` values, not source spellings.

## Inspect level constraints

```rust,ignore
use libcpg::{analyze_deadlock_freedom, CodePropertyGraph};

fn print_level_report(cpg: &CodePropertyGraph) {
    let report = analyze_deadlock_freedom(cpg);
    for level in &report.levels {
        println!("{:?}: cap={:?}, obl={:?}",
            level.channel, level.capability, level.obligation);
    }
    for witness in &report.witnesses {
        println!("cyclic obligation/capability evidence: {witness:?}");
    }
    if !report.complete {
        eprintln!("partial analysis: {:?}", report.issues);
    }
}
```

Finite levels mean the represented strict constraints are acyclic. `None`
marks channels in a cyclic system. `deadlock_freedom(cpg)` returns only the
witness vector and is appropriate for positive candidate reporting; an empty
vector cannot distinguish a clean complete model from incomplete evidence.

## Follow scope extrusion

For a represented communication `carrier!(payload)` matched with
`for (received <- carrier)`, NameFlow can transfer the fresh origin of
`payload` to the receive binder `received`. A later use or send of `received`
then retains the same origin. Inspect `Receives` and `Carries` relations or
`origins_of(receive_binder)` to replay that evidence.

## Materialize the overlay

```rust,ignore
use libcpg::{
    analyze_name_flow, materialize_name_flow_overlay, CodePropertyGraph,
    NameFlowOverlayError,
};

fn add_name_flow(cpg: &mut CodePropertyGraph) -> Result<usize, NameFlowOverlayError> {
    let report = analyze_name_flow(cpg);
    materialize_name_flow_overlay(cpg, &report)
}
```

The operation validates every relation endpoint before adding the first edge.
Repeating it adds zero edges. It preserves all nodes and non-NameFlow layers.

## Set service bounds

```rust
use libcpg::NameFlowCaps;

let caps = NameFlowCaps {
    max_origin_assignments: 250_000,
    max_relations: 250_000,
    max_communication_matches: 100_000,
    max_dependencies: 100_000,
    max_work_items: 500_000,
};
assert_eq!(caps.max_work_items, 500_000);
```

Use `analyze_name_flow_with_caps` or
`analyze_deadlock_freedom_with_caps`. A reached cap makes the report
incomplete; never translate it into an empty clean result. Heap-backed stack
safety does not bound resident memory, so hosts should also enforce process
memory and time policy for untrusted graphs.

## Related documentation

- [Theory](../theory/52-rholang-name-flow-and-deadlock-freedom.md)
- [Component contract](../components/graph/rholang-name-flow.md)
- [ADR-0069](../design/0069-stack-safe-rholang-name-flow.md)
- [Security boundary](../security/27-rholang-name-flow-boundary.md)
- [Validation](../scientific/64-rholang-name-flow-validation.md)
