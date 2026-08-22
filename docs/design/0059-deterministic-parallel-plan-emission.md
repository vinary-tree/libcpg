# ADR-0059: Parallelize immutable plans and retain one ordered emitter

- **Status:** Accepted
- **Date:** 2026-08-19
- **Scope:** per-function CFG, DFG, and PDG construction; ordered VF2 pattern batches; tree-sitter builder configuration

## Context

Control-flow graph (CFG), data-flow graph (DFG), and program-dependence graph
(PDG) construction previously iterated functions serially even though one
function's analysis reads no mutable result produced for another function.
Likewise, the VF2 subgraph-isomorphism engine could match one pattern at a time,
but had no ordered batch API. The crate already depended on Rayon, a Rust data-
parallel library, without using a parallel iterator.

Naively mutating one `CodePropertyGraph` (CPG) from several workers would create
lock contention, unstable edge identifiers, and schedule-dependent ordering.
Cloning the complete graph per function would replace contention with
quadratic-scale memory traffic. Neither approach preserves libcpg's exact
ordered semantics.

The required contract is stronger than set equality. For any admitted input
and any positive worker count, serial and parallel execution must preserve:

1. the same node and edge sequence, including edge identifiers and typed kinds;
2. the same CFG entry and exit order;
3. the same pattern-batch order and each pattern's VF2 embedding order;
4. idempotency in the presence of already-emitted edges; and
5. native-stack use independent of AST/CFG depth.

## Definitions

A **plan** is a heap-owned, function-local sequence of proposed edges and
metadata computed from an immutable CPG. An **indexed parallel iterator** is a
Rayon iterator whose collected output retains the input's positional order. An
**ordered emitter** is the single serial phase that applies plans in canonical
function order. VF2 uses the same reduction shape but returns ordered result
vectors instead of mutating a graph.

![Immutable workers produce indexed plans; one ordered emitter preserves exact graph order.](../diagrams/parallel-plan-emit.svg)

*Figure — the plan/emit architecture. Source:
[`diagrams/parallel-plan-emit.puml`](../diagrams/parallel-plan-emit.puml).*

## Decision

1. `CfgExtractor`, `DfgExtractor`, and `PdgBuilder` compute private owned plans
   from `&CodePropertyGraph`. They never clone the graph and never share mutable
   plan state.
2. Existing serial APIs remain serial. New `extract_parallel` and
   `build_all_parallel` methods execute plan discovery through indexed Rayon
   iterators on the caller's current thread pool.
3. Both serial and parallel all-function paths collect plans in function order,
   snapshot existing PDG edges once, and emit plans serially. This removes the
   former `PdgBuilder::build_all` whole-edge rescan per function without making
   the fair serial baseline use a weaker algorithm.
4. `Vf2Matcher::find_matches_many` and `find_matches_many_parallel` return one
   `Vec<PatternMatch>` per input pattern. Indexed collection preserves the outer
   pattern order; the existing stack-safe VF2 machine preserves each inner
   embedding order.
5. `CpgBuilderConfig::with_parallel_analysis(true)` makes the tree-sitter
   pipeline use parallel CFG and DFG planning. The default remains `false`
   because pool dispatch is not free for small single-function inputs.
6. Thread count, affinity, and worker-stack size remain caller policy. A caller
   installs a custom `rayon::ThreadPool` around the API when those controls are
   required. libcpg does not mutate Rayon's process-global pool.
7. Every worker continues to use the existing heap-resident pushdown machines
   and iterative graph traversals. Parallel dispatch introduces no recursive
   wrapper and tests run workers with 256 KiB stacks.

## Literate algorithm

The algorithm makes the mutation boundary explicit:

```text
ORDERED_PARALLEL_ANALYSIS(cpg, stable_items):
    plans <- indexed_parallel_map(stable_items, item -> PLAN(cpg, item))
    existing <- SNAPSHOT_EXISTING_OUTPUT(cpg)

    for plan in plans:                 // original stable_items order
        for proposed_output in plan:   // plan-local canonical order
            if existing.insert(proposed_output.identity):
                EMIT(cpg, proposed_output)

    return cpg
```

The serial variant replaces `indexed_parallel_map` with ordinary `map`; all
later steps are identical. VF2 replaces `PLAN` with `find_matches` and returns
the collected vector directly because matching is read-only.

## Correctness argument

Let the stable work sequence be $`I=(i_0,\ldots,i_{n-1})`$ and let
$`P(i_k)`$ be the deterministic plan computed from the same immutable graph
$`G`$. Indexed collection guarantees the plan sequence
$`(P(i_0),\ldots,P(i_{n-1}))`$ regardless of completion order. The emitter then
applies the same sequence and the same `connect_unique` predicate as the serial
path. By induction on the emitted output sequence, both executions have the
same graph after every emission prefix and therefore the same final graph.

