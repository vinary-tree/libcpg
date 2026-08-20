# Theory 45 — Deterministic dependence-aware CPG differencing

A structural diff between Code Property Graphs (CPGs) has two separate
problems: choose a correspondence between old and new nodes, then compare exact
evidence under that correspondence. libcpg uses the bounded bipartite
graph-edit-distance (GED) assignment for the first problem and a canonical
typed-multiset comparison for the second. Program Dependence Graph (PDG) edge
edits then provide a narrow, explicit classification signal.

![Deterministic CPG alignment and exact edit pipeline](../diagrams/cpg-diff-pipeline.svg)

## 1. Inputs and coordinates

Let the old and new CPGs be:

```math
G_o=(V_o,E_o,\kappa_o,\pi_o),\qquad
G_n=(V_n,E_n,\kappa_n,\pi_n),
```

where $`E`$ is a directed edge multiset, $`\kappa`$ is the exact typed edge
relation plus optional label, and $`\pi`$ is the complete represented node
payload. A `NodeId` names a coordinate inside one graph; it is not assumed to
be shared across versions.

An alignment is a partial injective map:

```math
A:V_o\rightharpoonup V_n.
```

Unmapped old nodes are deletions and unmapped new nodes are insertions. The
alignment comes from ADR-0023's deterministic bipartite GED approximation, whose
node substitution costs use coarse `NodeKindTag` compatibility and whose
concrete cost includes typed edge disagreement [[1]](#references). Forward and
reverse assignments are both evaluated; the reverse pairs are inverted into
old-to-new coordinates. The least-cost script wins, with a lexicographic pair
order breaking equal-cost ties. A delete-all candidate prevents a poor forced
assignment from masquerading as useful correspondence.

This alignment is a deterministic heuristic. It is neither a proof that two
nodes denote the same language entity nor exact GED.

## 2. Node equivalence under alignment

For an aligned pair $`(u,A(u))`$, ordinary fields compare exactly. Fields that
contain node coordinates compare after translation:

```math
u_{ref}\equiv_A v_{ref}
\quad\Longleftrightarrow\quad
A(u_{ref})=v_{ref}.
```

The rule applies to call targets, identifier definitions, lambda captures,
cached AST parents, and ordered cached AST children. It avoids reporting a
change merely because the same structural reference was renumbered.

Properties compare by logical value. Snapshots sort the closed keys
`Name`, `Type`, `Scope`, `Visibility`, `Mutable`, `Static`, `Async`, then
custom keys lexicographically. Nested `PropertyValue` data uses the flat
stack-safe representation; source depth never becomes native call depth.

The ordered changed-field vocabulary is:

```text
Kind, Range, Text, MessageKind, LockKind,
AllocationType, Properties, AstLinks
```

## 3. Exact directed edge multisets

For an old edge $`e=(u,v,k,l)`$ whose endpoints are aligned, translation is:

```math
A_E(e)=(A(u),A(v),k,l).
```

Exact translated edges are consumed from canonical multiset buckets first.
This preserves parallel multiplicity. Leftover old and new edges with equal
aligned endpoint pairs are paired in canonical order as substitutions; their
changed fields are `Kind`, `Label`, or both. Every remaining old edge is a
deletion and every remaining new edge is an insertion. Edge ids are retained
in evidence but do not participate in equality.

Literate pseudocode states the complete machine:

```text
DIFF(old, new, caps):
    ADMIT node and edge counts, or return BudgetExceeded
    alignment := least canonical GED script(old, new)
    old_to_new := map(alignment)

    FOR each aligned node pair in canonical order:
        compare exact payload under old_to_new
        append one node substitution when fields differ

    append snapshots for every unaligned old/new node

    index every new edge by (source, target, exact kind, label)
    consume exact translated old-edge matches first
    bucket remaining edges by aligned endpoint pair
    pair bucket prefixes as exact kind/label substitutions
    append every unpaired edge as deletion or insertion

    IF any edge edit has old or new PDG kind:
        classification := BehaviorChanging
    ELSE:
        classification := Refactoring

    RETURN the complete owned script, or BudgetExceeded
```

Every appended record first claims one edit-budget unit. Failure discards local
work through ordinary result ownership; callers never observe a partial script.

## 4. Dependence-aware classification

Let $`\Delta_E`$ be all inserted, removed, and substituted edges, and let
$`P(e)`$ mean that an edge kind is `ControlDependence` or `DataDependence`.
Then:

```math
\operatorname{kind}(\Delta)=
\begin{cases}
\textsf{BehaviorChanging}, & \exists e\in\Delta_E:P(e),\\
\textsf{Refactoring}, & \text{otherwise.}
\end{cases}
```

![Dependence-aware edit classification boundary](../diagrams/cpg-diff-classification.svg)

The name is deliberately evidence-bounded. Build comparable PDGs on both
graphs before calling `cpg_diff` when the classification matters. With no PDG,
an added branch may alter nodes and CFG edges yet still classify as
`Refactoring`. Conversely, `BehaviorChanging` says represented dependence
changed; it does not prove an observable runtime difference.

## 5. Determinism and output order

The result order is a contract:

1. alignments sort by old id then new id;
2. aligned node substitutions follow alignment order;
3. unaligned nodes sort by their local ids;
4. edges sort by source, target, exhaustive edge class, label, then local id;
5. changed fields follow their declaration order; and
6. property records follow the closed key order above.

Changing insertion order can change petgraph indices, but none of those
indices enter the output. Canonical ordering also makes serde bytes stable for
equal logical inputs.

## 6. Resource and stack bounds

For $`N=\max(|V_o|,|V_n|)`$ and
$`M=\max(|E_o|,|E_n|)`$, alignment costs $`O(N^3)`$ time and $`O(N^2)`$
space. Exact edge comparison is $`O(M\log M)`$ time because it sorts and uses
ordered buckets, with $`O(M)`$ auxiliary space. Node/payload comparison and
output construction are linear in the represented payload and edit volume.

The hard ceilings are 512 nodes, 2,000,000 edges per graph, and 2,000,000 edit
records. Defaults retain the node ceiling while admitting 200,000 edges and
200,000 edits. The effective cap is the lesser of caller and compile-time
values.

All matrices, queues, maps, buckets, snapshots, and output records live on the
heap. The result boxes large node-change variants. `Arc<str>` cloning shares
text storage, but arbitrary graph payload size is still an ingress concern.

## 7. Structural boundary

`cpg_diff` compares represented nodes and edges. It does not compare the CPG
container's language, source path, retained whole-source text, root field, or
recorded CFG entry/exit side tables. Consumers that version those fields must
compare them separately.

See the [component contract](../components/graph/cpg-diff.md), [usage
guide](../usage/28-cpg-diff.md), [ADR-0055](../design/0055-deterministic-dependence-aware-cpg-diff.md), [validation dossier](../scientific/50-cpg-diff-validation.md), and
[security boundary](../security/20-cpg-diff-boundary.md).

## References

1. Riesen, K., Bunke, H. (2009). *Approximate graph edit distance computation
   by means of bipartite graph matching.* Image and Vision Computing 27(7).
   DOI: [10.1016/j.imavis.2008.04.004](https://doi.org/10.1016/j.imavis.2008.04.004).
2. Ferrante, J., Ottenstein, K. J., Warren, J. D. (1987). *The Program
   Dependence Graph and Its Use in Optimization.* ACM TOPLAS 9(3). DOI:
   [10.1145/24039.24041](https://doi.org/10.1145/24039.24041).
