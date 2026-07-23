# Builder API Reference

This page is the authoritative reference for **constructing and enriching**
`libcpg` graphs: the [`CpgBuilder`](#cpgbuilder-trait) trait and its
tree-sitter implementation, the CFG and DFG extractors, the on-demand
[PDG](../GLOSSARY.md#program-dependence-graph-pdg) builder and program slicers,
and the parser registry and node mapper that back the tree-sitter frontend.
Every signature is transcribed from `src/builder/`.

For the *shape* of the graphs these APIs produce, see the
[Graph reference](graph-reference.md); for *analysis* over them, the
[Pattern reference](pattern-reference.md). Term definitions live in the
[Glossary](../GLOSSARY.md).

![The CPG construction pipeline: parse, AST, CFG, DFG](../diagrams/construction-pipeline.svg)

*Figure — the construction pipeline: tree-sitter parse → AST conversion → config-gated CFG extraction → config-gated DFG extraction. Source: [`diagrams/construction-pipeline.puml`](../diagrams/construction-pipeline.puml).*

---

## Where the builder types live

Re-exported from the crate root:

```rust
use libcpg::{
    CpgBuilder, CpgBuilderConfig, TreeSitterCpgBuilder,
    CfgExtractor, CfgExtractorConfig, BasicBlockIdentifier,
    DfgExtractor, DfgExtractorConfig,
    Definition, DefinitionKind, Use, UseKind, DefUseChain, build_def_use_chains,
    PdgBuilder, backward_slice, forward_slice,
};
```

Two builder types are **not** re-exported at the root — reach them by full path:

```rust
use libcpg::builder::{NodeMapper, ParserRegistry};
```

---

## `CpgBuilder` trait

The construction contract. `build` is the language-parsing entry point;
`build_file` and `supports_language` are provided.

```rust
pub trait CpgBuilder: Send + Sync {
    fn build(&self, source: &str, language: Language) -> Result<CodePropertyGraph>;

    fn build_file(&self, path: &std::path::Path) -> Result<CodePropertyGraph> { /* provided */ }

    fn supported_languages(&self) -> &[Language];

    fn supports_language(&self, language: Language) -> bool { /* provided */ }
}
```

| Method | Signature | Notes |
|--------|-----------|-------|
| `build` | `fn build(&self, source: &str, language: Language) -> Result<CodePropertyGraph>` | Parse `source` and construct the CPG. |
| `build_file` | `fn build_file(&self, path: &Path) -> Result<CodePropertyGraph>` | Reads the file, infers the language via [`Language::from_extension`](graph-reference.md#language), calls `build`, and records the source path. |
| `supported_languages` | `fn supported_languages(&self) -> &[Language]` | The languages this builder *could* handle. |
| `supports_language` | `fn supports_language(&self, language: Language) -> bool` | `supported_languages().contains(&language)`. |

> **Honesty — `default = []` means `build` fails for every language.** Parsing
> requires a `lang-*` grammar feature; with no features enabled, `build` (and
> `build_file`) return `Error::UnsupportedLanguage` because
> [`ParserRegistry`](#parserregistry) has no grammar registered. Only
> [`build_from_tree`](#treesittercpgbuilder) (Mode B) and the feature-free
> hand-built surface work with `default = []`.

The `Result` type is `libcpg::Result` (alias for
`std::result::Result<T, libcpg::Error>`); errors are
[`libcpg::Error`](graph-reference.md#error--result) — there is no `CpgError`.

---

## `CpgBuilderConfig`

Controls which overlays are built and how the input is bounded.

```rust
pub struct CpgBuilderConfig {
    pub retain_source: bool,
    pub build_cfg: bool,
    pub build_dfg: bool,
    pub include_comments: bool,
    pub max_file_size: usize,
    pub resolve_imports: bool,
}
```

| Field | Default | Meaning |
|-------|---------|---------|
| `retain_source` | `false` | Keep the source string in the graph (`source_code()`). |
| `build_cfg` | `true` | Run the [`CfgExtractor`](#cfgextractor--cfgextractorconfig) after AST construction. |
| `build_dfg` | `true` | Run the [`DfgExtractor`](#dfgextractor--dfgextractorconfig) after CFG. |
| `include_comments` | `false` | Keep comment nodes in the AST. |
| `max_file_size` | `10 * 1024 * 1024` (10 MiB) | Reject larger inputs in `build` (see the [security guidance](#security--max_file_size-applies-only-to-build)). |
| `resolve_imports` | `false` | Reserved for cross-file reference resolution. |

Builder methods (each returns `Self`): `new()`, `with_source(bool)`,
`with_cfg(bool)`, `with_dfg(bool)`, `with_comments(bool)`,
`with_max_file_size(usize)`, `with_import_resolution(bool)`. `Default` yields the
table above.

```rust
// requires: no features (config construction is feature-free)
use libcpg::CpgBuilderConfig;

let config = CpgBuilderConfig::new()
    .with_source(true)      // retain the source string
    .with_cfg(true)
    .with_dfg(false)        // AST + CFG only
    .with_comments(true);
```

---

## `TreeSitterCpgBuilder`

The tree-sitter-backed `CpgBuilder`. Wraps a `CpgBuilderConfig`.

```rust
pub struct TreeSitterCpgBuilder { /* private: config: CpgBuilderConfig */ }
```

| Method | Signature | Notes |
|--------|-----------|-------|
| `new` | `fn new() -> Self` | Default configuration. |
| `with_config` | `fn with_config(config: CpgBuilderConfig) -> Self` | Custom configuration. |
| `config` | `fn config(&self) -> &CpgBuilderConfig` | Borrow the configuration. |
| `set_config` | `fn set_config(&mut self, config: CpgBuilderConfig)` | Replace the configuration. |
| `build` | *(via `CpgBuilder`)* `fn build(&self, source: &str, language: Language) -> Result<CodePropertyGraph>` | **Mode A**: internal parse. |
| `build_from_tree` | `fn build_from_tree(&self, tree: &tree_sitter::Tree, source: &str, language: Language) -> Result<CodePropertyGraph>` | **Mode B**: caller-supplied tree. |
| `build_file` | *(via `CpgBuilder`)* `fn build_file(&self, path: &Path) -> Result<CodePropertyGraph>` | Infers language from the extension. |

### Mode A — `build` (internal parse)

`build` checks `source.len()` against `config.max_file_size`, fetches the grammar
from [`ParserRegistry::global()`](#parserregistry) (erroring with
`Error::UnsupportedLanguage` if the `lang-*` feature is off), parses with
tree-sitter, and then delegates the entire post-parse pipeline to
`build_from_tree` so both paths stay identical.

```rust
// requires: features = ["lang-rust"]
use libcpg::{TreeSitterCpgBuilder, CpgBuilder, Language};

let builder = TreeSitterCpgBuilder::new();
let cpg = builder.build("fn main() { let x = 1; let y = x; }", Language::Rust)?;
assert!(cpg.node_count() > 1);
# Ok::<(), libcpg::Error>(())
```

### Mode B — `build_from_tree` (caller-supplied tree)

![Mode B: the caller parses with its own grammar and hands the tree to build_from_tree](../diagrams/mode-b-sequence.svg)

*Figure — the Mode-B sequence: a caller (e.g. pgmcp) that already links a grammar parses the source itself and hands the `tree_sitter::Tree` to `build_from_tree`, which runs the identical AST → CFG → DFG pipeline without a `lang-*` feature. Source: [`diagrams/mode-b-sequence.puml`](../diagrams/mode-b-sequence.puml).*

[Mode B](../GLOSSARY.md#mode-b--build_from_tree) is for a caller that has *already*
parsed the source with a tree-sitter grammar it owns. It:

- needs **no** `lang-*` feature (the caller supplies the grammar);
- **skips** the `max_file_size` check (the caller already owns the tree);
- selects the [`NodeMapper`](#nodemapper) from `language`; and
- is the **only** path for [Rholang](../GLOSSARY.md#rholang) and
  [MeTTa](../GLOSSARY.md#metta), whose `rholang`/`metta` features are pure `cfg`
  toggles that gate node-mapper arms and vendor no grammar.

```rust
// requires: no libcpg lang feature; the caller owns the grammar crate
use libcpg::{TreeSitterCpgBuilder, Language};

// `tree` came from the caller's own tree-sitter parser for this language.
let builder = TreeSitterCpgBuilder::new();
let cpg = builder.build_from_tree(&tree, source, Language::Rholang)?;
# Ok::<(), libcpg::Error>(())
```

**Equivalence.** The inline test `test_build_from_tree_matches_build`
(`src/builder/tree_sitter.rs`) parses one Rust snippet both ways and asserts the
two graphs share `node_count()`, `edge_count()`, and `language()` — the
caller-supplied-tree path produces a graph identical in shape to the internal
parse.

### AST conversion

Both modes convert tree-sitter nodes recursively. For each retained node the
builder: maps its kind via [`NodeMapper::map_kind`](#nodemapper); records a
`SourceRange` from byte offsets; adds the node; wires an `AstChild` edge from the
parent **and** sets the child's `parent` pointer (ancestor-based analyses read
the pointer, not the edge); and marks every `Function` node as a CFG entry.
Comments and pure punctuation are filtered per
[`NodeMapper::should_include_node`](#nodemapper).

### Security — `max_file_size` applies only to `build`

`build` rejects `source.len() > config.max_file_size` (default 10 MiB) with
`Error::Construction`. `build_from_tree` performs **no** such check — a Mode-B
caller feeding untrusted input must bound the source (and parse time) itself.

---

## `ParserRegistry`

Lazily maps [`Language`](graph-reference.md#language) values to tree-sitter
grammars, one per enabled `lang-*` feature. Full path:
`libcpg::builder::ParserRegistry`.

| Method | Signature | Notes |
|--------|-----------|-------|
| `global` | `fn global() -> &'static ParserRegistry` | Process-wide singleton (`OnceLock`), initialized on first use. |
| `get` | `fn get(&self, lang: Language) -> Option<tree_sitter::Language>` | Grammar for `lang`, or `None` if its feature is off. |
| `supports` | `fn supports(&self, lang: Language) -> bool` | Whether a grammar is registered. |
| `supported_languages` | `fn supported_languages(&self) -> impl Iterator<Item = Language> + '_` | The **actually** registered languages. |
| `language_count` | `fn language_count(&self) -> usize` | How many grammars are registered. |

> **`supported_languages()` on the builder is a static list of 16 candidate
> languages regardless of features.** The *real* availability is
> `ParserRegistry::global().supports(lang)` (or `.get(lang).is_some()`), which
> reflects the `lang-*` features you actually compiled. Query the registry, not
> the builder's static list, when you need ground truth.

The 16 grammars gated by `lang-*` features: Rust, Python, JavaScript, TypeScript,
Go, Java, C, C++, JSON, HTML, CSS, Bash, TOML, YAML, Markdown, Ruby.

---

## `NodeMapper`

Translates a tree-sitter node-kind string into a
[`CpgNodeKind`](graph-reference.md#cpgnodekind), per language. Full path:
`libcpg::builder::NodeMapper`.

| Method | Signature | Notes |
|--------|-----------|-------|
| `new` | `fn new(language: Language) -> Self` | Mapper for one language. |
| `language` | `fn language(&self) -> Language` | The configured language. |
| `map_kind` | `fn map_kind(&self, ts_kind: &str, node: &tree_sitter::Node, source: &str) -> CpgNodeKind` | Dispatch on `language` to the per-language mapper. |
| `should_include` | `fn should_include(&self, ts_kind: &str, include_comments: bool) -> bool` | String-keyed inclusion test (drops punctuation, comments, transparent wrappers). |
| `should_include_node` | `fn should_include_node(&self, node: &tree_sitter::Node, include_comments: bool) -> bool` | Node-aware test used by the builder walk; additionally drops anonymous Rholang keyword/operator tokens via `is_named()`. |

`map_kind` dispatches to `map_rust`, `map_python`, `map_javascript` (also
TypeScript), `map_go`, `map_java`, `map_c_cpp`, `map_ruby`, `map_json`,
`map_html`, `map_css`, `map_bash`, `map_config` (YAML/TOML), `map_markdown`, and
— under the `rholang`/`metta` `cfg` toggles — **`map_rholang`** and
**`map_metta`** (both Mode-B only). Anything else falls through to `map_generic`
(a `CpgNodeKind::Unknown`). The Rholang/MeTTa mappings are detailed in the
[node-mapper component guide](../components/builder/node-mapper.md).

---

## `CfgExtractor` & `CfgExtractorConfig`

Adds [control-flow](../GLOSSARY.md#control-flow-graph-cfg) edges to a CPG that
already has AST edges. The extractor borrows `&self` and mutates the CPG through
`&mut`; `extract` returns `()`.

```rust
pub struct CfgExtractorConfig {
    pub include_fallthrough: bool,   // default true
    pub include_exceptions: bool,    // default true
    pub include_call_edges: bool,    // default true
}
```

| Method | Signature | Notes |
|--------|-----------|-------|
| `new` | `fn new() -> Self` | Default config. |
| `with_config` | `fn with_config(config: CfgExtractorConfig) -> Self` | Custom config. |
| `extract` | `fn extract(&self, cpg: &mut CodePropertyGraph)` | Build CFG for every `Function` node. |
| `extract_function_cfg` | `fn extract_function_cfg(&self, cpg: &mut CodePropertyGraph, function: NodeId)` | Build CFG for one function. |

`extract_function_cfg` marks the function as a CFG entry, connects it to its body
(the **last** AST child), and recursively processes control constructs — `If`,
`While`, `For`, `Loop`, `Match`, `Return`, `Break`, `Continue`, `Try`, `Throw`,
and (when `include_call_edges`) `Call` — emitting the 14 [`CfgEdgeKind`](graph-reference.md#cfgedgekind)
variants; a loop/`try` context stack routes `break`/`continue`/`throw` edges. It
is idempotent.

```rust
// requires: no features (feature-free over a hand-built or Mode-B graph)
use libcpg::CfgExtractor;

CfgExtractor::new().extract(&mut cpg);
```

### `BasicBlockIdentifier`

Groups CFG nodes into [basic blocks](../GLOSSARY.md#basic-block).

| Method | Signature | Notes |
|--------|-----------|-------|
| `new` | `fn new() -> Self` | Constructor (unit struct). |
| `identify` | `fn identify(&self, cpg: &CodePropertyGraph, function: NodeId) -> FxHashMap<NodeId, Vec<NodeId>>` | Maps each block **leader** to the node ids in its block. |

Leaders are the function itself, any branch/loop target, and the successors of a
branching instruction; a block extends along single `Sequential` successors that
are not themselves leaders. Once the CFG exists,
[`cyclomatic_complexity()`](graph-reference.md#graph-metrics) reads it directly.

---

## `DfgExtractor` & `DfgExtractorConfig`

Adds [data-flow](../GLOSSARY.md#data-flow-graph-dfg) edges via an
[AST-ordered reaching-definitions](../GLOSSARY.md#ast-ordered-reaching-definitions)
sweep. Like the CFG extractor, `extract` takes `&self` + `&mut cpg` and returns
`()`.

```rust
pub struct DfgExtractorConfig {
    pub include_field_access: bool,     // default true
    pub include_parameters: bool,       // default true
    pub include_return_values: bool,    // default true
    pub track_aliases: bool,            // default false
    pub max_iterations: usize,          // default 100
}
```

| Method | Signature | Notes |
|--------|-----------|-------|
| `new` | `fn new() -> Self` | Default config. |
| `with_config` | `fn with_config(config: DfgExtractorConfig) -> Self` | Custom config. |
| `extract` | `fn extract(&self, cpg: &mut CodePropertyGraph)` | Build DFG for every `Function`. |
| `extract_function_dfg` | `fn extract_function_dfg(&self, cpg: &mut CodePropertyGraph, function: NodeId)` | Build DFG for one function. |

The sweep abstract-interprets the function body in source order, threading each
statement's reaching definitions down into **nested** identifier uses (e.g. `buf`
in `decode(buf)`). A `let`/`Variable`, `Parameter`, or plain `Assignment`
generates a definition; in straight-line context it performs a
[strong update](../GLOSSARY.md#strong-update--weak-update) (latest write wins),
and inside a conditional region a weak update; loop bodies are swept twice for
loop-carried dependence. Each currently-reaching definition is linked to a use
with a `DataFlow(DefUse)` edge, and the pass is idempotent. It also emits
`Parameter`, `ReturnValue`, and field/index access edges per config. (Two earlier
CFG-fixpoint-based approaches are retained but compiled out under `#[cfg(any())]`;
this is **not** [SSA](../GLOSSARY.md#static-single-assignment-ssa).)

### Def / Use / chain types

The following are re-exported at the crate root:

```rust
pub struct Definition { pub variable: Arc<str>, pub node: NodeId, pub kind: DefinitionKind }
pub enum   DefinitionKind { Declaration, Assignment, Parameter, FieldWrite, IndexWrite }
pub struct Use { pub variable: Arc<str>, pub node: NodeId, pub kind: UseKind }
pub enum   UseKind { Read, FieldRead, IndexRead, Argument }
```

`DefUseChain` collects, per variable, its definitions, uses, and both directions
of the mapping:

```rust
pub struct DefUseChain {
    pub variable: Arc<str>,
    pub definitions: Vec<NodeId>,
    pub uses: Vec<NodeId>,
    pub def_to_uses: FxHashMap<NodeId, Vec<NodeId>>,
    pub use_to_defs: FxHashMap<NodeId, Vec<NodeId>>,
}
```

| `DefUseChain` method | Signature |
|----------------------|-----------|
| `new` | `fn new(variable: Arc<str>) -> Self` |
| `add_definition` | `fn add_definition(&mut self, def: NodeId)` |
| `add_use` | `fn add_use(&mut self, use_site: NodeId)` |
| `link` | `fn link(&mut self, def: NodeId, use_site: NodeId)` |
| `uses_of` | `fn uses_of(&self, def: NodeId) -> &[NodeId]` |
| `definitions_of` | `fn definitions_of(&self, use_site: NodeId) -> &[NodeId]` |

The free function `build_def_use_chains` builds these from the DFG edges already
in a graph:

```rust
pub fn build_def_use_chains(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> FxHashMap<Arc<str>, DefUseChain>;
```

---

## `PdgBuilder`, `backward_slice`, `forward_slice`

The [PDG](../GLOSSARY.md#program-dependence-graph-pdg) is built **on demand** —
never during initial construction — and is the substrate for
[program slicing](../GLOSSARY.md#program-slicing).

### `PdgBuilder`

```rust
pub struct PdgBuilder { /* private */ }
```

| Method | Signature | Notes |
|--------|-----------|-------|
| `new` | `fn new() -> Self` | Constructor. |
| `build` | `fn build(&self, cpg: &mut CodePropertyGraph, function: NodeId)` | Add `ControlDependence` + `DataDependence` edges for `function`. Idempotent. |

`build` requires that `function`'s CFG and DFG already exist. It computes
[control dependence](../GLOSSARY.md#control-dependence) [[1]](#references) as the
**reverse dominance frontier**: it forms the intraprocedural CFG, adds a virtual
`EXIT` node (id `u32::MAX`) linked from every real exit, computes post-dominators
with `petgraph::algo::dominators::simple_fast` on the reversed CFG, and walks the
Cytron et al. [[2]](#references) frontier. [Data dependence](../GLOSSARY.md#data-dependence)
edges re-project the DFG's `DefUse`/`ReachingDef` edges within the function.

### Slicing

Both slicers are free functions at the crate root, feature-free, and return an
`FxHashSet<NodeId>` (rustc-hash's set) — **not** a `Vec`:

```rust
pub fn backward_slice(cpg: &CodePropertyGraph, criterion: NodeId, max_nodes: usize) -> FxHashSet<NodeId>;
pub fn forward_slice (cpg: &CodePropertyGraph, criterion: NodeId, max_nodes: usize) -> FxHashSet<NodeId>;
```

| Function | Meaning |
|----------|---------|
| `backward_slice` | Nodes that (transitively) affect `criterion` — reverse-BFS over incoming PDG edges. |
| `forward_slice` | Nodes (transitively) affected by `criterion` — forward-BFS over outgoing PDG edges. |

Both include `criterion` itself and stop once the slice reaches `max_nodes`
nodes (a hard bound: pass a small cap for untrusted input). They traverse only
`ControlDependence`/`DataDependence` edges (`kind.is_pdg()`), so call
`PdgBuilder::build` first. Introduced by Weiser [[3]](#references).

```rust
// requires: no features (feature-free)
use libcpg::{CfgExtractor, DfgExtractor, PdgBuilder, backward_slice};

CfgExtractor::new().extract(&mut cpg);
DfgExtractor::new().extract(&mut cpg);
PdgBuilder::new().build(&mut cpg, function);

let slice = backward_slice(&cpg, use_site, 256);
assert!(slice.contains(&use_site)); // the criterion is always included
```

---

## See also

- [Graph reference](graph-reference.md) — the node/edge model these APIs build.
- [Pattern reference](pattern-reference.md) — analyses that consume the built
  graph.
- [Architecture: data flow](../architecture/data-flow.md) — the construction and
  analysis pipelines end to end.
- Component guides: [builder overview](../components/builder/overview.md),
  [CFG](../components/builder/cfg.md), [DFG](../components/builder/dfg.md),
  [PDG & slicing](../components/builder/pdg-and-slicing.md),
  [node mapper](../components/builder/node-mapper.md).
- [Glossary](../GLOSSARY.md) — reaching definitions, control/data dependence,
  slicing, Mode B, and more.

---

## References

1. Ferrante, J., Ottenstein, K. J., Warren, J. D. (1987). *The Program Dependence Graph and Its Use in Optimization.* ACM TOPLAS 9(3). DOI: [10.1145/24039.24041](https://doi.org/10.1145/24039.24041)
2. Cytron, R., Ferrante, J., Rosen, B. K., Wegman, M. N., Zadeck, F. K. (1991). *Efficiently Computing Static Single Assignment Form and the Control Dependence Graph.* ACM TOPLAS 13(4). DOI: [10.1145/115372.115320](https://doi.org/10.1145/115372.115320)
3. Weiser, M. (1984). *Program Slicing.* IEEE Transactions on Software Engineering SE-10(4). DOI: [10.1109/TSE.1984.5010248](https://doi.org/10.1109/TSE.1984.5010248) (originally ICSE '81).
