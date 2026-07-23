# Testing

`libcpg`'s automated tests are **99 inline unit tests**, every one written in a
`#[cfg(test)] mod tests { … }` block next to the code it exercises. There are, at
present, no integration tests, no runnable examples, no doctests, and no
benchmarks. This page is an honest census of what exists, how to run it under
different [feature](../GLOSSARY.md#feature-flag-cargo) subsets, and what is
deliberately absent.

## The test census

All 99 tests are `#[test]` functions inside `#[cfg(test)]` modules. Counted by
source file:

| Pillar | File | `#[test]` fns |
| --- | --- | ---: |
| **builder** | `src/builder/tree_sitter.rs` | 10 |
| | `src/builder/node_mapper.rs` | 10 |
| | `src/builder/dfg.rs` | 7 |
| | `src/builder/pdg.rs` | 6 |
| | `src/builder/parser_registry.rs` | 4 |
| | `src/builder/cfg.rs` | 3 |
| **patterns** (`design-patterns`) | `src/patterns/classification/classifier.rs` | 8 |
| | `src/patterns/design/dpml.rs` | 7 |
| | `src/patterns/design/gang_of_four.rs` | 7 |
| | `src/patterns/design/metrics.rs` | 3 |
| | `src/patterns/design/templates.rs` | 3 |
| **pattern** (always on) | `src/pattern/mod.rs` | 4 |
| | `src/pattern/similarity.rs` | 4 |
| | `src/pattern/vf2.rs` | 4 |
| **algorithms** (`algorithm-detection`) | `src/algorithms/detection/control_flow.rs` | 5 |
| | `src/algorithms/detection/complexity.rs` | 4 |
| | `src/algorithms/detection/mod.rs` | 2 |
| **graph** | `src/graph/cpg.rs` | 4 |
| **gnn** (`gnn`) | `src/gnn/cpgnn.rs` | 2 |
| | `src/gnn/embeddings.rs` | 2 |
| **Total** | | **99** |

Grouped by pillar: builder 40, patterns 28, pattern 12, algorithms 11, graph 4,
gnn 4.

### Not every test compiles under every build

Because whole modules are feature-gated, the number of tests that actually *run*
depends on the features you enable:

- The 28 `patterns` tests compile only under `design-patterns`; the 11
  `algorithms` tests only under `algorithm-detection`; the 4 `gnn` tests only
  under `gnn`.
- Within `builder`, several tests are further gated on a grammar. In
  `tree_sitter.rs`, `test_function_entry_points`, `test_retain_source`, and
  `test_build_from_tree_matches_build` are `#[cfg(feature = "lang-rust")]`; in
  `parser_registry.rs`, `test_rust_parser_available` needs `lang-rust` and
  `test_python_parser_available` needs `lang-python`.
- In `node_mapper.rs`, 2 tests are always on and the other 8 are Mode-B tests: 5
  are `#[cfg(feature = "rholang")]` and 3 are `#[cfg(feature = "metta")]`.

The remaining tests — the graph model, the CFG/DFG/PDG extractors, VF2, and
similarity — exercise the **feature-free surface** by building CPGs by hand, so
they run even on the bare `default = []` build.

### The Mode-B grammar dev-dependencies

The `rholang` and `metta` tests parse a real `.rho` / `.metta` snippet and drive
[`build_from_tree`](../GLOSSARY.md#mode-b--build_from_tree) end to end. To do that
they need actual grammars, which are declared as **test-only path
`[dev-dependencies]`**:

```toml
# Cargo.toml (excerpt) — TEST-ONLY; never propagated to downstream crates
[dev-dependencies]
tree-sitter-rholang = { package = "rholang-tree-sitter", path = "…/rholang-tree-sitter" }
tree-sitter-metta   = { package = "tree-sitter-metta",   path = "…/tree-sitter-metta" }
```

They are dev-dependencies precisely so that a consumer linking `libcpg` (e.g.
pgmcp, which links these same grammars itself) never pulls them in transitively —
avoiding the duplicate `tree_sitter_<lang>` C-symbol hazard described in
[`design/0002-mode-b-build-from-tree.md`](../design/0002-mode-b-build-from-tree.md).
Because they are `path` dependencies, the paths must resolve on your machine for
the `rholang`/`metta` tests to build.

## Running the tests

```sh
# Feature-free surface only (default = []): the graph model, CFG/DFG/PDG, VF2,
# similarity — everything that does not need a grammar or a gated module.
cargo test

# The maximal run: compiles and runs every test, including the Mode-B
# rholang/metta tests (which `full` omits) — requires the path dev-deps to resolve.
cargo test --all-features

# A single language plus one analysis: runs the always-on tests, the lang-rust
# grammar tests, and the design-patterns tests.
cargo test --features "lang-rust,design-patterns"

# Everything in `full` EXCEPT rholang/metta/gpu/ml-linfa (see build-and-features).
cargo test --features full

# Just the Mode-B mappers.
cargo test --features "rholang,metta"

# Scope to one module's tests by path filter.
cargo test --features lang-rust builder::tree_sitter
```

Under `default = []`, `cargo test` skips the gated modules entirely — this is
expected, not a failure. To exercise the pattern/algorithm/GNN suites you must
enable their features.

## The build-equivalence invariant

One test deserves special mention because it pins a core correctness property. The
crate offers two construction paths — the internal-parse `build` ("Mode A") and the
caller-supplied-tree `build_from_tree` ("Mode B") — and `build` is implemented by
*delegating* its post-parse pipeline to `build_from_tree`. The two must therefore
produce identical graphs.

![Mode A / Mode B build-equivalence invariant](../diagrams/build-equivalence.svg)

*Figure — `build(source, Rust)` and `build_from_tree(&tree, source, Rust)` must
yield the same node and edge counts and the same language. Source:
[`diagrams/build-equivalence.puml`](../diagrams/build-equivalence.puml).*

`test_build_from_tree_matches_build` (in `src/builder/tree_sitter.rs`, gated on
`lang-rust`) parses `fn main() { let x = 1; let y = x; }` externally, builds the
CPG both ways, and asserts:

```rust
// requires: features = ["lang-rust"]  (shape of the shipped test)
assert!(from_tree.node_count() > 1);
assert_eq!(from_tree.node_count(), from_source.node_count());
assert_eq!(from_tree.edge_count(), from_source.edge_count());
assert_eq!(from_tree.language(), Language::Rust);
```

The same invariant is analysed in
[`scientific/01-cpg-invariants-and-equivalence.md`](../scientific/01-cpg-invariants-and-equivalence.md).
Note also that the shipped tests consistently use `.expect("…")` rather than
`.unwrap()`, matching the [coding conventions](04-contributing.md).

## What is deliberately absent (documented gaps)

To avoid overselling, here is what the test tree does **not** contain:

| Surface | State |
| --- | --- |
| `examples/` directory | present but **empty** — there are no runnable `cargo run --example …` programs. |
| `tests/` directory | present but **empty** — there are no integration tests (`tests/*.rs`), so nothing exercises the crate purely through its public API across module boundaries. |
| Doctests | none run — every code fence in a doc comment is marked `ignore` / `rust,ignore`, so `cargo test --doc` executes nothing. |
| `proptest` | version `1.5` is a dev-dependency, but there are **zero** `proptest!` cases; property-based testing is available but unused. |
| Benchmarks | `criterion 0.5` is a dev-dependency, but both `[[bench]]` targets in `Cargo.toml` are **commented out** (`"Benchmarks will be added when implementations are complete"`), so `cargo bench` has no targets. See [performance](03-performance.md) for how to add them. |

These gaps are opportunities, not defects in the implemented code — the inline
suite covers the graph model, the three extractors, PDG slicing, VF2, similarity,
and the gated analyses. Adding integration tests, doctests, `proptest` cases, and
benches are all good first contributions; see [contributing](04-contributing.md).

## Related pages

- [Build and features](01-build-and-features.md) — the feature subsets referenced
  above.
- [Performance](03-performance.md) — the benchmarking gap and how to close it.
- [`scientific/00-overview.md`](../scientific/00-overview.md) — how the inline
  tests are read as validation evidence.
