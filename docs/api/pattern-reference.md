# Pattern & Analysis API Reference

This page is the authoritative reference for the four **analysis** surfaces of
`libcpg` that operate over a built [`CodePropertyGraph`](graph-reference.md):

| Module | Cargo feature | Purpose | Primary entry points |
|--------|---------------|---------|----------------------|
| `pattern` | **always on** | [Subgraph isomorphism](../GLOSSARY.md#isomorphism--subgraph-isomorphism) (VF2) and [graph similarity](../GLOSSARY.md#similarity-metric) | `Vf2Matcher`, `GraphSimilarity` |
| `patterns` | `design-patterns` | [Gang-of-Four](../GLOSSARY.md#gang-of-four-gof) detection, DPML, rule/ML classification, OO metrics | `GofPatternDetector`, `PatternClassifier` |
| `algorithms` | `algorithm-detection` | [Algorithm-family](../GLOSSARY.md#algorithm-family) and [complexity](../GLOSSARY.md#complexity-class--big-o) detection | `DefaultAlgorithmDetector` |
| `gnn` | `gnn` | Message-passing [embeddings](../GLOSSARY.md#embedding) | `CpgGnn` |

> **`pattern` (singular) and `patterns` (plural) are different modules.**
> `pattern` is the always-on VF2 / similarity toolkit. `patterns` is the
> feature-gated GoF layer *built on top of it*. Do not conflate them.

Term definitions are in the [Glossary](../GLOSSARY.md); graph and builder types
are in the [Graph](graph-reference.md) and [Builder](builder-reference.md)
references.

### Module paths at a glance

```rust
// pattern:: — re-exported at the crate root:
use libcpg::{PatternMatch, SubgraphMatcher};
// …the rest of pattern:: needs the full path:
use libcpg::pattern::{
    Vf2Matcher, Vf2State, GraphSimilarity, SimilarityMetric,
    PatternTemplate, NodeConstraint, EdgeConstraint,
    NodeKindMatcher, NodeKindTag, EdgeKindMatcher,
};

// patterns:: (feature "design-patterns"):
use libcpg::patterns::{PatternDetector, GofPatternDetector, GofPattern, PatternClassifier};
use libcpg::patterns::design::{
    GofCategory, build_pattern_cpg, build_pattern_template,
    DpmlTemplate, DpmlRole, DpmlConstraint, DpmlError, PatternMetrics,
};
use libcpg::patterns::classification::{ClassificationMode, FeatureVector};

// algorithms:: (feature "algorithm-detection"):
use libcpg::algorithms::{
    AlgorithmDetector, DetectedAlgorithm, AlgorithmSignature,
    ComplexityEstimate, ComplexityClass, AlgorithmFamily,
};
use libcpg::algorithms::detection::DefaultAlgorithmDetector;

// gnn:: (feature "gnn"):
use libcpg::GraphNeuralNetwork;                 // trait, re-exported at root
use libcpg::gnn::{CpgGnn, NodeEmbedding, SubgraphEmbedding};
```

---

## `pattern::` — subgraph matching & similarity (always on)

### `SubgraphMatcher` trait

The matching contract. `find_matches` is required; `find_matches_limited`,
`contains_pattern`, and `algorithm_name` are provided.

```rust
pub trait SubgraphMatcher: Send + Sync {
    fn find_matches(&self, pattern: &CodePropertyGraph, target: &CodePropertyGraph) -> Vec<PatternMatch>;
    fn find_matches_limited(&self, pattern: &CodePropertyGraph, target: &CodePropertyGraph, limit: usize) -> Vec<PatternMatch> { /* provided: truncates find_matches */ }
    fn contains_pattern(&self, pattern: &CodePropertyGraph, target: &CodePropertyGraph) -> bool { /* provided */ }
    fn algorithm_name(&self) -> &str;
}
```

Both `SubgraphMatcher` and `PatternMatch` are re-exported at the crate root.

### `PatternMatch`

The result of a match — a mapping from pattern nodes to target nodes plus
metadata.

```rust
pub struct PatternMatch {
    pub pattern_name: String,
    pub confidence: f64,
    pub node_mapping: FxHashMap<NodeId, NodeId>,   // pattern node → target node
    pub root: NodeId,                              // a target node
    pub metadata: FxHashMap<String, String>,
}
```

| Method | Signature | Notes |
|--------|-----------|-------|
| `new` | `fn new(pattern_name: impl Into<String>, root: NodeId, confidence: f64) -> Self` | Constructor. |
| `with_mapping` | `fn with_mapping(self, pattern_node: NodeId, target_node: NodeId) -> Self` | Builder: add a mapping. |
| `with_metadata` | `fn with_metadata(self, key: impl Into<String>, value: impl Into<String>) -> Self` | Builder: add metadata. |
| `matched_nodes` | `fn matched_nodes(&self) -> impl Iterator<Item = NodeId> + '_` | Target node ids (the mapping's values). |
| `match_size` | `fn match_size(&self) -> usize` | Number of mapped nodes. |

`confidence` is a score in $`[0, 1]`$.

### `Vf2Matcher`

[VF2](../GLOSSARY.md#vf2) subgraph isomorphism [[1]](#references). All three
configuration fields are private; use the builder methods.

```rust
pub struct Vf2Matcher { /* private: strict_kinds, strict_edges, max_matches */ }
```

| Method | Signature | Notes |
|--------|-----------|-------|
| `new` | `fn new() -> Self` | Relaxed matching, unlimited matches. |
| `with_strict_kinds` | `fn with_strict_kinds(self, strict: bool) -> Self` | Require exact node-kind tags. |
| `with_strict_edges` | `fn with_strict_edges(self, strict: bool) -> Self` | Require exact edge kinds. |
| `with_max_matches` | `fn with_max_matches(self, max: usize) -> Self` | Cap the result; `0` means unlimited. |
| `find_matches` | *(via `SubgraphMatcher`)* `fn find_matches(&self, pattern, target) -> Vec<PatternMatch>` | Find all embeddings. |

**Relaxed vs strict.** By default (`strict_kinds = false`, `strict_edges =
false`), two nodes are compatible when they fall in the same category
(declaration / expression / statement) or share a
[`NodeKindTag`](#patterntemplate--constraints), and two edges are compatible when they share an
AST/CFG/DFG/call family. Turning strictness on requires exact tag/kind equality.
Relaxed matching is what powers GoF detection (below). Worst-case cost is
$`O(N!\,N)`$ in the pattern size $`N`$, but feasibility pruning makes
it practical on the sparse graphs typical of code.

![A path pattern matched against a diamond target graph](../diagrams/vf2-pattern-target.svg)

*Figure — a 3-node path pattern `p0 → p1 → p2` matched against a diamond target `A → {B, C} → D` has exactly two embeddings (`A-B-D` and `A-C-D`); finding both requires correct mid-search backtracking. Source: [`diagrams/vf2-pattern-target.dot`](../diagrams/vf2-pattern-target.dot).*

```rust
// requires: no features (pattern:: is always on)
use libcpg::pattern::Vf2Matcher;
use libcpg::SubgraphMatcher;

let matches = Vf2Matcher::new()
    .with_strict_kinds(true)      // exact node-kind tags
    .with_max_matches(0)          // 0 = unlimited
    .find_matches(&pattern_cpg, &target_cpg);

for m in &matches {
    println!("matched {} nodes rooted at {:?}", m.match_size(), m.root);
}
```

The inline regression `test_multi_embedding_backtracking` (`src/pattern/vf2.rs`)
verifies that the diamond above yields **exactly two** embeddings — the matcher
finds every embedding, not just the first.

### `Vf2State`

The explicit VF2 search state, exposed for advanced callers who want to drive or
inspect the state-space search. Most callers use `Vf2Matcher` and never touch it.

| Method | Signature |
|--------|-----------|
| `new` | `fn new(pattern: &'a CodePropertyGraph, target: &'a CodePropertyGraph) -> Self` |
| `is_complete` | `fn is_complete(&self) -> bool` |
| `candidate_pairs` | `fn candidate_pairs(&self) -> Vec<(NodeId, NodeId)>` |
| `push_mapping` | `fn push_mapping(&mut self, pattern_node: NodeId, target_node: NodeId)` |
| `pop_mapping` | `fn pop_mapping(&mut self)` |
| `to_pattern_match` | `fn to_pattern_match(&self) -> PatternMatch` |

`push_mapping`/`pop_mapping` maintain an explicit push-order stack so backtracking
restores the mapping and [terminal sets](../GLOSSARY.md#terminal-set-vf2) exactly.

### `PatternTemplate` & constraints

A declarative alternative to hand-building a pattern CPG: describe nodes and
edges by *constraint*, then compile to a CPG for matching.

```rust
pub struct PatternTemplate {
    pub name: String,
    pub description: String,
    pub node_constraints: Vec<NodeConstraint>,
    pub edge_constraints: Vec<EdgeConstraint>,
    pub min_confidence: f64,   // default 0.8
}
```

| `PatternTemplate` method | Signature |
|--------------------------|-----------|
| `new` | `fn new(name: impl Into<String>, description: impl Into<String>) -> Self` |
| `with_node` | `fn with_node(self, constraint: NodeConstraint) -> Self` |
| `with_edge` | `fn with_edge(self, constraint: EdgeConstraint) -> Self` |
| `with_min_confidence` | `fn with_min_confidence(self, confidence: f64) -> Self` |
| `to_pattern_graph` | `fn to_pattern_graph(&self) -> CodePropertyGraph` |

**`NodeConstraint`** — `index: usize`, `kind: Option<NodeKindMatcher>`,
`name_pattern: Option<String>`, `properties: FxHashMap<String, String>`.
Builders: `new(index)`, `with_kind(NodeKindMatcher)`,
`with_name_pattern(impl Into<String>)`, `with_property(key, value)`.

**`NodeKindMatcher`** — how a node constraint matches a kind. Method
`matches(&self, kind: &CpgNodeKind) -> bool`.

| Variant | Matches |
|---------|---------|
| `Exact(NodeKindTag)` | one specific tag |
| `AnyOf(Vec<NodeKindTag>)` | any listed tag |
| `AnyDeclaration` | any declaration kind |
| `AnyExpression` | any expression kind |
| `AnyStatement` | any statement kind |
| `Any` | anything |

**`NodeKindTag`** — a flat, 29-variant tag for node kinds: `Root`, `Module`,
`Class`, `Struct`, `Enum`, `Trait`, `Impl`, `Function`, `Parameter`, `Block`,
`Variable`, `Field`, `Return`, `If`, `While`, `For`, `Loop`, `Match`, `BinaryOp`,
`UnaryOp`, `Assignment`, `Call`, `MemberAccess`, `IndexAccess`, `Identifier`,
`Literal`, `Lambda`, `Import`, `Unknown`. Methods: `from_kind(&CpgNodeKind) ->
NodeKindTag` and `matches(&self, &CpgNodeKind) -> bool`.

**`EdgeConstraint`** — `source: usize`, `target: usize`,
`kind: Option<EdgeKindMatcher>`. Builders: `new(source, target)`,
`with_kind(EdgeKindMatcher)`.

**`EdgeKindMatcher`** — `AnyAst`, `AnyCfg`, `AnyDfg`, `AnyCall`, `Any`. Method
`matches(&self, kind: &CpgEdgeKind) -> bool`. (There is no `Exact`/`AnyOf` edge
matcher — edges match by *family*.)

```rust
// requires: no features
use libcpg::pattern::{PatternTemplate, NodeConstraint, EdgeConstraint};
use libcpg::pattern::{NodeKindMatcher, NodeKindTag, EdgeKindMatcher};

let template = PatternTemplate::new("Singleton", "class + field")
    .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
    .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Field)))
    .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
    .with_min_confidence(0.9);

let pattern_cpg = template.to_pattern_graph(); // feed to Vf2Matcher::find_matches
```

### `GraphSimilarity` & `SimilarityMetric`

Whole-graph [similarity](../GLOSSARY.md#similarity-metric) scoring. Fields are
private; the defaults are `metric = Jaccard`, `structural_weight = 0.7`,
`label_weight = 0.3`.

```rust
pub struct GraphSimilarity { /* private: metric, structural_weight, label_weight */ }
```

| Method | Signature | Notes |
|--------|-----------|-------|
| `new` | `fn new() -> Self` | Jaccard, weights `0.7`/`0.3`. |
| `with_metric` | `fn with_metric(self, metric: SimilarityMetric) -> Self` | Choose the metric. |
| `with_structural_weight` | `fn with_structural_weight(self, weight: f64) -> Self` | Structural weight. |
| `with_label_weight` | `fn with_label_weight(self, weight: f64) -> Self` | Label weight. |
| `similarity` | `fn similarity(&self, g1: &CodePropertyGraph, g2: &CodePropertyGraph) -> f64` | Score in $`[0, 1]`$. |

**`SimilarityMetric`** — `Jaccard` (the `Default`), `Cosine`, `WeisfeilerLehman`,
`GraphEdit`. The default [Jaccard index](../GLOSSARY.md#jaccard-similarity) over
node-kind multisets is

```math
J(A, B) = \frac{|A \cap B|}{|A \cup B|}
```

`WeisfeilerLehman` refines node labels over 3 iterations; `GraphEdit`
approximates edit distance and blends structural (`structural_weight`) and label
(`label_weight`) components. Only `GraphEdit` currently consults the two weights.

```rust
// requires: no features
use libcpg::pattern::{GraphSimilarity, SimilarityMetric};

let score = GraphSimilarity::new()
    .with_metric(SimilarityMetric::WeisfeilerLehman)
    .similarity(&cpg_a, &cpg_b);
```

---

## `patterns::` — Gang-of-Four detection (feature `design-patterns`)

### `PatternDetector` trait

```rust
pub trait PatternDetector: Send + Sync {
    fn detect(&self, cpg: &CodePropertyGraph) -> Vec<PatternMatch>;
    fn supported_patterns(&self) -> &[&str];
}
```

### `GofPatternDetector`

Detects the 23 GoF patterns structurally. Private fields: `min_confidence`
(default `0.7`), `patterns_to_detect` (empty ⇒ all).

| Method | Signature | Notes |
|--------|-----------|-------|
| `new` | `fn new() -> Self` | Detect all patterns, `min_confidence = 0.7`. |
| `with_min_confidence` | `fn with_min_confidence(self, confidence: f64) -> Self` | Keep matches at/above this. |
| `with_patterns` | `fn with_patterns(self, patterns: Vec<GofPattern>) -> Self` | Restrict to specific patterns. |
| `detect` | *(via `PatternDetector`)* `fn detect(&self, cpg) -> Vec<PatternMatch>` | Run detection. |

`detect` runs a **relaxed** [`Vf2Matcher`](#vf2matcher) (`strict_kinds = false`,
`strict_edges = false`) against each pattern's template, scores every match with
a completeness-vs-template [confidence](../GLOSSARY.md#confidence-pattern-match),
keeps those at/above `min_confidence`, attaches `category` and `pattern_type =
"GoF"` metadata, and sorts by confidence descending.

```rust
// requires: features = ["design-patterns"]
use libcpg::patterns::{GofPatternDetector, GofPattern, PatternDetector};

let detector = GofPatternDetector::new()
    .with_patterns(vec![GofPattern::Singleton, GofPattern::FactoryMethod])
    .with_min_confidence(0.75);

for m in detector.detect(&cpg) {
    let category = m.metadata.get("category").map(String::as_str).unwrap_or("");
    println!("{} ({}) — {:.0}%", m.pattern_name, category, m.confidence * 100.0);
}
```

### `GofPattern` & `GofCategory`

The 23 [Gang-of-Four](../GLOSSARY.md#gang-of-four-gof) patterns [[3]](#references),
grouped into three categories. The creational factory variant is **`FactoryMethod`**
— there is no `Factory` variant.

![The Gang-of-Four taxonomy: 5 creational, 7 structural, 11 behavioral](../diagrams/gof-taxonomy.svg)

*Figure — the 23 GoF patterns by category. Source: [`diagrams/gof-taxonomy.puml`](../diagrams/gof-taxonomy.puml).*

| Category (`GofCategory`) | `GofPattern` variants |
|--------------------------|-----------------------|
| `Creational` (5) | `AbstractFactory`, `Builder`, `FactoryMethod`, `Prototype`, `Singleton` |
| `Structural` (7) | `Adapter`, `Bridge`, `Composite`, `Decorator`, `Facade`, `Flyweight`, `Proxy` |
| `Behavioral` (11) | `ChainOfResponsibility`, `Command`, `Interpreter`, `Iterator`, `Mediator`, `Memento`, `Observer`, `State`, `Strategy`, `TemplateMethod`, `Visitor` |

`GofPattern` methods: `name(&self) -> &'static str` and `category(&self) ->
GofCategory`. `GofCategory` (`Creational`, `Structural`, `Behavioral`) has
`name(&self) -> &'static str`.

### Template builders

Two free functions (in `libcpg::patterns::design`) produce the pattern graph and
template a detector matches against:

```rust
pub fn build_pattern_cpg(pattern: GofPattern) -> CodePropertyGraph;
pub fn build_pattern_template(pattern: GofPattern) -> PatternTemplate;
```

### DPML — declarative pattern templates

[DPML](../GLOSSARY.md#dpml-design-pattern-markup-language) declares a pattern as
roles + relationships in YAML or TOML, compiling to a
[`PatternTemplate`](#patterntemplate--constraints).

```rust
pub struct DpmlTemplate {
    pub name: String,
    pub description: String,
    pub category: String,
    pub roles: Vec<DpmlRole>,
    pub relationships: Vec<DpmlConstraint>,   // serde alias: "constraints"
}
pub struct DpmlRole { pub id: String, pub role_type: String, pub cardinality: String }
pub struct DpmlConstraint { pub source: String, pub target: String, pub constraint_type: String }
```

| `DpmlTemplate` method | Signature | Notes |
|-----------------------|-----------|-------|
| `new` | `fn new(name: impl Into<String>) -> Self` | Empty template. |
| `with_description` / `with_category` | `fn(self, impl Into<String>) -> Self` | Set metadata. |
| `with_role` | `fn with_role(self, role: DpmlRole) -> Self` | Add a role. |
| `with_relationship` | `fn with_relationship(self, constraint: DpmlConstraint) -> Self` | Add a relationship. |
| `parse` | `fn parse(content: &str) -> Result<Self, DpmlError>` | Auto-detect YAML then TOML *(design-patterns)*. |
| `parse_yaml` / `parse_toml` | `fn(content: &str) -> Result<Self, DpmlError>` | Explicit parsers *(design-patterns)*. |
| `validate` | `fn validate(&self) -> Result<(), DpmlError>` | Check role ids and relationship references. |
| `to_pattern_template` | `fn to_pattern_template(&self) -> Result<PatternTemplate, DpmlError>` | Compile (validates first). |

`DpmlRole::new(id, role_type)` / `with_cardinality`; `DpmlConstraint::new(source,
target, constraint_type)`. **`DpmlError`** has 8 variants: `FeatureDisabled`,
`YamlError`, `TomlError`, `MissingField`, `InvalidRole`, `DuplicateRole`,
`InvalidRelationship`, `InvalidSyntax` (each wraps a `String`); it implements
`std::error::Error` and `Display`.

### `PatternMetrics`

Object-oriented cohesion/coupling metrics (Chidamber & Kemerer
[[5]](#references)) useful as pattern evidence.

```rust
pub struct PatternMetrics {
    pub class_count: usize,
    pub interface_count: usize,
    pub inheritance_count: usize,
    pub composition_count: usize,
    pub avg_methods_per_class: f64,
    pub cohesion: f64,   // LCOM (higher ⇒ less cohesive)
    pub coupling: f64,   // CBO (distinct coupled classes, averaged)
}
```

Computed with the associated function `PatternMetrics::compute(cpg:
&CodePropertyGraph) -> Self`. `cohesion` is the average
[LCOM](../GLOSSARY.md#gang-of-four-gof) (Lack of Cohesion of Methods) over classes
and `coupling` the average CBO (Coupling Between Objects).

### Classification — `PatternClassifier`

A feature-vector alternative to template matching (in
`libcpg::patterns::classification`). Private fields: `min_confidence` (default
`0.7`), `mode` (default `RuleBased`).

| Method | Signature | Notes |
|--------|-----------|-------|
| `new` | `fn new() -> Self` | `RuleBased`, `min_confidence = 0.7`. |
| `with_min_confidence` | `fn with_min_confidence(self, confidence: f64) -> Self` | Threshold. |
| `with_mode` | `fn with_mode(self, mode: ClassificationMode) -> Self` | Pick a mode. |
| `classify` | `fn classify(&self, cpg: &CodePropertyGraph) -> Vec<PatternMatch>` | Score each class. |
| `supported_patterns` | `fn supported_patterns(&self) -> &[&str]` | `Singleton`, `Factory`, `Observer`, `Strategy`, `Decorator`. |

**`ClassificationMode`** — `RuleBased` (the `Default`), `MachineLearning` (uses a
trained model under `ml-linfa`; falls back to rules otherwise), `Hybrid` (merges
both, boosting agreement).

**`FeatureVector`** — an 11-field per-class summary
([feature vector](../GLOSSARY.md#feature-vector-classification)):

```rust
pub struct FeatureVector {
    pub method_count: usize,
    pub field_count: usize,
    pub method_field_ratio: f64,
    pub inheritance_depth: usize,
    pub interface_count: usize,
    pub static_method_count: usize,
    pub has_private_constructor: bool,
    pub factory_method_count: usize,
    pub observer_method_count: usize,
    pub interface_field_count: usize,
    pub is_decorator_candidate: bool,
}
```

`fn to_array(&self) -> [f64; 12]` flattens the 11 fields plus one derived feature
(the static-to-total method ratio) into a length-12 array for ML models.

> The classifier's rule-based label for a creational factory is the string
> `"Factory"`, which is distinct from the [`GofPattern::FactoryMethod`](#gofpattern--gofcategory)
> enum variant used by the template detector. The two detection paths use
> independent vocabularies.

---

## `algorithms::` — algorithm & complexity detection (feature `algorithm-detection`)

### `AlgorithmDetector` trait

Detection is **per function** — every method takes a `function: NodeId`.

```rust
pub trait AlgorithmDetector: Send + Sync {
    fn detect(&self, cpg: &CodePropertyGraph, function: NodeId) -> Vec<DetectedAlgorithm>;
    fn supported_families(&self) -> &[AlgorithmFamily];
}
```

### `DefaultAlgorithmDetector`

The shipped detector. Private fields: `min_confidence` (default `0.5`), plus a
control-flow and a complexity analyzer.

| Method | Signature | Notes |
|--------|-----------|-------|
| `new` | `fn new() -> Self` | `min_confidence = 0.5`. |
| `with_min_confidence` | `fn with_min_confidence(self, confidence: f64) -> Self` | Threshold. |
| `detect` | *(via `AlgorithmDetector`)* `fn detect(&self, cpg, function) -> Vec<DetectedAlgorithm>` | Analyze one function. |

`detect` runs loop/recursion analysis, estimates time complexity, then tries five
family routines (sorting, searching, graph, dynamic-programming,
divide-and-conquer), returning the survivors sorted by `confidence` **descending**
and filtered at `min_confidence`.

> **Honesty.** `supported_families()` lists `Greedy`, but there is no active
> greedy detector — only the five families above are actually detected. Detection
> is *heuristic* (name/shape matching), not a proof of identity. Note the default
> threshold here is `0.5`, distinct from the GoF detector's `0.7`.

```rust
// requires: features = ["algorithm-detection"]
use libcpg::algorithms::detection::DefaultAlgorithmDetector;
use libcpg::algorithms::AlgorithmDetector;

let detector = DefaultAlgorithmDetector::new();
for algo in detector.detect(&cpg, function_id) {
    println!("{} — {:.0}%", algo.family, algo.confidence * 100.0);
}
```

### `DetectedAlgorithm`

```rust
pub struct DetectedAlgorithm {
    pub family: AlgorithmFamily,
    pub name: Option<String>,        // e.g. Some("Binary Search")
    pub function: NodeId,
    pub key_nodes: Vec<NodeId>,
    pub signature: AlgorithmSignature,
    pub confidence: f64,
}
```

Builders: `new(family, function, confidence)`, `with_name`, `with_key_node`,
`with_signature`.

### `AlgorithmSignature` & `ComplexityEstimate`

```rust
pub struct AlgorithmSignature {
    pub loop_structure: Option<LoopStructure>,
    pub recursion_pattern: Option<RecursionPattern>,
    pub time_complexity: Option<ComplexityEstimate>,
    pub space_complexity: Option<ComplexityEstimate>,
    pub feature_vector: Vec<f32>,
}
pub struct ComplexityEstimate {
    pub class: ComplexityClass,
    pub confidence: f64,
    pub justification: String,
}
```

`AlgorithmSignature::new()` plus `with_loop_structure`, `with_recursion`,
`with_time_complexity`, `with_space_complexity`.

### `ComplexityClass`

The [Big-O ladder](../GLOSSARY.md#complexity-class--big-o).

![The complexity-class ladder from constant to factorial](../diagrams/complexity-ladder.svg)

*Figure — the ComplexityClass ladder, cheapest to most expensive. Source: [`diagrams/complexity-ladder.dot`](../diagrams/complexity-ladder.dot).*

| Variant | Meaning | `as_str()` returns |
|---------|---------|--------------------|
| `Constant` | $`O(1)`$ | `O(1)` |
| `Logarithmic` | $`O(\log n)`$ | `O(log n)` |
| `Linear` | $`O(n)`$ | `O(n)` |
| `Linearithmic` | $`O(n \log n)`$ | `O(n log n)` |
| `Quadratic` | $`O(n^2)`$ | `O(n²)` |
| `Cubic` | $`O(n^3)`$ | `O(n³)` |
| `Polynomial(u32)` | $`O(n^k)`$ | `O(n^k)` |
| `Exponential` | $`O(2^n)`$ | `O(2^n)` |
| `Factorial` | $`O(n!)`$ | `O(n!)` |
| `Unknown` | undetermined | `Unknown` |

`Unknown` is the `Default`. Methods: `as_str(&self) -> &'static str` (the literal
strings above — note the source uses unicode superscripts for `Quadratic`/`Cubic`)
and `is_better_than(&self, other: &Self) -> bool` (a smaller class is "better").

> **Honesty.** The shipped complexity analyzer caps non-divide-and-conquer
> recursion at `Exponential` and in practice **never emits `Factorial`** — the
> variant exists but is not produced. Treat all estimates as heuristic.

### `AlgorithmFamily`

Fourteen structural [families](../GLOSSARY.md#algorithm-family): `Sorting`,
`Searching`, `GraphTraversal`, `ShortestPath`, `MinimumSpanningTree`,
`DynamicProgramming`, `DivideAndConquer`, `Greedy`, `Backtracking`,
`StringMatching`, `TreeAlgorithm`, `Hashing`, `Mathematical`, `Other`. Methods:
`name(&self) -> &'static str` and `typical_complexity(&self) -> &'static str`
(e.g. `Sorting` returns `O(n log n)`); implements `Display` via `name()`. As
noted above, not every listed family has an active detector.

---

## `gnn::` — graph neural network embeddings (feature `gnn`)

### `GraphNeuralNetwork` trait

Re-exported at the crate root as `libcpg::GraphNeuralNetwork`. The two embedding
accessors exist only with the `gnn` feature (they return `ndarray` types).

```rust
pub trait GraphNeuralNetwork: Send + Sync {
    fn propagate(&mut self, iterations: usize);
    #[cfg(feature = "gnn")] fn node_embedding(&self, node: NodeId) -> Option<Array1<f32>>;
    #[cfg(feature = "gnn")] fn subgraph_embedding(&self, nodes: &[NodeId]) -> Array1<f32>;
    fn embedding_dim(&self) -> usize;
    fn is_initialized(&self) -> bool;
    fn reset(&mut self);
}
```

### `CpgGnn`

The message-passing implementation. It **owns** the CPG (by value — not `Arc`,
and there is no separate `GnnConfig`).

| Method | Signature | Notes |
|--------|-----------|-------|
| `new` | `fn new(cpg: CodePropertyGraph) -> Self` | Takes ownership; `embedding_dim = 128`, `num_layers = 3`, `dropout = 0.1`. |
| `with_embedding_dim` | `fn with_embedding_dim(self, dim: usize) -> Self` | Set embedding width. |
| `with_num_layers` | `fn with_num_layers(self, layers: usize) -> Self` | Set layer count. |
| `with_dropout` | `fn with_dropout(self, dropout: f32) -> Self` | Set dropout rate. |
| `cpg` | `fn cpg(&self) -> &CodePropertyGraph` | Borrow the owned graph. |

`propagate` initializes each node vector from a 16-dimensional node-kind one-hot
plus small random noise, then for each iteration
[aggregates](../GLOSSARY.md#message-passing) the mean of a node's AST
(children + parent), CFG (successors + predecessors), and DFG (successors +
predecessors) neighbours and applies a
[ReLU](../GLOSSARY.md#relu) nonlinearity:

```math
h_v^{(k)} = \mathrm{ReLU}\!\left( \mathrm{mean}\left( \{ h_u^{(k-1)} : u \in \mathcal{N}(v) \} \cup \{ h_v^{(k-1)} \} \right) \right)
```

Scarselli et al. [[2]](#references) introduced the GNN model this follows.

```rust
// requires: features = ["gnn"]
use libcpg::gnn::CpgGnn;
use libcpg::GraphNeuralNetwork;

let mut gnn = CpgGnn::new(cpg)          // moves `cpg` into the GNN
    .with_embedding_dim(64)
    .with_num_layers(2);
gnn.propagate(3);
let e = gnn.node_embedding(node_id);    // Option<Array1<f32>>
```

### `NodeEmbedding` / `SubgraphEmbedding` / `AggregationMethod`

```rust
pub struct NodeEmbedding {
    pub node_id: NodeId,
    #[cfg(feature = "gnn")] pub vector: Array1<f32>,   // serde-skipped
    pub dim: usize,
}
pub struct SubgraphEmbedding {
    pub node_ids: Vec<NodeId>,
    #[cfg(feature = "gnn")] pub vector: Array1<f32>,   // serde-skipped
    pub dim: usize,
    pub aggregation: AggregationMethod,
}
```

Both carry a `dim` and (under `gnn`) a `vector`. With the `serde` feature the
`vector` field is **skipped** during serialization (recomputed at load); only ids
and `dim`/`aggregation` round-trip.

| Type | Methods |
|------|---------|
| `NodeEmbedding` | `new(node_id, vector)` *(gnn)*, `norm() -> f32` *(gnn)*, `cosine_similarity(&other) -> f32` *(gnn)* |
| `SubgraphEmbedding` | `new(node_ids, vector, aggregation)` *(gnn)*, `norm() -> f32` *(gnn)*, `cosine_similarity(&other) -> f32` *(gnn)*, `node_count() -> usize` |

[Cosine similarity](../GLOSSARY.md#cosine-similarity) returns `0.0` when
dimensions differ or a norm is zero.

**`AggregationMethod`** — the enum stored in `SubgraphEmbedding::aggregation`:
`Mean` (the `Default`), `Sum`, `Max`, `Attention`, `Hierarchical`.

> **Honesty.** Message passing uses **`Mean`** aggregation only; `Attention` and
> `Hierarchical` are reserved placeholders, not yet wired. The `gpu` feature is
> likewise reserved (no code), and no SIMD path exists. `AggregationMethod` is
> not separately re-exported at `libcpg::gnn`; it is reached through the
> `SubgraphEmbedding::aggregation` field.

---

## See also

- [Graph reference](graph-reference.md) — the node/edge model matching operates
  over.
- [Builder reference](builder-reference.md) — producing the CPG these analyses
  consume.
- [Glossary](../GLOSSARY.md) — VF2, similarity metrics, GoF, complexity classes,
  GNN terms.
- Component guides: [patterns overview](../components/patterns/overview.md),
  [VF2 matching](../components/patterns/vf2-matching.md),
  [Gang of Four](../components/patterns/gang-of-four.md),
  [algorithms overview](../components/algorithms/overview.md),
  [complexity](../components/algorithms/complexity.md),
  [GNN overview](../components/gnn/overview.md),
  [embeddings](../components/gnn/embeddings.md).

---

## References

1. Cordella, L. P., Foggia, P., Sansone, C., Vento, M. (2004). *A (Sub)graph Isomorphism Algorithm for Matching Large Graphs.* IEEE TPAMI 26(10). DOI: [10.1109/TPAMI.2004.75](https://doi.org/10.1109/TPAMI.2004.75)
2. Scarselli, F., Gori, M., Tsoi, A. C., Hagenbuchner, M., Monfardini, G. (2009). *The Graph Neural Network Model.* IEEE Transactions on Neural Networks 20(1). DOI: [10.1109/TNN.2008.2005605](https://doi.org/10.1109/TNN.2008.2005605)
3. Gamma, E., Helm, R., Johnson, R., Vlissides, J. (1994). *Design Patterns: Elements of Reusable Object-Oriented Software.* Addison-Wesley. ISBN 978-0201633610 (no DOI).
4. Cormen, T. H., Leiserson, C. E., Rivest, R. L., Stein, C. (2009). *Introduction to Algorithms* (3rd ed.). MIT Press. ISBN 978-0262033848 (no DOI). *(Master Theorem.)*
5. Chidamber, S. R., Kemerer, C. F. (1994). *A Metrics Suite for Object Oriented Design.* IEEE Transactions on Software Engineering 20(6). DOI: [10.1109/32.295895](https://doi.org/10.1109/32.295895) *(LCOM/CBO.)*
