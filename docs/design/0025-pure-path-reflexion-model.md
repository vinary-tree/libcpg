# ADR 0025 — keep reflexion conformance pure and path-based

- Status: Accepted
- Date: 2026-08-12

## Context

The source capability in pgmcp couples reflexion logic to its configuration
types and uses a randomized `HashSet` for realized layer pairs. `libcpg` needs a
reusable feature-free kernel that pgmcp and other callers can feed without
pulling configuration, database, parser, or filesystem concerns into the
library. Its inputs may come from CPG imports, build metadata, or an external
dependency index.

## Decision

Expose `LayerSpec`, `ReflexionRules`, `ReflexionVerdict`,
`ReflexionDivergence`, and `ReflexionSummary` as independent value types.
Accept observed `(Arc<str>, Arc<str>)` path edges directly. Assign paths by the
first textual prefix match, preserve duplicate edge observations, store
realized allowed pairs in a `BTreeSet`, sort divergences by path pair, and
derive absences from the declared `allow` relation.

The crate does not provide a pgmcp-configuration conversion. That compatibility
shim belongs in pgmcp, where both types are available. The core performs no
path normalization or filesystem access because the dependency producer is the
only component that knows the intended path namespace.

## Consequences

- The API is available with `default = []` and has no new dependency.
- Callers can use dependency paths without manufacturing CPG nodes.
- First-match layer order and lexical path form are explicit policy inputs.
- Ordered values provide reproducible equality, serialization, and reports.
- Duplicate dependencies retain multiplicity; pair realization is set-valued.
- The caller must normalize and bound untrusted paths/policies before use.
- Adding CPG-derived convenience adapters later does not alter this kernel.

## Alternatives considered

**Bind to pgmcp configuration.** Rejected because it reverses dependency
direction, prevents general reuse, and imports application policy into a graph
library.

**Infer layers from directory components.** Rejected because repositories use
overlapping roots, generated code, workspaces, and virtual path namespaces;
inference would silently change declared intent.

**Canonicalize through the filesystem.** Rejected because inputs can be
virtual, nonexistent, or analyzed off-host, and I/O would make a pure result
environment-dependent.

**Deduplicate all observed edges.** Rejected because convergence/unlayered
counts and concrete divergence multiplicity describe the actual supplied
relation. Only the existential realized-pair question is deduplicated.

## Validation

The exact layered oracle covers all four edge verdicts plus absence. A 256-case
property reverses generated edge multisets and requires identical summaries,
sorted divergences, exhaustive classification, and permitted realized pairs.
Public integration and serde tests protect the crate-root boundary. See
[Scientific validation 20](../scientific/20-reflexion-model-validation.md).

## Reference

1. Murphy, G. C., Notkin, D., & Sullivan, K. J. (2001). Software reflexion
   models: Bridging the gap between design and implementation. *IEEE
   Transactions on Software Engineering, 27*(4), 364–380. DOI:
   [10.1109/32.917525](https://doi.org/10.1109/32.917525).
