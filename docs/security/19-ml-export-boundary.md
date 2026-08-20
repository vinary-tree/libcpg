# Security boundary — Deterministic ML export

## Security claim

The APIs produce deterministic, bounded, read-only representations of supplied
CPG evidence. They do not prove source safety, vulnerability, exploitability,
AST validity, data-set fitness, model accuracy, privacy, or freedom from
training leakage.

## Trust boundary

| Evidence | Trust condition |
|---|---|
| node class | represented `CpgNodeKind`, mapped exhaustively |
| edge class | represented typed edge kind, mapped exhaustively |
| dense identity | ascending stable `NodeId`, never internal petgraph index |
| source alignment | `CpgNode::range` copied beside its row |
| AST depth | unique root-reachable normalized `AstChild` parent chain |
| loop depth | loop kinds only along that known AST chain |
| centrality | canonical unweighted whole-CPG topology and explicit convergence flag |
| localization | complete backward PDG traversal through typed dependence edges |
| AST path | unique cycle-free `AstChild` chain to the selected function |
| token | represented text/name/operator/literal/kind fallback; never reparsed source |

No trained weight, textual vulnerability heuristic, runtime schedule, alias
guess, or parser grammar is introduced by this component.

## Hostile and incomplete input behavior

- Node and edge insertion order is neutralized by heap/ordered
  canonicalization.
- Duplicate typed edges retain multiplicity in `edge_index` and degree columns;
  duplicate AST endpoints are normalized only for structural parent counting.
- Multiple-parent and cyclic AST regions receive `ast.depth_known = 0` rather
  than a fabricated depth.
- Missing, multiple, or cyclic terminal parent chains are suppressed.
- Unknown selectors return typed errors.
- Slice, terminal, pair, context, retained-node, and ancestor-step limits fail
  before partial output is returned.
- Option values above compile-time ceilings are rejected.
- Every graph/source-depth traversal uses heap state and cycle guards.
- Outputs remain outside the graph and cannot masquerade as frontend evidence.

## Resource-exhaustion boundary

Whole-graph export necessarily allocates rows proportional to node count and
typed tuples proportional to edge count. `export_pyg` is intended for a CPG
that has already passed construction resource policy. PageRank is bounded by
the shared centrality iteration ceiling.

AST paths can grow quadratically in terminal count and linearly in aggregate
path length. `AstPathOptions` bounds all allocation-sensitive phases, and
compile-time maxima prevent unbounded caller settings. `max_path_width` is only
a filter and allocates no structure indexed by that value.

`export_pyg_with_options` computes a complete backward slice before enforcing
the caller's localization cap. This protects label integrity, but memory for a
graph-sized complete slice can still be required. Apply graph construction
limits before accepting hostile CPGs.

## Privacy and data leakage

`source_ranges`, stable ids, and terminal tokens can expose proprietary source
structure or content. Exact `CpgNode::text` has first token precedence.
Serialization therefore requires the same confidentiality controls as the
source CPG.

Forward/reverse PageRank is computed over the entire supplied CPG. If training
and evaluation examples are selected from one graph after export, centrality
can encode cross-split topology. Dependence-slice membership can also encode a
caller-selected label proxy. Partition programs and define criteria before
feature extraction when isolation is required.

## Integrity and schema evolution

The 69 feature names and 47 relation classes are an interchange contract, but
the current payload does not include a separate schema-version field. Persist
the libcpg version or a hash of `PYG_NODE_FEATURE_NAMES` with long-lived data
sets. A future node/edge enum addition must update the exhaustive mapping,
oracles, and documentation before release.

Serde accepts constructible public records. Treat deserialized `PygExport` as
untrusted data unless the consumer revalidates row cardinalities, endpoint
bounds, finite values, sorted ids, schema version, and source-range policy.

## False-positive and false-negative boundaries

These exports contain no built-in finding classification, so “positive” and
“negative” belong to the downstream model. libcpg does not justify either.
Unknown depth is explicitly marked, capped slices fail, and PageRank convergence
is retained; consumers must not discard these qualifications.

`ast_paths` does not return an issue count for malformed suppressed terminals.
An empty vector is not proof that a function has no terminals or useful syntax
paths. Validate frontend/AST health separately for completeness-sensitive
security workflows.

## Operational guidance

1. Enforce graph construction limits before export.
2. Resolve feature columns by name and persist a schema/library version.
3. Keep `ast.depth_known` and PageRank convergence flags with their values.
4. Reject a localization budget error rather than substituting a partial
   vector.
5. Validate deserialized payload shape before tensor allocation.
6. Treat tokens and ranges as source-confidential data.
7. Split data before whole-graph centrality when cross-sample leakage matters.
8. Review downstream labels, calibration, and adversarial robustness outside
   libcpg; the export is not a security verdict.

See [Theory 44](../theory/44-deterministic-cpg-ml-representations.md), the
[component guide](../components/graph/ml-export.md), [Usage
27](../usage/27-ml-export-and-ast-paths.md), and the [validation
dossier](../scientific/49-ml-export-validation.md).
