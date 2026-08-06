# Build and Features

`libcpg` compiles almost nothing by default and lets you opt in to exactly the
language grammars and analyses you need. This page is the authoritative map of the
[Cargo feature flags](../GLOSSARY.md#feature-flag-cargo): what each one turns on,
which optional dependencies it drags in, how the groups compose, and why the
grammar version pins are what they are. Everything here is taken from
[`Cargo.toml`](../../Cargo.toml).

![Feature-flag dependency map for libcpg](../diagrams/feature-flag-map.svg)

*Figure — the feature graph: `default = []`, the analysis features, the 16 `lang-*`
grammars and their groups, and the `full` umbrella. Source:
[`diagrams/feature-flag-map.puml`](../diagrams/feature-flag-map.puml).*

## Installing

Add the crate and choose features explicitly — the empty default set means an
unqualified dependency can parse nothing:

```toml
# Cargo.toml — a typical Rust-analysis setup
[dependencies]
libcpg = { version = "0.1", features = ["lang-rust", "design-patterns"] }
```

Or from the command line:

```sh
cargo add libcpg --features "lang-rust,design-patterns"
```

### The consequence of `default = []`

With no features, the only construction paths that work are the feature-free ones:
hand-assembling a `CodePropertyGraph` node by node, or the
[Mode B](../GLOSSARY.md#mode-b--build_from_tree) `build_from_tree` entry point
(where *you* own an already-parsed [tree-sitter](../GLOSSARY.md#tree-sitter) tree).
The internal-parse path fails because no grammar is registered:

```rust
// requires: no features (this is the default `default = []` build)
use libcpg::{CpgBuilder, TreeSitterCpgBuilder, Language};

let builder = TreeSitterCpgBuilder::new();
// With no `lang-*` feature, the parser registry is empty, so `build`
// returns Err(Error::UnsupportedLanguage(..)).
let result = builder.build("fn main() {}", Language::Rust);
assert!(result.is_err());
```

Enabling `lang-rust` makes the same call succeed. See
[`usage/00-getting-started.md`](../usage/00-getting-started.md) for a first
working build and [`usage/01-building-cpgs.md`](../usage/01-building-cpgs.md) for
`build` vs. `build_from_tree`.

## Feature matrix

### Analysis and serialization features

| Feature | Enables (deps) | What it gates | Notes |
| --- | --- | --- | --- |
| `gnn` | `ndarray`, `rand` | the `gnn` module: `CpgGnn`, `NodeEmbedding`, message passing | See [`components/gnn/overview.md`](../components/gnn/overview.md). |
| `design-patterns` | `serde_yaml`, `toml_edit` | the `patterns` module: `GofPatternDetector`, [DPML](../GLOSSARY.md#dpml-design-pattern-markup-language), classification | YAML/TOML deps are for loading DPML templates. |
| `algorithm-detection` | *(none)* | the `algorithms` module: `DefaultAlgorithmDetector`, complexity heuristics | Empty dep list — a pure `cfg` toggle. |
| `serde` | `dep:serde`, `petgraph/serde-1` | `Serialize`/`Deserialize` derives on the graph types | Derive-based; no bespoke on-disk format. See [`usage/05-serialization.md`](../usage/05-serialization.md). |
| `ml-linfa` | `linfa`, `linfa-trees` | the ML back end for `PatternClassifier` | **Excluded from `full`** (pulls a large ML stack). |
| `ml-rules` | *(none)* | the rule-based back end for `PatternClassifier` | The lightweight classifier path; included in `full`. |
| `gpu` | `wgpu`, `pollster` | *(reserved)* | **No GPU code is wired yet** — the flag reserves the dependency surface only. Excluded from `full`. |

The [`patterns`], [`algorithms`], and [`gnn`] modules are compiled *only* when their
feature is on (`#[cfg(feature = "…")]` on the `pub mod` in `lib.rs`); the `graph`,
`analysis`, `builder`, and [`pattern`](../GLOSSARY.md#vf2) modules are always compiled. The
feature-free `analysis` module supplies exact CFG and call-graph SCC decomposition. Note the
deliberate distinction between the always-on `pattern` module (VF2 / similarity) and
the feature-gated `patterns` module (Gang-of-Four detection) — they are different
modules, not a typo.

### Mode-B language toggles

| Feature | Enables (deps) | What it gates |
| --- | --- | --- |
| `rholang` | *(none — `= []`)* | the `map_rholang` node-mapper arm and its `#[cfg(feature = "rholang")]` unit tests |
| `metta` | *(none — `= []`)* | the `map_metta` node-mapper arm and its `#[cfg(feature = "metta")]` unit tests |

These are intentionally empty. The [Rholang](../GLOSSARY.md#rholang) and
[MeTTa](../GLOSSARY.md#metta) mappers reference only tree-sitter node-kind strings,
never a grammar symbol, so there is nothing to pull in: the ρ-calculus /
S-expression CPG is built from a **caller-supplied** parse tree via
`build_from_tree`. The grammars themselves are linked only as test-only path
`[dev-dependencies]` (`rholang-tree-sitter`, `tree-sitter-metta`) to drive the
Mode-B tests, and Cargo never propagates dev-dependencies downstream. The rationale
— avoiding a duplicate `tree_sitter_<lang>` C symbol in consumers like pgmcp — is
recorded in [`design/0002-mode-b-build-from-tree.md`](../design/0002-mode-b-build-from-tree.md).

### Language grammars (`lang-*`)

Sixteen grammars, each gating one optional `tree-sitter-<lang>` dependency and its
entry in the [`ParserRegistry`](../GLOSSARY.md#tree-sitter):

| Feature | Grammar crate | Pinned version |
| --- | --- | --- |
| `lang-rust` | `tree-sitter-rust` | `0.24` |
| `lang-python` | `tree-sitter-python` | `0.25` |
| `lang-javascript` | `tree-sitter-javascript` | `0.25` |
| `lang-typescript` | `tree-sitter-typescript` | `0.23` |
| `lang-go` | `tree-sitter-go` | `0.25` |
| `lang-java` | `tree-sitter-java` | `0.23` |
| `lang-c` | `tree-sitter-c` | `0.24` |
| `lang-cpp` | `tree-sitter-cpp` | `0.23` |
| `lang-json` | `tree-sitter-json` | `0.24` |
| `lang-html` | `tree-sitter-html` | `0.23` |
| `lang-css` | `tree-sitter-css` | `0.25` |
| `lang-bash` | `tree-sitter-bash` | `0.25` |
| `lang-toml` | `tree-sitter-toml-ng` | `0.7` |
| `lang-yaml` | `tree-sitter-yaml` | `0.7` |
| `lang-markdown` | `tree-sitter-md` | `0.5` |
| `lang-ruby` | `tree-sitter-ruby` | `0.23` |

### Language groups

Convenience umbrellas that pull in several grammars at once:

| Group | Expands to |
| --- | --- |
| `lang-systems` | `lang-rust`, `lang-c`, `lang-cpp`, `lang-go` |
| `lang-scripting` | `lang-python`, `lang-javascript`, `lang-typescript`, `lang-ruby` |
| `lang-web` | `lang-html`, `lang-css`, `lang-javascript`, `lang-typescript` |
| `lang-config` | `lang-json`, `lang-yaml`, `lang-toml` |
| `lang-all` | `lang-systems` + `lang-scripting` + `lang-web` + `lang-config` + `lang-bash` + `lang-markdown` + `lang-java` |

`lang-all` therefore covers all 16 grammars (the overlaps between `lang-scripting`
and `lang-web` on JavaScript/TypeScript are harmless — a feature enabled twice is
enabled once).

### The `full` umbrella

```text
full = ["gnn", "design-patterns", "algorithm-detection", "serde", "ml-rules", "lang-all"]
```

`full` is the "everything an application typically wants" set. It deliberately
**excludes** four features:

- `rholang` and `metta` — Mode-B-only, driven by test-only grammars (above);
- `gpu` — reserved, no code;
- `ml-linfa` — a heavy ML dependency stack (`ml-rules` provides a lightweight
  classifier instead).

To get those you must name them explicitly, e.g.
`features = ["full", "rholang", "metta"]`.

## Why the grammar pins are what they are

Tree-sitter grammars ship a C entry point named `tree_sitter_<lang>`. If two crates
in one dependency graph link *different* versions of the same grammar, the linker
sees the symbol twice and fails. Because pgmcp links several of these grammars
itself and then calls `libcpg`, the pins for the grammars they share must match
pgmcp's. In practice that fixes `tree-sitter-python` and `tree-sitter-javascript`
at `0.25` and `tree-sitter-c` at `0.24`; the remaining grammars are pinned to the
latest versions compatible with `tree-sitter 0.25`. The `Cargo.toml` comment
records this against pgmcp's design section, and the broader decision is
[`design/0005-feature-flag-taxonomy.md`](../design/0005-feature-flag-taxonomy.md).

## Building with features

```sh
# One language + design-pattern detection
cargo build --features "lang-rust,design-patterns"

# Everything in `full` (all 16 grammars, gnn, patterns, algorithms, serde, ml-rules)
cargo build --features full

# All features, including the ones full omits (rholang, metta, gpu, ml-linfa)
cargo build --all-features

# A single systems-language group
cargo build --features lang-systems
```

## `supported_languages()` is static; `ParserRegistry::supports` is real

Two APIs report "which languages are available", and they answer different
questions:

- `TreeSitterCpgBuilder::supported_languages()` returns a **static** slice of the
  16 languages the builder *could* support. It does not consult the enabled
  features, so it is the same regardless of how you compiled the crate. Treat it as
  a catalogue, not a capability check.
- `ParserRegistry::global().supports(lang)` reflects **actual** availability: the
  registry only inserts a grammar under its `#[cfg(feature = "lang-…")]`, so
  `supports` returns `true` only for grammars compiled in.

```rust
// requires: features = ["lang-rust"]
use libcpg::builder::ParserRegistry;
use libcpg::Language;

let registry = ParserRegistry::global();
assert!(registry.supports(Language::Rust));      // lang-rust is on
assert!(!registry.supports(Language::Python));   // lang-python is off
```

Use `ParserRegistry::supports` (or simply attempt `build` and handle
`Error::UnsupportedLanguage`) when you need to know whether a given build can
actually parse a language.

## Related pages

- [Testing](02-testing.md) — running the suite under different feature subsets.
- [Performance](03-performance.md) — which declared dependencies are actually wired.
- [`architecture/language-frontends.md`](../architecture/language-frontends.md) —
  how `ParserRegistry` and `NodeMapper` fit together.
