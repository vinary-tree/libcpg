# CWE template analysis

The feature-free `analysis::security::cwe` component evaluates declarative
Common Weakness Enumeration (CWE) templates against one immutable
`CodePropertyGraph`. It combines a bounded structural graph query with an
existing semantic primitive and returns qualified advisory candidates plus all
retained evidence needed to review completeness.

## Responsibilities

The component owns:

- validation and canonical ordering of `CweTemplate` values;
- construction of finite pattern CPGs from `CweStructuralPattern`;
- bounded stack-safe VF2 embedding and structural-site qualification;
- compilation of template roles into shared taint classes;
- projection of taint, null-flow, typestate, interval, and literal evidence;
- deterministic candidate deduplication and typed issue reporting.

It does not own parsing, language grammars, call resolution, the underlying
semantic solvers, exploitability decisions, severity scoring, or remediation.

![The component delegates semantic questions while retaining their full qualification](../../diagrams/cwe-template-pipeline.svg)

## Public entry points

| API | Contract |
|---|---|
| `builtin_cwe_templates()` | returns the canonical ten-template registry as caller-owned data |
| `analyze_builtin_cwes(cpg, resolution, config)` | evaluates that registry and retains shared reports |
| `analyze_cwe_templates(cpg, resolution, templates, config)` | evaluates a validated caller-supplied template set |

The input graph and `CallGraphResolution` are borrowed and never mutated.
`analyze_cwe_templates` returns `CweAnalysisError` only for invalid requests;
missing or capped represented evidence is returned inside a successful
`CweAnalysis` as `CweAnalysisIssue`.

## Template syntax

`CweRole` has three independent predicate collections:

- `call_patterns` match reconstructed typed callee paths;
- `identifier_patterns` match represented identifier/member roles;
- `operators` match exact normalized operators.

`CweStructuralPattern::new(nodes, edges, sink)` requires dense pattern indices.
Every edge endpoint and `sink` must index `nodes`. `CwePatternEdge::ast_child`
constructs the common AST-child relation. General edges preserve typed broad
AST, CFG, DFG, and call classes and exact PDG, reference, and scope kinds through
the shared VF2 relation.

For example, a caller can clone a built-in template and narrow only its catalog
roles without changing the solver:

```rust,ignore
use libcpg::{
    analyze_cwe_templates, builtin_cwe_templates, CallGraphResolution,
    CodePropertyGraph, CweAnalysisConfig, Pattern,
};

fn analyze_project_sql(cpg: &CodePropertyGraph) {
    let mut sql = builtin_cwe_templates()
        .into_iter()
        .find(|template| template.cwe == 89)
        .expect("the shipped SQL template exists");
    sql.source.call_patterns.push(Pattern::Substring("http.input".into()));
    sql.sanitizer
        .call_patterns
        .push(Pattern::Exact("project::PreparedQuery::bind".into()));
    sql.sink
        .call_patterns
        .push(Pattern::Substring("project::database::execute".into()));

    let report = analyze_cwe_templates(
        cpg,
        &CallGraphResolution::default(),
        &[sql],
        &CweAnalysisConfig::default(),
    )
    .expect("the customized template is valid");
    if report.is_complete() {
        println!("{} qualified candidate(s)", report.candidates.len());
    } else {
        eprintln!("incomplete represented evidence: {:?}", report.issues);
    }
}
```

`Pattern` matching is catalog matching over typed names. It is not source-code
parsing. Prefer `Exact` for stable project APIs and carefully reviewed
`Substring` values for multi-language or framework families.

## Primitive contracts

| `CwePrimitive` | Delegated provider | Projected evidence |
|---|---|---|
| `TaintFlow` | summary-based taint | origin, sink, parameter, canonical witness |
| `NullDereference` | summary-based maybe-null | null origin, dereference sink, parameter, witness |
| `Typestate` | finite-state typestate | object, pre-event state, forbidden event, witness |
| `IntegerOverflow` | interval analysis | result interval, represented fixed-width type domain |
| `OutOfBoundsWrite` | interval analysis plus array-origin walk | index interval and represented array length |
| `HardcodedCredential` | typed AST/literal evidence | literal provenance and credential role; no literal value |

