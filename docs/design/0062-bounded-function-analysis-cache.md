# ADR-0062: Bound exact per-function analysis reuse

- **Status:** Accepted
- **Date:** 2026-08-23
- **Scope:** caller-owned CFG/DFG/PDG memoization, exact invalidation, deterministic bounds, stack-safe lifecycle

## Context

An editor or indexer repeatedly rebuilds a code property graph (CPG) after a
small source edit. The abstract syntax tree (AST) must be rebuilt because every
`NodeId` belongs to one graph, but unchanged functions often produce the same
control-flow graph (CFG), data-flow graph (DFG), and program-dependence graph
(PDG) plans. Recomputing those plans wastes work. Reusing graph-specific node
identifiers would instead be unsound: insertion before a function can renumber
every later node.

A usable cache must therefore satisfy five forces simultaneously:

1. reuse only when every semantic dependency is represented;
2. compare complete canonical material rather than trust a digest;
3. retain function-local coordinates rather than foreign `NodeId` values;
4. obey caller-selected memory and entry bounds deterministically; and
5. keep graph depth, width, eviction, clearing, and destruction off the native
   call stack.

![Caller-owned cache lifecycle from fresh AST through exact key comparison and local-plan replay.](../diagrams/function-analysis-cache-lifecycle.svg)

*Figure -- exact cache lifecycle. Source:
[`diagrams/function-analysis-cache-lifecycle.puml`](../diagrams/function-analysis-cache-lifecycle.puml).*

## Decision

`FunctionAnalysisCache` is an explicit mutable argument. It has no global,
thread-local, filesystem, or process-persistent state. Its default bounds are
4,096 entries and 64 MiB of estimated resident key, plan, and entry-record
bytes; `FunctionAnalysisCacheLimits` lets the caller select both bounds.

### Canonical identity

Every key starts with a domain string and schema version, then records:

- the analysis family (CFG, DFG, or PDG);
- language and every builder option that can alter the result;
- exact function source bytes;
- the function-local node count and nested-function boundary; and
- for caller-supplied trees, local node tags, relative ranges, child
  cardinalities, child ordinals, and unknown-kind names.

DFG keys also encode local call sites and argument ordinals. A DFG plan is not
cached if it depends on an external caller, a preceding lexical binding, a
nested function, or receiver-field resolution. PDG keys include the sorted,
typed CFG/DFG overlay; an edge crossing the local boundary causes a fresh
build.

The 64-bit FNV-1a value is only a `BTreeMap` index. Every lookup compares the
full retained key bytes. A digest collision can affect search cost but never
correctness.

### Local plans and deterministic retention

Cached plans replace each graph `NodeId` with its ordinal in the canonical
function-local node sequence. Replay first validates every ordinal, expands it
against the new graph, and then emits edges in deterministic plan order.
Fresh plans are validated as local before localization and insertion.

Entries carry a monotonically increasing use clock. If either bound is
exceeded, the entry with the smallest clock is evicted; digest and bucket
position break impossible clock ties deterministically. A zero entry or byte
limit selects the ordinary builder path and records one capacity bypass.

## Literate algorithm

The algorithm separates authoritative reconstruction from optional reuse:

```text
BUILD_WITH_EXACT_CACHE(source, language, cache):
    parse source and rebuild a fresh AST
    if cache has zero capacity:
        run ordinary CFG and DFG extraction
        return graph

    CONTEXTS <- one iterative ownership scan over parser-built nodes
    for each function context in stable order:
        KEY <- canonical semantic dimensions and exact source bytes
        if full KEY equals one retained key:
            validate local plan ordinals
            replay plan against the new graph
        else:
            compute the ordinary plan
            emit the ordinary plan
            if every endpoint is function-local:
                translate endpoints to local ordinals
                insert under KEY
                evict least-recently-used entries until both bounds hold
    return graph
```

Caller-supplied trees use the same machine, but context construction includes
tree shape because source bytes alone do not prove which tree the caller
supplied.

## Correctness argument

