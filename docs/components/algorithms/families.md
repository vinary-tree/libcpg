# Algorithm Families

libcpg recognizes 14 algorithm families based on structural patterns in Code Property Graphs. This document details each family's characteristics and detection criteria.

## Family Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                     Algorithm Family Taxonomy                    │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                    Data Processing                        │   │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐ │   │
│  │  │ Sorting  │  │Searching │  │ Hashing  │  │Compress. │ │   │
│  │  └──────────┘  └──────────┘  └──────────┘  └──────────┘ │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                    Graph Algorithms                       │   │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐               │   │
│  │  │Traversal │  │ Shortest │  │   MST    │               │   │
│  │  │(BFS/DFS) │  │  Path    │  │          │               │   │
│  │  └──────────┘  └──────────┘  └──────────┘               │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                   Design Paradigms                        │   │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐ │   │
│  │  │ Divide & │  │ Dynamic  │  │  Greedy  │  │Backtrack │ │   │
│  │  │ Conquer  │  │ Program. │  │          │  │          │ │   │
│  │  └──────────┘  └──────────┘  └──────────┘  └──────────┘ │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                  │
│  ┌─────────────────────────────────────────────────────────┐   │
│  │                    Specialized                            │   │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐               │   │
│  │  │ String   │  │Numerical │  │   ML     │               │   │
│  │  │ Matching │  │          │  │          │               │   │
│  │  └──────────┘  └──────────┘  └──────────┘               │   │
│  └─────────────────────────────────────────────────────────┘   │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

## The AlgorithmFamily Enum

```rust
pub enum AlgorithmFamily {
    Sorting,              // Ordering elements
    Searching,            // Finding elements
    GraphTraversal,       // BFS, DFS
    ShortestPath,         // Dijkstra, Bellman-Ford
    MinimumSpanningTree,  // Prim, Kruskal
    DynamicProgramming,   // Optimal substructure
    DivideAndConquer,     // Recursive decomposition
    Greedy,               // Local optimum choices
    Backtracking,         // Exhaustive search with pruning
    StringMatching,       // Pattern searching in text
    Hashing,              // Hash-based operations
    Compression,          // Data compression
    Numerical,            // Mathematical computations
    MachineLearning,      // ML algorithms
    Unknown,              // Unclassified
}
```

## Data Processing Families

### Sorting

Algorithms that arrange elements in a specific order.

**Typical Complexity**: O(n log n) for comparison-based, O(n) for counting-based

**Detection Criteria**:
- Nested loops with comparison operations
- Swap patterns (temp = a; a = b; b = temp)
- Recursive decomposition with merge operations
- Partitioning logic (pivot selection)

**Structural Signatures**:
```rust
// Bubble/Selection/Insertion Sort pattern
LoopStructure {
    max_depth: 2,
    loops: vec![
        LoopType { kind: CountedFor, bounds: LinearN, depth: 0 },
        LoopType { kind: CountedFor, bounds: LinearN, depth: 1, has_early_exit: true },
    ],
}

// Quicksort/Mergesort pattern
RecursionPattern {
    kind: RecursionKind::Binary,
    reduction: ReductionPattern::Division(2),
    base_case: Some(base_node),
}
```

**Example Detection**:
```rust
let algorithms = detector.detect(&cpg, function_id);
for algo in algorithms.iter().filter(|a| a.family == AlgorithmFamily::Sorting) {
    println!("Sorting algorithm detected:");
    if let Some(name) = &algo.name {
        println!("  Name: {}", name);  // e.g., "quicksort", "mergesort"
    }
    println!("  Complexity: {:?}", algo.signature.time_complexity);
}
```

**Common Algorithms**:
| Algorithm | Complexity | Signature |
|-----------|------------|-----------|
| Bubble Sort | O(n²) | Nested loops, adjacent swaps |
| Insertion Sort | O(n²) | Nested loops, shift pattern |
| Selection Sort | O(n²) | Nested loops, min finding |
| Merge Sort | O(n log n) | Binary recursion, merge |
| Quick Sort | O(n log n) avg | Binary recursion, partition |
| Heap Sort | O(n log n) | Heapify operations |

### Searching

Algorithms that locate elements in data structures.

**Typical Complexity**: O(log n) to O(n)

