# Pattern Detection Overview

libcpg provides powerful tools for detecting structural patterns in code. This includes both low-level subgraph matching and high-level design pattern detection.

## What is Pattern Detection?

Pattern detection finds recurring structural arrangements in code. This can identify:

- **Design Patterns**: Well-known solutions like Singleton, Factory, Observer
- **Code Smells**: Anti-patterns like God Class, Spaghetti Code
- **Security Vulnerabilities**: Taint flow patterns, unsafe constructs
- **Custom Patterns**: Project-specific idioms and conventions

```
┌─────────────────────────────────────────────────────────────────┐
│                    Pattern Detection Pipeline                    │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│   Source Code                                                    │
│       │                                                          │
│       ▼                                                          │
│   ┌─────────┐         ┌──────────────────┐                      │
│   │   CPG   │────────▶│ Pattern Template │                      │
│   │ Builder │         │   (or custom)    │                      │
│   └─────────┘         └────────┬─────────┘                      │
│       │                        │                                 │
│       ▼                        ▼                                 │
│   ┌─────────┐         ┌──────────────────┐                      │
│   │ Target  │◀───────▶│ Subgraph Matcher │                      │
│   │   CPG   │         │ (VF2 Algorithm)  │                      │
│   └─────────┘         └────────┬─────────┘                      │
│                                │                                 │
│                                ▼                                 │
│                       ┌──────────────────┐                      │
│                       │  Pattern Matches │                      │
│                       │  + Confidence    │                      │
│                       └──────────────────┘                      │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

## Core Concepts

### Pattern Templates

A pattern template defines the structural constraints that must be satisfied:

```rust
use libcpg::pattern::{PatternTemplate, NodeConstraint, EdgeConstraint};
use libcpg::pattern::{NodeKindMatcher, NodeKindTag, EdgeKindMatcher};

// Define a simple factory method pattern template
let template = PatternTemplate::new("Factory Method", "Creates objects without specifying exact class")
    .with_node(
        NodeConstraint::new(0)
            .with_kind(NodeKindMatcher::Exact(NodeKindTag::Class))
    )
    .with_node(
        NodeConstraint::new(1)
            .with_kind(NodeKindMatcher::Exact(NodeKindTag::Function))
            .with_name_pattern("create.*|make.*|build.*")
    )
    .with_node(
        NodeConstraint::new(2)
            .with_kind(NodeKindMatcher::Exact(NodeKindTag::Return))
    )
    .with_edge(
        EdgeConstraint::new(0, 1)
            .with_kind(EdgeKindMatcher::AnyAst)
    )
    .with_edge(
        EdgeConstraint::new(1, 2)
            .with_kind(EdgeKindMatcher::AnyAst)
    )
    .with_min_confidence(0.8);
```

### Pattern Matches

When a pattern is found, the result includes:

```rust
pub struct PatternMatch {
    /// Name of the matched pattern
    pub pattern_name: String,
    /// Confidence score (0.0 to 1.0)
    pub confidence: f64,
    /// Mapping from pattern nodes to target nodes
    pub node_mapping: FxHashMap<NodeId, NodeId>,
    /// Root node in the target graph
    pub root: NodeId,
    /// Additional metadata
    pub metadata: FxHashMap<String, String>,
}
```

**Working with matches:**

```rust
for m in matches {
    println!("Found: {} (confidence: {:.2})", m.pattern_name, m.confidence);
    println!("  Root node: {:?}", m.root);
    println!("  Matched {} nodes", m.match_size());

    // Access matched nodes
    for target_id in m.matched_nodes() {
        let node = cpg.node(target_id).expect("node exists");
        println!("  - {:?} at line {}", node.kind, node.source_range.start_line);
    }
}
```

### Node and Edge Matchers

Flexible matching with wildcards and categories:

```rust
// Exact match
NodeKindMatcher::Exact(NodeKindTag::Class)

// Match any of several kinds
NodeKindMatcher::AnyOf(vec![NodeKindTag::Class, NodeKindTag::Struct])

// Match any declaration (class, struct, function, variable, etc.)
NodeKindMatcher::AnyDeclaration

// Match any expression (binary op, call, identifier, literal, etc.)
NodeKindMatcher::AnyExpression

// Match any statement (if, while, return, etc.)
NodeKindMatcher::AnyStatement

// Match anything
NodeKindMatcher::Any
```

Edge matchers work similarly:

```rust
// Any AST edge
EdgeKindMatcher::AnyAst

// Any control flow edge
EdgeKindMatcher::AnyCfg

// Any data flow edge
EdgeKindMatcher::AnyDfg

