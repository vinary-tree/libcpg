# Theory 26 — Interprocedural effect lattices

This chapter formalizes libcpg's constant/effect summary domain. The generic
fixed-point schedule is developed in [Theory 25](25-ifds-ide-summary-solving.md).
Here the question is what one function summary means and why composition is
monotone.

## 1. Summary domain

Let $`\mathcal{P}`$ be the finite represented place set and let
$`\mathcal{K}`$ be scalar constants. A function summary is the product

```math
E = (R,W,Q,M,C,T,B),
```

where $`R,W\subseteq\mathcal{P}`$ are reads and writes; $`Q`$ is purity;
$`M\in\{0,1\}^{64}`$ is parameter-to-return dependence; $`C`$ is the return
constant; $`T`$ records signature truncation; and $`B`$ records bounded
conservative widening.

Purity has the total order

```math
\mathrm{Pure}
\sqsubseteq \mathrm{ReadsGlobal}
\sqsubseteq \mathrm{WritesGlobal}
\sqsubseteq \mathrm{Impure}.
```

The constant coordinate is a flat lattice:

```math
\bot_C \sqsubset c \sqsubset \top_C \qquad (c\in\mathcal{K}).
```

Two equal constants join to themselves; two different constants join to
$`\top_C`$. This is the standard constant-propagation shape used by sparse
conditional constant propagation (SCCP) [[2]](#references).

## 2. Product join

For summaries $`E_1`$ and $`E_2`$, libcpg defines

```math
E_1\sqcup E_2 =
(R_1\cup R_2,
 W_1\cup W_2,
 \max(Q_1,Q_2),
 M_1\lor M_2,
 C_1\sqcup_C C_2,
 T_1\lor T_2,
 B_1\lor B_2).
```

Every coordinate is associative, commutative, and idempotent, so their product
is also a join-semilattice. `EffectSummary::subsumes` implements the matching
product order. Generated tests execute these laws over independently varied
coordinates.

## 3. Delayed place substitution

Let call site $`s`$ invoke function $`g`$ with actual arguments
$`a_0,\ldots,a_n`$. Define substitution $`\sigma_s`$ by

```math
\sigma_s(\mathrm{Param}(i)) = \operatorname{place}(a_i),
```

while globals and fields map to themselves. Then the caller incorporates

```math
R_f' = R_f \cup \sigma_s(R_g),
\qquad
W_f' = W_f \cup \sigma_s(W_g).
```

If $`a_i`$ is a local scalar with no externally observable place, the mapped
set is empty. If it is a global, parameter, or field, the caller acquires that
effect. This is why writing a formal parameter can be locally pure yet make a
particular caller `WritesGlobal`.

## 4. Return dependence

Bit $`i`$ of $`M_f`$ means that some represented return expression of $`f`$
may depend on formal parameter $`i`$. For a returned call, dependence composes
by substituting the callee mask through actual arguments. An unresolved call
may return any argument, so every represented argument dependence is retained.

The mask is a compact finite domain. Signatures wider than 64 parameters set an
explicit truncation flag. When such a truncated callee participates in return
composition, all caller arguments are followed and the aggregate analysis is
marked incomplete rather than silently dropping a possible dependence.

## 5. Constant composition

Let $`\operatorname{eval}(e,S)`$ evaluate expression $`e`$ using current callee
summaries $`S`$. Literals, checked unary/binary operations, assignments, and
def-use definitions reuse SCCP folding. For a resolved call to $`g`$,

```math
\operatorname{eval}(\operatorname{call}(g),S)=C_g.
```

External or unresolved calls produce $`\top_C`$. The function return constant
is the join of all represented return expressions. Consequently a caller can
move monotonically from bottom to a constant and then to overdefined, but can
never move back down.

## 6. Recursive fixed point

Direct effects are present in every candidate summary. Resolved callee effects
are monotone in the current summary map, and every composition operator is
monotone. The shared SCC solver therefore computes the least fixed point for a
finite-height domain, subject to explicit round and work caps [[1]](#references).

At a domain traversal cap, libcpg widens return dependence to all 64 bits and
the constant to $`\top_C`$. This preserves conservative positive evidence and
sets `capped`; clients may not turn missing evidence into a negative verdict.

## 7. Evidence boundary

The abstraction is sound relative to represented CPG evidence, not arbitrary
source semantics. Missing alias, field, def-use, or call-target evidence limits
precision. The analysis does not guess from source spelling. This explicit
boundary makes results reproducible and lets future points-to or resolution
passes improve precision without changing the summary algebra.

## References

1. Reps, T., Horwitz, S., Sagiv, M. (1995). *Precise Interprocedural Dataflow
   Analysis via Graph Reachability.* DOI:
   [10.1145/199448.199462](https://doi.org/10.1145/199448.199462).
2. Wegman, M. N., Zadeck, F. K. (1991). *Constant Propagation with Conditional
   Branches.* DOI:
   [10.1145/103135.103136](https://doi.org/10.1145/103135.103136).
