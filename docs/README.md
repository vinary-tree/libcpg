# libcpg documentation

**libcpg** is a Rust library for constructing and analyzing **Code Property
Graphs** (CPGs): a single `petgraph`-backed directed graph that overlays the
Abstract Syntax Tree (AST), Control Flow Graph (CFG), Data Flow Graph (DFG),
and — on demand — the Program Dependence Graph (PDG) on **one shared node set**.

## What is a Code Property Graph?

A CPG (Yamaguchi et al. 2014) represents source code as a graph that merges
complementary program views so a single query can reason over syntax, control
flow, and data flow at once:

- **AST** — syntactic structure (how the code parses);
- **CFG** — execution order between nodes;
- **DFG** — how values flow from definitions to uses;
- **PDG** — control- and data-dependence, built on demand for program slicing.

![A Code Property Graph overlays AST, control-flow, and data-flow edges on one shared node set](diagrams/cpg-overlay.svg)

*Figure — the unified CPG for a small function. Source: [`diagrams/cpg-overlay.dot`](diagrams/cpg-overlay.dot).*

Combining these views enables, for example: **taint / vulnerability queries**
(untrusted data reaching a sensitive sink), **design-pattern recognition**
(subgraph matching), **exact SCC decomposition** (loops and recursion),
**algorithm detection** (from control-flow structure),
**code-clone detection** (graph similarity), and **complexity analysis**.

## Quick start

```rust
// requires: features = ["lang-rust"]
use libcpg::{TreeSitterCpgBuilder, CpgBuilder, Language};

let builder = TreeSitterCpgBuilder::new();
let source = r#"
    fn factorial(n: u64) -> u64 {
        if n <= 1 { 1 } else { n * factorial(n - 1) }
    }
"#;
let cpg = builder.build(source, Language::Rust)?;

println!("nodes: {}, edges: {}", cpg.node_count(), cpg.edge_count());
for func in cpg.functions() {
    println!("function: {}", func.name().unwrap_or("<anonymous>"));
}
# Ok::<(), libcpg::Error>(())
```

With `default = []` no grammars are compiled in — enable a `lang-*` feature (as
above) or build from a caller-supplied tree with `build_from_tree` (Mode B).

### Detecting design patterns

```rust
// requires: features = ["lang-rust", "design-patterns"]
use libcpg::patterns::design::{GofPatternDetector, PatternDetector};

let detector = GofPatternDetector::new();
for m in detector.detect(&cpg) {
    println!("{} (confidence {:.2})", m.pattern_name, m.confidence);
}
```

### Program slicing

```rust
// requires: features = ["lang-rust"]
use libcpg::{TreeSitterCpgBuilder, CpgBuilder, PdgBuilder, backward_slice, Language};

let mut cpg = TreeSitterCpgBuilder::new().build(source, Language::Rust)?;
let func = cpg.functions().map(|n| n.id).next().expect("a function node");
PdgBuilder::new().build(&mut cpg, func);            // add PDG edges
let slice = backward_slice(&cpg, func, 256);         // bounded backward slice
# Ok::<(), libcpg::Error>(())
```

## Documentation map

### Theory — the foundations and the "why"
[Overview](theory/00-overview.md) · [Code property graphs](theory/01-code-property-graphs.md) · [Control flow & complexity](theory/02-control-flow-and-complexity.md) · [Data flow & reaching definitions](theory/03-data-flow-and-reaching-definitions.md) · [Program dependence & slicing](theory/04-program-dependence-and-slicing.md) · [Subgraph isomorphism (VF2)](theory/05-subgraph-isomorphism-vf2.md) · [Graph similarity](theory/06-graph-similarity.md) · [Design-pattern detection](theory/07-design-pattern-detection.md) · [Algorithm & complexity analysis](theory/08-algorithm-and-complexity-analysis.md) · [Graph neural networks](theory/09-graph-neural-networks.md)

### Architecture — the "what / where"
[Overview](architecture/overview.md) · [Graph data model](architecture/graph-data-model.md) · [Construction & analysis data flow](architecture/data-flow.md) · [Language frontends](architecture/language-frontends.md)

### Design decisions (ADRs)
[Overview](design/00-overview.md) · [0001 Unified overlay graph](design/0001-unified-overlay-graph.md) · [0002 Mode B: build_from_tree](design/0002-mode-b-build-from-tree.md) · [0003 AST-ordered reaching definitions](design/0003-ast-ordered-reaching-defs.md) · [0004 Relaxed VF2 detection](design/0004-relaxed-vf2-detection.md) · [0005 Feature-flag taxonomy](design/0005-feature-flag-taxonomy.md)

### API reference
[Graph / SCC reference](api/graph-reference.md) · [Builder reference](api/builder-reference.md) · [Pattern / algorithm / GNN reference](api/pattern-reference.md)

### Components

