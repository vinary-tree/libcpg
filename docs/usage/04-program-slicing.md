# Program Slicing

[Program slicing](../GLOSSARY.md#program-slicing) reduces a program to just the statements that affect — or are affected by — a chosen point of interest, the **slicing criterion**. It was introduced by Weiser [[1]](#references) and is one of the highest-value things a CPG enables: debugging ("what could have produced this wrong value?"), change-impact analysis ("what does editing this line break?"), and security triage all reduce to a slice.

In libcpg, slicing is a graph-reachability query over the [Program Dependence Graph](../GLOSSARY.md#program-dependence-graph-pdg) (PDG). That means it is a **two-step process**, and the order matters:

1. **Build the PDG** for the function with `PdgBuilder::new().build(&mut cpg, function)`. This *adds* [`ControlDependence`](../GLOSSARY.md#control-dependence) and [`DataDependence`](../GLOSSARY.md#data-dependence) edges to the graph.
2. **Slice** with `backward_slice(&cpg, criterion, max_nodes)` or `forward_slice(&cpg, criterion, max_nodes)`, which walk those PDG edges.

> If you slice **before** building the PDG, you get back only the criterion itself — there are no dependence edges to follow yet. This is the single most common slicing mistake; do step 1 first.

Both slicers are **feature-free** and live at the crate root, alongside `PdgBuilder`.

![A small function's PDG with a highlighted backward slice from a criterion node.](../diagrams/slice-example.svg)

*Figure — a backward slice highlighted over a function's dependence edges. Source: [`diagrams/slice-example.dot`](../diagrams/slice-example.dot).*

---

## Step 1: build the PDG

`PdgBuilder::build` requires that the function's CFG and DFG already exist, because control dependence is derived from the CFG (as the [reverse dominance frontier](../GLOSSARY.md#dominance-frontier--reverse-dominance-frontier), following Cytron et al.) and data dependence is re-projected from DFG def-use edges. A CPG from `build`/`build_from_tree` with the default config already has both overlays, so you can build the PDG straight away. It targets **one function at a time** and is [idempotent](../GLOSSARY.md#idempotent) — re-running never duplicates edges.

```rust
// requires: features = ["lang-rust"]
use libcpg::{CpgBuilder, Language, PdgBuilder, TreeSitterCpgBuilder};

fn main() -> Result<(), libcpg::Error> {
    let source = "fn f(x: bool) { if x { a(); } else { b(); } }";
    let mut cpg = TreeSitterCpgBuilder::new().build(source, Language::Rust)?;

    // Pick a function and add its PDG edges.
    let func = cpg.functions().map(|n| n.id).next().expect("a function node");
    PdgBuilder::new().build(&mut cpg, func);
    // `cpg` now carries ControlDependence + DataDependence edges for `func`.
    Ok(())
}
```

If you constructed the CPG with `with_cfg(false)` or `with_dfg(false)`, run the extractors first (`CfgExtractor::new().extract(&mut cpg)` then `DfgExtractor::new().extract(&mut cpg)`) before `PdgBuilder`. The construction details — the virtual `EXIT`, `petgraph`'s `simple_fast` dominators, and the Cytron frontier walk — are in [PDG and Slicing](../components/builder/pdg-and-slicing.md).

---

## Step 2: take a slice

With PDG edges in place, slicing is one call. The **backward** slice of a criterion `` $`s`$ `` is every node that can affect `` $`s`$ `` (its transitive PDG predecessors); the **forward** slice is every node that `` $`s`$ `` can affect (its transitive successors). Both **include the criterion itself**.

```rust
// requires: features = ["lang-rust"]
use libcpg::{backward_slice, CpgBuilder, Language, PdgBuilder, TreeSitterCpgBuilder};

fn main() -> Result<(), libcpg::Error> {
    let source = "fn f(x: bool) { if x { a(); } else { b(); } }";
    let mut cpg = TreeSitterCpgBuilder::new().build(source, Language::Rust)?;
    let func = cpg.functions().map(|n| n.id).next().expect("a function node");

    PdgBuilder::new().build(&mut cpg, func);

    // Backward slice from `func`, bounded to 64 nodes.
    let slice = backward_slice(&cpg, func, 64);
    assert!(slice.contains(&func)); // the criterion is always in its own slice

    for node_id in &slice {
        if let Some(node) = cpg.node(*node_id) {
            println!("in slice: {:?}", node.kind);
        }
    }
    Ok(())
}
```

Both slicers return a `FxHashSet<NodeId>` (the fast hash-set from `rustc-hash`). You rarely need to name the type — use it directly via `contains`, `len`, and iteration — but if you want a `Vec` or a `std::collections::HashSet`, just collect: `let v: Vec<_> = slice.into_iter().collect();`.

---

## A worked, feature-free example

The clearest way to *see* a slice is to build a tiny function by hand — a definition of `x` and a later use of `x` — run the overlays, build the PDG, and confirm the backward slice of the use reaches its [reaching definition](../GLOSSARY.md#reaching-definition). This mirrors libcpg's own `def_use_backward_slice` test.

```rust
// Feature-free: hand-build, extract overlays, build the PDG, then slice.
use libcpg::{
    backward_slice, forward_slice, CfgExtractor, CodePropertyGraph, CpgEdgeKind, CpgNode,
    CpgNodeKind, DfgExtractor, Language, MethodSignature, NodeId, PdgBuilder, ScopeId,
    SourceRange, Visibility,
};

/// Adds `kind` as an AST child of `parent`, wiring the edge, the parent
/// pointer, and the child list — the shape the extractors read.
fn add_child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let id = cpg.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()));
    cpg.connect(parent, id, CpgEdgeKind::AstChild);
    if let Some(node) = cpg.node_mut(id) {
        node.parent = Some(parent);
    }
    if let Some(p) = cpg.node_mut(parent) {
        p.children.push(id);
    }
    id
}

fn main() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);

    // fn f { <def x>; <use x> }
    let func = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Function {
            signature: MethodSignature {
                name: "f".into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Public,
            },
        },
        SourceRange::default(),
    ));
    let body = add_child(&mut cpg, func, CpgNodeKind::Block { scope: ScopeId::GLOBAL });
    let def_x = add_child(
        &mut cpg,
        body,
        CpgNodeKind::Variable { name: "x".into(), var_type: None, scope: ScopeId::GLOBAL, is_mutable: false },
    );
    let use_x = add_child(
        &mut cpg,
        body,
        CpgNodeKind::Identifier { name: "x".into(), definition: None },
    );

    // Overlays first, then the PDG.
    CfgExtractor::new().extract(&mut cpg);
    DfgExtractor::new().extract(&mut cpg);
    PdgBuilder::new().build(&mut cpg, func);

    // Backward slice of the use contains the use AND its reaching definition.
    let slice = backward_slice(&cpg, use_x, 100);
    assert!(slice.contains(&use_x));
    assert!(slice.contains(&def_x));

    // Forward slice of the def reaches the use.
    let forward = forward_slice(&cpg, def_x, 100);
    assert!(forward.contains(&def_x));
    assert!(forward.contains(&use_x));
}
```

The def-use edge produced by the DFG is re-projected by `PdgBuilder` as a `DataDependence` edge; the backward slice walks it from `use_x` back to `def_x`. That is the entire mechanism.

![Breadth-first slice expansion over PDG edges from the criterion outward.](../diagrams/slicing-bfs.svg)

*Figure — slicing as a bounded breadth-first traversal of PDG edges. Source: [`diagrams/slicing-bfs.puml`](../diagrams/slicing-bfs.puml).*

---

## Interpreting and bounding a slice

**What a slice means.** A backward slice answers "which statements could have influenced the value/behaviour at the criterion?" — it is exactly the transitive control- and data-dependence predecessors. A forward slice answers the dual: "which statements does the criterion influence?" A node that appears in *neither* the backward slice of a bug site nor any relevant forward slice is provably irrelevant to it.

**The `max_nodes` bound.** Slicing is a breadth-first traversal that **stops once the slice reaches `max_nodes` nodes**. This is a hard cap, not a hint — pass a generous number for exhaustive slices (the example uses `100`), or a small number to bound work on hostile or very large inputs. A `max_nodes` of `0` yields an empty set; `1` yields just the criterion (as the worked example asserts). Recommended caps for untrusted input are discussed in [Input and Resource Hardening](../security/01-input-and-resource-hardening.md).

**Empty slices.** If a slice comes back containing only the criterion, the usual cause is a forgotten PDG build (Step 1) — the graph had no dependence edges to follow. Confirm by checking that `cpg.edges_by_kind(|k| k.is_pdg()).count()` is non-zero after `PdgBuilder::build`.

---

## Next steps

- The theory — dominators, dominance frontiers, control vs data dependence, and Weiser's algorithm — is in [Program Dependence and Slicing](../theory/04-program-dependence-and-slicing.md).
- The construction internals are in [PDG and Slicing](../components/builder/pdg-and-slicing.md).
- Slicing composes naturally with the [taint walk](02-querying-and-traversal.md#worked-example-a-taint-style-data-flow-walk): a forward slice is a dependence-aware "what does this affect?" that respects control flow as well as data flow.

---

## References

1. Weiser, M. (1984). *Program Slicing.* IEEE Transactions on Software Engineering SE-10(4). DOI: [10.1109/TSE.1984.5010248](https://doi.org/10.1109/TSE.1984.5010248) (originally ICSE '81).
</content>
