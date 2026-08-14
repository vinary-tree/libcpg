# Usage 14 — Query may-happen-in-parallel sites

`may_happen_in_parallel` is feature-free and returns a deterministic, symmetric
relation over execution-site `NodeId` values. Frontends identify parallel
composition by attaching `MsgKind::Spawn` to a `CpgNodeKind::Block`; the
analyzer never parses source.

## 1. Query a relation

```rust
use libcpg::{may_happen_in_parallel, CodePropertyGraph, NodeId};

fn inspect_parallelism(cpg: &CodePropertyGraph, left: NodeId, right: NodeId) {
    let relation = may_happen_in_parallel(cpg);
    if relation.capped {
        eprintln!("MHP output reached its safety cap; absence is inconclusive");
    }
    if relation.contains(left, right) {
        println!("{left:?} and {right:?} may happen in parallel");
    }
    for partner in relation.partners(left) {
        println!("stable partner: {partner:?}");
    }
}
```

`pairs` stores each unordered relation member once with `first < second`.
`contains(left, right)` is symmetric, and `partners(node)` yields stable
ascending identifiers.

## 2. Supply typed parallel structure

A semantic adapter can mark an already constructed block:

```rust
use libcpg::{CpgNode, CpgNodeKind, MsgKind, NodeId, ScopeId, SourceRange};

let parallel = CpgNode::new(
    NodeId::new(10),
    CpgNodeKind::Block {
        scope: ScopeId::GLOBAL,
    },
    SourceRange::from_bytes(0, 20),
)
.with_message_kind(MsgKind::Spawn);
```

Connect each operand root as a direct `AstChild` of this node. Operand roots may
be direct process nodes or wrapper blocks. Execution sites in different roots
are related; two sites in one root are not. Do not synthesize source-based
markers in the analyzer.

## 3. Rholang Mode B

With the `rholang` feature, `TreeSitterCpgBuilder::build_from_tree` maps a real
`par` syntax node to the typed block automatically:

```rust
use libcpg::{may_happen_in_parallel, Language, TreeSitterCpgBuilder};

fn rholang_mhp(
    tree: &tree_sitter::Tree,
    source: &str,
) -> libcpg::Result<libcpg::MhpRelation> {
    let cpg = TreeSitterCpgBuilder::new()
        .build_from_tree(tree, source, Language::Rholang)?;
    Ok(may_happen_in_parallel(&cpg))
}
```

For `left!(1) | right!?(2)`, the parallel block can own both send `Call` nodes
directly. This is intentional; consumers should depend on direct-operand
semantics, not an assumed wrapper layout.

## 4. Interpret results safely

- A returned pair means structural concurrency is possible, not inevitable.
- MHP alone says nothing about shared storage or conflicting read/write access.
- If `capped` is true, every returned pair is valid but missing pairs are
  inconclusive.
- Nested function bodies are excluded until a future call-aware model provides
  evidence that they execute in the operand.
- Missing frontend metadata yields no invented relation.
- The source CPG is never changed.

See [Theory 31](../theory/31-may-happen-in-parallel.md),
[ADR-0038](../design/0038-typed-ast-mhp.md), the
[component guide](../components/graph/mhp-analysis.md), and the
[security boundary](../security/06-mhp-analysis.md).
