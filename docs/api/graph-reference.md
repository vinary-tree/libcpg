# Graph API Reference

This page is the authoritative reference for the **core graph types** of
`libcpg` — the [`CodePropertyGraph`](../GLOSSARY.md#code-property-graph-cpg)
container, its nodes and edges, and the identifier, location, language, and
error types that surround them. Every signature below is transcribed from
`src/graph/` (and the crate-root re-exports in `src/lib.rs`); nothing here is
aspirational.

If you are looking for how graphs are *built*, see the
[Builder reference](builder-reference.md); for how they are *analyzed*
(matching, similarity, GoF, algorithms, GNN) see the
[Pattern reference](pattern-reference.md). Conceptual definitions of every term
live in the [Glossary](../GLOSSARY.md).

![A Code Property Graph overlays AST, control-flow, and data-flow edges on one shared node set](../diagrams/cpg-overlay.svg)

*Figure — the four overlays of a CPG share one node set; each edge kind projects a different program view. Source: [`diagrams/cpg-overlay.dot`](../diagrams/cpg-overlay.dot).*

---

## Where these types live

Every type on this page is re-exported from the crate root, so a single `use`
suffices:

```rust
use libcpg::{
    CodePropertyGraph, CpgStats,
    CpgNode, CpgNodeKind, LiteralKind,
    CpgEdge, CpgEdgeKind, CfgEdgeKind, DfgEdgeKind,
    NodeId, EdgeId, SourceRange,
    Language, Paradigm,
    TypeInfo, MethodSignature, Visibility, ScopeId,
    PropertyKey, PropertyValue,
    Error, Result,
};
```

They also remain reachable at their defining paths (`libcpg::graph::…`), but the
root re-exports are canonical.

---

## `CodePropertyGraph`

The central container. It is **not** a bag of vectors: it wraps a
[petgraph](../GLOSSARY.md#petgraph) `DiGraph<CpgNode, CpgEdge>` plus two
`FxHashMap` side-tables that translate the stable `NodeId`/`EdgeId` the API
exposes into petgraph's internal `NodeIndex`/`EdgeIndex`. All fields are private;
you interact through the methods below.

```rust
pub struct CodePropertyGraph { /* private: petgraph DiGraph + id→index maps + metadata */ }
```

Besides the topology it records: the [`Language`](#language) it was built for, an
optional source path and retained source string, the AST `root` node, and the
[CFG](../GLOSSARY.md#control-flow-graph-cfg) entry/exit node lists.

### Construction & metadata

| Method | Signature | Notes |
|--------|-----------|-------|
| `new` | `fn new(language: Language) -> Self` | Empty graph for `language`. |
| `with_source_path` | `fn with_source_path(self, path: impl Into<Arc<str>>) -> Self` | Records the source file path. |
| `with_source_code` | `fn with_source_code(self, code: impl Into<Arc<str>>) -> Self` | Retains the source string (opt-in; off by default). |
| `language` | `fn language(&self) -> Language` | The graph's language. |
| `source_path` | `fn source_path(&self) -> Option<&str>` | The recorded path, if any. |
| `source_code` | `fn source_code(&self) -> Option<&str>` | The retained source, if any. |
| `root` | `fn root(&self) -> Option<NodeId>` | The AST root (the first node added). |
| `node_count` | `fn node_count(&self) -> usize` | Number of nodes. |
| `edge_count` | `fn edge_count(&self) -> usize` | Number of edges. |

`CodePropertyGraph` also implements `Clone`, `Debug`, and `Default` (a `Default`
graph has `Language::Unknown`).

### Node operations

| Method | Signature | Notes |
|--------|-----------|-------|
| `add_node` | `fn add_node(&mut self, node: CpgNode) -> NodeId` | Assigns a fresh `NodeId`, overwriting `node.id`. Sets `root` if this is the first node. |
| `add_node_with_id` | `fn add_node_with_id(&mut self, node: CpgNode) -> NodeId` | Keeps `node.id` (for reconstruction from serialized data). |
| `node` | `fn node(&self, id: NodeId) -> Option<&CpgNode>` | Look up a node. Returns `Option`, **not** `Result`. |
| `node_mut` | `fn node_mut(&mut self, id: NodeId) -> Option<&mut CpgNode>` | Mutable lookup. |
| `contains_node` | `fn contains_node(&self, id: NodeId) -> bool` | Membership test. |
| `nodes` | `fn nodes(&self) -> impl Iterator<Item = &CpgNode>` | Iterate all nodes. |
| `node_ids` | `fn node_ids(&self) -> impl Iterator<Item = NodeId> + '_` | Iterate all node ids. |

> There is no `remove_node` / `remove_edge`: a CPG is built once and analyzed;
> extraction stages *add* overlay edges rather than mutate topology in place.

### Edge operations

| Method | Signature | Notes |
|--------|-----------|-------|
| `add_edge` | `fn add_edge(&mut self, edge: CpgEdge) -> Option<EdgeId>` | Assigns a fresh `EdgeId`. `None` if either endpoint is absent. |
| `add_edge_with_id` | `fn add_edge_with_id(&mut self, edge: CpgEdge) -> Option<EdgeId>` | Keeps `edge.id` (reconstruction). |
| `connect` | `fn connect(&mut self, source: NodeId, target: NodeId, kind: CpgEdgeKind) -> Option<EdgeId>` | Convenience constructor + insert. Returns `None` if an endpoint is missing. |
| `edges_between` | `fn edges_between(&self, source: NodeId, target: NodeId) -> Vec<&CpgEdge>` | All parallel edges from `source` to `target`. |
| `edges` | `fn edges(&self) -> impl Iterator<Item = &CpgEdge>` | Iterate all edges. |

There is no `edge(id)` accessor; look edges up by endpoint with `edges_between`,
`outgoing_edges`, or `incoming_edges`.

### AST traversal

Because petgraph iterates a node's outgoing edges newest-first, `ast_children`
sorts `AstChild` edges by `EdgeId` (assigned monotonically) to recover true
**source order** — analyses rely on, e.g., an `If`'s children being
`[condition, then, else]`.

| Method | Signature | Notes |
|--------|-----------|-------|
| `ast_children` | `fn ast_children(&self, id: NodeId) -> Vec<NodeId>` | Children in source order. |
| `ast_parent` | `fn ast_parent(&self, id: NodeId) -> Option<NodeId>` | Reads the node's `parent` pointer. |
| `ast_descendants` | `fn ast_descendants(&self, id: NodeId) -> Vec<NodeId>` | Depth-first descendants. |
| `ast_ancestors` | `fn ast_ancestors(&self, id: NodeId) -> Vec<NodeId>` | Chain toward the root. |

### CFG traversal

CFG edges carry a [`CfgEdgeKind`](#cfgedgekind); the successor/predecessor
accessors return the neighbour paired with the edge kind, so a caller can branch
on, e.g., `ConditionalTrue` vs `LoopBack`.

| Method | Signature | Notes |
|--------|-----------|-------|
| `cfg_successors` | `fn cfg_successors(&self, id: NodeId) -> Vec<(NodeId, CfgEdgeKind)>` | Outgoing control flow. |
| `cfg_predecessors` | `fn cfg_predecessors(&self, id: NodeId) -> Vec<(NodeId, CfgEdgeKind)>` | Incoming control flow. |
| `cfg_entries` | `fn cfg_entries(&self) -> &[NodeId]` | Function entry nodes. |
| `cfg_exits` | `fn cfg_exits(&self) -> &[NodeId]` | Recorded exit nodes. |
| `add_cfg_entry` | `fn add_cfg_entry(&mut self, id: NodeId)` | Register an entry (dedup). |
| `add_cfg_exit` | `fn add_cfg_exit(&mut self, id: NodeId)` | Register an exit (dedup). |
| `cfg_nodes` | `fn cfg_nodes(&self) -> impl Iterator<Item = &CpgNode>` | Nodes with any incident CFG edge. |

### DFG traversal

| Method | Signature | Notes |
|--------|-----------|-------|
| `reaching_definitions` | `fn reaching_definitions(&self, use_site: NodeId) -> Vec<NodeId>` | Definitions reaching `use_site` (incoming `DefUse`/`ReachingDef`). |
| `uses_of_definition` | `fn uses_of_definition(&self, def: NodeId) -> Vec<NodeId>` | Uses reached by `def` (outgoing `DefUse`). |
| `dfg_successors` | `fn dfg_successors(&self, id: NodeId) -> Vec<(NodeId, DfgEdgeKind)>` | Outgoing data flow. |
| `dfg_predecessors` | `fn dfg_predecessors(&self, id: NodeId) -> Vec<(NodeId, DfgEdgeKind)>` | Incoming data flow. |

See [reaching definitions](../GLOSSARY.md#reaching-definition) for the semantics
these edges encode.

### Call graph

| Method | Signature | Notes |
|--------|-----------|-------|
| `call_sites` | `fn call_sites(&self, function: NodeId) -> Vec<NodeId>` | `Call` nodes in `function`'s subtree. |
| `callees` | `fn callees(&self, call_site: NodeId) -> Vec<NodeId>` | Targets via `CallSite`/`StaticCall`/`DynamicCall`. |
| `callers` | `fn callers(&self, function: NodeId) -> Vec<NodeId>` | Call sites that reach `function`. |

### Query by kind

`nodes_by_kind` takes a **predicate over the kind**, not a kind value — there is
no `nodes_of_kind(kind)`.

| Method | Signature | Notes |
|--------|-----------|-------|
| `functions` | `fn functions(&self) -> impl Iterator<Item = &CpgNode>` | All `Function` nodes. |
| `classes` | `fn classes(&self) -> impl Iterator<Item = &CpgNode>` | All `Class` nodes. |
| `variables` | `fn variables(&self) -> impl Iterator<Item = &CpgNode>` | All `Variable` nodes. |
| `calls` | `fn calls(&self) -> impl Iterator<Item = &CpgNode>` | All `Call` nodes. |
| `nodes_by_kind` | `fn nodes_by_kind<F: Fn(&CpgNodeKind) -> bool>(&self, predicate: F) -> impl Iterator<Item = &CpgNode>` | Filter by a kind predicate. |

### Query by position

| Method | Signature | Notes |
|--------|-----------|-------|
| `node_at_offset` | `fn node_at_offset(&self, offset: u32) -> Option<&CpgNode>` | Smallest node covering a byte offset. |
| `nodes_in_range` | `fn nodes_in_range(&self, range: SourceRange) -> Vec<&CpgNode>` | Nodes overlapping a range. |
| `scope_at_offset` | `fn scope_at_offset(&self, offset: u32) -> Option<&CpgNode>` | Innermost `Block`/`Function` at an offset. |

### Edge helpers

| Method | Signature | Notes |
|--------|-----------|-------|
| `outgoing_edges` | `fn outgoing_edges(&self, id: NodeId) -> impl Iterator<Item = &CpgEdge>` | All outgoing edges. |
| `incoming_edges` | `fn incoming_edges(&self, id: NodeId) -> impl Iterator<Item = &CpgEdge>` | All incoming edges. |
| `edges_by_kind` | `fn edges_by_kind<F: Fn(&CpgEdgeKind) -> bool>(&self, predicate: F) -> impl Iterator<Item = &CpgEdge>` | Filter edges by a kind predicate. |

### Graph metrics

| Method | Signature | Notes |
|--------|-----------|-------|
| `ast_depth` | `fn ast_depth(&self) -> usize` | Longest root-to-leaf AST path. |
| `cyclomatic_complexity` | `fn cyclomatic_complexity(&self) -> usize` | McCabe's metric over the CFG (below). |
| `stats` | `fn stats(&self) -> CpgStats` | Aggregate counts (see [`CpgStats`](#cpgstats)). |

`cyclomatic_complexity` computes [McCabe's metric](../GLOSSARY.md#cyclomatic-complexity) [[2]](#references)

```math
M = E - N + 2
```

where $`E`$ is the number of CFG edges and $`N`$ the number of CFG
nodes (`cfg_nodes().count()`); when there are no CFG nodes it returns $`1`$.
The implementation uses a saturating subtraction, so it never underflows.

### Subgraph extraction

| Method | Signature | Notes |
|--------|-----------|-------|
| `subgraph` | `fn subgraph(&self, node_ids: &[NodeId]) -> Self` | Induced subgraph over `node_ids` (edges kept via `add_edge_with_id`, preserving ids). |
| `function_cfg` | `fn function_cfg(&self, function: NodeId) -> Self` | Control-flow/expression subtree of `function`. |
| `function_dfg` | `fn function_dfg(&self, function: NodeId) -> Self` | Subtree nodes carrying DFG edges. |

### Worked example — a hand-built CPG (no features required)

Every method above is available with `default = []`; only *parsing* needs a
`lang-*` feature. This builds a two-node CPG by hand and reads it back:

```rust
// requires: no features (the feature-free hand-built surface)
use libcpg::{
    CodePropertyGraph, CpgNode, CpgNodeKind, CpgEdgeKind,
    Language, NodeId, ScopeId, SourceRange, MethodSignature, Visibility,
};

let mut cpg = CodePropertyGraph::new(Language::Rust);

let func = cpg.add_node(CpgNode::new(
    NodeId::new(0),                      // id is reassigned by add_node
    CpgNodeKind::Function {
        signature: MethodSignature {
            name: "main".into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Public,
        },
    },
    SourceRange::default(),
));
let body = cpg.add_node(CpgNode::new(
    NodeId::new(0),
    CpgNodeKind::Block { scope: ScopeId::GLOBAL },
    SourceRange::default(),
));
cpg.connect(func, body, CpgEdgeKind::AstChild);

assert_eq!(cpg.node_count(), 2);
assert_eq!(cpg.ast_children(func), vec![body]);
assert_eq!(cpg.functions().count(), 1);
```

---

## `CpgNode`

A node is a plain struct with **public fields** — read `node.kind`, not
`node.kind()`.

```rust
pub struct CpgNode {
    pub id: NodeId,
    pub kind: CpgNodeKind,
    pub range: SourceRange,
    pub text: Option<Arc<str>>,
    pub properties: FxHashMap<PropertyKey, PropertyValue>,
    pub children: SmallVec<[NodeId; 4]>,
    pub parent: Option<NodeId>,
}
```

| Field | Type | Meaning |
|-------|------|---------|
| `id` | `NodeId` | Unique identifier within the graph. |
| `kind` | [`CpgNodeKind`](#cpgnodekind) | The node's tagged variant + payload. |
| `range` | [`SourceRange`](#sourcerange) | Byte/line/column span in source. |
| `text` | `Option<Arc<str>>` | Verbatim source text (terminals). |
| `properties` | `FxHashMap<PropertyKey, PropertyValue>` | Extra metadata. |
| `children` | `SmallVec<[NodeId; 4]>` | AST child ids (source order). |
| `parent` | `Option<NodeId>` | AST parent id. |

### Methods

| Method | Signature | Notes |
|--------|-----------|-------|
| `new` | `fn new(id: NodeId, kind: CpgNodeKind, range: SourceRange) -> Self` | Constructor. |
| `with_text` | `fn with_text(self, text: impl Into<Arc<str>>) -> Self` | Builder: set source text. |
| `with_property` | `fn with_property(self, key: PropertyKey, value: PropertyValue) -> Self` | Builder: add a property. |
| `with_child` | `fn with_child(self, child: NodeId) -> Self` | Builder: append a child id. |
| `with_parent` | `fn with_parent(self, parent: NodeId) -> Self` | Builder: set the parent id. |
| `name` | `fn name(&self) -> Option<&str>` | Name for named kinds (below); `None` otherwise. |
| `is_declaration` | `fn is_declaration(&self) -> bool` | `Module`/`Class`/`Struct`/`Enum`/`Trait`/`Function`/`Variable`/`Field`/`Parameter`. |
| `is_statement` | `fn is_statement(&self) -> bool` | `Return`/`If`/`While`/`For`/`Loop`/`Match`/`Break`/`Continue`/`Throw`/`Try`. |
| `is_expression` | `fn is_expression(&self) -> bool` | `BinaryOp`/`UnaryOp`/`Assignment`/`Call`/`MemberAccess`/`IndexAccess`/`Identifier`/`Literal`/`Lambda`/`Await`/`Yield`. |
| `is_control_flow` | `fn is_control_flow(&self) -> bool` | `If`/`While`/`For`/`Loop`/`Match`/`Break`/`Continue`/`Return`/`Throw`/`Try`. |
| `is_error` | `fn is_error(&self) -> bool` | `Error` (parser recovery). |

---

## `CpgNodeKind`

The [node kind](../GLOSSARY.md#node-kind--edge-kind) is a 45-variant enum mixing
**unit variants** (e.g. `Root`, `If`, `Return`, `Await`) with **data-carrying
variants** (e.g. `Function { signature }`, `Call { target, is_method }`). Kinds
drive every query, pattern, and complexity heuristic.

![Taxonomy of the 45 CpgNodeKind variants grouped by category](../diagrams/node-kind-taxonomy.svg)

*Figure — the node-kind taxonomy: structural, function-level, variable, statement, expression, type, and special categories. Source: [`diagrams/node-kind-taxonomy.puml`](../diagrams/node-kind-taxonomy.puml).*

### Structural (7)

| Variant | Payload fields | `name()` |
|---------|----------------|----------|
| `Root` | — | — |
| `Module` | `name: Arc<str>` | ✓ |
| `Class` | `name: Arc<str>`, `is_abstract: bool` | ✓ |
| `Struct` | `name: Arc<str>` | ✓ |
| `Enum` | `name: Arc<str>` | ✓ |
| `Trait` | `name: Arc<str>` | ✓ |
| `Impl` | `for_type: Option<Arc<str>>`, `trait_name: Option<Arc<str>>` | — |

### Function-level (3)

| Variant | Payload fields | `name()` |
|---------|----------------|----------|
| `Function` | `signature: MethodSignature` | ✓ (from `signature.name`) |
| `Parameter` | `name: Arc<str>`, `param_type: Option<TypeInfo>`, `is_variadic: bool` | ✓ |
| `Block` | `scope: ScopeId` | — |

### Variable (2)

| Variant | Payload fields | `name()` |
|---------|----------------|----------|
| `Variable` | `name: Arc<str>`, `var_type: Option<TypeInfo>`, `scope: ScopeId`, `is_mutable: bool` | ✓ |
| `Field` | `name: Arc<str>`, `field_type: Option<TypeInfo>`, `visibility: Visibility` | ✓ |

### Statement (14, all unit variants)

`Return`, `If`, `Else`, `While`, `For`, `Loop`, `Match`, `MatchArm`, `Break`,
`Continue`, `Throw`, `Try`, `Catch`, `Finally`.

### Expression (11)

| Variant | Payload fields | `name()` |
|---------|----------------|----------|
| `BinaryOp` | `operator: Arc<str>` | — |
| `UnaryOp` | `operator: Arc<str>` | — |
| `Assignment` | `operator: Arc<str>` | — |
| `Call` | `target: Option<NodeId>`, `is_method: bool` | — |
| `MemberAccess` | `member: Arc<str>` | ✓ (the member) |
| `IndexAccess` | — | — |
| `Identifier` | `name: Arc<str>`, `definition: Option<NodeId>` | ✓ |
| `Literal` | `kind: LiteralKind` | — |
| `Lambda` | `captures: SmallVec<[NodeId; 4]>` | — |
| `Await` | — | — |
| `Yield` | — | — |

### Type (2)

| Variant | Payload fields | `name()` |
|---------|----------------|----------|
| `TypeAnnotation` | `type_info: TypeInfo` | — |
| `GenericParam` | `name: Arc<str>` | ✓ |

### Special (6)

| Variant | Payload fields | `name()` |
|---------|----------------|----------|
| `Comment` | `is_doc: bool` | — |
| `Import` | `path: Arc<str>` | ✓ (the path) |
| `Attribute` | `name: Arc<str>` | ✓ |
| `Macro` | `name: Arc<str>` | ✓ |
| `Error` | `message: Arc<str>` | — |
| `Unknown` | `kind: Arc<str>` | — |

Total: 7 + 3 + 2 + 14 + 11 + 2 + 6 = **45** variants.

---

## `LiteralKind`

The payload of `CpgNodeKind::Literal { kind }`.

| Variant | Payload | Meaning |
|---------|---------|---------|
| `Integer` | `i64` | Integer literal. |
| `Float` | `f64` | Float literal. |
| `String` | `Arc<str>` | String literal. |
| `Char` | `char` | Character literal. |
| `Bool` | `bool` | Boolean literal. |
| `Null` | — | `null` / `nil` / `None`. |
| `Array` | — | Array/list literal. |
| `Object` | — | Object/map literal. |
| `Regex` | `Arc<str>` | Regex literal. |

---

## Supporting node types

### `TypeInfo`

```rust
pub struct TypeInfo {
    pub name: Arc<str>,
    pub is_reference: bool,
    pub is_mutable: bool,
    pub generics: SmallVec<[Arc<str>; 2]>,
}
```

Builder methods: `new(name: impl Into<Arc<str>>)`, `with_reference(bool)`,
`with_mutable(bool)`, `with_generic(impl Into<Arc<str>>)`.

### `MethodSignature`

```rust
pub struct MethodSignature {
    pub name: Arc<str>,
    pub params: SmallVec<[TypeInfo; 4]>,
    pub return_type: Option<TypeInfo>,
    pub is_static: bool,
    pub is_async: bool,
    pub visibility: Visibility,
}
```

Carried by `CpgNodeKind::Function { signature }`.

### `Visibility`

Unit enum: `Public`, `Private` (the `Default`), `Protected`, `Package`, `Crate`.

### `ScopeId`

`pub struct ScopeId(pub u32)`. Constant `ScopeId::GLOBAL == ScopeId(0)`;
constructor `ScopeId::new(u32)`.

### `PropertyKey` / `PropertyValue`

`CpgNode::properties` maps keys to values:

- `PropertyKey`: `Name`, `Type`, `Scope`, `Visibility`, `Mutable`, `Static`,
  `Async`, `Custom(Arc<str>)`.
- `PropertyValue`: `String(Arc<str>)`, `Int(i64)`, `Uint(u64)`, `Bool(bool)`,
  `Float(f64)`, `List(Vec<PropertyValue>)`, `Null`. Accessors `as_str()`,
  `as_int()`, `as_bool()` return `Option`s.

---

## `CpgEdge`

Edges, like nodes, expose **public fields** — use `edge.source`, not
`edge.source()`.

```rust
pub struct CpgEdge {
    pub id: EdgeId,
    pub source: NodeId,
    pub target: NodeId,
    pub kind: CpgEdgeKind,
    pub label: Option<String>,
}
```

### Constructors & helpers

| Method | Signature | Notes |
|--------|-----------|-------|
| `new` | `fn new(id: EdgeId, source: NodeId, target: NodeId, kind: CpgEdgeKind) -> Self` | General constructor. |
| `with_label` | `fn with_label(self, label: impl Into<String>) -> Self` | Builder: set a label. |
| `ast_child` | `fn ast_child(id: EdgeId, parent: NodeId, child: NodeId) -> Self` | `AstChild` edge. |
| `control_flow` | `fn control_flow(id: EdgeId, from: NodeId, to: NodeId, kind: CfgEdgeKind) -> Self` | `ControlFlow(kind)` edge. |
| `data_flow` | `fn data_flow(id: EdgeId, from: NodeId, to: NodeId, kind: DfgEdgeKind) -> Self` | `DataFlow(kind)` edge. |
| `def_use` | `fn def_use(id: EdgeId, def: NodeId, use_site: NodeId) -> Self` | `DataFlow(DefUse)` shortcut. |
| `reference` | `fn reference(id: EdgeId, use_site: NodeId, def: NodeId) -> Self` | `Reference` edge. |
| `call_site` | `fn call_site(id: EdgeId, call: NodeId, callee: NodeId) -> Self` | `CallSite` edge. |
| `is_forward` | `fn is_forward(&self) -> bool` | `false` for `AstParent`, `AstPrevSibling`, `DataFlow(UseDef)`. |

> When you use [`connect`](#edge-operations) or `add_edge`, pass any `EdgeId`
> (e.g. `EdgeId::new(0)`) — the graph reassigns it.

---

## `CpgEdgeKind`

A 22-variant enum. Two variants wrap a finer kind:
`ControlFlow(CfgEdgeKind)` and `DataFlow(DfgEdgeKind)`.

![Taxonomy of CpgEdgeKind and its CFG/DFG sub-kinds](../diagrams/edge-kind-taxonomy.svg)

*Figure — the edge-kind taxonomy across the AST, CFG, DFG, PDG, call, type, reference, scope, and import families. Source: [`diagrams/edge-kind-taxonomy.puml`](../diagrams/edge-kind-taxonomy.puml).*

| Family | Variants |
|--------|----------|
| AST | `AstChild`, `AstParent`, `AstNextSibling`, `AstPrevSibling` |
| CFG | `ControlFlow(CfgEdgeKind)` |
| DFG | `DataFlow(DfgEdgeKind)` |
| PDG | `ControlDependence`, `DataDependence` |
| Call | `StaticCall`, `DynamicCall`, `CallSite` |
| Type | `TypeOf`, `Inherits`, `Implements`, `GenericInstance` |
| Reference | `Reference`, `Definition`, `Declaration` |
| Scope | `EnclosingScope`, `ContainedIn` |
| Import | `Imports`, `Exports` |

### Classifier helpers

Predicates on the kind, useful with `edges_by_kind`:

| Method | True for |
|--------|----------|
| `is_ast(&self) -> bool` | the four AST variants |
| `is_cfg(&self) -> bool` | `ControlFlow(_)` |
| `is_dfg(&self) -> bool` | `DataFlow(_)` |
| `is_pdg(&self) -> bool` | `ControlDependence`, `DataDependence` |
| `is_call(&self) -> bool` | `StaticCall`, `DynamicCall`, `CallSite` |
| `is_type(&self) -> bool` | `TypeOf`, `Inherits`, `Implements`, `GenericInstance` |

---

## `CfgEdgeKind`

The 14 [control-flow](../GLOSSARY.md#control-flow-graph-cfg) edge kinds carried by
`CpgEdgeKind::ControlFlow(_)`.

| Variant | Meaning |
|---------|---------|
| `Sequential` | Fallthrough between statements. |
| `ConditionalTrue` | Branch taken when a condition holds. |
| `ConditionalFalse` | Branch taken when a condition fails. |
| `LoopBack` | Back edge to a loop head. |
| `LoopExit` | Edge leaving a loop. |
| `Break` | `break` to loop exit. |
| `Continue` | `continue` to loop head. |
| `Return` | Edge to function exit. |
| `Throw` | Exception raise edge. |
| `Catch` | Edge into a handler. |
| `Call` | Edge into a callee. |
| `CallReturn` | Return from a callee. |
| `Case` | Match/switch case edge. |
| `DefaultCase` | Default case edge. |

Helpers: `is_conditional()` (`ConditionalTrue`/`ConditionalFalse`/`Case`/`DefaultCase`),
`is_loop()` (`LoopBack`/`LoopExit`/`Break`/`Continue`), `is_exception()`
(`Throw`/`Catch`).

---

## `DfgEdgeKind`

The 13 [data-flow](../GLOSSARY.md#data-flow-graph-dfg) edge kinds carried by
`CpgEdgeKind::DataFlow(_)`.

| Variant | Meaning |
|---------|---------|
| `DefUse` | Definition → use. |
| `UseDef` | Use → definition (reverse). |
| `ReachingDef` | A reaching definition. |
| `DataDependency` | Generic data dependency. |
| `Parameter` | Argument → parameter. |
| `ReturnValue` | Return expression → caller. |
| `FieldRead` | Object → member access (read). |
| `FieldWrite` | Field write. |
| `IndexRead` | Array/index read. |
| `IndexWrite` | Array/index write. |
| `Alias` | Alias relationship. |
| `Dereference` | Pointer dereference. |
| `AddressOf` | Address-of. |

Helpers: `is_read()` (`DefUse`/`FieldRead`/`IndexRead`/`Dereference`),
`is_write()` (`UseDef`/`FieldWrite`/`IndexWrite`).

---

## Identifiers & locations

### `NodeId` / `EdgeId`

```rust
pub struct NodeId(pub u32);
pub struct EdgeId(pub u32);
```

Both provide `new(u32)` and `as_u32(self) -> u32`. `NodeId` implements
`From<u32>` and `From<NodeId> for u32` (both directions); `EdgeId` implements
`From<u32>`. There is **no** `.index()` method — use `as_u32()` (or the `.0`
tuple field).

### `SourceRange`

Six `u32` fields (byte offsets are half-open `[start, end)`; lines/columns are
0-indexed):

```rust
pub struct SourceRange {
    pub start: u32,
    pub end: u32,
    pub start_line: u32,
    pub start_col: u32,
    pub end_line: u32,
    pub end_col: u32,
}
```

Constructors and helpers: `new(start, end, start_line, start_col, end_line, end_col)`,
`from_bytes(start, end)` (lines/cols zeroed), `len() -> u32`, `is_empty() -> bool`,
`to_text_range() -> text_size::TextRange`, and `Default` (all zeros).

---

## `Language` & `Paradigm`

### `Language`

A `#[non_exhaustive]` enum of ~40 languages spanning systems, JVM, scripting,
functional, .NET, Apple, shell, query, markup/config, and the F1R3FLY.io
languages **`Rholang`** and **`MeTTa`**, plus `Unknown` (the `Default`).

| Method | Signature | Notes |
|--------|-----------|-------|
| `name` | `fn name(&self) -> &'static str` | Display name (e.g. `"C++"`). |
| `extensions` | `fn extensions(&self) -> &'static [&'static str]` | Common file extensions (plural). |
| `from_extension` | `fn from_extension(ext: &str) -> Language` | Detect from an extension; returns `Unknown` (not `Option`) when unmatched. Leading `.` and case are ignored. |
| `is_systems` | `fn is_systems(&self) -> bool` | Rust/C/C++/Go/Zig. |
| `is_jvm` | `fn is_jvm(&self) -> bool` | Java/Kotlin/Scala/Groovy/Clojure. |
| `is_scripting` | `fn is_scripting(&self) -> bool` | Python/JS/TS/Ruby/PHP/Perl/Lua. |
| `is_functional` | `fn is_functional(&self) -> bool` | Haskell/OCaml/F#/Elixir/Erlang/Clojure/Lisp/Scheme. |
| `is_markup` | `fn is_markup(&self) -> bool` | JSON/YAML/TOML/XML/HTML/CSS/Markdown. |
| `is_f1r3fly` | `fn is_f1r3fly(&self) -> bool` | Rholang/MeTTa. |
| `paradigms` | `fn paradigms(&self) -> &'static [Paradigm]` | Primary paradigms. |

`Language` also implements `Display` (via `name()`).

### `Paradigm`

`Imperative`, `Procedural`, `ObjectOriented`, `Functional`, `Logic`,
`Concurrent`, `Reactive`, `ProcessCalculus`, `Declarative`, `EventDriven`. Method
`name() -> &'static str`; implements `Display`. Rholang reports
`[Concurrent, ProcessCalculus]`; MeTTa reports `[Logic, Functional]`.

---

## `Error` & `Result`

```rust
pub type Result<T> = std::result::Result<T, Error>;

pub enum Error {
    Construction(String),
    PatternMatch(String),
    #[cfg(feature = "gnn")] Gnn(String),
    InvalidNodeId(NodeId),
    InvalidEdgeId(EdgeId),
    UnsupportedLanguage(String),
    Io(#[from] std::io::Error),          // From<std::io::Error>
    #[cfg(feature = "serde")] Serialization(String),
}
```

`Error` derives `thiserror::Error` (so it implements `std::error::Error` and
`Display`). There is **no** `CpgError` type. `Gnn` exists only with the `gnn`
feature; `Serialization` only with `serde`. `From<std::io::Error>` lets `?`
propagate I/O errors from, e.g., `build_file`.

| Variant | Raised when |
|---------|-------------|
| `Construction(String)` | Parse failure, oversized input, or malformed graph construction. |
| `PatternMatch(String)` | A pattern-matching operation failed. |
| `Gnn(String)` *(gnn)* | A GNN operation failed. |
| `InvalidNodeId(NodeId)` | A referenced node id is absent. |
| `InvalidEdgeId(EdgeId)` | A referenced edge id is absent. |
| `UnsupportedLanguage(String)` | No grammar registered for a language (e.g. its `lang-*` feature is off). |
| `Io(std::io::Error)` | Filesystem read failure. |
| `Serialization(String)` *(serde)* | (De)serialization failure. |

---

## `CpgStats`

Returned by `CodePropertyGraph::stats()`; a `Debug + Clone + Default` snapshot of
aggregate counts.

```rust
pub struct CpgStats {
    pub node_count: usize,
    pub edge_count: usize,
    pub ast_edges: usize,
    pub cfg_edges: usize,
    pub dfg_edges: usize,
    pub call_edges: usize,
    pub function_count: usize,
    pub class_count: usize,
    pub cyclomatic_complexity: usize,
}
```

Each edge count is `edges().filter(kind_predicate).count()`; `function_count` and
`class_count` come from `functions()`/`classes()`; `cyclomatic_complexity`
mirrors [`cyclomatic_complexity()`](#graph-metrics).

---

## Serialization note

With the `serde` feature, all types on this page derive `Serialize`/`Deserialize`
(the vector field of GNN embeddings is the only exception, and lives in the
[Pattern reference](pattern-reference.md#gnn--graph-neural-network-embeddings-feature-gnn)). There is **no**
bespoke export/import function or on-disk format — round-trip through your own
`serde_json`. To reconstruct a graph while preserving ids, use `add_node_with_id`
and `add_edge_with_id` rather than `add_node`/`add_edge`.

---

## See also

- [Builder reference](builder-reference.md) — how these graphs are constructed
  and how CFG/DFG/PDG overlays are added.
- [Pattern reference](pattern-reference.md) — matching, similarity, GoF,
  algorithm detection, and GNN embeddings over the graph.
- [Glossary](../GLOSSARY.md) — definitions of AST, CFG, DFG, PDG, and every other
  term used here.
- [Architecture overview](../architecture/overview.md) and
  [data flow](../architecture/data-flow.md) — the design behind the model.
- Component guides: [graph overview](../components/graph/overview.md),
  [nodes](../components/graph/nodes.md), [edges](../components/graph/edges.md),
  [traversal](../components/graph/traversal.md).

---

## References

1. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and Discovering Vulnerabilities with Code Property Graphs.* 2014 IEEE Symposium on Security and Privacy. DOI: [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44)
2. McCabe, T. J. (1976). *A Complexity Measure.* IEEE Transactions on Software Engineering SE-2(4). DOI: [10.1109/TSE.1976.233837](https://doi.org/10.1109/TSE.1976.233837)
