# Scientific Validation 23 — Resolved Member and Index Flow

This page records the falsifiable evidence for the field/index builder contract
selected by [ADR-0028](../design/0028-evidence-bounded-member-field-resolution.md).
A **member access** is a `MemberAccess { member }` node; a **field declaration**
is a `Field { name }` node owned by a class/struct; and **resolution** means a
direct `Reference` edge from the access to that declaration.

## Hypotheses

| ID | Hypothesis | Refuting observation |
|---|---|---|
| H1 | A complex member/index assignment emits the corresponding write edge from the assignment to its first AST child. | The edge is absent, reversed, or targets another child. |
| H2 | `self.x`/`this.x` resolves only to the unique same-name field of the enclosing class/implemented type. | A foreign receiver, sibling scope, nested function, duplicate type, or duplicate field receives a reference. |
| H3 | `include_field_access = false` suppresses reads, writes, index dependencies, and field references together. | Any gated edge remains. |
| H4 | Extraction is append-only and idempotent. | A node disappears, or a second run changes the edge count. |
| H5 | Storage order cannot change the emitted semantic edge set. | Equivalent fixed-ID graphs produce different canonical edges. |
| H6 | Mode A and Mode B emit identical typed topology for the same tree. | Their node sets or canonical typed edge sets differ. |

![The six expected edges and the conservative resolution boundary](../diagrams/member-field-flow.svg)

*Figure — the exact oracle topology. Source:
[`diagrams/member-field-flow.puml`](../diagrams/member-field-flow.puml).*

## Experiments

### Exact hand-built oracle

`field_and_index_writes_resolve_scoped_field_idempotently` builds a well-formed
`Root -> Module -> Struct/Impl -> Function` AST with both complex l-values. It
asserts exactly the intended endpoint/kind pairs:

```text
self       --FieldRead-------> MemberAccess(x)
assignment --FieldWrite------> MemberAccess(x)
MemberAccess(x) --Reference--> Field(x)
array      --IndexRead-------> IndexAccess
subscript  --DataDependency--> IndexAccess
assignment --IndexWrite------> IndexAccess
```

The same experiment clones the input, disables `include_field_access`, and
requires all six gated families to be absent. It records the original node count,
runs extraction twice, and requires no node change and no second-run edge change.
The first run must add exactly six edges: five counted DFG edges plus the separate
`Reference` edge; this pins both `edge_count()` and `stats().dfg_edges`.

### Negative and ambiguity oracle

`class_field_resolution_is_receiver_local_and_ambiguity_safe` proves three
separating cases: an `Unknown("this")` keyword receiver resolves through a class
body wrapper; `other.x` does not resolve; and a fresh `this.x` does not resolve
after a duplicate `Field("x")` makes the declaration ambiguous. This experiment
would fail a global-name implementation.

`impl_field_resolution_is_scope_bounded_and_method_local` places `struct S` in
two sibling modules and proves that `impl S` resolves only to its same-scope
field. It then proves that a nested function cannot inherit the outer method's
owner and that a second same-scope `S` makes a fresh access unresolved. This
separates lexical identity from coincidental display-name equality.

`field_resolution_rejection_matrix_is_total` covers absent receivers,
non-identifier receivers, empty members, free-function accesses, ownerless and
function-owned fields, missing implementation types, malformed index accesses,
and childless assignments. Every case terminates without a reference or write
that its evidence cannot support.

### Property-based determinism

`prop_field_edges_are_deterministic` runs **256 generated cases**. Each case
varies the type name, field name, `self` versus `this`, and keyword-node versus
identifier-node receiver representation. It builds the same fixed-ID AST twice,
reverses petgraph node insertion order in one copy, extracts both, sorts the
typed edge triples, and compares them to the same six-edge oracle above.

### Parser and public-API integration

- `parsed_self_field_assignment_emits_write_and_reference` and
  `mode_a_rust_self_assignment_resolves_to_declared_field` parse a real Rust
  `struct`/`impl` fixture and require both `FieldWrite` and `Reference`.
- Eleven Mode-A fixtures verify named field declarations for Rust, JavaScript,
  TypeScript, Go, Java, C, and C++; the object-oriented fixtures additionally
  verify `this.x` writes and references for JavaScript, TypeScript, Java, and C++.
- `public_field_access_contract_is_gated_additive_and_idempotent` exercises the
  public feature-free API from `tests/field_access_integration.rs`.
- `test_build_from_tree_matches_build` compares canonical `(source, target,
  edge-kind)` triples for a Rust `self.x = 1` fixture, not only aggregate counts.

## Result and boundary

All experiments pass in the default and maximal feature matrices. The evidence
corroborates H1–H6 for the represented CPG semantics. It does **not** claim full
language type inference: an arbitrary `obj.x`, an external type, an alias, or an
ambiguous/qualified/generic implementation name remains unresolved. This is an
explicit false-negative boundary, not evidence that no declaration exists.

## Coverage checkpoint

The clean maximal stable run records **97.95% line**, **97.89% region**, and
**94.57% function** coverage for `builder/dfg.rs`. The nightly run records
**81.16% branch coverage** (112 of 138 outcomes), above the campaign's 90% line
and 80% branch thresholds. Repository totals are **98.27% line**, **98.17%
region**, **98.37% function**, and **87.16% branch** coverage. Coverage is
supporting evidence for executed decisions; the exact edge and negative oracles
remain the semantic evidence.

No external scientific claim is used; the oracle is the edge contract itself,
so this validation has no citation or DOI requirement.
