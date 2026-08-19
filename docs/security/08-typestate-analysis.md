# Security boundary for typestate analysis

Invalid resource protocols can contribute to use-after-close, double release,
missing initialization, or lock misuse. `analyze_typestate` is nevertheless an
advisory static evidence engine. A finding is not automatically an exploitable
vulnerability, and an empty report is not automatically proof of safety.

![Policy, untrusted CPG evidence, bounded propagation, and qualified findings](../diagrams/typestate-analysis.svg)

## 1. Assets and actors

| Actor or asset | Security role |
|---|---|
| analyzed repository | untrusted input that may be malformed, huge, cyclic, or incomplete |
| language frontend | supplies typed AST, CFG, DFG, reference, and call evidence |
| policy author | defines security-relevant states, events, and forbidden transitions |
| typestate analyzer | reads evidence and returns immutable candidates and qualifiers |
| downstream policy | decides whether findings, incompleteness, or terminal states block release |
| reviewer | confirms path feasibility, object identity, ownership, and impact |

## 2. Threats and controls

| Threat | Control | Residual risk |
|---|---|---|
| comments or strings mimic protected calls | events come from typed call nodes and callee/target evidence, never text parsing | faulty frontend mapping can still misclassify a node |
| shadowed names conflate resources | object identity is the allocation-site `NodeId`; uses require typed flow evidence | allocation-site abstraction merges loop instances |
| alias hides a protected object | alias, DFG, reference, and definition relations join object evidence | missing or imprecise flow edges cause false negatives or positives |
| malformed property weakens enforcement | complete closed-vocabulary and absorbing-error validation runs first | a valid but semantically inappropriate policy remains the author's responsibility |
| malformed AST/CFG causes nontermination | visited sets, flat projections, and heap worklists terminate cycles | very large valid inputs still consume bounded heap/time |
| deep CFG exhausts native stack | graph-depth traversal and witness reconstruction are iterative | unrelated frontend/build passes have their own stack-safety contracts |
| resource exhaustion suppresses findings silently | every applicable `IfdsCaps` boundary sets `capped` with typed evidence | callers may ignore the qualifier |
| unresolved or ambiguous event is treated as irrelevant | report issues make `is_complete()` false | external behavior not represented in the CPG remains unknown |
| infeasible CFG branch is called a vulnerability | API and docs call findings advisory may-evidence | consumers can still overstate a candidate |
| analysis mutates the program graph | report is out of graph and tests assert node/edge immutability | cached reports can become stale after later graph mutation |
| terminal non-acceptance is confused with forbidden transition | terminal states and violation findings are separate surfaces | a caller that never checks acceptance may miss leak policy |

## 3. Safe consumption

1. Call `PropertyFsm::validate` during policy loading and reject every issue.
   `analyze_typestate` repeats this check defensively.
2. Use `analyze_typestate`, not only `check_typestate`, for enforcement. Require
   `is_complete()` before accepting an empty findings vector.
3. Treat `capped`, `cap_events`, and `issues` as fail-closed policy inputs; show
   exact sites and limits to reviewers.
4. Present the allocation, violating site, prior state, event, and full witness
   together. Do not reduce a finding to a call name.
5. Check `terminal_states` or `object_is_accepting` separately when the policy
   requires release/closure before function exit.
6. Confirm CFG path feasibility and runtime alias/ownership semantics with
   complementary analyses or manual review.
7. Recompute after changing the CPG, frontend configuration, resolver, property,
   or caps.
8. Assign a vulnerability class only after application-specific reachability,
   trust-boundary, lifetime, and impact review.

## 4. False positives and false negatives

May-union retains all represented branch states, including infeasible paths not
eliminated by other analyses. Imprecise alias edges can also join objects. These
can produce false positives. Missing calls, CFG edges, DFG edges, external
effects, field/container flow, or dynamic targets can hide violations. Explicit
issues cover observable evidence gaps but cannot enumerate behavior absent from
the CPG entirely.

## 5. Non-goals

The analyzer does not prove memory safety, ownership, linearity, temporal logic,
path feasibility, race freedom, absence of leaks, whole-program alias precision,
runtime object identity, exploitability, or a Common Weakness Enumeration (CWE)
classification.

## References

1. Fink, S. J., Yahav, E., Dor, N., Ramalingam, G., Geay, E. (2008). *Effective
   Typestate Verification in the Presence of Aliasing.* DOI:
   [10.1145/1348250.1348255](https://doi.org/10.1145/1348250.1348255).
