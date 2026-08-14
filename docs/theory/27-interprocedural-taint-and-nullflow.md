# Theory 27 — Interprocedural taint and null-flow summaries

This chapter formalizes the T3.3 domains built on the bounded
[IFDS/IDE scheduler](25-ifds-ide-summary-solving.md). **IFDS** means
*Interprocedural, Finite, Distributive, Subset*: an interprocedural data-flow
problem whose finite facts propagate through distributive transfer functions.
**IDE** means *Interprocedural Distributive Environment*: the related framework
whose path edges carry values. libcpg materializes compact per-procedure jump
functions and canonical path evidence rather than a separate exploded graph.

![Data-driven taint summary pipeline](../diagrams/taint-summary-flow.svg)

## 1. Program and fact model

Let the represented program be a CPG $`G=(V,E)`$. For function $`f`$, let
$`V_f\subseteq V`$ be its typed AST scope, excluding nested functions. Let
$`P_f=\{0,\ldots,p_f-1\}`$ be positional formals, $`C_s`$ be open source
classes, and $`C_k`$ be open sink classes.

The taint fact universe is

```math
D_f = \{\operatorname{Param}(i)\mid i\in P_f\}
      \cup
      \{\operatorname{Ambient}(c)\mid c\in C_s\}.
```

The implementation additionally pairs each ambient fact with its concrete
origin $`o\in V_f`$. This refinement is essential: two environment reads of
the same class are distinct candidate sources and require distinct witnesses.

The maybe-null universe replaces ambient classes with typed origins:

```math
N_f = \{\operatorname{Param}(i)\mid i\in P_f\}
      \cup
      \{\operatorname{Origin}(k,o)\mid k\in K_N,\ o\in V_f\},
```

where $`K_N`$ contains literal-null, uninitialized-variable, and
nullable-return evidence.

## 2. Local transfer relation

Define $`u\leadsto_f v`$ when typed CPG evidence permits value flow inside
$`f`$. Admitted evidence includes forward DFG edges, resolved definition-to-use
links, and child-to-parent AST value composition. Calls are transfer barriers:
their result is computed by catalog precedence and callee summaries rather than
blindly unioning all children. This barrier is what makes sanitizer killing
effective.

For ordinary transfer, the may-fact environment $`X:V_f\to 2^{D_f}`$ obeys

```math
X(v) \supseteq \bigcup_{u\leadsto_f v} X(u).
```

All updates are unions, so transfer is monotone. Sanitizers define the constant
empty result function; sources define a singleton ambient fact; resolved calls
substitute actual arguments through current callee return summaries.

## 3. Procedure-summary lattice

A taint summary is abstractly

```math
S_f=(K_f,R_f,A_f,W_f,T_f,B_f),
```

where:

- $`K_f\subseteq P_f\times C_k`$ is parameter-to-sink reachability;
- $`R_f\subseteq P_f`$ is parameter-to-return reachability;
- $`A_f\subseteq C_s`$ is source-class-to-return reachability;
- $`W_f`$ maps semantic relations to canonical witnesses;
- $`T_f`$ records fixed-width parameter-mask truncation; and
- $`B_f`$ records cap evidence.

Semantic coordinates join by set union or Boolean disjunction. Witnesses for a
fixed semantic key join by selecting the minimum under one total order. Thus

```math
S_1\sqcup S_2 =
(K_1\cup K_2,
 R_1\cup R_2,
 A_1\cup A_2,
 \min_{\prec}(W_1,W_2),
 T_1\lor T_2,
 B_1\lor B_2).
```

Each coordinate is associative, commutative, and idempotent. Their product is
a finite-height join-semilattice, which is the algebra required by the shared
recursive SCC solver.

## 4. Call composition

Suppose call site $`s`$ invokes $`g`$ with actual arguments
$`a_0,\ldots,a_n`$. For any caller fact $`d\in X_f(a_i)`$:

1. if $`(i,k)\in K_g`$, then $`d`$ reaches sink class $`k`$ through $`s`$;
2. if $`i\in R_g`$, then $`d`$ reaches the result of $`s`$; and
3. if $`c\in A_g`$, an ambient fact of class $`c`$ reaches the result of
   $`s`$.

