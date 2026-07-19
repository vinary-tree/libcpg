# Pattern API Reference

This reference documents the pattern detection and matching APIs in libcpg.

## Pattern Matching

### SubgraphMatcher Trait

Interface for subgraph matching algorithms.

```rust
pub trait SubgraphMatcher: Send + Sync {
    fn find_matches(
        &self,
        pattern: &CodePropertyGraph,
        target: &CodePropertyGraph,
    ) -> Vec<PatternMatch>;

    fn contains_pattern(
        &self,
        pattern: &CodePropertyGraph,
        target: &CodePropertyGraph,
    ) -> bool {
        !self.find_matches(pattern, target).is_empty()
    }

    fn find_matches_limited(
        &self,
        pattern: &CodePropertyGraph,
        target: &CodePropertyGraph,
        max: usize,
    ) -> Vec<PatternMatch>;
}
```

---

### Vf2Matcher

VF2 subgraph isomorphism implementation.

```rust
pub struct Vf2Matcher {
    strict_kinds: bool,
    strict_edges: bool,
    max_matches: Option<usize>,
}
```

#### Constructor

| Method | Returns | Description |
|--------|---------|-------------|
| `Vf2Matcher::new()` | `Vf2Matcher` | Create with defaults |

#### Configuration

| Method | Returns | Description |
|--------|---------|-------------|
| `with_strict_kinds(enabled: bool)` | `Self` | Require exact node kind matches |
| `with_strict_edges(enabled: bool)` | `Self` | Require exact edge kind matches |
| `with_max_matches(n: usize)` | `Self` | Stop after n matches |

#### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `find_matches(pattern, target)` | `Vec<PatternMatch>` | Find all matches |
| `contains_pattern(pattern, target)` | `bool` | Check if pattern exists |
| `find_matches_limited(pattern, target, max)` | `Vec<PatternMatch>` | Find up to max matches |

#### Example

```rust
use libcpg::pattern::{Vf2Matcher, SubgraphMatcher};

let matcher = Vf2Matcher::new()
    .with_strict_kinds(true)
    .with_max_matches(10);

let matches = matcher.find_matches(&pattern_cpg, &target_cpg);

for m in matches {
    println!("Match found at {:?}", m.root);
    println!("  Confidence: {:.2}", m.confidence);
    println!("  Mapped {} nodes", m.match_size());
}
```

---

### PatternMatch

Result of pattern matching.

```rust
pub struct PatternMatch {
    pub pattern_name: String,
    pub confidence: f64,
    pub node_mapping: FxHashMap<NodeId, NodeId>,
    pub root: NodeId,
    pub metadata: FxHashMap<String, String>,
}
```

| Method | Returns | Description |
|--------|---------|-------------|
| `match_size()` | `usize` | Number of mapped nodes |
| `matched_nodes()` | `impl Iterator<Item = NodeId>` | Target node IDs |
| `pattern_node_for(target: NodeId)` | `Option<NodeId>` | Reverse lookup |

---

### PatternTemplate

Declarative pattern definition.

```rust
pub struct PatternTemplate {
    name: String,
    description: String,
    node_constraints: Vec<NodeConstraint>,
    edge_constraints: Vec<EdgeConstraint>,
    min_confidence: f64,
}
```

#### Construction

| Method | Returns | Description |
|--------|---------|-------------|
| `PatternTemplate::new(name, description)` | `PatternTemplate` | Create template |
| `with_node(constraint: NodeConstraint)` | `Self` | Add node constraint |
| `with_edge(constraint: EdgeConstraint)` | `Self` | Add edge constraint |
| `with_min_confidence(c: f64)` | `Self` | Set minimum confidence |
| `to_pattern_graph()` | `CodePropertyGraph` | Convert to CPG |

#### Example

```rust
use libcpg::pattern::{PatternTemplate, NodeConstraint, EdgeConstraint};
use libcpg::pattern::{NodeKindMatcher, NodeKindTag, EdgeKindMatcher};

let template = PatternTemplate::new("Null Check", "Check for null before use")
    .with_node(
        NodeConstraint::new(0)
            .with_kind(NodeKindMatcher::Exact(NodeKindTag::If))
    )
    .with_node(
        NodeConstraint::new(1)
            .with_kind(NodeKindMatcher::Exact(NodeKindTag::BinaryOp))
    )
    .with_edge(
        EdgeConstraint::new(0, 1)
            .with_kind(EdgeKindMatcher::AnyAst)
    )
    .with_min_confidence(0.8);

let pattern_cpg = template.to_pattern_graph();
```

