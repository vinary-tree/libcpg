# ADR-0060: Canonicalize mapped strings within each CPG build

- **Status:** Accepted
- **Date:** 2026-08-19
- **Scope:** tree-sitter Mode-A and Mode-B AST construction, mapped `Arc<str>` ownership, internal hashing dependencies

## Context

Tree-sitter frontends preserve names, type spellings, operators, literals,
imports, and unknown grammar-kind labels as `Arc<str>` fields in
`CpgNodeKind`, `MethodSignature`, and `TypeInfo`. An **atomically reference-
counted string** (`Arc<str>`) is an immutable UTF-8 string allocation whose
small cloned handles share ownership. The public representation is useful:
cloning a graph does not copy string bytes, and Serde persists ordinary string
contents. The mapper, however, previously created a fresh allocation for every
occurrence even when thousands of nodes carried equal text.

An inventory of three real Rust modules found 28,510 mapped string occurrences
and 1,810 distinct values. Every occurrence had its own allocation. The graph
therefore retained 26,700 avoidable allocations and 261,622 avoidable payload
bytes, before allocator metadata and `Arc` control blocks were counted.

The optimization must not replace public field types, change debug or
serialization contents, introduce process-global mutable state, retain a pool
after construction, or map source depth onto native recursion. It must also
select a hasher from measured production-like evidence rather than dependency
comments.

## Definitions

A **canonical string allocation** is the one `Arc<str>` allocation selected for
all equal mapped contents during one graph build. A **build-local pool** is a
temporary set that exists only inside `TreeSitterCpgBuilder::build_ast`. It is
not a process-wide symbol table and does not cross a CPG boundary. **String
identity** below means content equality; pointer equality remains an internal
storage observation, not a public semantic contract.

![Each mapped node consults one temporary pool before entering the CPG; the pool is dropped before later analyses.](../diagrams/build-local-string-pool.svg)

*Figure — complete Mode-A/Mode-B ownership flow. Source:
[`diagrams/build-local-string-pool.puml`](../diagrams/build-local-string-pool.puml).*

## Decision

1. Every tree-sitter AST build creates one private `CpgStringPool` backed by
   `FxHashSet<Arc<str>>`.
2. Immediately after mapping one `CpgNode`, and before appending it to the CPG,
   a fixed-depth visitor canonicalizes every mapper-populated string position:
   node-kind fields, function names and parameter/return types, variable and
   field types, generic spellings, optional node text, and allocation type.
3. A borrowed `&str` lookup avoids allocating a key. If equal content already
   exists, the visitor clones its `Arc` handle into the node and releases the
   duplicate allocation. Otherwise it stores one clone as the canonical
   handle.
4. The pool is dropped when AST construction ends. Only graph-owned handles
   remain. Separate builds deliberately retain separate ownership domains.
5. `rustc_hash::FxHashSet` remains the internal hasher. A controlled comparison
   found it faster than `AHashSet` for both the measured string distribution and
   a 100,000-key integer map. The unused `ahash` dependency is removed.
6. The unused `string_cache` dependency is removed. No global interner, mutex,
   leaked string, or hidden lifetime is introduced.
7. Direct calls to the public `NodeMapper::map_kind` retain their established
   standalone behavior. Canonicalization is a `TreeSitterCpgBuilder` storage
   optimization, not a pointer-identity promise on mapper results.

## Literate algorithm

The algorithm intentionally operates on one completed node at a time. This
keeps the public mapper API unchanged and releases duplicate ownership before
the node becomes persistent graph state.

```text
CANONICALIZE_NODE(pool, node):
    for field in FLAT_MAPPED_STRING_FIELDS(node):
        // Hash and compare the existing UTF-8 contents without allocating a key.
        if pool contains canonical where canonical.contents = field.contents:
            field <- clone_handle(canonical)
        else:
            pool.insert(clone_handle(field))

BUILD_AST(tree):
    pool <- empty FxHashSet<Arc<str>>
    work <- [(tree.root, no_parent)]

    while work is not empty:
        frame <- work.pop()
        if frame.syntax_node is included:
            node <- MAP_ONE_NODE(frame.syntax_node)
            CANONICALIZE_NODE(pool, node)
            APPEND_NODE_AND_AST_EDGE(node, frame.parent)
        PUSH_CHILDREN_IN_REVERSE_SOURCE_ORDER(work, frame)

    drop(pool)
```

