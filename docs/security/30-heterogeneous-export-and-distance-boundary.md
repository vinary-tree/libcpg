# Security boundary -- Heterogeneous export and directed-fuzzing distance

## Security claim

libcpg returns deterministic, read-only representations of supplied CPG
evidence. It does not certify source safety, model accuracy, vulnerability,
target correctness, runtime reachability, fuzzing effectiveness, privacy, or
freedom from training leakage.

## Trust boundary

| Evidence | Trust condition |
|---|---|
| dense identity | ascending stable `NodeId`, never petgraph insertion index |
| node class | exhaustive represented `CpgNodeKind` mapping |
| token | represented text/name/operator/literal/kind fallback, never reparsed source |
| token hash | fixed signed width-1/2/3 UTF-8 byte n-gram algorithm |
| relation name/class | append-only exhaustive 67-class registry |
| relation endpoints | represented typed CPG edges with preserved multiplicity |
| distance edge | represented CFG or static/dynamic/call-site edge |
| distance value | shortest represented paths and deterministic harmonic aggregation |

No learned weight, vulnerability heuristic, corpus vocabulary, fuzzer energy
schedule, instrumentation trace, or parser grammar enters the feature-free
analysis.

## False-positive and false-negative boundaries

- A finite distance proves only a represented CFG/call path. Path feasibility,
  dynamic dispatch, exceptions, indirect calls, and generated behavior may be
  incomplete.
- Positive infinity means no path in the admitted relation subset. It is not
  proof that runtime execution cannot reach the target.
- Unknown target ids are ignored. Strict services must validate selectors
  before calling.
- Feature-hash collisions can merge or cancel token n-grams. Exact exported
  tokens remain authoritative.
- The export provides model inputs, not classifications. Every positive or
  negative prediction belongs to the downstream model and data policy.

## Privacy and leakage

Exact tokens and source ranges can expose proprietary source. Stable ids and
relation topology can expose program structure even when text is removed.
Apply source-level access control to serialized exports.

Split training/evaluation programs before feature extraction when graph
isolation matters. Relation neighborhoods and selected targets can encode
label proxies. libcpg does not detect data-set leakage or membership inference.

## Resource-exhaustion boundary

Heterogeneous export allocates 109 floating-point values per node and one
owned record for each of 67 relation classes. Distance performs one
reverse-breadth-first traversal per valid unique target. Both use heap-backed
state and remain native-stack-safe at graph depth 100,000, but neither API has
an analysis-specific graph/target cap.

At an untrusted boundary, limit graph size, target count, wall time, resident
memory, swap, and tasks. Do not confuse constant native call depth with a
resident-memory bound.

## Serialization and schema integrity

Treat deserialized `HeteroGnnExport` as untrusted data. Revalidate aligned
lengths, 109-column rows, node-type bounds, sorted unique ids, relation count,
class/name alignment, endpoint bounds, source-range policy, and numerical
finiteness. Persist a libcpg/schema version. Older consumers may preserve an
unknown append-only suffix numerically without understanding its semantics.

Positive infinity requires an explicit external representation. Do not map it
to zero, which would turn unreachable nodes into targets, or to an undocumented
finite sentinel, which would invent a path length.

## Operational checklist

1. Validate graph provenance and target ids.
2. Enforce process-level memory, swap, time, and task limits.
3. Protect tokens, ranges, stable ids, and topology as source-confidential.
4. Persist relation names, feature dimensions, and library version.
5. Validate serialized record shape before tensor allocation.
6. Represent unreachable distance with an explicit tagged/null state outside
   formats that reject infinity.
7. Keep downstream model, target, and fuzzing-schedule claims outside libcpg's
   structural evidence boundary.

See [Theory 55](../theory/55-heterogeneous-graphs-and-directed-fuzzing-distance.md),
the [component guide](../components/graph/heterogeneous-export-and-distance.md),
[Usage 40](../usage/40-heterogeneous-export-and-fuzzing-distance.md), and the
[validation dossier](../scientific/67-heterogeneous-export-distance-validation.md).
