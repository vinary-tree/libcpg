# Engineering Overview

This is the entry point to `libcpg`'s **engineering** documentation: how the crate
is built and configured, how it is tested, how it performs (and how to measure
that rigorously), and how to contribute to it. Its sibling pillar,
[`security/`](../security/00-threat-model.md), documents the threat model and the
input/resource bounds that keep analysis finite on untrusted input.

`libcpg` is a Rust library for constructing and querying [Code Property
Graphs](../GLOSSARY.md#code-property-graph-cpg) (CPGs) — a single graph that
overlays a program's [AST](../GLOSSARY.md#abstract-syntax-tree-ast),
[CFG](../GLOSSARY.md#control-flow-graph-cfg),
[DFG](../GLOSSARY.md#data-flow-graph-dfg), and, on demand, its
[PDG](../GLOSSARY.md#program-dependence-graph-pdg) — plus exact SCC decomposition,
pattern detection, algorithm/complexity heuristics, and graph-neural embeddings on top of that
substrate. The conceptual "why" lives in [`theory/`](../theory/00-overview.md);
the "what/where" of the code lives in
[`architecture/`](../architecture/overview.md); this pillar is the "how to work
on it".

## Crate metadata

The following is taken verbatim from [`Cargo.toml`](../../Cargo.toml).

| Field | Value |
| --- | --- |
| Package name | `libcpg` |
| Version | `0.1.1` |
| Rust edition | `2021` |
| License | `MIT OR Apache-2.0` |
| Repository | <https://github.com/f1r3fly-io/libcpg> |
| Author | Dylon Edwards `<dylon@f1r3fly.io>` |
| Keywords | `code-analysis`, `cpg`, `ast`, `cfg`, `dfg`, `pattern-detection`, `gnn` |
| Categories | `development-tools`, `parsing`, `algorithms` |
| Default features | `default = []` (nothing on by default) |
| MSRV | not declared — `Cargo.toml` carries no `rust-version`; any edition-2021 toolchain that satisfies `tree-sitter 0.26` should build it |

The single most important fact for a newcomer is the empty default feature set:
with no [feature flags](../GLOSSARY.md#feature-flag-cargo) enabled,
`TreeSitterCpgBuilder::build(source, language)` fails for *every* language,
because no [tree-sitter](../GLOSSARY.md#tree-sitter) grammar is compiled in. Only
the feature-free surface — hand-built CPGs, the
[Mode B](../GLOSSARY.md#mode-b--build_from_tree) `build_from_tree` entry point,
exact CFG/call-graph [SCC](../GLOSSARY.md#strongly-connected-component-scc)
decomposition, [VF2](../GLOSSARY.md#vf2) matching, and PDG slicing — works out of the box. The
[build and features](01-build-and-features.md) page explains how to turn on what
you need.

## Ecosystem context

`libcpg` is a standalone crate in the [F1R3FLY.io](https://f1r3fly.io) workspace.
It has one especially relevant downstream consumer, **pgmcp**, which links its own
tree-sitter grammars and drives `libcpg` through
[Mode B](../GLOSSARY.md#mode-b--build_from_tree). Two engineering decisions follow
directly from that relationship, and both are visible in `Cargo.toml`:

- **Grammar version pins are matched to pgmcp** so the two crates do not each try
  to link a different copy of the same `tree_sitter_<lang>` C symbol (a duplicate
  symbol is a hard link-time error). The [build and features](01-build-and-features.md)
  page reproduces the pin table.
- **Rholang and MeTTa are Mode-B-only.** Their `rholang` / `metta` features are
  *empty* (`= []`) logical toggles that gate only the `map_rholang` / `map_metta`
  node-mapper arms; the grammars are linked solely as **test-only** path
  `[dev-dependencies]` (which Cargo never propagates downstream). See
  [`design/0002-mode-b-build-from-tree.md`](../design/0002-mode-b-build-from-tree.md).

## Map of the engineering pillar

| Page | What it covers |
| --- | --- |
| [01 — Build and features](01-build-and-features.md) | The full feature matrix: 16 `lang-*` grammars, the language groups, the analysis features, the Mode-B toggles, `serde`, the reserved `gpu` flag, and the `full` umbrella; the pgmcp grammar-pin table; how to enable a language. |
| [02 — Testing](02-testing.md) | The 474-test suite: 399 example-based tests, 63 `proptest` properties, and the `tests/integration.rs` / `tests/robustness.rs` public-API suites; the `arb_*` generators; how to run them under each feature subset; measured coverage (97.4 % lines); the fourteen defects the suite found; and an honest inventory of what is still absent (empty `examples/`, no doctests, no benches). |
| [03 — Performance](03-performance.md) | The data-structure rationale (what is actually wired vs. merely declared), the asymptotic cost of the key operations, and the (currently unimplemented) benchmarking methodology with instructions for adding `criterion` benches. |
| [04 — Contributing](04-contributing.md) | Step-by-step recipes: add a language mapper, add a Gang-of-Four template, add a similarity metric; plus the coding conventions the crate follows. |

## Map of the security pillar

| Page | What it covers |
| --- | --- |
| [`security/00` — Threat model](../security/00-threat-model.md) | Assets, attackers, and the two trust boundaries (untrusted source bytes; untrusted serialized graphs); why tree-sitter parsing executes no analyzed code; why detection results are advisory. |
| [`security/01` — Input and resource hardening](../security/01-input-and-resource-hardening.md) | The concrete bounds — `max_file_size`, DFG `max_iterations`, VF2 `max_matches` and strict toggles, slice `max_nodes` — with recommended caps for hostile input and where caller-side parser timeouts belong. |

## Honest project status

`libcpg` is at version `0.1.1`; several surfaces are intentionally minimal or
aspirational. This pillar documents them plainly rather than implying more
maturity than exists. The most consequential items:

| Aspect | Reality | Detailed in |
| --- | --- | --- |
| Default build | `default = []`; `build` fails without a `lang-*` feature | [01](01-build-and-features.md) |
| `examples/` directory | present but **empty** — no runnable examples (`tests/` now holds the integration and robustness suites) | [02](02-testing.md) |
| Benchmarks | `criterion` is a dev-dependency but both `[[bench]]` targets are **commented out** | [02](02-testing.md), [03](03-performance.md) |
| `rayon`, `ahash`, `string_cache`, `regex` | declared in `Cargo.toml` but **not yet wired** into `src/` (`proptest` is now used throughout the test suite) | [02](02-testing.md), [03](03-performance.md) |
| `gpu` feature | **reserved** — enables `wgpu`/`pollster` but no GPU code is written | [01](01-build-and-features.md) |
| Detection results | pattern/algorithm/complexity outputs are **heuristics**, not proofs | [`security/00`](../security/00-threat-model.md) |

None of these gaps affect the correctness of the features that *are* implemented —
CPG construction, CFG/DFG extraction, PDG slicing, VF2 matching, and (behind their
features) Gang-of-Four detection, algorithm heuristics, and GNN embeddings — each
of which is exercised by the inline test suite catalogued in
[testing](02-testing.md).

## Where to go next

- New to CPGs? Start with [`theory/00-overview.md`](../theory/00-overview.md) and
  the [glossary](../GLOSSARY.md).
- Want to build something today? Enable a grammar per
  [build and features](01-build-and-features.md), then follow
  [`usage/00-getting-started.md`](../usage/00-getting-started.md).
- Integrating Rholang/MeTTa? See
  [`usage/06-f1r3fly-rholang-metta.md`](../usage/06-f1r3fly-rholang-metta.md) and
  the Mode-B decision record,
  [`design/0002-mode-b-build-from-tree.md`](../design/0002-mode-b-build-from-tree.md).
