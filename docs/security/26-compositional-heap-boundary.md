# Security boundary -- compositional heap analysis

## Security claim

libcpg returns deterministic advisory evidence for modeled null dereferences,
use-after-free paths, and leaks. It does not prove memory safety,
exploitability, ownership correctness, absence of undefined behavior, or
completeness of the supplied frontend and call graph.

## Trusted evidence

| Evidence | Trust condition |
|---|---|
| allocation | frontend populated `allocation_type` correctly |
| null | represented `LiteralKind::Null` has the language's null semantics |
| dereference | typed DFG or unary operator represents a real memory access |
| release | caller-owned exact `HeapModel` name truly invalidates its first argument |
| origin | typed definition/AST/DFG relation preserves value identity |
| order | canonical function CFG represents relevant normal control flow |
| call effect | call resolves to exactly one represented callee and summaries are complete |
| exit | canonical CFG exit set includes every relevant termination path |

The analysis never reparses source. Unresolved calls, foreign/native code,
inline assembly, exceptional unwinding, process termination, custom allocators,
pointer arithmetic, field-sensitive aliases, ownership transfer, concurrency,
and frontend omissions remain outside the default model.

## False-positive boundary

Allocation-site abstraction conflates concrete objects created by one site.
May-state union retains both sides of branches and loops. DFG alias evidence
can conservatively associate several origins with one value. A reported leak
or use-after-free path can therefore be infeasible in the concrete program.
Findings require source review or a more precise downstream analysis.

## False-negative boundary

An unmodeled release, dereference, alias, call target, or exit can suppress a
finding. Missing points-to precision can prevent an operation from being tied
to its allocation. An empty result is usable only as “none found in the
represented model,” and only when `HeapAnalysis::is_complete()` is true.

Same-SCC recursive must-invalidate effects are not trusted at an intermediate
monotone fixed point. The affected summary is explicitly partial. Consumers
must not discard `capped`, `domain_capped_functions`, `cap_events`, or
`issues`.

## Resource-exhaustion boundary

The analysis inherits `IfdsCaps`:

- `max_functions` bounds admitted procedures;
- `max_facts_per_node` bounds origins, assertions, relations, and findings;
- `max_worklist_steps` bounds condensation, origin, descendant, and lifetime
  work;
- `max_scc_iters` bounds recursive summary rounds; and
- `max_call_depth` bounds callee-path reconstruction and retained witnesses.

Bounds qualify or stop the complete result; they are not stack-protection
hacks. AST, CFG, SCC, origin, composition, overlay, serialization, and drop
control lives in heap collections. A 100,000-level integration case runs on a
256 KiB native stack.

Heap-backed stack safety does not bound resident memory. Enforce graph-size,
time, and process-memory policy outside libcpg. For hostile workloads, run the
host process in a memory/task-limited cgroup and preserve cap evidence.

## Mutation and deserialization integrity

Summaries are read-only and out of graph. Overlay materialization validates all
source and target nodes before mutation. `HeapOverlayError` leaves the graph
unchanged; successful repetition is idempotent.

Serde constructs public evidence records but does not attest that a summary
was derived from the supplied CPG. `memory_safety` turns mismatched or missing
references into capped `InvalidSummaryReference` findings. Treat deserialized
summaries as untrusted until their origin, libcpg version, caps, model, and CPG
identity are authenticated.

## Operational checklist

1. Resolve calls and validate frontend graph invariants before analysis.
2. Use an exact, reviewed release vocabulary for the program and language.
3. Retain the full `HeapAnalysis`; do not infer cleanliness from the map-only
   projection.
4. Require `is_complete()` before any negative statement.
5. Review every witness against source, alias, exception, and concurrency
   context.
6. Materialize only into the same CPG from which summaries were derived.
7. Bound graph construction, process RSS, and wall time at hostile boundaries.

See the [theory](../theory/51-compositional-heap-analysis.md),
[component contract](../components/graph/compositional-heap-analysis.md),
[usage guide](../usage/36-compositional-heap-analysis.md), and
[validation dossier](../scientific/63-compositional-heap-validation.md).

## References

1. Calcagno, C., Distefano, D., O'Hearn, P. W., Yang, H. (2011).
   DOI: [10.1145/2049697.2049700](https://doi.org/10.1145/2049697.2049700).
2. Calcagno, C., Distefano, D., O'Hearn, P. W., Yang, H. (2009).
   DOI: [10.1145/1480881.1480917](https://doi.org/10.1145/1480881.1480917).
