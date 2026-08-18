# CK, LCOM, and function metrics

`libcpg` exposes feature-free, read-only metrics for two related questions:

1. how a nominal type's methods depend on its fields, inheritance, and resolved
   calls; and
2. how much operator vocabulary and control-flow nesting one function's CPG
   scope contains.

The first family is the Chidamber–Kemerer (CK) suite plus Lack of Cohesion in
Methods variants LCOM1 through LCOM5. The second family is Halstead volume,
McCabe cyclomatic complexity, Campbell cognitive complexity, and the
Oman–Hagemeister maintainability index (MI). A **nominal type** is a named
`Class`, `Struct`, `Enum`, or `Trait` declaration. A **method-field graph** is a
bipartite graph whose declaration partitions are methods and fields and whose
links retain the concrete access-expression node.

![Resolved CPG evidence flows into class-level and function-level metric reports.](../../diagrams/code-metrics-pipeline.svg)

*Figure 1 — complete metrics pipeline. Source:
[`code-metrics-pipeline.puml`](../../diagrams/code-metrics-pipeline.puml).*

## Public API

```rust
use libcpg::{
    ck_metrics, function_metrics, method_field_graph, AnalysisError,
    CkOptions, CodePropertyGraph,
};

fn inspect(cpg: &CodePropertyGraph) -> Result<(), AnalysisError> {
    for class in ck_metrics(cpg, &CkOptions::default()) {
        println!(
            "{:?}: WMC={} RFC={} LCOM4={}",
            class.class, class.wmc, class.rfc, class.lcom.lcom4,
        );

        let evidence = method_field_graph(cpg, class.class);
        assert_eq!(evidence.lcom(), class.lcom);
        for access in evidence.accesses {
            println!(
                "method {:?} accesses field {:?}: read={} write={}",
                access.method, access.field, access.reads, access.writes,
            );
        }
    }

    for function in cpg.functions() {
        let metrics = function_metrics(cpg, function.id)?;
        println!(
            "{}: cyclomatic={} cognitive={} volume={:.2} MI={:.2}",
            function.name().unwrap_or("<anonymous>"),
            metrics.cyclomatic_complexity,
            metrics.cognitive_complexity,
            metrics.halstead.volume,
            metrics.maintainability_index,
        );
    }
    Ok(())
}
```

No Cargo feature is required. Feature `serde` serializes all public options and
reports. `function_metrics` returns `AnalysisError::UnknownNode` or
`AnalysisError::NotAFunction` for an invalid selector. `method_field_graph`
instead returns an empty graph carrying the requested ID for a missing or
non-nominal selector; this keeps batch class discovery total.

## Evidence contract

The analysis does not infer a field from spelling. It admits a method-field link
only when the same access node has both kinds of evidence:

- `access -> Field` as `Reference`; and
- a touching `DataFlow(FieldRead)` or `DataFlow(FieldWrite)` edge.

The [member-field builder contract](../../design/0028-evidence-bounded-member-field-resolution.md)
creates these edges for uniquely resolved `self.x` and `this.x`. An unresolved
`other.x`, an ambiguous declaration, or an access without typed DFG evidence
contributes no cohesion link. This creates a false-negative bias instead of
inventing a false relation.

For Rust-style sibling implementation blocks, `Impl { for_type: T }` contributes
methods only when `T` identifies exactly one nominal declaration in the same
root/module declaration scope. Fields remain owned by the nominal declaration;
an implementation block cannot introduce a field partition.

`MethodFieldGraph` exposes four canonical vectors:

| Field | Meaning |
|---|---|
| `methods` | Direct method IDs, including uniquely associated sibling implementations. |
| `fields` | Direct field-declaration IDs. |
| `accesses` | Sorted `(method, access, field, reads, writes)` evidence. |
| `method_calls` | Sorted resolved calls whose caller and callee are both methods of the type. |

