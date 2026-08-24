# Usage 36 -- compositional heap analysis

The heap API is feature-free. Build and resolve a CPG with the normal frontend
pipeline, compute its call-graph SCCs, then request a qualified report.

## Analyze with the default release model

```rust,ignore
use libcpg::{analyze_heap, call_graph_sccs, CodePropertyGraph, IfdsCaps};

fn heap_report(cpg: &CodePropertyGraph) -> libcpg::HeapAnalysis {
    let call_graph = call_graph_sccs(cpg);
    analyze_heap(cpg, &call_graph, &IfdsCaps::default())
}
```

The snippet is a valid library function but uses `ignore` because callers
supply a constructed CPG. The default model recognizes exact full or terminal
names `free`, `dealloc`, `delete`, and `operator_delete`. Allocation evidence
comes from `CpgNode::allocation_type`; dereference evidence comes from typed
DFG relations or a represented unary `*` node.

Always qualify absence:

```rust,ignore
let report = heap_report(&cpg);
let findings = report.violations(&cpg);
if report.is_complete() && findings.is_empty() {
    println!("no violation in the represented heap model");
} else {
    for finding in findings {
        println!("{:?}: {:?}", finding.kind, finding.witness);
    }
}
```

Even a complete empty result is not a whole-program safety proof: frontend,
call-resolution, alias, native-code, exception, and concurrency evidence may
remain outside the model.

## Add project-specific releases

```rust,ignore
use libcpg::{
    analyze_heap_with_model, call_graph_sccs, CodePropertyGraph, HeapModel,
    IfdsCaps,
};

fn project_heap_report(cpg: &CodePropertyGraph) -> libcpg::HeapAnalysis {
    let model = HeapModel::default()
        .with_release_call("destroy")
        .with_release_call("acme::arena::retire");
    analyze_heap_with_model(cpg, &call_graph_sccs(cpg), &IfdsCaps::default(), &model)
}
```

Names are exact and case-sensitive. A full path and its terminal segment are
both accepted. Do not add a broad name such as `close` unless every represented
call with that name invalidates the modeled heap cell under the intended
policy.

## Interpret summaries

```rust,ignore
use libcpg::{HeapLocation, HeapPredicate};

for (&function, summary) in &report.summaries {
    for requirement in &summary.preconditions {
        if let (HeapPredicate::Valid, HeapLocation::Parameter(index)) =
            (requirement.predicate, requirement.location)
        {
            println!("{function:?} requires parameter {index} to be live");
        }
    }
}
```

`preconditions` are the synthesized anti-frame. `postconditions` describe
represented exits. `composed_callees` records the resolved functions whose
summary evidence was observed. `capped`, `cap_events`, and `issues` are trust
evidence, not diagnostic decoration.

## Materialize the Heap overlay

Summary analysis is read-only. Materialization is an explicit, transactional
mutation:

```rust,ignore
use libcpg::{materialize_heap_overlay, HeapOverlayError};

fn add_heap_edges(
    cpg: &mut libcpg::CodePropertyGraph,
    report: &libcpg::HeapAnalysis,
) -> Result<usize, HeapOverlayError> {
    materialize_heap_overlay(cpg, &report.summaries)
}
```

Every endpoint is validated before the first edge is added. Repeating the call
returns `Ok(0)`. The node set and all non-Heap overlays are unchanged. Query
with `heap_edges`, `heap_successors`, `heap_predecessors`, or
`CpgEdgeKind::is_heap`.

## Choose caps

`IfdsCaps` is shared with other summary domains:

```rust
use libcpg::IfdsCaps;

let caps = IfdsCaps {
    max_scc_iters: 64,
    max_facts_per_node: 4_096,
    max_worklist_steps: 1_000_000,
    max_call_depth: 128,
    max_functions: 100_000,
};
assert_eq!(caps.max_call_depth, 128);
```

Use lower values at an untrusted service boundary, but never translate a cap
event into an empty clean result. `biabduction_summaries` is convenient for
callers that need only the map; completeness-sensitive callers must retain the
full `HeapAnalysis`.

## Finding meanings

| Kind | Meaning in the represented model |
|---|---|
| `NullDereference` | a typed null origin satisfies a local or composed dereference requirement |
| `UseAfterFree` | the CFG may carry `Freed` into a dereference of the same symbolic location |
| `Leak` | a local allocation may be `Live` at a represented function exit |
| `InvalidSummaryReference` | caller-supplied summary evidence names a node outside the supplied CPG |

Witnesses are ordered semantic program points, not necessarily every CFG node
between them. Replay them against the CPG and source ranges during review.

## Related documentation

- [Theory](../theory/51-compositional-heap-analysis.md)
- [Component contract](../components/graph/compositional-heap-analysis.md)
- [ADR-0068](../design/0068-compositional-heap-summaries.md)
- [Security boundary](../security/26-compositional-heap-boundary.md)
- [Validation](../scientific/63-compositional-heap-validation.md)