The same substitution composes maybe-null parameter-to-dereference and
parameter-to-return relations. A concrete callee source-to-return witness
retains its original source node when extended into the caller.

![Canonical source-to-sink summary composition](../diagrams/taint-witness-sequence.svg)

## 5. Least fixed point over recursion

Let $`F_f(S)`$ recompute function $`f`$ from local transfer and current callee
summaries. For a recursive call SCC, libcpg starts at bottom and evaluates

```math
S_f^{(k+1)} = S_f^{(k)} \sqcup F_f(S^{(k)}).
```

Because $`F_f`$ is monotone and the semantic domain is finite, unconstrained
Kleene iteration reaches the least fixed point. `IfdsCaps::max_scc_iters`
provides an operational ceiling; reaching it returns a marked partial prefix
instead of asserting convergence.

## 6. Canonical witnesses

For one semantic flow key, witness $`w_1`$ is preferred to $`w_2`$ by:

1. uncapped before capped;
2. higher minimum evidence confidence;
3. fewer nodes; then
4. lexicographically smaller `NodeId` sequence.

This is a total order. Its minimum is stable under insertion permutation and
safe inside lattice join. A composed witness concatenates caller prefix and
callee summary suffix, suppressing only an identical boundary node.

The confidence of path $`\pi`$ is

```math
\operatorname{conf}(\pi)=\min_{e\in\pi}\operatorname{conf}(e),
```

where evidence edges include catalog matches, call-resolution tiers, external
models, and summary suffixes. This quantity ranks review evidence; it is not a
probability.

## 7. Catalog separation and language independence

The CPG supplies path structure, not source language policy. A bounded typed
walk reconstructs the first callee subtree from `Identifier` and
`MemberAccess` nodes. `Pattern::{Exact,Segment,Suffix,Substring}` then applies
literal policy to that path. No grammar, source buffer, or regular-expression
engine is required.

Separating catalog from transfer has two consequences:

- changing a project policy changes findings without changing graph structure
  or analysis code; and
- the same analysis can consume CPGs built from Rust, Python, JavaScript,
  Rholang, MeTTa, or a caller-defined frontend, provided equivalent typed
  evidence is present.

## 8. Soundness boundary and incompleteness

For an uncapped run, the implementation computes the least may-flow fixed point
for its admitted transfer relation and catalog. This statement does **not**
imply semantic soundness for unrepresented runtime behavior. Reflection,
dynamic dispatch, native code, missing aliases, infeasible paths, and an
incorrect sanitizer catalog remain outside or approximate the model.

Resource caps may under-approximate a may-analysis by omitting facts. Therefore
a capped result permits false negatives and cannot support a negative safety
claim. Positive flows remain candidate evidence, not verdicts, whether capped
or complete.

## 9. Null-flow interpretation

Maybe-null is deliberately separate from injection taint. Null classes are
typed origin reasons rather than catalog strings, and dereference is recognized
from `DfgEdgeKind::Dereference` or unary `*`. A nullable return type introduces
a possible null at the call site only when the typed name is explicitly
nullable. This avoids treating every unknown return as null while preserving
the evidence boundary.

Path feasibility and language-specific null guarantees are not solved here.
Future value-range, points-to, typestate, or System Dependence Graph analyses
may refine the candidate set without changing the summary algebra.

## References

1. Reps, T., Horwitz, S., Sagiv, M. (1995). *Precise Interprocedural Dataflow
   Analysis via Graph Reachability.* DOI:
   [10.1145/199448.199462](https://doi.org/10.1145/199448.199462).
2. Sagiv, M., Reps, T., Horwitz, S. (1996). *Precise Interprocedural Dataflow
   Analysis with Applications to Constant Propagation.* DOI:
   [10.1016/0304-3975(96)00072-2](https://doi.org/10.1016/0304-3975(96)00072-2).
3. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and
   Discovering Vulnerabilities with Code Property Graphs.* DOI:
   [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44).
