# ADR-0048: Compose declarative, qualified CWE templates

- **Status:** Accepted
- **Date:** 2026-08-20
- **Scope:** feature-free `analysis::security::cwe`

## Context

The **Common Weakness Enumeration (CWE)** is a catalog of software and hardware
weakness classes. A **CWE template** in libcpg is a data value that binds a
finite structural graph pattern, named source/sanitizer/sink roles, and one
existing semantic analysis primitive. It is not a parser, a vulnerability
verdict, or a source-rewriting rule.

Yamaguchi et al. established the Code Property Graph (CPG) as a joint substrate
for graph-shaped vulnerability templates over syntax, control, and dependence
relations [[1]](#references). More recent systems demonstrate the continuing
value of explicit vulnerability-pattern languages and CPG-selected context
[[2]](#references) [[3]](#references). libcpg already supplies the required
feature-free primitives: stack-safe VF2 subgraph matching, summary-based taint
and null flow, finite-state typestate, and interval analysis. The missing
decision is how to compose those engines without duplicating their solvers,
parsing source again, hiding bounded evidence, or mutating the graph.

## Decision

### 1. Keep syntax declarative and semantics delegated

`CweTemplate` carries:

- a unique stable identifier, CWE number, and advisory title;
- `CweRole` values for origins, neutralizers, and sensitive uses;
- one or more `CweStructuralPattern` alternatives;
- exactly one `CwePrimitive` selecting an existing semantic engine.

Each structural pattern is a dense vector of exact `NodeKindTag` values plus
typed directed edges and a designated sink index. It is converted to a small
CPG and embedded with the existing stack-safe `Vf2Matcher`. The template layer
does not tokenize text, compile regular expressions over source, or implement a
second data-flow solver.

### 2. Require structural qualification before semantic projection

Let $`P_t`$ be the union of target sites produced by complete retained
embeddings of template $`t`$, and let $`S_t`$ be the sites reported by its
semantic primitive. The candidate-site relation is:

```math
C_t = P_t \cap S_t
```

For taint-backed templates, the full candidate relation also retains the
origin and witness:

```math
F_t = \{(o,s,w) \mid s \in P_t \land (o,s,w) \in \operatorname{Taint}_t\}
```

This two-stage contract prevents a broad semantic result from escaping the
template's graph shape and prevents a structural resemblance from becoming a
security claim without semantic evidence.

![Structural qualification and delegated evidence converge on an immutable report](../diagrams/cwe-template-pipeline.svg)

### 3. Ship ten explicit templates

| CWE | Shipped primitive | Required evidence |
|---:|---|---|
| 22 path traversal | taint | typed input-to-filesystem flow without a matching canonicalizer/validator |
| 78 command injection | taint | typed input-to-command flow without shell neutralization |
| 79 cross-site scripting | taint | typed input-to-response flow without output neutralization |
| 89 SQL injection | taint | typed input-to-query flow without parameterization/neutralization |
| 190 integer overflow | intervals | arithmetic interval exceeds the exact represented fixed-width integer domain |
| 476 null dereference | null flow | maybe-null origin reaches a typed dereference |
| 416 use after free | typestate | a represented use/read/write/dereference event occurs after `free` |
| 787 out-of-bounds write | intervals | index interval is not contained in the represented array domain |
| 798 hard-coded credentials | structural | typed string literal is assigned to a credential-named target; value is not copied into the finding |
| 502 unsafe deserialization | taint | typed input reaches a deserializer without a matching schema/signature/safe-loader step |

The names and security meanings follow the corresponding MITRE CWE entries
[[4]](#references). Call and identifier patterns are transparent catalog data;
call matching consumes reconstructed typed callee paths, not raw source text.

### 4. Make completeness part of the result

`CweAnalysis::is_complete()` is true only when:

1. the CWE layer has no issue;
2. the retained shared taint report is complete;
3. the retained shared null-flow report is complete; and
4. every retained typestate report is complete.

The issue vocabulary distinguishes structural-match exhaustion, missing
function or interval context, missing/ambiguous/unsupported integer types,
missing/ambiguous array lengths, interval solver errors, and array-origin
budget exhaustion. Ambiguity is never collapsed into absence. Configuration
validation rejects zero, excessive, duplicate, empty, and malformed inputs
before analysis.

| Resource | Default | Compile-time ceiling |
|---|---:|---:|
| templates per request | 128 | 256 |
| structural alternatives per template | 16 | 64 |
| retained embeddings per alternative | 10,000 | 100,000 |
| nodes visited per array-origin query | 100,000 | 1,000,000 |

The delegated IFDS-family analyses retain their own `IfdsCaps` policy.

### 5. Preserve immutable, out-of-graph evidence

`CweCandidate` retains the template identity, CWE number, represented site,
optional represented origin, canonical witness, integer evidence score, and a
typed `CweEvidence` variant. It does not add nodes or edges, rewrite source, or
claim exploitability. `confidence_milli` is a deterministic evidence score in
`0..=1000`; it is not a calibrated probability.

Hard-coded credential evidence retains the literal's `NodeId` but never copies
the credential value into the candidate. This permits a caller with legitimate
graph access to inspect provenance while reducing accidental secret exposure
in serialized reports.

### 6. Keep input-shaped work on the heap

The reused VF2 engine stores search states explicitly. Function ownership,
reachable-block discovery, nearest CFG-context discovery, array-origin walks,
canonicalization, and deduplication use `VecDeque`, `BTreeMap`, or `BTreeSet`.
The interval expression adapter reuses the shared flat expression evaluator.
Native call-stack depth is therefore independent of AST depth, CFG depth,
candidate count, and structural search depth.

![Every negative claim crosses explicit evidence and completeness gates](../diagrams/cwe-evidence-qualification.svg)

## Consequences

### Positive

- The public registry is feature-free and serializable under `serde`.
- Templates are data-driven while semantic engines remain single-sourced.
- Reports are deterministic under template and graph-relation insertion
  permutations.
- Structural and semantic evidence remain inspectable and out of the input CPG.
- Caps and missing or contradictory representations cannot masquerade as a
  clean scan.
- Deep input executes with constant native-stack demand.

### Trade-offs

- Name catalogs approximate library intent and require caller review or custom
  templates for project-specific APIs.
- Interval and typestate results are relative to represented static evidence;
  reflection, native code, infeasible paths, or missing frontend relations can
  change real runtime behavior.
- Broad edge classes make structural alternatives portable across frontends,
  but they do not imply exact source syntax.
- Candidate evidence supports triage; it does not prove reachability by an
  adversary, exploitability, severity, or a safe repair.

### Rejected alternatives

- **Hand-roll a source parser.** That would duplicate language frontends and
  make template behavior syntax-dependent.
- **Encode each CWE as a bespoke traversal.** That would duplicate validation,
  bounds, ordering, and evidence qualification.
- **Trust name matching alone.** Names do not establish data flow, nullness,
  typestate, or numeric feasibility.
- **Drop capped or ambiguous sites.** Silent omission would make an empty
  finding vector unsafe to interpret.
- **Copy literal secrets into findings.** Provenance is sufficient and safer.
- **Use native recursive walks.** Input depth is untrusted and must not consume
  the native stack.

## Verification

- Example-based unit tests cover all ten registry entries, their clean
  counterparts, validation failures, serde, and ambiguity.
- A 128-case property campaign permutes template and relation insertion order.
- A 100,000-node interval expression runs on a 64-KiB native stack.
- Public integration tests exercise hand-built CPG evidence and a Rust
  Tree-sitter construction pipeline.
- The executable example uses only the feature-free public API.
- The Criterion target asserts exact candidates before timing 20- and
  100-endpoint corpora.

## References

1. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and
   Discovering Vulnerabilities with Code Property Graphs.* DOI:
   [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44).
2. Li, P., Yao, S., Korich, J. S., Luo, C., Yu, J., Cao, Y., Yang, J. (2025).
   *Neuro-symbolic Static Analysis with LLM-generated Vulnerability Patterns.*
   DOI: [10.48550/arXiv.2504.16057](https://doi.org/10.48550/arXiv.2504.16057).
3. Lekssays, A., Mouhcine, H., Tran, K., Yu, T., Khalil, I. (2025).
   *LLMxCPG: Context-Aware Vulnerability Detection Through Code Property
   Graph-Guided Large Language Models.* DOI:
   [10.48550/arXiv.2507.16585](https://doi.org/10.48550/arXiv.2507.16585).
4. MITRE. *Common Weakness Enumeration*, version 4.20. Individual definitions:
   [22](https://cwe.mitre.org/data/definitions/22.html),
   [78](https://cwe.mitre.org/data/definitions/78.html),
   [79](https://cwe.mitre.org/data/definitions/79.html),
   [89](https://cwe.mitre.org/data/definitions/89.html),
   [190](https://cwe.mitre.org/data/definitions/190.html),
   [416](https://cwe.mitre.org/data/definitions/416.html),
   [476](https://cwe.mitre.org/data/definitions/476.html),
   [502](https://cwe.mitre.org/data/definitions/502.html),
   [787](https://cwe.mitre.org/data/definitions/787.html), and
   [798](https://cwe.mitre.org/data/definitions/798.html).
