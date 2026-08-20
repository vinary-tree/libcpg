# Security boundary 17 — Virtual-call type refinement

CHA/RTA/VTA target sets can change interprocedural reachability, taint,
typestate, slicing, and vulnerability results downstream. They are therefore a
security-relevant evidence boundary even though `devirtualize` is not itself a
vulnerability detector.

## Trust model

Treat these inputs as untrusted or incomplete unless established otherwise:

- nominal names and receiver `TypeInfo`;
- `Inherits`, `Implements`, `TypeOf`, AST, and DFG edges;
- method ownership and static flags;
- constructor `allocation_type` metadata;
- library inclusion and closed-world assumptions; and
- a C1 `CallGraphResolution` supplied by another process.

The analysis validates represented endpoint kinds, rejects ambiguous nominal
names, deduplicates hierarchy edges, and terminates cycles. It cannot establish
that omitted runtime types, reflection, or dynamic loading do not exist.

## Threats and controls

| Threat | Control | Residual risk |
|---|---|---|
| same-named type poisoning | require exactly one represented nominal name | qualified-name normalization is caller/frontend responsibility |
| fabricated allocation from ordinary call syntax | only frontend-normalized `allocation_type` is accepted | unsupported languages provide no allocation evidence |
| stack exhaustion from deep hierarchy/definitions | explicit heap queues/stacks and cycle sets | heap grows with finite graph size |
| call-edge corruption by a speculative mode | immutable `&CodePropertyGraph`; `topology_edges = 0` | a downstream caller may materialize without recording assumptions |
| RTA/VTA false negatives from omitted allocations | document closed-world meaning and preserve mode | no completeness flag can recover absent upstream evidence |
| VTA cross-scope contamination | stop assignment AST traversal at nested functions | field/interprocedural aliases remain unmodeled |
| multiple-inheritance conflict guessed away | retain conservative union | language-specific linearization can be more precise |

## Safe downstream use

- Preserve C1 and refined reports separately.
- Record the selected `DevirtualizationMode` and graph revision.
- Use CHA when allocation completeness is not defensible.
- Treat RTA/VTA empty sets as represented observations, never proof of runtime
  unreachability.
- Do not remove broader topology solely from a narrower immutable report.
- If appending singleton edges, make the operation explicit, auditable, and
  invalidated by hierarchy/allocation/DFG changes.
- For security-critical absence claims, supplement static evidence with a
  frontend/runtime model appropriate to reflection, loading, foreign code, and
  framework dispatch.

![Every precision gain adds an evidence assumption; no mode silently writes the graph.](../diagrams/type-refinement-lattice.svg)

See [ADR-0052](../design/0052-closed-world-type-refinement.md),
[Usage 25](../usage/25-virtual-call-type-refinement.md), and
[Scientific validation 47](../scientific/47-type-refinement-validation.md).