#### Graph core
[Overview](components/graph/overview.md) · [Nodes](components/graph/nodes.md) · [Edges](components/graph/edges.md) · [Traversal](components/graph/traversal.md) · [Strongly-connected components](components/graph/scc-analysis.md)

#### Builder
[Overview](components/builder/overview.md) · [CFG extraction](components/builder/cfg.md) · [DFG extraction](components/builder/dfg.md) · [PDG & slicing](components/builder/pdg-and-slicing.md) · [Node mapper](components/builder/node-mapper.md)

#### Pattern detection
[Overview](components/patterns/overview.md) · [VF2 matching & similarity](components/patterns/vf2-matching.md) · [Gang of Four](components/patterns/gang-of-four.md) · [DPML](components/patterns/dpml.md) · [Classification](components/patterns/classification.md)

#### Algorithm detection
[Overview](components/algorithms/overview.md) · [Families](components/algorithms/families.md) · [Complexity](components/algorithms/complexity.md)

#### Graph neural networks
[Overview](components/gnn/overview.md) · [Message passing](components/gnn/message-passing.md) · [Embeddings](components/gnn/embeddings.md)

### Usage guides
[Getting started](usage/00-getting-started.md) · [Building CPGs](usage/01-building-cpgs.md) · [Querying & traversal](usage/02-querying-and-traversal.md) · [Pattern detection](usage/03-pattern-detection.md) · [Program slicing](usage/04-program-slicing.md) · [Serialization](usage/05-serialization.md) · [F1R3FLY: Rholang & MeTTa](usage/06-f1r3fly-rholang-metta.md)

### Engineering
[Overview](engineering/00-overview.md) · [Build & features](engineering/01-build-and-features.md) · [Testing](engineering/02-testing.md) · [Performance](engineering/03-performance.md) · [Contributing](engineering/04-contributing.md)

### Scientific validation
[Overview](scientific/00-overview.md) · [CPG invariants & equivalence](scientific/01-cpg-invariants-and-equivalence.md) · [Reaching-defs validation](scientific/02-reaching-defs-validation.md) · [VF2 completeness](scientific/03-vf2-completeness.md) · [Measurement methodology](scientific/04-measurement-methodology.md)

### Security
[Threat model](security/00-threat-model.md) · [Input & resource hardening](security/01-input-and-resource-hardening.md)

### Reference
[Glossary](GLOSSARY.md) · [Diagram catalog](diagrams/README.md)

## Features

`default = []` — nothing is enabled by default; opt in to what you need.

| Feature | Enables |
|---|---|
| `lang-rust` … `lang-ruby` (16 grammars) | the tree-sitter grammar for that language (enables internal `build`) |
| `lang-systems` / `lang-scripting` / `lang-web` / `lang-config` / `lang-all` | grammar groups |
| `design-patterns` | Gang-of-Four detection (`patterns::`) |
| `algorithm-detection` | algorithm-family recognition + complexity (`algorithms::`) |
| `serde` | `Serialize` / `Deserialize` derives |
| `gnn` | graph-neural-network embeddings (`gnn::CpgGnn`) |
| `ml-linfa` / `ml-rules` | ML- / rule-based pattern classification |
| `rholang` / `metta` | the Rholang / MeTTa Mode-B node mappers |
| `full` | `gnn + design-patterns + algorithm-detection + serde + ml-rules + lang-all` |

*(`gpu` is reserved and wires no code yet.)* Full detail:
[engineering/build-and-features](engineering/01-build-and-features.md).

## Supported languages

| Path | Languages | How |
|---|---|---|
| Internal parse (`build`) | Rust, Python, JavaScript, TypeScript, Go, Java, C, C++, JSON, HTML, CSS, Bash, TOML, YAML, Markdown, Ruby | enable the matching `lang-*` feature |
| Mode B (`build_from_tree`) | Rholang, MeTTa (F1R3FLY.io) | caller supplies a parse tree; features `rholang` / `metta` enable the mappers |

`TreeSitterCpgBuilder::supported_languages()` lists the 16 built-ins statically;
actual availability for a given build is `ParserRegistry::supports(lang)`.

## Documentation conventions

- **Accuracy** — every type, method, and snippet is written against the real
  API and annotated with the feature flag it needs.
- **Math** — expressions use GitHub math spans (inline ``$`…`$``, display
  ` ```math ` blocks), never bare `$…$` or unicode literals.
- **Diagrams** — every figure is a committed PlantUML/Graphviz source rendered
  to a committed SVG; see the [diagram catalog](diagrams/README.md).
- **Citations** — claims from the literature link to DOIs where they exist; each
  substantive page ends with a *References* section. Terms are defined once in
  the [glossary](GLOSSARY.md).

## Related projects

Part of the [F1R3FLY.io](https://f1r3fly.io) ecosystem; libcpg's Rholang and
MeTTa frontends integrate with F1R3FLY tooling (e.g. the pgmcp code index, which
drives CPG construction in Mode B).

## License

Licensed under either of Apache License 2.0 or MIT, at your option.
