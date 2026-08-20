# Security boundary for API-usage protocol mining

API-usage and temporal findings can reveal missing cleanup, invalid lifecycle
transitions, and anomalous call contexts. They are advisory evidence, not
security verdicts or authorized patches. This page defines the trust boundary
for `analyze_api_usage_mining`.

## 1. Actors and assets

| Actor or asset | Security role |
|---|---|
| untrusted source corpus | controls graph size, syntax shape, call density, and apparent peer conventions |
| language frontend | decides which AST/CFG/DFG/PDG facts are represented |
| analysis caller | supplies support/population policy, caps, and named temporal FSMs |
| libcpg analyzer | validates, bounds, canonicalizes, and qualifies represented evidence |
| reviewer | decides whether a convention/property applies and whether a finding has impact |
| input CPG | integrity asset; the advisory pipeline must not mutate it |

## 2. Threats and controls

| Threat | Control | Residual risk |
|---|---|---|
| corpus poisoning makes an unsafe usage dominant | exact support and supporting sites are exposed; findings remain advisory | a carefully poisoned complete population can still dominate |
| incomplete sites bias support | any missing owner/identity/CFG/capped graph is excluded and recorded; global inventory overflow fails atomically | frontend omissions unknown to the CPG cannot be reported |
| call identity conflates APIs | resolved targets or bounded typed callee paths only | dynamic dispatch/reflection may remain unresolved |
| path-insensitive order overclaims obligation | `Order` is documented as may-reachability | reviewer must distinguish may-follow from must-follow |
| adversarial graph exhausts memory/time | configurable caps plus compile-time hard ceilings | permitted work can still be expensive |
| deep graph exhausts native stack | heap queues, flat vectors, ordered heap collections, typestate worklist | heap exhaustion remains possible within configured limits |
| malicious FSM creates ambiguity | unique non-empty names and validated deterministic property FSMs | caller can intentionally specify the wrong policy |
| empty vector is treated as proof of safety | full report carries issues/delegated analyses; `is_complete()` is required | convenience API can be misused by an incautious consumer |
| report changes the analyzed program | every path is read-only and integration tests pin node/edge counts | later external tooling could still apply an unsafe repair |

## 3. Positive and negative claims

A positive usage finding means:

> Within one complete represented API population, this complete site lacks or
> differs from a fragment whose exact support met the configured strict-majority
> threshold.

A positive temporal finding means:

> Under this caller-supplied finite-state property and represented object/event
> flow, this abstract allocation encounters a forbidden transition or reaches a
> represented exit in a non-accepting state.

A negative claim is permitted only when:

1. `ApiUsageMiningAnalysis::is_complete()` is true;
2. the caller accepts the population boundary and FSM as security policy;
3. the frontend/call-resolution coverage is appropriate for the target;
4. the relevant finding collection is empty.

Even then, the conclusion is relative to represented evidence, not a proof of
all runtime executions.

## 4. Population-poisoning analysis

Let $`P`$ be the complete occurrences of an anchor API and $`S\subseteq P`$ the
sites carrying one fragment. Mining accepts when:

```math
\frac{|S|}{|P|} \geq \tau
\quad\text{with}\quad
\tau > \frac{1}{2}
```

Exact arithmetic prevents rounding attacks, and strict majority prevents tied
contradictory single-valued expectations. Neither control prevents an attacker
from contributing enough internally complete poisoned peers. Security-sensitive
automation must therefore authenticate and scope the source corpus; libcpg
exposes supporting site identifiers so a reviewer can audit provenance.

## 5. Temporal-policy integrity

Named temporal properties are trusted configuration. Before analysis:

- version-control the FSM beside the security policy it enforces;
- ensure allocation/event identities match the frontend's stable API identities;
- document accepting states and why every omitted transition is forbidden;
- set `IfdsCaps` high enough for the intended program, then require completeness;
- treat allocation-site abstraction as may-alias evidence, especially in loops.

The analysis does not synthesize temporal policy from population frequency.
This separation prevents a popular misuse from redefining an explicit security
obligation.

## 6. Resource hardening

Never lift all caps in response to an issue. Choose bounds from an authenticated
corpus profile and preserve hard ceilings. `CallSiteCap` is intentionally
all-or-nothing; per-site CFG/AUG caps are intentionally site-local. Delegated
typestate caps are retained in each temporal analysis rather than flattened
into a generic success/failure bit.

The 100,000-node small-stack test validates native-stack independence, not
unbounded computation. Process-level memory and time limits remain appropriate
for hostile repositories.

## 7. Automated remediation is outside the trust boundary

ASAP-Repair studies transformation from API-usage templates, and ProveNFix
studies temporal-property-guided repair. libcpg implements neither repair
system. It provides immutable evidence suitable for review or for a separately
validated consumer. Before any external consumer edits code, it must establish
path feasibility, API contracts, ownership semantics, side effects, formatting,
compilation, tests, and rollback.

![Security-relevant evidence remains qualified through the complete pipeline](../diagrams/api-usage-temporal-pipeline.svg)

See the [component contract](../components/graph/api-usage-protocol-mining.md),
[Theory 37](../theory/37-api-usage-graphs-and-temporal-properties.md), and the
[validation ledger](../scientific/42-api-usage-temporal-validation.md).

## References

1. Nielebock, S., Blockhaus, P., Krüger, J., Ortmeier, F. (2024).
   *ASAP-Repair: API-Specific Automated Program Repair Based on API Usage
   Graphs.* DOI:
   [10.1145/3643788.3648011](https://doi.org/10.1145/3643788.3648011).
2. Song, Y., Gao, X., Li, W., Chin, W.-N., Roychoudhury, A. (2024).
   *ProveNFix: Temporal Property-Guided Program Repair.* DOI:
   [10.1145/3643737](https://doi.org/10.1145/3643737).