This proof depends on plan purity. A plan may read AST, CFG, DFG, call, field,
and projection evidence that existed before the phase; it may not read another
plan's proposed output. The implementation audits and tests that boundary.

## Work, span, memory, and stack bounds

Let $`n`$ be independent items, $`w_i`$ the work for item $`i`$, $`e`$ the
total proposed outputs, and $`p`$ workers. Plan work is
$`W=\sum_{i=0}^{n-1}w_i`$. Ordered emission is $`O(e)`$ and intentionally
serial. The idealized span is:

```math
T_p = O\left(\frac{W}{p}+\max_i w_i+e\right)
```

The owned plans require $`O(e+n)`$ additional heap storage. No graph is cloned.
The serial path also plans before emission so it shares the same semantic and
asymptotic architecture. Native call-stack use is $`O(1)`$ with respect to
input depth; heap use retains the existing analysis-specific bounds.

Amdahl's law limits speedup when ordered emission or one unusually large
function dominates. Consequently, parallelism is explicit rather than
unconditionally enabled.

## Public usage

Tree-sitter construction can opt in directly:

```rust
// requires: features = ["lang-rust"]
use libcpg::{CpgBuilder, CpgBuilderConfig, Language, TreeSitterCpgBuilder};

let source = "fn f(x: i32) -> i32 { x + 1 }";
let builder = TreeSitterCpgBuilder::with_config(
    CpgBuilderConfig::default().with_parallel_analysis(true),
);
let cpg = builder.build(source, Language::Rust)?;
# Ok::<(), libcpg::Error>(())
```

Applications that own scheduling install a pool around explicit APIs:

```rust
use libcpg::{CfgExtractor, DfgExtractor, PdgBuilder};

# fn analyze(mut cpg: libcpg::CodePropertyGraph) {
let pool = rayon::ThreadPoolBuilder::new()
    .num_threads(8)
    .stack_size(256 * 1024)
    .build()
    .expect("valid Rayon pool");
pool.install(|| {
    CfgExtractor::new().extract_parallel(&mut cpg);
    DfgExtractor::new().extract_parallel(&mut cpg);
    PdgBuilder::new().build_all_parallel(&mut cpg);
});
# }
```

## Rejected alternatives

### Lock one shared graph

Fine- or coarse-grained graph locks would serialize edge insertion, expose
schedule order through edge identifiers, and make deadlock/poison behavior part
of a formerly pure analysis boundary. Ordered emission is simpler and exact.

### Clone the graph per worker

A full clone per function multiplies node, edge, string, and projection-cache
traffic by the function count. Plans retain only proposed outputs.

### Collect through an unordered channel

Sorting completed plans by function id would add work and still require every
plan to carry a key. Indexed collection already expresses the intended order
and applies equally to caller-supplied pattern slices.

### Enable parallel construction by default

The one-worker benchmark measures about 0.4% dispatch overhead on the controlled
many-function fixture. Smaller files offer less work to amortize it. Explicit
opt-in retains predictable small-input behavior.

## Security and resource boundary

Parallel execution changes simultaneous resource pressure, not admission
policy. Callers processing untrusted corpora must still bound file size, graph
size, pattern count, and match caps. They should select a worker count compatible
with available cores and memory. A caller-supplied Rayon pool is also the place
to bound worker stack reservation. Plans contain owned node identifiers and edge
kinds only; workers do not retain mutable graph references beyond the call.

## Consequences

- Multi-function construction and multi-pattern matching scale without changing
  serialized output.
- The implementation carries temporary plan storage and one ordered emission
  phase; workloads dominated by emission cannot scale linearly.
- `PdgBuilder::build_all` is materially faster even in serial mode because one
  existing-edge snapshot replaces a graph-wide rescan per function.
- Public single-function and single-pattern APIs remain unchanged.

The controlled evidence and falsification boundaries are recorded in
[scientific validation 54](../scientific/54-parallel-analysis-validation.md).

## References

1. Blumofe, R. D., and Leiserson, C. E. (1999). *Scheduling Multithreaded
   Computations by Work Stealing.* Journal of the ACM 46(5), 720–748. DOI:
   [10.1145/324133.324234](https://doi.org/10.1145/324133.324234).
2. Amdahl, G. M. (1967). *Validity of the Single Processor Approach to Achieving
   Large Scale Computing Capabilities.* AFIPS Conference Proceedings. DOI:
   [10.1145/1465482.1465560](https://doi.org/10.1145/1465482.1465560).
3. Knuth, D. E. (1984). *Literate Programming.* The Computer Journal 27(2),
   97–111. DOI: [10.1093/comjnl/27.2.97](https://doi.org/10.1093/comjnl/27.2.97).
