# Security boundary -- Semantic Datalog evidence

## Security claim

libcpg exports semantic EDB rows only from canonical stable source identities
and independently replayed exact proof evidence. Nontrivial class memberships
must be connected by replayed edges; CSE and PRE references must match and span
their classes. The bridge is immutable, owned, bounded, stack-safe, and
one-way: a Datalog tuple cannot gain equality-proof authority.

This claim does not establish caller rule soundness, frontend completeness,
whole-program equivalence, transformation correctness, profitability, safe
file handling, or external-engine availability.

## Trust boundaries

| Input or artifact | Treatment |
|---|---|
| semantic flow report | immutable source; relevant invariants are rechecked |
| exact proof tape | authority only after independent replay |
| representative self-membership | equality reflexivity over one stable source identity |
| non-reflexive class membership | accepted only with a replayed path to the representative |
| CSE/PRE candidate | advisory row after endpoint/spanning validation |
| Replete e-class/e-node id | private engine state; forbidden in public facts |
| fact serialization | audit/data transport, never proof restoration |
| caller Datalog rule/provenance | relational evidence, never equality authority |
| missing row under a limit | inconclusive, never false |
| ordinary CPG Datalog EDB | independent immutable export |
| output path and Datalog process | caller-owned security boundary |
| MeTTaIL | immutable provenance outside campaign write/build authority |

## Hazards and controls

### Forged or stale class membership

The exporter does not trust a representative label. It requires one class per
occurrence, canonical order, smallest-id representation, replayed exact edges,
and iterative connectivity for every nontrivial class.

### Proof-index confusion

CSE indices must name exact producer/use endpoints. PRE indices must stay
within and span the whole class. Missing, cross-class, mismatched, or
overflowing indices return typed errors before publication.

### Completion laundering

Five semantic dimensions retain exact limiting causes. Caller Datalog outcome
is separate. `can_conclude_absence()` is true only for semantic `Complete` plus
Datalog `Complete`; unknown, cancellation, memory, time, iteration, and work
limits remain inconclusive.

### Engine-local identity leakage

Only stable `NodeId` values and checked source-report proof indices are public.
The boundary gate rejects Replete use in the bridge, Datalog use in equality,
and a Replete-to-libcpg reverse dependency.

### Mutable-state coupling

The bridge owns flat rows and retains no CPG, report, Replete, or Datalog-engine
reference. Source checks reject reference-counted sharing, locks, cells, and
mutable statics in the module.

### Denial of service and stack exhaustion

Fact and byte limits sit beneath public hard ceilings. Size arithmetic is
checked. Connectivity uses a heap worklist; rendering and copying are
iterative. External memory, no-swap, CPU, time, filesystem-quota, and process
isolation remain required for untrusted workloads.

### Unsafe file or rule execution

libcpg returns strings and never writes files or launches Soufflé. Callers must
constrain directories, atomically create files, reject path traversal, pin and
isolate the engine, review rules, and report completion honestly.

## Operational checklist

1. Use a live report from the current immutable CPG and semantic profile.
2. Configure fact and byte limits from deployment policy.
3. Reject every `SemanticDatalogError`; never consume partial state.
4. Preserve all semantic completion rows.
5. Record external Datalog completion separately.
6. Require both dimensions before concluding absence.
7. Retain and replay source proofs before authoritative action.
8. Treat CSE/PRE and Datalog derivations as advisory until consumer
   obligations are verified.
9. Sandbox file I/O and Datalog execution outside libcpg.
10. Revalidate after frontend, profile, Replete, proof, flow, schema, or rule
    changes.
11. Run the reasoning-boundary script and full repository gate.
12. Do not modify or build MeTTaIL in this campaign.

See [ADR-0077](../design/0077-owned-semantic-datalog-evidence.md), the
[component guide](../components/graph/semantic-datalog-boundary.md), and the
[formal plan](../verification/t4-4-responsibility-boundary-formal-plan.md).

## References

1. Nelson, G., Oppen, D. C. (1980). *Fast Decision Procedures Based on
   Congruence Closure.* DOI:
   [10.1145/322186.322198](https://doi.org/10.1145/322186.322198).
2. Jordan, H., Scholz, B., Subotić, P. (2016). *Soufflé: On Synthesis of
   Program Analyzers.* DOI:
   [10.1007/978-3-319-41540-6_23](https://doi.org/10.1007/978-3-319-41540-6_23).
