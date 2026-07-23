# CPG invariants and the build ≡ build_from_tree equivalence

A [Code Property Graph](../GLOSSARY.md#code-property-graph-cpg) is only useful if it is *well-formed*: every overlay must index the same nodes, the tree structure must be navigable both ways, and re-deriving an overlay must not corrupt the graph. This page states those structural invariants precisely, grounds each in a real inline test, and then proves the pillar's central equivalence — that the caller-supplied-tree construction path ([Mode B](../GLOSSARY.md#mode-b--build_from_tree), `build_from_tree`) produces a graph of identical shape to the internal-parse path (`build`).

Each claim is framed as **hypothesis → experiment → result**, with the deciding assertion quoted verbatim from the source.

## 0. The object under test

`CodePropertyGraph` wraps a single [petgraph](../GLOSSARY.md#petgraph) `DiGraph<CpgNode, CpgEdge>`. The four program views are not four graphs; they are **edge-typed overlays over one node set** — the defining idea of the [Code Property Graph](../GLOSSARY.md#code-property-graph-cpg) introduced by Yamaguchi et al. [[1]](#references). `stats()` makes this concrete — it partitions the *one* edge collection by kind:

```rust
pub fn stats(&self) -> CpgStats {
    let ast_edges = self.edges().filter(|e| e.kind.is_ast()).count();
    let cfg_edges = self.edges().filter(|e| e.kind.is_cfg()).count();
    let dfg_edges = self.edges().filter(|e| e.kind.is_dfg()).count();
    // …
}
```

That AST/CFG/DFG counts are *filters over a shared edge set* — not fields of separate structures — is the mechanical root of every invariant below.

## 1. Invariant I1 — one shared node set with graph-assigned unique IDs

**Hypothesis.** Nodes are owned by the graph, which assigns each a unique, monotonically increasing [`NodeId`](../GLOSSARY.md#node-kind--edge-kind); all overlays (AST, CFG, DFG, PDG) reference the *same* IDs, so an edge of any kind connects nodes that already exist in the one node set.

**Mechanism.** `add_node` ignores the caller's placeholder ID and stamps a fresh one:

```rust
pub fn add_node(&mut self, mut node: CpgNode) -> NodeId {
    let id = NodeId::new(self.next_node_id);
    self.next_node_id += 1;
    node.id = id;
    // …insert into the petgraph, record id → index…
    id
}
```

**Experiment — uniqueness.** `test_multi_embedding_backtracking` (in `src/pattern/vf2.rs`) builds seven nodes, *every one* constructed with the same placeholder `NodeId::new(0)`, then relies on them being distinct:

```rust
fn node(g: &mut CodePropertyGraph) -> NodeId {
    g.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::If,
        SourceRange::default(),
    ))
}
// Pattern: path p0 -> p1 -> p2.
let p: Vec<NodeId> = (0..3).map(|_| node(&mut pattern)).collect();
// Target: diamond t0 -> {t1, t2} -> t3.
let t: Vec<NodeId> = (0..4).map(|_| node(&mut target)).collect();
```

Were IDs *not* reassigned, `p[0] == p[1] == p[2]` and the three-node path could not exist; the test's later assertion that the search yields two three-node embeddings (`assert_eq!(matches.len(), 2, …)`) would be impossible. Its passing is a direct witness that `add_node` produces distinct IDs.

**Experiment — shared node set across overlays.** `def_use_backward_slice` (in `src/builder/pdg.rs`) creates a definition and a use as *AST children*, runs the CFG, DFG, and PDG builders, then slices over the resulting [PDG](../GLOSSARY.md#program-dependence-graph-pdg) edges and asserts the slice contains those same AST node IDs:

```rust
let def_x = add_child(&mut cpg, body, variable("x"));
let use_x = add_child(&mut cpg, body, ident("x"));
CfgExtractor::new().extract(&mut cpg);
DfgExtractor::new().extract(&mut cpg);
PdgBuilder::new().build(&mut cpg, func);

let slice = backward_slice(&cpg, use_x, 100);
assert!(slice.contains(&use_x), "slice must contain the criterion");
assert!(
    slice.contains(&def_x),
    "backward slice of a use must contain its reaching definition"
);
```

**Result.** The PDG's data-dependence edges connect the *identical* IDs that name AST nodes (`def_x`, `use_x`). Control, data, and syntax overlays share one node set — I1 holds.

## 2. Invariant I2 — AST child/parent consistency

**Hypothesis.** For every syntactic containment, the builder maintains an `AstChild` edge *and* a parent back-pointer in lockstep, so the child-edge traversal (`ast_children`, `ast_descendants`) and the parent chain (`ast_parent`, `ast_ancestors`) describe the same tree.

**Mechanism.** Construction sets both halves together, as the DFG test fixture `create_test_function` shows:

```rust
cpg.connect(func, body, CpgEdgeKind::AstChild);
cpg.node_mut(body).unwrap().parent = Some(func);
```

**Experiment.** `parsed_nested_use_resolves_and_chains` (in `src/builder/dfg.rs`) uses the *descendant* traversal as an oracle: the nested `buf` identifier inside `decode(buf)` must be reported as living inside the `let out = …` statement's subtree.

```rust
assert!(
    cpg.ast_descendants(out_def).contains(&buf_use),
    "the `buf` use lives inside the `let out` statement"
);
```

**Result.** `ast_descendants` (a walk over `AstChild` edges) agrees with the actual nesting the source expresses, so the child-edge overlay faithfully encodes the parse tree. Because the same node also carries a `parent` field set at construction, both navigation directions are consistent — I2 holds. (This same containment fact is load-bearing for reaching-defs; see [02](02-reaching-defs-validation.md).)

## 3. Invariant I3 — idempotent extractors

**Hypothesis.** Re-running a construction stage does not change the graph: an [idempotent](../GLOSSARY.md#idempotent) extractor adds an edge only if an identical one is absent, so stages can be safely re-applied (e.g. by a consumer that rebuilds the DFG, or a [PDG](../GLOSSARY.md#program-dependence-graph-pdg) that is added on demand after the DFG already exists).

**Mechanism.** `add_node`/`connect` themselves do *not* deduplicate — `connect` unconditionally calls `add_edge`. Idempotency is therefore the *extractor's* responsibility: the DFG and PDG builders snapshot the existing edges of the kind they are about to add and skip duplicates. The DFG builder documents this directly:

> The pass is intraprocedural, bounded, language-agnostic … and idempotent: an edge is added only when an identical one does not already exist, so uses that *are* CFG nodes are not double-linked and re-running `extract` is a no-op.

**Experiment.** `parsed_shadowing_and_idempotent` (in `src/builder/dfg.rs`) measures the [DFG](../GLOSSARY.md#data-flow-graph-dfg) edge count, re-runs extraction, and asserts it is unchanged:

```rust
// Idempotency: a second extraction pass must not add any edge.
let before = cpg.stats().dfg_edges;
DfgExtractor::new().extract(&mut cpg);
assert_eq!(before, cpg.stats().dfg_edges, "extract must be idempotent");
```

**Result.** A second full DFG sweep adds exactly zero edges — I3 holds for the DFG, the stage most at risk of double-linking (it runs multiple sub-sweeps). The `PdgBuilder` implements the same snapshot-and-skip guard — its source states it is "idempotent (existing PDG edges are not duplicated)" and it snapshots existing PDG edges "so a re-build does not duplicate them" — so an on-demand PDG added after construction cannot corrupt an earlier one. The directly regression-locked evidence is the DFG assertion above; the PDG guard is documented and implemented in the same style.

## 4. The equivalence theorem: `build` ≡ `build_from_tree`

This is the pillar's central structural claim. `libcpg` offers two construction entry points:

- **Mode A — `build(source, language)`.** The builder parses the source with its own registered tree-sitter grammar. Requires the relevant `lang-*` feature.
- **[Mode B](../GLOSSARY.md#mode-b--build_from_tree) — `build_from_tree(&tree, source, language)`.** The **caller** supplies an already-parsed `tree_sitter::Tree`. Needs no `lang-*` feature, skips the `max_file_size` check, and is the *only* path available for Rholang and MeTTa and the one `pgmcp` uses.

Consumers pick the mode for operational reasons (who owns the grammar, whether the input was pre-parsed), and they must be able to do so **without changing the resulting analysis.** That is only true if both paths build the *same graph*.

**Hypothesis.** For the same source and grammar, `build_from_tree` and `build` produce graphs with identical node counts, identical edge counts, and the same language tag.

**Experiment.** `test_build_from_tree_matches_build` (in `src/builder/tree_sitter.rs`) parses the source *externally* — exactly as a Mode-B caller such as `pgmcp` would — then builds both ways and compares:

```rust
let source = "fn main() { let x = 1; let y = x; }";

// Parse externally, exactly as a Mode-B caller (e.g. pgmcp) would.
let ts_language = ParserRegistry::global()
    .get(Language::Rust)
    .expect("rust grammar should be registered");
let mut parser = tree_sitter::Parser::new();
parser.set_language(&ts_language).expect("set language should succeed");
let tree = parser.parse(source, None).expect("parse should succeed");

let builder = TreeSitterCpgBuilder::new();
let from_tree = builder
    .build_from_tree(&tree, source, Language::Rust)
    .expect("build_from_tree should succeed");
let from_source = builder
    .build(source, Language::Rust)
    .expect("build should succeed");

// The caller-supplied-tree path must produce a graph identical in shape
// to the internal-parse path (same nodes, same edges, same language).
assert!(from_tree.node_count() > 1);
assert_eq!(from_tree.node_count(), from_source.node_count());
assert_eq!(from_tree.edge_count(), from_source.edge_count());
assert_eq!(from_tree.language(), Language::Rust);
```

**Why this input is well-chosen.** `fn main() { let x = 1; let y = x; }` is not a trivial one-node program: it forces all three overlays to be exercised, so the counts being compared are non-degenerate. It contains a function (an AST subtree and a CFG entry), two `let` bindings, and — crucially — `let y = x`, which creates a [reaching definition](../GLOSSARY.md#reaching-definition) from the binding of `x` to its use in the initializer of `y`, i.e. a real [DFG](../GLOSSARY.md#data-flow-graph-dfg) def-use edge. The `assert!(from_tree.node_count() > 1)` guards against the vacuous case where both paths agree only because both produced nothing.

**Result.** The two paths agree on `node_count()`, on `edge_count()`, and on `language()`. Because the CPG builder is a **pure function of the parse tree** (it walks the tree and emits nodes/edges deterministically; the only difference between the modes is *who* produced the tree and whether the size guard ran), agreeing cardinalities across the AST+CFG+DFG overlays are exactly the observable signature of identical structure. The test asserts the *necessary* cardinality-and-language condition rather than a node-by-node isomorphism, and determinism of the walk supplies the rest.

![Mode A and Mode B converge on one identical CPG](../diagrams/build-equivalence.svg)

*Figure — `build` (internal parse) and `build_from_tree` (caller-supplied tree) as two entry points onto one deterministic tree-walk, yielding graphs of identical node/edge count and language. Source: [`diagrams/build-equivalence.puml`](../diagrams/build-equivalence.puml).*

**Consequence.** Everything validated for Mode A transfers to Mode B, and vice versa. The reaching-defs corpus in [02](02-reaching-defs-validation.md) is written against `build` (Mode A) for convenience; this equivalence is what licenses trusting the same guarantees for the Rholang/MeTTa/`pgmcp` Mode-B path, which cannot use `build` at all. See [`design/0002-mode-b-build-from-tree.md`](../design/0002-mode-b-build-from-tree.md) for the decision record and [`components/builder/overview.md`](../components/builder/overview.md) for the construction stages.

## 5. Summary of invariants

| Invariant | Statement | Anchoring test | Deciding assertion |
|---|---|---|---|
| I1 (shared node set) | Graph-assigned unique IDs; all overlays index one node set | `test_multi_embedding_backtracking`; `def_use_backward_slice` | distinct-node reliance; `slice.contains(&def_x)` |
| I2 (AST consistency) | `AstChild` edges and parent pointers agree | `parsed_nested_use_resolves_and_chains` | `ast_descendants(out_def).contains(&buf_use)` |
| I3 (idempotency) | Re-running an extractor adds no edge | `parsed_shadowing_and_idempotent` | `assert_eq!(before, cpg.stats().dfg_edges, …)` |
| E (equivalence) | `build` ≡ `build_from_tree` in shape | `test_build_from_tree_matches_build` | equal `node_count`/`edge_count`/`language` |

See also: [`theory/01-code-property-graphs.md`](../theory/01-code-property-graphs.md) for the formal overlay model, and [`components/builder/overview.md`](../components/builder/overview.md) for how `convert_node` emits the AST edges and CFG entry points these invariants govern.

## References

1. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and Discovering Vulnerabilities with Code Property Graphs.* 2014 IEEE Symposium on Security and Privacy. DOI: [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44)
