# VF2 Subgraph Isomorphism

The VF2 algorithm finds occurrences of a pattern graph within a larger target graph. libcpg uses this for precise pattern detection in Code Property Graphs.

## What is Subgraph Isomorphism?

Subgraph isomorphism asks: "Does graph G contain a subgraph that is structurally identical to pattern graph P?"

```
Pattern P:              Target G:

   A ─── B                 1 ─── 2 ─── 3
                           │     │
                           4 ─── 5

Matches in G:
  - (A→1, B→2)
  - (A→1, B→4)
  - (A→2, B→1)
  - (A→2, B→3)
  - (A→2, B→5)
  ... and more
```

In code analysis, this finds structural patterns regardless of variable names or specific values.

## The VF2 Algorithm

VF2 (Vento and Foggia, 2004) is an efficient algorithm for subgraph isomorphism that uses state-space search with pruning rules.

### Algorithm Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                     VF2 Algorithm                                │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  1. Initialize empty mapping M = {}                             │
│                                                                  │
│  2. If M is complete (all pattern nodes mapped):                │
│       ✓ Report match and return                                 │
│                                                                  │
│  3. Generate candidate pairs (p, t) where:                      │
│       - p is an unmapped pattern node                           │
│       - t is an unused target node                              │
│       - Prefer "terminal" nodes (adjacent to mapped nodes)      │
│                                                                  │
│  4. For each candidate pair (p, t):                             │
│       a. Check feasibility:                                     │
│          - Node compatibility (types match?)                    │
│          - Edge consistency (edges exist in target?)            │
│       b. If feasible:                                           │
│          - Add (p → t) to mapping M                             │
│          - Recurse to step 2                                    │
│          - Backtrack: remove (p → t) from M                     │
│                                                                  │
│  5. Return all found matches                                    │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

### State Space Representation

The algorithm maintains a state representing the current partial mapping:

```rust
pub struct Vf2State<'a> {
    /// The pattern graph
    pattern: &'a CodePropertyGraph,
    /// The target graph
    target: &'a CodePropertyGraph,
    /// Current mapping: pattern node → target node
    mapping: FxHashMap<NodeId, NodeId>,
    /// Reverse mapping: target node → pattern node
    reverse_mapping: FxHashMap<NodeId, NodeId>,
    /// Pattern nodes not yet mapped
    unmapped_pattern: FxHashSet<NodeId>,
    /// Target nodes not yet used
    unused_target: FxHashSet<NodeId>,
    /// Pattern nodes adjacent to mapped nodes (terminal set)
    pattern_terminal: FxHashSet<NodeId>,
    /// Target nodes adjacent to used nodes (terminal set)
    target_terminal: FxHashSet<NodeId>,
}
```

### Terminal Sets

Terminal sets are key to VF2's efficiency. They contain nodes that are:
- Adjacent to at least one mapped node
- Not yet mapped themselves

```
Mapping so far: A → 1

Pattern:              Target:
   A* ─── B              1* ─── 2
   │                     │     │
   C                     3 ─── 4

Terminal sets:
  Pattern: {B, C}       Target: {2, 3}

* = mapped

Next candidates: (B,2), (B,3), (C,2), (C,3)
```

By preferring terminal nodes, VF2 explores connected regions first, allowing faster pruning.

## Using the VF2 Matcher

### Basic Usage

```rust
use libcpg::pattern::{Vf2Matcher, SubgraphMatcher};

// Create matcher with default settings
let matcher = Vf2Matcher::new();

// Find all matches
let matches = matcher.find_matches(&pattern_cpg, &target_cpg);

for m in matches {
    println!("Match at root {:?}", m.root);
    for (pattern_id, target_id) in &m.node_mapping {
        println!("  {:?} → {:?}", pattern_id, target_id);
    }
}
```

### Configuration Options

```rust
let matcher = Vf2Matcher::new()
    // Require exact node kind matches
    .with_strict_kinds(true)
    // Require exact edge kind matches
    .with_strict_edges(true)
    // Stop after finding 10 matches
    .with_max_matches(10);
```

### Strict vs Relaxed Matching

**Strict matching** requires exact type equality:

```rust
// Pattern node: Function
// Target node must be: Function (exactly)
let matcher = Vf2Matcher::new().with_strict_kinds(true);
```

**Relaxed matching** allows compatible types:

```rust
// Pattern node: Function
// Target node can be: Function, Method (same category)
let matcher = Vf2Matcher::new().with_strict_kinds(false);
```

Category compatibility:
- **Declarations**: Module, Class, Struct, Enum, Trait, Function, Variable, Field
- **Expressions**: BinaryOp, UnaryOp, Call, MemberAccess, IndexAccess, Identifier, Literal, Lambda
- **Statements**: Return, If, While, For, Loop, Match, Break, Continue

## Feasibility Checking

Each candidate pair must pass feasibility checks before being added to the mapping.

### Node Compatibility

Nodes must be "compatible" based on matching mode:

```rust
fn nodes_compatible(&self, pattern_node: &CpgNodeKind, target_node: &CpgNodeKind) -> bool {
    if self.strict_kinds {
        // Exact match required
        NodeKindTag::from_kind(pattern_node) == NodeKindTag::from_kind(target_node)
    } else {
        // Category match allowed
        // declarations match declarations
        // expressions match expressions
        // statements match statements
        // OR exact match
    }
}
```

### Edge Consistency

For each edge in the pattern involving mapped nodes, a compatible edge must exist in the target:

