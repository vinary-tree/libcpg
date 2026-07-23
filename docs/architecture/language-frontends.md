# Language Frontends

A frontend turns source text into the shared CPG vocabulary. libcpg has exactly one parsing technology — [tree-sitter](../GLOSSARY.md#tree-sitter) — and one normalization layer — the per-language `NodeMapper`. This page describes the parser registry, the two build modes (internal-parse *Mode A* and caller-supplied-tree *Mode B*), the `map_kind` dispatch, and the two [F1R3FLY.io](../GLOSSARY.md#rholang) frontends ([Rholang](../GLOSSARY.md#rholang) and [MeTTa](../GLOSSARY.md#metta)) that exist only through Mode B. The mapping is intentionally *sound but possibly incomplete* — it never emits an edge the language semantics forbid, though it may omit one they allow — the standard multi-language-CPG posture of Yamaguchi et al. [[1]](#references).

## Tree-sitter and the parser registry

Tree-sitter produces a concrete syntax tree from source text; libcpg walks that tree to build the AST overlay. Grammars are **feature-gated**: the crate ships 16 (`lang-rust`, `lang-python`, `lang-javascript`, `lang-typescript`, `lang-go`, `lang-java`, `lang-c`, `lang-cpp`, `lang-json`, `lang-html`, `lang-css`, `lang-bash`, `lang-toml`, `lang-yaml`, `lang-markdown`, `lang-ruby`), and each is compiled in only when its `lang-*` feature is enabled.

The `ParserRegistry` is a lazily-initialized global (`OnceLock`) that maps a `Language` to its compiled tree-sitter grammar, holding only the grammars whose features are on:

```rust
use libcpg::builder::ParserRegistry;
use libcpg::Language;

let registry = ParserRegistry::global();
if registry.supports(Language::Rust) {          // true only if `lang-rust` is enabled
    let _ts = registry.get(Language::Rust);      // Option<tree_sitter::Language>
}
println!("{} grammars compiled in", registry.language_count());
```

Two "supported languages" surfaces exist, and they mean different things:

- `TreeSitterCpgBuilder::supported_languages()` returns a **static 16-element list** — the languages libcpg *knows how to map* — regardless of which features are enabled.
- `ParserRegistry::supports(lang)` (and `language_count()`, `supported_languages()`) reflects the grammars **actually compiled in**.

Under `default = []` the registry is empty, so `supported_languages()` still lists 16 but `build` fails for all of them with `Error::UnsupportedLanguage`. Enable the grammars you need (or a group: `lang-systems`, `lang-scripting`, `lang-web`, `lang-config`, `lang-all`).

## Two ways to build: Mode A and Mode B

![Component diagram of the language frontend: parser registry and Mode-B tree feeding the NodeMapper dispatch](../diagrams/language-frontend-pipeline.svg)

*Figure — the frontend: Mode A parses via the feature-gated `ParserRegistry`, Mode B accepts a caller-supplied tree; both feed the same `NodeMapper` dispatch and post-parse pipeline. Source: [`diagrams/language-frontend-pipeline.puml`](../diagrams/language-frontend-pipeline.puml).*

**Mode A — `build(source, language)`** is the internal-parse path. It enforces `config.max_file_size`, looks the grammar up in the registry (erroring if absent), parses, and then *delegates to Mode B* for the post-parse pipeline. It therefore needs the matching `lang-*` feature.

**Mode B — [`build_from_tree`](../GLOSSARY.md#mode-b--build_from_tree)`(&tree, source, language)`** accepts a `tree_sitter::Tree` the caller already parsed. It needs **no** `lang-*` feature (the caller owns the grammar), skips the `max_file_size` check, and runs the identical AST → CFG → DFG pipeline. Because Mode A funnels through Mode B, the two produce structurally identical graphs — a regression test asserts equal `node_count`, `edge_count`, and `language` for the same source.

```rust
// Mode B: the caller has already parsed `source` with a grammar it links itself.
// build_from_tree is feature-free; `language` only selects the NodeMapper.
use libcpg::{TreeSitterCpgBuilder, Language};

fn build(tree: &tree_sitter::Tree, source: &str) -> libcpg::Result<libcpg::CodePropertyGraph> {
    TreeSitterCpgBuilder::new().build_from_tree(tree, source, Language::Rust)
}
```

![Sequence diagram of build_from_tree with a caller-supplied grammar](../diagrams/mode-b-sequence.svg)

*Figure — Mode B: the caller parses with its own grammar and hands the tree to `build_from_tree`, which runs AST construction then the CFG/DFG passes. Source: [`diagrams/mode-b-sequence.puml`](../diagrams/mode-b-sequence.puml).*

`build_file(&Path)` is a convenience over Mode A: it reads the file, infers the language with `Language::from_extension` (which lowercases, strips a leading `.`, and returns `Language::Unknown` — never an `Option` — for unrecognized extensions), and records the path on the resulting graph.

## The `NodeMapper` dispatch

Once a tree exists, AST construction (`convert_node`) asks the `NodeMapper` two questions per tree-sitter node: *should this node be kept?* and *what `CpgNodeKind` is it?*

### Inclusion: `should_include` and `should_include_node`

`should_include(ts_kind, include_comments)` is the string-keyed filter: it drops pure punctuation (`(`, `)`, `{`, `,`, `;`, …), drops comments unless configured, and drops language-specific *transparent wrapper / grouping-container* rules whose names never collide with a semantic rule (for MeTTa, the `expression`/`atom_expression`/`prefixed_expression` wrappers; for Rholang, container/marker rules like `names`, `receipts`, `send_single`). Dropping a wrapper reparents its children so the real structure is visible to `map_kind`.

`should_include_node(node, include_comments)` layers a *node-aware* check on top: for Rholang it additionally drops anonymous keyword/operator **tokens** (`is_named() == false`) whose `kind()` string collides with a same-named semantic rule — the `"contract"` keyword token versus the `contract` rule node, for instance. The rule is kept; the bare keyword is dropped, yielding clean child ordering for the CFG builder.

### Kind mapping: `map_kind`

`map_kind(ts_kind, node, source)` dispatches on the mapper's `Language` to a per-language routine; several languages share one routine, and unknown languages fall through to a generic mapper that emits `Unknown { kind }`:

| `Language` | Routine | | `Language` | Routine |
|------------|---------|---|------------|---------|
| `Rust` | `map_rust` | | `Ruby` | `map_ruby` |
| `Python` | `map_python` | | `Json` | `map_json` |
| `JavaScript` \| `TypeScript` | `map_javascript` | | `Html` | `map_html` |
| `Go` | `map_go` | | `Css` | `map_css` |
| `Java` | `map_java` | | `Bash` | `map_bash` |
| `C` \| `Cpp` | `map_c_cpp` | | `Yaml` \| `Toml` | `map_config` |
| `Markdown` | `map_markdown` | | `Rholang` \| `MeTTa` | `map_rholang` / `map_metta` (feature-gated) |

Each routine matches the grammar's node-kind strings onto shared `CpgNodeKind` variants — `function_item` → `Function`, `if_expression` → `If`, `call_expression` → `Call`, `integer_literal` → `Literal`, and so on — and always has an `Unknown { kind }` fallback so an unrecognized construct is preserved rather than lost. Two invariants the mappers uphold make the downstream overlays work: a named function/procedure maps to `Function` (so the CFG builder seeds an entry and treats its trailing body child as the body), and a *bound* name maps to `Variable`/`Parameter` (a DFG definition) while a *referenced* name maps to `Identifier` (a DFG use). The per-language routines are catalogued in [`../components/builder/node-mapper.md`](../components/builder/node-mapper.md).

## The Rholang frontend (Mode B)

Rholang is the concurrent [process-calculus](../GLOSSARY.md#rholang) language of the F1R3FLY.io ecosystem (`Paradigm::ProcessCalculus`). Its grammar has no imperative anchors of its own, so `map_rholang` normalizes ρ-calculus constructs onto the nearest CPG kind that keeps the CFG/DFG sound:

![Mapping of Rholang constructs to CPG node kinds](../diagrams/rholang-mapping.svg)

*Figure — Rholang constructs mapped to CPG node kinds: contracts become functions, sends/receives become calls, bound names become variables/parameters, and a `rho:` URI declaration becomes an import. Source: [`diagrams/rholang-mapping.dot`](../diagrams/rholang-mapping.dot).*

| Rholang construct | `CpgNodeKind` | Why |
|-------------------|---------------|-----|
| `contract`, `constructor_decl`, `method_decl`, `default_decl` | `Function` | Named process abstractions; seed a CFG entry, body is the last child |
| `agent_block` | `Class` | Object-like grouping of a constructor + methods |
| `new`, `let`, `for`-`input`, `{…}`, `par`, `bundle` | `Block` | Process regions / restriction scopes |
| `ifElse` | `If`; `match`, `choice` → `Match`; `case`, `branch` → `MatchArm` | Control flow |
| `send`, `send_sync` (`x!(v)`) | `Call` | The load-bearing send site |
| `linear_bind`, `repeated_bind`, `peek_bind` (`x <- src`) | `Call` | A receive consumes on `src` |
| `name_decl` with a `` `rho:…` `` URI | `Import` (URI, backticks stripped) | The polyglot system-process anchor |
| `var` (context-dependent) | `Variable` / `Parameter` / `Identifier` | Definition vs use, by syntactic position |
| `eval` (`*x`), `quote` (`@P`) | `UnaryOp` | Dereference / reify |

The `var` classification (`classify_rholang_var`) is what makes the Rholang DFG sound: a name bound by a `new`, a `let` binder, or a contract/agent formal or receive pattern is a *definition* (`Variable`/`Parameter`); a name used as a send channel, argument, or `*x` target is a *use* (`Identifier`). This yields real def-use edges — e.g. in `contract @"main"() = { new c in { c!(1) } }` the new-bound channel `c` (a `Variable`) links to its use as the send channel via a `DefUse` edge.

```rust
// requires: features = ["rholang"]   (an empty cfg toggle; the caller links the grammar)
use libcpg::{TreeSitterCpgBuilder, CodePropertyGraph, Language};

// The caller owns `tree_sitter_rholang` and parses first (Mode B).
fn build_rholang() -> Result<CodePropertyGraph, Box<dyn std::error::Error>> {
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&tree_sitter_rholang::LANGUAGE.into())?;
    let src = r#"new stdout(`rho:io:stdout`) in { contract @"greet"(@name) = { stdout!("hi " ++ *name) } }"#;
    let tree = parser.parse(src, None).expect("parse");
    let cpg = TreeSitterCpgBuilder::new().build_from_tree(&tree, src, Language::Rholang)?;
    // `greet` is a Function (CFG entry); `stdout!(…)` is a Call; `rho:io:stdout` is an Import.
    Ok(cpg)
}
```

## The MeTTa frontend (Mode B)

MeTTa is a rewriting language over symbolic [S-expressions](../GLOSSARY.md#s-expression) (`Paradigm::Logic`/`Functional`). Its grammar wraps every node in transparent `expression`/`atom_expression` layers and expresses definitions as a `list` whose *head atom* carries the semantics; `map_metta` therefore dispatches on each list's unwrapped head:

![Mapping of MeTTa S-expressions to CPG node kinds by head atom](../diagrams/metta-mapping.svg)

*Figure — MeTTa lists mapped by head atom: `(= …)` becomes a function, `(: …)` a type annotation, `(import! …)` an import, and grounded operations become calls. Source: [`diagrams/metta-mapping.dot`](../diagrams/metta-mapping.dot).*

| MeTTa form | `CpgNodeKind` | Why |
|------------|---------------|-----|
| `(= LHS RHS)`, `(:= LHS RHS)` | `Function` (named from the rule head) | Rule definition; body is the RHS (last child), seeds a CFG entry |
| `(: name Type)`, `(-> A B R)` | `TypeAnnotation` | Type / arrow-type |
| `(if …)` | `If`; `(case …)`/`(match …)` → `Match`; `(let …)`/`(let* …)` → `Block` | Built-in control/binding forms |
| `(import! &space module)` | `Import` (module path) | Module import |
| `(+ $a $b)`, `($f …)`, `(&self …)` | `Call` | Grounded / higher-order / space application |
| `$x` (context-dependent) | `Parameter` / `Identifier` | Rule-LHS binder (definition) vs use |
| `identifier`, `&self`, `%Undefined%` | `Identifier` | Atoms and space/type references |

`classify_metta_var` promotes a `$variable` to a `Parameter` definition when it is a rule-LHS binder — the `$x` in `(= (double $x) …)` — and leaves it an `Identifier` use everywhere else. So `(= (double $x) (* $x 2))` becomes a `Function` named `double` with the LHS `$x` (a `Parameter`) linked to the RHS `$x` (a use) by a `DefUse` edge.

## Why Mode B, and why the toggles are empty

The `rholang` and `metta` features are **empty** feature lists (`rholang = []`, `metta = []`): pure logical `cfg` toggles that gate only the `map_rholang`/`map_metta` arms and their dispatch. Those arms reference only `ts_kind` strings and `tree_sitter::Node` navigation — never a grammar crate symbol — so enabling them pulls in nothing. The ρ-calculus / S-expression CPG is built from a caller-supplied tree via `build_from_tree`.

The reason is duplicate-symbol avoidance. A consumer such as pgmcp already links the Rholang and MeTTa grammars; if libcpg vendored them as regular dependencies, the shared `tree_sitter_<lang>` C symbol could be defined twice at link time. Instead the grammars are **test-only path `[dev-dependencies]`** (which Cargo never propagates downstream), used solely to drive the Mode-B unit tests, while the runtime features stay dependency-free. The 16 first-class grammars are likewise pinned to versions that match pgmcp's (python/javascript → 0.25, c → 0.24, …) for the same reason. The full rationale is [`../design/0002-mode-b-build-from-tree.md`](../design/0002-mode-b-build-from-tree.md).

## Adding a language

A new first-class frontend is three coordinated changes (walked through in [`../engineering/04-contributing.md`](../engineering/04-contributing.md)):

1. add a `map_<lang>` arm to `NodeMapper::map_kind` (and any `should_include` wrappers the grammar needs);
2. register the grammar in `ParserRegistry::new` under a `#[cfg(feature = "lang-<lang>")]` block; and
3. declare the optional grammar dependency and the `lang-<lang>` feature in `Cargo.toml`.

A Mode-B-only frontend (like Rholang/MeTTa) skips step 2 and uses an empty `cfg` toggle in place of step 3, relying on the caller to supply the tree.

## Where to go next

- [`overview.md`](overview.md) — where the frontend sits in the architecture.
- [`data-flow.md`](data-flow.md) — the post-parse pipeline the frontend feeds.
- [`../components/builder/node-mapper.md`](../components/builder/node-mapper.md) — every per-language mapper in detail.
- [`../usage/06-f1r3fly-rholang-metta.md`](../usage/06-f1r3fly-rholang-metta.md) — a Mode-B integration walkthrough.
- [`../design/0002-mode-b-build-from-tree.md`](../design/0002-mode-b-build-from-tree.md) — the Mode-B decision record.

## References

1. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and Discovering Vulnerabilities with Code Property Graphs.* 2014 IEEE Symposium on Security and Privacy. DOI: [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44)
