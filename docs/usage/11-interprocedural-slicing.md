# Usage 11 — Context-sensitive interprocedural slicing

This guide starts from a CPG whose calls are resolved and whose DFG/PDG edges
are available, computes procedure effects, materializes System Dependence Graph
(SDG) summary edges, and obtains deterministic bounded slices. The APIs are
feature-free; only source parsing needs a language feature.

## 1. Prepare the overlays

```rust
use libcpg::{
    build_summary_edges, call_graph_sccs, effect_summaries,
    CodePropertyGraph, IfdsCaps, PdgBuilder,
};

fn prepare_sdg(cpg: &mut CodePropertyGraph) {
    let functions: Vec<_> = cpg.functions().map(|node| node.id).collect();
    for function in functions {
        PdgBuilder::new().build(cpg, function);
    }

    let call_sccs = call_graph_sccs(cpg);
    let effects = effect_summaries(cpg, &call_sccs, &IfdsCaps::default());
    let _added = build_summary_edges(cpg, &effects);
}
```

Resolve calls before extracting interprocedural DFG edges. With parsed input,
enable `CpgBuilderConfig::with_import_resolution(true)` so call resolution runs
before DFG extraction. A caller-built graph must supply `CallSite` or
static/dynamic call edges plus `Parameter` and `ReturnValue` flow itself.

`build_summary_edges` is safe to repeat: it returns the number of newly added
edges, and a second call with unchanged evidence returns zero.

## 2. Take a backward slice

```rust
use libcpg::{
    interprocedural_backward_slice, CodePropertyGraph, NodeId, SliceCaps,
};
use std::collections::BTreeSet;

fn dependencies_of(
    cpg: &CodePropertyGraph,
    criterion: NodeId,
) -> BTreeSet<NodeId> {
    interprocedural_backward_slice(
        cpg,
        criterion,
        &SliceCaps::new(20_000, 64),
    )
}
```

The criterion is included when it exists and `max_nodes` is positive. The
result can contain caller arguments, callee formals, callee statements, and
return expressions. It does not follow an unrealizable return through another
call site that happens to share the callee.

## 3. Take a forward slice

```rust
use libcpg::{
    interprocedural_forward_slice, CodePropertyGraph, NodeId, SliceCaps,
};
use std::collections::BTreeSet;

fn affected_by(
    cpg: &CodePropertyGraph,
    source: NodeId,
) -> BTreeSet<NodeId> {
    interprocedural_forward_slice(
        cpg,
        source,
        &SliceCaps::new(20_000, 64),
    )
}
```

Forward slicing uses the direction-dual masks. It can descend from a caller
argument into the intended callee but cannot return into a sibling caller
context through the shared return expression.

## 4. Choose caps deliberately

`max_nodes` bounds the distinct returned nodes. `max_call_depth` bounds the
number of `Parameter` or `ReturnValue` boundaries crossed on one traversal
path; summary and ordinary PDG edges do not consume depth.

- Use `max_call_depth = 0` for caller-local PDG and summary reachability.
- Increase call depth for wrapper-heavy programs.
- Size `max_nodes` from the largest acceptable response and memory budget.
- Treat a result that reaches `max_nodes` as potentially partial.

The API returns a set, not a completeness report. Preserve the supplied caps
with the result. Also inspect the effect analysis before constructing summary
edges: if any relevant `EffectSummary::capped` bit is set, the builder widens
that call's summary edges, but the upstream summary remains incomplete.

## 5. Inspect summary edges

```rust
use libcpg::{CodePropertyGraph, CpgEdgeKind, SUMMARY_EDGE_LABEL};

fn summary_edge_count(cpg: &CodePropertyGraph) -> usize {
    cpg.edges()
        .filter(|edge| {
            edge.kind == CpgEdgeKind::DataDependence
                && edge.label.as_deref() == Some(SUMMARY_EDGE_LABEL)
        })
        .count()
}
```

Do not classify every `DataDependence` edge as a summary. The stable label is
part of summary-edge identity, so an ordinary unlabeled dependence can coexist
between the same endpoints.

## 6. Interpret results

An SDG slice answers reachability under represented control, data, parameter,
return, and effect-summary evidence. It does not by itself prove path
feasibility, alias precision, exception behavior, dynamic dispatch coverage,
or runtime security. Use the slice as an explanation and narrowing tool, and
retain its caps and input-analysis provenance.

## 7. Reproduce focused validation

```sh
cargo test --no-default-features --lib sdg
cargo test --no-default-features --test sdg_integration
cargo test --no-default-features --features serde --lib sdg
```

See [Theory 28](../theory/28-system-dependence-and-slicing.md), the
[component guide](../components/graph/system-dependence-slicing.md), and
[Scientific validation 30](../scientific/30-sdg-slicing-validation.md).

## References

1. Horwitz, S., Reps, T., Binkley, D. (1990). *Interprocedural Slicing Using
   Dependence Graphs.* DOI:
   [10.1145/77606.77608](https://doi.org/10.1145/77606.77608).