// Any edge
EdgeKindMatcher::Any
```

## Detection Approaches

### 1. Subgraph Isomorphism (VF2)

The most precise approach - finds exact structural matches:

```rust
use libcpg::pattern::{Vf2Matcher, SubgraphMatcher};

let matcher = Vf2Matcher::new()
    .with_strict_kinds(true)   // Exact node type matches
    .with_strict_edges(false); // Relaxed edge matching

let pattern_cpg = build_pattern_cpg();
let matches = matcher.find_matches(&pattern_cpg, &target_cpg);
```

**When to use:**
- Detecting specific code structures
- Finding exact pattern implementations
- Security vulnerability patterns

### 2. Template-Based Detection

Higher-level detection using pattern templates:

```rust
use libcpg::patterns::GofPatternDetector;

let detector = GofPatternDetector::new()
    .with_min_confidence(0.8)
    .with_patterns(vec![GofPattern::Singleton, GofPattern::Factory]);

let matches = detector.detect(&cpg);
```

**When to use:**
- Design pattern detection
- Multiple patterns at once
- Need confidence scores

### 3. Graph Similarity

Compare overall graph structure:

```rust
use libcpg::pattern::{GraphSimilarity, SimilarityMetric};

let similarity = GraphSimilarity::new()
    .with_metric(SimilarityMetric::WeisfeilerLehman);

let score = similarity.similarity(&cpg1, &cpg2);
println!("Similarity: {:.2}", score);
```

**When to use:**
- Code clone detection
- Clustering similar functions
- Finding related code

## Quick Start

### Detect Design Patterns

```rust
use libcpg::{TreeSitterCpgBuilder, Language};
use libcpg::patterns::{GofPatternDetector, PatternDetector};

// Build CPG from source
let builder = TreeSitterCpgBuilder::new();
let source = include_str!("my_code.rs");
let cpg = builder.build(source, Language::Rust)?;

// Detect Gang of Four patterns
let detector = GofPatternDetector::new()
    .with_min_confidence(0.7);

let matches = detector.detect(&cpg);

for m in matches {
    println!("Found {} pattern (confidence: {:.0}%)",
             m.pattern_name,
             m.confidence * 100.0);

    if let Some(category) = m.metadata.get("category") {
        println!("  Category: {}", category);
    }
}
```

### Custom Pattern Detection

```rust
use libcpg::pattern::{Vf2Matcher, SubgraphMatcher, PatternTemplate};

// Define your pattern
let template = PatternTemplate::new("Null Check Before Use", "Defensive null checking")
    .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::If)))
    .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::AnyExpression))
    .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst))
    .with_min_confidence(0.85);

// Convert to CPG and match
let pattern_cpg = template.to_pattern_graph();
let matcher = Vf2Matcher::new().with_strict_kinds(false);

let matches = matcher.find_matches(&pattern_cpg, &target_cpg);
```

## Pattern Categories

### By Purpose

| Category | Examples | Use Case |
|----------|----------|----------|
| Design | Singleton, Factory, Observer | Architecture analysis |
| Structural | God Class, Long Method | Code quality |
| Security | SQL Injection, XSS | Vulnerability detection |
| Idiom | Error handling, Resource management | Language-specific patterns |

### By Detection Method

| Method | Precision | Recall | Speed |
|--------|-----------|--------|-------|
| Exact isomorphism | High | Low | Slow |
| Relaxed matching | Medium | Medium | Medium |
| Template-based | Medium | High | Fast |
| Similarity-based | Low | High | Fast |

## Performance Considerations

### VF2 Complexity

VF2 has worst-case exponential complexity, but performs well on sparse graphs:

- **Best case**: O(n) for quick rejection
- **Typical case**: O(n²) for code graphs
- **Worst case**: O(n! × n) for dense graphs

**Optimization tips:**

```rust
// Limit number of matches for early termination
let matcher = Vf2Matcher::new()
    .with_max_matches(10);  // Stop after 10 matches

// Use strict matching to prune search space
let matcher = Vf2Matcher::new()
    .with_strict_kinds(true)
    .with_strict_edges(true);
```

### Parallel Detection

Run pattern detection in parallel using rayon:

```rust
use rayon::prelude::*;

let patterns = vec![GofPattern::Singleton, GofPattern::Factory, GofPattern::Observer];

let all_matches: Vec<PatternMatch> = patterns
    .par_iter()
    .flat_map(|pattern| {
        let detector = GofPatternDetector::new()
            .with_patterns(vec![*pattern]);
        detector.detect(&cpg)
    })
    .collect();
```

## Next Steps

- [Gang of Four Patterns](gang-of-four.md) - GoF pattern catalog
- [VF2 Algorithm](vf2-matching.md) - Subgraph isomorphism details
- [Graph Overview](../graph/overview.md) - CPG structure
