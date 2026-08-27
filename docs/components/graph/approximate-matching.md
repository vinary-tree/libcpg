# Bounded approximate matching

The optional `approximate-matching` component turns immutable text or typed
token projections into deterministic advisory candidates. It never mutates a
Code Property Graph (CPG), establishes identity, or emits a final security or
clone verdict.

![The approximate pipeline separates typed rewriting, the reversible vocabulary, the sequence dictionary, bounded distance, and exact confirmation.](../../diagrams/approximate-automata-adoption.svg)

## Component roles

| Component | Owned responsibility | Explicit non-responsibility |
|---|---|---|
| libcpg | stable records, limits, completion, provenance, total ordering, confirmation type | edit or rewrite semantics |
| `duallity` | weighted finite-state tape rewrites | e-graph equality saturation |
| `lling-llang` | typed composition of complete weighted relations | partial-result associativity |
| `libdictenstein` | reversible term identifiers and `u64` sequence dictionary | approximate cost |
| `liblevenshtein` | standard, OSA, Damerau, and merge/split edit search | exact CPG facts |
| Replete | e-graph equivalence and extraction in the later integration phase | approximate sequence ranking |

**OSA** means optimal string alignment. A **DAWG** is a directed acyclic word
graph: a finite automaton that shares common sequence prefixes and suffixes.

## One-shot and reusable surfaces

`approximate_candidates` is the one-shot surface. Text is compared directly.
Typed tokens are mapped pairwise to collision-free Unicode scalar symbols and
then delegated to the same bounded liblevenshtein kernels. The mapping is an
equality-preserving adapter, not a second distance implementation.

`ApproximateTokenIndex` is the reusable surface. Its build has two levels:

```text
exact term <-> canonical u64 vocabulary id
sequence<u64> -> ordered stable candidate slots
```

The index retains all slots for duplicate sequences. `token_id` and `token`
expose the forward and reverse vocabulary relation. Query-local unknown terms
receive fresh identifiers and never mutate the corpus vocabulary.

## Completion and ordering

Every report is either `Complete` or `Incomplete(reasons)`. An incomplete empty
report means “unknown under these limits,” never “no match.” Build limitations
remain attached to every later index query.

Evidence is ordered by exact normalized rational cost, exact raw rational cost,
and stable candidate identifier. Canonicalization uses iterative binary heaps;
public order is independent of input order, hash iteration, internal automaton
state numbers, and thread count.

## Resource model

The caller supplies bounds for candidate count, input units, per-pair work,
total work, state count, memory, and unique tokens. The index build additionally
accounts for stable ordering, vocabulary text, encoded sequences, candidate
slots, DAWG nodes and an edge upper bound. Search accounts for query encoding,
unknown vocabulary terms, automaton traversal, duplicate yields, and retained
top-k evidence.

All deep control is held in vectors, arenas, finite-state iterators, or binary
heaps. Construction, search, rank, clone, debug formatting, serialization, and
drop are qualified at 100,000 token units on a 256 KiB native stack.

## Related documentation

- [ADR-0073](../../design/0073-bounded-composable-approximate-evidence.md)
- [Theory 56](../../theory/56-weighted-relations-and-approximate-matching.md)
- [API reference](../../api/approximate-matching-reference.md)
- [Usage 41](../../usage/41-bounded-approximate-matching.md)
- [Security boundary](../../security/31-approximate-evidence-boundary.md)
- [Scientific validation](../../scientific/68-approximate-automata-validation.md)
