# Deterministic CPG diff

`analysis::diff` compares two Code Property Graphs (CPGs) through a canonical
node correspondence and returns a complete owned structural edit script. It is
feature-free, read-only, deterministic, optionally serializable, and stack-safe
with respect to graph and property depth.

## Public surface

```rust,ignore
pub fn cpg_diff(
    old: &CodePropertyGraph,
    new: &CodePropertyGraph,
    options: &CpgDiffOptions,
) -> Result<CpgDiff, AnalysisError>;
```

| Type | Contract |
|---|---|
| `CpgDiffOptions` | independent per-input node/edge caps and aggregate edit cap |
| `CpgDiff` | canonical alignment, retained GED script cost, added/removed/changed records, and `ChangeKind` |
| `NodeAlignment` | old/new stable-id coordinate pair |
| `CpgElement` | complete boxed node snapshot or typed edge snapshot |
| `CpgChange` | node-field or edge-field substitution with both snapshots |
| `DiffNode` | exact represented node payload with sorted properties and cached AST links |
| `DiffEdge` | exact local id, endpoints, typed relation, and label |
| `ChangeKind` | `Refactoring` or `BehaviorChanging` under the represented-PDG rule |

The crate root re-exports the function, types, and hard ceilings.

## Pipeline

![Deterministic CPG alignment and exact edit pipeline](../../diagrams/cpg-diff-pipeline.svg)

The correspondence reuses the symmetric bounded bipartite graph-edit-distance
(GED) assignment. It is deterministic heuristic alignment, not exact graph
isomorphism. After translation into new-graph coordinates, nodes compare by
complete represented payload and edges compare as directed typed/labelled
multisets. Exact matches consume multiplicity before same-endpoint leftovers
become substitutions.

Embedded `NodeId` values in call targets, identifier definitions, lambda
captures, and cached AST links compare under the alignment. Graph-local node
and edge ids alone do not create changes.

## Classification contract

`BehaviorChanging` means at least one added, removed, or substituted edge has
an old or new `ControlDependence` or `DataDependence` kind. Otherwise the result
is `Refactoring`.

![Dependence-aware edit classification boundary](../../diagrams/cpg-diff-classification.svg)

Construct comparable Program Dependence Graphs (PDGs) on both inputs before
using this classification. `Refactoring` without PDGs is not a proof of
behavioral equivalence, and `BehaviorChanging` is represented dependence
evidence rather than proof of observable runtime change.

## Canonical evidence

- alignments sort by old then new stable id;
- node insertions/deletions sort by local stable id;
- edges sort by endpoints, exact relation class, label, and local edge id;
- node changed fields and edge changed fields use declaration order;
- known property keys precede custom keys, whose strings sort
  lexicographically; and
- exact parallel edge multiplicity is retained.

`DiffNode` includes kind/range/text, message and lock normalization, allocation
type, properties, children, and parent. `DiffEdge` retains its graph-local id
for audit even though equality ignores that id.

The graph container's language, source path, retained whole-source text, root,
and CFG entry/exit side tables are outside this structural contract.

## Budgets and complexity

| Quantity | Default | Hard ceiling |
|---|---:|---:|
| nodes per input | 512 | `MAX_CPG_DIFF_NODES = 512` |
| edges per input | 200,000 | `MAX_CPG_DIFF_EDGES = 2,000,000` |
| aggregate edits | 200,000 | `MAX_CPG_DIFF_EDITS = 2,000,000` |

Node alignment costs cubic time and quadratic matrix space in the admitted
node count. Edge comparison sorts and indexes exact edge occurrences. Every cap
is complete-or-error; the API never returns a truncated script or classifies a
prefix.

All input-sized state is heap-backed. Flat `PropertyValue` snapshots preserve
constant native call depth even for deeply nested logical properties. Neither
input graph is mutated.

See [Theory 45](../../theory/45-deterministic-cpg-diff.md),
[ADR-0055](../../design/0055-deterministic-dependence-aware-cpg-diff.md), [Usage
28](../../usage/28-cpg-diff.md), the [validation dossier](../../scientific/50-cpg-diff-validation.md), and the [security boundary](../../security/20-cpg-diff-boundary.md).
