# Heterogeneous export and directed-fuzzing distance

This feature-free component exposes two immutable views of a Code Property
Graph (CPG): relation-separated tensors for an external heterogeneous graph
neural network (GNN), and harmonic shortest-path distances for an external
directed fuzzer. It performs no training, inference, instrumentation,
scheduling, graph mutation, or source parsing.

## Public surface

```rust,ignore
pub fn export_heterogeneous_gnn(cpg: &CodePropertyGraph) -> HeteroGnnExport;

pub fn distance_to_targets(
    cpg: &CodePropertyGraph,
    targets: &[NodeId],
) -> BTreeMap<NodeId, f64>;
```

`HeteroGnnExport` aligns ascending `node_ids`, `source_ranges`, `node_types`,
exact normalized `tokens`, and 109-column `node_features`. Its `relations`
vector always contains the 67 `EDGE_KIND_CLASS_NAMES` buckets in class order.
`dense_index`, `relation_by_class`, and `relation` provide stable lookups.

![Complete heterogeneous export pipeline](../../diagrams/heterogeneous-export-pipeline.svg)

## Evidence contracts

| Output | Authoritative evidence |
|---|---|
| node type | exhaustive `node_kind_class(CpgNodeKind)` |
| token | retained text, semantic name, represented operator/literal, or kind fallback |
| kind prefix | exact one-hot value in columns 0--44 |
| token suffix | deterministic signed UTF-8 byte n-gram feature hash in columns 45--108 |
| relation bucket | exhaustive `edge_kind_class(CpgEdgeKind)` and `EDGE_KIND_CLASS_NAMES` |
| dense endpoints | ascending stable `NodeId` map |
| distance topology | stored-direction CFG and static/dynamic/call-site edges only |
| target aggregation | zero at targets, harmonic mean of reachable positive shortest paths, infinity otherwise |

Heterogeneous relation buckets retain parallel multiplicity. Distance
predecessor rows collapse equal endpoints because repeated unweighted edges do
not alter a shortest path.

## Determinism and stack safety

Binary heaps canonicalize node, edge, and target order. Ordered maps preserve
stable identity. Distance uses a reverse `VecDeque` breadth-first search for
each valid unique target. Token hashing uses nested iterative loops over at
most three byte widths. None of these operations recurses with source or graph
depth.

![Reverse queue traversal and harmonic aggregation](../../diagrams/directed-fuzzing-distance.svg)

Unknown target ids are ignored because the public distance signature is
infallible. Callers that require strict selectors should check
`CodePropertyGraph::contains_node` before calling. Empty/all-unknown targets
return a complete map of positive infinity values.

## Consumer responsibilities

- Treat `tokens` and source ranges as source-confidential data.
- Version `EDGE_KIND_CLASS_NAMES`, `HETERO_NODE_FEATURE_COUNT`, and the libcpg
  release beside persisted data.
- Validate deserialized heterogeneous records before allocating downstream
  tensors.
- Decide relation normalization, attention, learned weights, batching, and
  feature standardization outside libcpg.
- Validate target selection and convert infinity explicitly before formats
  such as JSON that reject non-finite numbers.
- Interpret distance as represented graph evidence, not runtime reachability or
  a vulnerability verdict.

## Related documentation

- [Theory 55](../../theory/55-heterogeneous-graphs-and-directed-fuzzing-distance.md)
- [ADR-0072](../../design/0072-deterministic-heterogeneous-export-and-fuzzing-distance.md)
- [Usage 40](../../usage/40-heterogeneous-export-and-fuzzing-distance.md)
- [Security boundary](../../security/30-heterogeneous-export-and-distance-boundary.md)
- [Scientific validation](../../scientific/67-heterogeneous-export-distance-validation.md)
- [Homogeneous PyG and AST-path export](ml-export.md)