`FLAT_MAPPED_STRING_FIELDS` is an exhaustive match over `CpgNodeKind`, plus
ordinary loops over parameter and generic `SmallVec` values. Adding a new node
variant forces the production match to be updated at compile time.

## Correctness argument

Let $`s_0,\ldots,s_{m-1}`$ be the mapped string contents in visitation order.
After processing prefix $`0..k`$, maintain the invariant that the pool contains
exactly one handle for each distinct content in that prefix and every processed
graph field points to the matching allocation. The empty prefix satisfies the
invariant. For $`s_k`$, a successful content lookup clones the existing handle;
an unsuccessful lookup inserts the first handle for that content. Neither case
changes bytes. Induction therefore establishes the invariant for all fields.

Dropping the pool decreases each canonical strong count once but cannot free a
value still owned by a graph field. Consequently, content, clone behavior,
debug formatting, and serialization are unchanged, while equal fields within
one build share storage.

## Work, memory, and stack bounds

Let $`m`$ be the number of mapped string occurrences, $`u`$ the number of
distinct contents, and $`B=\sum_{i=0}^{m-1}|s_i|`$ the bytes hashed. Under the
expected constant-time hash-table model, canonicalization costs $`O(B)`$ time
and $`O(u)`$ temporary handles/buckets. The unique payload bytes already belong
to graph nodes; the pool adds no second string payload.

`FxHashSet` is deliberately non-cryptographic. Under adversarial collisions,
set operations can degrade beyond their expected bound. Mode A already limits
source bytes with `max_file_size`; Mode-B callers must enforce their own input
bound before parsing. This optimization is not an input-admission mechanism.

The visitor has fixed control depth and loops over flat vectors. The enclosing
AST walk keeps frames on an explicit heap vector. Native call-stack use is
$`O(1)`$ with respect to source depth; the public depth-100,000 test runs on a
256 KiB native stack.

## Consequences

- Real-module retained string allocations fall materially without changing the
  public graph schema or serialized string contents.
- Separate files do not share ownership. `merge_from` and deserialization do
  not retroactively intern strings, so applications must never use `Arc`
  pointer equality as semantic equality.
- Total allocation traffic changes little because the mapper still creates a
  candidate allocation before the pool releases a duplicate. Peak live blocks,
  payload memory, RSS, and parser time are the relevant measured outcomes.
- The exhaustive visitor is another node-schema maintenance point, but the
  compiler exposes omissions when a new `CpgNodeKind` variant is added.
- Removing two unused dependencies shrinks the runtime dependency graph and
  avoids implying an unimplemented process-global atom policy.

## Alternatives considered

### Replace public strings with `string_cache::DefaultAtom`

Rejected. Dynamic atoms use process-global storage and synchronized buckets;
the type also has distinct short-string representation, debug shape, and Serde
behavior. Adopting it would change public fields and make graph lifetime depend
on shared process state.

### Keep one global `Arc<str>` interner

Rejected. A global table needs synchronization, eviction/lifetime policy, and
denial-of-service accounting. It can retain unrelated project identifiers long
after a graph is dropped. A CPG-local ownership boundary is sufficient.

### Put a mutex-backed pool inside `NodeMapper`

Rejected. Mapping one tree is serial, so every string would pay an uncontended
lock solely to preserve the public `&self` method. The build boundary already
owns a mutable local pool without changing `NodeMapper`.

### Sweep the complete graph after construction

Rejected. A post-pass retains all duplicates through the complete AST build,
walks the graph again, and delays the memory benefit. Per-node canonicalization
uses the same fixed-depth visitor before insertion.

### Select AHash

Rejected by measurement. On the H11 distribution, `FxHashSet` completed in
266.8 µs versus 309.8 µs for `AHashSet`; a 100,000-key integer insert/lookup
case completed in 1.704 ms versus 2.034 ms. The existing Fx choice wins both
representative classes.

The complete inventory, timing, allocator, counter, and test evidence is in
[scientific validation 55](../scientific/55-string-storage-validation.md).

## References

1. Rust Project Developers. [`std::sync::Arc`](https://doc.rust-lang.org/std/sync/struct.Arc.html), Rust standard-library documentation.
2. Rust Project Developers. [`std::collections::HashSet`](https://doc.rust-lang.org/std/collections/struct.HashSet.html), Rust standard-library documentation.
3. Knuth, D. E. (1984). *Literate Programming.* The Computer Journal 27(2),
   97–111. DOI: [10.1093/comjnl/27.2.97](https://doi.org/10.1093/comjnl/27.2.97).
