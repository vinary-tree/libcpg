# Security boundary -- MeTTa rewriting analysis

## Security claim

libcpg returns deterministic, bounded evidence about the represented
first-order MeTTa rewrite system. It does not certify the complete Hyperon
runtime, higher-order/effectful semantics, fairness, resource consumption,
semantic equivalence, or confluence from an unqualified empty vector.

## Trusted evidence

| Evidence | Trust condition |
|---|---|
| rule shape | frontend mapped exactly one LHS and RHS beneath a rule `Function` |
| symbol identity | identifiers, literals, and operator lexemes retain their normalized semantic payload |
| variable identity | typed rule-local LHS binder names and RHS uses denote the same MeTTa variable |
| dependency candidate | exact trie root match plus complete first-order unification |
| cycle structure | the DP projection contains every retained pair and `projection_sccs` completed |
| termination proof | every rewrite rule is weak and every internal cyclic DP is strict under one reduction pair |
| critical-pair proof | rules are left-linear; every non-variable overlap and joinability search completed |
| overlay | report and target CPG refer to the same graph identity and feature/schema version |

The analyzer never reparses source and never uses approximate matching for a
proof decision.

## False-positive boundary

A dependency pair is an over-approximate possible recursive call; its presence
does not assert a realizable infinite execution. An unoriented SCC is
`Unknown`, not nontermination. A critical conflict is exact for the represented
first-order rewrite relation, but a host with additional equations, theories,
effects, or evaluation rules may join the branches outside this model.

## False-negative boundary

Malformed frontend shapes, missing operator payloads, unresolved/ambiguous
variables, omitted runtime rules, non-left-linear LHSs, and reached caps can
suppress proof coverage. These conditions clear completeness or produce
`TerminationVerdict::Unknown`. The concrete refuter currently recognizes
alpha-equivalent root self-loops; absence of that witness is not termination.

`critical_pairs(cpg)` deliberately discards completeness. An empty convenience
vector must never be interpreted as local confluence. Retain
`CriticalPairAnalysis`, require `complete`, require the left-linear represented
fragment, and compose it with an actual termination proof before applying
Newman's Lemma.

## Resource and stack boundary

`RewriteAnalysisCaps` bounds rule count, term nodes, dependency pairs,
unification work, conflicts, and aggregate joinability states. Every cap is
evidence-bearing. Flat arenas, explicit PDA frames, and BFS queues make native
call depth constant in term/search depth; a 100,000-level test runs on a 64 KiB
stack.

Heap allocation can still exhaust resident memory, and a finite rewrite graph
can approach the state cap. At an untrusted boundary, limit source/graph size,
wall time, resident memory, swap, and task count at the process level. Do not
retry incomplete analyses with unbounded caps.

## Mutation and serialization integrity

`materialize_rewrite_dep_overlay` validates all endpoints before adding the
first edge and accepts only existing `Function` nodes. Repetition is
idempotent. A deserialized `DpGraph` or `CriticalPairAnalysis` is data, not an
attestation that it belongs to the current CPG. Authenticate graph identity,
libcpg version, feature set, caps, and schema before external materialization.

New `CpgEdgeKind`, `ProjectionKind`, and statistics fields are append-only;
new ML columns/classes are suffixes. Consumers must still version the complete
schema because older software cannot interpret a new variant merely because
earlier numeric identifiers were preserved.

## Operational checklist

1. Validate the caller-owned MeTTa grammar and Mode-B frontend.
2. Retain qualified results whenever absence matters.
3. Treat every issue, cap, or non-left-linear rule as incomplete evidence.
4. Replay rules, call sites, overlaps, and reduct fingerprints against source.
5. Materialize only into the graph from which both reports were derived.
6. Enforce host resource limits independently of algorithmic stack safety.

See the [theory](../theory/53-dependency-pairs-and-critical-pair-confluence.md),
[component contract](../components/graph/metta-rewriting-analysis.md),
[usage guide](../usage/38-metta-rewriting-analysis.md), and
[validation dossier](../scientific/65-metta-rewriting-validation.md).

## References

1. Arts, T., Giesl, J. (2000). DOI:
   [10.1016/S0304-3975(99)00207-8](https://doi.org/10.1016/S0304-3975(99)00207-8).
2. Newman, M. H. A. (1942). DOI:
   [10.2307/1968867](https://doi.org/10.2307/1968867).