**Detection Criteria**:
- Single loop with early exit on match
- Binary division of search space
- Comparison with target value
- Index manipulation (halving, bounds adjustment)

**Structural Signatures**:
```rust
// Linear search
LoopStructure {
    max_depth: 1,
    loops: vec![
        LoopType { kind: WhileLoop, bounds: LinearN, has_early_exit: true },
    ],
}

// Binary search
LoopStructure {
    max_depth: 1,
    loops: vec![
        LoopType { kind: WhileLoop, bounds: Logarithmic, has_early_exit: true },
    ],
}
```

**Common Algorithms**:
| Algorithm | Complexity | Signature |
|-----------|------------|-----------|
| Linear Search | O(n) | Single loop, early exit |
| Binary Search | O(log n) | Halving loop, bounds update |
| Jump Search | O(√n) | Block jumping + linear |
| Interpolation | O(log log n) avg | Position estimation |

### Hashing

Algorithms using hash functions for fast lookup.

**Typical Complexity**: O(1) average, O(n) worst case

**Detection Criteria**:
- Hash function computation
- Modulo operations for indexing
- Collision resolution (chaining or probing)
- Key-value pair operations

**Structural Signatures**:
```rust
// Hash table insert/lookup
LoopStructure {
    max_depth: 1,
    loops: vec![
        LoopType { kind: WhileLoop, bounds: Constant, has_early_exit: true },
    ],
}
```

### Compression

Algorithms for data compression and decompression.

**Typical Complexity**: O(n) to O(n log n)

**Detection Criteria**:
- Frequency counting operations
- Tree/heap construction
- Bit manipulation
- Dictionary building

**Common Algorithms**:
| Algorithm | Complexity | Signature |
|-----------|------------|-----------|
| Huffman | O(n log n) | Heap-based tree construction |
| LZW | O(n) | Dictionary building |
| Run-Length | O(n) | Sequential scan |

## Graph Algorithm Families

### Graph Traversal

Algorithms for visiting all vertices in a graph.

**Typical Complexity**: O(V + E)

**Detection Criteria**:
- Queue (BFS) or stack (DFS) data structure
- Visited set tracking
- Neighbor iteration
- Recursive calls or explicit stack

**Structural Signatures**:
```rust
// BFS pattern
LoopStructure {
    max_depth: 2,
    loops: vec![
        LoopType { kind: WhileLoop, bounds: LinearN, depth: 0 },
        LoopType { kind: ForEach, bounds: Variable, depth: 1 },
    ],
}

// DFS pattern (recursive)
RecursionPattern {
    kind: RecursionKind::Direct,
    reduction: ReductionPattern::Constant(1),
}
```

**Example Detection**:
```rust
// Find all graph traversal implementations
for func in cpg.functions() {
    let algorithms = detector.detect(&cpg, func.id());
    for algo in algorithms.iter().filter(|a| a.family == AlgorithmFamily::GraphTraversal) {
        // Distinguish BFS from DFS
        if algo.signature.recursion_pattern.is_some() {
            println!("{}: DFS (recursive)", func.name());
        } else {
            println!("{}: BFS (iterative)", func.name());
        }
    }
}
```

### Shortest Path

Algorithms for finding minimum-cost paths in graphs.

**Typical Complexity**: O(E log V) to O(V³)

**Detection Criteria**:
- Distance array/map maintenance
- Priority queue operations
- Edge relaxation pattern
- Negative cycle detection (Bellman-Ford)

**Common Algorithms**:
| Algorithm | Complexity | Signature |
|-----------|------------|-----------|
| Dijkstra | O(E log V) | Priority queue, relaxation |
| Bellman-Ford | O(VE) | V-1 iterations over edges |
| Floyd-Warshall | O(V³) | Triple nested loops |
| A* | O(E log V) | Priority queue with heuristic |

### Minimum Spanning Tree

Algorithms for finding minimum-weight spanning trees.

**Typical Complexity**: O(E log V)

**Detection Criteria**:
- Edge sorting or priority queue
- Union-Find operations (Kruskal)
- Cut property exploitation
- Tree construction

**Common Algorithms**:
| Algorithm | Complexity | Signature |
|-----------|------------|-----------|
| Kruskal | O(E log E) | Edge sort, Union-Find |
| Prim | O(E log V) | Priority queue, cut property |
| Boruvka | O(E log V) | Component merging |

