# Security boundary -- Rust place-capability analysis

## Security claim

libcpg returns deterministic, replayable advisory ownership and borrowing
evidence for the Rust operations represented in one function's Code Property
Graph (CPG). It does not replace rustc, certify memory safety, prove absence of
undefined behavior, or reconstruct every Rust trait, projection, region, or
non-lexical lifetime (NLL) fact.

## Trusted evidence

| Evidence | Trust condition |
|---|---|
| operation kind | Rust frontend normalized a typed move/shared-borrow/mutable-borrow syntax node |
| local identity | source place reaches the operand by `DefUse` or `ReachingDef` |
| field identity | operand has an exact `Reference` to a represented field declaration |
| function boundary | operation and target belong to iterative AST function scope |
| ordering | represented CFG reachability connects the relevant operation/use sites |
| loan liveness | a later reachable DFG use of the destination reference exists |
| copy decision | normalized `TypeInfo` belongs to the documented closed classification |
| overlay | report and target CPG share node identity and compatible schema/version |

The analyzer never reparses source and never matches ownership by token text.

## False-positive boundary

- A nominal type may implement `Copy` outside represented evidence. libcpg
  retains a possible move and marks its findings qualified.
- Same `NodeId` source identity is exact for represented definitions, but
  compiler projection/disjoint-field knowledge may prove two accesses do not
  overlap more precisely.
- CFG reachability is a may-path relation. A path-infeasible later use can
  support an advisory finding.
- The closed standard owning-type list classifies common library containers;
  custom semantic wrappers remain unknown.

## False-negative boundary

- Missing frontend ownership metadata or DFG/`Reference` identity can omit a
  transfer; `MissingSource` is visible only for represented operations.
- The CPG lacks rustc trait solving, MIR place projections, full NLL/Polonius
  origins, two-phase borrows, composite lifetime projections, and compiler
  desugaring facts.
- Field/index alias overlap and interprocedural ownership effects are bounded
  by existing graph identity.
- Loop/path precision is limited to repeated CFG reachability and represented
  destination uses.

Therefore an empty `borrow_check` vector is not a memory-safety certificate.

## Stack and resource boundary

AST scope, operand discovery, ancestor walks, and CFG reachability use explicit
heap worklists and cycle guards. Native call depth is constant in source and
graph depth; a 100,000-level test runs on a 64 KiB worker stack.

The current API has no analysis-specific retained-work cap. A large function
can require quadratic borrow-pair reachability work and substantial ordered-set
memory. At an untrusted boundary, limit input/graph size, wall time, resident
memory, swap, and task count at the process level. Stack safety is not resident
memory safety.

## Mutation and serialization integrity

`materialize_place_capability_overlay` validates every relation endpoint
before adding the first edge, adds no nodes, and is idempotent. A deserialized
`Pcg` is data, not an attestation that it belongs to the target CPG. Bind
reports to graph identity, libcpg version, frontend version, and export schema
before materialization.

Serialized edge variants and PyG columns/classes are append-only, but consumers
must version the full schema. Preserving older numeric identifiers does not
teach older software the meaning of a new suffix.

## Operational checklist

1. Validate the caller-owned Rust grammar/frontend version.
2. Require typed DFG/`Reference` evidence; do not recover identity from text.
3. Retain `Pcg::issues` and the `qualified` finding bit.
4. Replay exact function, place, operation, and witness node identifiers.
5. Materialize only into the graph from which the report was derived.
6. Enforce process limits independently of algorithmic stack safety.
7. Use rustc for authoritative compilation and borrow checking.

See the [theory](../theory/54-place-capability-ownership-and-borrowing.md),
[component contract](../components/graph/place-capability-analysis.md),
[usage guide](../usage/39-place-capability-analysis.md), and
[validation dossier](../scientific/66-place-capability-validation.md).

## References

1. Grannan, D. et al. (2026). DOI:
   [10.1145/3763122](https://doi.org/10.1145/3763122).
2. Matsushita, Y. et al. (2020). DOI:
   [10.1007/978-3-030-44914-8_18](https://doi.org/10.1007/978-3-030-44914-8_18).
3. Crichton, W. et al. (2022). DOI:
   [10.1145/3519939.3523445](https://doi.org/10.1145/3519939.3523445).
