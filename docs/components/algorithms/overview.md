# Algorithm Detection Overview

libcpg provides tools for identifying algorithm families and estimating computational complexity from code structure analysis.

## What is Algorithm Detection?

Algorithm detection identifies common algorithmic patterns in code by analyzing:

- **Loop structures**: Nesting depth, bounds, iteration patterns
- **Recursion patterns**: Direct, tail, binary recursion
- **Data access patterns**: Sequential, random, tree-structured
- **Control flow signatures**: Characteristic branch patterns

```
┌─────────────────────────────────────────────────────────────────┐
│                   Algorithm Detection Pipeline                   │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│   Source Code                                                    │
│       │                                                          │
│       ▼                                                          │
│   ┌─────────────┐                                               │
│   │    CPG      │                                               │
│   │ Construction│                                               │
│   └──────┬──────┘                                               │
│          │                                                       │
│          ▼                                                       │
│   ┌─────────────────────────────────────────────────────────┐   │
│   │              Feature Extraction                          │   │
│   │  ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐     │   │
│   │  │  Loop   │  │Recursion│  │  Data   │  │ Control │     │   │
│   │  │Analysis │  │Analysis │  │ Access  │  │  Flow   │     │   │
│   │  └────┬────┘  └────┬────┘  └────┬────┘  └────┬────┘     │   │
│   │       └────────────┴────────────┴────────────┘          │   │
│   └─────────────────────────┬───────────────────────────────┘   │
│                             │                                    │
│                             ▼                                    │
│   ┌─────────────────────────────────────────────────────────┐   │
│   │              Algorithm Signature                         │   │
│   │  • Loop structure                                        │   │
│   │  • Recursion pattern                                     │   │
│   │  • Complexity estimate                                   │   │
│   │  • Feature vector                                        │   │
│   └─────────────────────────┬───────────────────────────────┘   │
│                             │                                    │
│                             ▼                                    │
│   ┌─────────────────────────────────────────────────────────┐   │
│   │              Classification                              │   │
│   │  Match against known algorithm families                  │   │
│   └─────────────────────────┬───────────────────────────────┘   │
│                             │                                    │
│                             ▼                                    │
│   ┌─────────────────────────────────────────────────────────┐   │
│   │              Detected Algorithms                         │   │
│   │  • Family: Sorting                                       │   │
│   │  • Name: quicksort (if identifiable)                     │   │
│   │  • Complexity: O(n log n)                                │   │
│   │  • Confidence: 0.85                                      │   │
│   └─────────────────────────────────────────────────────────┘   │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

## Core Types

### AlgorithmSignature

Captures the structural characteristics of an algorithm:

```rust
pub struct AlgorithmSignature {
    /// Loop structure characteristics
    pub loop_structure: Option<LoopStructure>,
    /// Recursion pattern (if any)
    pub recursion_pattern: Option<RecursionPattern>,
    /// Estimated time complexity
    pub time_complexity: Option<ComplexityEstimate>,
    /// Estimated space complexity
    pub space_complexity: Option<ComplexityEstimate>,
    /// Feature vector for ML classification
    pub feature_vector: Vec<f32>,
}
```

### DetectedAlgorithm

Represents a detected algorithm instance:

```rust
pub struct DetectedAlgorithm {
    /// The algorithm family
    pub family: AlgorithmFamily,
    /// Specific algorithm name (if identifiable)
    pub name: Option<String>,
    /// The function containing this algorithm
    pub function: NodeId,
    /// Key nodes in the implementation
    pub key_nodes: Vec<NodeId>,
    /// Extracted signature
    pub signature: AlgorithmSignature,
    /// Confidence score (0.0 to 1.0)
    pub confidence: f64,
}
```

## Quick Start

### Basic Detection

```rust
use libcpg::algorithms::{DefaultAlgorithmDetector, AlgorithmDetector};

// Create detector
let detector = DefaultAlgorithmDetector::new()
    .with_min_confidence(0.6);

// Detect algorithms in a function
let algorithms = detector.detect(&cpg, function_node_id);

for algo in algorithms {
    println!("Detected: {} ({})", algo.family, algo.confidence);

    if let Some(name) = &algo.name {
        println!("  Specific algorithm: {}", name);
    }

    if let Some(ref complexity) = algo.signature.time_complexity {
        println!("  Time complexity: {}", complexity.class);
    }
}
```

### Building Signatures

```rust
use libcpg::algorithms::{AlgorithmSignature, ComplexityEstimate, ComplexityClass};
use libcpg::algorithms::signatures::{LoopStructure, LoopType, LoopKind, LoopBounds};

