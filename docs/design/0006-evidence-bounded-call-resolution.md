# 0006 — Evidence-bounded call-target resolution

## Status

**Accepted.** Implemented by `analysis::resolve` and opt-in builder integration.
This ADR is the repository-local form of the libcpg analysis epic's ADR-044
evidence boundary.

## Context

A parsed call carries syntax but not necessarily identity. Before resolution,
`Call { target: None }` correctly says that the graph has not established a
callee. Downstream call-graph SCCs, recursion detection, centrality, coupling
metrics, and interprocedural data flow all treat a call edge as fact.

Bare-name matching is useful but dangerous. Two functions may share a name in
different modules, implementations, nested scopes, or overload sets. Selecting
one by hash-table iteration would make results nondeterministic; connecting a
call to every candidate would assert topology not justified by the source.
Either choice can invent recursion and value flow.

## Decision

**Separate evidence records from graph topology, and create topology only for a
unique target supported by an approved evidence tier.**

The approved topology tiers are `ExactInFile`, `ExactViaImport`,
`ExactViaBinding`, and `BareNameUnique`. `BareNameAmbiguous`, `External`, and
`Unresolved` remain out-of-graph records with bounded candidate evidence. They
set no target and emit no reference/call edge.

Resolution is an explicit, feature-free post-pass. The standard builder exposes
it through the existing `resolve_imports` flag, renamed pedagogically in its
method documentation but kept source-compatible. The flag defaults to `false`.
When enabled, resolution runs after the CFG and before the DFG.

The complete tier scores, emission matrix, caps, and pseudocode are in the
[call-resolution component guide](../components/graph/call-resolution.md).

## Consequences

### Positive

- **No fabricated SCC edges.** Ambiguity cannot merge call-graph components.
- **Determinism.** Sorted identifiers, bounded candidate buckets, and `BTreeMap`
  results remove insertion/hash-order decisions.
- **Auditability.** Every processed call has an authoritative out-of-graph
  record even when the graph deliberately stays unchanged.
- **Interprocedural activation.** Unique calls feed existing parameter and
  return-value DFG construction before extraction completes.
- **Shape compatibility.** Default construction remains byte-for-byte opt-out;
  rerunning an enabled pass adds no duplicate edges.

### Negative

- **Conservative false negatives.** Dynamic dispatch, overload typing, external
  package indexes, and ambiguous bare names remain unresolved until stronger
  evidence is available.
- **Additional opt-in edges.** Enabling the pass adds scope, reference, and call
  relations and populates `Call::target`; consumers comparing graph shape must
  treat the option as a semantic construction change.
- **Bounded evidence.** Candidate identifiers beyond
  `max_candidates_per_name` are not retained, although their count is preserved
  so truncation can never create false uniqueness.

## Rejected alternatives

### Pick the first candidate

Rejected because insertion and hash order are not semantic evidence. It would
make topology nondeterministic and incorrect.

### Connect every ambiguous candidate

Rejected because a may-call over-approximation is not interchangeable with the
definite-call graph currently consumed by SCCs, `callers`, and `callees`.
Introducing a separate may-call overlay can be considered independently.

### Resolve automatically in every build

Rejected because existing consumers and regression tests depend on default
construction shape. Explicit post-pass/opt-in behavior preserves compatibility.

### Store confidence only on edges

Rejected because the cases most important to audit—ambiguous, external, and
unresolved calls—must not have an edge. `CallGraphResolution` is authoritative;
edge labels are a convenient copy for emitted topology.

## Reference

- Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and Discovering Vulnerabilities with Code Property Graphs.* 2014 IEEE Symposium on Security and Privacy, 590–604. DOI: [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44).
