# Contributing

This page gives concrete, code-anchored recipes for the three most common
extensions to `libcpg` — adding a language, adding a Gang-of-Four pattern
template, and adding a graph-similarity metric — followed by the coding
conventions the crate holds itself to. Each recipe names the real functions you
will edit; nothing here invents an API.

Before starting, skim [build and features](01-build-and-features.md) (so you know
which feature gates your code) and [testing](02-testing.md) (so you know where the
tests go).

## Recipe 1 — Add a language

There are two flavours, distinguished by *who owns the grammar*.

![Language front-end pipeline](../diagrams/language-frontend-pipeline.svg)

*Figure — from source (or a caller-supplied tree) through `ParserRegistry` and
`NodeMapper` to CPG nodes. Source:
[`diagrams/language-frontend-pipeline.puml`](../diagrams/language-frontend-pipeline.puml).*

### Flavour A — a vendored grammar usable through `build`

Use this when `libcpg` should link the grammar itself and parse the source
internally (like the 16 built-in `lang-*` languages).

1. **Ensure a `Language` variant exists.** `Language` is `#[non_exhaustive]`; if
   your language is not already a variant, add one and give it a `name()`,
   `extensions()`, and `paradigms()` so `Language::from_extension` and the
   predicates work.
2. **Declare the grammar and feature** in `Cargo.toml`:
   ```toml
   tree-sitter-foo = { version = "0.23", optional = true }

   [features]
   lang-foo = ["dep:tree-sitter-foo"]
   ```
   Pin the version to match any other crate in the target dependency graph that
   links the same grammar (see the pgmcp pin discussion in
   [build and features](01-build-and-features.md)); optionally add `lang-foo` to a
   group and to `lang-all`.
3. **Register the grammar** in `ParserRegistry::new` (`src/builder/parser_registry.rs`),
   gated on the feature so a default build stays empty:
   ```rust
   // src/builder/parser_registry.rs — inside ParserRegistry::new
   #[cfg(feature = "lang-foo")]
   {
       parsers.insert(Language::Foo, tree_sitter_foo::LANGUAGE.into());
   }
   ```
4. **Add a mapper arm.** `NodeMapper::map_kind` dispatches on `self.language`; add
   an arm that calls a new `map_foo`:
   ```rust
   // src/builder/node_mapper.rs — inside NodeMapper::map_kind's `match self.language`
   Language::Foo => self.map_foo(ts_kind, node, source),
   ```
   Then write `fn map_foo(&self, ts_kind: &str, node: &tree_sitter::Node, source: &str) -> CpgNodeKind`,
   translating that grammar's node-kind strings into `CpgNodeKind` variants
   (`Function`, `If`, `Call`, `Identifier`, …). If some rule nodes are transparent
   wrappers, extend `should_include` / `should_include_node` to drop them.
5. **Test it**, gated on the feature (so it only runs when the grammar is present):
   ```rust
   // requires: features = ["lang-foo"]
   #[test]
   #[cfg(feature = "lang-foo")]
   fn foo_builds() {
       let cpg = TreeSitterCpgBuilder::new()
           .build("…foo source…", Language::Foo)
           .expect("build should succeed");
       assert!(cpg.node_count() > 1);
   }
   ```

### Flavour B — a Mode-B-only language (no vendored grammar)

