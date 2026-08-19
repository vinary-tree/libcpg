# Scientific validation 40 — Population anomaly detectors

This ledger validates I6 missing-check and I7 deviant-behavior reports against
independent exact population oracles. It tests deterministic consequences of
represented graph evidence, not whether a population convention is correct or
a deviation exploitable.

![Two exact population fixtures and their expected deviations](../diagrams/population-support-oracles.svg)

## 1. Hypotheses

| ID | Hypothesis | Falsifying observation |
|---|---|---|
| H1 | four equivalent guarded sensitive sites and one unguarded site infer exactly one missing check at support `4/5` | another site/count/fraction is reported, or the anomaly is missed |
| H2 | local identifier renaming preserves a check fingerprint while semantic operators, literals, members, APIs, and types distinguish it | spelling fragments peers or semantic changes merge |
| H3 | call-neighborhood scope admits exactly same-API sites whose owner functions lie within the configured undirected radius | an outside peer votes or an inside peer disappears |
| H4 | a supported later API call induces a call-order belief and only predecessor occurrences lacking reachable successors deviate | belief or finding disagrees with independent CFG reachability |
| H5 | nine checked results and one unchecked result infer exactly one return-check deviation at support `9/10` | reverse DFG, a non-guard use, or the unchecked call becomes support |
| H6 | thresholds are exact rational decisions | any boundary differs from integer cross multiplication |
| H7 | incomplete evidence is excluded and visibly prevents a clean negative claim | capped/missing/cyclic evidence votes or `is_complete()` remains true |
| H8 | equivalent relation insertion and repeated execution produce equal ordered reports | report ordering or values change |
| H9 | JSON round trips preserve typed-key evidence maps and reject duplicate keys | encoding fails, changes values, or silently accepts a duplicate |
| H10 | analyzers are read-only | node or edge counts change |
| H11 | represented depth does not consume proportional native stack | a 100,000-node fixture overflows a 256 KiB worker stack |

## 2. Independent missing-check oracle

For generated population size $`p`$ and anomaly count $`q`$, the fixture adds
the same normalized guard to the first $`p-q`$ sensitive sites and no guard to
the last $`q`$. The test computes support directly as
$`(p-q)/p`$, compares it to the generated exact policy through independent
integer cross multiplication, and predicts either the exact anomalous suffix
or no inferred fingerprint.

The oracle does not call production fingerprint collection, peer selection, or
threshold code. A separate insertion-order input reverses admissible relations;
both reports must remain equal.

## 3. Independent deviant-behavior oracle

Generated checked/unchecked populations connect selected call results forward
to represented guards. The oracle counts selected Booleans and predicts a
return-check belief exactly when the count and minimum-population policy pass.
Predicted findings are the unchecked site identifiers in canonical order.

Call-order examples use a separately enumerated CFG reachability set. They pin
reachable nonadjacent successors, branches without a successor, calls outside
the CFG, and traversal-cap boundaries without reusing the production walk.

## 4. Example matrix

| Family | Examples |
|---|---|
| sensitive admission | exact catalog sink, selected/all classes, unrelated APIs, represented versus named identity |
| check evidence | direct/ancestor PDG controllers, call-as-guard, identifier abstraction, semantic distinctions, duplicate guards |
| peer topology | whole program, zero/one/two call-graph radius, shared callee connection, minimum population |
| missing qualification | missing/capped identity, call outside function, cyclic/missing/oversized/arity check evidence |
| call order | normal successor, one missing successor, branch reachability, exact support, call absent from CFG |
| return check | forward DFG/PDG to guard, non-guard use, reverse `UseDef` exclusion, exact 9/10 oracle |
| deviant qualification | identity, CFG, value-evidence and traversal cap issues; invalid options |
| common contracts | determinism, graph immutability, positive projections, exact ratios, serde round trips |
| public/frontends | combined crate-root API and parsed Rust PDG 4/5 oracle |
| stack safety | 100,000-node check and CFG paths on 256 KiB |

## 5. Metamorphic properties

- Reordering equivalent nodes/relations does not change canonical reports.
- Renaming local guard identifiers does not change their fingerprint.
- Adding an unrelated API population does not change another API's beliefs.
- Repeating an equivalent guard at one site does not give that site two votes.
- Increasing the anomaly count changes the exact numerator predictably and can
  only cross a fixed threshold from accepted to rejected.
- Analyzer execution leaves graph counts and input node payloads unchanged.
- Serde encode/decode preserves exact fractions and ordered typed-key maps.

## 6. Stack-safety experiment

One worker with a 256 KiB native stack normalizes a 100,000-node guard AST.
Another traverses a 100,000-node CFG/value-evidence path. Success requires the
expected report, no panic or overflow, and clean teardown. These experiments
exercise explicit postorder frames, `VecDeque` frontiers, visited sets, report
ordering, and flat fingerprint destruction. They support native-stack use
independent of represented depth; heap and total-work bounds remain explicit.

## 7. Executed gates

| Gate | Evidence |
|---|---|
| feature-free unit/property | both detector modules, exact oracles, issues, policies, determinism, depth |
| public integration | simultaneous 4/5 and 9/10 findings without graph mutation |
| parser integration | Rust frontend plus PDG construction identifies only the fifth unguarded caller |
| all-feature unit/integration | feature interactions and parser surface |
| serde regressions | population reports plus shared taint, null-flow, and typestate typed-key maps |
| strict Clippy | all targets under no-default and all-feature configurations with warnings denied |
| rustdoc/docs | public API docs, internal links, diagrams, snippets, spelling, and citations |

No dedicated timing claim belongs to this ledger. The acceptance target is
semantic, determinism, integration, and stack-safety evidence; asymptotic bounds
are documented in the component and engineering performance guides.

## 8. Interpretation limits

Passing these tests establishes conformance to the finite represented-evidence
model. It does not establish catalog suitability, semantic equivalence of all
peers, complete dynamic dispatch, path feasibility, external behavior,
correctness of the dominant convention, exploitability, or defect absence.

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
