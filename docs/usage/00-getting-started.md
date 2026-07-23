# Getting Started with libcpg

`libcpg` is a Rust library for building and analysing **Code Property Graphs** (CPGs) — a single graph that overlays a program's [Abstract Syntax Tree](../GLOSSARY.md#abstract-syntax-tree-ast), [Control Flow Graph](../GLOSSARY.md#control-flow-graph-cfg), [Data Flow Graph](../GLOSSARY.md#data-flow-graph-dfg), and (on demand) [Program Dependence Graph](../GLOSSARY.md#program-dependence-graph-pdg) onto one shared node set. The CPG idea comes from Yamaguchi et al. [[1]](#references), who introduced it so that a *single* query could reason about syntax, control, and data flow at once — the combination needed to express real vulnerability patterns.

This page gets you from an empty `Cargo.toml` to your first inspected CPG. It covers:

1. Adding the dependency and understanding why `default = []` matters.
2. Choosing [feature flags](../GLOSSARY.md#feature-flag-cargo) for the languages and analyses you need.
3. Building your first CPG two ways — a **feature-free hand-built** graph, and a parsed graph via the `lang-rust` grammar.
4. Inspecting node/edge counts and [`CpgStats`](#step-4c--inspect-counts-and-statistics).

Once you are comfortable here, continue to [Building CPGs](01-building-cpgs.md) for the full construction surface, then [Querying and Traversal](02-querying-and-traversal.md).

---

## Step 1 — Add the dependency

`libcpg` is published as the crate `libcpg` (v0.1.1, edition 2021, licensed MIT OR Apache-2.0). Add it to your `Cargo.toml`:

```toml
[dependencies]
libcpg = "0.1"
```

That single line pulls in the **core graph machinery** — the `CodePropertyGraph` type, all node/edge kinds, the always-on [VF2](../GLOSSARY.md#vf2) matcher, the PDG builder, and the program slicer — but **no language grammars and no optional analyses**. That is deliberate, and it is the first thing to understand about the library.

---

## Step 2 — Understand `default = []`

`libcpg` ships with an **empty default feature set**:

```toml
# From libcpg's own Cargo.toml
[features]
default = []
```

The practical consequence is blunt and worth stating up front:

> With no features enabled, `TreeSitterCpgBuilder::build(source, language)` **fails for every language** — there is no grammar registered to parse with. The only construction paths that work feature-free are a **hand-built** CPG (you add nodes and edges yourself) and **`build_from_tree`** (you hand libcpg a tree you parsed with your own grammar; this is [Mode B](../GLOSSARY.md#mode-b--build_from_tree)).

Why design it this way? Three reasons, expanded in [ADR-0005](../design/0005-feature-flag-taxonomy.md):

- **Compile time & dependency surface.** Each of the 16 tree-sitter grammars is a C library; pulling all of them in unconditionally would balloon build times and the dependency tree for users who need only one language.
- **Duplicate-symbol avoidance.** A host that already links, say, `tree-sitter-python` would collide with a second copy at link time. Keeping grammars opt-in (and Rholang/MeTTa out of the regular dependency graph entirely) sidesteps this.
- **Pay only for what you use.** Pattern detection, algorithm detection, the GNN, and serde are each independent opt-ins.

So the very next thing you do is pick features.

---

## Step 3 — Choose your features

Features fall into a few groups. Enable the ones your task needs.

| Group | Flags | Enables |
|-------|-------|---------|
| Language grammars | `lang-rust`, `lang-python`, `lang-javascript`, `lang-typescript`, `lang-go`, `lang-java`, `lang-c`, `lang-cpp`, `lang-json`, `lang-html`, `lang-css`, `lang-bash`, `lang-toml`, `lang-yaml`, `lang-markdown`, `lang-ruby` | Registers that grammar so `build()` can parse it |
| Language bundles | `lang-systems`, `lang-scripting`, `lang-web`, `lang-config`, `lang-all` | Convenience groups of the above |
| Design patterns | `design-patterns` | The [Gang-of-Four](../GLOSSARY.md#gang-of-four-gof) detector and DPML/classifier |
| Algorithm detection | `algorithm-detection` | Per-function algorithm & [complexity](../GLOSSARY.md#complexity-class--big-o) analysis |
| GNN | `gnn` | Graph-neural-network [embeddings](../GLOSSARY.md#embedding) |
| Serialisation | `serde` | `Serialize`/`Deserialize` on the graph types |
| F1R3FLY.io mappers | `rholang`, `metta` | The [Rholang](../GLOSSARY.md#rholang)/[MeTTa](../GLOSSARY.md#metta) Mode-B node-mappers |
| Umbrella | `full` | `gnn` + `design-patterns` + `algorithm-detection` + `serde` + `ml-rules` + `lang-all` |

Note that `full` intentionally **excludes** `rholang`, `metta`, `gpu`, and `ml-linfa` — those are specialised opt-ins.

![Map of libcpg feature flags, their dependencies, and the code they gate.](../diagrams/feature-flag-map.svg)

*Figure — the feature-flag taxonomy: how `lang-*`, analysis, and umbrella features relate. Source: [`diagrams/feature-flag-map.puml`](../diagrams/feature-flag-map.puml).*

For this guide, enable Rust parsing:

```toml
[dependencies]
libcpg = { version = "0.1", features = ["lang-rust"] }
```

A fuller setup for someone doing pattern and complexity work on scripting languages might be:

```toml
[dependencies]
libcpg = { version = "0.1", features = ["lang-scripting", "design-patterns", "algorithm-detection", "serde"] }
```

The full feature matrix, the grammar version pins, and the reasoning behind them live in [Build and Features](../engineering/01-build-and-features.md).

---

## Step 4 — Build your first CPG (feature-free, hand-built)

Because the core graph is always available, you can construct a CPG **with no features at all** by adding nodes and edges directly. This is exactly how libcpg's own extractor tests build fixtures, and it is the clearest way to see the data model.

A CPG is a directed graph of [`CpgNode`](../components/graph/nodes.md) values connected by typed [`CpgEdge`](../components/graph/edges.md) values. `add_node` returns a freshly assigned [`NodeId`](../GLOSSARY.md#node-kind--edge-kind); `connect` adds a typed edge and returns `Option<EdgeId>` (it returns `None` if either endpoint is missing).

```rust
// Feature-free: no lang-* grammar required.
use libcpg::{
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, Language, MethodSignature,
    NodeId, SourceRange, Visibility,
};

// An empty graph, tagged with the language it models.
let mut cpg = CodePropertyGraph::new(Language::Rust);

// The AST root. `add_node` assigns the real id, so the placeholder
// `NodeId::new(0)` passed to the constructor is overwritten.
let root = cpg.add_node(CpgNode::new(
    NodeId::new(0),
    CpgNodeKind::Root,
    SourceRange::default(),
));

// A function node. `Function` carries a `MethodSignature`.
let func = cpg.add_node(CpgNode::new(
    NodeId::new(0),
    CpgNodeKind::Function {
        signature: MethodSignature {
            name: "main".into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Public,
        },
    },
    SourceRange::default(),
));

// Wire the function as an AST child of the root.
cpg.connect(root, func, CpgEdgeKind::AstChild);

assert_eq!(cpg.node_count(), 2);
assert_eq!(cpg.edge_count(), 1);
assert_eq!(cpg.language(), Language::Rust);
```

A few things worth noticing, because they recur throughout the API:

- **`CpgNodeKind` is an enum with 45 variants**, a mix of unit variants (`Root`, `If`, `Return`, …) and data-carrying ones (`Function { signature }`, `Call { target, is_method }`, `Identifier { name, definition }`, …). See the [node reference](../components/graph/nodes.md).
- **Node and edge fields are public.** You read `node.kind`, `node.range`, `edge.source` — not method calls like `node.kind()`.
- **`node(id)` returns `Option`, not `Result`.** A missing id is `None`.
- Errors are the single enum [`libcpg::Error`](../api/graph-reference.md#error--result) (there is no `CpgError`).

To attach child pointers the way the parser does (so ancestor-based analyses work), also set `node.parent` and push onto the parent's `children` list — the [builder overview](../components/builder/overview.md) shows this in full. For a bare counts-and-kinds demonstration, the AST edge above is enough.

---

## Step 4b — Build your first CPG (parsed, `lang-rust`)

With the `lang-rust` feature enabled, the `TreeSitterCpgBuilder` parses source for you and runs the full construction pipeline — AST, then [CFG](../GLOSSARY.md#control-flow-graph-cfg), then [DFG](../GLOSSARY.md#data-flow-graph-dfg). The `build` method lives on the `CpgBuilder` trait, so that trait must be in scope.

```rust
// requires: features = ["lang-rust"]
use libcpg::{CpgBuilder, Language, TreeSitterCpgBuilder};

fn main() -> Result<(), libcpg::Error> {
    let builder = TreeSitterCpgBuilder::new();
    let source = "fn main() { let x = 1; let y = x; }";
    let cpg = builder.build(source, Language::Rust)?;

    // A real parse yields many nodes: root, function, block, lets, idents…
    assert!(cpg.node_count() > 1);
    assert_eq!(cpg.language(), Language::Rust);
    Ok(())
}
```

If you run this **without** the `lang-rust` feature, `build` returns `Err(Error::UnsupportedLanguage("Rust"))` because the [`ParserRegistry`](../architecture/language-frontends.md) has no grammar to hand the parser. That is the `default = []` rule from Step 2 in action. When you genuinely cannot enable a `lang-*` feature but *can* parse elsewhere, use `build_from_tree` — see [Building CPGs](01-building-cpgs.md#mode-b-build_from_tree).

---

## Step 4c — Inspect counts and statistics

Every CPG answers the basic structural questions directly:

```rust
// Feature-free: works on any CodePropertyGraph, however it was built.
use libcpg::CodePropertyGraph;

fn describe(cpg: &CodePropertyGraph) {
    println!("language:  {}", cpg.language());
    println!("nodes:     {}", cpg.node_count());
    println!("edges:     {}", cpg.edge_count());

    // `stats()` bundles the per-overlay edge breakdown and headline metrics
    // into one `CpgStats` value (all fields public).
    let stats = cpg.stats();
    println!("ast edges: {}", stats.ast_edges);
    println!("cfg edges: {}", stats.cfg_edges);
    println!("dfg edges: {}", stats.dfg_edges);
    println!("functions: {}", stats.function_count);
    println!("cyclomatic complexity: {}", stats.cyclomatic_complexity);
}
```

`CpgStats` is a plain struct with public fields — `node_count`, `edge_count`, `ast_edges`, `cfg_edges`, `dfg_edges`, `call_edges`, `function_count`, `class_count`, and `cyclomatic_complexity`. The complexity figure is McCabe's [cyclomatic complexity](../GLOSSARY.md#cyclomatic-complexity), $`M = E - N + 2`$ over the CFG — see [Querying and Traversal](02-querying-and-traversal.md#metrics) and the [complexity theory page](../theory/02-control-flow-and-complexity.md), which derive and cite it.

---

## Where to go next

You now have a CPG and can read its shape. The rest of the usage guide builds on this foundation:

- **[Building CPGs](01-building-cpgs.md)** — `build` vs `build_from_tree`, `CpgBuilderConfig`, `build_file`, and checking real grammar availability with `ParserRegistry::supports`.
- **[Querying and Traversal](02-querying-and-traversal.md)** — walking AST/CFG/DFG/call overlays, `nodes_by_kind`, a [taint](../GLOSSARY.md#taint-analysis)-style data-flow walk, and subgraph extraction.
- **[Pattern Detection](03-pattern-detection.md)** — the always-on VF2 matcher and the feature-gated GoF detector.
- **[Program Slicing](04-program-slicing.md)** — build a PDG and take backward/forward slices.
- **[Serialization](05-serialization.md)** — round-trip a CPG through `serde_json`.
- **[F1R3FLY.io: Rholang & MeTTa](06-f1r3fly-rholang-metta.md)** — Mode-B integration for process-calculus and S-expression code.

For the conceptual foundations, start at the [theory overview](../theory/00-overview.md); for the corrected API surface, see the [graph reference](../api/graph-reference.md) and [builder reference](../api/builder-reference.md).

---

## References

1. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and Discovering Vulnerabilities with Code Property Graphs.* 2014 IEEE Symposium on Security and Privacy. DOI: [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44)
</content>
</invoke>
