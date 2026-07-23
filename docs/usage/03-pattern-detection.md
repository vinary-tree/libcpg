# Pattern Detection

libcpg finds recurring structures in a CPG in three complementary ways. Before any code, fix one distinction that the API makes and the documentation never blurs:

- **`pattern`** (singular, module `libcpg::pattern`) is **always compiled**. It provides the general [VF2](../GLOSSARY.md#vf2) [subgraph-isomorphism](../GLOSSARY.md#isomorphism--subgraph-isomorphism) matcher and the [graph-similarity](../GLOSSARY.md#similarity-metric) metrics. No feature flag required.
- **`patterns`** (plural, module `libcpg::patterns`) is **gated behind the `design-patterns` feature**. It provides the [Gang-of-Four](../GLOSSARY.md#gang-of-four-gof) detector, the DPML template language, and the feature-vector classifier.

They are different modules for different jobs. The three approaches, from most general to most specialised:

| Approach | Module | Feature | Best for |
|----------|--------|---------|----------|
| VF2 subgraph isomorphism | `pattern::Vf2Matcher` | none (always on) | Any structural motif you can express as a small graph |
| GoF template detection | `patterns::GofPatternDetector` | `design-patterns` | The 23 classic [design patterns](../GLOSSARY.md#design-pattern) |
| Feature-vector classifier | `patterns::PatternClassifier` | `design-patterns` (+ `ml-linfa` for ML mode) | Class-level pattern labelling from numeric features |

![The pattern-detection pipeline: a pattern graph or template is matched against the target CPG by a relaxed VF2 matcher, scored for confidence, and filtered.](../diagrams/pattern-detection-pipeline.svg)

*Figure — the pattern-detection pipeline shared by the VF2 matcher and the GoF detector. Source: [`diagrams/pattern-detection-pipeline.puml`](../diagrams/pattern-detection-pipeline.puml).*

---

## VF2 subgraph isomorphism (feature-free)

[VF2](../GLOSSARY.md#vf2) (Cordella, Foggia, Sansone & Vento [[1]](#references)) finds **all** occurrences of a small *pattern* graph inside a larger *target* graph. `Vf2Matcher` implements it as a depth-first state-space search with feasibility pruning and exact backtracking. The `find_matches` method comes from the `SubgraphMatcher` trait, so bring that trait into scope.

The example below is the library's own backtracking regression, distilled: a 3-node directed **path** pattern `p0 → p1 → p2` matched against a **diamond** target `t0 → {t1, t2} → t3`. There are exactly **two** embeddings — one through `t1`, one through `t2` — and finding the second requires the search to backtrack out of the first without losing `p0 → t0`.

```rust
// Feature-free: the `pattern` module is always compiled.
use libcpg::{
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, Language, NodeId, SourceRange,
};
use libcpg::pattern::Vf2Matcher;
use libcpg::SubgraphMatcher; // brings `find_matches` into scope

fn node(g: &mut CodePropertyGraph) -> NodeId {
    // All nodes share a kind so structure alone decides the matches.
    g.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::If, SourceRange::default()))
}

fn main() {
    // Pattern: path p0 -> p1 -> p2.
    let mut pattern = CodePropertyGraph::new(Language::Rust);
    let p: Vec<NodeId> = (0..3).map(|_| node(&mut pattern)).collect();
    pattern.connect(p[0], p[1], CpgEdgeKind::AstChild);
    pattern.connect(p[1], p[2], CpgEdgeKind::AstChild);

    // Target: diamond t0 -> {t1, t2} -> t3.
    let mut target = CodePropertyGraph::new(Language::Rust);
    let t: Vec<NodeId> = (0..4).map(|_| node(&mut target)).collect();
    target.connect(t[0], t[1], CpgEdgeKind::AstChild);
    target.connect(t[0], t[2], CpgEdgeKind::AstChild);
    target.connect(t[1], t[3], CpgEdgeKind::AstChild);
    target.connect(t[2], t[3], CpgEdgeKind::AstChild);

    let matches = Vf2Matcher::new().find_matches(&pattern, &target);
    assert_eq!(matches.len(), 2); // both diamond embeddings, no more, no fewer
}
```

### Strict vs relaxed matching

`Vf2Matcher` is a builder with three switches:

```rust
use libcpg::pattern::Vf2Matcher;

let matcher = Vf2Matcher::new()
    .with_strict_kinds(true)  // require identical node-kind tags (default: false)
    .with_strict_edges(true)  // require identical edge kinds (default: false)
    .with_max_matches(10);    // stop after N matches (0 = unlimited, the default)
```

- **Relaxed (default).** Two nodes match if they fall in the same broad category (declaration / expression / statement) or share a kind tag; two edges match if they are in the same overlay (AST/CFG/DFG/call). This maximises recall and is what the GoF detector uses.
- **Strict.** Node-kind tags and edge kinds must be identical. Use this for exact structural search.
- **`with_max_matches`** bounds the search — indispensable on large targets where an unbounded VF2 can be expensive (worst case `` $`O(N!\,N)`$ ``, though pruning makes it practical in code graphs). It is a `usize`; `0` means unlimited.

### Inspecting a `PatternMatch`

Every match is a `PatternMatch` with public fields and two helpers:

```rust
use libcpg::pattern::Vf2Matcher;
use libcpg::{CodePropertyGraph, SubgraphMatcher};

fn report(pattern: &CodePropertyGraph, target: &CodePropertyGraph) {
    for m in Vf2Matcher::new().find_matches(pattern, target) {
        println!("pattern '{}' (confidence {:.2})", m.pattern_name, m.confidence);
        println!("  root in target: {:?}", m.root);
        println!("  matched {} nodes", m.match_size());

        // The target nodes that were matched:
        for target_id in m.matched_nodes() {
            let _ = target_id;
        }
        // The full pattern-node -> target-node correspondence:
        for (pattern_id, target_id) in &m.node_mapping {
            let _ = (pattern_id, target_id);
        }
        // Free-form metadata (the GoF detector fills "category" and "pattern_type"):
        if let Some(category) = m.metadata.get("category") {
            println!("  category: {category}");
        }
    }
}
```

`node_mapping` is a `FxHashMap<NodeId, NodeId>` from pattern node to target node; `match_size()` is its length; `matched_nodes()` iterates the target side.

### Describing a pattern without hand-wiring it

You can also specify a pattern declaratively as a `PatternTemplate` of `NodeConstraint`/`EdgeConstraint` values and compile it to a searchable graph with `to_pattern_graph()`:

```rust
// Feature-free.
use libcpg::pattern::{
    EdgeConstraint, EdgeKindMatcher, NodeConstraint, NodeKindMatcher, NodeKindTag,
    PatternTemplate, Vf2Matcher,
};
use libcpg::{CodePropertyGraph, SubgraphMatcher};

fn find_class_with_field(target: &CodePropertyGraph) -> usize {
    let template = PatternTemplate::new("class-with-field", "a class owning a field")
        .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::Class)))
        .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Field)))
        .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst));

    let pattern = template.to_pattern_graph();
    Vf2Matcher::new().find_matches(&pattern, target).len()
}
```

The constraint vocabulary (`NodeKindMatcher`, `NodeKindTag`, `EdgeKindMatcher`) and the [similarity](../GLOSSARY.md#similarity-metric) metrics that complement VF2 are documented in [VF2 Matching](../components/patterns/vf2-matching.md).

---

## Gang-of-Four detection (`design-patterns`)

`GofPatternDetector` recognises the 23 [Gang-of-Four](../GLOSSARY.md#gang-of-four-gof) patterns [[2]](#references) by matching each pattern's built-in template against the CPG with a **relaxed** VF2 matcher and scoring the result. Detection lives in `libcpg::patterns`; the `detect` method comes from the `PatternDetector` trait.

```rust
// requires: features = ["design-patterns", "lang-rust"]
use libcpg::{CpgBuilder, Language, TreeSitterCpgBuilder};
use libcpg::patterns::{GofPattern, GofPatternDetector, PatternDetector};

fn main() -> Result<(), libcpg::Error> {
    let source = r#"
        trait PaymentStrategy { fn pay(&self, amount: i32); }
        struct CardPayment;
        impl PaymentStrategy for CardPayment { fn pay(&self, amount: i32) { let _ = amount; } }
        struct Processor { strategy: CardPayment }
    "#;
    let cpg = TreeSitterCpgBuilder::new().build(source, Language::Rust)?;

    let detector = GofPatternDetector::new()
        .with_min_confidence(0.7)
        .with_patterns(vec![GofPattern::Strategy, GofPattern::Observer]);

    for m in detector.detect(&cpg) {
        println!(
            "{} [{}] confidence {:.2}",
            m.pattern_name,
            m.metadata.get("category").map(String::as_str).unwrap_or("?"),
            m.confidence,
        );
    }
    Ok(())
}
```

### The pattern catalogue

`GofPattern` names all 23, grouped by `GofCategory`. **Use `FactoryMethod`, never `Factory`** — the latter is not a variant.

- **Creational (5):** `AbstractFactory`, `Builder`, `FactoryMethod`, `Prototype`, `Singleton`.
- **Structural (7):** `Adapter`, `Bridge`, `Composite`, `Decorator`, `Facade`, `Flyweight`, `Proxy`.
- **Behavioral (11):** `ChainOfResponsibility`, `Command`, `Interpreter`, `Iterator`, `Mediator`, `Memento`, `Observer`, `State`, `Strategy`, `TemplateMethod`, `Visitor`.

`GofPattern::name()` returns the display name (`FactoryMethod` → `"Factory Method"`), and `GofPattern::category()` returns its `GofCategory`. Passing no patterns via `with_patterns` (the default) searches **all 23**.

### Confidence and tuning

Each match carries a [`confidence`](../GLOSSARY.md#confidence-pattern-match) in `` $`[0, 1]`$ `` measuring how completely the candidate subgraph fills the pattern's template. The detector keeps only matches at or above `min_confidence`:

- The detector's default `min_confidence` is **`0.7`**.
- Individual templates may set their own floor — the `Observer` template, for instance, uses **`0.8`** internally, so it is inherently more conservative.
- **Lower `min_confidence` to raise recall** (more, weaker candidates); **raise it to raise precision** (fewer, stronger ones). Results are returned sorted by confidence, highest first.

```rust
// requires: features = ["design-patterns"]
use libcpg::patterns::GofPatternDetector;

let high_recall = GofPatternDetector::new().with_min_confidence(0.5); // more candidates
let high_precision = GofPatternDetector::new().with_min_confidence(0.9); // stronger only
let _ = (high_recall, high_precision);
```

The scoring formula, the per-pattern templates, and the node/edge kinds each uses (`Trait` for interfaces, `Class`, `Field`, `Function`, and `Inherits`/`Implements`/`TypeOf` edges) are detailed in [Gang of Four](../components/patterns/gang-of-four.md).

---

## The classifier alternative (`design-patterns`)

Where the GoF detector matches *graph structure*, `PatternClassifier` scores a **12-field [feature vector](../GLOSSARY.md#feature-vector-classification)** per class (method count, field count, static-method count, factory-like method count, and so on). It is an alternative lens, useful when the structural template is too rigid.

```rust
// requires: features = ["design-patterns", "lang-rust"]
use libcpg::{CpgBuilder, Language, TreeSitterCpgBuilder};
use libcpg::patterns::PatternClassifier;
use libcpg::patterns::classification::ClassificationMode;

fn main() -> Result<(), libcpg::Error> {
    let cpg = TreeSitterCpgBuilder::new()
        .build("struct Logger; impl Logger { fn instance() -> Logger { Logger } }", Language::Rust)?;

    let classifier = PatternClassifier::new()
        .with_min_confidence(0.7)
        .with_mode(ClassificationMode::RuleBased); // default; no ML dependency

    for m in classifier.classify(&cpg) {
        println!("{} confidence {:.2}", m.pattern_name, m.confidence);
    }
    Ok(())
}
```

`ClassificationMode` selects the scoring strategy: `RuleBased` (the default, hand-crafted heuristics, no extra dependency), `MachineLearning` (requires the `ml-linfa` feature and a trained model), or `Hybrid` (both, boosting confidence when they agree). Both `classify` and `detect` return `Vec<PatternMatch>`, so the two approaches are interchangeable downstream. See [Classification](../components/patterns/classification.md) for the full feature list and modes.

---

## Choosing an approach

- **Reach for VF2** when you can draw the motif as a graph and want *every* occurrence — it is always available and precise.
- **Reach for the GoF detector** when you specifically want the classic design patterns with confidence scores and category metadata.
- **Reach for the classifier** when structural templates are too brittle and a numeric, class-level judgement fits better — or when you have an `ml-linfa` model to apply.

All three are heuristic aids, not proofs: treat their output as advisory. The theory behind relaxed template matching and confidence scoring is in [Design Pattern Detection](../theory/07-design-pattern-detection.md).

---

## References

1. Cordella, L. P., Foggia, P., Sansone, C., Vento, M. (2004). *A (Sub)graph Isomorphism Algorithm for Matching Large Graphs.* IEEE TPAMI 26(10). DOI: [10.1109/TPAMI.2004.75](https://doi.org/10.1109/TPAMI.2004.75)
2. Gamma, E., Helm, R., Johnson, R., Vlissides, J. (1994). *Design Patterns: Elements of Reusable Object-Oriented Software.* Addison-Wesley. ISBN 978-0201633610 (no DOI).
</content>