Every vector is sorted by stable `NodeId`; repeated CPG storage orders therefore
produce equal reports.

## CK metrics

Let $`C`$ be a class-like type, $`M_C`$ its method set, $`c(m)`$ the cyclomatic
complexity of method $`m`$, and $`R(m)`$ the set of directly resolved callees of
$`m`$. The implementation reports:

```math
\operatorname{WMC}(C)=\sum_{m\in M_C}c(m)
```

```math
\operatorname{RFC}(C)=\left|M_C\cup\bigcup_{m\in M_C}R(m)\right|
```

The other fields are:

| Metric | `libcpg` semantics |
|---|---|
| DIT | Longest parent-component path through selected `Inherits` edges; a root has depth zero. Inheritance cycles are collapsed before the longest-path pass. |
| NOC | Number of distinct immediate incoming children whose inheritance edge targets the class. |
| CBO | Number of distinct other nominal owners connected by a resolved call in either direction or by a CPG type edge in either direction. |

`CkOptions::default()` emits `Class` and `Struct` rows, excludes `Enum` rows,
and does not treat `Implements` as inheritance. `include_enums` and
`include_implements` make those choices explicit. Traits/interfaces remain
possible coupling and type-edge endpoints, but do not emit concrete CK rows.

This is a static structural interpretation of the CK suite introduced by
Chidamber and Kemerer in 1991 and refined in 1994. Call-dependent CBO and RFC
consume the same resolved-call semantics as `call_graph_projection`; unresolved
calls do not invent coupling. See the [call-resolution contract](call-resolution.md).

## LCOM1 through LCOM5

For each method $`m`$, let $`F(m)`$ be the set of resolved fields it reads or
writes. Partition unordered distinct method pairs into:

```math
P=\{\{m_i,m_j\}\mid F(m_i)\cap F(m_j)=\varnothing\}
```

```math
Q=\{\{m_i,m_j\}\mid F(m_i)\cap F(m_j)\ne\varnothing\}
```

The pair-count variants are:

```math
\operatorname{LCOM}_1=|P|
```

```math
\operatorname{LCOM}_2=\max(|P|-|Q|,0)
```

For the graph variants, define $`G_F`$ with one vertex per method and an
undirected edge whenever two methods share at least one field. Define $`G_C`$
with an undirected edge for every resolved internal call. If $`\kappa(G)`$
denotes the number of connected components of $`G`$, then:

```math
\operatorname{LCOM}_3=\kappa(G_F)
```

```math
\operatorname{LCOM}_4=\kappa(G_F\cup G_C)
```

LCOM3 is the Li–Henry connected-set reformulation; LCOM4 is the
Hitz–Montazeri extension that admits calls as cohesion evidence. Isolated
methods each form a component. A type with no methods has both values zero.

For LCOM5, let $`m=|M_C|`$, $`a`$ be the number of declared fields, and
$`\mu(f)`$ the number of methods accessing field $`f`$. The Henderson-Sellers
normalization is:

```math
\operatorname{LCOM}_5=
\frac{m-\frac{1}{a}\sum_f\mu(f)}{m-1}
```

The implementation defines LCOM5 as zero when $`m<2`$ or $`a=0`$ and clamps
rounding noise to $`[0,1]`$. Zero means every method accesses every declared
field; one means each field is, on average, isolated to one method. This is
structural evidence, not proof that the type has one or several business
responsibilities.

## Function metrics

`function_metrics` walks the function's AST scope iteratively and stops before
nested functions. It classifies semantic `CpgNodeKind` values rather than
lexing source text, so it works when source retention is disabled and across
all frontends that normalize to the common node vocabulary.

### Cyclomatic complexity

McCabe's cyclomatic complexity counts independent control-flow paths. For the
structured CPG vocabulary, `libcpg` uses the pgmcp-compatible decision form:

```math
v(G)=1+d
```

