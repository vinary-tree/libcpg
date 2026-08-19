# ADR-0044: Compose advisory bug detectors from existing CPG evidence

- **Status:** Accepted
- **Date:** 2026-08-19
- **Scope:** always-on `analysis::bugs`

## Context

A resource leak, an uninitialized read, and an escaping exception are different
defect families, but all three are questions about evidence already normalized
in a Code Property Graph (CPG). Re-parsing source text inside a detector would
duplicate each language frontend, disagree on recovered syntax, and miss the
typed control-flow, data-flow, definition, call, type, and inheritance relations
that the builder has already established.

The detectors must remain useful on partial static evidence without presenting
an empty result as proof of safety. They must also preserve graph identity, be
available with `default = []`, return deterministic values, and keep native
call-stack use independent of source and graph depth.

![Three detectors compose normalized evidence without parsing or mutating the graph](../diagrams/bug-detector-pipeline.svg)

## Decision

1. Place all three detectors in the feature-free `analysis::bugs` module and
   re-export their report and projection APIs from the crate root.
2. Implement resource-leak detection as a projection of typestate terminal
   evidence. A non-accepting `(object, exit, state)` tuple is a leak candidate;
   the complete underlying `TypestateAnalysis` remains available to callers.
3. Implement uninitialized-use detection by reusing the shared, binder-aware
   variable fact universe and represented reaching-definition edges. Exclude
   declaration-signature and unbound callee syntax because neither is an
   executable variable read.
4. Normalize exception constructs in the frontend as reserved `Throw` and
   `Catch` nodes. Emit `CfgEdgeKind::Catch` from every represented catch clause
   to its handler body so handler entry is distinguishable from ordinary
   sequencing.
5. Represent a function's exception summary as a finite ordered set of nominal
   `ExceptionType` values. Reuse the shared strongly connected component (SCC)
   summary solver to propagate callee summaries through recursive call groups.
6. Dispatch direct and propagated exceptions through lexically nearest handler
   layers in source order. Match exact nominal types or represented transitive
   `Inherits`/`Implements` supertypes; an untyped catch is catch-all.
7. Report exceptions escaping every function in a source SCC of the call
   condensation graph. Report a catch as dead when no represented direct or
   propagated exception is dispatched to it.
8. Retain canonical node-identifier witnesses for leak exits and uncaught
   propagation. Return unknown types, ambiguous calls, malformed catch-entry
   edges, missing witnesses, and shared-solver caps as typed completeness
   evidence.
9. Keep the CPG read-only. Use `BTreeMap`, `BTreeSet`, vectors, dense graph
   projections, and explicit heap worklists for all input-sized traversal and
   witness reconstruction.
10. Keep convenience functions (`resource_leaks`, `uninitialized_uses`,
    `uncaught_exceptions`, and `dead_catches`) positive-only. Require the
    corresponding analysis report before a consumer makes a negative claim.

## Rejected alternatives

### Parse source strings in each detector

Rejected. Text matching cannot reliably distinguish code from comments and
strings, resolve definitions, follow aliases, identify handler scope, or honor
frontend recovery. The language mapper is the single syntax-normalization
boundary.

### Build a second private control-flow graph

Rejected. Parallel topology would drift from the public CPG and make witnesses
unrelatable to caller node identifiers. The detectors consume the canonical CFG
view and reserved edge kinds already present in the graph.

### Treat every identifier with no incoming DFG edge as uninitialized

Rejected. Function names, method names, type syntax, declaration names, and
unbound callee identifiers are not variable reads. The shared fact universe
supplies binder-aware uses; scope membership qualifies reaching definitions.

### Infer release calls from a fixed name list

Rejected. Ownership protocols are application-specific. `PropertyFsm` lets the
caller define allocation, release, and acceptable terminal states while the
detector reuses the same validated transfer semantics.

### Traverse call paths recursively

Rejected. Mutual recursion is a graph fixed point, not a native-language call
stack problem. SCC condensation plus bounded Kleene iteration terminates over
the finite exception-type lattice and remains safe on deep call chains.

### Suppress findings when evidence is incomplete

Rejected. Positive evidence can remain reviewable under a cap, while a negative
claim cannot. Findings therefore carry cap qualification and reports expose all
known incompleteness.

## Consequences

### Positive

- One normalized graph supports three distinct detectors without a language
  parser or graph mutation in the analysis layer.
- Allocation identity, reaching definitions, nominal types, handler entry, call
  summaries, and witnesses remain inspectable.
- Ordered finite domains make repeated runs and serialized reports stable.
- Deep AST/CFG/call evidence is processed by heap-backed machines; a public
  100,000-level integration test completes on a 256 KiB native stack.

### Negative

- Findings are may-analysis candidates. Infeasible branches, conservative
  aliases, or imprecise call targets can produce false positives.
- Missing frontend, type, definition, call, or inheritance evidence can hide a
  defect; observable gaps are issues, but absent external behavior cannot be
  enumerated.
- Resource leaks inherit allocation-site abstraction and caller policy.
- Exception summaries are type sets, not path-, object-, or context-sensitive
  exception states; catch ordering is lexical and evidence-bounded.
- Uninitialized-use completeness describes the represented CPG relation, not
  language-runtime initialization outside that relation.

## Verification

Example tests cover balanced and leaking branches, bodyless/manual and parsed
functions, shadowed/out-of-scope definitions, direct and interprocedural throws,
nested handlers, catch-all and subtype matching, dead catches, malformed
evidence, serialization, and every applicable cap. Deterministic generated
oracles cover leak terminal projection, reaching-definition emptiness, and 256
exception-call chains. Public Rust and Java integrations compare `build` with
`build_from_tree`, assert idempotent catch topology, preserve graph counts, and
exercise 100,000-level input on a 256 KiB stack.

## References

1. Shadab, N., Gharat, P., Ernst, M. D., Kellogg, M., Lahiri, S. K., Lal, A.,
   Sridharan, M. (2025). *Lightweight and Modular Resource Leak Checking
   (Extended Version).* International Journal on Software Tools for Technology
   Transfer, 27, 267–288. DOI:
   [10.1007/s10009-025-00804-2](https://doi.org/10.1007/s10009-025-00804-2).
2. Reps, T., Horwitz, S., Sagiv, S. (1995). *Precise Interprocedural Dataflow
   Analysis via Graph Reachability.* POPL. DOI:
   [10.1145/199448.199462](https://doi.org/10.1145/199448.199462).
3. Sinha, S., Harrold, M. J. (2000). *Analysis and Testing of Programs with
   Exception Handling Constructs.* IEEE Transactions on Software Engineering,
   26(9), 849–871. DOI:
   [10.1109/32.877846](https://doi.org/10.1109/32.877846).
