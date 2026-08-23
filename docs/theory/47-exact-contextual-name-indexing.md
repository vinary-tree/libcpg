# Theory 47 -- exact contextual name indexing

Call-target lookup is not ordinary dictionary search. The same spelling can
denote different declarations in nested scopes, imports can introduce local
aliases, and a near spelling is not evidence of identity. This chapter states
the mathematical relation that an index may accelerate without changing.

## 1. Names live in contexts

Let $`S`$ be represented lexical scopes, $`N`$ exact valid `str` spellings,
and $`V`$ function node identities. An index is a finite relation

```math
I \subseteq S \times N \times V.
```

For scope $`s`$ and spelling $`n`$, define the exact bucket

```math
I(s,n)=\{v\in V\mid(s,n,v)\in I\}.
```

The implementation stores this relation as ordered maps whose leaves are
sorted, deduplicated node vectors plus a full pre-cap count. The representation
is not the semantics: a trie, DAWG, hash table, or ordered tree is lawful only
when it returns the same relation and canonical order.

## 2. Shadowing is an ordered search, not a merged set

Let $`p:S\rightharpoonup S`$ be the enclosing-scope relation and
$`s_0,s_1,\ldots`$ the bounded chain beginning at the call's scope. Exact
lexical resolution selects the first non-empty bucket:

```math
k=\min\{i\mid I(s_i,n)\ne\varnothing\}.
```

It does not union all buckets. A unique outer declaration cannot override two
same-named declarations in the nearest non-empty scope. Import aliases form a
second represented exact relation under the local binding; global bare-name
lookup is a weaker final evidence tier.

Candidate retention is also distinct from cardinality. For cap $`K`$, the
report may retain

```math
R_K(s,n)=\operatorname{take}_K(\operatorname{sort}(I(s,n))),
```

but uniqueness is decided by $`\lvert I(s,n)\rvert`$, never
$`\lvert R_K(s,n)\rvert`$.

## 3. Exact, prefix, and edit relations differ

A prefix relation

```math
P(s,n)=\{v\mid\exists m,\;n\preceq m\land(s,m,v)\in I\}
```

and a bounded edit relation

```math
F_d(s,n)=\{(v,m)\mid(s,m,v)\in I\land\delta(n,m)\le d\}
```

answer useful search questions. Here $`\preceq`$ means “is a prefix of” and
$`\delta`$ is a caller-selected edit distance. Neither is exact identity:

```math
I(s,n) \subseteq P(s,n),
\qquad
I(s,n) \subseteq \pi_V(F_d(s,n)),
```

and both inclusions can be strict. Selecting a singleton from a larger
relation after an exact miss would silently change the resolver's proposition
from “this spelling denotes this represented function” to “this spelling is
similar to this represented function.” Performance cannot validate that
semantic substitution.

![Only exact contextual buckets reach the topology decision; prefix and edit relations terminate as separately measured non-semantics.](../diagrams/resolver-index-decision.svg)

*Figure -- contextual exact relation and rejected widened relations. Source:
[`diagrams/resolver-index-decision.puml`](../diagrams/resolver-index-decision.puml).*

## 4. Ordered-map and trie cost models

For $`M`$ distinct names, total name units $`U`$, and $`Q`$ lookups, a
comparison-tree model has approximate cost

```math
C_{\mathrm{tree}}=B_{\mathrm{tree}}(M,U)
 + QL_{\mathrm{tree}}(M,n).
```

A static trie family has

```math
C_{\mathrm{trie}}=B_{\mathrm{trie}}(M,U)
 + QL_{\mathrm{trie}}(n).
```

Asymptotically cheaper $`L_{\mathrm{trie}}`$ does not imply a cheaper complete
resolver. Construction is paid inside the resolver lifetime, candidate payload
materialization remains, and cache ownership/invalidation determines whether
the same build can serve more than one analysis epoch. The measured break-even
quantity

```math
Q^*=\frac{B_{\mathrm{trie}}-B_{\mathrm{tree}}}
{L_{\mathrm{tree}}-L_{\mathrm{trie}}}
```

is meaningful only when its numerator, denominator, and reuse epoch describe
the actual application.

Ordered trees also supply prefix ranges: begin at the lower bound of $`n`$ and
stop at the first key that does not start with $`n`$. That operation visits
only the search path and matching interval, so a specialized prefix zipper is
not automatically superior.

## 5. Automaton intersection

A Levenshtein automaton recognizes all strings within distance $`d`$ of a
query. Intersecting it lazily with a dictionary prunes subtrees whose automaton
frontier is dead. This explains why an ordered fuzzy query can beat a complete
scan by orders of magnitude: it changes the algorithm from “measure every
term” to “traverse only product states that can still accept.”

That gain is algorithmic and is independent of whether a particular transition
uses SIMD. SIMD accelerates a selected recurrence over several lanes; it does
not create the pruning relation or authorize fuzzy resolver semantics.

## 6. Stack depth and heap shape are independent

An explicit worklist can make native call depth constant while still retaining
too much heap. In the evaluated positional transition cache, each generated
state owns a dense row of stride

```math
s(q)=2^{\lceil\log_2(q+1)\rceil}.
```

If an exact path generates $`g(q)=O(q)`$ states, target storage is

```math
M(q)=g(q)s(q)z=O(q^2),
```

where $`z`$ is target-cell size. A pushdown or iterative implementation proves
constant native stack; it does not discharge this heap obligation. Resource
admission must test and bound both axes.

## 7. Determinism and topology

Canonical ordering is required because a resolver report is serialized,
compared, and consumed by graph analyses. Yet order is not evidence strength.
The rule is:

```text
INDEX_EXACT(scope, spelling):
    visit lexical scopes from nearest to farthest
    return the first exact non-empty canonical bucket
    otherwise inspect represented exact imports, then global exact names
    decide ambiguity from the full count
    emit topology only for one candidate in an authorized evidence tier
```

This literate algorithm separates relation lookup, evidence classification,
and graph mutation. An optimization may replace the first operation only after
proving equality and the complete lifecycle cost/resource contract.

## References

1. Aoe, J. (1989). *An Efficient Digital Search Algorithm by Using a
   Double-Array Structure.* IEEE Transactions on Software Engineering 15(9),
   1066--1077. DOI:
   [10.1109/32.31365](https://doi.org/10.1109/32.31365).
2. Schulz, K. U., Mihov, S. (2002). *Fast String Correction with Levenshtein
   Automata.* International Journal on Document Analysis and Recognition 5,
   67--85. DOI:
   [10.1007/s10032-002-0082-8](https://doi.org/10.1007/s10032-002-0082-8).
