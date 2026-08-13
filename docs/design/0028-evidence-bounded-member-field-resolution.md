# ADR-0028 — Evidence-Bounded Member-to-Field Resolution

- **Status:** Accepted
- **Date:** 2026-08-13
- **Decision owners:** `libcpg` maintainers
- **Scope:** `DfgExtractor::build_field_access_edges` and declarative-field normalization

## Context

The [Data Flow Graph (DFG)](../GLOSSARY.md#data-flow-graph-dfg) already represented
member and index reads, but an assignment such as `self.x = value` carried neither
a `FieldWrite` edge nor a link from the `MemberAccess("x")` node to the
`Field("x")` declaration. Downstream object-oriented metrics therefore had to
approximate which methods touched which fields.

The missing topology cannot be repaired by matching every member name globally.
Two unrelated types can both declare `x`, and an unresolved `other.x` provides no
evidence that `other` is the enclosing instance. A useful builder pass must add
real edges while remaining conservative when the shared, language-agnostic
[CPG](../GLOSSARY.md#code-property-graph-cpg) lacks type evidence.

## Decision

Keep the existing `include_field_access` gate and make its output symmetric for
reads and writes:

| Syntax shape | Emitted edge |
|---|---|
| member access receiver | `receiver -> MemberAccess` as `DataFlow(FieldRead)` |
| assignment whose first AST child is a member access | `Assignment -> MemberAccess` as `DataFlow(FieldWrite)` |
| index receiver | `receiver -> IndexAccess` as `DataFlow(IndexRead)` |
| index expression | `subscript -> IndexAccess` as `DataFlow(DataDependency)` |
| assignment whose first AST child is an index access | `Assignment -> IndexAccess` as `DataFlow(IndexWrite)` |

For a `MemberAccess { member: m }`, emit
`MemberAccess -> Field(m)` as `Reference` only when all of the following evidence
is present:

1. the first AST child is the receiver keyword or identifier `self`/`this`;
2. the nearest function ancestor reaches an enclosing `Class` or `Impl`
   without crossing a second function boundary; for `Impl { for_type: T }`,
   the exact nominal type declaration must be unique in the same lexical
   declaration scope;
3. exactly one `Field { name: m }` exists below that owner, descending through
   grammar wrappers but not through nested functions or nominal types.

Missing, qualified/generic, external, or ambiguous evidence produces no
`Reference`. The pass does not parse a type string, infer aliases, or guess the
runtime type of arbitrary receivers. Every edge is added with `connect_unique`,
so a repeated extraction is a no-op.

![Read, write, and declaration-reference edges for member and index assignments](../diagrams/member-field-flow.svg)

*Figure — orange edges are DFG flow; the grey edge is the conservative
member-to-declaration reference. Source:
[`diagrams/member-field-flow.puml`](../diagrams/member-field-flow.puml).*

## Literate algorithm

Let $`a`$ be an assignment, $`t(a)`$ its first AST child, $`r(m)`$ the first
child of member access $`m`$, and $`F(o,n)`$ the set of fields named $`n`$
owned by type $`o`$. The write rules are:

```math
t(a) \in \mathit{MemberAccess}
\Longrightarrow (a,t(a),\mathit{FieldWrite})
```

```math
t(a) \in \mathit{IndexAccess}
\Longrightarrow (a,t(a),\mathit{IndexWrite})
```

Resolution is intentionally a partial function:

```math
\mathit{resolve}(m)=
\begin{cases}
f, & r(m)\in\{\mathtt{self},\mathtt{this}\}
     \land \mathit{owner}(m)=o
     \land F(o,\mathit{name}(m))=\{f\} \\
\bot, & \text{otherwise.}
\end{cases}
```

The implementation follows this procedure:

```text
for each collected member access m:
    emit receiver(m) --FieldRead--> m
    if receiver(m) is self/this:
        method = nearest Function ancestor
        owner = first Class/Impl ancestor above method, stopping at another Function
        if owner is Impl(T): owner = unique exact T in the same declaration scope
        field = unique same-name Field below owner, excluding nested declarations
        if field exists: emit m --Reference--> field

for each collected index access i:
    emit receiver(i) --IndexRead--> i
    emit subscript(i) --DataDependency--> i

for each collected assignment a:
    inspect only its first AST child
    emit FieldWrite or IndexWrite when that child is the corresponding access kind
```

## Frontend normalization

The resolver consumes `CpgNodeKind`, not grammar syntax. Parser-backed tests
verify named `Field` declarations for every Mode-A frontend with explicit
struct/class field syntax: Rust, JavaScript, TypeScript, Go, Java, C, and C++.
Go now maps `field_declaration`; Java and C/C++ extract the identifier below a
declarator; JavaScript's `property` and TypeScript's `name` grammar slots are
normalized by one helper. Python and Ruby fields are generally implicit runtime
attributes rather than declaration nodes, so no declaration reference is
invented for them.

## Consequences

- Method-to-field analyses can consume real `FieldRead`/`FieldWrite` plus
  `Reference` topology.
- The change is append-only: it adds no nodes and removes no edges.
- `stats().dfg_edges` grows for complex assignment l-values; `Reference` remains
  outside the DFG count because it is a distinct `CpgEdgeKind` family.
- Mode A and Mode B remain shape-equivalent because both execute the same mapper
  and DFG post-parse pipeline.
- Exact matching leaves some valid programs unresolved (aliases, qualified or
  generic `Impl` names, arbitrary receiver variables). This false-negative bias
  is preferable to a false declaration edge.

## Alternatives rejected

1. **Global member-name matching.** Rejected because same-named fields in
   unrelated types would create false references.
2. **Parse type strings inside the DFG pass.** Rejected because it would duplicate
   frontend grammars and still guess through aliases.
3. **Create a new edge kind.** Rejected because `Reference` already expresses a
   use-to-declaration relation and existing consumers understand it.
4. **Synthesize implicit dynamic-language fields.** Rejected here because it is
   a separate language-semantic decision, not evidence present in the common AST.

## Validation

The [validation ledger](../scientific/23-member-field-flow-validation.md) records
hand-built, parser-backed, integration, idempotency, gate, ambiguity, and
256-case insertion-order experiments. No external algorithm or empirical result
is used by this decision, so there is no external citation or DOI to verify.