## Design Paradigm Families

### Divide and Conquer

Algorithms that recursively break problems into subproblems.

**Typical Complexity**: O(n log n)

**Detection Criteria**:
- Binary or multiple recursive calls
- Problem division step
- Solution combination step
- Base case for small inputs

**Structural Signatures**:
```rust
RecursionPattern {
    kind: RecursionKind::Binary,
    reduction: ReductionPattern::Division(2),
    base_case: Some(base_node),
    tail_optimizable: false,
}
```

**Example**:
```rust
// Detect divide-and-conquer algorithms
for algo in algorithms {
    if algo.family == AlgorithmFamily::DivideAndConquer {
        if let Some(ref rec) = algo.signature.recursion_pattern {
            match rec.reduction {
                ReductionPattern::Division(2) =>
                    println!("Binary divide (like mergesort)"),
                ReductionPattern::Division(k) =>
                    println!("{}-way divide", k),
                _ => {}
            }
        }
    }
}
```

**Common Algorithms**:
| Algorithm | Complexity | Signature |
|-----------|------------|-----------|
| Merge Sort | O(n log n) | Binary recursion, linear merge |
| Quick Sort | O(n log n) avg | Binary recursion, partition |
| Strassen | O(n^2.807) | 7-way matrix recursion |
| Karatsuba | O(n^1.585) | 3-way multiplication |

### Dynamic Programming

Algorithms that solve problems by combining solutions to overlapping subproblems.

**Typical Complexity**: Varies (often polynomial)

**Detection Criteria**:
- Memoization table (array or map)
- Recurrence relation pattern
- Bottom-up or top-down structure
- Optimal substructure exploitation

**Structural Signatures**:
```rust
// Bottom-up DP
LoopStructure {
    max_depth: 2,  // Often 2D table
    loops: vec![
        LoopType { kind: CountedFor, bounds: LinearN, depth: 0 },
        LoopType { kind: CountedFor, bounds: LinearN, depth: 1 },
    ],
}

// Top-down DP (memoization)
RecursionPattern {
    kind: RecursionKind::Multiple,
    reduction: ReductionPattern::Constant(1),
    base_case: Some(base_node),
}
```

**Common Algorithms**:
| Algorithm | Complexity | Signature |
|-----------|------------|-----------|
| Fibonacci (memo) | O(n) | Linear recursion with cache |
| Knapsack | O(nW) | 2D table filling |
| LCS | O(nm) | 2D table, diagonal access |
| Edit Distance | O(nm) | 2D table, 3-way min |
| Floyd-Warshall | O(n³) | 3D iteration pattern |

### Greedy

Algorithms that make locally optimal choices at each step.

**Typical Complexity**: O(n log n) typically

**Detection Criteria**:
- Sorting or priority queue for selection
- Single-pass decision making
- No backtracking
- Selection based on local criterion

**Structural Signatures**:
```rust
LoopStructure {
    max_depth: 1,
    loops: vec![
        LoopType { kind: ForEach, bounds: LinearN, has_early_exit: false },
    ],
}
// Often preceded by sorting
```

**Common Algorithms**:
| Algorithm | Complexity | Signature |
|-----------|------------|-----------|
| Huffman | O(n log n) | Heap-based selection |
| Prim's MST | O(E log V) | Priority queue |
| Activity Selection | O(n log n) | Sort + linear scan |
| Fractional Knapsack | O(n log n) | Ratio-based selection |

### Backtracking

Algorithms that explore all possibilities with pruning.

**Typical Complexity**: O(k^n) exponential

**Detection Criteria**:
- Recursive exploration with state
- Constraint checking before recursion
- State backtrack (undo) after recursion
- Early termination on constraint violation

**Structural Signatures**:
```rust
RecursionPattern {
    kind: RecursionKind::Multiple,
    reduction: ReductionPattern::Constant(1),
    base_case: Some(base_node),
    branches: k,  // Multiple recursive calls
}
```

**Example Detection**:
```rust
for algo in algorithms {
    if algo.family == AlgorithmFamily::Backtracking {
        // Check for exponential complexity
        if let Some(ref tc) = algo.signature.time_complexity {
            if matches!(tc.class, ComplexityClass::Exponential) {
                println!("Warning: Exponential backtracking detected");
                println!("  Consider pruning or memoization");
            }
        }
    }
}
```

