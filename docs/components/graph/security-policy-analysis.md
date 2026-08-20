# Secret-flow and access-policy component

The feature-free `analysis::security::{secrets,authz}` components turn typed
CPG evidence into qualified advisory candidates. They reuse, rather than
replace, the interprocedural taint and population missing-check engines.

## Public surface

### Secret flow

| API | Contract |
|---|---|
| `builtin_secret_rules()` | returns owned authentication, network, crypto-key, and name-hint defaults |
| `analyze_builtin_secrets(cpg, resolution, config)` | runs the shipped secret policy |
| `analyze_secret_flows(cpg, resolution, rules, config)` | runs a caller-supplied `SecretRuleSet` |
| `SecretSinkRule` / `SecretSinkFamily` | named callee alternatives, optional positions, and boundary category |
| `SecretAnalysisConfig` | entropy thresholds, literal/context bounds, and delegated `IfdsCaps` |
| `SecretFinding` / `SecretLiteralEvidence` | node-only provenance, witness, non-sensitive qualifier, and cap flag |
| `SecretAnalysis` | findings, complete delegated taint report, issues, and `is_complete()` |

### Missing access guards

| API | Contract |
|---|---|
| `builtin_authz_rules()` | returns owned mutation and authN/authZ guard defaults |
| `analyze_builtin_authz(cpg, config)` | runs the shipped access policy |
| `analyze_missing_authz(cpg, rules, config)` | runs a caller-supplied `AuthzRuleSet` |
| `AuthzMutationRule` | stable name plus typed mutation-call alternatives |
| `AuthzAnalysisConfig` | exact support, minimum population, peer scope, and fingerprint/path bounds |
| `AuthzFinding` | missing normalized access guards, exact support, peer sites, and guard witnesses |
| `AuthzAnalysis` | findings, admitted sites, translated issues, and `is_complete()` |

All types are available with `default = []`. Policy and report types implement
Serde only when `serde` is enabled.

## Composition flow

![Secret qualification and access-guard filtering delegate semantic fixed points](../../diagrams/security-policy-pipeline.svg)

J3 performs these steps:

1. Validate nonempty rules, patterns, and entropy thresholds.
2. Enumerate typed string literals canonically under an explicit cap.
3. Qualify entropy directly or typed binding context with a heap worklist.
4. Compile all string origins and security sinks into `TaintCatalog`.
5. Run `analyze_taint_report` with caller-supplied call resolution and caps.
6. Retain only qualified origins and governed argument positions.

J4 performs these steps:

1. Validate mutation and access-guard patterns.
2. Compile valid mutation calls into the missing-check sink catalog.
3. Run `analyze_missing_checks` with exact support and peer topology.
4. Scan each flat expected fingerprint for typed authN/authZ evidence.
5. Retain corresponding support and witness maps without recomputation.

## Invariants

- The input `CodePropertyGraph` is never mutated.
- Neither component parses retained source text.
- Secret findings contain no literal or binding value.
- J3 flow witnesses come from the shared IFDS-style taint engine.
- J4 support, peers, fingerprints, and witnesses come from the shared I6 engine.
- Non-access checks cannot enter `AuthzFinding`.
- Invalid patterns are reported and excluded from matching.
- A local or delegated issue makes the relevant report incomplete.
- All depth-proportional continuation state is heap-resident.

## Extension guidance

Prefer `Pattern::Exact` for stable project APIs and reviewed `Segment` patterns
for cross-namespace method families. In `SecretSinkRule`, leave
`argument_indices` as `None` only when every parameter genuinely crosses the
same security boundary. Use `with_argument` for key and credential positions.

Tune entropy thresholds against a versioned corpus with both real/test secrets
and ordinary identifiers. Binding-name hints should describe semantic names,
not values. For authZ, require multiple comparable sites and a threshold that
matches the desired evidence standard; never treat local convention as policy
truth without review.

See [ADR-0050](../../design/0050-composed-secret-and-access-policy-analysis.md),
[Theory 40](../../theory/40-secret-flow-and-access-policy-inference.md), and
[Usage 23](../../usage/23-secret-and-access-policy-analysis.md).
