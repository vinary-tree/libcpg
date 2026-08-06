# libcpg

A Rust library for constructing and analyzing **Code Property Graphs** (CPGs).
A CPG (Yamaguchi et al. 2014) merges complementary views of a program — the
**Abstract Syntax Tree** (AST), the **Control Flow Graph** (CFG), and the
**Data Flow Graph** (DFG) — into one unified `petgraph`-backed graph, and adds
the **Program Dependence Graph** (PDG) on top for program slicing. On this
substrate it offers subgraph-isomorphism pattern matching, Gang-of-Four design
pattern detection, algorithm/complexity analysis, and graph-neural-network
embeddings.

## Architecture at a glance

The defining idea: **one shared node set carries several typed edge overlays** —
the same nodes are simultaneously an AST, a CFG, and a DFG (and, on demand, a
PDG). A query can therefore mix syntax, control flow, and data flow freely.

![A Code Property Graph: one shared node set with AST, control-flow, and data-flow edge overlays](docs/diagrams/cpg-overlay.svg)

*Figure — the unified CPG for a small function. Source: [`docs/diagrams/cpg-overlay.dot`](docs/diagrams/cpg-overlay.dot).*

## Capabilities

- **CPG construction** from source via tree-sitter (`TreeSitterCpgBuilder`),
  either parsing internally (`build`, requires the matching `lang-*` feature) or
  from a caller-supplied parse tree (`build_from_tree` — "Mode B" — so a host
  that already parsed a file can reuse its own grammar and avoid a second parse).
- **CFG extraction** — structural control-flow edges (14 `CfgEdgeKind` variants)
  for block/if/while/for/loop/match/return/break/continue/try/throw/call.
- **Strongly-connected components** — exact, deterministic decomposition of
  per-function CFGs and the resolved whole-CPG call graph, including
  loop/recursion classification and the condensation DAG.
- **DFG extraction** — intraprocedural, AST-ordered reaching definitions
  (Kildall 1973) and def-use chains (13 `DfgEdgeKind` variants).
- **PDG + program slicing** — control-dependence edges via the reverse dominance
  frontier (Ferrante–Ottenstein–Warren 1987; Cytron et al. 1991) plus
  data-dependence edges, with bounded backward/forward Weiser slices
  (`PdgBuilder`, `backward_slice`, `forward_slice`).
- **Subgraph isomorphism** — VF2 pattern matching (`pattern::Vf2Matcher`) and
  graph similarity (Jaccard, cosine, Weisfeiler-Lehman, graph-edit).
- **Design-pattern detection** (23 Gang-of-Four patterns) and **algorithm /
  complexity analysis**, behind the `design-patterns` and `algorithm-detection`
  features.
- Optional **GNN** embeddings (`gnn`) and **serde** serialization (`serde`).
- **F1R3FLY.io languages** — **Rholang** and **MeTTa** are supported through
  Mode B (`build_from_tree`); their node mappers are implemented (features
  `rholang` / `metta`).

## Quick start

```rust
// requires: features = ["lang-rust"]
use libcpg::{TreeSitterCpgBuilder, CpgBuilder, PdgBuilder, backward_slice, Language};

let builder = TreeSitterCpgBuilder::new();
let source = "fn f(x: i32) -> i32 { let y = x + 1; if y > 0 { y } else { 0 } }";
let mut cpg = builder.build(source, Language::Rust)?;

// Add Program Dependence Graph edges for the first function, then slice.
let func = cpg.functions().map(|n| n.id).next().expect("a function node");
PdgBuilder::new().build(&mut cpg, func);
let slice = backward_slice(&cpg, func, 256);
println!("{} nodes in the backward slice", slice.len());
# Ok::<(), libcpg::Error>(())
```

With the **default** feature set (`default = []`) no grammars are compiled in,
so `build` returns `Error::UnsupportedLanguage`; enable a `lang-*` feature (as
above) or use the feature-free `build_from_tree` path. See
[`docs/usage/00-getting-started.md`](docs/usage/00-getting-started.md).

## Features

`default = []` — nothing is enabled by default. Opt in to exactly what you need.

