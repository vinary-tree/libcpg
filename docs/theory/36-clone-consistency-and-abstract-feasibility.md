# Theory 36 — Clone consistency and abstract feasibility

This chapter defines libcpg's feature-free I8 inconsistent-clone and I9
infeasible-path analyses. Both return finite evidence for review. They do not
claim that a divergent clone is wrong, that an unreachable branch should be
deleted, or that unreported defects are absent when evidence is incomplete.

![The two advisory paths share typed graph evidence but retain distinct proof obligations](../diagrams/clone-feasibility-advisory-pipeline.svg)

## 1. Evidence universe

Let $`G=(V,E)`$ be a finite Code Property Graph (CPG). A function root
$`f\in V`$ owns a typed AST. Conditional CFG edges leave a source $`s\in V`$
with outcomes in $`\{\mathsf{true},\mathsf{false}\}`$. Data Flow Graph (DFG)
edges connect represented definitions and uses.

For I8, an exact candidate pair $`(f_l,f_r)`$ is admitted by the existing
Type-2 or Type-3 clone relation. Type-2 means equal normalized AST shape under
permitted payload renaming; Type-3 means sufficient normalized multiset
similarity despite insertion or deletion. This pair relation is stronger than
membership in one transitive clone class.

For I9, a guard $`g`$ is either the conditional source itself or the first AST
child of a recognized control wrapper. Sparse conditional constant propagation
(SCCP) supplies executable edges and scalar constants. Interval analysis
supplies a map $`\rho_s`$ from variable names to integer intervals immediately
before $`s`$.

## 2. Low-edit clone candidates

Each function becomes a flat typed tape whose entries record node identity,
normalized kind, and direct-child count. The alignment yields edit count $`e`$
and larger tape size $`m`$. For configured exact threshold $`a/b`$, the pair is
low-edit precisely when:

```math
e b \leq a m.
```

No floating-point quotient decides that boundary. The upstream clone
similarity and bounded GED upper bound remain report evidence. A capped GED is
not silently treated as exact.

### 2.1 Rename conventions

Aligned identifier pairs induce a multimap. For one source spelling $`x`$, let
$`c(x,y)`$ count occurrences aligned with destination spelling $`y`$, and let:

```math
y^*(x)=\operatorname*{arg\,max}_{y} c(x,y).
```

Ties are broken canonically by spelling. A mapping is dominant only when its
exact support meets policy and its population meets the minimum occurrence
count. Any aligned $`x\mapsto y`$ with $`y\ne y^*(x)`$ becomes a concrete
rename anomaly. The analysis tests both directions because a many-to-one map
can be consistent one way and anomalous the other.

### 2.2 Guard divergence

For aligned nodes within represented guards, I8 compares exact payloads of
binary operators, unary operators, and literal kinds/values. A difference such
as `>` versus `<`, `true` versus `false`, or `5` versus `3` becomes typed
`GuardDivergence` evidence. A structural insertion outside a guard is not by
itself a bug finding.

