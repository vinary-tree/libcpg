# Security boundary for population anomaly detectors

Missing guards, unchecked returns, and omitted follow-up calls can contribute
to authorization bypass, validation failures, corrupted state, denial of
service, or unsafe cleanup. libcpg reports population-supported deviations,
not vulnerabilities. Security policy must preserve evidence completeness and
separate statistical convention from application correctness.

![Untrusted graph evidence becomes qualified advisory findings](../diagrams/population-anomaly-pipeline.svg)

## 1. Assets and actors

| Actor or asset | Security role |
|---|---|
| analyzed repository/CPG | untrusted, potentially huge, cyclic, incomplete, or population-poisoned evidence |
| frontend and resolver | trusted normalization boundary for AST/CFG/DFG/PDG/call/type evidence |
| catalog author | selects security-sensitive APIs and semantic classes |
| policy author | selects peers, minimum population, exact support, and traversal budgets |
| detector | bounded read-only transformer from represented populations to evidence |
| reviewer | decides semantic comparability, path feasibility, convention correctness, and impact |
| downstream enforcement | must preserve issues and reject unqualified negative claims |

## 2. Threats and controls

| Threat | Control | Residual risk |
|---|---|---|
| comments or strings imitate guards/calls | typed nodes and edges only; no source-text parser in the detector | faulty frontends can omit or misclassify evidence |
| same spelling conflates different APIs | represented sorted target sets are authoritative | unresolved external overloads can share a named path |
| local variable names fragment equivalent checks | fingerprint abstracts local identifier/declaration spelling | abstraction can merge guards whose hidden semantics differ |
| semantically different checks merge | operators, literals, member/API/type/import/macro/opaque kinds are retained | alias, path, and language semantics remain limited by the CPG |
| one site votes repeatedly | per-site check sets deduplicate fingerprints | frontend-duplicated call sites still appear as distinct occurrences |
| threshold rounding flips a result | exact integers and `u128` cross multiplication | policy can still choose an inappropriate threshold |
| incomplete evidence biases a fraction | incomplete occurrence is excluded and a typed issue makes the report incomplete | behavior entirely absent from the CPG cannot produce an issue |
| unrelated contexts dominate peers | call-graph-radius scope can narrow the population | structural proximity does not prove semantic comparability |
| common bug becomes a belief | findings remain advisory with exact population evidence | a poisoned or uniformly defective population can suppress anomalies |
| rare correct behavior is labeled deviant | evidence includes support and sites for review | rarity is not incorrectness |
| reverse value relation fabricates a return check | `UseDef` is excluded from forward return evidence | imprecise forward DFG/PDG edges can still over-approximate |
| huge/cyclic input exhausts resources or native stack | visited sets, explicit node/depth caps, heap worklists, flat tapes | allowed work can still consume substantial heap/CPU |
| cap exhaustion looks clean | typed issues set `is_complete()` false | consumers can discard the report qualifier |
| serialized typed-key evidence is reordered or duplicated | canonical sequence encoding and duplicate-key rejection | transport/storage integrity remains caller responsibility |
| analysis changes shared topology | APIs are read-only; integration checks graph counts | cached reports become stale after caller mutation |

## 3. Population poisoning and common-mode failure

An attacker or accidental code generator can add many similar occurrences to
raise or lower support. More fundamentally, a convention followed everywhere
may be uniformly insecure. Exact fractions make this evidence auditable but do
not make it causal. For security enforcement:

1. choose populations from a reviewed scope and stable baseline;
2. version catalog and threshold policy alongside results;
3. display numerator, denominator, peers, and concrete guard/support witnesses;
4. investigate abrupt population changes as well as individual findings; and
5. combine anomaly evidence with specifications, taint, typestate, tests, or
   manual review rather than treating frequency as a security oracle.

## 4. Safe consumption

1. Validate catalog patterns and sensitive classes against the target API
   version and resolver behavior.
2. Use `analyze_missing_checks` and `analyze_deviant_behavior` for decisions;
   require `is_complete()` before interpreting an empty result.
3. Treat every issue as fail-closed input to enforcement and show it to the
   reviewer with its site and exact cap where present.
4. Confirm that peers are semantically comparable and that the expected check,
   return guard, or later call applies to the anomalous site.
5. Confirm runtime path feasibility, aliases, external contracts, implicit
   exceptions, and dynamic dispatch before assigning impact.
6. Recompute after any graph, frontend, resolver, catalog, peer policy,
   threshold, or cap change.
7. Preserve exact fractions; use `ratio()` only for display.
8. Assign a CWE only after application-specific trust-boundary and impact
   review. A catalog rule's CWE metadata classifies policy intent, not proof.

## 5. Resource and stack controls

`max_callee_depth`, `max_check_nodes`, `max_cfg_nodes_per_call`, and
`max_return_evidence_nodes` bound input-shaped traversals. Minimum population
limits inference, not resource consumption. Hostile repositories should also
be constrained by caller-level graph-size, time, and memory isolation.

No input-sized operation uses native recursion. Check postorder frames,
call-neighborhood queues, CFG/value queues, and visited sets live on the heap.
Tests cross 100,000 represented nodes on a 256 KiB native stack. This controls
call-stack risk but does not promise constant memory.

## 6. Non-goals

The detectors do not prove authorization, validation sufficiency, return-code
meaning, sequencing requirements, path feasibility, peer equivalence,
population integrity, external-library behavior, exploitability, or absence of
missing-check and deviant-behavior defects.

## References

1. Yamaguchi, F., Wressnegger, C., Gascon, H., Rieck, K. (2013). *Chucky:
   Exposing Missing Checks in Source Code for Vulnerability Discovery.* DOI:
   [10.1145/2508859.2516665](https://doi.org/10.1145/2508859.2516665).
2. Lu, K., Pakki, A., Wu, Q. (2019). *Detecting Missing-Check Bugs via
   Semantic- and Context-Aware Criticalness and Constraints Inferences.*
   [USENIX Security 2019](https://www.usenix.org/conference/usenixsecurity19/presentation/lu).
3. Engler, D., Chen, D. Y., Hallem, S., Chou, A., Chelf, B. (2001). *Bugs as
   Deviant Behavior: A General Approach to Inferring Errors in Systems Code.*
   DOI: [10.1145/502034.502041](https://doi.org/10.1145/502034.502041).