**Common Algorithms**:
| Algorithm | Complexity | Signature |
|-----------|------------|-----------|
| N-Queens | O(n!) | Constraint checking, backtrack |
| Sudoku Solver | O(9^81) worst | Cell-by-cell exploration |
| Graph Coloring | O(k^n) | Color assignment with check |
| Subset Sum | O(2^n) | Include/exclude branching |

## Specialized Families

### String Matching

Algorithms for pattern searching in text.

**Typical Complexity**: O(n + m) to O(nm)

**Detection Criteria**:
- Character-by-character comparison
- Failure function or skip table
- Prefix/suffix matching
- Rolling hash computation

**Common Algorithms**:
| Algorithm | Complexity | Signature |
|-----------|------------|-----------|
| Naive | O(nm) | Nested loops, char compare |
| KMP | O(n + m) | Failure function, linear scan |
| Rabin-Karp | O(n + m) avg | Rolling hash comparison |
| Boyer-Moore | O(n/m) best | Skip table, right-to-left |

### Numerical

Mathematical computation algorithms.

**Typical Complexity**: Varies widely

**Detection Criteria**:
- Mathematical operations (sqrt, pow, mod)
- Matrix operations
- Iterative refinement
- Convergence testing

**Common Algorithms**:
| Algorithm | Complexity | Signature |
|-----------|------------|-----------|
| GCD (Euclidean) | O(log min(a,b)) | Division/modulo loop |
| Power (fast) | O(log n) | Halving exponent |
| Matrix Mult | O(n³) or O(n^2.807) | Triple/recursive loops |
| Newton-Raphson | O(log n) iterations | Convergence loop |

### Machine Learning

Algorithms for learning from data.

**Typical Complexity**: Varies by algorithm

**Detection Criteria**:
- Gradient computation
- Loss function evaluation
- Iterative optimization
- Matrix/tensor operations

**Common Patterns**:
| Pattern | Signature |
|---------|-----------|
| Gradient Descent | Iterative update loop |
| K-Means | Cluster assignment + centroid update |
| Decision Tree | Recursive splitting |
| Neural Network | Layer-by-layer propagation |

## Working with Families

### Filtering by Family

```rust
use libcpg::algorithms::{DefaultAlgorithmDetector, AlgorithmFamily};

let detector = DefaultAlgorithmDetector::new()
    .with_families(vec![
        AlgorithmFamily::Sorting,
        AlgorithmFamily::Searching,
    ]);

let algorithms = detector.detect(&cpg, function_id);
```

### Family Metadata

```rust
impl AlgorithmFamily {
    pub fn typical_complexity(&self) -> ComplexityClass {
        match self {
            Self::Sorting => ComplexityClass::Linearithmic,
            Self::Searching => ComplexityClass::Logarithmic,
            Self::GraphTraversal => ComplexityClass::Linear,
            Self::DynamicProgramming => ComplexityClass::Polynomial(2),
            Self::Backtracking => ComplexityClass::Exponential,
            // ...
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Sorting => "Algorithms that arrange elements in order",
            Self::Searching => "Algorithms that locate elements",
            // ...
        }
    }
}
```

### Cross-Family Analysis

Some algorithms belong to multiple paradigms:

```rust
// Quicksort is both Sorting AND Divide-and-Conquer
// Dijkstra is both ShortestPath AND Greedy
// Memoized DFS is both GraphTraversal AND DynamicProgramming

fn analyze_paradigms(algo: &DetectedAlgorithm) -> Vec<AlgorithmFamily> {
    let mut families = vec![algo.family.clone()];

    // Check for additional paradigm indicators
    if algo.signature.recursion_pattern.as_ref()
        .map(|r| matches!(r.kind, RecursionKind::Binary))
        .unwrap_or(false)
    {
        families.push(AlgorithmFamily::DivideAndConquer);
    }

    families
}
```

## Next Steps

- [Complexity Analysis](complexity.md) - Complexity estimation
- [Algorithm Overview](overview.md) - Detection concepts
- [Pattern Detection](../patterns/overview.md) - Design patterns
