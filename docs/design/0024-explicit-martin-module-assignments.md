# ADR 0024 — Explicit module assignments for Martin metrics

## Status

**Accepted.** Implemented by `src/analysis/martin.rs`.

## Context

Robert C. Martin's package metrics require two inputs that a Code Property
Graph does not uniquely determine: a dependency relation and a package/module
boundary [[1]](#references). A caller may want imports between files, calls
between functions grouped by namespace, references between classes grouped by
crate, or a separately constructed service graph. Inferring one policy inside
the metric would make results non-reproducible and misleading across consumers.

The pgmcp implementation being ported computes coupling over file graph
directory buckets. It initially filled `abstractness` with `0.0`, then repaired
it from separate symbol-table data. libcpg already carries `Class {
is_abstract }`, `Trait`, `Struct`, and `Enum` nodes, so it can compute
abstractness directly without content re-parsing or a placeholder.

`GraphProjection` supplies canonical stable IDs, merged adjacency, and
forward/reverse CSR validation. CPG AST parent chains supply enclosing module
evidence. Both inputs have public fields and serde support, so malformed or
caller-mutated values must remain total.

## Decision

### 1. Make module assignment a first-class value

`ModuleAssignment` stores a sorted vector of `Arc<str>` bucket names and a
`BTreeMap<NodeId,u32>` index. It is separate from both the CPG and the
projection. `from_pairs` supports caller policy; `by_module_nodes` and
`by_source_path` provide explicit built-ins.

Nested module nodes produce qualified `outer::inner` names. Nodes outside a
module use `<root>`; absent source metadata uses `<unknown-source>`. Duplicate
pair input is canonicalized independently of iteration order.

### 2. Count distinct external vertices, not edges or weights

For each module, afferent and efferent coupling are ordered sets of outside
projection `NodeId` values. Parallel projection edges have already been merged,
and weights are ignored. Several edges from the same outside vertex into one
module count once; dependencies on several distinct outside vertices count
separately. Internal edges do not contribute.

This preserves Martin's class-count interpretation for class projections and
pgmcp's file-count interpretation for file projections without hard-coding
either vertex kind.

### 3. Compute abstractness from assigned CPG type nodes

The numerator is abstract `Class` plus every `Trait`. The denominator is all
`Class`, `Struct`, `Enum`, and `Trait` nodes. A module with no type declaration
has abstractness zero. Type nodes need not occur in the dependency projection;
the assignment relates both evidence sources to the same module name.

### 4. Publish one compact SDP record per module pair

`sdp_violations` applies the strict condition $`I(X)<I(Y)`$ to every
cross-module projection edge. All violating edges for a module pair are
aggregated into one count while retaining the lexicographically smallest
stable-node edge as a witness. Equal instabilities are compliant.

### 5. Preserve total signatures

The task contract requires `Vec` return values rather than `Result`. A malformed
projection returns an empty vector before traversal. Invalid assignment indices
and unmapped nodes are inert. SDP comparison rejects an entire metric slice
with duplicate names, non-finite values, or values outside $`[0,1]`$, because
choosing among contradictory values would fabricate a verdict.

### 6. Keep the implementation feature-free and out-of-graph

The analysis uses only ordered standard-library collections, CPG type payloads,
and `GraphProjection`. It adds no feature or dependency and mutates neither
input. Serde derives use the existing `serde` feature.

![The selected assignment and projection stay explicit through coupling, abstraction, scalar metrics, and witnessed SDP comparison.](../diagrams/martin-metrics-pipeline.svg)

*Figure — decision data flow. Source:
[`diagrams/martin-metrics-pipeline.puml`](../diagrams/martin-metrics-pipeline.puml).*

## Consequences

### Positive

- Consumers can compare boundary policies without rebuilding the CPG.
- Abstractness is real CPG evidence rather than pgmcp's old placeholder.
- Sparse stable IDs, duplicate edges, hash order, and input order cannot change
  public ordering.
- Every SDP claim retains a concrete dependency witness and multiplicity.
- The feature-free port composes with call, import, module, or external graphs.

### Negative

- Results are not comparable unless vertex granularity, edge semantics, and
  assignment policy are held fixed.
- The built-in source-path assigner represents the current single-file CPG
  metadata; future multi-file merge APIs must supply per-node file assignments.
- Empty vectors conflate malformed input with a valid assignment containing no
  buckets; callers that accept untrusted public structs should validate them
  before presentation if that distinction matters.
- Ordered sets add logarithmic factors compared with hash sets, in exchange for
  reproducible behavior.

## Alternatives considered

### Infer buckets from path strings inside `martin_metrics`

Rejected. Directory depth, Cargo crate, namespace, and service boundaries are
different policies. Hidden truncation would make the same CPG produce
consumer-specific results without an auditable assignment value.

### Require dependency edges to be module nodes only

Rejected. Martin's original definition counts classes, and pgmcp counts files.
Restricting projection vertices would prevent both function-to-module call
analysis and direct file-graph reuse.

### Count cross-module edges

Rejected. Repeated references between the same external elements would inflate
coupling. Distinct external vertices preserve the class/file interpretation.

### Add analysis edges to the CPG

Rejected. Metrics and SDP findings are derived reports, not source facts. They
remain out-of-graph under the unified-overlay discipline.

### Keep abstractness at zero

Rejected. It inverts the interpretation of distance for abstract packages and
throws away type evidence already represented by the CPG.

### Return one SDP result per projection edge

Rejected. It repeats the same module-level conclusion and obscures scope.
Aggregating by module pair while retaining a smallest witness is compact and
auditable.

## References

1. Martin, R. C. (1994). *OO Design Quality Metrics: An Analysis of
   Dependencies.* [Archived report
   PDF](https://condor.depaul.edu/dmumaugh/OOT/Design-Principles/oodmetrc.pdf).
   No DOI assigned.
2. Martin, R. C. (2003). *Agile Software Development: Principles, Patterns,
   and Practices.* ISBN 978-0-13-597444-5. [Publisher
   record](https://www.informit.com/store/agile-software-development-principles-patterns-and-9780135974445).
