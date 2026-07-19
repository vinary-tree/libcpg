# Complexity Estimation

libcpg estimates algorithmic complexity by analyzing loop structures, recursion patterns, and data access patterns in Code Property Graphs.

## How Complexity Estimation Works

```
┌─────────────────────────────────────────────────────────────────┐
│                  Complexity Estimation Pipeline                  │
├─────────────────────────────────────────────────────────────────┤
│                                                                  │
│   Function CPG                                                   │
│       │                                                          │
│       ├───────────────────────────────────────┐                 │
│       │                                       │                 │
│       ▼                                       ▼                 │
│   ┌───────────────────┐             ┌───────────────────┐      │
│   │   Loop Analysis   │             │ Recursion Analysis │      │
│   │                   │             │                    │      │
│   │ • Nesting depth   │             │ • Recursion kind   │      │
│   │ • Iteration count │             │ • Reduction factor │      │
│   │ • Early exits     │             │ • Base case depth  │      │
│   └─────────┬─────────┘             └──────────┬─────────┘      │
│             │                                   │                │
│             └───────────────┬───────────────────┘                │
│                             │                                    │
│                             ▼                                    │
│             ┌───────────────────────────────┐                   │
│             │     Complexity Combiner       │                   │
│             │                               │                   │
│             │ • Nested loops → multiply     │                   │
│             │ • Sequential → add            │                   │
│             │ • Recursion → recurrence      │                   │
│             └───────────────┬───────────────┘                   │
│                             │                                    │
│                             ▼                                    │
│             ┌───────────────────────────────┐                   │
│             │   ComplexityEstimate          │                   │
│             │   • class: O(n log n)         │                   │
│             │   • confidence: 0.85          │                   │
│             │   • justification: "..."      │                   │
│             └───────────────────────────────┘                   │
│                                                                  │
└─────────────────────────────────────────────────────────────────┘
```

## Core Types

### ComplexityClass

Represents Big-O complexity classes:

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

**Ordering**:
```
O(1) < O(log n) < O(n) < O(n log n) < O(n²) < O(n³) < O(2^n) < O(n!)
```

### ComplexityEstimate

Combines the estimated class with confidence and explanation:

```rust
pub struct ComplexityEstimate {
    /// The complexity class
    pub class: ComplexityClass,
    /// Confidence score (0.0 to 1.0)
    pub confidence: f64,
    /// Human-readable justification
    pub justification: String,
}
```

**Example**:
```rust
ComplexityEstimate {
    class: ComplexityClass::Quadratic,
    confidence: 0.9,
    justification: "Two nested loops, each iterating n times".to_string(),
}
```

## Using the ComplexityAnalyzer

### Basic Usage

```rust
use libcpg::algorithms::detection::ComplexityAnalyzer;

let analyzer = ComplexityAnalyzer::new();

// Estimate time complexity
let time = analyzer.estimate_time_complexity(&cpg, function_id);
println!("Time: {} (confidence: {:.0}%)", time.class, time.confidence * 100.0);

// Estimate space complexity
let space = analyzer.estimate_space_complexity(&cpg, function_id);
println!("Space: {} (confidence: {:.0}%)", space.class, space.confidence * 100.0);
```

### Configuration Options

```rust
let analyzer = ComplexityAnalyzer::new()
    // Consider input size variable 'n' comes from first parameter
    .with_input_parameter(0)
    // Assume arrays are of size n
    .with_array_size_assumption(ArraySizeAssumption::LinearN)
    // Include recursive call analysis
    .with_recursion_analysis(true);
```

## Loop Analysis

The analyzer examines loop structures to estimate iteration counts.

### Loop Bounds Detection

```rust
pub enum LoopBounds {
    Constant,       // Fixed iterations (e.g., for i in 0..10)
    LinearN,        // O(n) iterations
    Logarithmic,    // O(log n) iterations (halving pattern)
    Variable,       // Unknown bounds
}
```

**Detection heuristics**:

```rust
// Constant bounds
for i in 0..10 { ... }              // Constant

// Linear bounds
for i in 0..n { ... }               // LinearN
for item in items { ... }           // LinearN (array iteration)
while !done { i += 1; ... }         // LinearN (linear progression)

// Logarithmic bounds
while low < high { mid = (low+high)/2; ... }  // Logarithmic (binary search)
while n > 0 { n /= 2; ... }                    // Logarithmic (halving)
```

