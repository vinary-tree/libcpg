# Security boundary -- Rholang NameFlow and level analysis

## Security claim

libcpg returns deterministic, bounded evidence about modeled fresh-name
provenance and cyclic receive-before-send level constraints. It does not prove
operational deadlock freedom, lock freedom, fairness, termination, protocol
correctness, confidentiality, or completeness of the supplied Rholang
frontend.

## Trusted evidence

| Evidence | Trust condition |
|---|---|
| restriction and reflection | frontend assigned exact `NameOperationKind` metadata |
| message role | frontend assigned the correct `MsgKind` |
| identity | unique typed definition/reaching-definition evidence denotes the intended binder |
| event shape | source-ordered AST operands distinguish channel, payload, and receive binders |
| communication | matching channel origins are a valid may-match approximation |
| process order | source order represents the modeled receive-before-send obligation |
| SCC | supplied channel projection contains every dependency retained by the model |

The analyzer never reparses source or guesses identity from a name string.
Ambiguous identity is an explicit issue.

## False-positive boundary

May-matching pairs every represented producer and receiver sharing a fresh
channel origin. Guards, patterns, races, cancellation, and reduction order may
make a retained pair infeasible. Source-ordered receive-before-send constraints
do not encode every control path. A cyclic SCC therefore warrants review; it
does not assert that a concrete execution reaches deadlock.

## False-negative boundary

Frontend omissions, unresolved definitions or channels, native/external
processes, unrepresented higher-order reductions, unmatched arity, and reached
caps can suppress flow or dependencies. The report exposes those conditions
through `complete == false` and typed issues. Even a complete empty report only
covers the represented finite model.

## Resource and stack boundary

`NameFlowCaps` bounds origin assignments, relations, communication matches,
dependencies, and aggregate queue/ancestor work. Every cap is evidence-bearing
and invalidates completeness. All source-shaped traversal and fixed-point state
uses heap collections; a 100,000-level AST integration case runs analysis,
materialization, graph statistics, and explicit destruction on a 256 KiB
native stack.

Heap allocation can still exhaust resident memory. At an untrusted service
boundary, limit graph size, wall time, resident memory, swap, and task count in
the host environment, and preserve partial reports rather than retrying with
unbounded resources.

## Mutation and serialization integrity

`materialize_name_flow_overlay` prevalidates all source and target nodes. An
unknown endpoint returns `NameFlowOverlayError` before any mutation; successful
repetition is idempotent. A deserialized report is data, not attestation that it
was derived from the current CPG. Authenticate the graph identity, libcpg
version, feature set, and caps before relying on external reports.

## Operational checklist

1. Validate the Mode-B frontend and resolve definitions before analysis.
2. Retain the full qualified report for negative statements.
3. Treat every issue or cap as incomplete evidence.
4. Replay witnesses against source, guards, patterns, and external producers.
5. Materialize only into the graph from which the report was derived.
6. Bound process resources independently of algorithmic stack safety.

See the [theory](../theory/52-rholang-name-flow-and-deadlock-freedom.md),
[component contract](../components/graph/rholang-name-flow.md),
[usage guide](../usage/37-rholang-name-flow-deadlock.md), and
[validation dossier](../scientific/64-rholang-name-flow-validation.md).

## References

1. Kobayashi, N. (2006). DOI:
   [10.1007/11817949_16](https://doi.org/10.1007/11817949_16).
2. Kobayashi, N., Sangiorgi, D. (2010). DOI:
   [10.1145/1745312.1745313](https://doi.org/10.1145/1745312.1745313).
