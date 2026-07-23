# Complexity Analysis

`ComplexityAnalyzer` turns a function's loop-nesting and recursion shape into a **[Big-O complexity class](../../GLOSSARY.md#complexity-class--big-o)** with a confidence score and a human-readable justification. It is the phase-2 engine behind [algorithm detection](overview.md), and it is usable on its own. Like the rest of the module it is **heuristic**: it counts structural features, it does not solve recurrences symbolically.

> **Feature gate.** `ComplexityAnalyzer`, `ComplexityEstimate`, and `ComplexityClass` are part of the `algorithm-detection`-gated `algorithms` module. `libcpg`'s `default = []`, so enable the feature to use them (see [Overview](overview.md)).
>
> The type is **`ComplexityAnalyzer`** — there is no `ComplexityEstimator`.

## The `ComplexityClass` ladder

`ComplexityClass` is a 10-variant enum ordered from cheapest to most expensive. Each variant has an `as_str()` label, and the enum's `Display` impl delegates to it.

```rust
// requires: features = ["algorithm-detection"]
pub enum ComplexityClass {
    Constant,        // O(1)
    Logarithmic,     // O(log n)
    Linear,          // O(n)
    Linearithmic,    // O(n log n)
    Quadratic,       // O(n^2)
    Cubic,           // O(n^3)
    Polynomial(u32), // O(n^k)
    Exponential,     // O(2^n)
    Factorial,       // O(n!)
    Unknown,
}
```

The `as_str()` labels are fixed strings — note that a few use Unicode superscripts and that `Polynomial` returns a *constant* label rather than interpolating its `k`:

| Variant | `as_str()` returns | Meaning |
|---|---|---|
| `Constant` | `"O(1)"` | $`O(1)`$ |
| `Logarithmic` | `"O(log n)"` | $`O(\log n)`$ |
| `Linear` | `"O(n)"` | $`O(n)`$ |
| `Linearithmic` | `"O(n log n)"` | $`O(n \log n)`$ |
| `Quadratic` | `"O(n²)"` | $`O(n^2)`$ |
| `Cubic` | `"O(n³)"` | $`O(n^3)`$ |
| `Polynomial(k)` | `"O(n^k)"` (literal — `k` is **not** substituted) | $`O(n^k)`$ |
| `Exponential` | `"O(2^n)"` | $`O(2^n)`$ |
| `Factorial` | `"O(n!)"` | $`O(n!)`$ |
| `Unknown` | `"Unknown"` | undetermined |

The ladder is, from cheapest to dearest:

```math
O(1) < O(\log n) < O(n) < O(n \log n) < O(n^2) < O(n^3) < O(n^k) < O(2^n) < O(n!)
```

