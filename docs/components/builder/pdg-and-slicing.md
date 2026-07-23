# Builder — Program Dependence Graph and Slicing

The [Program Dependence Graph](../../GLOSSARY.md#program-dependence-graph-pdg) (PDG) is the
overlay that makes [program slicing](../../GLOSSARY.md#program-slicing) a simple graph
reachability query. It adds two dependence relations on top of the CFG/DFG (Ferrante,
Ottenstein & Warren [[1]](#references)):

- **Control dependence** $`a \to b`$: whether $`b`$ executes is decided by branch
  $`a`$. libcpg has no other builder for these — this module computes them and emits
  `CpgEdgeKind::ControlDependence`.
- **Data dependence** $`d \to u`$: use $`u`$ reads a value that definition $`d`$
  produces. The [DFG](dfg.md) already materialises these as `DefUse`/`ReachingDef` edges;
  the PDG re-projects them as first-class `CpgEdgeKind::DataDependence` so a slicer can
  traverse one uniform edge set.

Unlike the AST/CFG/DFG overlays, the PDG is **not** built during construction — it is added
**on demand, per function**, after the CPG already carries CFG and DFG edges.

---

## `PdgBuilder`

```rust
// PdgBuilder, backward_slice, forward_slice are all feature-free (crate root).
use libcpg::PdgBuilder;

// Given a `cpg` whose CFG and DFG have been extracted, and a `function: NodeId`:
// PdgBuilder::new().build(&mut cpg, function);
```

`PdgBuilder::new().build(&mut cpg, function)` computes both dependence relations while
borrowing `cpg` immutably, then emits the edges. It is **idempotent**: it snapshots the
existing `ControlDependence`/`DataDependence` edges first and never duplicates them, so
re-building is safe. It requires that the function's CFG (`ControlFlow` edges) and DFG
(`DataFlow` edges) already exist — i.e. the CPG came from
[`TreeSitterCpgBuilder::build`](overview.md) (or `build_from_tree`) with `build_cfg` /
`build_dfg` enabled, or the extractors were run manually.

![PDG construction: from CFG and DFG overlays to control-dependence and data-dependence edges via the reverse dominance frontier.](../../diagrams/pdg-construction.svg)

*Figure — building the PDG from the CFG (control dependence) and DFG (data dependence). Source: [`diagrams/pdg-construction.puml`](../../diagrams/pdg-construction.puml).*

---

## Control dependence = reverse dominance frontier

The classic result (Cytron, Ferrante, Rosen, Wegman & Zadeck [[2]](#references)) is that
**the control-dependence relation is exactly the
[dominance frontier](../../GLOSSARY.md#dominance-frontier--reverse-dominance-frontier) of
the reverse CFG.** A [post-dominator](../../GLOSSARY.md#dominator--post-dominator) is a
dominator computed on the reversed CFG: $`p`$ post-dominates $`n`$ when every path from
$`n`$ to the exit passes through $`p`$. Node $`b`$ is control-dependent on branch
$`a`$ precisely when $`b`$ post-dominates a successor of $`a`$ but not $`a`$ itself
— the reverse-dominance-frontier condition.

`compute_control_dependence` implements this in six steps:

```text
compute_control_dependence(function):

    # 1. Intraprocedural CFG node set: reachable from `function` over
    #    ControlFlow edges, EXCLUDING Call edges (which cross into callees).
    cfg_nodes ← DFS from function over non-Call ControlFlow successors
    if |cfg_nodes| < 2: return []

    # 2. Build the REVERSE CFG as a DiGraphMap keyed by NodeId.0, plus a
    #    virtual EXIT sentinel (u32::MAX — real ids are small, so no collision).
    #    For each forward edge (a → b) add reverse edge (b → a).
    reverse ← { EXIT } ∪ cfg_nodes
    for each non-Call ControlFlow edge (a → b) with a,b ∈ cfg_nodes:
        reverse.add_edge(b, a)

    # 3. Connect EXIT to every real exit: the CFG builder's RECORDED exits
    #    (e.g. a while-guard's false branch — not a sink), structural sinks
    #    (no successor), and Return/Throw nodes.
    for node in cfg_nodes:
        if node has no successor or is Return/Throw or node ∈ cfg_exits():
            reverse.add_edge(EXIT, node)
    if no exit was connected: return []          # post-dominance undefined

    # 3b. Ensure every node can reach EXIT (an infinite loop with no recorded
    #     exit is wired to EXIT directly — a sound virtual edge).
    for node not reachable from EXIT in `reverse`: reverse.add_edge(EXIT, node)

    # 4. Post-dominators = dominators of the reverse CFG rooted at EXIT.
    postdom ← simple_fast(reverse, EXIT)          # petgraph immediate-dominators

    # 5. Reverse dominance frontier (Cytron et al.). A node `c` that is a JOIN
    #    in the reverse CFG (>= 2 predecessors there = a FORK in the forward
    #    CFG, i.e. a branch) controls every node on the post-dominator-tree
    #    walk from each predecessor up to ipdom(c).
    for c in reverse.nodes(), c ≠ EXIT:
        preds ← reverse-CFG predecessors of c
        if |preds| < 2: continue
        ipdom_c ← postdom.immediate_dominator(c)
        for p in preds:
            runner ← p
            while runner ≠ ipdom_c:
                if runner ≠ EXIT: emit ControlDependence(c → runner)
                runner ← postdom.immediate_dominator(runner)
```

`simple_fast` is petgraph's Cooper–Harvey–Kennedy immediate-dominator algorithm; running it
on a reversed view yields immediate **post**-dominators for free. A loop guard legitimately
appears as its own controller (self-dependence via the back edge), matching the
Ferrante–Ottenstein–Warren construction — the inline test
`while_body_control_dependent_on_guard` asserts exactly this.

![Dominance frontier: post-dominator tree of the reverse CFG and the frontier nodes that become control-dependence edges.](../../diagrams/dominance-frontier.svg)

*Figure — the reverse dominance frontier that defines control dependence. Source: [`diagrams/dominance-frontier.dot`](../../diagrams/dominance-frontier.dot).*

### Two deliberate deviations

- **`function_cfg` is not used.** That helper filters to control-flow/expression nodes and
  drops `Block` nodes — the CFG's connective tissue — which would fragment the graph. The
  builder instead walks the real `ControlFlow` edges on the full CPG.
- **Exits come from `cfg_exits()`, not sink-detection alone.** A `while` guard is a recorded
  exit (its false branch leaves the loop) yet still has a successor (the body), so it is not
  a structural sink; using the recorded exits captures it correctly.

---

## Data dependence

`compute_data_dependence` re-projects the DFG. It walks the function's AST subtree
(`ast_descendants(function)` plus the function itself) and, for every
`DataFlow(DefUse | ReachingDef)` edge whose **both** endpoints lie in that subtree, emits a
deduplicated `DataDependence` edge $`d \to u`$. This keeps the dependence intraprocedural
and gives the slicer a single edge kind to follow regardless of whether the underlying
value flow was recorded as a def-use or a reaching-definition edge.

Together, `ControlDependence` and `DataDependence` are the two edge kinds for which
`CpgEdgeKind::is_pdg()` returns `true` — the exact set the slicer traverses.

---

## Program slicing

A [slice](../../GLOSSARY.md#backward-slice--forward-slice) reduces a program to just the
nodes that affect (backward) or are affected by (forward) a chosen **criterion** node
(Weiser [[3]](#references)). Because both PDG edge kinds point influence-source →
influenced, the two directions are simple bounded breadth-first traversals over PDG edges:

```rust
use libcpg::{backward_slice, forward_slice};

// After PdgBuilder::build(&mut cpg, function):
// let back = backward_slice(&cpg, criterion, 256);   // FxHashSet<NodeId>
// let fwd  = forward_slice(&cpg, criterion, 256);     // FxHashSet<NodeId>
```

Both return `FxHashSet<NodeId>` and always include the criterion itself:

- `backward_slice(&cpg, criterion, max_nodes)` follows **incoming** PDG edges (the
  criterion's PDG predecessors — everything it transitively depends on).
- `forward_slice(&cpg, criterion, max_nodes)` follows **outgoing** PDG edges (its PDG
  successors — everything it transitively influences).

The `max_nodes` argument bounds the traversal: it stops as soon as the slice reaches
`max_nodes` nodes (and returns an empty set if `max_nodes == 0` or the criterion does not
exist). This is the primary defence against a pathological or hostile graph producing an
unbounded slice — see
[`../../security/01-input-and-resource-hardening.md`](../../security/01-input-and-resource-hardening.md).

```text
slice(criterion, max_nodes, direction):        # direction ∈ {Incoming, Outgoing}
    if max_nodes = 0 or criterion ∉ cpg: return ∅
    slice ← { criterion }
    queue ← [ criterion ]
    while queue not empty:
        node ← queue.pop_front()
        neighbors ← PDG edges of `node` in `direction`, filtered by kind.is_pdg()
                    (Incoming → edge.source; Outgoing → edge.target)
        for next in neighbors:
            if |slice| >= max_nodes: return slice         # bound reached
            if slice.insert(next): queue.push_back(next)
    return slice
```

![Slicing BFS: breadth-first traversal over PDG edges from the criterion, bounded by max_nodes.](../../diagrams/slicing-bfs.svg)

*Figure — the bounded breadth-first slice traversal. Source: [`diagrams/slicing-bfs.puml`](../../diagrams/slicing-bfs.puml).*

The traversal is $`O(V + E)`$ in the visited PDG subgraph, capped at `max_nodes` nodes.

---

## Worked example

![Slice example: a backward slice highlighting the definition that reaches a use, plus the branch it is control-dependent on.](../../diagrams/slice-example.svg)

*Figure — a backward slice over a def-use chain and its controlling branch. Source: [`diagrams/slice-example.dot`](../../diagrams/slice-example.dot).*

```rust
// requires: features = ["lang-rust"]
use libcpg::{CpgBuilder, TreeSitterCpgBuilder, PdgBuilder, backward_slice, Language};

let src = "fn f(x: bool) { if x { a(); } else { b(); } }";
let mut cpg = TreeSitterCpgBuilder::new().build(src, Language::Rust)?;
let func = cpg.functions().next().expect("one function");

// Add the PDG overlay for this function, then slice.
PdgBuilder::new().build(&mut cpg, func);

let slice = backward_slice(&cpg, func, 64);
assert!(slice.contains(&func));           // the criterion is always included
# Ok::<(), libcpg::Error>(())
```

For a hand-built (feature-free) def-use chain, the inline test `def_use_backward_slice`
shows the end-to-end guarantee: after `CfgExtractor`, `DfgExtractor`, and `PdgBuilder` have
run, the backward slice of a use contains its reaching definition, the forward slice of the
definition contains the use, and `backward_slice(&cpg, use, 1)` returns exactly one node —
demonstrating the `max_nodes` bound. Because `PdgBuilder`, `backward_slice`, and
`forward_slice` are feature-free, this works without any `lang-*` feature when the CPG is
constructed by hand or via [Mode B](../../GLOSSARY.md#mode-b--build_from_tree).

---

## See also

- [`overview.md`](overview.md) — the construction pipeline the PDG is layered on top of.
- [`cfg.md`](cfg.md) — the control-flow edges and recorded exits the post-dominator step
  consumes.
- [`dfg.md`](dfg.md) — the `DefUse`/`ReachingDef` edges re-projected as data dependence.
- [`../../usage/04-program-slicing.md`](../../usage/04-program-slicing.md) — a task-oriented
  slicing walkthrough.
- [`../../api/builder-reference.md`](../../api/builder-reference.md) — `PdgBuilder`,
  `backward_slice`, and `forward_slice` signatures.
- [`../../theory/04-program-dependence-and-slicing.md`](../../theory/04-program-dependence-and-slicing.md)
  — dominators, dominance frontiers, and slicing theory in full.

---

## References

1. Ferrante, J., Ottenstein, K. J., Warren, J. D. (1987). *The Program Dependence Graph and Its Use in Optimization.* ACM TOPLAS 9(3). DOI: [10.1145/24039.24041](https://doi.org/10.1145/24039.24041)
2. Cytron, R., Ferrante, J., Rosen, B. K., Wegman, M. N., Zadeck, F. K. (1991). *Efficiently Computing Static Single Assignment Form and the Control Dependence Graph.* ACM TOPLAS 13(4). DOI: [10.1145/115372.115320](https://doi.org/10.1145/115372.115320)
3. Weiser, M. (1984). *Program Slicing.* IEEE Transactions on Software Engineering SE-10(4). DOI: [10.1109/TSE.1984.5010248](https://doi.org/10.1109/TSE.1984.5010248) (originally ICSE '81).