---

### NodeConstraint

Constraint on pattern nodes.

```rust
pub struct NodeConstraint {
    id: usize,
    kind_matcher: Option<NodeKindMatcher>,
    name_pattern: Option<String>,
    required: bool,
}
```

| Method | Returns | Description |
|--------|---------|-------------|
| `NodeConstraint::new(id: usize)` | `NodeConstraint` | Create constraint |
| `with_kind(matcher: NodeKindMatcher)` | `Self` | Set kind matcher |
| `with_name_pattern(pattern: &str)` | `Self` | Set regex for name |
| `optional()` | `Self` | Mark as optional |

---

### EdgeConstraint

Constraint on pattern edges.

```rust
pub struct EdgeConstraint {
    source_id: usize,
    target_id: usize,
    kind_matcher: Option<EdgeKindMatcher>,
    required: bool,
}
```

| Method | Returns | Description |
|--------|---------|-------------|
| `EdgeConstraint::new(src: usize, tgt: usize)` | `EdgeConstraint` | Create constraint |
| `with_kind(matcher: EdgeKindMatcher)` | `Self` | Set kind matcher |
| `optional()` | `Self` | Mark as optional |

---

### NodeKindMatcher

Flexible node kind matching.

```rust
pub enum NodeKindMatcher {
    Exact(NodeKindTag),
    AnyOf(Vec<NodeKindTag>),
    AnyDeclaration,
    AnyExpression,
    AnyStatement,
    Any,
}
```

| Variant | Description |
|---------|-------------|
| `Exact(tag)` | Match specific kind |
| `AnyOf(tags)` | Match any of the kinds |
| `AnyDeclaration` | Match any declaration |
| `AnyExpression` | Match any expression |
| `AnyStatement` | Match any statement |
| `Any` | Match anything |

---

### EdgeKindMatcher

Flexible edge kind matching.

```rust
pub enum EdgeKindMatcher {
    Exact(CpgEdgeKind),
    AnyOf(Vec<CpgEdgeKind>),
    AnyAst,
    AnyCfg,
    AnyDfg,
    Any,
}
```

| Variant | Description |
|---------|-------------|
| `Exact(kind)` | Match specific edge |
| `AnyOf(kinds)` | Match any of the kinds |
| `AnyAst` | Match any AST edge |
| `AnyCfg` | Match any CFG edge |
| `AnyDfg` | Match any DFG edge |
| `Any` | Match any edge |

---

### NodeKindTag

Simplified node kind for matching.

```rust
pub enum NodeKindTag {
    // Declarations
    Module, Class, Struct, Enum, Trait, Function, Variable, Field, Parameter,

    // Expressions
    BinaryOp, UnaryOp, Call, MemberAccess, IndexAccess, Identifier, Literal, Lambda,

    // Statements
    Return, If, While, For, Loop, Match, Break, Continue, Block,

    // Control Flow
    Entry, Exit,

    // Other
    Unknown,
}
```

| Method | Returns | Description |
|--------|---------|-------------|
| `from_kind(kind: &CpgNodeKind)` | `NodeKindTag` | Convert from full kind |
| `is_declaration()` | `bool` | Is declaration category |
| `is_expression()` | `bool` | Is expression category |
| `is_statement()` | `bool` | Is statement category |

---

## Design Pattern Detection

### PatternDetector Trait

Interface for design pattern detection.

```rust
pub trait PatternDetector: Send + Sync {
    fn detect(&self, cpg: &CodePropertyGraph) -> Vec<PatternMatch>;

    fn detect_in_function(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
    ) -> Vec<PatternMatch>;
}
```

---

### GofPatternDetector

Gang of Four pattern detector.

```rust
pub struct GofPatternDetector {
    patterns: Vec<GofPattern>,
    min_confidence: f64,
}
```

#### Constructor

| Method | Returns | Description |
|--------|---------|-------------|
| `GofPatternDetector::new()` | `GofPatternDetector` | Detect all patterns |

#### Configuration

| Method | Returns | Description |
|--------|---------|-------------|
| `with_patterns(patterns: Vec<GofPattern>)` | `Self` | Filter to specific patterns |
| `with_min_confidence(c: f64)` | `Self` | Set minimum confidence |

#### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `detect(cpg: &CodePropertyGraph)` | `Vec<PatternMatch>` | Detect all matches |

#### Example

