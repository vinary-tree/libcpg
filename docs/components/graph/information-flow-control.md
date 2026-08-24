# PDG information-flow and constant-time analysis

This feature-free component queries an immutable program dependence graph
(PDG) and represented system-dependence summary edges. It returns canonical
High-to-Low flow witnesses and secret-dependent timing-site evidence without
modifying the code property graph (CPG).

![The immutable CPG is projected once, then separate product-state and timing-site searches emit canonical advisory evidence.](../../diagrams/ifc-evidence-pipeline.svg)

*Figure -- component inputs, searches, and public outputs. Source:
[`diagrams/ifc-evidence-pipeline.puml`](../../diagrams/ifc-evidence-pipeline.puml).*

## Public API

```rust
use std::collections::BTreeSet;

use libcpg::{
    constant_time, noninterference, CodePropertyGraph, FlowViolation,
    LeakSite, NodeId, SecurityLabels,
};

fn audit(
    cpg: &CodePropertyGraph,
    high: NodeId,
    low: NodeId,
) -> (Vec<FlowViolation>, Vec<LeakSite>) {
    let labels = SecurityLabels::new([high], [low]);
    let leaks = noninterference(cpg, &labels);
    let timing = constant_time(cpg, &BTreeSet::from([high]));
    (leaks, timing)
}
```

Both functions are available with `default = []`. A parser feature is needed
only when libcpg itself parses source; callers using `build_from_tree` or a
hand-built CPG do not enable an analysis feature.

## Policy and finding types

| Type | Meaning |
|---|---|
| `SecurityLabels` | Ordered High sources, Low sinks, and declassifier cut vertices. |
| `FlowViolation` | Source, sink, path classification, and optional explicit/implicit witnesses. |
| `FlowViolationKind` | `Explicit`, `Implicit`, or `ExplicitAndImplicit`. |
| `InformationFlowWitness` | Ordered source-to-target dependence steps; empty when source equals site/sink. |
| `InformationFlowEdgeKind` | Ordinary data dependence, labeled summary dependence, or control dependence. |
| `LeakSite` | Secret, timing site, timing mechanism, and canonical influence witness. |
| `LeakSiteKind` | True conditional edge, false conditional edge, or index access. |

All fields are public. Under the `serde` feature, the policy, findings,
witnesses, steps, and enums derive serialization. `SecurityLabels::new`
creates a policy with no cuts; `with_declassifier` adds one cut while retaining
builder-style ownership.

## Admitted graph evidence

The projection follows only:

- `CpgEdgeKind::DataDependence`;
- `DataDependence` with the exact SDG summary label, classified separately as
  `InformationFlowEdgeKind::Summary`;
- `CpgEdgeKind::ControlDependence`.

It deliberately does not follow AST, CFG, raw DFG, call, parameter, or return
edges. Build the PDG first with `PdgBuilder`. Build or compose system-dependence
summaries when interprocedural return influence is required. Raw call-boundary
edges do not encode realizable context matching by themselves.

`constant_time` separately scans CFG edges whose kind is `ConditionalTrue` or
`ConditionalFalse`, and nodes whose kind is `IndexAccess`. Each distinct
conditional edge is a distinct finding; a conventional binary branch can
therefore yield two findings at one site.

## Algorithm and ordering

`noninterference` uses one dense product projection with two states per node.
The second state component records whether the path has encountered a control
dependence. `constant_time` uses ordinary influence reachability and a stable
timing-site table.

```text
PROJECT(cpg):
    sort represented node ids
    map node ids to dense indices
    retain admitted influence edges
    sort and deduplicate every outgoing row by stable target and edge class

SEARCH(source):
    enqueue the source state
    visit each state at most once in breadth-first order
    record the first parent edge
    reconstruct a witness by walking parents, then reverse it iteratively
```

Public ordering is deterministic:

1. sources or secrets by `NodeId`;
2. sinks or timing sites by `NodeId`;
3. timing mechanisms by enum order;
4. shortest-path ties by stable target and influence-edge class.

Parallel duplicate dependence edges do not duplicate reachability. Parallel
conditional edges with equal source, target, and kind collapse to one timing
mechanism; distinct targets or true/false kinds remain separate.

## Declassification

A declassifier is a sanctioned confidentiality release and a hard graph cut.
Traversal cannot start at or enter it. Consequently a High-to-Low path that
must cross the declassifier is clean under that policy. The analysis does not
try to infer declassifiers from names or source syntax.

Declassification does not apply to `constant_time`. An authorized output can
still leak more information through a secret-dependent branch or address.

## Totality and immutability

Labels outside the graph are ignored. An empty graph, empty labels, or absent
timing sites produces an empty vector. The CPG is borrowed immutably, and
repeated calls return equal values.

There is no result cap: every represented source/sink pair and timing mechanism
is returned. The caller must bound graph admission and response handling. This
choice prevents cap exhaustion from masquerading as an empty security result.

## Complexity and stack safety

Dense construction and canonical sorting cost
$`O(n\log n+m\log m)`$ in the worst case. Each represented source then costs
$`O(n+m)`$ reachability plus emitted witness length. Queues, parent arrays,
product bits, and witness vectors are heap resident. Construction, search,
reconstruction, comparison, debug, serialization, and destruction contain no
input-depth-dependent recursion owned by this component.

## Interpretation

Positive findings are exact for the admitted relations, labels, and timing
site definitions. They remain advisory about runtime feasibility and impact.
An empty result is not proof about omitted frontend facts, dynamic dispatch,
aliases, native code, concurrency, speculation, caches, or other platform
timing behavior.

See [Theory 49](../../theory/49-pdg-information-flow-and-constant-time.md),
[ADR-0066](../../design/0066-deterministic-pdg-information-flow.md), the
[security boundary](../../security/24-pdg-information-flow-boundary.md), and
[Usage 34](../../usage/34-pdg-information-flow.md).

## References

1. Hammer, C., Snelting, G. (2009). *Flow-sensitive, context-sensitive, and
   object-sensitive information flow control based on program dependence
   graphs.* DOI:
   [10.1007/s10207-009-0086-1](https://doi.org/10.1007/s10207-009-0086-1).
2. Barthe, G., Betarte, G., Campo, J. D., Luna, C., Pichardie, D. (2014).
   *System-level non-interference for constant-time cryptography.* DOI:
   [10.1145/2660267.2660283](https://doi.org/10.1145/2660267.2660283).
