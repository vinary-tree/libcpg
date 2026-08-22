# ADR-0056: Cache exact edge projections lazily on the CPG

- **Status:** Accepted
- **Date:** 2026-08-19
- **Scope:** feature-free `graph::CodePropertyGraph` traversal substrate

## Context

The Code Property Graph (CPG) stores all typed relations in one petgraph
`DiGraph`. Historically, `cfg_successors`, `dfg_successors`, and
`edges_between` rediscovered a small relation by traversing petgraph adjacency,
filtering edge kinds, and collecting a new vector on every call. Algorithms
such as strongly connected components (SCC), Program Dependence Graph (PDG)
construction, graph projection, and VF2 matching perform these reads many times.
The repeated filtering and allocation were visible hot paths.

The existing public [`GraphProjection`](../components/graph/projections.md)
cannot simply replace those methods. It intentionally discards edge payloads,
merges parallel endpoint pairs, owns a dense `NodeId` mapping, and represents
one immutable analysis topology. CPG accessors must instead preserve every
exact `CpgEdge`, including typed sub-kind, label, stable `EdgeId`, and parallel
multiplicity, while the source graph remains mutable during construction.

The required design therefore has four simultaneous constraints:

1. repeated stable reads must be allocation-free and proportional to the
   selected row rather than all incident edge kinds;
2. results must be deterministic under node, edge, and explicit-id insertion
   permutations;
3. a mutation must never leave a raw petgraph edge index observable through a
   stale cache; and
4. cache construction, traversal, invalidation, clone, serialization, and
   destruction must not consume native call frames proportional to graph depth.

## Decision

1. Add one private `OnceLock<Box<CpgProjectionCache>>` to
   `CodePropertyGraph`. The first qualifying immutable read builds the cache;
   later reads reuse it.
2. Store exact petgraph edge indices as `u32`, not cloned `CpgEdge` payloads.
   Resolve a payload only when an iterator yields it.
3. Build three compressed-sparse-row (CSR) orders with one counting/scatter
   pass per order:

   - forward rows owned by source, sorted by overlay, target `NodeId`,
     `EdgeId`, and raw edge index;
   - reverse rows owned by target, sorted by overlay, source `NodeId`,
     `EdgeId`, and raw edge index; and
   - source-owned endpoint rows sorted by target `NodeId`, overlay, `EdgeId`,
     and raw edge index, with aligned target ids for binary partitioning.

4. Classify exact edges into AST, CFG, DFG, PDG, call, and other layers. Obtain
   one layer from a forward/reverse row with two binary partition searches.
5. Expose borrowed `CpgEdges`, `CfgNeighbors`, and `DfgNeighbors` views. They
   are cloneable, double-ended, exact-size iterators. CFG/DFG views provide
   `to_vec()` only as an explicit caller-selected ownership boundary.
6. Preserve the owned `GraphProjection` as a separate public abstraction.
   Projection constructors may consume cached exact edges, then normalize
   nodes, merge endpoint pairs, and own the result.
7. Invalidate before `add_node`, `add_node_with_id`, `node_mut`, `add_edge`, or
   `add_edge_with_id`. `connect` reaches the edge insertion path. Clone and
   deserialization create an empty cache; Serde omits the derived field.
8. Keep build-and-append algorithms on crate-private native petgraph adjacency.
   A mutation loop must not build the global cache immediately before adding
   one edge and invalidating it.
9. Implement construction and lookup with finite loops, heap vectors,
   `partition_point`, and row-local `sort_unstable_by`. No graph-depth recursion,
   helper thread, larger-stack escape, semantic depth cap, or fallback path is
   introduced.

![The lazy exact-edge cache from first read through mutation and rebuild](../diagrams/projection-cache-lifecycle.svg)

*Figure — cache lifecycle and the boundary between mutation-native adjacency
and stable cached analysis reads. Source:
[`diagrams/projection-cache-lifecycle.puml`](../diagrams/projection-cache-lifecycle.puml).*

## Construction contract

The algorithm is stated in literate form so ordering and lifetime invariants
remain reviewable together:

```text
BUILD_EXACT_CACHE(graph):
    enumerate every live node as (stable NodeId, raw NodeIndex)
    sort that list by stable NodeId
    enumerate every live edge into a finite RawEdge record
    for each of forward, reverse, and by-target orders:
        count row degrees into checked u32 offsets
        prefix-sum offsets
        scatter raw-edge positions into contiguous rows
        sort only rows whose degree is at least two by the order's stable key
        replace raw-edge positions with exact raw EdgeIndex values
    align each by-target edge index with its target NodeId
    return the heap-owned cache
```

Let $`V`$ be node count, $`E`$ edge count, and $`d_r`$ the degree of row $`r`$.
The build uses $`O(V+E)`$ space and
$`O(V+E+\sum_r d_r\log d_r)`$ time. Warm whole-row lookup is $`O(1)`$ plus
iteration; layer and endpoint selection add $`O(\log d_r)`$ partition work.
These bounds describe represented graph size, not source depth.

