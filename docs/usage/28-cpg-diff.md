# Usage 28 — Compare two CPG versions

`cpg_diff` returns a deterministic structural edit script between two Code
Property Graphs (CPGs). The API is feature-free and leaves both graphs
unchanged.

## Compare complete represented structure

```rust,ignore
use libcpg::{
    cpg_diff, AnalysisError, CodePropertyGraph, CpgDiff, CpgDiffOptions,
};

fn compare(
    old: &CodePropertyGraph,
    new: &CodePropertyGraph,
) -> Result<CpgDiff, AnalysisError> {
    cpg_diff(old, new, &CpgDiffOptions::default())
}
```

The result contains:

- `alignment`: structurally corresponding old/new node ids;
- `alignment_cost`: the concrete cost of the retained bounded GED script;
- `added` and `removed`: complete node or edge snapshots;
- `changed`: aligned node/edge substitutions with exact changed fields; and
- `classification`: a dependence-sensitive review label.

Node and edge ids belong to their snapshot's input graph. Use `alignment` to
translate node identity across versions; do not compare numeric ids directly.

## Build PDGs before classifying behavior

The label is meaningful only for represented Program Dependence Graph (PDG)
evidence. Build comparable PDGs on both graphs first:

```rust,ignore
use libcpg::{
    cpg_diff, AnalysisError, ChangeKind, CodePropertyGraph, CpgDiffOptions,
    NodeId, PdgBuilder,
};

fn compare_function_dependence(
    old: &mut CodePropertyGraph,
    new: &mut CodePropertyGraph,
    old_function: NodeId,
    new_function: NodeId,
) -> Result<ChangeKind, AnalysisError> {
    PdgBuilder::new().build(old, old_function);
    PdgBuilder::new().build(new, new_function);
    Ok(cpg_diff(old, new, &CpgDiffOptions::default())?.classification)
}
```

`BehaviorChanging` means the script changed at least one represented control-
or data-dependence edge. `Refactoring` means it did not. Neither label proves
language-level behavioral equivalence or difference. Without PDGs, even an
added guard can classify as `Refactoring` because only AST/CFG/node evidence is
present.

## Inspect exact edits

```rust,ignore
use libcpg::{CpgChange, CpgDiff, CpgElement};

fn summarize(diff: &CpgDiff) -> (usize, usize) {
    let changed_nodes = diff
        .changed
        .iter()
        .filter(|edit| matches!(edit, CpgChange::Node { .. }))
        .count();
    let added_edges = diff
        .added
        .iter()
        .filter(|edit| matches!(edit, CpgElement::Edge(_)))
        .count();
    (changed_nodes, added_edges)
}
```

Node substitutions list any changed `Kind`, `Range`, `Text`, `MessageKind`,
`LockKind`, `AllocationType`, `Properties`, or `AstLinks`. Edge substitutions
list changed `Kind` and/or `Label`. Complete old/new snapshots make each record
auditable without looking up a graph that may later be dropped.

Parallel typed edges remain separate records. Edge ids are evidence
coordinates, not equality keys, so pure edge renumbering is unchanged.

## Set admission budgets

```rust,ignore
use libcpg::{cpg_diff, AnalysisError, CodePropertyGraph, CpgDiff, CpgDiffOptions};

fn bounded(
    old: &CodePropertyGraph,
    new: &CodePropertyGraph,
) -> Result<CpgDiff, AnalysisError> {
    cpg_diff(
        old,
        new,
        &CpgDiffOptions {
            max_nodes: 256,
            max_edges: 50_000,
            max_edits: 25_000,
        },
    )
}
```

The effective limits are the caller values clamped to hard ceilings of 512
nodes, 2,000,000 edges per input, and 2,000,000 aggregate edits. A graph or
script that exceeds a limit returns `AnalysisError::BudgetExceeded`. No partial
script or prefix-based classification escapes.

Alignment is cubic in the admitted node count, so select `max_nodes` from your
latency policy rather than automatically using the hard maximum. Apply a
separate ingress policy to total node-payload size; property/text snapshots are
complete.

## Serialize stable evidence

With `features = ["serde"]`, the options and complete output derive serde:

```rust,ignore
use libcpg::{cpg_diff, CodePropertyGraph, CpgDiffOptions};

fn json(old: &CodePropertyGraph, new: &CodePropertyGraph) -> serde_json::Result<Vec<u8>> {
    let diff = cpg_diff(old, new, &CpgDiffOptions::default())
        .expect("admitted CPG diff");
    serde_json::to_vec(&diff)
}
```

Ordering is canonical for the same logical graph pair, independent of node and
edge insertion order. Persist the libcpg version with long-lived payloads;
public CPG node/edge schemas can evolve between releases.

## Know what is not compared

`cpg_diff` compares represented nodes and edges. It does not compare language,
source path, retained whole-source text, the root field, or recorded CFG
entry/exit side tables. Compare those container fields separately if they are
part of your versioning contract.

See the [component contract](../components/graph/cpg-diff.md), [Theory
45](../theory/45-deterministic-cpg-diff.md), [ADR-0055](../design/0055-deterministic-dependence-aware-cpg-diff.md), and the [security
boundary](../security/20-cpg-diff-boundary.md).