### Nesting Analysis

Nested loops multiply their complexities:

```rust
// O(n²) - two nested linear loops
for i in 0..n {
    for j in 0..n {
        // ...
    }
}

// O(n log n) - linear outer, logarithmic inner
for i in 0..n {
    while j > 0 {
        j /= 2;
    }
}

// O(n) - outer linear, inner constant
for i in 0..n {
    for j in 0..5 {  // constant inner
        // ...
    }
}
```

**Implementation**:
```rust
fn analyze_nested_loops(&self, loops: &[LoopType]) -> ComplexityClass {
    let mut complexity = ComplexityClass::Constant;

    for loop_type in loops {
        complexity = match (complexity, &loop_type.bounds) {
            (ComplexityClass::Constant, LoopBounds::Constant) => ComplexityClass::Constant,
            (ComplexityClass::Constant, LoopBounds::Logarithmic) => ComplexityClass::Logarithmic,
            (ComplexityClass::Constant, LoopBounds::LinearN) => ComplexityClass::Linear,
            (ComplexityClass::Linear, LoopBounds::LinearN) => ComplexityClass::Quadratic,
            (ComplexityClass::Linear, LoopBounds::Logarithmic) => ComplexityClass::Linearithmic,
            (ComplexityClass::Quadratic, LoopBounds::LinearN) => ComplexityClass::Polynomial(3),
            // ...
        };
    }

    complexity
}
```

### Early Exit Detection

Loops with early exits may have lower complexity:

```rust
// Linear search: O(n) worst, O(1) best
for i in 0..n {
    if arr[i] == target {
        return i;  // Early exit
    }
}

// Binary search: O(log n)
while low <= high {
    let mid = (low + high) / 2;
    if arr[mid] == target {
        return mid;  // Early exit
    }
    // ...
}
```

## Recursion Analysis

The analyzer uses the Master Theorem and recurrence relations to estimate recursive complexity.

### Recursion Patterns

```rust
pub enum RecursionKind {
    Direct,   // f(n) calls f(n-k)
    Indirect, // f() calls g() calls f()
    Tail,     // Recursive call is last operation
    Binary,   // Two recursive calls (divide and conquer)
    Multiple, // More than two recursive calls
}

pub enum ReductionPattern {
    Constant(u32),  // n - k (linear reduction)
    Linear(u32),    // n - c*k
    Division(u32),  // n / k
    Custom,         // Unknown pattern
}
```

### Master Theorem Application

For recurrences of the form `T(n) = aT(n/b) + f(n)`:

```rust
fn apply_master_theorem(
    &self,
    num_calls: usize,      // a
    division_factor: u32,  // b
    combine_work: ComplexityClass, // f(n)
) -> ComplexityClass {
    let a = num_calls as f64;
    let b = division_factor as f64;
    let log_b_a = a.log(b);

    match combine_work {
        // Case 1: f(n) = O(n^c) where c < log_b(a)
        // T(n) = O(n^(log_b(a)))

        // Case 2: f(n) = O(n^(log_b(a)))
        // T(n) = O(n^(log_b(a)) * log n)

        // Case 3: f(n) = O(n^c) where c > log_b(a)
        // T(n) = O(f(n))

        // Simplified implementation
        ComplexityClass::Constant if log_b_a > 0.0 => {
            ComplexityClass::Polynomial(log_b_a.ceil() as u32)
        }
        ComplexityClass::Linear if num_calls == 2 && division_factor == 2 => {
            ComplexityClass::Linearithmic  // O(n log n)
        }
        _ => combine_work,
    }
}
```

### Common Recursion Patterns

| Pattern | Recurrence | Complexity |
|---------|------------|------------|
| Linear recursion | T(n) = T(n-1) + O(1) | O(n) |
| Tail recursion | T(n) = T(n-1) + O(1) | O(n) [optimizable to O(1) space] |
| Binary recursion | T(n) = 2T(n/2) + O(n) | O(n log n) |
| Binary search | T(n) = T(n/2) + O(1) | O(log n) |
| Tree recursion | T(n) = 2T(n-1) + O(1) | O(2^n) |

