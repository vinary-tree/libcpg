# Pattern Detection Overview

`libcpg` detects recurring *structural* arrangements in code by matching graph shapes
against a [Code Property Graph](../../GLOSSARY.md#code-property-graph-cpg) (CPG). This
page maps the whole pattern-detection surface: what each piece is, how the pieces fit
together, and when to reach for which.

The single most important distinction on this page is between the two sibling modules,
because their names differ by one letter and their guarantees differ by a lot:

| Module | What it holds | Feature gate | Availability |
| :----- | :------------ | :----------- | :----------- |
| `libcpg::pattern` (singular) | [VF2](../../GLOSSARY.md#vf2) subgraph matching, pattern templates, and [graph similarity](../../GLOSSARY.md#similarity-metric) | none — **always compiled** | every build |
| `libcpg::patterns` (plural) | the 23 [Gang-of-Four](../../GLOSSARY.md#gang-of-four-gof) detectors, [DPML](../../GLOSSARY.md#dpml-design-pattern-markup-language) templates, [`PatternMetrics`](../../GLOSSARY.md#feature-vector-classification), and the heuristic classifier | `design-patterns` | opt-in |

Throughout this documentation, **`pattern`** means the always-on matching engine and
**`patterns`** means the feature-gated Gang-of-Four layer built *on top of* it. They are
never interchangeable.

## What pattern detection is (and is not)

A detector answers a structural question — "does this code contain a subgraph shaped
like *X*?" — where *X* is a small **pattern graph** and the code is a large **target
graph**. Because the match is over graph structure (node [kinds](../../GLOSSARY.md#node-kind--edge-kind)
and typed edges), it is oblivious to identifier names, whitespace, and comment text: a
`Singleton` written in Rust and one written in Java match the *same* template.

This is **not** semantic proof. A structural match says "the shape is here", not "the
program truly behaves as a Singleton". Every result is *advisory* and carries a
[confidence](../../GLOSSARY.md#confidence-pattern-match) score in $`[0, 1]`$ so callers
can threshold according to their tolerance for false positives.

![Pattern detection pipeline: source is parsed into a CPG, a pattern template is compiled into a pattern graph, the VF2 subgraph matcher aligns the two, and scored PatternMatch results emerge.](../../diagrams/pattern-detection-pipeline.svg)

*Figure — the end-to-end detection pipeline: build a target CPG, compile a pattern into a small pattern graph, run VF2, then score and filter the matches. Source: [`diagrams/pattern-detection-pipeline.puml`](../../diagrams/pattern-detection-pipeline.puml).*

## The result type: `PatternMatch`

Every detector, whether it works by subgraph isomorphism, template compilation, or
heuristics, returns the same value — a `pattern::PatternMatch` (also re-exported at the
crate root):

```rust
use rustc_hash::FxHashMap;
use libcpg::NodeId;

pub struct PatternMatch {
    /// Name/identifier of the matched pattern.
    pub pattern_name: String,
    /// Confidence score in [0.0, 1.0].
    pub confidence: f64,
    /// Mapping from pattern node IDs to target node IDs.
    pub node_mapping: FxHashMap<NodeId, NodeId>,
    /// The root node of the match in the target graph.
    pub root: NodeId,
    /// Free-form metadata (e.g. "category" → "Behavioral").
    pub metadata: FxHashMap<String, String>,
}
```

Two convenience methods read the mapping without exposing the map directly:
`match_size()` returns the number of mapped nodes, and `matched_nodes()` iterates the
target-side `NodeId`s. A worked inspection loop:

```rust
use libcpg::pattern::{Vf2Matcher, SubgraphMatcher};

let matcher = Vf2Matcher::new();
let matches = matcher.find_matches(&pattern_cpg, &target_cpg);

for m in &matches {
    println!("{} (confidence {:.2}) rooted at {:?}", m.pattern_name, m.confidence, m.root);
    println!("  spans {} nodes", m.match_size());
    for target_id in m.matched_nodes() {
        let node = target_cpg.node(target_id).expect("mapped id is a live node");
        // `range` is the SourceRange field on CpgNode; `.name()` is Option<&str>.
        println!("  - {:?} @ line {}", node.kind, node.range.start_line);
    }
}
```

Note `node(id)` returns `Option<&CpgNode>` (not `Result`), and `node.kind` / `node.range`
are public *fields*, not methods.

## Four detection approaches

`libcpg` offers four complementary ways to find patterns. The first three live in the
always-on `pattern` module; the fourth lives in the feature-gated `patterns` module.

### 1. Subgraph isomorphism — `pattern::Vf2Matcher`

The most precise approach. You supply a small pattern CPG and a target CPG; VF2 finds
every embedding of the pattern in the target, pruning with node- and edge-feasibility
rules. It supports **strict** matching (exact [node-kind](../../GLOSSARY.md#node-kind--edge-kind)
and edge-kind equality) and **relaxed** matching (category-level equality — any
declaration matches any declaration, and so on).

```rust
use libcpg::pattern::{Vf2Matcher, SubgraphMatcher};

let matcher = Vf2Matcher::new()
    .with_strict_kinds(true)    // exact node-kind equality
    .with_strict_edges(false)   // relaxed edge matching
    .with_max_matches(0);       // 0 = unlimited

let matches = matcher.find_matches(&pattern_cpg, &target_cpg);
```

Use it when you need exact structures, security-relevant shapes, or full control over
strictness. See [VF2 subgraph matching](vf2-matching.md) for the algorithm and the API.

### 2. Template-based detection — `PatternTemplate` and DPML

Instead of hand-building a pattern CPG, declare it as a `pattern::PatternTemplate` (node
and edge constraints) or as a [DPML](dpml.md) document (roles and relationships in YAML
or TOML). Both compile to a pattern CPG via `to_pattern_graph()`, which then feeds VF2 —
exactly what the Gang-of-Four detector does internally.

```rust
use libcpg::pattern::{
    PatternTemplate, NodeConstraint, EdgeConstraint,
    NodeKindMatcher, NodeKindTag, EdgeKindMatcher, Vf2Matcher, SubgraphMatcher,
};

let template = PatternTemplate::new("Guarded Return", "an `if` whose body returns")
    .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::If)))
    .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::Return)))
    .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
    .with_min_confidence(0.85);

let pattern_cpg = template.to_pattern_graph();
let matches = Vf2Matcher::new().find_matches(&pattern_cpg, &target_cpg);
```

Use it when the pattern is declarative, reusable, or authored by non-Rust users (DPML).

### 3. Graph similarity — `pattern::GraphSimilarity`

Rather than locating a sub-shape, similarity scores how alike two *whole* graphs are —
useful for clone detection and clustering. Four metrics are available; the default is
[Jaccard](../../GLOSSARY.md#jaccard-similarity) over node-kind multisets.

```rust
use libcpg::pattern::{GraphSimilarity, SimilarityMetric};

let score = GraphSimilarity::new()
    .with_metric(SimilarityMetric::WeisfeilerLehman)
    .similarity(&cpg_a, &cpg_b);   // score in [0.0, 1.0]
```

Use it when you want a fuzzy, whole-graph resemblance signal rather than an exact
sub-structure. Similarity is covered alongside VF2 in [VF2 subgraph matching](vf2-matching.md).

### 4. Heuristic classification — `patterns::classification::PatternClassifier`

A cheaper, name- and shape-aware alternative to VF2 for a handful of patterns. It walks
each class, builds a 12-element [feature vector](../../GLOSSARY.md#feature-vector-classification),
and applies hand-written rules (optionally blended with a linfa model). It recognises
five patterns only — Singleton, Factory, Observer, Strategy, Decorator — and requires
the `design-patterns` feature.

```rust
// requires: features = ["design-patterns"]
use libcpg::patterns::classification::PatternClassifier;

let matches = PatternClassifier::new().with_min_confidence(0.7).classify(&cpg);
```

Use it when you want fast, linear-time screening and can accept lower precision. See
[Pattern classification](classification.md).

### Choosing an approach

| Approach | Module (feature) | Precision | Recall | Cost |
| :------- | :--------------- | :-------- | :----- | :--- |
| Strict VF2 | `pattern` (none) | high | low | highest |
| Relaxed VF2 / templates | `pattern` (none) | medium | medium | high |
| Gang-of-Four detector | `patterns` (`design-patterns`) | medium | medium–high | high |
| Graph similarity | `pattern` (none) | low | high | low |
| Heuristic classifier | `patterns::classification` (`design-patterns`) | low–medium | medium | low |

## The Gang-of-Four detector at a glance

`patterns::GofPatternDetector` packages approach #2 for all 23 GoF patterns. It builds a
template CPG per pattern, matches it with a **relaxed** VF2 matcher (both strictness
toggles off, for recall), scores each hit against the template's completeness, keeps the
matches at or above `min_confidence` (default `0.7`), and returns them sorted by
confidence, descending.

```rust
// requires: features = ["design-patterns"]
use libcpg::patterns::{GofPatternDetector, GofPattern, PatternDetector};

let detector = GofPatternDetector::new()
    .with_patterns(vec![GofPattern::Singleton, GofPattern::FactoryMethod])
    .with_min_confidence(0.75);

let matches = detector.detect(&cpg);
for m in &matches {
    let category = m.metadata.get("category").map(String::as_str).unwrap_or("?");
    println!("{} [{}] — {:.0}%", m.pattern_name, category, m.confidence * 100.0);
}
```

The variant is `GofPattern::FactoryMethod` — there is no `GofPattern::Factory`. Calling
`.detect()` requires the `PatternDetector` trait to be in scope. The full catalogue,
per-pattern intent, and detection signatures live in
[Gang-of-Four patterns](gang-of-four.md).

## Running detectors in parallel

Detectors take `&CodePropertyGraph` and return owned `Vec<PatternMatch>`, so they compose
cleanly with [`rayon`](https://docs.rs/rayon). Splitting the pattern set across a thread
pool is safe because each closure reads the shared CPG immutably:

```rust
// requires: features = ["design-patterns"]
use rayon::prelude::*;
use libcpg::pattern::PatternMatch;
use libcpg::patterns::{GofPatternDetector, GofPattern, PatternDetector};

let wanted = [GofPattern::Singleton, GofPattern::Observer, GofPattern::Strategy];

let all: Vec<PatternMatch> = wanted
    .par_iter()
    .flat_map(|&p| GofPatternDetector::new().with_patterns(vec![p]).detect(&cpg))
    .collect();
```

## Honesty about the surface

- With the default feature set (`default = []`) `build(source, language)` fails for
  every language — a `lang-*` feature (or the feature-free
  [Mode B](../../GLOSSARY.md#mode-b--build_from_tree) `build_from_tree`) is required to
  obtain a CPG in the first place. The `pattern` module then matches whatever CPG you
  hand it, regardless of how it was built.
- The `pattern` module (VF2, templates, similarity) is always compiled; the `patterns`
  module (Gang-of-Four, DPML parsing, classifier) needs `design-patterns`.
- Every result is a heuristic. Structural matching cannot distinguish a genuine pattern
  from a coincidental shape; the confidence score exists precisely so you can tune the
  trade-off.

## See also

- [VF2 subgraph matching](vf2-matching.md) — the matching engine and graph similarity.
- [Gang-of-Four patterns](gang-of-four.md) — the 23-pattern catalogue.
- [DPML templates](dpml.md) — declaring patterns in YAML/TOML.
- [Pattern classification](classification.md) — the heuristic classifier and metrics.
- Theory: [subgraph isomorphism & VF2](../../theory/05-subgraph-isomorphism-vf2.md),
  [graph similarity](../../theory/06-graph-similarity.md),
  [design-pattern detection](../../theory/07-design-pattern-detection.md).
- API: [pattern reference](../../api/pattern-reference.md).

## References

4. Cordella, L. P., Foggia, P., Sansone, C., Vento, M. (2004). *A (Sub)graph Isomorphism Algorithm for Matching Large Graphs.* IEEE TPAMI 26(10). DOI: [10.1109/TPAMI.2004.75](https://doi.org/10.1109/TPAMI.2004.75)
