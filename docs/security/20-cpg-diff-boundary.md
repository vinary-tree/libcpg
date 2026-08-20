# Security boundary — Deterministic CPG diff

## Security claim

`cpg_diff` produces a deterministic, bounded, read-only structural edit script
for two supplied Code Property Graphs (CPGs). It does not prove source safety,
runtime equivalence, exploitability, semantic identity, parser correctness, or
that a change is a refactoring in the language-theoretic sense.

## Trust boundary

| Evidence | Trust condition |
|---|---|
| node alignment | selected symmetric bounded GED correspondence; deterministic heuristic, not identity proof |
| node substitution | exact represented payload after translating embedded aligned node ids |
| edge equality | exact directed endpoints, typed relation, label, and occurrence multiplicity |
| edge substitution | canonical leftover pair with equal aligned endpoints |
| `BehaviorChanging` | at least one edited old/new edge is represented PDG dependence |
| `Refactoring` | no represented PDG dependence edge was edited; no stronger equivalence claim |
| ordering | stable ids, exhaustive edge classes, labels, field order, and canonical property keys |

No source parser, textual heuristic, alias guess, runtime trace, language memory
model, or external repository state enters the classifier.

## Hostile and incomplete input behavior

- Node and edge insertion order is neutralized by canonical sorting and
  assignment tie-breaking.
- Node and edge ids are treated as local coordinates, not cross-version
  identity.
- Parallel edge multiplicity is retained instead of deduplicated.
- Embedded call/definition/capture/AST references compare through alignment.
- Malformed cached parent/children metadata is reported, not repaired.
- Node, edge, and edit limits return typed errors before a partial result.
- Every input-sized continuation, matrix, map, queue, and result lives on the
  heap; flat property values preserve constant native call depth.
- Neither input CPG is mutated.

The diff does not validate endpoint existence or reconcile all malformed graph
side tables beyond what `CodePropertyGraph` construction already enforces.
Apply graph validation and construction policy before trusting completeness.

## Dependence-classification boundary

![Dependence-aware edit classification boundary](../diagrams/cpg-diff-classification.svg)

Build PDGs independently on both graph versions before classification. If one
side omits PDG construction, an edge delta can reflect pipeline configuration
rather than source behavior. If both sides omit PDGs, `Refactoring` means only
that no PDG edge was available to change.

A changed PDG edge is still abstract evidence. It can arise from conservative
data flow, frontend normalization, or analysis-version changes without an
observable runtime difference. Conversely, unchanged represented dependencies
can miss aliasing, reflection, dynamic dispatch, concurrency, undefined
behavior, environment effects, or facts outside the CPG.

## Resource-exhaustion boundary

The default/hard limits are 512 nodes, 200,000/2,000,000 edges per input, and
200,000/2,000,000 aggregate edit records. The cubic Hungarian matrix makes node
admission the primary CPU boundary; exact edge buckets and complete evidence
snapshots make edges, edits, and payload size independent memory boundaries.

Caller values above a hard ceiling are clamped rather than expanding authority.
Any exceeded effective cap yields `BudgetExceeded`, and no prefix is returned.
This matters because an omitted late PDG edit could invert classification.

Node payload bytes are not separately capped. `Arc<str>` avoids copying text
bytes, and flat `PropertyValue` cloning is stack-safe, but a hostile already-
constructed graph may still contain large strings or property trees. Bound and
validate CPG payloads at ingestion.

## Integrity and serialization

With serde enabled, deserialization can construct public result records without
re-running alignment or classification. Treat deserialized `CpgDiff` as
untrusted evidence unless its ids, ordering, fields, caps, and classification
are revalidated or authenticated. Persist the libcpg version with long-lived
results because node/edge vocabularies may evolve.

Complete snapshots can expose exact source text, ranges, names, type facts, and
custom properties. Protect serialized diffs with the same confidentiality
policy as the input CPGs.

## Structural omissions

Language, source path, retained whole-source text, graph root, and recorded CFG
entries/exits are not compared. A security workflow that depends on provenance
or build identity must bind and compare those values separately. A clean
structural diff is not a provenance certificate.

## Operational guidance

1. Build both CPGs with the same frontend and analysis configuration.
2. Construct comparable PDGs before interpreting `ChangeKind`.
3. Set node/edge/edit caps from a resource policy below hard maxima as needed.
4. Validate graph and payload size at ingress.
5. Review exact edits; never use the two-valued label as the sole security
   decision.
6. Compare container metadata separately when provenance matters.
7. Authenticate or revalidate serialized results and protect source-bearing
   fields.

See [Theory 45](../theory/45-deterministic-cpg-diff.md), the [component
contract](../components/graph/cpg-diff.md), [Usage 28](../usage/28-cpg-diff.md),
and the [validation dossier](../scientific/50-cpg-diff-validation.md).