Here $`d`$ counts `If`, `While`, `For`, `Loop`, and `Catch`; short-circuit
`&&`, `||`, `??`, `and`, and `or`; and every direct `MatchArm` beyond the first
for a `Match`. The result is at least one and uses saturating `u32` arithmetic.

### Cognitive complexity

Cognitive complexity measures nesting burden rather than independent paths.
Each `If`, loop, `Match`, or `Catch` contributes $`1+n`$, where $`n`$ is the
current structural nesting depth. A short-circuit logical operator, `Break`,
`Continue`, or resolved self-recursive call contributes one. Nested functions
are independent scopes. These rules follow Campbell's published overview and
the shared pgmcp scoring kernel; they intentionally remain an inspectable
language-neutral subset rather than pretending to model every language's
surface syntax.

### Halstead metrics

Let $`\eta_1`$ and $`\eta_2`$ be the numbers of distinct operators and operands,
and $`N_1`$ and $`N_2`$ their total occurrences. `HalsteadMetrics` stores these
four counts plus:

```math
\eta=\eta_1+\eta_2,\qquad N=N_1+N_2
```

```math
V=N\log_2\eta
```

```math
D=\frac{\eta_1}{2}\frac{N_2}{\eta_2},\qquad E=DV,\qquad B=\frac{V}{3000}
```

Volume $`V`$ is zero when the vocabulary is empty. Difficulty $`D`$ is zero
when either distinct partition is empty. Every derived value is therefore
finite for any finite CPG.

The semantic classifier treats control constructs, expression operators,
calls, member/index access, lambdas, await/yield, and named macros as
operators. Parameters, variables, fields, identifiers, member names, literals,
generic parameters, and type annotations are operands. Declarations and uses
of the same symbol spelling share one operand key. Literal kinds and values
remain distinct. Delimiters and discarded grammar tokens are not counted, so
the values are comparable between CPGs built under the same normalization
contract, not byte-for-byte interchangeable with a token-level Halstead tool.

### Maintainability index

Let $`L`$ be the inclusive function source-line span, $`C`$ the merged number
of distinct comment lines inside that span, $`V`$ Halstead volume, and $`v(G)`$
cyclomatic complexity. The pgmcp-compatible SEI form is:

```math
\operatorname{MI}=171-5.2\ln(\max(V,1))-0.23v(G)
-16.2\ln(\max(L,1))+50\sin\sqrt{2.4C/L}
```

The result is clamped directly to $`[0,100]`$. This is the raw-then-clamped
variant used by pgmcp, not the alternative formula that first divides by 171.
Comment intervals are clipped to the function and merged without allocating
one element per source line. If comments were excluded during CPG construction,
$`C=0`$.

## Literate algorithm

```text
build_method_field_graph(C):
    collect direct methods and fields below C, stopping at nested declarations
    attach sibling Impl(T) methods only when T resolves uniquely beside C
    for each method m:
        visit m's function scope, stopping before nested functions
        for each access a with a --Reference--> owned Field f:
            inspect typed DFG edges touching a
            if FieldRead or FieldWrite exists: retain (m, a, f, read, write)
    retain resolved call pairs whose endpoints are both methods of C
    sort every public vector by stable NodeId

compute_ck(C):
    WMC = sum function cyclomatic values for C's methods
    collapse inheritance cycles and compute longest root distance
    count immediate children
    aggregate distinct external call/type owners for CBO
    union own methods with direct resolved callees for RFC
    compute LCOM1-5 from the method-field graph

compute_function_metrics(f):
    obtain f's finite AST scope, excluding nested functions
    count structured decisions and cognitive increments
    classify semantic operator and operand keys
    merge clipped comment-line intervals
    evaluate Halstead and MI formulae with defined zero cases
```

## Determinism, complexity, and limits

All traversals are bounded by the finite input CPG. The analysis has no
exponential search, convergence loop, recursion, I/O, or graph mutation.
Ordered maps/sets and final stable-ID sorting remove petgraph insertion-order
effects. Inheritance cycles are condensed before DIT calculation, preventing a
malformed cycle from manufacturing unbounded depth.

