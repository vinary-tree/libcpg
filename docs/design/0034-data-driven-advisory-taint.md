# ADR-0034: Data-driven advisory taint and null-flow summaries

- **Status:** Accepted
- **Date:** 2026-08-13
- **Scope:** feature-free `analysis::taint`

## Context

Security analysis needs to recognize sources, sinks, sanitizers, opaque APIs,
and interprocedural laundering. Hard-coding a closed language-specific enum in
the transfer engine would force libcpg to change whenever a consumer adds an
API or language. Parsing source again would violate the typed CPG evidence
boundary and disagree with caller-supplied Mode-B trees.

T3.2 supplies a deterministic, bounded summary scheduler over call strongly
connected components. T3.3 needs a finite domain that preserves pgmcp's
defensive taint scenarios, supports recursion, returns concrete witnesses, and
never converts incomplete evidence into a security verdict.

## Decision

1. Separate policy from transfer with `TaintCatalog`. Source and sink classes
   are open `Arc<str>` values.
2. Restrict matching to `Exact`, `Segment`, `Suffix`, and `Substring` over a
   callee path reconstructed from typed `Identifier` and `MemberAccess` nodes.
3. Use no source parser and no regular expression in the analysis.
4. Give opaque calls explicit `Passthrough`, `Havoc`, `Source`, `Sink`, or
   `Sanitizer` models; default to low-confidence passthrough.
5. Represent local facts as formal parameters or ambient source classes, while
   retaining concrete ambient origins internally.
6. Materialize parameter-to-sink, parameter-to-return, and source-to-return
   relations in a lawful finite `TaintSummary` lattice.
7. Compose summaries through the existing validated call-SCC solver.
8. Retain one deterministic witness per semantic key, preferring uncapped,
   stronger, shorter, then lexicographically smaller evidence.
9. Expose report APIs carrying summaries, caps, issues, and `is_complete()`;
   retain vector-only functions as documented convenience projections.
10. Implement maybe-null as a separate summary domain over typed null origins
    and dereference evidence, not as magic taint catalog strings.
11. Emit no graph mutation and label every finding advisory.

## Rejected alternatives

### Reparse source or hand-roll a language parser

Rejected. libcpg already has typed AST nodes, call resolution, and DFG edges.
A second parser would create inconsistent evidence, bind the analysis to
surface grammars, and violate the shared-overlay architecture.

### Closed source and sink enums

Rejected. Closed enums made pgmcp's original catalog easy to exhaustively match
but prevented downstream projects from naming domain-specific classes without
forking code.

### Regular-expression catalog

Rejected for the feature-free core. Literal path patterns cover the ported
cross-language rules, have transparent cost and matching semantics, and avoid
an unnecessary policy execution surface.

### Treat every unknown call as a sink

Rejected because it fabricates vulnerabilities. Default passthrough preserves
possible wrapper flow without inventing dangerous consumption.

### Treat every unknown call result as null

Rejected because absence of type evidence is not positive nullable evidence.
Null-return sources require a represented nullable type or a callee summary.

### Return only finding vectors

Rejected as the sole interface because an empty capped vector is ambiguous.
The report types preserve completeness evidence; vector projections remain for
ergonomic positive-flow consumers.

## Consequences

### Positive

- One catalog can analyze any language frontend that emits equivalent typed
  CPG evidence.
- Consumers can replace or extend policy without changing graph topology.
- Recursive, helper-laundered, parameter-return, and source-return flows share
  one bounded fixed-point mechanism.
- Findings retain CWE, confidence, endpoint, parameter-position, and canonical
  witness evidence.
- Null-flow reuses the interprocedural algebra without conflating nullness and
  injection policy.

### Negative

- Precision is limited by represented def-use, call, type, alias, and path
  evidence.
- Literal patterns cannot express every possible policy language; consumers
  must add several rules when one broad expression would otherwise suffice.
- `u64` parameter-to-return masks require explicit truncation handling after 64
  formals.
- Default opaque passthrough improves recall but carries deliberately low
  confidence.
- A catalog sanitizer is trusted policy, not proof that the called operation is
  semantically safe.

## Verification

The implementation ports twelve pgmcp scenarios, including direct flow,
helper laundering, safe constants, sanitizer killing, all three relevant
external-call behaviors, self and mutual recursion, parameter return, and
source return. Generated properties compare summary results with independent
brute-force reachability, exercise lattice laws and insertion permutation, and
validate every emitted witness against typed transfer evidence. Public tests
cover typed member-path reconstruction, catalog replacement, null-flow, and cap
visibility. The `ifds_taint` benchmark asserts an exact source-to-sink oracle
before timing path, ring, and clustered call graphs.

## References

1. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014), DOI
   [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44).
2. Reps, T., Horwitz, S., Sagiv, M. (1995), DOI
   [10.1145/199448.199462](https://doi.org/10.1145/199448.199462).
3. Sagiv, M., Reps, T., Horwitz, S. (1996), DOI
   [10.1016/0304-3975(96)00072-2](https://doi.org/10.1016/0304-3975(96)00072-2).