This realizes the inconsistent-change motivation studied by Juergens et al.
while keeping the library's conclusion narrower: the report identifies a
small, typed divergence in an exact near-clone pair [[1]](#references).

## 3. Boolean path partitions

I9 asks which interval environments can make a condition true and which can
make it false. A **state** consists of the current AST arena node, required
truth value, interval environment, accumulated predicate witnesses, and an
explicit continuation stack.

For condition denotations $`T(e,\rho)`$ and $`F(e,\rho)`$, conjunction obeys:

```math
T(a\land b,\rho)=\bigcup_{\rho'\in T(a,\rho)}T(b,\rho'),
```

```math
F(a\land b,\rho)=F(a,\rho)\;\cup\!
\bigcup_{\rho'\in T(a,\rho)}F(b,\rho').
```

Disjunction is dual:

```math
F(a\lor b,\rho)=\bigcup_{\rho'\in F(a,\rho)}F(b,\rho'),
```

```math
T(a\lor b,\rho)=T(a,\rho)\;\cup\!
\bigcup_{\rho'\in F(a,\rho)}T(b,\rho').
```

Negation swaps requested truth. These equations encode language-independent
short-circuit path structure without evaluating source text.

![Boolean recursion is executed by explicit frames, environments, and alternatives](../diagrams/boolean-condition-pushdown-machine.svg)

## 4. Comparison regions

Normalize comparisons so the variable is on the left. For integer constant
$`c`$, required truth maps to one or two exact regions:

| Predicate requirement | Admissible integer region(s) |
|---|---|
| $`x<c`$ true | $`[-\infty,c-1]`$ |
| $`x<c`$ false | $`[c,+\infty]`$ |
| $`x\leq c`$ true | $`[-\infty,c]`$ |
| $`x\leq c`$ false | $`[c+1,+\infty]`$ |
| $`x=c`$ true, or $`x\ne c`$ false | $`[c,c]`$ |
| $`x=c`$ false, or $`x\ne c`$ true | $`[-\infty,c-1]`$ and $`[c+1,+\infty]`$ |

Checked predecessor/successor arithmetic makes $`x<\mathrm{i128::MIN}`$ and
$`x>\mathrm{i128::MAX}`$ empty instead of overflowing. Each region meets the
current interval. An empty meet terminates that alternative and retains the
ordered predicates that caused the contradiction.

For the example $`x>5\land x<3`$, the true path first refines to
$`[6,+\infty]`$, then meets $`[-\infty,2]`$:

```math
[6,+\infty]\cap[-\infty,2]=\varnothing.
```

Every true alternative is empty, so the true CFG outcome is infeasible, the
condition is always false, and the predicate evidence is contradictory.

## 5. Combining SCCP and intervals

SCCP and intervals answer different questions. SCCP can prove `if true` and
fold scalar expressions through DFG cells. Intervals can prove correlation
within one short-circuit path even when neither comparison is constant alone.

For outcome $`q`$, I9 considers it impossible when either:

- SCCP marks every represented $`q`$ edge dead; or
- the Boolean interval machine produces no live $`q`$ alternative.

The finding identifies `Sccp`, `Intervals`, or `SccpAndIntervals` as the proof
source. If only one outcome remains possible, the guard is redundant. A
`ContradictoryCondition` additionally requires empty true alternatives with
interval predicate evidence, preventing a literal `false` from being mislabeled
as a range contradiction.

## 6. Completeness, determinism, and stack safety

I8 issues expose missing/cyclic/shared ASTs, AST and alignment caps, allocation
failure, and capped GED. I9 issues expose missing guard/state/node evidence,
cycles, unsupported condition nodes, and node/state/step caps. Only complete
evidence can justify an empty-result claim.

Stable identifiers, ordered collections, canonical pair direction, canonical
tie breaking, and sorted findings make reports independent of graph insertion
order. No source-sized process uses native recursion. AST tapes, edit matrices,
condition arenas, continuations, alternative queues, active-node sets, and
predicate witnesses live on the heap. Explicit caps bound heap and total work;
they do not change the native-stack guarantee.

## 7. Interpretation limits

The clone analyzer does not establish semantic equivalence, change intent, or
fault impact. The path analyzer does not model floating-point comparisons,
arbitrary symbolic relations, heap aliases, nonlinear arithmetic, external
effects, or unrepresented paths. Both results are exact only for their stated
typed evidence and policy. They are review inputs, not correctness proofs.

## References

1. Juergens, E., Deissenboeck, F., Hummel, B., Wagner, S. (2009). *Do Code
   Clones Matter?* ICSE, 485–495. DOI:
   [10.1109/ICSE.2009.5070547](https://doi.org/10.1109/ICSE.2009.5070547).
2. Wegman, M. N., Zadeck, F. K. (1991). *Constant Propagation with Conditional
   Branches.* ACM TOPLAS 13(2), 181–210. DOI:
   [10.1145/103135.103136](https://doi.org/10.1145/103135.103136).
3. Cousot, P., Cousot, R. (1977). *Abstract Interpretation: A Unified Lattice
   Model for Static Analysis of Programs by Construction or Approximation of
   Fixpoints.* POPL, 238–252. DOI:
   [10.1145/512950.512973](https://doi.org/10.1145/512950.512973).
