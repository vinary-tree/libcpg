# Component — API-usage protocol mining

The feature-free `analysis::bugs::api_mining` component turns existing typed
graph evidence into centered API Usage Graphs (AUGs), exact population patterns,
advisory deviations, and named temporal-property findings. It does not parse
source, add graph edges, modify metadata, or apply repairs.

## 1. Public entry points

| API | Contract |
|---|---|
| `analyze_api_usage_mining(cpg, properties, config)` | full qualified mining and temporal report |
| `api_usage_misuses(cpg, config)` | positive-only usage deviations; discards completeness and temporal evidence |
| `ApiUsageGraph::is_acyclic()` | iterative endpoint and directed-cycle validation |
| `ApiUsageMiningAnalysis::is_complete()` | mining issues empty and every delegated temporal report complete |

Use the full analyzer for every negative claim. An empty
`api_usage_misuses` vector alone cannot distinguish a clean complete population
from excluded or capped evidence.

## 2. Input contract

The component consumes:

- `CodePropertyGraph::calls()` for the finite inventory;
- the shared `call_owners` AST scope relation;
- `call_evidence` for resolved-target or typed-callee `ApiIdentity`;
- `cfg_projection` for per-function reachability;
- `call_arguments` for positional arguments;
- typed `MemberAccess` structure for a method receiver;
- incoming `ControlDependence` edges for direct control context;
- caller-owned `NamedTemporalProperty` values.

Every input relation is read-only. A parser feature may construct the graph, but
the analysis itself remains available under `--no-default-features`.

## 3. Centered graph schema

`ApiUsageGraph` retains the anchor CPG node, containing function, anchor API,
dense local nodes, and typed edges. Local node identifiers are indices into the
ordered node vector and are independent from source `NodeId` values.

| Node kind | Source evidence |
|---|---|
| `AnchorCall` | admitted call site |
| `FollowingCall` | another admitted call reachable later in the same CFG |
| `Data` | argument or typed method receiver |
| `Control` | direct controller with normalized context |

| Edge kind | Direction | Meaning |
|---|---|---|
| `Parameter(i)` | data → anchor | zero-based positional argument |
| `Receiver` | data → anchor | method receiver |
| `Selection` | control → anchor | direct control-dependence source |
| `Order` | anchor → action | other call may execute later |

