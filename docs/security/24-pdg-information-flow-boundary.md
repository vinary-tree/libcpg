# PDG information-flow and constant-time security boundary

The feature-free IFC APIs expose exact reachability evidence over represented
program/system dependence. They are security review tools, not authority to
claim that a source program or deployed binary is non-interfering or
constant-time.

## Assets and trust boundaries

| Input or output | Trusted property | Untrusted or caller-owned property |
|---|---|---|
| CPG nodes and dependence edges | Stable identities and exact stored edge kinds/labels | Frontend completeness, aliases, dynamic behavior, libraries, native code |
| `SecurityLabels` | Exact ordered node sets supplied to the API | Whether the classification and declassification policy is appropriate |
| SDG summary edge | Exact caller-local labeled relation represented in the CPG | Completeness of callee effects and call resolution |
| `FlowViolation` / `LeakSite` | Canonical replay of admitted graph evidence | Runtime feasibility, exploitability, severity, remediation |
| Empty result | No matching path/site in the admitted finite relation | Whole-language or machine-level security |

![Explicit, implicit, declassified, branch-timing, and index-timing paths share one represented influence relation but have different interpretations.](../diagrams/ifc-policy-paths.svg)

*Figure -- confidentiality and timing interpretation boundaries. Source:
[`diagrams/ifc-policy-paths.puml`](../diagrams/ifc-policy-paths.puml).*

## Threats addressed

- direct represented flow from a confidential source to a public observation;
- implicit represented flow through a control-dependent operation;
- secret influence reaching true/false conditional CFG edges;
- secret influence reaching an array or collection index node;
- accidental continuation through a caller-declared declassification point;
- nondeterministic or non-replayable evidence that frustrates audit.

## Deliberate non-claims

The analysis does not prove path feasibility, alias completeness, dynamic-call
completeness, exception semantics, concurrency semantics, speculative safety,
cache-obliviousness, instruction-level timing, or hardware constant time. It
does not infer labels or declassifiers from names. It does not sanitize,
rewrite, suppress, or prioritize findings.

Control dependence may over-approximate runtime-feasible execution and create
advisory positives. Missing dependence edges may omit a real flow. A consumer
must preserve that direction of uncertainty instead of turning an empty vector
into a certification.

## Declassification is not timing release

`SecurityLabels::declassifiers` authorizes a confidentiality release by cutting
the admitted High-to-Low relation at a node. The cut prevents a witness from
entering the release and resuming beyond it. It does not erase the node or its
edges from the CPG.

`constant_time` intentionally has no declassifier parameter. A value may be
authorized for eventual output while still leaking unauthorized intermediate
information through branch choice or memory address. Conflating those policies
would hide timing evidence.

## Resource and denial-of-service posture

The analysis is finite but deliberately uncapped at the result boundary. It
returns every represented source/sink pair and timing mechanism, so cap
exhaustion cannot masquerade as a clean result. A hostile caller can still
supply a large graph, many sources, and long witnesses. Admission, process RSS,
execution time, and response size must therefore be bounded outside the API.

All depth-shaped state is heap resident. A deep graph cannot overflow the
native stack through this component, but ordinary heap exhaustion remains a
resource failure and must not be interpreted as successful analysis.

## Safe consumption checklist

1. Build the required PDG and exact SDG summaries before analysis.
2. Derive labels from an explicit, reviewed policy.
3. Treat every positive as evidence requiring contextual review.
4. Treat zero findings as represented-model absence only.
5. Preserve source, sink/site, kind, and complete witness in downstream data.
6. Do not apply confidentiality declassification to timing findings.
7. Bound graph admission and presentation without silently dropping analysis
   failures or pretending a truncated response is exhaustive.

See the [component contract](../components/graph/information-flow-control.md),
[Theory 49](../theory/49-pdg-information-flow-and-constant-time.md), and
[Usage 34](../usage/34-pdg-information-flow.md).

## References

1. Hammer, C., Snelting, G. (2009). *Flow-sensitive, context-sensitive, and
   object-sensitive information flow control based on program dependence
   graphs.* DOI:
   [10.1007/s10207-009-0086-1](https://doi.org/10.1007/s10207-009-0086-1).
2. Barthe, G., Betarte, G., Campo, J. D., Luna, C., Pichardie, D. (2014).
   *System-level non-interference for constant-time cryptography.* DOI:
   [10.1145/2660267.2660283](https://doi.org/10.1145/2660267.2660283).
