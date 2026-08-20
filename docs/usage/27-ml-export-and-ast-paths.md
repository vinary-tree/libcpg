# Usage 27 — Export CPG tensors and exact AST paths

The ML export APIs are feature-free and read-only. They return ordinary Rust
vectors so applications can choose PyTorch Geometric (PyG), another tensor
runtime, or a non-neural consumer without adding an ML dependency to libcpg.

## Export a whole graph

```rust,ignore
use libcpg::{export_pyg, CodePropertyGraph, PygExport};

fn export_for_training(cpg: &CodePropertyGraph) -> PygExport {
    let export = export_pyg(cpg);

    assert_eq!(export.node_features.len(), export.node_ids.len());
    assert_eq!(export.source_ranges.len(), export.node_ids.len());
    assert!(export
        .node_features
        .iter()
        .all(|row| row.len() == libcpg::PYG_NODE_FEATURE_COUNT));

    export
}
```

The snippet is a valid library function but uses `ignore` because the caller
must supply a CPG. `node_features[d]`, `node_ids[d]`, and `source_ranges[d]`
are one aligned record. `edge_index` contains sorted
`(source_dense, target_dense, relation_class)` tuples. It is an edge list, not
an already transposed PyG `2 x E` tensor.

Resolve columns by schema name:

```rust
use libcpg::PygExport;

let depth = PygExport::feature_index("ast.depth").expect("stable column");
let known = PygExport::feature_index("ast.depth_known").expect("stable column");
assert_eq!(depth, 63);
assert_eq!(known, 64);
```

Do not interpret `ast.depth` without `ast.depth_known`. A root has depth zero
and knownness one; a multiple-parent or cyclic AST node has storage depth zero
and knownness zero.

## Add complete PDG localization

To mark all represented dependencies of one criterion:

```rust,ignore
use libcpg::{
    export_pyg_with_options, AnalysisError, CodePropertyGraph, NodeId,
    PygExport, PygExportOptions,
};

fn localized(
    cpg: &CodePropertyGraph,
    criterion: NodeId,
) -> Result<PygExport, AnalysisError> {
    export_pyg_with_options(
        cpg,
        PygExportOptions {
            slice_criterion: Some(criterion),
            max_slice_nodes: 50_000,
        },
    )
}
```

The membership column is `localization.backward_pdg_slice`, and the exact
sorted stable ids are also available as `slice_node_ids`. If the complete
backward Program Dependence Graph (PDG) slice contains more than 50,000 nodes,
the call returns `AnalysisError::BudgetExceeded`. It does not emit a truncated
slice that could be mistaken for negative labels.

## Convert to a tensor runtime

The following Python function shows the mechanical shape conversion after an
application has transferred the serde JSON payload to Python:

```python
import torch
from torch_geometric.data import Data


def to_pyg(payload: dict) -> Data:
    features = torch.tensor(payload["node_features"], dtype=torch.float32)
    triples = torch.tensor(payload["edge_index"], dtype=torch.long).reshape(-1, 3)
    edge_index = triples[:, :2].transpose(0, 1).contiguous()
    edge_type = triples[:, 2].contiguous()
    return Data(x=features, edge_index=edge_index, edge_type=edge_type)
```

This adapter preserves relation classes separately in `edge_type`. Parallel
tuples remain parallel edges. The centrality columns were computed on an
unweighted simple endpoint topology; do not infer that parallel edge count was
used as PageRank weight.

With `features = ["serde"]`, serialize the complete Rust result normally:

```rust,ignore
use libcpg::{export_pyg, CodePropertyGraph};

fn as_json(cpg: &CodePropertyGraph) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(&export_pyg(cpg))
}
```

## Extract code2vec-style path contexts

`ast_paths` selects one function and returns source-ordered terminal pairs with
their complete Abstract Syntax Tree (AST) paths:

```rust,ignore
use libcpg::{
    ast_paths, AnalysisError, AstPathOptions, CodePropertyGraph, NodeId,
    PathContext,
};

fn contexts(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<Vec<PathContext>, AnalysisError> {
    ast_paths(
        cpg,
        function,
        AstPathOptions {
            max_path_length: 8,
            max_path_width: 2,
            max_terminals: 10_000,
            max_pair_checks: 1_000_000,
            max_contexts: 200_000,
            max_total_path_nodes: 2_000_000,
            max_ancestor_steps: 2_000_000,
        },
    )
}
```

For each context:

- `start` and `end` are exact terminal node ids;
- `path` walks start → lowest common ancestor → end;
- `path[ancestor_index] == ancestor`;
- `node_kinds` is aligned one-for-one with `path`; and
- `start_token`/`end_token` prefer exact source text, then represented semantic
  values, and finally a stable kind token.

![Exact terminal-to-terminal AST path reconstruction](../diagrams/ast-path-context.svg)

The width filter is the difference between the source-ordered child slots
immediately below the lowest common ancestor. The length filter is the number
of edges on the exact path. These are semantic filters: a filtered pair does
not cause an error.

## Choose caps deliberately

Path pairs grow quadratically with terminal count. Every allocation-sensitive
quantity has a caller cap and a compile-time hard ceiling:

| Option | Default | Compile-time ceiling | Meaning |
|---|---:|---:|---|
| `max_path_length` | 8 | 1,000,000 | maximum retained path edges |
| `max_terminals` | 10,000 | 100,000 | scoped AST terminals before pairing |
| `max_pair_checks` | 1,000,000 | 10,000,000 | all admitted unordered pairs, including filtered pairs |
| `max_contexts` | 200,000 | 1,000,000 | retained contexts |
| `max_total_path_nodes` | 2,000,000 | 20,000,000 | aggregate nodes in retained paths |
| `max_ancestor_steps` | 2,000,000 | 20,000,000 | aggregate terminal-to-function chain nodes |

`max_path_width` is a pure filter and does not allocate by itself, so it has no
compile-time ceiling. A cap failure returns `BudgetExceeded`; an option above a
hard ceiling returns `InvalidInput`. Neither returns a partial vector.

## Interpret malformed syntax conservatively

`ast_paths` follows exact `AstChild` edges. A terminal chain with no parent,
multiple parents, or a cycle is suppressed rather than repaired. Therefore an
empty result can mean:

1. fewer than two admitted terminals;
2. every pair failed length/width filters; or
3. represented AST parent relations were malformed.

It does not certify that the AST is valid. Validate frontend output separately
when completeness is required.

## Separate export from learning

`analysis::ml` produces deterministic evidence. It does not train a graph
neural network, normalize features across a data set, choose a loss, or split
training and evaluation samples. In particular, whole-CPG PageRank and token
text can expose context outside a later sample boundary. Split programs and
apply privacy policy before using these fields in a training corpus.

The optional `gnn` module has different semantics: it owns the CPG and performs
randomly initialized, untrained mean propagation. Do not substitute its vectors
for a canonical export when reproducibility is required.

See the [component contract](../components/graph/ml-export.md), [Theory
44](../theory/44-deterministic-cpg-ml-representations.md), [ADR-0054](../design/0054-deterministic-cpg-ml-exports.md), and the [security
boundary](../security/19-ml-export-boundary.md).