![The ComplexityClass ladder ordered from O(1) to O(n!), with each rung's Big-O label.](../../diagrams/complexity-ladder.svg)

*Figure — the `ComplexityClass` ordering. Source: [`diagrams/complexity-ladder.dot`](../../diagrams/complexity-ladder.dot).*

### Comparing classes

`ComplexityClass` does **not** implement `PartialOrd`/`Ord`, so `<`/`>` do not work on it. Ordering is exposed through one public method, `is_better_than`, which compares an internal ordinal:

```rust
// requires: features = ["algorithm-detection"]
use libcpg::algorithms::ComplexityClass;

assert!(ComplexityClass::Linear.is_better_than(&ComplexityClass::Quadratic));
assert!(!ComplexityClass::Exponential.is_better_than(&ComplexityClass::Linear));
```

The internal ordinal that backs `is_better_than` is:

| Class | Ordinal |
|---|---|
| `Constant` | 0 |
| `Logarithmic` | 1 |
| `Linear` | 2 |
| `Linearithmic` | 3 |
| `Quadratic` | 4 |
| `Cubic` | 5 |
| `Polynomial(k)` | $`5 + k`$ |
| `Exponential` | 100 |
| `Factorial` | 200 |
| `Unknown` | 1000 |

One quirk worth knowing: because `Polynomial(k)` scores $`5 + k`$, a `Polynomial(2)` (ordinal 7) is ranked *worse* than the dedicated `Quadratic` (ordinal 4) even though both denote $`O(n^2)`$. In practice the analyzer only ever emits `Polynomial(d)` for loop-nesting depth $`d \ge 4`$, so this rarely bites — but do not treat the two encodings of the same degree as interchangeable.

## `ComplexityEstimate`

Every estimate is a class plus its evidence:

```rust
// requires: features = ["algorithm-detection"]
pub struct ComplexityEstimate {
    pub class: ComplexityClass,
    pub confidence: f64,       // 0.0 ..= 1.0
    pub justification: String, // e.g. "Nested loops (depth 2) detected"
}
```

## Using the analyzer

The public surface is deliberately tiny — a constructor and two estimators. There are **no** configuration builders (no `with_input_parameter`, no assumption knobs); those never existed.

```rust
// requires: features = ["algorithm-detection"]
use libcpg::algorithms::detection::ComplexityAnalyzer;

let analyzer = ComplexityAnalyzer::new();

let time  = analyzer.estimate_time_complexity(&cpg, function_id);
let space = analyzer.estimate_space_complexity(&cpg, function_id);

println!("time:  {} ({:.0}% — {})", time.class, time.confidence * 100.0, time.justification);
println!("space: {} ({:.0}%)",      space.class, space.confidence * 100.0);
```

## How the time estimate is derived

`estimate_time_complexity` computes a loop estimate and a recursion estimate independently, then keeps the **worse** of the two.

```text
estimate_time_complexity(cpg, function):
  loops     ← detect_loops(cpg, function)
  recursion ← detect_recursion(cpg, function)          # None if not self-recursive

  loop_est ← analyze_loop_complexity(loops)            # None if there are no loops
  rec_est  ← recursion.map(analyze_recursion_complexity)

  match (loop_est, rec_est):
    (None,  None)  → Constant, confidence 0.9   # "No loops or recursion detected"
    (Some l, None) → l
    (None,  Some r) → r
    (Some l, Some r) → if r.class.is_better_than(l.class) then l else r   # keep the worse
```

![Activity flow mapping loop nesting and recursion shape to a ComplexityClass, backed conceptually by the Master Theorem.](../../diagrams/complexity-heuristics.svg)

*Figure — structure-to-complexity heuristics. Source: [`diagrams/complexity-heuristics.puml`](../../diagrams/complexity-heuristics.puml).*

### Loop heuristic

`analyze_loop_complexity` keys off the maximum loop-nesting depth and whether every loop is *counted* (bounded). A loop is counted when it is a `For` with a range/counting iterator, or a `While`/`DoWhile` with both a comparison and an increment.

| Max nesting depth | All loops counted? | Class | Confidence |
|---|---|---|---|
| 0 (no loops) | — | `Constant` | 0.9 |
| 1 | yes | `Linear` | 0.8 |
| 1 | no | `Linear` | 0.6 |
| 2 | yes | `Quadratic` | 0.8 |
| 2 | no | `Quadratic` | 0.5 |
| 3 | — | `Cubic` | 0.7 |
| $`d \ge 4`$ | — | `Polynomial(d)` | 0.6 |

### Recursion heuristic

`analyze_recursion_complexity` first asks whether the recursion looks *divide-and-conquer*: the analyzer scans the body for a division or right-shift by an integer literal `2`, or for an identifier named `mid`, `middle`, `pivot`, or containing `half`. It then keys off the recursion kind and the number of recursive call sites.

| Recursion kind | Divide-and-conquer cue? | Recursive calls | Class | Confidence |
|---|---|---|---|---|
| `Tail` | — | — | `Linear` | 0.8 |
| `Direct`/`Indirect` | yes | 1 | `Logarithmic` | 0.8 |
| `Direct`/`Indirect` | yes | 2 | `Linearithmic` | 0.7 |
| `Direct`/`Indirect` | yes | $`\ge 3`$ | `Polynomial(calls)` | 0.5 |
| `Direct`/`Indirect` | no | 1, with a base case | `Linear` | 0.7 |
| `Direct`/`Indirect` | no | 1, no clear base case | `Linear` | 0.4 |
| `Direct`/`Indirect` | no | 2 | `Exponential` | 0.7 |
| `Direct`/`Indirect` | no | $`\ge 3`$ | `Exponential` | 0.6 |

### The Master Theorem, and what the analyzer really does

The divide-and-conquer rows above are a shallow stand-in for the **[Master Theorem](../../GLOSSARY.md#master-theorem)** [[1]](#references), which solves recurrences of the form

```math
T(n) = a \, T(n/b) + f(n), \qquad a \ge 1,\ b > 1
```

by comparing the per-call work $`f(n)`$ against $`n^{\log_b a}`$. For example $`a = b = 2`$ with $`f(n) = O(n)`$ yields $`T(n) = O(n \log n)`$ — the merge-sort case.

`ComplexityAnalyzer` does **not** evaluate $`\log_b a`$ numerically. It has no view of $`b`$ beyond "is there a `/2` or a `mid`/`pivot`", and it approximates $`a`$ by counting recursive call sites. So it recognises the *shape* $`a\,T(n/b)+f(n)`$ and maps one call to $`O(\log n)`$ and two to $`O(n \log n)`$, but it cannot classify, say, Karatsuba's three-way split or Strassen's seven-way split correctly — both fall into the $`\ge 3`$ bucket as `Polynomial(calls)`.

### Honesty: `Factorial` is never emitted

The `ComplexityClass::Factorial` variant exists (so callers can pattern-match on it and so it has a Big-O label), but **the shipped `ComplexityAnalyzer` never produces it**. Non-divide-and-conquer recursion tops out at `Exponential`, and loop nesting tops out at `Polynomial(depth)`. If you are checking for pathological cost, match on `Exponential` — a hit on `Factorial` will not occur from this analyzer.

## Worked examples

The table below shows real functions, the recurrence they embody, and the class the analyzer *actually* assigns.

| Function | Recurrence | Emitted class |
|---|---|---|
| `fn factorial(n){ if n<=1 {1} else { n*factorial(n-1) } }` | $`T(n)=T(n-1)+O(1)`$ | `Linear` (0.7) |
| `fn fib(n){ if n<=1 {n} else { fib(n-1)+fib(n-2) } }` | $`T(n)=2T(n-1)+O(1)`$ | `Exponential` (0.7) |
| `fn merge_sort(a){ …; merge_sort(l); merge_sort(r); merge(a) }` | $`T(n)=2T(n/2)+O(n)`$ | `Linearithmic` (0.7) |
| `fn bsearch(a,x){ let mid=(lo+hi)/2; …; bsearch(...) }` | $`T(n)=T(n/2)+O(1)`$ | `Logarithmic` (0.8) |
| two counted nested `for` loops | $`\sum O(n)\cdot O(n)`$ | `Quadratic` (0.8) |

A crucial disambiguation lives in the first row: the function *called* `factorial` runs in $`O(n)`$ time (it performs $`n`$ multiplications), so the analyzer's `Linear` verdict is correct. That is completely unrelated to the `Factorial` complexity **class** $`O(n!)`$, which — as noted above — the analyzer never assigns to anything.

For `merge_sort`, the merge loop also produces a `Linear` loop estimate; the combine rule keeps the worse of `Linear` and `Linearithmic`, yielding `Linearithmic`.

## Space complexity

`estimate_space_complexity` is a separate public method (the detection pipeline does not call it, but you can):

- **Recursive** functions: `Tail` recursion → `Constant` (a compiler could reuse the frame); `Direct`/`Indirect` → `Linear` (stack depth), confidence `0.6`.
- **Non-recursive** functions: if the body allocates a collection (an array literal, or a call whose callee name contains `vec`, `new`, `alloc`, `create`, `clone`, or `collect`) → `Linear`, confidence `0.5`; otherwise `Constant`, confidence `0.7`.

## Limits

The analyzer models structure, not semantics. It cannot see:

1. **Input distribution** — average vs worst case are indistinguishable.
2. **Constant factors** — $`O(100n)`$ and $`O(n)`$ both read as `Linear`.
3. **Amortised behaviour** — it reasons per-call, not over operation sequences.
4. **Data-dependent bounds** — a `while` without a clear counter is treated conservatively.
5. **Genuine $`O(n!)`$ cost** — not represented in output at all.

Treat every `ComplexityEstimate` as a hint whose `confidence` and `justification` fields tell you how much to trust it.

## Related reading

- [Overview](overview.md) — where the complexity estimate feeds the family detectors.
- [Algorithm families](families.md) — the family gates that consume these classes.
- [Cyclomatic complexity](../../GLOSSARY.md#cyclomatic-complexity) — a distinct, exact structural metric (`cyclomatic_complexity()` over the CFG), not to be confused with these Big-O estimates.
- [Theory: algorithm & complexity analysis](../../theory/08-algorithm-and-complexity-analysis.md) and [API: pattern & analysis reference](../../api/pattern-reference.md).

## References

1. Cormen, T. H., Leiserson, C. E., Rivest, R. L., Stein, C. (2009). *Introduction to Algorithms* (3rd ed.). MIT Press. ISBN 978-0262033848 (no DOI). *(Master Theorem, Chapter 4.)*
