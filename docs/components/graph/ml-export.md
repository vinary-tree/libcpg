# Component — Deterministic ML export

## Responsibility

`analysis::ml` converts an immutable Code Property Graph (CPG) into canonical
graph tensors or exact terminal-to-terminal Abstract Syntax Tree (AST) path
contexts. It owns stable evidence extraction and resource limits. It does not
parse source, initialize learned embeddings, train a model, allocate a device
tensor, or modify the CPG.

![Canonical CPG evidence exported for external ML consumers](../../diagrams/ml-export-pipeline.svg)

## Public surface

| API or type | Contract |
|---|---|
| `export_pyg(&CodePropertyGraph) -> PygExport` | infallible whole-graph export without PDG localization |
| `export_pyg_with_options(&CodePropertyGraph, PygExportOptions) -> Result<PygExport, AnalysisError>` | whole-graph export plus optional complete bounded backward-PDG localization |
| `PygExport` | aligned node rows, typed dense edges, stable ids/ranges, slice evidence, and PageRank convergence flags |
| `PygExportOptions` | optional criterion and maximum accepted complete slice size |
| `PygExport::dense_index` | stable `NodeId` to dense-row lookup by binary search |
| `PygExport::feature_index` | schema-name to feature-column lookup |
| `PYG_NODE_FEATURE_NAMES` | ordered 77-column schema registry |
| `node_kind_class` | exhaustive 45-kind stable class map |
| `edge_kind_class` | exhaustive 67-relation stable class map |
| `ast_paths(&CodePropertyGraph, NodeId, AstPathOptions)` | complete retained source-ordered terminal-pair contexts for one function |
| `AstPathOptions` | semantic length/width filters and explicit resource caps |
| `PathContext` | terminals, exact path, aligned kinds/tokens, lowest common ancestor, and ancestor index |

The module is feature-free. `PygExport`, `PygExportOptions`, `PathContext`, and
`AstPathOptions` derive serialization only under the existing `serde` feature.
No `gnn`, `ndarray`, `rand`, Python, PyTorch, or PyG dependency is required.

## Graph-export pipeline

1. A binary min-order heap canonicalizes stable node ids without source-depth
   recursion.
2. An ordered map establishes `NodeId -> dense u32` identity.
3. One edge scan builds thirteen-layer in/out degrees and typed dense tuples.
4. A second heap orders tuples while retaining duplicates.
5. A normalized root-first AST queue computes depth, knownness, and loop depth.
6. Canonical `GraphProjection` values compute unweighted forward/reverse
   PageRank for the whole CPG.
7. Optional localization computes the complete backward PDG slice and then
   admits or rejects it against `max_slice_nodes`.
8. One aligned row is materialized per canonical node.

The centrality projection merges repeated endpoint pairs. `edge_index` and
layer degrees retain typed edge multiplicity; these are distinct, documented
semantics rather than inconsistent normalization.

## Feature layout

| Columns | Family | Semantics |
|---|---|---|
| 0–44 | `kind.*` | one exact node-kind indicator |
| 45–55 | `degree.in.*` | incoming degree for AST, CFG, DFG, PDG, call, type, reference, scope, import, heap, and NameFlow |
| 56–66 | `degree.out.*` | outgoing degree for the same eleven layers |
| 67 | `ast.depth` | represented depth, interpreted only with column 68 |
| 68 | `ast.depth_known` | one iff a unique root-reachable parent chain exists |
| 69 | `ast.loop_depth` | `While`/`For`/`Loop` nesting on that known chain |
| 70 | `centrality.pagerank` | whole-CPG unweighted directed PageRank |
| 71 | `centrality.reverse_pagerank` | PageRank on reversed topology |
| 72 | `localization.backward_pdg_slice` | complete-slice membership or zero when no criterion was requested |
| 73 | `degree.in.rewrite_dep` | incoming `RewriteDep` degree |
| 74 | `degree.out.rewrite_dep` | outgoing `RewriteDep` degree |
| 75 | `degree.in.place_capability` | incoming `PlaceCapability` degree |
| 76 | `degree.out.place_capability` | outgoing `PlaceCapability` degree |

