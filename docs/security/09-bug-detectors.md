# Security boundary for advisory bug detectors

Resource leaks, reads without represented initialization, and uncaught
exceptions can contribute to availability failures, corrupted state, denial of
service, or unsafe cleanup. libcpg reports static candidates, not vulnerability
verdicts. Security policy must distinguish positive evidence, incomplete
evidence, and runtime impact.

Missing-check and deviant-behavior populations have a separate
[security boundary](10-population-anomaly-detectors.md), including population
poisoning and peer-policy trust.
Clone-consistency and path-feasibility advisories have a separate
[security boundary](11-clone-feasibility-detectors.md), including pair/guard
trust, SCCP/interval limits, and remediation controls.
Centered API-usage and named temporal findings have a separate
[security boundary](12-api-usage-protocol-mining.md), including population/FSM
trust, exact support, resource caps, completeness, and repair separation.

![Untrusted graph evidence, bounded analysis, and qualified detector reports](../diagrams/bug-detector-pipeline.svg)

## 1. Assets and actors

| Actor or asset | Security role |
|---|---|
| analyzed repository | untrusted source that may be malformed, huge, cyclic, or intentionally misleading |
| language frontend | trusted normalization boundary for typed AST/CFG/DFG/type/call evidence |
| resource-policy author | defines acquisitions, releases, legal transitions, and accepting terminal states |
| detector | read-only bounded transformer from represented evidence to candidates |
| downstream policy | decides whether findings or incompleteness block release |
| reviewer | confirms path feasibility, ownership, initialization semantics, exception contracts, and impact |

## 2. Threats and controls

| Threat | Control | Residual risk |
|---|---|---|
| comments/strings imitate allocation, release, throw, or catch syntax | detectors consume typed nodes and edges, never source-text matching | a faulty or unsupported frontend can omit/misclassify evidence |
| shadowed names conflate resources | typestate keys objects by allocation call site and follows typed flow evidence | one allocation site can represent many loop instances |
| a CFG evaluation side sink is mistaken for function exit | builder-recorded and explicit return/throw exits are authoritative; structural sinks are legacy fallback only | malformed manual graphs with no semantic exits remain less precise |
| attacker supplies a policy with no release success | `NoAcceptingState` prevents leak projection and completeness | a valid but inappropriate accepting policy remains caller responsibility |
| declaration or method spelling triggers an uninitialized false positive | binder-aware facts exclude unbound signature and callee syntax | incomplete binding evidence can still misclassify executable identifiers |
| an outer definition hides a missing local initialization | only definitions in the current function scope initialize; outside definitions remain visible evidence | cross-language closure/capture semantics require accurate scope normalization |
| exception type is absent | retain `Unknown` and `UnknownThrowType`; only catch-all handles it | missing type evidence can hide typed handler compatibility |
| polymorphic call has multiple targets | `AmbiguousCall` prevents a clean result | the finite analysis does not union ambiguous dynamic targets |
| malformed catch topology suppresses handler reasoning | `MissingCatchEdge` exposes absent typed handler entry | external or implicit runtime handlers remain unrepresented |
| cyclic inheritance or call graph causes nontermination/overflow | visited sets, SCC condensation, finite joins, and heap worklists | large valid inputs consume bounded heap/time |
| cap exhaustion looks clean | typed cap events set `capped`; report `is_complete()` becomes false | consumers can ignore the qualifier |
| positive candidate is presented as an exploitable flaw | APIs and docs label findings advisory and retain node witnesses | downstream tooling can overstate evidence |
| analysis mutates topology used by another check | all reports are out of graph; integration asserts node/edge immutability | cached reports become stale if a caller later mutates the CPG |

## 3. Safe consumption

1. Validate resource policies and require at least one accepting terminal state.
2. Use report APIs for decisions. Require `is_complete()` before treating an
   empty leak or exception finding set as absence evidence.
3. Treat every cap and issue as fail-closed input to enforcement; display the
   exact typed boundary to reviewers.
4. Present each node witness with the finding. Do not reduce a leak to a method
   name, a use to a spelling, or an exception to a type alone.
5. Confirm that allocation/release policy matches the library's ownership
   contract and that represented exits correspond to actual cleanup duties.
6. Confirm language-specific initialization, capture, default-value, and field
   semantics before classifying an uninitialized use.
7. Confirm handler ordering, runtime type hierarchy, dynamic targets, implicit
   exceptions, and external contracts before classifying exception impact.
8. Recompute reports after any graph, parser, resolver, policy, or cap change.
9. Assign a Common Weakness Enumeration (CWE) category only after application-
   specific reachability, trust-boundary, ownership, and impact review.

## 4. False positives and false negatives

May-union can retain infeasible leak branches. Conservative alias evidence can
merge resource flows. An outer function definition deliberately does not
initialize a local use, but a frontend that fails to model a language capture
may produce a false positive. Lexical exception summaries can over-approximate
which call and handler paths execute.

Missing allocations, releases, definitions, calls, implicit throws, types,
inheritance edges, external summaries, or CFG paths can cause false negatives.
Observable gaps become report issues where the API can identify them; behavior
entirely absent from the CPG cannot be enumerated.

## 5. Resource controls

Resource leaks inherit typestate's function, fact, work, and callee-path caps.
Exception summaries inherit function, fact, work, and recursive-SCC iteration
caps. Uninitialized-use detection is finite over represented functions, uses,
and reaching definitions and uses ordered heap collections for scope and
binding queries.

The implementation maps no input-sized traversal to native recursion. Public
integration crosses 100,000 AST levels on a 256 KiB stack. This protects native
stack depth; `IfdsCaps`, input-size policy, and caller time/memory isolation
remain necessary for hostile large inputs.

## 6. Non-goals

The detectors do not prove memory safety, resource ownership, path feasibility,
definite runtime initialization, exception safety, external-library behavior,
dynamic-dispatch completeness, exploitability, race freedom, or absence of all
resource/definition/exception defects.

## References

1. Shadab, N., Gharat, P., Ernst, M. D., Kellogg, M., Lahiri, S. K., Lal, A.,
   Sridharan, M. (2025). *Lightweight and Modular Resource Leak Checking
   (Extended Version).* DOI:
   [10.1007/s10009-025-00804-2](https://doi.org/10.1007/s10009-025-00804-2).
2. Sinha, S., Harrold, M. J. (2000). *Analysis and Testing of Programs with
   Exception Handling Constructs.* DOI:
   [10.1109/32.877846](https://doi.org/10.1109/32.877846).
