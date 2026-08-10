# Dominator and post-dominator analysis

`libcpg` provides deterministic, feature-free dominator trees for any
[`GraphProjection`](projections.md) and post-dominator trees for one function's
control-flow graph (CFG). The same post-dominator result is the single source of
truth for [`PdgBuilder`](../builder/pdg-and-slicing.md) control-dependence edges.

Dominance answers a necessary-path question. For a root $`r`$, node $`a`$
dominates node $`b`$ exactly when every directed path from $`r`$ to $`b`$
contains $`a`$. Post-dominance reverses the question: $`a`$ post-dominates
$`b`$ when every modeled path from $`b`$ to an exit contains $`a`$.

![A graph projection flows through deterministic reverse postorder and the CHK fixed point into dominator, post-dominator, frontier, and PDG results.](../../diagrams/dominator-analysis.svg)

*Figure — one CHK engine serves forward dominance and reversed-CFG
post-dominance. Source: [`diagrams/dominator-analysis.puml`](../../diagrams/dominator-analysis.puml).*

## Public API

The functions and result types are available from both `libcpg::analysis` and
the crate root:

```rust
use libcpg::{
    dominator_tree, post_dominator_tree, AnalysisError, CodePropertyGraph,
    DominatorTree, GraphProjection, NodeId, PostDominatorTree,
};

fn dominance(
    graph: &GraphProjection,
    entry: NodeId,
) -> Result<DominatorTree, AnalysisError> {
    dominator_tree(graph, entry)
}

fn post_dominance(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<PostDominatorTree, AnalysisError> {
    post_dominator_tree(cpg, function)
}
```

```rust
pub fn dominator_tree(
    graph: &GraphProjection,
    root: NodeId,
) -> Result<DominatorTree, AnalysisError>;

pub fn post_dominator_tree(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<PostDominatorTree, AnalysisError>;
```

Both operations are read-only. They return stable [`NodeId`](nodes.md)
values and never mutate the projection or CPG.

## `DominatorTree` contract

Only nodes reachable from `root` participate. An unreachable projection node
is absent from all maps, `dominates(a, unreachable)` is `false`, and
`dominators_of(unreachable)` is empty.

| Field or method | Meaning |
|---|---|
| `root` | The caller-selected reachable root. |
| `idom` | Immediate dominator of every reachable non-root node. The root is intentionally absent. |
| `children` | Sorted dominator-tree children for every reachable node, including leaves with an empty vector. |
| `frontier` | Sorted Cytron dominance frontier for every reachable node. |
| `immediate_dominator(node)` | `idom[node]`, or `None` for the root and unrepresented nodes. |
| `dominates(a, b)` | Constant-time tree-ancestry test. Unknown and unreachable nodes return `false`. |
| `dominators_of(node)` | Iterates from `node` through its immediate dominators to `root`; empty for an unrepresented node and size-bounded for a malformed deserialized parent cycle. |

The dominance frontier of $`x`$ is

$`DF(x) = \{ y \mid \exists p \in \operatorname{pred}(y): x\ \mathrm{dom}\ p \land \neg(x\ \mathrm{sdom}\ y) \}`$,

where $`\mathrm{dom}`$ denotes dominance and $`\mathrm{sdom}`$ denotes strict
dominance (dominance between distinct nodes).

It identifies the join points immediately outside the region strictly
dominated by $`x`$. Map and vector order is ascending by `NodeId`, independent
of edge insertion or hash-table iteration order.

This feature-free example constructs a diamond with sparse node identifiers:

```rust
use libcpg::{dominator_tree, GraphProjection, NodeId, ProjectionKind};

let entry = NodeId::new(10);
let left = NodeId::new(20);
let right = NodeId::new(30);
let join = NodeId::new(40);
let graph = GraphProjection::from_edges(
    ProjectionKind::External,
    [entry, left, right, join],
    [
        (entry, left, 1.0),
        (entry, right, 1.0),
        (left, join, 1.0),
        (right, join, 1.0),
    ],
);

let tree = dominator_tree(&graph, entry)?;
assert_eq!(tree.immediate_dominator(join), Some(entry));
assert_eq!(tree.frontier[&left], vec![join]);
assert!(tree.dominates(entry, join));
# Ok::<(), libcpg::AnalysisError>(())
```

## `PostDominatorTree` contract

`post_dominator_tree` validates that `function` exists and is a function, then
uses the shared crate-private `FunctionCfgView` to select its CFG reachable from
the function node. Interprocedural `CfgEdgeKind::Call` edges are excluded. An
absent selector returns `AnalysisError::UnknownNode`; a non-function selector
returns `AnalysisError::NotAFunction`. Its modeled exit set is the sorted union
of:

- exits recorded by the CFG extractor;
- structural sinks with no intraprocedural CFG successor; and
- nodes whose kind is `Return` or `Throw`, even if malformed input gives them
  an outgoing edge.

A private synthetic exit connects to those nodes in the reversed CFG. The
synthetic identifier is a dense index outside the projection, never a sentinel
`NodeId`, so it cannot collide with user data. A region that cannot reach any
modeled exit is attached directly to the synthetic root. This makes the tree
total without pretending that one real exit post-dominates an infinite region.