**Examples**:
```rust
// T(n) = T(n-1) + O(1) → O(n)
fn factorial(n: u64) -> u64 {
    if n <= 1 { 1 } else { n * factorial(n - 1) }
}

// T(n) = 2T(n/2) + O(n) → O(n log n)
fn merge_sort(arr: &mut [i32]) {
    if arr.len() <= 1 { return; }
    let mid = arr.len() / 2;
    merge_sort(&mut arr[..mid]);
    merge_sort(&mut arr[mid..]);
    merge(arr, mid);  // O(n) work
}

// T(n) = 2T(n-1) + O(1) → O(2^n)
fn fibonacci(n: u32) -> u32 {
    if n <= 1 { n } else { fibonacci(n-1) + fibonacci(n-2) }
}
```

## Space Complexity

Space analysis considers:
- Stack depth (recursion)
- Allocated data structures
- Auxiliary arrays

### Stack Space

```rust
fn estimate_stack_space(&self, pattern: &RecursionPattern) -> ComplexityClass {
    match pattern.reduction {
        ReductionPattern::Division(k) => ComplexityClass::Logarithmic,  // O(log n)
        ReductionPattern::Constant(1) => ComplexityClass::Linear,       // O(n)
        _ => ComplexityClass::Linear,
    }
}
```

### Heap Space

```rust
fn estimate_heap_space(&self, cpg: &CodePropertyGraph, func: NodeId) -> ComplexityClass {
    // Look for array allocations
    let allocations = self.find_allocations(cpg, func);

    let mut max_space = ComplexityClass::Constant;
    for alloc in allocations {
        let size = self.estimate_allocation_size(alloc);
        max_space = max_space.max(size);
    }

    max_space
}
```

## Confidence Scoring

The analyzer provides confidence scores based on analysis certainty.

### Confidence Factors

| Factor | High Confidence | Low Confidence |
|--------|-----------------|----------------|
| Loop bounds | Clearly derived from n | Variable/unknown bounds |
| Recursion | Clear reduction pattern | Complex mutual recursion |
| Early exits | None | Multiple conditional exits |
| Data access | Sequential | Pointer-based |

### Combining Confidences

```rust
fn combine_confidence(&self, loop_conf: f64, rec_conf: f64) -> f64 {
    // Geometric mean for combining independent estimates
    (loop_conf * rec_conf).sqrt()
}
```

## Practical Examples

### Analyzing a Sorting Function

```rust
use libcpg::algorithms::detection::ComplexityAnalyzer;

let analyzer = ComplexityAnalyzer::new();

// Analyze bubble sort
let bubble_time = analyzer.estimate_time_complexity(&cpg, bubble_sort_id);
assert!(matches!(bubble_time.class, ComplexityClass::Quadratic));
println!("Bubble sort: {} - {}", bubble_time.class, bubble_time.justification);
// Output: "Bubble sort: O(n²) - Two nested loops, each iterating up to n times"

// Analyze quicksort
let quick_time = analyzer.estimate_time_complexity(&cpg, quicksort_id);
assert!(matches!(quick_time.class, ComplexityClass::Linearithmic));
println!("Quicksort: {} - {}", quick_time.class, quick_time.justification);
// Output: "Quicksort: O(n log n) - Binary recursion with n/2 reduction"
```

### Finding Inefficient Code

```rust
fn find_inefficient_functions(cpg: &CodePropertyGraph) -> Vec<(String, ComplexityEstimate)> {
    let analyzer = ComplexityAnalyzer::new();
    let mut warnings = Vec::new();

    for func in cpg.functions() {
        let time = analyzer.estimate_time_complexity(cpg, func.id());

        // Flag potentially inefficient algorithms
        match time.class {
            ComplexityClass::Exponential | ComplexityClass::Factorial => {
                warnings.push((func.name().to_string(), time));
            }
            ComplexityClass::Polynomial(k) if k >= 3 => {
                warnings.push((func.name().to_string(), time));
            }
            _ => {}
        }
    }

    warnings
}

// Usage
let warnings = find_inefficient_functions(&cpg);
for (name, estimate) in warnings {
    println!("Warning: {} has {} complexity", name, estimate.class);
    println!("  Reason: {}", estimate.justification);
}
```