Use this when the caller already links the grammar and will hand you a parsed tree
— exactly how [Rholang](../GLOSSARY.md#rholang) and [MeTTa](../GLOSSARY.md#metta)
work. The difference from Flavour A is that the grammar is **never** a regular
dependency and the language is **not** registered in `ParserRegistry`.

1. Add the `Language` variant (as above).
2. Declare an **empty** feature — a pure `cfg` toggle:
   ```toml
   [features]
   foo = []
   ```
3. **Do not** register a grammar in `ParserRegistry`; the caller supplies the tree
   to [`build_from_tree`](../GLOSSARY.md#mode-b--build_from_tree).
4. Add the mapper arm **gated on the empty feature**, referencing only `ts_kind`
   strings and `tree_sitter::Node` navigation (never a grammar symbol):
   ```rust
   // src/builder/node_mapper.rs — inside map_kind
   #[cfg(feature = "foo")]
   Language::Foo => self.map_foo(ts_kind, node, source),
   ```
5. Add the grammar only as a **test-only path `[dev-dependency]`** and write
   `#[cfg(feature = "foo")]` tests that parse a snippet and call `build_from_tree`,
   mirroring the existing `rholang`/`metta` tests in `node_mapper.rs`. Because
   Cargo never propagates dev-dependencies, downstream consumers stay free of the
   grammar and its C symbols. The rationale is
   [`design/0002-mode-b-build-from-tree.md`](../design/0002-mode-b-build-from-tree.md);
   the mapper details are in
   [`components/builder/node-mapper.md`](../components/builder/node-mapper.md).

## Recipe 2 — Add a Gang-of-Four template

Pattern detection lives in the `patterns` module (feature `design-patterns`).
Templates are structural subgraphs matched by a **relaxed** [VF2](../GLOSSARY.md#vf2)
matcher; a match is scored against template completeness and kept if its
[confidence](../GLOSSARY.md#confidence-pattern-match) is at least
`min_confidence` (default `0.7`; the Observer template uses `0.8`).

1. **Pick or add a `GofPattern` variant.** The enum already carries all 23 GoF
   patterns (`GofPattern::FactoryMethod`, never `Factory`); a genuinely new
   structural template needs a new variant.
2. **Write the template.** In `src/patterns/design/templates.rs`, add a
   `foo_template() -> PatternTemplate` modelled on the existing ones (e.g.
   `singleton_template`, `observer_template`). They build
   `NodeConstraint`/`EdgeConstraint` values over
   `NodeKindMatcher`/`NodeKindTag`/`EdgeKindMatcher` using node kinds like
   [`Trait`](../GLOSSARY.md#node-kind--edge-kind) (interfaces), `Class`, `Field`,
   `Function` and edges `AstChild`/`Inherits`/`Implements`/`TypeOf`. Copy the
   nearest existing template and adjust.
3. **Wire it into both dispatchers** in the same file so the detector and the
   test-CPG builder can find it:
   ```rust
   // src/patterns/design/templates.rs
   pub fn build_pattern_template(pattern: GofPattern) -> PatternTemplate {
       match pattern {
           // … existing arms …
           GofPattern::Foo => foo_template(),
       }
   }
   ```
   If you want a hand-built exemplar CPG for tests, also add a
   `build_foo_pattern() -> CodePropertyGraph` and its arm in `build_pattern_cpg`.
4. **Detect and test.** `GofPatternDetector` runs the relaxed matcher and confidence
   scoring:
   ```rust
   // requires: features = ["design-patterns"]
   use libcpg::patterns::{GofPatternDetector, GofPattern, PatternDetector};

   let detector = GofPatternDetector::new()
       .with_patterns(vec![GofPattern::Foo])
       .with_min_confidence(0.7);
   let matches = detector.detect(&cpg);
   ```
   Add tests to `gang_of_four.rs` / `templates.rs`. Pattern intents and detection
   signatures are documented in
   [`components/patterns/gang-of-four.md`](../components/patterns/gang-of-four.md).

## Recipe 3 — Add a similarity metric

Graph similarity lives in the always-on `pattern` module, so no feature gate is
needed.

1. **Add a `SimilarityMetric` variant** (alongside `Jaccard`, `Cosine`,
   `WeisfeilerLehman`, `GraphEdit`).
2. **Handle it** in the `match self.metric` inside `GraphSimilarity::similarity`
   in `src/pattern/similarity.rs`, returning an `f64` in `[0, 1]`. Respect the
   structural/label blend (weights `0.7` / `0.3`) if your metric has both
   components.
   ```rust
   // src/pattern/similarity.rs — inside GraphSimilarity::similarity
   pub fn similarity(&self, g1: &CodePropertyGraph, g2: &CodePropertyGraph) -> f64 {
       match self.metric {
           // … existing arms …
           SimilarityMetric::Foo => self.foo_similarity(g1, g2),
       }
   }
   ```
3. **Test it** in `similarity.rs`, comparing a graph to itself (expect `1.0`) and
   to a clearly different graph. The metrics are described in
   [`theory/06-graph-similarity.md`](../theory/06-graph-similarity.md).

## Coding conventions

`libcpg` compiles under `#![warn(missing_docs)]` and `#![warn(clippy::all)]`, and
follows these house rules — please keep new code consistent with them:

- **Prefer `.expect("message")` over `.unwrap()`.** A panic should say what
  invariant broke; the shipped tests already do this everywhere.
- **Document every public item.** `missing_docs` is a warning; do not add
  undocumented `pub` items.
- **Preallocate when the count is known.** Reach for `Vec::with_capacity` /
  `FxHashMap::with_capacity` when you can size a collection up front — this is a
  standing optimization opportunity in construction, not a premature one (see
  [performance](03-performance.md)).
- **Prefer pattern matching to predicate chains.** `match` / `matches!` on node
  and edge kinds is both clearer and, in Rust, usually faster than a ladder of
  boolean predicates — the mappers and extractors are written this way.
- **Keep the extractors [idempotent](../GLOSSARY.md#idempotent).**
  `CfgExtractor::extract`, `DfgExtractor::extract`, and `PdgBuilder::build` must
  not duplicate edges when re-run; preserve that when editing them.
- **Return `libcpg::Error`.** The crate's error type is `Error` (with `Result<T>`);
  there is no `CpgError`. Add a variant if you need a new failure mode.
- **If you introduce concurrency, keep it non-blocking.** Any future parallelism
  (the `rayon` dependency is declared but currently unused — see
  [performance](03-performance.md)) should favour data-parallel iterators and
  persistent/atomic structures over locks.

## Related pages

- [Testing](02-testing.md) — where each kind of test belongs and how to run it.
- [`architecture/language-frontends.md`](../architecture/language-frontends.md) —
  the front-end that Recipe 1 extends.
- [`components/patterns/vf2-matching.md`](../components/patterns/vf2-matching.md) —
  the matcher that Recipes 2 and 3 build on.