| Field or method | Meaning |
|---|---|
| `exits` | Sorted explicit and structural real exits. |
| `ipdom` | Immediate real post-dominator. Nodes immediately below the synthetic root are absent. |
| `virtual_children` | Sorted real children of the synthetic exit, including disconnected infinite regions when present. |
| `frontier` | Reverse dominance frontier keyed by dependent node; each value is its sorted controller set. |
| `post_dominates(a, b)` | Constant-time ancestry test in the post-dominator tree. |
| `control_dependence_edges()` | Sorted, deduplicated `(controller, dependent)` pairs derived from `frontier`. |
| `is_empty()` | Whether post-dominance is undefined because the CFG has fewer than two nodes or no modeled exit. |

An exit-free CFG deliberately returns `Ok` with an empty tree. This is different
from an invalid function selector, which returns a typed `AnalysisError`. The
empty result lets callers analyze incomplete or non-terminating code without
inventing an exit relation. `PdgBuilder` consumes `control_dependence_edges()`
directly, preserving its graceful behavior on malformed inputs.

## Algorithm

Both APIs use the Cooper–Harvey–Kennedy (CHK) iterative immediate-dominator
algorithm. `GraphProjection` supplies canonical forward and reverse adjacency,
and all additional adjacency vectors are sorted and deduplicated.

```text
CHK(graph, root):
    rpo = iterative_reverse_postorder(graph, root)
    idom[root] = root

    repeat at most |V| + 2 passes:
        changed = false
        for node in rpo excluding root:
            known = predecessors(node) whose idom is known
            candidate = first(known)
            for predecessor in rest(known):
                candidate = intersect(candidate, predecessor, idom, rpo_position)
            update idom[node] and changed
        stop when not changed

    children = invert(idom)
    frontier = cytron_frontier(predecessors, idom)
    intervals = iterative_euler_tour(children)
```

`dominator_tree` applies CHK directly. `post_dominator_tree` obtains the
canonical reachable CFG and exit-like set from `analysis::cfg_scope`, reverses
that projection, adds the synthetic exit root, and invokes the same engine.
An iterative DFS computes reverse postorder, and an iterative Euler tour assigns
private entry/exit timestamps. Therefore deep paths do not consume the native
call stack, while interval containment implements `dominates` and
`post_dominates` in $`O(1)`$ time.

The fixed-point cap is the analyzed graph's vertex count plus two. Failure to
converge within that structural bound returns
`AnalysisError::IterationLimitExceeded` instead of running without a bound.
CHK visits all relevant predecessor edges per pass, so its conservative
worst-case time bound is $`O(VE)`$ and its auxiliary-space bound is
$`O(V + E)`$. Frontier construction is bounded by $`O(VE)`$ in the worst case.
`dominators_of` is $`O(h)`$ for dominator-tree height $`h`$.

## Determinism, serialization, and validation

`BTreeMap` result maps, sorted children/frontier vectors, canonical CSR input,
and stable reverse postorder make results byte-for-byte deterministic for the
same node and edge set. Parallel projection edges do not alter dominance.

With the `serde` feature, both tree types serialize their public relation and
private ancestry index together. Round-tripping therefore preserves constant-
time dominance queries as well as visible maps.

Validation includes exact diamond and conditional examples, unknown and
non-function errors, singleton and exit-free CFGs, mixed finite/infinite
regions, a 25,000-node path, serde round-tripping, and three 256-case
property-based suites. The shared CFG view is invariant under node, edge, and
exit insertion permutations and is checked against the former PDG scope rules
over a parsed Rust corpus. Forward immediate dominators are checked against
`petgraph::algo::dominators::simple_fast`; post-dominators and control
dependence are checked against the former production PDG algorithm. Parsed-Rust
integration tests also prove that `PdgBuilder` emits exactly the public tree's
control-dependence relation. `petgraph` is an independent test oracle only; the
production analysis uses `GraphProjection`.

## References

- Cooper, K. D., Harvey, T. J., Kennedy, K. (2001). *A Simple, Fast Dominance Algorithm.* Rice University Computer Science Technical Report TR-06-33870.
- Cytron, R., Ferrante, J., Rosen, B. K., Wegman, M. N., Zadeck, F. K. (1991). *Efficiently Computing Static Single Assignment Form and the Control Dependence Graph.* ACM TOPLAS 13(4), 451–490. DOI: [10.1145/115372.115320](https://doi.org/10.1145/115372.115320).
- Lengauer, T., Tarjan, R. E. (1979). *A Fast Algorithm for Finding Dominators in a Flowgraph.* ACM TOPLAS 1(1), 121–141. DOI: [10.1145/357062.357071](https://doi.org/10.1145/357062.357071).
- Ferrante, J., Ottenstein, K. J., Warren, J. D. (1987). *The Program Dependence Graph and Its Use in Optimization.* ACM TOPLAS 9(3), 319–349. DOI: [10.1145/24039.24041](https://doi.org/10.1145/24039.24041).