```rust
use libcpg::patterns::{GofPatternDetector, GofPattern, PatternDetector};

let detector = GofPatternDetector::new()
    .with_patterns(vec![
        GofPattern::Singleton,
        GofPattern::Factory,
        GofPattern::Observer,
    ])
    .with_min_confidence(0.75);

let matches = detector.detect(&cpg);

for m in matches {
    let category = m.metadata.get("category").unwrap();
    println!("{} ({}) - confidence: {:.0}%",
             m.pattern_name, category, m.confidence * 100.0);
}
```

---

### GofPattern

All 23 Gang of Four patterns.

```rust
pub enum GofPattern {
    // Creational (5)
    AbstractFactory,
    Builder,
    FactoryMethod,
    Prototype,
    Singleton,

    // Structural (7)
    Adapter,
    Bridge,
    Composite,
    Decorator,
    Facade,
    Flyweight,
    Proxy,

    // Behavioral (11)
    ChainOfResponsibility,
    Command,
    Interpreter,
    Iterator,
    Mediator,
    Memento,
    Observer,
    State,
    Strategy,
    TemplateMethod,
    Visitor,
}
```

| Method | Returns | Description |
|--------|---------|-------------|
| `category()` | `GofCategory` | Get pattern category |
| `name()` | `&'static str` | Pattern name |
| `description()` | `&'static str` | Brief description |

---

### GofCategory

Pattern categories.

```rust
pub enum GofCategory {
    Creational,
    Structural,
    Behavioral,
}
```

---

## Graph Similarity

### GraphSimilarity

Computes similarity between graphs.

```rust
pub struct GraphSimilarity {
    metric: SimilarityMetric,
    structural_weight: f64,
    label_weight: f64,
}
```

#### Constructor

| Method | Returns | Description |
|--------|---------|-------------|
| `GraphSimilarity::new()` | `GraphSimilarity` | Create with defaults |

#### Configuration

| Method | Returns | Description |
|--------|---------|-------------|
| `with_metric(metric: SimilarityMetric)` | `Self` | Set similarity metric |
| `with_structural_weight(w: f64)` | `Self` | Weight for structure |
| `with_label_weight(w: f64)` | `Self` | Weight for labels |

#### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `similarity(a, b)` | `f64` | Compute similarity (0.0-1.0) |

#### Example

```rust
use libcpg::pattern::{GraphSimilarity, SimilarityMetric};

let similarity = GraphSimilarity::new()
    .with_metric(SimilarityMetric::WeisfeilerLehman)
    .with_structural_weight(0.7)
    .with_label_weight(0.3);

let score = similarity.similarity(&cpg1, &cpg2);
println!("Similarity: {:.2}", score);
```

---

### SimilarityMetric

Available similarity metrics.

```rust
pub enum SimilarityMetric {
    Jaccard,
    Cosine,
    WeisfeilerLehman,
    GraphEdit,
}
```

| Metric | Description | Complexity |
|--------|-------------|------------|
| `Jaccard` | Set-based node type comparison | O(n) |
| `Cosine` | Feature vector cosine similarity | O(n) |
| `WeisfeilerLehman` | Graph kernel with iterated labels | O(n × k) |
| `GraphEdit` | Approximate edit distance | O(n²) |

---

## Algorithm Detection

### AlgorithmDetector Trait

Interface for algorithm detection.

```rust
pub trait AlgorithmDetector: Send + Sync {
    fn detect(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
    ) -> Vec<DetectedAlgorithm>;
}
```

---

### DefaultAlgorithmDetector

Standard algorithm detector.

```rust
pub struct DefaultAlgorithmDetector {
    min_confidence: f64,
    families: Option<Vec<AlgorithmFamily>>,
}
```

#### Configuration

| Method | Returns | Description |
|--------|---------|-------------|
| `DefaultAlgorithmDetector::new()` | `Self` | Create detector |
| `with_min_confidence(c: f64)` | `Self` | Set minimum confidence |
| `with_families(families: Vec<AlgorithmFamily>)` | `Self` | Filter to families |

---

### DetectedAlgorithm

Result of algorithm detection.

```rust
pub struct DetectedAlgorithm {
    pub family: AlgorithmFamily,
    pub name: Option<String>,
    pub function: NodeId,
    pub key_nodes: Vec<NodeId>,
    pub signature: AlgorithmSignature,
    pub confidence: f64,
}
```

---

### AlgorithmSignature

Structural characteristics of an algorithm.

