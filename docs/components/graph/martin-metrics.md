# Martin package metrics and SDP analysis

`libcpg::martin_metrics` computes Robert C. Martin's afferent coupling,
efferent coupling, instability, abstractness, and main-sequence distance for a
caller-selected module assignment. `libcpg::sdp_violations` then reports module
dependencies that point from a more stable source to a less stable target.
Both operations are feature-free, deterministic, read-only, and
language-independent after CPG construction.

![Module assignment, dependency projection, CPG declarations, metrics, and SDP witnesses form one deterministic pipeline.](../../diagrams/martin-metrics-pipeline.svg)

*Figure 1 — data and evidence flow. Source:
[`diagrams/martin-metrics-pipeline.puml`](../../diagrams/martin-metrics-pipeline.puml).*

## Public API

```rust
use libcpg::{
    martin_metrics, sdp_violations, CodePropertyGraph, GraphProjection,
    MartinMetrics, ModuleAssignment, SdpViolation,
};

fn architecture_metrics(
    cpg: &CodePropertyGraph,
    dependencies: &GraphProjection,
) -> (Vec<MartinMetrics>, Vec<SdpViolation>) {
    let assignment = ModuleAssignment::by_module_nodes(cpg);
    let metrics = martin_metrics(cpg, &assignment, dependencies);
    let violations = sdp_violations(&metrics, dependencies, &assignment);
    (metrics, violations)
}
```

The snippet requires no feature. Enable `serde` when assignments and results
must be serialized.

### `ModuleAssignment`

```rust
use std::{collections::BTreeMap, sync::Arc};
use libcpg::NodeId;

pub struct ModuleAssignment {
    pub buckets: Vec<Arc<str>>,
    pub of_node: BTreeMap<NodeId, u32>,
}
```

| Constructor/accessor | Contract |
|---|---|
| `from_pairs` | Canonical caller assignment; names sorted/deduplicated, duplicate nodes choose the smallest name. |
| `by_module_nodes` | Nearest enclosing `Module`; nested names use `::`; outside nodes use `<root>`. |
| `by_source_path` | Every node uses the graph source path or `<unknown-source>`; an empty CPG stays empty. |
| `module_of` | Returns the assigned name, or `None` for an absent node/invalid index. |

The fields remain public for explicit deserialization and custom assignment.
Duplicate names in a caller-mutated `buckets` vector are merged by metric name.
An out-of-range index is inert rather than an indexing panic.

### `MartinMetrics`

| Field | Meaning |
|---|---|
| `module` | Canonical module name; results are lexical by this field. |
| `ca` | Distinct external projection nodes with edges into the module. |
| `ce` | Distinct external projection nodes reached by edges out of the module. |
| `instability` | $`I=C_e/(C_a+C_e)`$; zero when both counts are zero. |
| `abstractness` | $`A=(\text{abstract classes}+\text{traits})/(\text{classes}+\text{structs}+\text{enums}+\text{traits})`$; zero without type declarations. |
| `distance` | $`D=|A+I-1|`$, in $`[0,1]`$. |

Projection weights do not affect coupling. Internal edges, self-loops,
unassigned endpoints, and assignments outside the selected projection are
ignored. Type declarations need not themselves be projection vertices: they
contribute to abstractness whenever the assignment maps them into a bucket.

### `SdpViolation`

| Field | Meaning |
|---|---|
| `source_module` / `target_module` | Module dependency $`X\to Y`$ with $`I(X)<I(Y)`$. |
| `source` / `target` | Smallest stable projection edge witnessing that module pair. |
| `dependency_count` | Number of canonical projection edges aggregated into the pair. |
| `source_instability` / `target_instability` | Exact metric values that decided the strict inequality. |

Results are ordered by `(source_module, target_module)`. The function requires
one finite instability in $`[0,1]`$ per metric name. Duplicate names or invalid
numbers make the whole metric collection unsuitable for comparison, so it
returns no verdict. Missing assignment/metric data only suppresses the affected
edge.

## Choosing the dependency projection

The API does not guess what “depends on” means. Common choices are:

| Projection | Vertex interpretation | Edge interpretation | Metric reading |
|---|---|---|---|
| `module_graph_projection` | roots/modules | importer to imported module | module/import coupling |
| `call_graph_projection` | functions | caller to callee | function-level package coupling |
| caller `External` graph | files, classes, crates, services | caller-defined | policy-specific architecture |

The assignment must match the chosen vertex granularity. For example, a
file-level external graph should use source-path buckets supplied for those file
vertices, while a complete CPG call graph can use enclosing `Module` buckets.
An edge-direction reversal exchanges the architectural meaning of $`C_a`$ and
$`C_e`$; it is not a harmless presentation choice.

## Worked caller assignment

```rust
use libcpg::{ModuleAssignment, NodeId};

let assignment = ModuleAssignment::from_pairs([
    (NodeId::new(10), "api"),
    (NodeId::new(20), "domain"),
    (NodeId::new(30), "application"),
]);

assert_eq!(assignment.buckets.iter().map(AsRef::as_ref).collect::<Vec<_>>(),
           ["api", "application", "domain"]);
```

`from_pairs` accepts any value convertible to `Arc<str>`. Stable node IDs may
be sparse and need not match projection dense indices.

## Failure, security, and complexity contracts

- malformed forward/reverse compressed-sparse-row (CSR) fields produce empty
  results; no indexed traversal occurs;
- all AST ancestor walks used by `by_module_nodes` inherit the CPG's cycle and
  dangling-parent guards;
- public output contains only copied names, stable IDs, counts, and scalars;
  neither input is mutated;
- no work grows beyond the supplied CPG/projection, so no separate cap is
  required for these linear/log-linear passes; and
- deterministic `BTreeMap`/`BTreeSet` aggregation makes insertion order and
  hash randomization irrelevant.

For $`n`$ projection vertices, $`m`$ edges, $`t`$ CPG nodes, and $`k`$ module
names, metrics take $`O(m\log n+t\log k)`$ time and $`O(n+k)`$ auxiliary space.
SDP detection takes $`O(m\log k)`$ time and at most one record per module pair.

## Interpretation boundary

These results are exact for the supplied projection and assignment. They do
not prove runtime dependency, change frequency, ownership, package quality, or
the necessity of a refactoring. Parser omissions, unresolved calls, missing
imports, a coarse bucket policy, and generated/adaptor boundaries can all
change the appropriate interpretation. Report the projection kind, assignment
policy, raw counts, and concrete witnesses with any architectural conclusion.

See [Theory 18](../../theory/18-martin-package-metrics.md), [ADR
0024](../../design/0024-explicit-martin-module-assignments.md), and the
[scientific validation record](../../scientific/19-martin-metrics-validation.md).

## References

1. Martin, R. C. (1994). *OO Design Quality Metrics: An Analysis of
   Dependencies.* [Archived report
   PDF](https://condor.depaul.edu/dmumaugh/OOT/Design-Principles/oodmetrc.pdf).
   No DOI assigned.
2. Martin, R. C. (2003). *Agile Software Development: Principles, Patterns,
   and Practices.* ISBN 978-0-13-597444-5. [Publisher
   record](https://www.informit.com/store/agile-software-development-principles-patterns-and-9780135974445).
