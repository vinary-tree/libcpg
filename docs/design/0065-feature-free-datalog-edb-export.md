# ADR-0065: Keep Datalog export feature-free and rule evaluation external

- **Status:** Accepted
- **Date:** 2026-08-23
- **Scope:** relational export, Soufflé interoperability, deterministic
  ordering, stack safety, query-engine boundary

## Context

Many code analyses are concise as relational rules over AST, CFG, DFG, PDG,
call, type, scope, and import facts. Embedding one Datalog runtime would,
however, couple libcpg to its parser, compiler/runtime, deployment model,
resource policy, and release cadence.

## Decision

1. `export_datalog(&CodePropertyGraph) -> DatalogFacts` is
   feature-free.
2. libcpg emits typed Rust tuples and canonical in-memory Soufflé
   `.facts` buffers using RFC 4180 tab-separated fields. Shipped input
   directives opt into that exact format. libcpg performs no file I/O and
   evaluates no caller rule.
3. Relation-specific vectors use set semantics. A lossless
   `cpg_edge` master relation retains edge identity, parallel
   multiplicity, exact kind, and label for overlay reconstruction.
4. Export uses explicit iteration and sorting; CPG depth never becomes native
   call depth.
5. Rule files are examples. They do not expand libcpg's correctness claims,
   and external Datalog execution requires its own caps and trust boundary.

![The feature-free boundary ends at canonical relation buffers; parsing and fixed-point evaluation stay with the external Datalog engine.](../diagrams/datalog-export-pipeline.svg)

*Figure -- accepted ownership boundary. Source:
[`diagrams/datalog-export-pipeline.puml`](../diagrams/datalog-export-pipeline.puml).*

## Consequences

Consumers can select Soufflé or another engine without changing libcpg's
feature graph. Exact typed facts are usable directly in Rust even when no
Datalog binary is installed. libcpg cannot promise that arbitrary external
rules terminate within a caller's time or memory budget.

The duplication between relation-specific vectors and `cpg_edge` is
intentional. Removing the master relation would collapse parallel edges under
Datalog set semantics; removing the query-oriented relations would force every
ruleset to repeatedly decode edge tags.

## Alternatives considered

### Embed Soufflé glue behind a `datalog` feature

Rejected for the initial surface. The pure exporter needs no dependency, and
process/library integration would add deployment and resource-policy questions
without changing EDB semantics.

### Implement a Datalog parser and evaluator in libcpg

Rejected. That would duplicate mature engines and recreate parser, optimizer,
fixed-point, provenance, and resource-control responsibilities unrelated to
the CPG core.

### Export only a generic edge table

Rejected. It is lossless but needlessly verbose for ordinary rule authors.
The accepted dual surface preserves both query ergonomics and reconstruction.

### Use e-graphs instead

Rejected as a substitution. E-graphs represent congruence/equality saturation;
Datalog represents relational consequence. A future composition is tracked in
the Dovetail follow-up epic.

## References

1. Bravenboer, M., Smaragdakis, Y. (2009). *Strictly Declarative Specification
   of Sophisticated Points-to Analyses.* OOPSLA. DOI:
   [10.1145/1640089.1640108](https://doi.org/10.1145/1640089.1640108).
2. Jordan, H., Scholz, B., Subotić, P. (2016). *Soufflé: On Synthesis of Program
   Analyzers.* CC. DOI:
   [10.1007/978-3-319-41540-6_23](https://doi.org/10.1007/978-3-319-41540-6_23).
