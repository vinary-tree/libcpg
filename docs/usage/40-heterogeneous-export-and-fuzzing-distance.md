# Usage 40 -- Export heterogeneous tensors and fuzzing distance

Both APIs are available with `default = []`. A language feature is needed only
when libcpg itself parses source; callers may also supply or construct a CPG.

## Export relation-separated tensors

```rust,ignore
use libcpg::{
    export_heterogeneous_gnn, CodePropertyGraph, CpgEdgeKind,
};

fn inspect_export(cpg: &CodePropertyGraph) {
    let export = export_heterogeneous_gnn(cpg);
    assert_eq!(export.node_features.len(), export.node_ids.len());
    assert_eq!(export.relations.len(), 67);

    let calls = export.relation(&CpgEdgeKind::CallSite);
    for &(source_dense, target_dense) in &calls.edge_index {
        let source = export.node_ids[source_dense as usize];
        let target = export.node_ids[target_dense as usize];
        println!("call {:?} -> {:?}", source, target);
    }
}
```

Every feature row has `HETERO_NODE_FEATURE_COUNT == 109` values. Columns
0--44 are the exact node-kind one-hot prefix and columns 45--108 are the
deterministic token feature hash. Use `node_types` and `tokens` when exact
categorical/text evidence is preferable to the numerical initialization.

Empty relation buckets are intentional. They preserve the distinction between
an implemented relation with no occurrences and an unknown schema. Parallel
CPG edges remain repeated coordinates.

## Compute harmonic target distance

```rust,ignore
use libcpg::{distance_to_targets, CodePropertyGraph, NodeId};

fn print_distances(cpg: &CodePropertyGraph, targets: &[NodeId]) {
    let distances = distance_to_targets(cpg, targets);
    for (node, distance) in distances {
        if distance.is_finite() {
            println!("{:?}: {distance}", node);
        } else {
            println!("{:?}: unreachable", node);
        }
    }
}
```

Paths follow stored-direction control-flow and static, dynamic, or call-site
edges. A target is zero. A non-target that reaches path lengths two and three
has distance `12.0 / 5.0`. A node reaching no valid target is positive
infinity. Duplicate targets are counted once; unknown ids are ignored.

## Prepare parser-built input

```rust,ignore
// requires: features = ["lang-rust"]
use libcpg::{
    distance_to_targets, export_heterogeneous_gnn, CpgBuilder,
    CpgBuilderConfig, Language, TreeSitterCpgBuilder,
};

fn analyze_rust(source: &str) -> Result<(), libcpg::Error> {
    let cpg = TreeSitterCpgBuilder::with_config(
        CpgBuilderConfig::new().with_cfg(true).with_dfg(true),
    )
    .build(source, Language::Rust)?;
    let Some(&target) = cpg.cfg_exits().first() else {
        return Ok(());
    };

    let heterogeneous = export_heterogeneous_gnn(&cpg);
    let distances = distance_to_targets(&cpg, &[target]);
    println!("{} nodes, target distance {}",
        heterogeneous.node_ids.len(), distances[&target]);
    Ok(())
}
```

## Persist safely

With feature `serde`, `HeteroGnnExport` and its relation records serialize
deterministically. Persist the libcpg version and schema constants beside the
payload. `distance_to_targets` returns a plain map containing
`f64::INFINITY`; JSON does not represent that value. Convert unreachable
entries to a caller-defined tagged or nullable form rather than silently using
a large finite sentinel.

For untrusted inputs, bound graph size, unique target count, wall time,
resident memory, swap, and tasks. Stack safety prevents source depth from
becoming call depth; it does not make dense feature output memory-free.

## Related documentation

- [Theory 55](../theory/55-heterogeneous-graphs-and-directed-fuzzing-distance.md)
- [Component contract](../components/graph/heterogeneous-export-and-distance.md)
- [ADR-0072](../design/0072-deterministic-heterogeneous-export-and-fuzzing-distance.md)
- [Security boundary](../security/30-heterogeneous-export-and-distance-boundary.md)
- [Scientific validation](../scientific/67-heterogeneous-export-distance-validation.md)