### Comparing Implementations

```rust
fn compare_implementations(
    cpg: &CodePropertyGraph,
    func1: NodeId,
    func2: NodeId,
) {
    let analyzer = ComplexityAnalyzer::new();

    let time1 = analyzer.estimate_time_complexity(cpg, func1);
    let time2 = analyzer.estimate_time_complexity(cpg, func2);

    println!("Implementation 1: {}", time1.class);
    println!("Implementation 2: {}", time2.class);

    if time1.class < time2.class {
        println!("Implementation 1 is asymptotically faster");
    } else if time2.class < time1.class {
        println!("Implementation 2 is asymptotically faster");
    } else {
        println!("Both have the same asymptotic complexity");
    }
}
```

## Limitations

### What the Analyzer Cannot Determine

1. **Input distribution**: Average-case vs. worst-case requires knowing input distribution
2. **Constant factors**: O(100n) vs O(n) both appear as O(n)
3. **Amortized complexity**: Requires understanding of operation sequences
4. **Cache effects**: Memory hierarchy not modeled
5. **Parallelism**: Assumes sequential execution

### Improving Accuracy

```rust
// Provide hints for better analysis
let analyzer = ComplexityAnalyzer::new()
    // Hint: main input is first parameter
    .with_input_parameter(0)
    // Hint: this is a graph algorithm, edges ≈ O(V²)
    .with_graph_assumption(GraphAssumption::Dense)
    // Hint: hash operations are O(1) amortized
    .with_hash_assumption(HashAssumption::Amortized);
```

## Complexity Class Operations

### Ordering and Comparison

```rust
impl PartialOrd for ComplexityClass {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.order_value().cmp(&other.order_value()))
    }
}

impl ComplexityClass {
    fn order_value(&self) -> u32 {
        match self {
            Self::Constant => 0,
            Self::Logarithmic => 1,
            Self::Linear => 2,
            Self::Linearithmic => 3,
            Self::Quadratic => 4,
            Self::Polynomial(k) => 4 + k,
            Self::Exponential => 100,
            Self::Factorial => 101,
        }
    }
}
```

### Multiplication (Nested Loops)

```rust
impl ComplexityClass {
    pub fn multiply(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Constant, x) | (x, Self::Constant) => x.clone(),
            (Self::Logarithmic, Self::Linear) | (Self::Linear, Self::Logarithmic) =>
                Self::Linearithmic,
            (Self::Linear, Self::Linear) => Self::Quadratic,
            (Self::Linear, Self::Quadratic) | (Self::Quadratic, Self::Linear) =>
                Self::Polynomial(3),
            (Self::Polynomial(a), Self::Polynomial(b)) => Self::Polynomial(a + b),
            (Self::Exponential, _) | (_, Self::Exponential) => Self::Exponential,
            (Self::Factorial, _) | (_, Self::Factorial) => Self::Factorial,
            _ => Self::Polynomial(2),  // Conservative estimate
        }
    }
}
```

### Addition (Sequential Blocks)

```rust
impl ComplexityClass {
    pub fn add(&self, other: &Self) -> Self {
        // Take the larger complexity
        if self > other { self.clone() } else { other.clone() }
    }
}
```

## Display Formatting

```rust
impl std::fmt::Display for ComplexityClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Constant => write!(f, "O(1)"),
            Self::Logarithmic => write!(f, "O(log n)"),
            Self::Linear => write!(f, "O(n)"),
            Self::Linearithmic => write!(f, "O(n log n)"),
            Self::Quadratic => write!(f, "O(n²)"),
            Self::Polynomial(k) => write!(f, "O(n^{})", k),
            Self::Exponential => write!(f, "O(2^n)"),
            Self::Factorial => write!(f, "O(n!)"),
        }
    }
}
```

## Next Steps

- [Algorithm Families](families.md) - Algorithm categories
- [Algorithm Overview](overview.md) - Detection concepts
- [GNN Embeddings](../gnn/embeddings.md) - Code similarity