Taint templates share one solver invocation and one retained `TaintAnalysis`.
Null templates share one retained `NullAnalysis`. Typestate templates retain
their individual `TypestateAnalysis` values because each contains a distinct
finite-state property.

Interval requests are grouped by containing function. Each function is solved
once, and selected expression roots are evaluated in the nearest represented
CFG contexts with the shared flat expression evaluator.

## Built-in registry

The shipped identifiers are stable:

| Identifier | CWE | Primitive |
|---|---:|---|
| `cwe-22-path-traversal` | 22 | taint |
| `cwe-78-command-injection` | 78 | taint |
| `cwe-79-cross-site-scripting` | 79 | taint |
| `cwe-89-sql-injection` | 89 | taint |
| `cwe-190-integer-overflow` | 190 | interval |
| `cwe-476-null-dereference` | 476 | null flow |
| `cwe-416-use-after-free` | 416 | typestate |
| `cwe-787-out-of-bounds-write` | 787 | interval and array origin |
| `cwe-798-hardcoded-credentials` | 798 | typed literal provenance |
| `cwe-502-unsafe-deserialization` | 502 | taint |

Callers own the returned vector and may extend, remove, or specialize templates
before invoking `analyze_cwe_templates`.

## Result contract

`CweAnalysis` contains:

- `templates`: evaluated stable identifiers in canonical order;
- `structural_matches`: retained pattern-index-to-`NodeId` embeddings;
- `candidates`: ordered immutable `CweCandidate` values;
- `taint_analysis` and `null_analysis`: shared delegated reports when used;
- `typestate_analyses`: per-template delegated reports;
- `issues`: ordered CWE-layer incomplete-evidence records.

The candidate witness and optional origin always refer to nodes in the input
graph. `confidence_milli` is an evidence score, not a probability. Exact finite
interval and structural/typestate evidence use `1000`; an unbounded interval
uses `700`; taint/null scores preserve the delegated solver score after bounded
conversion.

The safe consumer rule is:

```text
If candidates are nonempty:
    Review each candidate and its delegated evidence.
If candidates are empty and report.is_complete():
    State only that no requested pattern matched represented evidence.
If report.is_complete() is false:
    Inspect issues and delegated reports; do not make a clean-scan claim.
```

## Errors and issues

`CweAnalysisError` rejects malformed inputs:

- a zero cap or cap above its compile-time ceiling;
- more templates than the configured bound;
- duplicate or empty template identifiers;
- CWE zero or an empty title/role name;
- an empty structural-alternative list;
- non-dense edge/sink indices;
- a taint template with an empty source or sink call catalog.

`CweAnalysisIssue` qualifies valid requests whose represented evidence is
incomplete. Important distinctions include missing versus contradictory integer
types, missing versus contradictory array lengths, solver failure versus absent
CFG context, and retained-prefix exhaustion versus a complete match set.

## Resource and stack model

`CweAnalysisConfig` exposes four CWE-layer bounds and nests `IfdsCaps` for the
delegated solvers. Compile-time constants prevent a caller from selecting
unreasonably high structural or origin-walk limits.

All target-depth work resides in heap data structures. The component uses
explicit VF2 states, queues for reachability and origin tracing, ordered sets for
AST-parent frontiers, and the flat interval expression machine. The 100,000-node
small-stack oracle validates this contract.

## Feature and serialization contract

The complete API is available with `--no-default-features`. Enabling `serde`
adds `Serialize` and `Deserialize` to templates, configurations, issues,
structural matches, candidates, evidence, delegated reports, and the aggregate
analysis. Serialization does not alter ordering or completeness semantics.

See [Theory 38](../../theory/38-declarative-cwe-query-semantics.md),
[ADR-0048](../../design/0048-declarative-qualified-cwe-templates.md),
[Security 13](../../security/13-cwe-template-analysis.md), and
[Usage 21](../../usage/21-cwe-template-analysis.md).

## References

1. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and
   Discovering Vulnerabilities with Code Property Graphs.* DOI:
   [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44).
2. MITRE. *Common Weakness Enumeration*, version 4.20.
   [CWE list](https://cwe.mitre.org/data/definitions/1000.html).
