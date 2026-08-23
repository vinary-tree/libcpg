# Theory 46 -- content-addressed exact incremental analysis

Incremental analysis asks which result from a previous program revision may be
reused after an edit. libcpg answers a deliberately narrow version: reuse a
function-local CFG, DFG, or PDG plan only when a complete canonical description
of its represented dependencies is byte-equal.

## 1. Identity, digest, and evidence

Let $`X`$ be the set of admitted function contexts and $`P`$ the set of local
analysis plans. The ordinary analysis is a function
$`A:X\rightarrow P`$. A canonical encoder $`K:X\rightarrow\{0,\ldots,255\}^*`$
is valid for exact reuse when

```math
K(x)=K(y)\Longrightarrow A(x)=A(y).
```

This is the cache's semantic obligation. A digest
$`h:\{0,\ldots,255\}^*\rightarrow\{0,\ldots,2^{64}-1\}`$ is not injective and
cannot establish it. libcpg therefore uses $`h(K(x))`$ only to find a small
bucket, then decides a hit with $`K(x)=K(y)`$.

This is **content-addressed** in the broad engineering sense that content
determines lookup identity, but it is not a cryptographic content-address. No
security property depends on FNV-1a collision resistance.

## 2. Coordinate transport

A graph revision changes global node identities. For a function context
$`x`$, let $`L_x:V_x\rightarrow\{0,\ldots,|V_x|-1\}`$ map each local graph node
to its canonical ordinal. The cache stores $`L_x(A(x))`$. A hit for context
$`y`$ reconstructs:

```math
A(y)=L_y^{-1}\!\left(L_x(A(x))\right).
```

The equation is meaningful only when the plan is local and every ordinal is in
range. That is why insertion validates all fresh endpoints and replay validates
all cached endpoints before graph mutation.

## 3. Dependency closure and conservative rejection

A key is sound only if it is closed over what the analysis reads. Define
$`D_A(x)`$ as the semantic dependency set read by analysis $`A`$. Exact reuse
requires either:

```math
D_A(x)\subseteq \operatorname{represented}(K(x)),
```

or rejection from the cache domain. libcpg chooses rejection for dependencies
whose compact local representation would be misleading:

- external callers and their arguments;
- preceding lexical definitions;
- nested-function scope;
- nominal owner and field declarations; and
- PDG overlay edges that cross a function boundary.

This is a partial memo function: ordinary analysis remains total over inputs
the builder accepts, while memoization is defined only over the exact local
subset.

![The local dependency closure and conservative bypass boundary.](../diagrams/function-analysis-cache-boundary.svg)

*Figure -- represented dependencies enter the canonical key; nonlocal evidence
selects ordinary analysis. Source:
[`diagrams/function-analysis-cache-boundary.puml`](../diagrams/function-analysis-cache-boundary.puml).*

## 4. Bounded least-recently-used retention

Least-recently-used (LRU) retention orders entries by their most recent exact
access. If $`t(e)`$ is an entry's logical timestamp, eviction selects:

```math
e_{\mathrm{victim}}=\operatorname*{arg\,min}_{e\in C} t(e).
```

libcpg applies stable digest and bucket-position tie-breaks. The cache must
satisfy both $`|C|\le E`$ and $`B(C)\le B_{\max}`$, where $`E`$ is the entry
limit and $`B(C)`$ is the additive estimated residency. An entry larger than
the byte limit is emitted normally but not retained.

## 5. Invalidation as key inequality

There is no mutation-driven invalidation table. Editing a represented
dependency changes canonical bytes and yields a miss. Editing an unrelated
function leaves the unchanged function key equal. This gives a simple
invalidation law:

```math
K(x)\ne K(y)\Longrightarrow \text{reuse is forbidden},
```

without claiming the converse for rejected contexts. Schema/version changes
also change the key domain.

## 6. Determinism and stack safety

Canonical node order, sorted overlay tuples, explicit edge-kind codes, full
byte comparison, indexed parallel collection, ordered emission, and
deterministic LRU selection remove hash-map and scheduler order from observable
results.

The semantic equations do not require recursive implementation. libcpg uses a
pushdown-style heap worklist for tree discovery and flat loops for coordinate
transport and destruction. Thus graph depth contributes heap state, not native
call frames.

## 7. What this theory does not claim

- A hit does not reuse parsing or AST allocation.
- A miss does not imply semantic change; it means exact reuse was not proved.
- A bypass is not an approximation; it runs the ordinary exact implementation.
- Estimated resident bytes do not include allocator-private bookkeeping.
- Equal local plans do not prove language-level program equivalence.
- The cache is in-memory and caller-owned, not a persistent build artifact.

## References

1. Michie, D. (1968). *Memo Functions and Machine Learning.* Nature 218,
   19--22. DOI:
   [10.1038/218019a0](https://doi.org/10.1038/218019a0).
2. Acar, U. A., Blelloch, G. E., and Harper, R. (2006). *Adaptive Functional
   Programming.* ACM Transactions on Programming Languages and Systems 28(6),
   990--1034. DOI:
   [10.1145/1186632.1186634](https://doi.org/10.1145/1186632.1186634).
3. Mattson, R. L., Gecsei, J., Slutz, D. R., and Traiger, I. L. (1970).
   *Evaluation Techniques for Storage Hierarchies.* IBM Systems Journal 9(2),
   78--117. DOI:
   [10.1147/sj.92.0078](https://doi.org/10.1147/sj.92.0078).