Columns 73–74 are deliberately appended. Columns 0–72 retain their original
meaning and position, so extending the typed graph did not silently reinterpret
persisted pre-FT6 feature matrices.

Use `PygExport::feature_index` or `PYG_NODE_FEATURE_NAMES`; numeric positions
are shown for schema review, not as an invitation to duplicate offsets.

## AST-path machine

The path extractor first obtains the selected function's AST scope, stopping
before nested functions. Exact `AstChild` edges—not node `parent` fields—build
normalized parent and source-ordered child tables. Terminals enter a heap keyed
by source range and stable id.

Each terminal-to-function walk owns a heap vector. A generation-mark array
detects cycles without clearing a visited set for every terminal. Only a unique
chain reaching the selected function is admitted. Pairs compare chain suffixes
to find a lowest common ancestor and then materialize the exact upward and
downward branches.

![Exact terminal-to-terminal AST path reconstruction](../../diagrams/ast-path-context.svg)

## Invariants

1. `node_features.len() == node_ids.len() == source_ranges.len()`.
2. Every node row has exactly `PYG_NODE_FEATURE_COUNT == 77` finite values.
3. Every dense edge endpoint indexes `node_ids`; every class is below 67.
4. Parallel typed edge multiplicity is preserved.
5. `slice_node_ids` is sorted, exact, complete, and empty without a criterion.
6. Unknown depth is distinguishable from root depth through
   `ast.depth_known`.
7. `path[ancestor_index] == ancestor`, and every adjacent path pair is one
   `AstChild` relation in either direction.
8. `node_kinds.len() == path.len()`.
9. Output ordering depends on source/stable identity, never insertion order.
10. Every cap failure returns an error before a partial output is returned.
11. All input-sized traversal state is heap allocated; graph depth does not
    increase native call depth.
12. Neither API changes nodes, edges, root metadata, or side tables.

## Failure and uncertainty

| Condition | Behavior |
|---|---|
| unknown slice criterion | `AnalysisError::UnknownNode` |
| complete slice exceeds caller cap | `AnalysisError::BudgetExceeded` |
| unknown function | `AnalysisError::UnknownNode` |
| selected node is not a function | `AnalysisError::NotAFunction` |
| option exceeds a compile-time ceiling | `AnalysisError::InvalidInput` |
| terminals/pairs/contexts/nodes/steps exceed a caller cap | `AnalysisError::BudgetExceeded` |
| duplicate identical AST relation | normalized for parent/depth structure |
| multiple-parent or cyclic depth region | zero depth with known bit cleared |
| missing, multiple, or cyclic terminal parent chain | terminal chain suppressed |
| PageRank reaches iteration cap | scores retained; convergence flag cleared |

## Relationship to `gnn`

`analysis::ml` is the canonical interchange boundary. The optional `gnn`
module is an experimental, untrained, randomly initialized mean-propagation
runtime that owns its CPG. One can feed `PygExport` to any external trained
model, including a relational GNN, but the export itself performs no learning.

## Tests

Seven focused tests pin the schema registry, exact aligned export, complete PDG
localization, path reconstruction, malformed AST behavior, every resource
boundary, serde, immutability, and 128 insertion-order permutations. A public
integration test uses only crate-root APIs. A 100,000-node AST exercises both
exports on a 256 KiB native stack.

See [Theory 44](../../theory/44-deterministic-cpg-ml-representations.md),
[ADR-0054](../../design/0054-deterministic-cpg-ml-exports.md), [Usage
27](../../usage/27-ml-export-and-ast-paths.md), the [scientific validation
dossier](../../scientific/49-ml-export-validation.md), and the [security
boundary](../../security/19-ml-export-boundary.md).

For relation-separated tensors and target distance, see the [heterogeneous
export and directed-distance component](heterogeneous-export-and-distance.md).
