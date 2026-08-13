# ADR 0027 — validate one evidence bundle for advisory architectural smells

- Status: Accepted
- Date: 2026-08-12

## Context

Architectural-smell detection combines topology, module identity, and numerical
metrics. Each input is independently public and serializable. If callers pair a
projection with a stale SCC result, partial assignment, or unrelated Martin
metrics, the detector could otherwise return plausible but false findings.

The output is inherently advisory. A cycle or hub can be intentional, and a
size outlier does not prove excessive responsibility. The API must retain
inspectable evidence without mutating source topology or presenting a binary
quality verdict.

## Decision

Expose `architecture_smells(&GraphProjection, &SccDecomposition,
&[MartinMetrics], &ModuleAssignment, &SmellOptions) -> Vec<ArchSmell>`.

Validate all inputs as one snapshot. Recompute SCCs through the existing
iterative Tarjan engine and require exact equality. Require every projected
node to have a valid module and every used module to have one finite bounded
Martin record. Reject invalid thresholds, malformed CSR, duplicates, and
missing metrics by returning no advisory verdict.

Implement four independent feature-free detectors:

1. multi-node SCC cyclic dependencies;
2. two-sided mean-plus-sigma fan hubs;
3. source-module shares of edges toward greater Martin instability;
4. assigned projection-node-count z-score outliers.

Use stable node evidence, detector-specific finite severity, ordered maps, and
one canonical total output order. Ignore projection weights and self-loops for
fan/module-edge metrics.

## Consequences

- Findings from mismatched analysis generations fail closed.
- Callers can audit every result using nodes and named numerical evidence.
- The same engine serves module, call, and external projections.
- Recomputing SCCs adds one linear pass but removes ambiguous provenance.
- God-component size is portable node count, not source lines; the signature
  contains no line-of-code table, and inventing one would overstate precision.
- Empty output means either no finding or invalid evidence. Callers needing
  differentiated diagnostics should validate inputs before calling; the
  deliberately compact advisory API has no partial-result state.
- Results must be interpreted by humans or higher-level policy, never as
  automatic refactoring authority.

## Alternatives considered

**Trust caller-supplied SCC and metrics.** Rejected because public structs can
be deserialized or reused after a projection changes. Cheap equality validation
is stronger than undocumented temporal coupling.

**Compute all prerequisite analyses internally.** Rejected because Martin
abstractness requires the CPG and module boundaries are caller policy. The
explicit signature makes identity choices visible and reusable.

**Use absolute degree and size constants.** Rejected because fixed constants do
not scale across small and large projections. Configurable population z-score
thresholds preserve relative interpretation.

**Count singleton self-loops as cyclic dependencies.** Rejected by the task's
SCC-at-least-two contract. Self-loops remain represented in the SCC input but
do not become this smell family.

**Return module names instead of nodes.** Rejected because the specified output
is projection-generic and stable `NodeId` evidence composes with existing CPG
queries. Martin evidence still identifies the numerical source-module state.

## Validation

Four isolated oracles trigger each smell, and a clean chain plus singleton
self-cycle trigger none. A 256-case property varies graph topology, edge order,
weights, and assignments while requiring byte-equivalent findings, sorted
in-graph nodes, finite evidence, and canonical order. Hostile tests cover every
option field, metric field, malformed CSR, stale SCC, missing/duplicate metric,
partial assignment, and outside assignment. Public integration exercises the
real CPG import-to-module-projection pipeline in feature-free and serde builds.

See [Scientific validation 22](../scientific/22-architectural-smells-validation.md).

## References

1. Fontana, F. A. et al. (2017). *Arcan: A Tool for Architectural Smells
   Detection*. DOI:
   [10.1109/ICSAW.2017.16](https://doi.org/10.1109/ICSAW.2017.16).
2. Fontana, F. A. et al. (2016). *Automatic Detection of Instability
   Architectural Smells*. DOI:
   [10.1109/ICSME.2016.33](https://doi.org/10.1109/ICSME.2016.33).
