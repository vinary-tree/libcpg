# Builder — Overview

The `builder` module is the factory floor of libcpg: it turns **source text** into a
fully-populated [Code Property Graph](../../GLOSSARY.md#code-property-graph-cpg) (CPG).
Everything else in the library — traversal, slicing, pattern detection, the GNN — reads
the graph this module produces. This page maps the construction pipeline end to end,
documents the [`CpgBuilder`](../../GLOSSARY.md#tree-sitter) trait and its tree-sitter
implementation, and explains the two ways a CPG is born: the internal-parse path
(`build`) and the caller-supplied-tree path ([Mode B](../../GLOSSARY.md#mode-b--build_from_tree),
`build_from_tree`).

The related component pages drill into each construction stage:

| Stage | Page | What it adds |
|-------|------|--------------|
| Control flow | [`cfg.md`](cfg.md) | `ControlFlow(CfgEdgeKind)` edges |
| Call resolution | [`../graph/call-resolution.md`](../graph/call-resolution.md) | scoped symbols plus uniquely supported reference/call edges |
| Data flow | [`dfg.md`](dfg.md) | `DataFlow(DfgEdgeKind)` edges (AST-ordered reaching defs) |
| Program dependence & slicing | [`pdg-and-slicing.md`](pdg-and-slicing.md) | `ControlDependence` / `DataDependence` edges, backward/forward slices |
| Language front-ends | [`node-mapper.md`](node-mapper.md) | tree-sitter node kinds → `CpgNodeKind` |

---

## Why a builder at all?

A CPG unifies the [AST](../../GLOSSARY.md#abstract-syntax-tree-ast), the
[CFG](../../GLOSSARY.md#control-flow-graph-cfg), and the
[DFG](../../GLOSSARY.md#data-flow-graph-dfg) onto **one shared node set** so that a single
query can span syntax, control, and data flow simultaneously — the design that made CPGs
effective for vulnerability discovery (Yamaguchi et al. [[1]](#references)). Achieving that
unification means the three overlays must be laid over *identical* nodes. The builder is
what guarantees this: it constructs the AST once, then hands the same graph to each
overlay extractor in turn, so a `ControlFlow` edge and a `DataFlow` edge that touch the
"same statement" really do share a [`NodeId`](../../GLOSSARY.md#node-kind--edge-kind).

The builder also isolates the one language-specific concern — the shape of each grammar's
parse tree — behind the [`NodeMapper`](node-mapper.md), so that the CFG and DFG extractors
can be written *once*, against the language-agnostic `CpgNodeKind` vocabulary, and work for
every supported language.

---

## The construction pipeline

![Construction pipeline: source text becomes AST nodes, then CFG, optional evidence-bounded call resolution, and DFG stages enrich the same graph.](../../diagrams/construction-pipeline.svg)

*Figure — the five construction phases: parse, AST construction, CFG
extraction, optional call resolution, and DFG extraction. Source:
[`diagrams/construction-pipeline.puml`](../../diagrams/construction-pipeline.puml).*

Construction runs in ordered phases; each later phase reads the graph the earlier phases
built:

1. **Parse.** Tree-sitter turns source text into a concrete syntax tree. In the internal
   path this is done by libcpg; in [Mode B](../../GLOSSARY.md#mode-b--build_from_tree) the
   caller has already produced the `tree_sitter::Tree`.
2. **AST construction.** `build_ast` walks the tree-sitter tree with an explicit
   preorder work stack, maps each node's kind
   string to a `CpgNodeKind` via the [`NodeMapper`](node-mapper.md), creates a `CpgNode`,
   and wires an `AstChild` edge (plus a parent pointer) from parent to child. Function
   nodes are registered as CFG entry points here.
3. **CFG extraction** (config-gated by `build_cfg`). [`CfgExtractor`](cfg.md) overlays
   `ControlFlow` edges onto the AST nodes.
4. **Call resolution** (config-gated by `resolve_imports`).
   [`resolve_calls`](../graph/call-resolution.md) builds scoped symbols and
   materializes only uniquely supported call targets. It deliberately runs
   before the DFG so parameter/return edges can consume resolved callers.
5. **DFG extraction** (config-gated by `build_dfg`). [`DfgExtractor`](dfg.md) overlays
   `DataFlow` edges using an [AST-ordered reaching-definitions](../../GLOSSARY.md#ast-ordered-reaching-definitions)
   sweep.

The [PDG](pdg-and-slicing.md) is deliberately **not** part of this pipeline — it is built
on demand, per function, after construction (see
[`pdg-and-slicing.md`](pdg-and-slicing.md)).

---

## The `CpgBuilder` trait

Every builder implements one small trait. It is `Send + Sync`, so a builder can be shared
across threads (e.g. to build many files in parallel with rayon).

```rust
use libcpg::{CodePropertyGraph, Language, Result};

pub trait CpgBuilder: Send + Sync {
    /// Parse `source` in `language` and construct a CPG.
    fn build(&self, source: &str, language: Language) -> Result<CodePropertyGraph>;

    /// Build from a file, inferring the language from the extension. Provided.
    fn build_file(&self, path: &std::path::Path) -> Result<CodePropertyGraph> { /* … */ }

    /// The languages this builder can name (a static list; see the honesty note below).
    fn supported_languages(&self) -> &[Language];

    /// Whether `language` is in `supported_languages()`. Provided.
    fn supports_language(&self, language: Language) -> bool { /* … */ }
}
```

`build_file` reads the file, infers the language with
[`Language::from_extension`](../../GLOSSARY.md#tree-sitter) (which returns
`Language::Unknown` for anything it does not recognise, never an `Option`), delegates to
`build`, and records the source path on the resulting graph. Errors surface as the crate's
[`Error`](../../api/graph-reference.md) enum — for example `Error::UnsupportedLanguage`
when no grammar is registered, or `Error::Construction` on a parse failure. (There is no
`CpgError` type.)

> **Honesty — `supported_languages()` is aspirational.** It returns a **static list of 16
> languages** regardless of which grammars were actually compiled in. The *real*
> availability of a grammar is `ParserRegistry::global().supports(language)`, which
> reflects the enabled `lang-*` Cargo features. See
> [`node-mapper.md`](node-mapper.md#parser-registry) and
> [`../../engineering/01-build-and-features.md`](../../engineering/01-build-and-features.md).

---

## `TreeSitterCpgBuilder`

`TreeSitterCpgBuilder` is the one shipped implementation. It holds a
`CpgBuilderConfig` and nothing else, so it is cheap to clone.

```rust
use libcpg::{CpgBuilderConfig, TreeSitterCpgBuilder};

// Default configuration.
let builder = TreeSitterCpgBuilder::new();

// Or a custom configuration.
let config = CpgBuilderConfig::new()
    .with_source(true)   // retain the source text on the graph
    .with_cfg(true)      // run the CFG extractor
    .with_dfg(true)      // run the DFG extractor
    .with_comments(false) // drop comment nodes
    .with_import_resolution(true); // resolve unique calls before the DFG
let builder = TreeSitterCpgBuilder::with_config(config);
```

### Configuration — `CpgBuilderConfig`

| Field | Default | Effect |
|-------|---------|--------|
| `retain_source` | `false` | Keep the source text on the CPG (`cpg.source_code()`). |
| `build_cfg` | `true` | Run [`CfgExtractor`](cfg.md) as phase 3. |
| `build_dfg` | `true` | Run [`DfgExtractor`](dfg.md) as phase 5. |
| `include_comments` | `false` | Emit `Comment` nodes instead of dropping them. |
| `max_file_size` | `10 * 1024 * 1024` (10 MB) | Reject larger sources in `build` (see the security note). |
| `resolve_imports` | `false` | Run evidence-bounded in-file/import call resolution as phase 4. |

The builder API for the config is fluent: `CpgBuilderConfig::new()` then any of
`with_source`, `with_cfg`, `with_dfg`, `with_comments`, `with_max_file_size`,
`with_import_resolution`.

---

## Two ways to build: `build` vs `build_from_tree`

libcpg exposes **two entry points** into the identical post-parse pipeline. They differ
only in *who owns the parse*.

```rust
// Mode A — internal parse. requires: features = ["lang-rust"]
use libcpg::{CpgBuilder, TreeSitterCpgBuilder, Language};

let builder = TreeSitterCpgBuilder::new();
let cpg = builder.build("fn main() { let x = 1; let y = x; }", Language::Rust)?;
assert!(cpg.node_count() > 1);
# Ok::<(), libcpg::Error>(())
```

```rust
// Mode B — caller-supplied tree. Feature-free in libcpg; the CALLER links the grammar.
use libcpg::{TreeSitterCpgBuilder, Language};

// The caller parses with whatever tree-sitter grammar it already links.
let mut parser = tree_sitter::Parser::new();
parser.set_language(&tree_sitter_rust::LANGUAGE.into())
    .expect("set language");
let source = "fn main() { let x = 1; let y = x; }";
let tree = parser.parse(source, None).expect("parse");

// libcpg builds the CPG from the tree — no lang-* feature required.
let builder = TreeSitterCpgBuilder::new();
let cpg = builder.build_from_tree(&tree, source, Language::Rust)?;
assert!(cpg.node_count() > 1);
# Ok::<(), libcpg::Error>(())
```

Internally, `build` is a thin wrapper: it enforces `max_file_size`, looks up the grammar in
the [`ParserRegistry`](node-mapper.md#parser-registry), parses, and then **delegates to
`build_from_tree`** so both paths share one implementation of AST/CFG/DFG construction.
Because they share that code, the two paths are guaranteed to produce structurally
identical graphs — an invariant pinned by the inline test
`test_build_from_tree_matches_build`, which asserts equal `node_count()`, `edge_count()`,
`language()`, and canonical typed edge triples for the same source (see
[`../../scientific/01-cpg-invariants-and-equivalence.md`](../../scientific/01-cpg-invariants-and-equivalence.md)).

### When to use which

| | `build(source, language)` (Mode A) | `build_from_tree(&tree, source, language)` (Mode B) |
|---|---|---|
| Who parses | libcpg, via `ParserRegistry` | The **caller**, with its own grammar |
| Cargo feature | needs the matching `lang-*` feature | **feature-free** in libcpg |
| `max_file_size` check | enforced | **skipped** (caller already owns the tree) |
| Rholang / MeTTa | not available (no registered grammar) | **the only path** — see [`node-mapper.md`](node-mapper.md) |
| Typical caller | a standalone tool building one language | a host that already parses (e.g. pgmcp), or a polyglot integration |

> **Honesty — `default = []`.** With no features enabled, **no grammar is registered**, so
> `build(source, language)` fails with `Error::UnsupportedLanguage` for *every* language.
> Only `build_from_tree` (Mode B) and hand-built graphs work feature-free. Enable a
> `lang-*` feature (e.g. `lang-rust`) to make `build` usable for that language. This is why
> every `build` snippet in these docs opens with a `// requires: features = ["lang-…"]`
> comment.

> **Security — the size guard is Mode-A only.** `build` rejects sources larger than
> `max_file_size` (default 10 MB) to bound work on hostile input; `build_from_tree` does
> **not**, because the caller already produced the tree and is expected to have bounded it.
> A host feeding untrusted code through Mode B must impose its own limits (parser timeout,
> input size). See [`../../security/01-input-and-resource-hardening.md`](../../security/01-input-and-resource-hardening.md).

---

## Inside AST construction: `build_ast`

Phase 2 is a stack-safe preorder walk of the tree-sitter tree. Its explicit
`Vec<(tree_sitter::Node, Option<NodeId>)>` work stack holds source-shaped
pending work on the heap rather than in native Rust frames. Understanding it
explains three load-bearing invariants the rest of the library depends on: the
`AstChild` edge, the parent pointer, and the function → CFG-entry registration.

The walk is driven by `build_ast(tree, source, mapper, cpg)`, expressed here as
literate pseudocode:

```text
work ← [(tree.root_node(), None)]
while work is not empty:
    (ts_node, parent_id) ← pop(work)
    kind_str ← ts_node.kind()                          # the grammar's rule/token name

    # 1. Inclusion filter (node-aware; see node-mapper.md).
    if not mapper.should_include_node(ts_node, include_comments):
        child_parent ← parent_id                       # transparent reparenting
    else:
        # 2. Map the grammar kind string to the language-agnostic CpgNodeKind.
        cpg_kind ← mapper.map_kind(kind_str, ts_node, source)

        # 3. Create the CPG node, then canonicalize its flat string fields
        #    through this build's temporary pool before graph insertion.
        node ← CpgNode(cpg_kind, SourceRange(ts_node))
        string_pool.intern_node(node)
        node_id ← cpg.add_node(node)

        # 4. Wire the AST edge. connect maintains both navigation indices.
        if parent_id is Some(parent):
            cpg.connect(parent, node_id, AstChild)

        # 5. A function is a CFG entry point.
        if cpg_kind is Function { .. }:
            cpg.add_cfg_entry(node_id)
        child_parent ← Some(node_id)

    # 6. Append children, then reverse only that suffix. LIFO popping now
    #    retains exact source order without native recursive descent.
    first_child ← len(work)
    for child in ts_node.children():
        work.push((child, child_parent))
    reverse(work[first_child..])
```

Four details are worth emphasising:

- **Build-local string ownership.** Names, types, operators, literals, imports,
  optional node text, and allocation types remain public `Arc<str>` values, but
  equal contents within one build share one allocation. The temporary
  `FxHashSet` is dropped before CFG construction; it is neither a global
  interner nor part of the returned graph. Separate builds, manual graphs,
  merges, and deserialized graphs need not share pointers, so consumers compare
  string contents rather than `Arc` addresses. See
  [ADR-0060](../../design/0060-build-local-string-canonicalization.md) and
  [validation 55](../../scientific/55-string-storage-validation.md).

- **One invariant-owning insertion.** `cpg.connect(parent, child, AstChild)`
  appends `child` to the parent's ascending-edge-id child index and maintains
  the child's parent pointer while it inserts the edge. `cpg.ast_children(id)`
  borrows the first index; `cpg.ast_parent(id)` / `ast_ancestors(id)` read the
  second. The builder makes one call, so the DFG's binder detection, the PDG's
  enclosing-function test, and the slicer can all ask "who is my ancestor?"
  cheaply without duplicated bookkeeping.
- **Reparenting of skipped nodes.** When `should_include_node` rejects a node (punctuation,
  a comment, or a transparent grammar wrapper), its frames retain the *current*
  parent. The wrapper disappears and its meaningful contents
  become direct children of the enclosing construct — which is exactly what the CFG's
  child-ordering assumptions (`[condition, then, else]`) and the MeTTa/Rholang mappers
  depend on. See [`node-mapper.md`](node-mapper.md).
- **Function entries seed the CFG.** Registering every `Function` node as a CFG entry here
  means the CFG and DFG extractors can find the functions to process by walking
  `cpg.functions()`.

After the walk, phases 3 through 5 run when enabled. Resolution is positioned
between CFG and DFG so the DFG's interprocedural parameter and return helpers
observe established callers without changing the default opt-out shape.

When `CpgBuilderConfig::parallel_analysis` is `true`, phases 3 and 5 compute
independent per-function plans on the current Rayon pool. Each phase still emits
plans serially in function order, and resolution remains between them. The
default is serial; see [parallel analysis](../../usage/29-parallel-analysis.md)
for pool configuration, exactness, and resource guidance.

---

## What the builder does *not* do

- It does **not** build the [PDG](pdg-and-slicing.md); control- and data-dependence edges
  are added later, per function, by `PdgBuilder::build`.
- It does **not** run pattern, algorithm, or GNN analyses; those consume the finished CPG.
- It does **not** load a package index or merge another file's declarations.
  The `resolve_imports` flag resolves only evidence represented by the current
  graph; absent imported declarations are reported as `External` without
  creating topology.

---

## See also

- [`cfg.md`](cfg.md) — control-flow extraction and cyclomatic complexity.
- [`dfg.md`](dfg.md) — AST-ordered reaching definitions and def-use chains.
- [`../graph/call-resolution.md`](../graph/call-resolution.md) — evidence
  tiers, scope symbols, topology boundary, and resource caps.
- [`pdg-and-slicing.md`](pdg-and-slicing.md) — program dependence and slicing.
- [`node-mapper.md`](node-mapper.md) — per-language node mapping, incl. Rholang/MeTTa.
- [`../../api/builder-reference.md`](../../api/builder-reference.md) — exhaustive method
  signatures for the builder surface.
- [`../../architecture/language-frontends.md`](../../architecture/language-frontends.md) —
  where the builder sits in the overall architecture.
- [`../../design/0002-mode-b-build-from-tree.md`](../../design/0002-mode-b-build-from-tree.md)
  — the design rationale for Mode B.
- [`../../design/0060-build-local-string-canonicalization.md`](../../design/0060-build-local-string-canonicalization.md)
  — why mapper-produced strings share only within one build.

---

## References

1. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and Discovering Vulnerabilities with Code Property Graphs.* 2014 IEEE Symposium on Security and Privacy. DOI: [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44)
