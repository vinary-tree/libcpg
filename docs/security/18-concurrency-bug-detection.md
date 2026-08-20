# Security boundary — Race and atomicity candidate detection

## Security claim

The APIs report deterministic advisory evidence from exact represented
locations, typed access kinds, typed locks, and supplied MHP. They do not prove
that a race or atomicity violation is reachable, exploitable, or even possible
under a language's complete memory model.

## Trusted evidence

| Evidence | Trust condition |
|---|---|
| field/index access | typed DFG relation and exactly one exact `Reference` |
| channel access | typed `MsgKind` and resolved definition identity |
| executable site | unique nearest execution-bearing AST ancestor |
| execution strand | unique nearest function or typed parallel operand |
| lock protection | resolved `LockKind`, exact resource, and source-ordered replay |
| parallelism | canonical pair present in the caller-supplied MHP relation |

No source spelling, fuzzy name match, inferred parser grammar, or fabricated
edge is a trust input.

## Hostile and incomplete input behavior

- Cyclic parent overlays terminate through heap work queues and visited sets.
- Equally near multiple sites or strands suppress evidence.
- Multiple location or lock identities suppress identity rather than selecting
  one.
- Reversed, duplicate, or reflexive public MHP pairs are normalized.
- An unresolved acquisition never creates protection.
- An unresolved release clears held state so stale protection cannot hide a
  report.
- A capped MHP relation stays visibly incomplete in every witness.
- All outputs remain out of graph, preventing advisory state from masquerading
  as source-derived topology.

## False-positive boundary

Structural MHP may include unreachable or guard-incompatible sites. libcpg does
not model atomics, volatile semantics, barriers, condition variables, lock-free
algorithms, runtime channel guarantees, reentrant lock behavior, or every
language memory model. These can make a reported candidate safe.

## False-negative boundary

Missing frontend metadata, DFG/Reference edges, aliases, object identities,
dynamic process construction, interprocedural access summaries, or capped MHP
can hide candidates. An empty vector is therefore not a security certificate.

## Operational guidance

1. Reject capped MHP when a workflow requires complete represented coverage.
2. Preserve witness cap state in serialized reports.
3. Review exact locations and source ranges against runtime aliasing and memory
   semantics.
4. Treat findings as triage inputs, not automatic vulnerability labels or
   rewrite authorization.
5. Keep frontend/adapter `MsgKind` and `LockKind` assignment behind resolved
   semantic evidence.

See [Theory 43](../theory/43-evidence-bounded-race-and-atomicity.md), the
[component guide](../components/graph/concurrency-bug-detection.md), and the
[validation dossier](../scientific/48-concurrency-bug-validation.md).