![The component's graph schema is centered, typed, source-linked, and acyclic](../../diagrams/centered-api-usage-graph.svg)

`ApiDataShape` is language-neutral but evidence-preserving: it distinguishes
identifiers, variables, members, indexes, nested calls, exact literal kinds and
values, unary/binary/assignment operators, lambdas, unknown frontend kinds, and
other typed nodes. Method-receiver extraction requires an actual method call and
typed `MemberAccess`; it never guesses from text.

## 4. Analysis phases

### Phase A — validate

Reject invalid exact thresholds, non-majority thresholds, population size below
two, zero/excessive caps, zero callee depth, excessive property count, empty
property names, and duplicate property names. Each `PropertyFsm` is validated by
the delegated typestate engine.

### Phase B — temporal properties

Sort properties by name. For each property, run `analyze_resource_leaks` with
the supplied `IfdsCaps`; project typestate violations to
`ForbiddenTransition` and resource leaks to `NonAcceptingExit`; retain the
entire delegated report.

### Phase C — call admission

Sort all call sites. If their count exceeds `max_call_sites`, record one atomic
`CallSiteCap` and return without partial mining. Otherwise resolve each owner and
identity. Missing and depth-capped identities remain explicit issues.

### Phase D — centered AUG construction

Group calls by function, create one CFG projection, and breadth-first traverse
from every anchor. Gather later calls, positional data, optional receiver, and
direct controllers. Calculate required node/edge counts before allocation.
Exclude the entire site on a reachability or graph cap.

### Phase E — exact fragment mining

Group complete observations by anchor API. Populations smaller than
`minimum_population` are intentionally not inferred and are not incomplete.
Count each fragment in ordered maps, compare support exactly, emit patterns, and
emit a deviation for each non-supporting complete site.

### Phase F — canonicalize

Sort/deduplicate patterns, usage deviations, and temporal deviations.
`BTreeSet` already canonicalizes issues. The report is stable across repeated
runs and CFG edge insertion permutations.

![The complete component flow preserves qualification from evidence to report](../../diagrams/api-usage-temporal-pipeline.svg)

## 5. Issue semantics

| Issue | Scope | Interpretation |
|---|---|---|
| `CallSiteCap` | whole mining phase | no call enters any population |
| `CallOutsideFunction` | site | AST owner not represented |
| `MissingCalleeIdentity` | site | no represented target/path identity |
| `CalleeIdentityCapped` | site | typed callee walk hit its depth bound |
| `CallOutsideCfg` | site | owner exists but site is absent from CFG |
| `CfgEvidenceCap` | site | reachability exceeded its node budget |
| `GraphNodeCap` | site | centered graph would exceed node budget |
| `GraphEdgeCap` | site | centered graph would exceed edge budget |

Any issue makes the aggregate report incomplete. Delegated typestate issues do
not enter this enum; they stay losslessly available in `temporal_analyses` and
also make `is_complete()` false.

## 6. Stack-safety inventory

| Input-shaped operation | Stack-safe construct |
|---|---|
| CFG reachability | `VecDeque` breadth-first worklist |
| call/function/API grouping | heap `BTreeMap` |
| reached nodes/fragments/issues | heap `BTreeSet` |
| AUG node/edge storage | flat `Vec` arenas |
| acyclicity | iterative Kahn queue |
| temporal propagation | existing typestate heap worklist |
| output destruction | flat, non-recursive public values |

The 100,000-CFG-node test runs the entire mining path on a 64-KiB native thread
stack. Resource caps bound memory/work independently from native stack size.

## 7. Complexity

Let $`C`$ be admitted call sites, $`N_f`$ and $`E_f`$ the nodes and edges in
function $`f`$'s CFG, and $`A_c`$ the size of centered graph $`c`$. The current
per-anchor reachability cost is:

```math
O\!\left(
\sum_{c\in C}
(N_{f(c)}+E_{f(c)}+A_c)
\right)
```

Ordered aggregation adds logarithmic map/set factors. This bound explains why
`max_cfg_nodes_per_site` is distinct from graph-node and graph-edge caps.
Temporal cost is the sum of the delegated typestate runs and their configured
`IfdsCaps`.

## 8. Ownership and module boundaries

- `bugs::evidence` owns shared `ApiIdentity`, exact population arithmetic, and
  typed callee evidence.
- `analysis::projection` owns canonical CFG projections.
- `builder` owns frontend syntax normalization and `call_arguments`.
- `typestate` and `bugs::leaks` own finite-state propagation and terminal
  projection.
- `api_mining` owns centered usage graphs, pattern census, deviations, property
  naming, and the aggregate completeness rule.

This division prevents a detector-specific parser, reachability engine, support
arithmetic implementation, or temporal solver.

See [Theory 37](../../theory/37-api-usage-graphs-and-temporal-properties.md),
[ADR-0047](../../design/0047-evidence-bounded-api-usage-and-temporal-mining.md),
the [validation ledger](../../scientific/42-api-usage-temporal-validation.md),
and [Usage 20](../../usage/20-api-usage-protocol-mining.md).

## References

1. Nielebock, S., Blockhaus, P., Krüger, J., Ortmeier, F. (2024).
   *ASAP-Repair: API-Specific Automated Program Repair Based on API Usage
   Graphs.* DOI:
   [10.1145/3643788.3648011](https://doi.org/10.1145/3643788.3648011).
2. Song, Y., Gao, X., Li, W., Chin, W.-N., Roychoudhury, A. (2024).
   *ProveNFix: Temporal Property-Guided Program Repair.* DOI:
   [10.1145/3643737](https://doi.org/10.1145/3643737).