```rust
pub struct AlgorithmSignature {
    pub loop_structure: Option<LoopStructure>,
    pub recursion_pattern: Option<RecursionPattern>,
    pub time_complexity: Option<ComplexityEstimate>,
    pub space_complexity: Option<ComplexityEstimate>,
    pub feature_vector: Vec<f32>,
}
```

---

### ComplexityEstimate

Estimated complexity.

```rust
pub struct ComplexityEstimate {
    pub class: ComplexityClass,
    pub confidence: f64,
    pub justification: String,
}
```

---

### ComplexityClass

Big-O complexity classes.

```rust
pub enum ComplexityClass {
    Constant,       // O(1)
    Logarithmic,    // O(log n)
    Linear,         // O(n)
    Linearithmic,   // O(n log n)
    Quadratic,      // O(n²)
    Polynomial(u32),// O(n^k)
    Exponential,    // O(2^n)
    Factorial,      // O(n!)
}
```

| Method | Returns | Description |
|--------|---------|-------------|
| `multiply(other: &Self)` | `Self` | Multiply complexities |
| `add(other: &Self)` | `Self` | Take maximum |

---

### AlgorithmFamily

Algorithm categories.

```rust
pub enum AlgorithmFamily {
    Sorting,
    Searching,
    GraphTraversal,
    ShortestPath,
    MinimumSpanningTree,
    DynamicProgramming,
    DivideAndConquer,
    Greedy,
    Backtracking,
    StringMatching,
    Hashing,
    Compression,
    Numerical,
    MachineLearning,
    Unknown,
}
```

| Method | Returns | Description |
|--------|---------|-------------|
| `typical_complexity()` | `ComplexityClass` | Common complexity |
| `description()` | `&'static str` | Brief description |

---

## GNN API

### GraphNeuralNetwork Trait

Interface for GNN-based analysis.

```rust
pub trait GraphNeuralNetwork: Send + Sync {
    fn propagate(&mut self, iterations: usize);
    fn node_embedding(&self, node: NodeId) -> Option<Array1<f32>>;
    fn subgraph_embedding(&self, nodes: &[NodeId]) -> Array1<f32>;
    fn embedding_dim(&self) -> usize;
}
```

---

### CpgGnn

GNN implementation for CPGs.

```rust
pub struct CpgGnn {
    cpg: Arc<CodePropertyGraph>,
    embeddings: FxHashMap<NodeId, Array1<f32>>,
    config: GnnConfig,
}
```

| Method | Returns | Description |
|--------|---------|-------------|
| `CpgGnn::new(cpg, config)` | `CpgGnn` | Create GNN |
| `propagate(iterations)` | `()` | Run message passing |
| `node_embedding(id)` | `Option<Array1<f32>>` | Get node embedding |
| `subgraph_embedding(nodes)` | `Array1<f32>` | Get aggregated embedding |

---

### NodeEmbedding

Wrapper for node embeddings.

```rust
pub struct NodeEmbedding {
    pub node_id: NodeId,
    pub vector: Array1<f32>,
    pub dim: usize,
}
```

| Method | Returns | Description |
|--------|---------|-------------|
| `new(node_id, vector)` | `NodeEmbedding` | Create embedding |
| `norm()` | `f32` | L2 norm |
| `cosine_similarity(other)` | `f32` | Similarity score |

---

### SubgraphEmbedding

Aggregated embedding for multiple nodes.

```rust
pub struct SubgraphEmbedding {
    pub node_ids: Vec<NodeId>,
    pub vector: Array1<f32>,
    pub dim: usize,
    pub aggregation: AggregationMethod,
}
```

---

### AggregationMethod

Methods for combining node embeddings.

```rust
pub enum AggregationMethod {
    Mean,
    Sum,
    Max,
    Attention,
    Hierarchical,
}
```

---

## Feature Flags

| Feature | Description |
|---------|-------------|
| `default` | Pattern matching only |
| `gnn` | Enable GNN embeddings |
| `gpu` | GPU acceleration (wgpu) |
| `design-patterns` | GoF pattern detection |
| `algorithm-detection` | Algorithm family detection |
| `rholang` | Rholang-specific patterns |
| `metta` | MeTTa-specific patterns |

---

## See Also

- [Pattern Overview](../components/patterns/overview.md)
- [VF2 Matching](../components/patterns/vf2-matching.md)
- [Gang of Four Patterns](../components/patterns/gang-of-four.md)
- [Graph API Reference](graph-reference.md)
