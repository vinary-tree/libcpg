# Usage 39 -- Analyze Rust place capabilities

Build a Rust Code Property Graph (CPG), select a function node, and retain the
qualified `Pcg` report whenever absence matters. The analysis is feature-free;
only parser-owned construction needs feature `lang-rust`.

## Inspect ownership relations

```rust,ignore
use libcpg::{
    place_capability_graph, AnalysisError, CodePropertyGraph, NodeId,
};

fn print_capabilities(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<(), AnalysisError> {
    let pcg = place_capability_graph(cpg, function)?;
    for relation in &pcg.capability_edges {
        println!(
            "{:?}: {:?} -> {:?} at {:?}",
            relation.kind, relation.source, relation.target, relation.operation
        );
    }
    if !pcg.is_complete() {
        eprintln!("qualified ownership evidence: {:?}", pcg.issues);
    }
    Ok(())
}
```

Known primitive values and shared references have copy semantics, so their
ordinary value use does not become `Move`. Standard owning values such as
`String`, `Vec`, and `Box` do. An unresolved nominal type yields both the
possible move relation and `UnknownCopySemantics`.

## Inspect advisory violations

```rust,ignore
use libcpg::{borrow_check, AnalysisError, CodePropertyGraph, NodeId};

fn print_borrow_findings(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<(), AnalysisError> {
    for finding in borrow_check(cpg, function)? {
        println!("{:?}: {:?}", finding.kind, finding.witness);
        if finding.qualified {
            eprintln!("copy-trait evidence is incomplete");
        }
    }
    Ok(())
}
```

A later DFG use reachable from a move becomes `UseAfterMove`. A shared and a
mutable loan of the same represented source conflict only when a later DFG use
keeps the earlier destination live. Two shared loans are clean. These are
source-independent advisory findings, not rustc diagnostics.

## Materialize typed relations

```rust,ignore
use libcpg::{
    materialize_place_capability_overlay, place_capability_graph,
    AnalysisError, CodePropertyGraph, NodeId, PlaceCapabilityOverlayError,
};

fn add_capability_edges(
    cpg: &mut CodePropertyGraph,
    function: NodeId,
) -> Result<usize, Box<dyn std::error::Error>> {
    let pcg = place_capability_graph(cpg, function)
        .map_err(|error: AnalysisError| Box::new(error) as Box<dyn std::error::Error>)?;
    materialize_place_capability_overlay(cpg, &pcg)
        .map_err(|error: PlaceCapabilityOverlayError| Box::new(error) as Box<dyn std::error::Error>)
}
```

Every source and target is validated before the first edge is added. Repeating
the call returns zero. Materialization adds no nodes and removes no existing
edge. Query the result with `place_capability_edges`,
`place_capability_successors`, or `place_capability_predecessors`.

## Operational interpretation

1. Use a current Rust frontend so ownership and structured reference metadata
   are present.
2. Retain `Pcg::issues`; `is_complete()` is represented-transfer completeness,
   not compiler completeness.
3. Replay each finding's stable `NodeId` witness against the same CPG.
4. Treat an empty finding vector as clean only for the documented represented
   model.
5. Enforce process memory, time, swap, and task limits independently of native
   stack safety.

## Related documentation

- [Theory](../theory/54-place-capability-ownership-and-borrowing.md)
- [Component contract](../components/graph/place-capability-analysis.md)
- [ADR-0071](../design/0071-stack-safe-place-capability-analysis.md)
- [Security boundary](../security/29-place-capability-boundary.md)
- [Validation](../scientific/66-place-capability-validation.md)