// Build a signature for a sorting algorithm
let signature = AlgorithmSignature::new()
    .with_loop_structure(
        LoopStructure::new()
            .with_max_depth(2)
            .with_loop(LoopType {
                header: outer_loop_id,
                kind: LoopKind::CountedFor,
                depth: 0,
                bounds: LoopBounds::LinearN,
                has_early_exit: false,
            })
            .with_loop(LoopType {
                header: inner_loop_id,
                kind: LoopKind::CountedFor,
                depth: 1,
                bounds: LoopBounds::LinearN,
                has_early_exit: true,
            })
    )
    .with_time_complexity(ComplexityEstimate {
        class: ComplexityClass::Quadratic,
        confidence: 0.8,
        justification: "Nested loops, each O(n)".to_string(),
    });
```

### Analyzing Recursion

```rust
use libcpg::algorithms::signatures::{RecursionPattern, RecursionKind, ReductionPattern};

// Identify binary recursion (like mergesort)
let pattern = RecursionPattern::new(RecursionKind::Binary)
    .with_base_case(base_case_node)
    .with_recursive_call(left_call_node)
    .with_recursive_call(right_call_node)
    .with_reduction(ReductionPattern::Division(2))
    .with_tail_optimizable(false);
```

## Algorithm Families

libcpg recognizes these algorithm families:

| Family | Examples | Typical Complexity |
|--------|----------|-------------------|
| Sorting | Quicksort, Mergesort, Bubblesort | O(n log n) |
| Searching | Binary search, Linear search | O(log n) to O(n) |
| Graph Traversal | BFS, DFS | O(V + E) |
| Shortest Path | Dijkstra, Bellman-Ford | O(E log V) |
| Minimum Spanning Tree | Prim, Kruskal | O(E log V) |
| Dynamic Programming | Knapsack, LCS | Varies |
| Divide and Conquer | Mergesort, Strassen | O(n log n) |
| Greedy | Huffman, Prim | O(n log n) |
| Backtracking | N-Queens, Sudoku | O(k^n) |
| String Matching | KMP, Rabin-Karp | O(n + m) |

## Detection Methods

### Loop Analysis

Analyzes loop structures to determine complexity:

```rust
// Nested loops → polynomial complexity
// Single loop → linear complexity
// Logarithmic bounds → log(n) complexity

let loop_info = LoopStructure::new()
    .with_max_depth(depth)
    .with_loop(loop_type);
```

Key indicators:
- **Nesting depth**: Contributes to polynomial factor
- **Loop bounds**: Linear (n), logarithmic (log n), constant
- **Independence**: Can loops run in parallel?

### Recursion Analysis

Identifies recursion patterns:

```rust
pub enum RecursionKind {
    Direct,   // f(n) calls f(n-1)
    Indirect, // f() calls g() calls f()
    Tail,     // Recursive call is last operation
    Binary,   // Two recursive calls (divide and conquer)
    Multiple, // More than two calls
}
```

Reduction patterns:
- **Constant**: n - k (linear recursion)
- **Linear**: n - 1 (linear time)
- **Division**: n / k (logarithmic depth)

### Complexity Estimation

Estimates complexity from structural features:

```rust
use libcpg::algorithms::detection::ComplexityAnalyzer;

let analyzer = ComplexityAnalyzer::new();
let time = analyzer.estimate_time_complexity(&cpg, function_id);
let space = analyzer.estimate_space_complexity(&cpg, function_id);

println!("Time: {} (confidence: {:.0}%)",
         time.class, time.confidence * 100.0);
println!("Space: {} (confidence: {:.0}%)",
         space.class, space.confidence * 100.0);
```

## Use Cases

### Code Quality Analysis

Find inefficient algorithms:

```rust
for func in cpg.functions() {
    let algorithms = detector.detect(&cpg, func.id());

    for algo in algorithms {
        if let Some(ref tc) = algo.signature.time_complexity {
            if matches!(tc.class, ComplexityClass::Exponential | ComplexityClass::Factorial) {
                println!("Warning: {} has {} complexity",
                         func.name(), tc.class);
            }
        }
    }
}
```

### Algorithm Recommendations

Suggest better alternatives:

```rust
match algo.family {
    AlgorithmFamily::Sorting => {
        if algo.signature.time_complexity.as_ref()
            .map(|c| c.class == ComplexityClass::Quadratic)
            .unwrap_or(false)
        {
            println!("Consider using a O(n log n) sorting algorithm");
        }
    }
    // ...
}
```

### Educational Tools

Identify and explain algorithm patterns:

```rust
if let Some(ref recursion) = algo.signature.recursion_pattern {
    match recursion.kind {
        RecursionKind::Tail =>
            println!("This uses tail recursion - can be optimized to a loop"),
        RecursionKind::Binary =>
            println!("This is divide-and-conquer with binary recursion"),
        _ => {}
    }
}
```

## Feature Flags

Algorithm detection is included by default but can be disabled:

```toml
[dependencies]
libcpg = { version = "0.1", default-features = false }
# To enable:
libcpg = { version = "0.1", features = ["algorithm-detection"] }
```

## Next Steps

- [Algorithm Families](families.md) - Detailed family descriptions
- [Complexity Analysis](complexity.md) - Complexity estimation
- [Pattern Detection](../patterns/overview.md) - Design patterns