## Rejected alternatives

### Return newly collected vectors

Rejected. This was the prior behavior and allocated once per CFG/DFG/endpoint
query. The controlled traversal fixture performed hundreds of thousands of
such allocations across ten profile repetitions.

### Cache copied `CpgEdge` payloads

Rejected. It duplicates labels and edge metadata, separates the view from the
authoritative graph weight, and makes invalidation more expensive. Exact raw
indices preserve one payload owner.

### Reuse `GraphProjection` as the cache

Rejected. `GraphProjection` has intentionally different semantics: parallel
edge merging, optional numeric weight aggregation, a selected vertex universe,
and independent ownership. Treating it as exact CPG adjacency would erase
evidence.

### Maintain the cache incrementally during mutation

Rejected. CSR insertion is not local: inserting an edge shifts a contiguous
suffix and may reorder one row. A mutable delta layer would complicate every
read, persistence boundary, and determinism proof. Construction is already
append-heavy and analysis is read-heavy, so lazy invalidation matches the
actual phase boundary.

### Use one row order for both layer and endpoint lookup

Rejected. Ordering by overlay makes layer slices contiguous but interleaves
targets across layers; ordering by target makes endpoint slices contiguous but
interleaves layers. A secondary source-owned edge-index order is the simplest
exact solution and benchmarked substantially faster than dereferencing an edge
payload inside every binary-search comparison.

### Rebuild the cache inside mutation-oriented algorithms

Rejected. This caused the cache to be rebuilt after individual edge additions,
turning a lazy read optimization into repeated whole-graph work. Native
petgraph adjacency is the correct substrate while topology is changing; cached
rows are the correct substrate after the mutation phase stabilizes.

### Hide cold-cache cost behind eager construction

Rejected. Many callers construct a graph without invoking cached accessors.
Eager construction would impose time and retained memory on them. `OnceLock`
makes the cost explicit at the first relevant read and allows mutation to
discard it safely.

## Consequences

### Positive

- CFG, DFG, and endpoint reads are allocation-free, exact-size, and
  deterministic.
- Exact parallel edges and all payloads remain authoritative in one graph.
- SCC, PDG, VF2, projection, and statistics consumers share one read substrate.
- Mutation invalidation is centralized and serialization remains free of
  derived implementation state.
- Cache build, lookup, clone, invalidation, and drop are stack-safe with respect
  to graph depth and high degree.

### Negative

- The first cached read pays a whole-graph build cost even if the caller then
  performs only one small lookup.
- Once built, three edge-index orders and aligned target ids increase peak live
  heap. The controlled 8,192-node traversal fixture increased DHAT peak live
  bytes from 2,999,920 to 4,310,480 (+43.69%).
- `CodePropertyGraph` grows from 184 to 200 bytes because it retains the lazy
  lock and box pointer, although this remains smaller than the 216-byte
  pre-H05 graph.
- A borrowed view prevents simultaneous mutable access. Callers that genuinely
  need mutation must intentionally collect or call `to_vec()` first.
- Cache invalidation is conservative for `node_mut`, because public node fields
  include identity and cached traversal ordering must never assume a mutation
  was harmless.

## Verification

The H07 campaign compares every cached accessor with direct edge filtering over
256 generated graphs, compares logical results across 256 insertion-order
permutations, and repeats the oracle over parser-built Rust graphs after call
resolution and PDG construction. Exact examples pin parallel-edge ordering,
unknown nodes, mutation invalidation, serde omission/legacy input, and
double-ended/exact-size behavior. A 100,000-node graph with a 99,999-edge row
builds and traverses on a 256 KiB native stack.

Against immutable `main`, Criterion reports 30.83–37.71% lower time for
1,024-node CFG/DFG reads, 18.33–18.93% lower time at 8,192 nodes, and
44.99–45.71% lower time for endpoint lookup. Complete corpus analysis improves
6.74%, 8.54%, and 11.01% on the small Rust, medium Rust, and cross-language
cases; the 5,000-line Rust result is within the configured noise threshold.
Headless DHAT records 66.61% fewer allocation blocks and 2.12% fewer total
allocated bytes, while exposing the retained-peak cost above. Full measurements
and falsification boundaries are in
[scientific validation 51](../scientific/51-projection-cache-validation.md).

## References

1. Barrett, R., Berry, M., Chan, T. F., Demmel, J., Donato, J., Dongarra, J.,
   Eijkhout, V., Pozo, R., Romine, C., and van der Vorst, H. (1994).
   *Templates for the Solution of Linear Systems: Building Blocks for Iterative
   Methods* (2nd ed.). SIAM. DOI:
   [10.1137/1.9781611971538](https://doi.org/10.1137/1.9781611971538).
