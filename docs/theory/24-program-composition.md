# Theory 24 — Deterministic multi-file program composition

This chapter defines how independent per-file Code Property Graphs (CPGs)
become one program CPG without identifier collision, dangling references, or
unsupported import topology. The construction is an identity-preserving
renaming followed by a conservative evidence relation; it is not source parsing
or package resolution.

## 1. Per-file graphs and disjoint identity

Let file $`i`$ contain a finite CPG
$`G_i = (V_i, E_i)`$. Stable identifiers are local to $`G_i`$, so two files may
both contain `NodeId(0)`. A program graph cannot use the raw union
$`\bigcup_i V_i`$ because equal numeric ids would collapse distinct syntax.

For every file in input order, composition constructs an injective renaming

```math
\mu_i : V_i \hookrightarrow V_P
```

whose range is disjoint from every earlier range. `add_node` supplies the next
fresh program id, while ascending source ids make $`\mu_i`$ deterministic.
Consequently,

```math
\lvert V_P \rvert = \sum_i \lvert V_i \rvert
```

and every program node has exactly one owning `FileScope`.

## 2. Homomorphic edge and reference translation

Every source edge $`(u,v,k) \in E_i`$ becomes
$`(\mu_i(u),\mu_i(v),k)`$ with a fresh program `EdgeId`. Therefore edge kind,
direction, and label are preserved while identity is renewed:

```math
\lvert E_P^{\mathrm{source}} \rvert = \sum_i \lvert E_i \rvert
```

for well-formed input graphs. Import/export edges are a later overlay and are
not part of this source-edge equality.

Node payloads also contain references. Define the partial lifted translation

```math
\widehat{\mu_i}(x) =
\begin{cases}
\mu_i(x) & x \in V_i,\\
\bot & x \notin V_i.
\end{cases}
```

Optional parents, call targets, and identifier definitions use
$`\widehat{\mu_i}`$ directly. Child and capture sequences retain, in order,
only members with a defined image. This fail-closed rule prevents a malformed
source id from accidentally resolving to an unrelated existing program node.

## 3. Append-only composition

For destination graph $`D`$ and source $`S`$, `merge_from` satisfies:

```math
V_D \subseteq V_{D'}
\qquad\text{and}\qquad
E_D \subseteq E_{D'},
```

where $`D'`$ is the result. No destination node or edge is removed. Applying the
operation to an equal snapshot intentionally creates a disjoint second copy, so
node and edge counts double. Merge is not idempotent; import-overlay construction
is idempotent.

## 4. File alias evidence

Let $`A_f`$ be the finite set of token sequences derived from file $`f`$'s
extensionless path suffixes and module names. Let $`T_p`$ be the token sequence
of import path $`p`$. A file match has strength equal to the length of the
longest $`a \in A_f`$ occurring contiguously in $`T_p`$:

```math
s(f,p) = \max\{\lvert a\rvert \mid a \in A_f \land a
\text{ occurs contiguously in } T_p\}.
```

The importing file is excluded. Let $`s^* = \max_f s(f,p)`$ over positive
scores. The classification is:

```math
\operatorname{resolve}(p) =
\begin{cases}
\mathrm{External} & \text{if no positive } s^* \text{ exists},\\
\mathrm{Exact}(f) & \text{if exactly one } f \text{ has } s(f,p)=s^*,\\
\mathrm{Ambiguous} & \text{otherwise}.
\end{cases}
```

This is conservative in the important direction: ambiguity produces evidence
but no dependency topology.

## 5. Two import granularities

An exact match adds two complementary relations:

1. file-root dependency $`r_a \xrightarrow{Imports} r_b`$, consumed by module
   projections and architecture analyses; and
2. import-statement evidence $`p \xrightarrow{Imports[b]} d`$ for selected
   declarations $`d`$ under local binding $`b`$, consumed by lexical call
   resolution.

Exports use $`r_b \xrightarrow{Exports} d`$. All endpoints are existing program
nodes, preserving the shared-node overlay discipline. The `ImportGraph` result
does not compete with these edges: it records why they were or were not emitted.

## 6. Determinism and limits of interpretation

The public boundary is ordered: files retain input order; nodes, declarations,
imports, candidates, dependencies, and results are sorted by stable ids or file
indices. Hash-map iteration is never an output ordering source.

An `Exact` file alias is exact only for the represented CPG evidence. It does not
prove that a compiler accepts the import, that conditional compilation selects
the file, or that a package exists outside the program graph. Similarly,
`Exports` denotes a file-namespace declaration represented in the CPG; it is not
a complete language-specific visibility proof. Cross-file call topology still
requires the evidence boundary in [call resolution](../components/graph/call-resolution.md).

## 7. Worked trace

Suppose `app.rs` imports `crate::math::add`, while `math.rs` and `rival.rs` both
declare `add`.

```text
path aliases:
    math.rs  -> [math]
    rival.rs -> [rival]

import tokens: [use, crate, math, add]
strongest file match: math.rs
selected declaration: math::add

emitted:
    Imports(app_root, math_root)
    Exports(math_root, math_add)
    Imports(import_node, math_add)[add]

call add(): ExactViaImport -> math_add
```

The duplicate `rival::add` remains in the global symbol index but cannot make
the represented import ambiguous, because the import-to-declaration relation
already selected one target.

## See also

- [Component guide](../components/graph/program-composition.md)
- [ADR-0031](../design/0031-deterministic-program-composition.md)
- [Scientific validation 26](../scientific/26-program-composition-validation.md)
- [Usage 07](../usage/07-multi-file-programs.md)

## References

The construction is specified by `libcpg`'s identifier and evidence contracts,
not by an external algorithm. The repository-level theoretical foundation for
the shared-node overlay model remains [Theory 01](01-code-property-graphs.md).
