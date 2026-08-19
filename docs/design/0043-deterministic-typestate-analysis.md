# ADR-0043: Check finite-state object properties with deterministic CPG evidence

- **Status:** Accepted
- **Date:** 2026-08-19
- **Scope:** always-on `analysis::typestate`

## Context

A **typestate property** constrains which operations are valid for an object in
each abstract state. For example, `close` is valid for an open resource, while
`use` after `close` is forbidden. Strom and Yemini introduced typestate as a
mechanism for associating legal operations with an object's state
[[1]](#references); later work combined typestate checking with scalable static
analysis [[2]](#references).

The epic requires a feature-free advisory checker over existing Code Property
Graph (CPG) evidence. It must not invent a parser, mutate graph topology, hide
missing evidence, conflate allocations, depend on hash iteration, or map graph
depth onto the native call stack.

![The finite-state property, typed CPG evidence, object flow, CFG facts, and qualified output](../diagrams/typestate-analysis.svg)

## Decision

1. Represent caller policy as `PropertyFsm`: closed state and event
   vocabularies, deterministic ordinary transitions, allocation-event seed
   states, accepting states, and one absorbing error state.
2. Validate the complete property before inspecting the CPG. Invalid
   vocabularies, allocation seeds, or error transitions produce ordered
   `PropertyFsmIssue` values and no partial execution.
3. Classify events from the existing typed callee subtree and represented call
   targets. Do not parse source text. Preserve unresolved, ambiguous, and
   depth-capped classification as explicit report evidence.
4. Give every allocation call its own abstract-object identity. Propagate that
   identity over typed DFG, reference, definition, and alias evidence with an
   ordered heap worklist.
5. Project each represented function through the canonical CFG view and
   propagate may-facts `(allocation_site, property_state)`. Join by set union.
6. Treat a missing transition as forbidden and enter the designated absorbing
   error state. Emit the first forbidden event for each path state rather than
   duplicate follow-on findings.
7. Retain the allocation, violating site, prior state, event, and a canonical
   predecessor witness. Return terminal state sets separately so clients may
   check accepting-state policy without confusing it with forbidden events.
8. Apply `IfdsCaps` to function count, facts per node, total worklist steps, and
   typed callee-path depth. This intraprocedural finite worklist requires no SCC
   convergence rounds, so `max_scc_iters` is intentionally inapplicable.
9. Keep all input-sized state in `BTreeMap`, `BTreeSet`, vectors used as explicit
   worklists, or the canonical flat graph projection. No recursive graph walk or
   input-sized slice sort is part of the analysis path.
10. Expose `analyze_typestate` for qualified reports and `check_typestate` only
    as the positive-finding convenience projection. A clean verdict requires
    `TypestateAnalysis::is_complete()`.

## Rejected alternatives

### Hand-roll a language parser

Rejected. The frontend already owns grammar recovery and emits typed AST, CFG,
DFG, and call evidence. A source-text parser in the analyzer would disagree
with frontends and could misread comments, strings, macros, or malformed code.

### Key objects by variable spelling

Rejected. Names are shadowable and aliases can change spelling. Allocation-site
identity is stable, distinct, and backed by CPG nodes; DFG/reference evidence
connects uses to that identity.

### Mutate typestate facts into the CPG

Rejected. Facts are policy-dependent derived evidence, not source topology.
Out-of-graph reports keep repeated policies independent and preserve CPG node
and edge counts.

### Treat missing evidence as a clean result

Rejected. A missing CFG point, unresolved event, invalid AST parent relation,
or exhausted cap invalidates a negative claim. The report records the exact
boundary.

### Use recursive path exploration

Rejected. CPGs can be adversarially deep. Ordered heap worklists provide the
same finite fixed point without consuming native stack per graph node.

### Require a general interprocedural IFDS domain for the first property engine

Rejected for this scope. Allocation identity and property transfer are local to
the represented function, while typed DFG/call-target evidence already locates
events. The fact lattice is finite and intraprocedural; forcing procedure
summaries would add a second semantics without improving this contract.

## Consequences

### Positive

- Policies are language-neutral, feature-free, deterministic, serializable
  under `serde`, and auditable.
- Allocation identity, alias flow, branch joins, terminal states, and witnesses
  remain explicit.
- Every partial boundary is machine-readable; empty findings cannot silently
  mean success.
- The implementation crosses a CFG depth of 100,000 on a 256 KiB native stack.

### Negative

- The analysis is a may-analysis and can report an infeasible branch unless a
  preceding path-sensitive analysis removes it.
- Heap, field, container, interprocedural-return, and dynamic-dispatch precision
  are limited to represented DFG/reference/call evidence.
- Allocation-site abstraction merges multiple runtime instances created by the
  same call inside a loop.
- Accepting terminal states are summarized but not automatically reported as
  violations; clients choose leak policy explicitly.

## Verification

Example tests cover legal and forbidden sequences, absorbing error behavior,
branch joins, aliases, represented call targets, malformed evidence, every
applicable cap, validation invariants, immutability, and serialization. Three
256-case properties compare linear programs with an independent oracle, reverse
CFG/DFG insertion order, and check work-budget monotonicity. Public integration
exercises hand-built and parser-built CPGs plus the 100,000-node small-stack
gate.

## References

1. Strom, R. E., Yemini, S. (1986). *Typestate: A Programming Language Concept
   for Enhancing Software Reliability.* IEEE Transactions on Software
   Engineering. DOI:
   [10.1109/TSE.1986.6312929](https://doi.org/10.1109/TSE.1986.6312929).
2. Fink, S. J., Yahav, E., Dor, N., Ramalingam, G., Geay, E. (2008). *Effective
   Typestate Verification in the Presence of Aliasing.* ACM Transactions on
   Software Engineering and Methodology. DOI:
   [10.1145/1348250.1348255](https://doi.org/10.1145/1348250.1348255).
