# libcpg

A Rust library for constructing and analyzing **Code Property Graphs** (CPGs).
A CPG (Yamaguchi et al. 2014) merges three complementary views of a program —
the **Abstract Syntax Tree** (AST), the **Control Flow Graph** (CFG), and the
**Data Flow Graph** (DFG) — into one unified `petgraph`-backed graph, and adds
the **Program Dependence Graph** (PDG) on top for program slicing.

## Capabilities

- **CPG construction** from source via tree-sitter (`TreeSitterCpgBuilder`),
  either parsing internally (`build`) or from a caller-supplied parse tree
  (`build_from_tree`, so a host that already parsed a file can avoid a second
  parse and reuse its own grammar).
- **CFG extraction** — structural control-flow edges for
  block/if/while/for/loop/match/return/break/continue/try/throw/call.
- **DFG extraction** — intraprocedural reaching-definitions (Kildall 1973) and
  def-use chains.
- **PDG + program slicing** — control-dependence edges via the reverse
  dominance frontier (Ferrante–Ottenstein–Warren 1987; Cytron et al. 1991) plus
  data-dependence edges, with backward/forward Weiser slices (`PdgBuilder`,
  `backward_slice`, `forward_slice`).
- **Subgraph isomorphism** — VF2 pattern matching (`Vf2Matcher`).
- **Design-pattern detection** (GoF) and **complexity estimation**, behind the
  `design-patterns` and `algorithm-detection` features.
- Optional **GNN** embeddings (`gnn`) and **serde** serialization (`serde`).

## Quick start

```rust
use libcpg::{TreeSitterCpgBuilder, CpgBuilder, PdgBuilder, backward_slice, Language};

let builder = TreeSitterCpgBuilder::new();
let source = "fn f(x: i32) -> i32 { let y = x + 1; if y > 0 { y } else { 0 } }";
let mut cpg = builder.build(source, Language::Rust)?;

// Add Program Dependence Graph edges for the first function, then slice.
let func = cpg.functions().map(|n| n.id).next().unwrap();
PdgBuilder::new().build(&mut cpg, func);
let slice = backward_slice(&cpg, func, 256);
println!("{} nodes in the backward slice", slice.len());
# Ok::<(), libcpg::Error>(())
```

## Features

| Feature | Description |
|---|---|
| `default` | Core CPG construction, CFG/DFG/PDG, VF2, slicing |
| `lang-rust`, `lang-python`, `lang-javascript`, … | tree-sitter grammars |
| `design-patterns` | Gang-of-Four pattern detection |
| `algorithm-detection` | Algorithm-family recognition + complexity |
| `serde` | Serialization / deserialization |
| `gnn` | Graph neural network embeddings |

Language support is feature-gated per grammar (`lang-rust`, `lang-python`,
`lang-javascript`, `lang-typescript`, `lang-go`, `lang-java`, `lang-c`,
`lang-cpp`, `lang-ruby`, plus config/markup grammars). `rholang` and `metta`
feature flags are reserved for F1R3FLY.io process-calculus support (planned).

## Documentation

Extensive design, architecture, and API docs live under [`docs/`](docs/README.md)
— graph model, CFG/DFG construction pipeline, VF2 matching, GoF catalog, and
complexity estimation.

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option.
