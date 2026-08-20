# ADR-0055: Produce deterministic dependence-aware CPG edit scripts

- **Status:** Accepted
- **Date:** 2026-08-19
- **Scope:** feature-free `analysis::diff`

## Context

Consumers need to compare two versions of a Code Property Graph (CPG) without
confusing graph-local identifiers with semantic identity. A useful result must
align structurally corresponding nodes, preserve exact typed and labelled edge
multiplicity, expose concrete additions/deletions/substitutions, and remain
stable when either graph was inserted in a different order.

The existing bounded graph-edit-distance (GED) analysis computes a scalar
upper bound with two deterministic Hungarian assignments [[1]](#references).
Its internal assignment already contains the correspondence needed by a diff,
but the public scalar deliberately does not expose an edit script. Rebuilding a
separate matcher would create competing alignment semantics.

“Behavior-changing” also needs a precise evidence boundary. libcpg represents
control and data dependence as Program Dependence Graph (PDG) edges. A changed
PDG edge is review-worthy behavioral evidence; an unchanged or absent PDG is
not a proof of runtime equivalence.

## Decision

1. Add a feature-free `analysis::diff` module and crate-root `cpg_diff` API.
2. Reuse ADR-0023's symmetric bipartite GED assignment. Compare forward and
   reverse assignments plus the delete-all script, retaining the least-cost
   correspondence with a canonical lexicographic tie-break.
3. Treat `NodeId` and `EdgeId` as graph-local coordinates. Compare semantic
   node payloads and edges after translating old node references through the
   chosen alignment.
4. Snapshot every changed/added/removed node completely: kind, range, text,
   normalized message/lock/allocation facts, canonically ordered properties,
   and cached AST parent/children. Translate embedded node references in calls,
   identifiers, lambda captures, and AST links when testing equivalence.
5. Compare directed edges as exact typed, labelled multisets. Match exact edges
   first. Pair remaining edges with the same aligned endpoints as substitutions;
   retain all other leftovers as insertions or deletions. Ignore edge ids for
   equality but retain them in evidence snapshots.
6. Return `Refactoring` only when the concrete script contains no inserted,
   removed, or substituted `ControlDependence`/`DataDependence` edge. Return
   `BehaviorChanging` when at least one such PDG edge changes.
7. Keep alignment, node, edge, and edit ordering canonical and independent of
   graph storage order. Optional serde derives preserve the complete records.
8. Bound nodes, represented edges, and returned edit records independently.
   Exceeding any effective cap returns `BudgetExceeded`; no partial script is
   returned.
9. Store assignments, buckets, property snapshots, edit records, and traversal
   continuations on the heap. Reuse the stack-safe flat `PropertyValue`
   lifecycle when cloning property payloads.
10. Leave graph-container metadata—language, source path/source text, root, and
    recorded CFG entries/exits—outside this structural diff. The result covers
    represented nodes and edges only and does not mutate either graph.

![Deterministic CPG alignment and exact edit pipeline](../diagrams/cpg-diff-pipeline.svg)

![Dependence-aware edit classification boundary](../diagrams/cpg-diff-classification.svg)

## Rejected alternatives

### Match equal stable ids

Rejected. Stable ids persist inside one graph, but independently constructed or
reparsed versions can assign different ids to corresponding syntax. Equality by
id would turn harmless renumbering into delete-and-insert noise.

### Hand-roll another parser or syntax matcher

Rejected. The inputs are already typed CPGs, and ADR-0023 already owns bounded GED
alignment. The diff neither parses source text nor invents a second matching
algorithm.

### Classify every node edit as behavior-changing

Rejected. A spelling/range/property change is useful structural evidence, but
it is not represented dependence evidence. This would mislabel pure renames
and obscure the exact reason for classification.

### Infer behavioral equivalence when no PDG edge changes

Rejected. PDGs are constructed on demand and are an abstraction of represented
dependencies. `Refactoring` means only “no represented PDG edge edit in this
script.” It is not observational or language-semantic equivalence.

### Collapse parallel edges or compare only coarse overlay classes

Rejected. Multiplicity, exact relation subtype, direction, and label are all
part of CPG evidence. Collapsing them would hide real graph changes.

### Return a truncated edit prefix

Rejected. A prefix can omit the dependence edge that decides classification.
Complete-or-error admission preserves the meaning of both the script and its
label.

## Consequences

### Positive

- One alignment implementation now serves both scalar GED and concrete diffs.
- Renumbered but structurally corresponding graphs compare in aligned
  coordinates.
- Exact node fields and edge relations remain auditable and serializable.
- PDG changes have a deterministic, explicit review label.
- Hostile graph size, edge multiplicity, edit volume, and source depth have
  bounded memory/native-stack behavior.

### Negative

- Bipartite GED alignment is heuristic correspondence, not exact graph
  isomorphism or semantic identity.
- Hungarian assignment is cubic in the admitted node count and requires a
  quadratic matrix, so the hard node ceiling remains 512.
- Exact edge indexing is linear in represented edge count and can be large up
  to the independent edge cap.
- Complete snapshots clone node payloads. `Arc<str>` text is cheap to clone,
  but callers must still bound hostile input payload size at graph admission.
- A graph without built PDG edges can receive `Refactoring` even when source
  behavior changed.
- Container metadata changes are intentionally invisible.

## Verification

Exact examples pin identical graphs, pure renames, an added control guard,
typed parallel-edge substitutions, translated embedded node ids, canonical
property order, every budget, malformed metadata, and serde round trips. A
128-case property reverses graph storage order and requires byte-stable logical
results. Public integration distinguishes rename evidence from a parsed Rust
guard after `PdgBuilder` runs. A depth-100,000 flat property and a 128-node
alignment complete on a 256 KiB native stack.

The scalar GED regression suite still validates brute-force Hungarian
optimality and concrete-script bounds. Feature-free, all-feature, clippy,
rustdoc, documentation, and complete libcpg verification gates pass. Runtime
benchmarking remains an explicitly recorded gap rather than a correctness
claim.

## References

1. Riesen, K., Bunke, H. (2009). *Approximate graph edit distance computation
   by means of bipartite graph matching.* Image and Vision Computing 27(7).
   DOI: [10.1016/j.imavis.2008.04.004](https://doi.org/10.1016/j.imavis.2008.04.004).
2. Ferrante, J., Ottenstein, K. J., Warren, J. D. (1987). *The Program
   Dependence Graph and Its Use in Optimization.* ACM TOPLAS 9(3). DOI:
   [10.1145/24039.24041](https://doi.org/10.1145/24039.24041).
