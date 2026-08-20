# Security 14 — Cryptographic API-misuse boundary

The crypto checker reports evidence for security review. This chapter defines
its trust boundary and the claims consumers may make.

## 1. Assets and actors

| Actor or asset | Security significance |
|---|---|
| frontend and dependency owner | determines which calls, literals, CFG, and DFG relations are represented |
| policy author | chooses call patterns, argument positions, finite-state protocol, and random-source catalog |
| target CPG | may contain sensitive literal values and attacker-shaped depth, breadth, and cycles |
| `CryptoAnalysis` | carries findings plus all completeness evidence |
| report consumer | resolves witnesses, validates runtime context, and decides remediation |

The graph, custom policy, and requested caps are untrusted for availability.
Literal contents are sensitive for confidentiality.

## 2. Threats and controls

### Partial evidence mistaken for a clean scan

Call or provenance caps, capped callee reconstruction, invalid policy, or an
incomplete delegated solver can omit candidates.

**Control:** require `CryptoAnalysis::is_complete()` before making even a
represented-domain negative claim. Retain `issues`, `typestate`, and `taint`
when serializing or aggregating a report.

### Secret amplification

A hardcoded key is already present in the analyzed program. Copying it into a
finding, log, or dashboard would increase exposure.

**Control:** the finding stores only literal `NodeId` provenance and a witness.
The underlying protected CPG may still contain the value and requires source
repository access controls.

### Resource exhaustion and stack overflow

Deep alias chains, malformed cycles, many call sites, and high fan-in can be
attacker controlled.

**Control:** heap-resident queues and ordered visited sets; independent call,
origin-node, and delegated IFDS/typestate caps; compile-time ceilings; typed cap
issues; no native recursion proportional to input depth.

### Name spoofing and incomplete library models

An identifier can resemble a known crypto API without having its semantics.
Reflection, dynamic dispatch, native code, or missing dependencies can hide the
real target.

**Control:** use the best available `CallGraphResolution`; prefer exact patterns
for project APIs; inspect typed witnesses; test policy changes against positive
and clean fixtures. A name match is classification evidence, not proof.

### False confidence in randomness

A call named `SecureRandom` may be seeded incorrectly; a random IV may still be
reused; a nonce can have adequate entropy but violate uniqueness.

**Control:** interpret secure-random patterns only as represented provenance.
Review initialization, deployment entropy, nonce lifecycle, and library
contracts separately.

### Advisory results used as automatic enforcement

Static may-analysis joins paths and objects. A candidate can be infeasible; an
empty complete report covers only the represented policy.

**Control:** do not automatically rewrite source, rotate keys, block deployment,
or assert cryptographic safety from this report alone.

![Every negative interpretation passes local and delegated completeness gates](../diagrams/crypto-evidence-qualification.svg)

## 3. Finding interpretation

| Finding | Review questions |
|---|---|
| weak hash | Is MD5/SHA-1 used for security, compatibility, or a non-adversarial checksum? |
| ECB mode | Does the library actually select ECB; is structured plaintext confidentiality required? |
| predictable IV | Is the value fixed at runtime; does the mode require unpredictability, uniqueness, or both? |
| hardcoded key | Is the literal a real key, a test fixture, an identifier, or public material? |
| invalid call order | Is the represented receiver/object alias correct; does the library protocol match the policy? |

## 4. Safe review procedure

```text
REVIEW-CRYPTO-REPORT(report, protected_cpg, policy_version):
    Record the graph build, call resolution, policy version, and caps.
    If report is incomplete:
        inspect every local and delegated issue;
        repair missing graph/policy evidence or approve a bounded increase;
        do not publish a clean result.

    For each finding:
        resolve every NodeId against the protected CPG;
        confirm callee identity, argument position, provenance, and path context;
        compare with the deployed library's cryptographic contract;
        record triage separately from the immutable report.

    If the report is complete and empty:
        state only that the selected policy found no represented candidate.
```

## 5. Serialization and logging

The `serde` feature preserves ordered policy, findings, issues, and delegated
reports. Never export `findings` alone. Protect call paths and source locations
as source-analysis metadata. Do not enrich hardcoded-key findings with literal
text in routine logs.

## 6. Non-goals

The component does not prove algorithm implementation correctness, entropy or
nonce uniqueness, side-channel resistance, protocol security, exploitability,
severity, or universal absence of cryptographic misuse.

See [ADR-0049](../design/0049-composed-cryptographic-api-misuse.md),
[Theory 39](../theory/39-cryptographic-api-misuse.md), and
[Usage 22](../usage/22-cryptographic-api-misuse.md).

## References

1. Krüger, S., Späth, J., Ali, K., Bodden, E., Mezini, M. (2019). *CrySL: An
   Extensible Approach to Validating the Correct Usage of Cryptographic APIs.*
   DOI: [10.1109/TSE.2019.2948910](https://doi.org/10.1109/TSE.2019.2948910).
2. Rahaman, S., et al. (2019). *CryptoGuard: High Precision Detection of
   Cryptographic Vulnerabilities in Massive-sized Java Projects.* DOI:
   [10.1145/3319535.3345659](https://doi.org/10.1145/3319535.3345659).
