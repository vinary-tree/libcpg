# ADR-0010: Dual-worklist sparse conditional constant propagation

## Status

Accepted.

## Context

Plain CFG reachability treats both sides of every conditional as possible.
Ordinary constant propagation over a basic-block environment likewise lets
definitions from infeasible branches degrade a value at later joins. libcpg
needs one feature-free analysis that couples path feasibility with scalar value
facts while preserving the unified-overlay discipline of
[ADR-0001](0001-unified-overlay-graph.md).

The source CPG is deliberately not in static single assignment (SSA) form.
Instead, its DFG exposes definition-to-use edges, and its AST exposes expression
operands. The decision therefore has to use those existing overlays rather than
materialize a second intermediate representation.

## Decision

Implement Wegman–Zadeck SCCP as two deterministic worklists:

1. a flow worklist marks canonical `FunctionCfgView` edges executable; and
2. a value worklist propagates a height-two constant lattice through AST parent
   dependencies and DFG `DefUse` successors.

Identifier uses join only executable reaching definitions. Constant boolean
guards admit only the matching true/false edge. Every result is an out-of-graph
`SccpResult`; `ReachabilityReport::refine_with_sccp` composes structural and
value-sensitive evidence without changing nodes or edges.

Worklists are `BTreeSet<NodeId>` values, public collections are canonically
ordered, integer folding is checked, and a defensive
$`4(N + D)`$ event cap bounds malformed inputs for $`N`$ scope nodes and $`D`$
`DefUse` edges.

## Consequences

### Positive

- Path and value precision reinforce one another; a dead definition cannot
  degrade a live use.
- The analysis reuses AST, CFG, and DFG evidence already present in the CPG.
- No SSA construction, phi-node insertion, or graph mutation is required.
- `default = []` consumers receive the complete API.
- Checked arithmetic never turns undefined or overflowing integer evaluation
  into a false constant proof.
- Stable worklists and ordered output make results reproducible across graph
  insertion orders.

### Negative

- Precision is limited by the existing `DefUse` overlay; absent or conservative
  DFG edges lead to `Overdefined` results.
- Case labels are not encoded in `CfgEdgeKind`, so `Case` and `DefaultCase`
  cannot yet be pruned by scalar comparison.
- The domain does not model heap aliases, overloaded operators,
  interprocedural effects, or language-specific numeric widths.
- A separate interval analysis remains necessary for non-singleton numeric
  ranges.

## Alternatives considered

### Dense block-environment constant propagation

Rejected as the primary algorithm because every block would carry a complete
variable map and infeasible branch definitions would merge before path
feasibility could remove them. It also ignores the sparse `DefUse` overlay.

### Build SSA first

Rejected for this layer because it would add a second IR, phi placement, and
rename machinery before libcpg can answer the narrower constant/path query.
Implicit joins over executable `DefUse` predecessors provide the required
sound over-approximation without abandoning stable CPG node identity.

### Rewrite the CPG in place

Rejected because deleting dead edges or replacing expressions would violate
the append-only unified-overlay discipline and would conflate advisory evidence
with an approved language-aware transformation.

### Treat every unknown as immediately overdefined

Rejected because it would mark both conditional edges before a not-yet-reached
definition had the opportunity to become constant. `Unknown` is lattice bottom;
runtime inputs become `Overdefined` only when executable.

## References

1. Wegman, M. N., Zadeck, F. K. (1991). *Constant Propagation with Conditional
   Branches.* ACM TOPLAS 13(2), 181–210. DOI:
   [10.1145/103135.103136](https://doi.org/10.1145/103135.103136)
