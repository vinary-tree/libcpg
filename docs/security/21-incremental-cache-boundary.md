# Security 21 -- incremental-analysis cache boundary

The incremental cache is an optimization over source-derived analysis plans.
It is not a trust boundary, authorization decision, cryptographic
content-address, or persistent artifact format.

## Assets and untrusted inputs

The assets are result integrity, process memory, deterministic behavior, and
separation between caller-controlled analysis sessions. Source text,
caller-supplied trees, graph shape, edit sequences, and cache-limit
configuration may be adversarial.

![Only represented function-local dependencies enter the cache; all other evidence is recomputed.](../diagrams/function-analysis-cache-boundary.svg)

*Figure -- security and soundness boundary. Source:
[`diagrams/function-analysis-cache-boundary.puml`](../diagrams/function-analysis-cache-boundary.puml).*

## Integrity controls

1. The digest does not authorize reuse. Full canonical bytes must compare
   equal.
2. Cached plans contain local ordinals, never graph-specific identifiers.
3. Every ordinal is validated before the first replay mutation.
4. DFG and PDG dependencies that cross the represented local boundary select
   ordinary computation.
5. Language, options, schema version, source bytes, and Mode-B tree shape
   separate semantic domains.
6. Parallel misses are collected by stable function index and emitted
   serially.

A forced-collision unit test places different full keys in one digest bucket
and proves they remain independently addressable.

## Availability and memory controls

Both entry count and estimated resident bytes are hard admission dimensions.
An oversized entry is not retained. Deterministic LRU eviction restores both
limits after insertion. The entry ceiling independently bounds uncounted
allocator and `BTreeMap` overhead that the byte estimate does not claim.

All graph-shaped walks, coordinate conversion, clear, and cache destruction
are iterative. Deep and wide 100,000-node tests run on a 256 KiB worker stack.
Heap exhaustion remains possible if a caller admits an enormous source or
chooses enormous limits; pair cache limits with Mode-A `max_file_size` and
service-level request budgets.

## Confidentiality and tenancy

Canonical keys retain exact function source bytes for the cache lifetime.
Therefore:

- do not share one cache across tenants or security domains;
- clear the cache when its owning document/session closes;
- treat memory dumps as potentially source-bearing; and
- do not serialize cache internals.

The API exposes no cache persistence or key iteration. `Debug` output must
still be treated as internal diagnostic data rather than logged across trust
boundaries.

## Mode-B responsibility

Mode B accepts a caller-supplied tree and does not apply Mode A's file-size
check. Its key records both source and local tree shape, but that does not
validate that the tree was parsed from the source. The caller must:

- admit source/tree size before construction;
- use a grammar compatible with libcpg's node mapper; and
- keep tree and source ownership coherent.

## Fail-closed table

| Condition | Behavior |
|---|---|
| digest collision | compare full bytes; wrong entry cannot hit |
| changed source/options/language | exact miss |
| external caller or lexical dependency | ordinary DFG computation |
| nested function or receiver-field resolution | ordinary DFG computation |
| cross-boundary PDG overlay | ordinary PDG computation |
| malformed/out-of-range cached ordinal | reject replay and recompute |
| zero capacity or oversized entry | ordinary result, no retention |
| counter overflow | saturate; never wrap into misleading small values |

## Non-claims

- FNV-1a is not collision-resistant.
- A cache hit does not prove semantic equivalence beyond represented CPG
  analysis.
- The resident-byte number is an owned-data estimate, not allocator RSS.
- Conservative bypasses do not make the ordinary analysis more precise than
  its documented semantics.
- Bounds do not replace upstream source, graph, request-time, or tenant quotas.

The [scientific record](../scientific/57-function-analysis-cache-validation.md)
contains collision, malformed-graph, eviction, deep-stack, RSS, DHAT, Massif,
and instruction evidence.
