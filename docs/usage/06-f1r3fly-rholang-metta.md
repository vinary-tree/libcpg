# F1R3FLY.io: Rholang and MeTTa

libcpg has **working** CPG node-mappers for two [F1R3FLY.io](https://f1r3fly.io) languages: [Rholang](../GLOSSARY.md#rholang), the concurrent process-calculus language of the RChain/F1R3FLY ecosystem, and [MeTTa](../GLOSSARY.md#metta), the symbolic rewriting language of the Hyperon stack. These are not planned or stub mappers — they are implemented, tested, and produce full AST/CFG/DFG overlays. But they are reached differently from the 16 built-in languages, and this guide shows exactly how.

The one structural fact to internalise:

> Rholang and MeTTa have **no grammar registered** in libcpg's [`ParserRegistry`](../architecture/language-frontends.md). `TreeSitterCpgBuilder::build(source, Language::Rholang)` will therefore always fail with `UnsupportedLanguage`. The **only** construction path is [Mode B](../GLOSSARY.md#mode-b--build_from_tree): you parse with a grammar you own and call `build_from_tree`.

![Sequence: a Mode-B caller parses Rholang/MeTTa with its own grammar and hands the tree to build_from_tree, which runs the same AST/CFG/DFG pipeline.](../diagrams/mode-b-sequence.svg)

*Figure — the Mode-B call sequence used for Rholang and MeTTa. Source: [`diagrams/mode-b-sequence.puml`](../diagrams/mode-b-sequence.puml).*

---

## Why Mode B, and why it is (almost) feature-free

The `rholang` and `metta` Cargo features are **empty feature lists** — pure logical `cfg` toggles:

```toml
# From libcpg's Cargo.toml
rholang = []
metta = []
```

They gate only the `map_rholang` / `map_metta` arms inside the node-mapper. Those arms reference **only** tree-sitter node-kind *strings* and `tree_sitter::Node` navigation — never a symbol from a grammar crate. So enabling `rholang` or `metta` pulls in **no dependency at all**; it just switches on mapping logic that is already there. The ρ-calculus / S-expression grammar itself is supplied by the **caller**, who parses and hands over an already-built `tree_sitter::Tree`.

This is a deliberate decision (recorded in [ADR-0002](../design/0002-mode-b-build-from-tree.md)) with a concrete payoff: a host such as pgmcp already links the Rholang and MeTTa grammars. If libcpg *also* vendored them, the shared `tree_sitter_<lang>` C symbol would be **duplicated at link time**. Keeping the grammars out of libcpg's dependency graph — they are only *test-only* dev-dependencies, which Cargo never propagates downstream — makes libcpg a clean guest in any host that already speaks these languages.

The soundness posture of the mappings follows the standard multi-language-CPG discipline of Yamaguchi et al. [[1]](#references): each process-calculus or S-expression construct is normalised onto the nearest imperative CPG anchor that keeps the CFG/DFG **sound** (never asserting an edge the semantics forbid), even where it is necessarily *incomplete*.

---

## Prerequisites

Two things must be in place:

1. **Enable the mapper.** Turn on the `rholang` and/or `metta` feature on libcpg. Without it, `Language::Rholang`/`MeTTa` fall through to the generic mapper and every node becomes `Unknown` — you get a tree of the right shape but no meaningful kinds.
2. **Provide a grammar.** Add a tree-sitter grammar crate for the language to *your* `Cargo.toml`. libcpg's own tests use `rholang-tree-sitter` (exposed as `tree_sitter_rholang`) and a MeTTa grammar shim (`vendor/tree-sitter-metta`, exposed as `tree_sitter_metta`) that recompiles the canonical MeTTa grammar against `tree-sitter 0.26`.

```toml
[dependencies]
libcpg = { version = "0.1", features = ["rholang", "metta"] }

# The grammars you parse with — your dependency, not libcpg's.
tree-sitter = "0.26"
tree-sitter-rholang = { package = "rholang-tree-sitter", git = "..." } # or a path
tree-sitter-metta = { git = "..." }                                    # or a path
```

Whichever grammars you use **must be compatible with the same `tree-sitter` version libcpg links** (0.26) and should expose the modern [`tree-sitter-language`](https://docs.rs/tree-sitter-language) `LanguageFn` handle:

- Rholang: `rholang-tree-sitter` exposes `tree_sitter_rholang::LANGUAGE` (a `LanguageFn`) — call `.into()`.
- MeTTa: use a `tree-sitter 0.26`-compatible MeTTa grammar exposing `tree_sitter_metta::LANGUAGE` (a `LanguageFn`) — call `.into()`. libcpg's own tests link the shim in `vendor/tree-sitter-metta`, which recompiles the canonical MeTTa C grammar against `tree-sitter-language`. (The older crates.io `tree-sitter-metta`, which exposes a `language()` function, pins `tree-sitter 0.25` and will **not** link alongside libcpg 0.26.)

---

## Rholang

### Building a Rholang CPG

```rust
// requires: libcpg features = ["rholang"]; plus your Rholang grammar crate
use libcpg::{Language, TreeSitterCpgBuilder};

fn main() -> Result<(), libcpg::Error> {
    let source = "new stdout(`rho:io:stdout`) in {\n  \
                  contract @\"greet\"(@name) = {\n    \
                  stdout!(\"hello, \" ++ *name)\n  }\n}\n";

    // 1. Parse with the caller-owned Rholang grammar.
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rholang::LANGUAGE.into())
        .expect("load the Rholang grammar");
    let tree = parser.parse(source, None).expect("parse Rholang");

    // 2. Build the CPG in Mode B — no lang-* feature, no registered grammar.
    let cpg = TreeSitterCpgBuilder::new().build_from_tree(&tree, source, Language::Rholang)?;

    // The `contract` mapped to a Function (seeding a CFG entry) and the
    // `stdout!(…)` send mapped to a Call.
    assert_eq!(cpg.functions().count(), 1);
    assert!(!cpg.cfg_entries().is_empty());
    Ok(())
}
```

### How Rholang maps to CPG vocabulary

The mapper (`map_rholang`) normalises ρ-calculus onto the CPG's imperative vocabulary so the existing CFG and DFG builders work unchanged. The load-bearing choices:

| Rholang construct | CPG node | Why |
|-------------------|----------|-----|
| `contract`, `constructor_decl`, `method_decl`, `default_decl` | `Function` | Seeds a CFG entry; its trailing `block` is the body |
| `agent_block` | `Class` | An object-like grouping of a constructor + methods |
| `new … in`, `let … in`, `for(…){…}`, `{…}`, `\|` (par), `bundle` | `Block` | Each introduces a process region / scope |
| `x!(v)` send (`send`, `send_sync`) | `Call` | The load-bearing send site |
| `x!m(v)` / method send (`send_method`, `method`) | `Call` (method) | Method-style dispatch |
| receive binds (`linear_bind`, `repeated_bind`, `peek_bind`) | `Call` | Consumes on the source channel |
| `ifElse` | `If`; `match`, `choice` (select) | `Match` | Scrutinee dispatch / non-deterministic choice |
| a **bound** name (`new`-restricted, receive-bound, contract formal, `let` binder) | `Variable` / `Parameter` | A DFG **definition** |
| a **referenced** name | `Identifier` | A DFG **use** |
| a `` `rho:…` `` URI decl (e.g. `new stdout(`rho:io:stdout`)`) | `Import` | The polyglot system-process anchor (backticks stripped) |
| `*x` (eval), `@P` (quote) | `UnaryOp` | Dereference / reify |

The def-versus-use distinction (`classify_rholang_var`) is what makes the Rholang DFG **sound**: a name in a binder position becomes a definition, a name mentioned elsewhere becomes a use, and reaching-definitions links them.

![Rholang constructs mapped onto CPG node and edge kinds.](../diagrams/rholang-mapping.svg)

*Figure — the Rholang → CPG node/edge mapping. Source: [`diagrams/rholang-mapping.dot`](../diagrams/rholang-mapping.dot).*

### Data-flow soundness in practice

Because a `contract` is a `Function`, the DFG runs inside it, and a channel bound by `new` links to its use as a send channel. This snippet builds a contract whose body binds `c` and sends on it, then confirms a `DefUse` edge formed:

```rust
// requires: libcpg features = ["rholang"]; plus your Rholang grammar crate
use libcpg::{CpgEdgeKind, DfgEdgeKind, Language, TreeSitterCpgBuilder};

fn main() -> Result<(), libcpg::Error> {
    let source = "contract @\"main\"() = { new c in { c!(1) } }\n";

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rholang::LANGUAGE.into())
        .expect("load the Rholang grammar");
    let tree = parser.parse(source, None).expect("parse Rholang");

    let cpg = TreeSitterCpgBuilder::new().build_from_tree(&tree, source, Language::Rholang)?;

    // `c` was defined by `new` and used as the send channel: a def-use edge exists.
    let has_defuse = cpg
        .edges_by_kind(|k| matches!(k, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse)))
        .next()
        .is_some();
    assert!(has_defuse);
    Ok(())
}
```

---

## MeTTa

### Building a MeTTa CPG

```rust
// requires: libcpg features = ["metta"]; plus your MeTTa grammar crate
use libcpg::{CpgEdgeKind, DfgEdgeKind, Language, TreeSitterCpgBuilder};

fn main() -> Result<(), libcpg::Error> {
    let source = "(= (double $x) (* $x 2))\n";

    // 1. Parse with the caller-owned MeTTa grammar (a `tree-sitter 0.26`-compatible
    //    grammar exposing `LANGUAGE`, e.g. libcpg's `vendor/tree-sitter-metta` shim).
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_metta::LANGUAGE.into())
        .expect("load the MeTTa grammar");
    let tree = parser.parse(source, None).expect("parse MeTTa");

    // 2. Build in Mode B.
    let cpg = TreeSitterCpgBuilder::new().build_from_tree(&tree, source, Language::MeTTa)?;

    // The `(= …)` rule became a Function named "double" (a CFG entry),
    // and the LHS `$x` (a Parameter) links to the RHS `$x` (a use).
    assert_eq!(cpg.functions().count(), 1);
    assert!(!cpg.cfg_entries().is_empty());
    let has_defuse = cpg
        .edges_by_kind(|k| matches!(k, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse)))
        .next()
        .is_some();
    assert!(has_defuse);
    Ok(())
}
```

### How MeTTa maps to CPG vocabulary

MeTTa is a minimal [S-expression](../GLOSSARY.md#s-expression) grammar: a form like `(= (f $x) body)` is not a distinct grammar node but a `list` whose **head atom** carries the meaning. `map_metta` therefore dispatches on each list's head:

| MeTTa form (by head) | CPG node | Why |
|----------------------|----------|-----|
| `(= LHS RHS)`, `(:= LHS RHS)` | `Function` | A rule definition; name from the rule-head atom, body = RHS = last child (CFG entry) |
| `(: name Type)`, `(-> A B R)` | `TypeAnnotation` | Type annotation / function-type |
| `(if …)` | `If`; `(case …)`, `(match …)` | `Match` | Built-in control forms |
| `(let …)`, `(let* …)` | `Block` | Binding scope |
| `(import! &space module)` | `Import` | Module import (path = last named child) |
| `(+ $a $b)`, general application | `Call` | Grounded-operator / function application |
| `$x` in a rule LHS (`(= (foo $x) …)`) | `Parameter` | A DFG **definition** (the binder) |
| `$x` elsewhere (RHS, non-rule) | `Identifier` | A DFG **use** |
| bare atom, `&self`, `%Undefined%`, literals | `Identifier` / `Literal` | Leaf atoms |

The `$x` classifier (`classify_metta_var`) walks the parent chain to distinguish a rule-LHS binder (definition) from every other occurrence (use), so `(= (double $x) (* $x 2))` yields a `$x` def → use edge — exactly the assertion in the snippet above.

![MeTTa S-expression forms mapped onto CPG node and edge kinds by head-atom dispatch.](../diagrams/metta-mapping.svg)

*Figure — the MeTTa → CPG node/edge mapping. Source: [`diagrams/metta-mapping.dot`](../diagrams/metta-mapping.dot).*

---

## Caveats and honesty

- **The feature is mandatory for meaning.** Build with `Language::Rholang`/`MeTTa` but **without** the `rholang`/`metta` feature and the generic mapper produces `Unknown` for every node — structurally a tree, semantically blank. Always enable the toggle.
- **Grammar version.** The mapper keys on the node-kind strings of a specific grammar (the F1R3FLY.io `rholang-tree-sitter` and `tree-sitter-metta` grammars). A materially different grammar fork may emit different kind strings, in which case unmatched nodes fall through to `Unknown` — sound, but coarse.
- **`build_from_tree` skips the size check.** Mode B does not apply `max_file_size` (the caller already owns the tree), so bound untrusted input yourself before parsing — see [Input and Resource Hardening](../security/01-input-and-resource-hardening.md).
- **Mappings are sound, not complete.** They never assert an edge the semantics forbid, but a process calculus's concurrency (e.g. `par` interleaving) is not fully modelled. Treat the CPG as a faithful-but-partial view.

---

## Next steps

- The per-construct mapping rules in full, including `classify_rholang_var`/`classify_metta_var`, are in [Node Mapper](../components/builder/node-mapper.md).
- The Mode-B decision and the duplicate-symbol reasoning are in [ADR-0002](../design/0002-mode-b-build-from-tree.md).
- Once built, a Rholang/MeTTa CPG is an ordinary `CodePropertyGraph`: query it ([Querying and Traversal](02-querying-and-traversal.md)), slice it ([Program Slicing](04-program-slicing.md)), or detect patterns over it ([Pattern Detection](03-pattern-detection.md)).

---

## References

1. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and Discovering Vulnerabilities with Code Property Graphs.* 2014 IEEE Symposium on Security and Privacy. DOI: [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44)
</content>
