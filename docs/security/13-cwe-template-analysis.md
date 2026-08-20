# Security 13 — CWE-template analysis boundary

Declarative Common Weakness Enumeration (CWE) templates are security-analysis
inputs, but their output remains evidence for human or policy review. This
chapter defines trust boundaries, misuse resistance, confidentiality behavior,
resource controls, and the exact claims a caller may make.

## 1. Assets and actors

| Actor or asset | Security significance |
|---|---|
| source and parser owner | determines which program relations enter the CPG |
| template author | defines name catalogs, structural shapes, and semantic primitive |
| analysis caller | selects caps, call resolution, templates, and interpretation |
| `CodePropertyGraph` | contains typed syntax, flow, dependence, calls, literals, and locations |
| delegated report | carries taint/null/typestate/interval qualification and witnesses |
| `CweCandidate` | carries advisory evidence and represented node identifiers |
| report consumer | decides triage priority and may access source or serialized output |

The target program, parser output, custom templates, and analysis configuration
are untrusted with respect to availability. Literal contents can also be
sensitive with respect to confidentiality.

## 2. Threats and controls

### 2.1 False certainty from partial evidence

An empty candidate vector is not independently meaningful. Structural
enumeration may be capped; a function, CFG state, integer type, or array length
may be absent; a delegated solver may be incomplete. libcpg records those cases
and defines `CweAnalysis::is_complete()` across both the CWE layer and every
retained delegated report.

**Control:** permit a negative represented-domain claim only when
`is_complete()` is true. Even then, do not generalize beyond the built CPG, the
selected templates, and their name catalogs.

### 2.2 Availability attacks through graph size or depth

A target may contain very deep ASTs, large control-flow regions, high structural
match multiplicity, or cyclic/malformed relations. Native recursion would make
depth an implicit process-stack limit.

**Controls:** explicit VF2 search state; queue-based CFG and array-origin walks;
ordered heap frontiers for AST parents; flat interval expression evaluation;
validated configurable caps under compile-time ceilings; typed cap issues.

### 2.3 Secret disclosure from hard-coded credentials

Detecting CWE-798 requires observing a typed string literal near a
credential-named assignment. Copying that value into logs, JSON, or dashboards
would amplify the original secret exposure.

**Control:** `HardcodedCredential` retains only the credential role and graph
provenance. `CweCandidate::source` and `witness` identify the represented literal
node, but the candidate contains no literal text. Restrict access to the input
CPG separately because the graph itself may retain source-derived values.

### 2.4 Malicious or careless templates

A custom template can use overly broad substrings, select the wrong primitive,
or request a shape that matches many nodes. It cannot gain code execution, but
it can create misleading candidates or consume the configured work budget.

**Controls:** dense structural validation, unique stable identifiers, explicit
role catalogs, hard ceilings, a cap-plus-one completeness check, immutable
output, and full structural embeddings for audit. Treat templates as versioned
security policy and review changes like code.

### 2.5 Name spoofing and unresolved callees

Call-role matching operates on represented typed callee paths. A misleading
identifier, unresolved dynamic dispatch, reflection, or missing dependency can
change the represented identity relation.

**Control:** pass the best available `CallGraphResolution`; inspect delegated
issues and witnesses; use exact project-specific patterns where stable; never
interpret a name match without the selected semantic primitive.

### 2.6 Advisory output used as an automated verdict

Static may-analysis can include infeasible paths, allocation-site joins, or
library models that are conservative. A candidate does not establish attacker
reachability or impact.

**Control:** keep candidate handling out of automatic source rewriting and
access-control decisions. Correlate with authentication boundaries, deployment
configuration, reachability, tests, and runtime evidence before severity or
remediation decisions.

![The clean-scan path stays closed until structural, semantic, and completeness evidence all pass](../diagrams/cwe-evidence-qualification.svg)

## 3. CWE-specific interpretation limits

| CWE family | Represented claim | Important residual uncertainty |
|---|---|---|
| 22/78/79/89/502 | named origin may flow to named sensitive use without a recognized neutralizer | custom encoders, implicit framework guarantees, path feasibility, reflection |
| 190 | abstract result may exceed the represented fixed-width type domain | language overflow mode, checked operations, compiler transformations |
| 476 | maybe-null origin may reach a typed dereference | path feasibility, external contracts, unsafe/native behavior |
| 416 | represented post-free event violates the finite-state property | allocator aliases, custom ownership, hidden frees/uses |
| 787 | index range may leave the represented array domain at a write | dynamic allocation size, pointer arithmetic, custom bounds checks |
| 798 | a credential-named target receives a typed string literal | whether value is a real secret, test-only context, later rotation |

The official MITRE definitions describe weakness classes, not a guarantee that
one static pattern maps perfectly to every real vulnerability
[[1]](#references). The implementation deliberately names results
“candidates.”

## 4. Safe operational procedure

```text
REVIEW-CWE-REPORT(report, protected_cpg):
    Authorize access to the CPG and report independently.
    If report is incomplete:
        Triage every CWE issue and delegated-analysis issue.
        Rerun with repaired graph evidence or an approved larger bounded policy.
        Do not publish a clean-scan statement.

    For each candidate:
        Resolve every witness NodeId against the protected CPG.
        Confirm frontend identity, control/data path, and relevant library model.
        Evaluate runtime reachability, attacker control, and deployed mitigations.
        Record the human or policy decision separately from the immutable report.

    If no candidate exists and the report is complete:
        State only the templates, graph build, and represented evidence examined.
```

Do not increase caps merely to silence an issue. First determine whether the
graph or template is malformed, whether the match family is expected, and what
resource envelope is acceptable. Cap changes are security-policy changes.

## 5. Serialization and logging

With `serde`, the aggregate report preserves stable order and typed evidence.
Before exporting:

- apply the same access controls as the source-analysis system;
- avoid joining literal text from the CPG into CWE-798 report logs;
- retain `issues` and delegated reports rather than exporting candidates alone;
- record template versions and configuration beside the report;
- treat source locations and API paths as potentially sensitive metadata.

## 6. Non-goals

The component does not provide:

- proof of exploitability or absence of all vulnerabilities;
- Common Vulnerability Scoring System (CVSS) severity;
- automatic Common Vulnerabilities and Exposures (CVE) mapping;
- source repair, sanitizer synthesis, or policy enforcement;
- whole-program guarantees beyond represented CPG and summary evidence;
- secret scanning outside the typed CWE-798 assignment relation.

See [ADR-0048](../design/0048-declarative-qualified-cwe-templates.md),
[Theory 38](../theory/38-declarative-cwe-query-semantics.md), and the
[component contract](../components/graph/cwe-template-analysis.md).

## References

1. MITRE. *Common Weakness Enumeration*, version 4.20.
   [CWE list](https://cwe.mitre.org/data/definitions/1000.html).
2. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and
   Discovering Vulnerabilities with Code Property Graphs.* DOI:
   [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44).
