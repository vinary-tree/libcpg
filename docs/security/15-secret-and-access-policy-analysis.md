# Security 15 — Secret-flow and access-policy boundary

The J3 and J4 detectors produce evidence for human security review. This
chapter defines the trust boundary and claims consumers may make.

## 1. Assets and actors

| Actor or asset | Security significance |
|---|---|
| frontend and graph owner | determines which literals, calls, CFG, DFG, PDG, and call targets exist |
| policy author | chooses secret thresholds/hints/sinks and mutation/access-guard patterns |
| target CPG | may contain actual credentials and attacker-shaped depth, breadth, and cycles |
| `SecretAnalysis` | carries findings, delegated taint report, and qualification issues |
| `AuthzAnalysis` | carries missing access guards, admitted populations, and translated issues |
| report consumer | validates witnesses and deployed policy before remediation |

The graph, custom policies, and requested caps are untrusted for availability.
Literal payloads are sensitive for confidentiality.

## 2. Threats and controls

### Secret amplification

A candidate credential already exists in protected source. Copying it into a
finding, log, ticket, or dashboard expands exposure.

**Control:** findings retain `NodeId`, byte length, integer entropy, matched
policy pattern, typed context, and flow witness. They omit literal and binding
text. The underlying CPG remains sensitive and needs repository-equivalent
access control.

### High entropy mistaken for secrecy

Hashes, generated identifiers, compressed data, and test fixtures can have high
entropy. Low-entropy passwords can be genuine secrets.

**Control:** entropy and names qualify candidates only. Require a represented
flow to a reviewed sink, triage source context, and maintain project-specific
positive and clean fixtures. Never describe entropy as probability or validity.

### Name and API spoofing

An identifier can contain `auth`, `secret`, or `update` without the expected
semantics. Reflection, unresolved dispatch, native code, and missing dependencies
can hide the actual target.

**Control:** prefer exact project patterns and the best available call
resolution. Inspect typed witnesses and serialized policy versions. A name match
is classification evidence, not semantic proof.

### Common-but-wrong authorization convention

Population inference assumes repeated practice is useful evidence. Every peer
could omit a required check or repeat the same insufficient guard.

**Control:** J4 reports only deviations from represented peers. Compare results
with an authoritative access-control specification, framework middleware,
deployment policy, and complete-mediation requirements. Do not infer that
reported peers are secure.

### Non-access check relabeled as authorization

Peers may share null, quota, range, or feature checks around a mutation.

**Control:** the specialization retains only normalized fingerprints containing
a configured typed authN/authZ call/member/attribute/macro. Generic identifiers
are abstracted and cannot qualify a guard by spelling alone.

### Partial evidence mistaken for a clean scan

Literal/context/IFDS caps, malformed graph relations, missing call identities,
fingerprint caps, or invalid policy can omit candidates.

**Control:** retain aggregate reports and require `is_complete()`. Every local
and delegated issue is typed. Findings may be reviewed under partial evidence;
an empty incomplete result is never clean.

### Resource exhaustion and stack overflow

Deep wrappers, cyclic AST/DFG edges, large populations, and broad rules may be
attacker controlled.

**Control:** compile-time ceilings, caller bounds, ordered visited sets,
heap-resident queues/flat tapes, and explicit cap issues. No traversal recurses
in proportion to source or graph depth.

![Interpretation passes confidentiality, policy, and completeness gates](../diagrams/security-policy-qualification.svg)

## 3. Safe review procedure

```text
REVIEW-SECURITY-POLICY-REPORT(report, protected_cpg, policy_version):
    Record graph build, call resolution, PDG status, policy version, and caps.
    If report is incomplete:
        inspect every local and delegated issue;
        repair missing evidence or approve a bounded increase;
        do not publish a clean result.

    For each secret finding:
        resolve NodeIds only inside the protected environment;
        confirm source meaning, sink identity, argument role, and witness path;
        test whether the credential is real/live through an authorized process.

    For each missing access-guard finding:
        inspect exact peer population, support, fingerprint, and witnesses;
        compare the mutation with authoritative authN/authZ policy;
        account for framework/middleware guards absent from the CPG.

    If a complete report is empty:
        state only that the selected represented policy found no candidate.
```

## 4. Non-goals

The components do not prove credential liveness, randomness, exploitability,
runtime path feasibility, complete mediation, identity correctness, least
privilege, policy correctness, or universal absence of secret/access-control
defects. They do not rotate credentials, rewrite source, or block deployment.

See [ADR-0050](../design/0050-composed-secret-and-access-policy-analysis.md),
[Theory 40](../theory/40-secret-flow-and-access-policy-inference.md), and
[Usage 23](../usage/23-secret-and-access-policy-analysis.md).

## References

1. Meli, M., McNiece, M. R., Reaves, B. (2019). *How Bad Can It Git?
   Characterizing Secret Leakage in Public GitHub Repositories.* DOI:
   [10.14722/ndss.2019.23418](https://doi.org/10.14722/ndss.2019.23418).
2. Yamaguchi, F., Wressnegger, C., Gascon, H., Rieck, K. (2013). *Chucky:
   Exposing Missing Checks in Source Code for Vulnerability Discovery.* DOI:
   [10.1145/2508859.2516665](https://doi.org/10.1145/2508859.2516665).