| Feature | Enables |
|---|---|
| `lang-rust`, `lang-python`, `lang-javascript`, `lang-typescript`, `lang-go`, `lang-java`, `lang-c`, `lang-cpp`, `lang-json`, `lang-html`, `lang-css`, `lang-bash`, `lang-toml`, `lang-yaml`, `lang-markdown`, `lang-ruby` | the tree-sitter grammar for that language (each enables internal `build`) |
| `lang-systems` / `lang-scripting` / `lang-web` / `lang-config` / `lang-all` | grammar groups |
| `design-patterns` | Gang-of-Four detection (`patterns::`) |
| `algorithm-detection` | algorithm-family recognition + complexity (`algorithms::`) |
| `serde` | `Serialize` / `Deserialize` derives |
| `gnn` | graph-neural-network embeddings (`gnn::CpgGnn`) |
| `ml-linfa` / `ml-rules` | ML- / rule-based pattern classification |
| `rholang` / `metta` | the Rholang / MeTTa Mode-B node mappers |
| `full` | `gnn + design-patterns + algorithm-detection + serde + ml-rules + lang-all` |

*(`gpu` is reserved for future work and wires no code yet.)*

## Supported languages

Sixteen languages are parsed internally via feature-gated tree-sitter grammars:
Rust, Python, JavaScript, TypeScript, Go, Java, C, C++, JSON, HTML, CSS, Bash,
TOML, YAML, Markdown, Ruby. **Rholang** and **MeTTa** (F1R3FLY.io) are built
through Mode B (`build_from_tree`) with a caller-supplied grammar; see
[`docs/usage/06-f1r3fly-rholang-metta.md`](docs/usage/06-f1r3fly-rholang-metta.md).

## Documentation

Comprehensive documentation lives under [`docs/`](docs/README.md):

- **[Theory](docs/theory/00-overview.md)** — the CPG model, control/data flow, program dependence & slicing, subgraph isomorphism, similarity, pattern detection, complexity, and GNNs, with proofs, math, and citations.
- **[Architecture](docs/architecture/overview.md)** — module map, the graph data model, construction & analysis pipelines, and language frontends.
- **[Design decisions](docs/design/00-overview.md)** — ADR-style records (unified overlay, Mode B, AST-ordered reaching defs, relaxed VF2, feature taxonomy).
- **[API reference](docs/api/graph-reference.md)** — graph/SCC, builder, and pattern/algorithm/GNN reference.
- **[Components](docs/README.md#components)** — graph and SCC analysis, builder, patterns, algorithms, and GNN internals.
- **[Usage guides](docs/usage/00-getting-started.md)** — task-oriented how-tos.
- **[Engineering](docs/engineering/00-overview.md)** · **[Scientific validation](docs/scientific/00-overview.md)** · **[Security](docs/security/00-threat-model.md)**.
- **[Glossary](docs/GLOSSARY.md)** and the **[diagram catalog](docs/diagrams/README.md)**.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.

## References

- Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and Discovering Vulnerabilities with Code Property Graphs.* IEEE S&P. DOI: [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44)
- Ferrante, J., Ottenstein, K. J., Warren, J. D. (1987). *The Program Dependence Graph and Its Use in Optimization.* ACM TOPLAS. DOI: [10.1145/24039.24041](https://doi.org/10.1145/24039.24041)
- Cytron, R., et al. (1991). *Efficiently Computing Static Single Assignment Form and the Control Dependence Graph.* ACM TOPLAS. DOI: [10.1145/115372.115320](https://doi.org/10.1145/115372.115320)
- Weiser, M. (1984). *Program Slicing.* IEEE TSE. DOI: [10.1109/TSE.1984.5010248](https://doi.org/10.1109/TSE.1984.5010248)
- Cordella, L. P., et al. (2004). *A (Sub)graph Isomorphism Algorithm for Matching Large Graphs.* IEEE TPAMI. DOI: [10.1109/TPAMI.2004.75](https://doi.org/10.1109/TPAMI.2004.75)
- Kildall, G. A. (1973). *A Unified Approach to Global Program Optimization.* POPL. DOI: [10.1145/512927.512945](https://doi.org/10.1145/512927.512945)