For $`N`$ CPG nodes, $`E`$ edges, $`M`$ methods in one class, $`F`$ fields, and
$`A`$ retained accesses, one method-field report takes $`O(N+E)`$ evidence
scanning plus $`O(M^2+MF)`$ LCOM work. A full CK report repeats ownership and
field construction per nominal type in the current simple implementation; its
worst case is $`O(T(N+E+M^2+MF))`$ for $`T`$ nominal types. Function metrics are
$`O(N_f+E_f)`$ for the selected function scope and use $`O(N_f)`$ temporary
space. Callers handling hostile repository-scale graphs should cap graph size
at ingestion; results are never partial or silently truncated.

## Interpretation boundary

Metrics summarize represented structure. They do not establish defects,
maintainability, comprehension time, or architectural quality by themselves.
In particular:

- an unresolved field or call lowers observed coupling/cohesion evidence;
- generated accessors can increase method counts while still connecting LCOM4;
- inherited methods are not copied into a child's method set;
- dynamic dispatch contributes only when the CPG records a resolved target;
- semantic Halstead counts depend on frontend normalization; and
- MI is a comparative index, not a calibrated probability of maintenance cost.

Retain the CPG, builder/resolver configuration, and `CkOptions` beside any
stored report. Compare values only under the same evidence policy.

See [Theory 22](../../theory/22-object-oriented-and-function-metrics.md),
[ADR-0029](../../design/0029-evidence-bounded-code-metrics.md), and
[Scientific validation 24](../../scientific/24-code-metrics-validation.md).

## References

1. McCabe, T. J. (1976). *A Complexity Measure*. DOI:
   [10.1109/TSE.1976.233837](https://doi.org/10.1109/TSE.1976.233837).
2. Chidamber, S. R., and Kemerer, C. F. (1991). *Towards a Metrics Suite for
   Object Oriented Design*. DOI:
   [10.1145/117954.117970](https://doi.org/10.1145/117954.117970).
3. Chidamber, S. R., and Kemerer, C. F. (1994). *A Metrics Suite for Object
   Oriented Design*. DOI:
   [10.1109/32.295895](https://doi.org/10.1109/32.295895).
4. Li, W., and Henry, S. (1993). *Object-Oriented Metrics that Predict
   Maintainability*. DOI:
   [10.1016/0164-1212(93)90077-B](https://doi.org/10.1016/0164-1212(93)90077-B).
5. Hitz, M., and Montazeri, B. (1995). *Measuring Coupling and Cohesion in
   Object-Oriented Systems*. Proceedings of the International Symposium on
   Applied Corporate Computing. [Author-hosted paper](https://www.isys.uni-klu.ac.at/PDF/1995-0043-MHBM.pdf).
   No DOI was assigned.
6. Henderson-Sellers, B. (1996). *Object-Oriented Metrics: Measures of
   Complexity*. Prentice Hall PTR. ISBN 0-13-239872-9. No DOI was assigned.
7. Halstead, M. H. (1977). *Elements of Software Science*. Elsevier/North-Holland.
   ISBN 0-444-00205-7. No DOI was assigned.
8. Campbell, G. A. (2018). *Cognitive Complexity: An Overview and Evaluation*.
   DOI: [10.1145/3194164.3194186](https://doi.org/10.1145/3194164.3194186).
9. Oman, P., and Hagemeister, J. (1992). *Metrics for Assessing a Software
   System's Maintainability*. DOI:
   [10.1109/ICSM.1992.242525](https://doi.org/10.1109/ICSM.1992.242525).
10. Coleman, D., Ash, D., Lowther, B., and Oman, P. (1994). *Using Metrics to
    Evaluate Software System Maintainability*. DOI:
    [10.1109/2.303623](https://doi.org/10.1109/2.303623).
