# Theory 25 — IFDS/IDE procedure-summary fixed points

This chapter develops the mathematical contract behind libcpg's generic
interprocedural summary solver. **Interprocedural Finite Distributive Subset
(IFDS)** analysis reduces finite-fact, distributive interprocedural data flow
to graph reachability [[1]](#references). **Interprocedural Distributive
Environment (IDE)** analysis generalizes the propagated value from membership
to a distributive environment transformer [[2]](#references). libcpg factors
out the shared whole-program summary schedule while leaving local flow
functions to a domain.

## 1. Program and call-graph model

Let the represented functions be the finite set $`\mathcal{F}`$. Definite call
evidence induces a directed graph
$`G_F=(\mathcal{F},E_F)`$, where $`(f,g)\in E_F`$ means function $`f`$ may call
represented function $`g`$. The edge therefore points from caller to callee.
External and unresolved call sites are evidence values but do not invent
vertices in $`\mathcal{F}`$.

The strongly-connected components (SCCs) of $`G_F`$ partition
$`\mathcal{F}`$. Contracting each SCC gives a directed acyclic graph (DAG)
$`G_C`$. If $`A\to B`$ in $`G_C`$, a summary in $`A`$ may depend on summaries
in $`B`$. A valid evaluation order must therefore visit $`B`$ before $`A`$:
it is reverse topological with respect to caller-to-callee edges.

## 2. Summary lattice

Let $`(L,\sqsubseteq,\sqcup,\bot)`$ be a join-semilattice with bottom.
For $`x,y\in L`$:

- $`x\sqsubseteq y`$ means $`y`$ contains at least the information in $`x`$;
- $`x\sqcup y`$ is their least upper bound; and
- $`\bot\sqsubseteq x`$ for every $`x`$.

The Rust predicate `a.subsumes(b)` represents $`b\sqsubseteq a`$. The mutating
operation `a.join(b)` replaces $`a`$ with $`a\sqcup b`$ and returns whether the
semantic value changed.

For each function $`f`$, a domain defines a monotone summary functional
$`\Phi_f:L^{\mathcal{F}}\to L`$. Its argument is the current map of every
represented function summary; its result incorporates local flow plus the
currently known callee effects. Monotonicity requires

```math
S \sqsubseteq T \Longrightarrow \Phi_f(S) \sqsubseteq \Phi_f(T),
```

where the map order is pointwise.

## 3. Acyclic components

For an acyclic singleton $`\{f\}`$, no self-call exists. Every represented
callee belongs to a component already evaluated by the reverse-topological
schedule. The final summary is therefore computed once:

```math
S_f = \bot \sqcup \Phi_f(S).
```

No global iteration is necessary. This is why component order is semantic,
not merely a performance choice.

## 4. Recursive components

Let recursive component $`C=(f_1,\ldots,f_m)`$ be ordered by ascending stable
`NodeId`. Initialize $`S_f^{(0)}=\bot`$ for every $`f\in C`$. During round
$`k+1`$, visit members sequentially and update

```math
S_{f_i}^{(k+1)}
=
S_{f_i}^{(k)}
\sqcup
\Phi_{f_i}\!\left(
S_{f_1}^{(k+1)},\ldots,S_{f_{i-1}}^{(k+1)},
S_{f_i}^{(k)},\ldots,S_{f_m}^{(k)}
\right).
```

The notation shows deterministic Gauss–Seidel iteration: later members in a
round observe earlier members' new values. Every update is inflationary, so

```math
S^{(0)} \sqsubseteq S^{(1)} \sqsubseteq S^{(2)} \sqsubseteq \cdots .
```

For a finite-height lattice and monotone functionals, this ascending chain
stabilizes at the least fixed point. IFDS facts are finite by construction;
an IDE domain must likewise provide a finite-height or otherwise convergent
summary representation. libcpg also enforces a finite operational round cap,
so termination does not depend on a correct client domain.

## 5. IFDS and IDE responsibilities

Classical IFDS defines a finite fact set $`D`$, adds a distinguished zero fact,
and requires distributive flow functions over subsets of $`D`$. Its exploded
supergraph has vertices pairing program points with facts. Path reachability
then answers whether a fact reaches a point [[1]](#references).

The libcpg framework does not prescribe the local exploded graph. A domain's
`Fact` is $`D`$, `seed_facts` provides initial point/fact pairs, and
`summarize` computes the function-level relation or another equivalent finite
summary. This separation lets taint, effects, and typestate reuse one SCC
schedule without forcing one local control-flow encoding.

IDE replaces set membership with values in an environment domain and
distributive micro-functions along edges [[2]](#references). An IDE
instantiation can store a composed environment transformer in `Summary`.
The solver only requires that summary composition expose the same
$`\bot`$/$`\sqcup`$ contract.

The type system cannot prove distributivity or monotonicity. Those properties
are scientific obligations tested by each domain. Runtime lattice probes catch
reflexivity, idempotence, change-report, and upper-bound contradictions, but
they are not a proof of the full algebra.

## 6. Determinism

Determinism is defined over equal CPG evidence, not petgraph insertion order:

1. `call_graph_sccs` canonicalizes nodes, members, component ids, and
   condensation edges;
2. condensation sinks are keyed by smallest member `NodeId`;
3. recursive members use ascending `NodeId`;
4. facts are sorted and deduplicated before retention; and
5. summaries and evidence use ordered maps and sets.

Consequently, for deterministic domain functions $`\Phi_f`$,

```math
\operatorname{solve}(G,D,\kappa)
=
\operatorname{solve}(\pi(G),D,\kappa)
```

for any insertion permutation $`\pi`$ that preserves nodes and typed edges.

## 7. Explicitly partial fixed points

Let $`\kappa_I`$ bound recursive rounds and $`\kappa_W`$ bound scheduler work.
If a cap stops the chain at $`k`$, the retained summary satisfies

```math
\bot \sqsubseteq S^{(k)} \sqsubseteq \operatorname{lfp}(\Phi).
```

For a may analysis, this prefix may omit a reachable fact. It is safe as
partial evidence but unsafe as a negative verdict. `IfdsOutput::is_complete`
therefore requires both no cap and no structural/domain issue. Callers receive
the prefix, the exact cap event, and completed-round counts together.

Raising a round or work cap cannot turn an already complete deterministic run
into a capped run. This monotonicity is executable as generated tests and is
separate from the domain's lattice monotonicity.

## 8. Structural trust boundary

A decomposition can be internally tidy yet stale after new call targets are
resolved. Let $`\operatorname{SCC}(G_F)`$ be a fresh canonical decomposition.
The solver accepts supplied value $`C`$ only if

```math
C = \operatorname{SCC}(G_F).
```

This exact check makes evaluation order evidence-bound to the current CPG. It
also avoids attaching a version number or mutable cache identity to an
otherwise immutable analysis value.

## See also

- [Component semantics](../components/graph/ifds-ide-framework.md)
- [ADR-0032](../design/0032-deterministic-ifds-ide-summaries.md)
- [Scientific validation 27](../scientific/27-ifds-ide-validation.md)
- [Usage 08](../usage/08-ifds-summary-domains.md)

## References

1. Reps, T., Horwitz, S., Sagiv, M. (1995). *Precise Interprocedural Dataflow
   Analysis via Graph Reachability.* Proceedings of POPL, 49–61. DOI:
   [10.1145/199448.199462](https://doi.org/10.1145/199448.199462).
2. Sagiv, M., Reps, T., Horwitz, S. (1996). *Precise Interprocedural Dataflow
   Analysis with Applications to Constant Propagation.* Theoretical Computer
   Science 167(1–2), 131–170. DOI:
   [10.1016/0304-3975(96)00072-2](https://doi.org/10.1016/0304-3975(96)00072-2).