Let $`G`$ be a newly built graph, $`f`$ one function, $`K(G,f)`$ its canonical
key, $`P(G,f)`$ the ordinary analysis plan, and $`L_G`$ the bijection from
function-local node identifiers to local ordinals. Insertion stores
$`L_G(P(G,f))`$ only when every plan endpoint lies in the domain of $`L_G`$.

For a later graph $`G'`$, a hit requires byte equality
$`K(G,f)=K(G',f')`$, not merely digest equality. The key includes every admitted
dependency; dependencies that cannot be represented cause a bypass. Therefore
the ordinary local plan is unchanged up to local identity:

```math
P(G',f') = L_{G'}^{-1}\!\left(L_G(P(G,f))\right).
```

Replay validates the codomain of $`L_{G'}^{-1}`$ before mutation. Consequently
cached emission equals ordinary emission, while a malformed ordinal, missing
key dimension, or cross-boundary dependency fails closed to recomputation.

## Resource bounds and stack safety

For entry limit $`E`$, estimated-byte limit $`B`$, graph nodes $`N`$, graph
edges $`M`$, and largest function-local plan size $`P`$, retained cache memory
is bounded by both configured ceilings. Canonicalization and replay are
$`O(N+M)`$ over the admitted build, while a cache lookup is
$`O(\log E + C)`$, where $`C`$ is the size of the digest-collision bucket.
Because $`E`$ is bounded, so is $`C`$.

AST ownership, malformed-cycle detection, plan conversion, LRU eviction,
`clear`, and `Drop` all use explicit heap vectors, maps, and loops. No
input-shaped recursion is introduced. The standard-library balanced-tree depth
is logarithmic in the caller-bounded entry count rather than graph depth.

## Alternatives considered

### Cache complete graphs or graph-specific identifiers

Rejected. A harmless edit before a function can renumber nodes, and retaining
whole graphs duplicates AST/source ownership.

### Trust a hash or use a cryptographic digest

Rejected. No finite digest proves equality. Retaining and comparing canonical
bytes gives exact collision behavior; a stronger digest would add cost without
removing the comparison requirement.

### Cache every DFG by expanding the key to the whole program

Rejected. It would turn a per-function cache into a whole-program snapshot and
make small external edits invalidate large keys. Exact conservative bypasses
keep ownership and invalidation comprehensible.

### Process-global or thread-local cache

Rejected. Hidden lifetime, cross-tenant retention, synchronization, and
unbounded high-water memory conflict with explicit resource control.

### Recursive traversal or recursive destruction

Rejected. Source nesting and graph width are untrusted input dimensions.
Explicit heap worklists provide the same semantics with constant native call
depth.

## Consequences

- unchanged 64-function reanalysis removes 17.10% of retired instructions;
- a cold cache adds 1.32% retired instructions and 1.43 MB of allocation
  traffic for the measured corpus;
- exact keys are 37.98% of normal resident cache bytes, a visible cost rather
  than hidden metadata;
- edits with unrepresented dependencies recompute exactly and expose bypass
  counters;
- callers must retain and mutably pass the cache whose lifetime they want; and
- changing key or plan semantics requires incrementing the internal schema
  version.

Complete test and measurement evidence is in
[scientific validation 57](../scientific/57-function-analysis-cache-validation.md).

## References

1. Michie, D. (1968). *Memo Functions and Machine Learning.* Nature 218,
   19--22. DOI:
   [10.1038/218019a0](https://doi.org/10.1038/218019a0).
2. Mattson, R. L., Gecsei, J., Slutz, D. R., and Traiger, I. L. (1970).
   *Evaluation Techniques for Storage Hierarchies.* IBM Systems Journal 9(2),
   78--117. DOI:
   [10.1147/sj.92.0078](https://doi.org/10.1147/sj.92.0078).
3. Knuth, D. E. (1984). *Literate Programming.* The Computer Journal 27(2),
   97--111. DOI:
   [10.1093/comjnl/27.2.97](https://doi.org/10.1093/comjnl/27.2.97).
