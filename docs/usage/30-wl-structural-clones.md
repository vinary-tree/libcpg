# Use deterministic WL structural-clone candidates

Use projection-level Weisfeiler--Lehman (WL) refinement when you need a fast,
bounded partition of vertices by local topology. WL is a candidate generator:
equal labels do not prove exact graph isomorphism, source equivalence, or
behavioral equivalence.

## Build and label a projection

The API is feature-free. This complete example constructs two directed rings,
labels them for four rounds, and obtains one ordered structural-clone class:

```rust
use libcpg::{
    structural_clone_classes, wl_labels, GraphProjection, NodeId, ProjectionKind,
};

fn main() {
    let nodes = (0..6).map(NodeId::new).collect::<Vec<_>>();
    let edges = [(0, 1), (1, 2), (2, 0), (3, 4), (4, 5), (5, 3)]
        .map(|(source, target)| (NodeId::new(source), NodeId::new(target), 1.0));
    let graph = GraphProjection::from_edges(ProjectionKind::External, nodes, edges);

    let labels = wl_labels(&graph, 4);
    assert_eq!(labels.len(), graph.node_count());
    assert!(labels.iter().all(|label| *label == labels[0]));

    let classes = structural_clone_classes(&graph, 4, 2);
    assert_eq!(classes, vec![(0..6).map(NodeId::new).collect::<Vec<_>>()]);
}
```

`labels[d]` is aligned with `graph.nodes[d]`. Clone classes contain stable
`NodeId` values, members are ascending, and classes are ordered by descending
size then smallest member.

## Choose rounds and support

- `iterations = 0` distinguishes only directed in/out degree.
- Larger values include successively wider incident neighborhoods.
- Requests above `MAX_WL_ITERATIONS` (16) are clamped to 16.
- `min_class` values zero or one mean two; values above the node count return
  no class.

Use the same round count when comparing results. Numeric labels are
graph-and-round-scoped structural tokens; persist class membership and the
configuration rather than interpreting a rank as a universal identifier.

## Interpret results safely

WL ignores edge weights, node payloads, source text, names, and behavior. For a
security or transformation workflow:

```text
construct the intended validated projection
compute bounded WL classes with a recorded round count
apply application-specific node and edge predicates
run exact labeled matching when proof is required
report WL equality only as candidate evidence
```

Malformed caller-mutated or deserialized projections return empty labels and
classes before indexed traversal. For a valid nonempty graph, successful labels
always have `node_count` entries, so callers can distinguish safe failure from
an aligned result.

## Resource and stack behavior

Refinement is iterative. It alternates two call-local dense label matrices and
reuses the capacities of its canonical vocabulary and incident-neighbor
buffer; no state survives the call. A 100,000-node path and star both complete
on a 256 KiB native stack. Heap work still scales with supplied graph size, so
admit untrusted projections with explicit node, edge, and byte limits.

See the [component guide](../components/graph/wl-analysis.md) for formulas and
the complete algorithm, [ADR-0061](../design/0061-reusable-wl-refinement-buffers.md)
for ownership rationale, and [scientific validation 56](../scientific/56-wl-buffer-reuse-validation.md)
for exact compatibility, property, performance, and stack evidence.