```rust
// If pattern has: mapped_pattern_node → pattern_candidate
// And we're mapping pattern_candidate → target_candidate
// Then target must have: mapped_target_node → target_candidate
```

```
Pattern:                   Target (so far):
  A* ─edge1─▶ B             1* ─edge1'─▶ 2
  │                         │
  edge2                     edge2'
  │                         │
  ▼                         ▼
  C* ─edge3─▶ B             3* ─??????─▶ ?

Mapping: {A→1, C→3}
Candidate: (B, 2)

Must verify:
  - Target has edge 1→2 compatible with edge1
  - Target has edge 3→2 compatible with edge3

If either edge missing/incompatible → reject candidate
```

## Building Pattern Graphs

### Manual Construction

```rust
use libcpg::{CodePropertyGraph, CpgNode, CpgEdge, CpgNodeKind, CpgEdgeKind};

// Create a pattern for: if (condition) { return x; }
let mut pattern = CodePropertyGraph::new(Language::Rust);

let if_node = pattern.add_node(CpgNode::new(
    NodeId::new(0),
    CpgNodeKind::If,
    SourceRange::default(),
));

let return_node = pattern.add_node(CpgNode::new(
    NodeId::new(0),
    CpgNodeKind::Return,
    SourceRange::default(),
));

// Connect with AST edge
pattern.add_edge(CpgEdge::new(
    EdgeId::new(0),
    if_node,
    return_node,
    CpgEdgeKind::AstChild,
));
```

### From Pattern Templates

```rust
use libcpg::pattern::PatternTemplate;

let template = PatternTemplate::new("Null Check", "Check for null before use")
    .with_node(NodeConstraint::new(0).with_kind(NodeKindMatcher::Exact(NodeKindTag::If)))
    .with_node(NodeConstraint::new(1).with_kind(NodeKindMatcher::Exact(NodeKindTag::BinaryOp)))
    .with_edge(EdgeConstraint::new(0, 1).with_kind(EdgeKindMatcher::AnyAst));

let pattern_cpg = template.to_pattern_graph();
```

## Graph Similarity

For approximate matching, use the `GraphSimilarity` calculator:

```rust
use libcpg::pattern::{GraphSimilarity, SimilarityMetric};

let similarity = GraphSimilarity::new()
    .with_metric(SimilarityMetric::WeisfeilerLehman)
    .with_structural_weight(0.7)
    .with_label_weight(0.3);

let score = similarity.similarity(&cpg1, &cpg2);
```

### Similarity Metrics

| Metric | Description | Best For |
|--------|-------------|----------|
| `Jaccard` | Set intersection / union of node types | Quick comparison |
| `Cosine` | Cosine of feature vectors | Normalized comparison |
| `WeisfeilerLehman` | Graph kernel using iterative labeling | Structural similarity |
| `GraphEdit` | Approximate edit distance | Fuzzy matching |

### Weisfeiler-Lehman Kernel

The WL kernel iteratively refines node labels based on neighbors:

```
Iteration 0: Labels = node types
Iteration 1: Labels = hash(type, sorted_neighbor_labels)
Iteration 2: Labels = hash(iter1_label, sorted_neighbor_labels)
...

After K iterations, compare label distributions
```

This captures K-hop neighborhood structure for each node.

## Performance

### Complexity

- **Worst case**: O(n! × n) where n = pattern size
- **Typical case**: O(n² × m) where m = target size
- **Best case**: O(n) with early pruning

### Optimization Strategies

**1. Use max_matches for early termination:**
```rust
let matcher = Vf2Matcher::new().with_max_matches(1);  // Stop at first match
let has_pattern = !matcher.find_matches(&pattern, &target).is_empty();
```

**2. Use strict matching to prune:**
```rust
let matcher = Vf2Matcher::new()
    .with_strict_kinds(true)    // Fewer compatible nodes
    .with_strict_edges(true);   // Stronger edge constraints
```

**3. Order pattern nodes strategically:**
- Rare node types first (more pruning power)
- High-degree nodes first (more edge constraints)

**4. Parallelize across patterns:**
```rust
use rayon::prelude::*;

let patterns: Vec<CodePropertyGraph> = load_patterns();
let all_matches: Vec<_> = patterns
    .par_iter()
    .flat_map(|p| matcher.find_matches(p, &target))
    .collect();
```

### Memory Usage

VF2 uses O(n + m) memory for:
- Mapping tables
- Terminal sets
- Node/edge iterators

No graph copying required - operates on references.

## Common Use Cases

### Find All If-Return Patterns

```rust
// Pattern: if (...) { return ...; }
let mut pattern = CodePropertyGraph::new(Language::Rust);
let if_id = pattern.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::If, SourceRange::default()));
let ret_id = pattern.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::Return, SourceRange::default()));
pattern.add_edge(CpgEdge::new(EdgeId::new(0), if_id, ret_id, CpgEdgeKind::AstChild));

let matches = Vf2Matcher::new().find_matches(&pattern, &target);
```

### Check If Pattern Exists

```rust
let matcher = Vf2Matcher::new();
let exists = matcher.contains_pattern(&pattern, &target);
```

### Find Up to N Matches

```rust
let matches = Vf2Matcher::new()
    .find_matches_limited(&pattern, &target, 5);  // At most 5 matches
```

## Next Steps

- [Gang of Four Patterns](gang-of-four.md) - Pattern catalog
- [Pattern Overview](overview.md) - Detection concepts
- [Graph Traversal](../graph/traversal.md) - Navigation patterns
