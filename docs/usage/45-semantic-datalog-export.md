# Export semantic evidence for Datalog

This guide begins after `analyze_semantic_expression_flow` has produced a live
report. Read [Usage 44](44-semantic-expression-flow.md) first if expression
intermediate representations, proof replay, or semantic flow are unfamiliar.

## 1. Export with an explicit policy

```rust,ignore
use libcpg::analysis::{export_semantic_datalog, SemanticDatalogLimits};

let semantic_facts = export_semantic_datalog(
    &report,
    SemanticDatalogLimits {
        max_facts: 1_000_000,
        max_rendered_bytes: 64 * 1024 * 1024,
    },
)?;
```

The call borrows `report`, replays proofs, and validates candidate indices.
Keep the report alive if a later consumer must replay `proof_pair`; the fact
bundle cannot recreate proof authority.

## 2. Preserve semantic completion

```rust,ignore
use libcpg::analysis::{
    DatalogEvaluationCompletion, SemanticDatalogDimension,
};

let status = semantic_facts.query_completion(
    SemanticDatalogDimension::ExactEquality,
    DatalogEvaluationCompletion::Unknown,
);
assert!(!status.can_conclude_absence());
```

A limited report may contain valid positive rows. Load them if useful, but do
not interpret a missing row as false until the relevant semantic dimension and
the external Datalog fixed point both complete.

## 3. Declare matching relations

```prolog
.decl semantic_class(function:number, representative:number, occurrence:number)
.decl semantic_proof(domain:number, left:number, right:number, proof_pair:number)
.decl semantic_available(function:number, node:number, class:number)
.decl semantic_anticipable(function:number, node:number, class:number)
.decl semantic_cse_candidate(function:number, class:number, producer:number, use:number, proof_pair:number)
.decl semantic_pre_candidate(function:number, class:number, placement:number, occurrence:number)
.decl semantic_completion(function:number, dimension:symbol, completion:symbol)

.input semantic_class(IO=file, rfc4180=true, delimiter="\t")
.input semantic_proof(IO=file, rfc4180=true, delimiter="\t")
.input semantic_available(IO=file, rfc4180=true, delimiter="\t")
.input semantic_anticipable(IO=file, rfc4180=true, delimiter="\t")
.input semantic_cse_candidate(IO=file, rfc4180=true, delimiter="\t")
.input semantic_pre_candidate(IO=file, rfc4180=true, delimiter="\t")
.input semantic_completion(IO=file, rfc4180=true, delimiter="\t")
```

These relations consume evidence; they do not prove equality. A derived tuple
retains Datalog provenance and cannot become a Replete/libcpg proof.

## 4. Join with the ordinary CPG EDB

Call `export_datalog(&cpg)` independently and give both bundles to the same
caller-owned engine. Rules may join a CSE candidate's stable `use` node with
`contained_in`, `cfg`, or `declared_type`. Do not merge the Rust types or
mutate the CPG to store derived rows: their completion contracts differ.

## 5. Write and evaluate outside libcpg

`souffle_files()` returns memory buffers. The caller must:

1. choose an approved output directory;
2. use atomic, permission-safe file creation;
3. prevent relation-name path traversal;
4. configure engine memory, swap, CPU, and time limits;
5. record exact rules and engine version; and
6. map engine completion into `DatalogEvaluationCompletion`.

libcpg neither launches a Datalog engine nor opens a GUI.

## 6. Recheck authority at action time

Before a transformation or security decision, replay referenced source proofs
and revalidate required CPG facts against the current graph. CSE/PRE rows are
advisory. They do not prove source rewriting, profitability, whole-program
behavior, alias completeness, or exception-motion safety beyond the model.

## Repository boundary

libcpg already imports standalone Replete. Until Replete is published, this
feature branch uses a sibling path; release migration is a dependency-source
change to an immutable Git revision or exact registry version. No MeTTaIL
modification, build, formatting, branching, or commit is part of this workflow.

## Related documentation

- [API reference](../api/semantic-datalog-reference.md)
- [Component contract](../components/graph/semantic-datalog-boundary.md)
- [ADR-0077](../design/0077-owned-semantic-datalog-evidence.md)
- [Security boundary](../security/35-semantic-datalog-boundary.md)
- [Scientific validation](../scientific/72-semantic-datalog-boundary-validation.md)
- [Theory 57](../theory/57-datalog-egraphs-and-typed-expression-semantics.md)
