# Theory 48 -- Datalog and relational CPG semantics

Datalog is a finite relational language well suited to code property graph
(CPG) queries whose result is a monotone consequence of represented facts.
This chapter defines that claim, its compositional interpretation, and its
boundary with equality saturation.

## 1. From a typed multigraph to relations

Let a represented CPG be

```math
G=(V,E,s,t,k,\ell),
```

where $`V`$ is the finite node set, $`E`$ is the finite
edge set, $`s,t:E\to V`$ are source and target, $`k`$
is the edge kind, and $`\ell`$ is an optional label. The exporter
maps each typed edge family to a relation. For example,

```math
R_{\mathrm{dfg}}
=\{(s(e),t(e),k(e))\mid e\in E\land k(e)\text{ is data flow}\}.
```

The relation-specific EDBs use set semantics. A separate master relation

```math
R_E\subseteq E\times V\times V\times K\times D\times L
```

retains identity, detail, and label, so parallel CPG edges are not lost.

## 2. Least fixed-point meaning

An EDB contains input facts. IDB rules define derived facts. For a positive
Datalog program $`P`$, let $`T_P`$ be its
immediate-consequence operator over the finite lattice of possible facts.
Because $`T_P`$ is monotone, the program meaning is its least fixed
point:

```math
\operatorname{lfp}(T_P)
=\bigcup_{i\ge 0}T_P^i(\varnothing).
```

The finite domain guarantees termination of this ascending chain. libcpg does
not implement $`T_P`$: the external Datalog engine owns rule
validation, planning, recursion, and resource policy. libcpg's proof obligation
is the canonical, exact EDB boundary.

## 3. Composition and the category of relations

Category theory supplies a practical vocabulary for binary graph relations.
Treat node domains as objects and binary relations as morphisms in the
category **Rel**. The identity is the diagonal relation
$`I_V=\{(v,v)\mid v\in V\}`$. Composition is existential join:

```math
S\circ R
=\{(x,z)\mid\exists y,\;(x,y)\in R\land(y,z)\in S\}.
```

Thus a data-flow step followed by a control-dependence step is ordinary
relational composition. Datalog syntax makes these compositions explicit and
reusable. Union gives alternative paths; recursive rules compute transitive
closure as a least fixed point.

This categorical view is useful because it identifies laws that integrations
may test: identity, associativity of exact relation composition, and
distributivity over union. It does not imply that every libcpg analysis should
be expressed in category-theory terminology, nor does it make a bounded or
incomplete relation lawful to reassociate as though it were complete.

## 4. Datalog and e-graphs solve different problems

Datalog grows a set of consequences from relational facts. An e-graph grows
equivalence classes under rewrite rules and extracts representatives under a
cost model. A points-to closure, reachability query, or custom CWE relation is
naturally Datalog. Expression equivalence and rewrite-space optimization are
naturally e-graph problems.

The two can compose: a Datalog query may select candidate expression regions,
an e-graph may explore equality-preserving rewrites, and exact libcpg evidence
may confirm the result. Conflating them would lose their different proof
obligations. The current FT.1 surface therefore exports relations only;
equality-saturation integration belongs to the separately tracked Dovetail
follow-up epic.

## 5. Stable translation laws

Let $`F`$ be the CPG-to-EDB translation. For two observations of
the same graph,

```math
G_1=G_2\Longrightarrow F(G_1)=F(G_2)
```

byte for byte after canonical rendering. A stable identity-preserving insertion
permutation changes neither side. `cpg_edge` also admits an exact
left inverse on represented edge records:

```math
\operatorname{decodeEdges}(F(G))=E.
```

These are the executable differential and property-test oracles. They do not
claim source-language semantic completeness.

## References

1. Bravenboer, M., Smaragdakis, Y. (2009). *Strictly Declarative Specification
   of Sophisticated Points-to Analyses.* OOPSLA. DOI:
   [10.1145/1640089.1640108](https://doi.org/10.1145/1640089.1640108).
2. Jordan, H., Scholz, B., Subotić, P. (2016). *Soufflé: On Synthesis of Program
   Analyzers.* CC. DOI:
   [10.1007/978-3-319-41540-6_23](https://doi.org/10.1007/978-3-319-41540-6_23).
