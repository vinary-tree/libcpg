# Theory 31 — May-happen-in-parallel relations

A **may-happen-in-parallel relation** (MHP relation) identifies pairs of
execution sites for which the program structure permits overlapping execution.
It is a *may* property: membership says that concurrency is structurally
possible, not that every execution overlaps or that the pair accesses shared
state. This distinction makes MHP useful as a substrate for race analysis while
keeping the result honest about scheduling and reachability.

libcpg derives MHP solely from typed parallel-composition blocks. A frontend
preserves a parallel construct as `CpgNodeKind::Block` with
`message_kind == Some(MsgKind::Spawn)`; the analysis never parses source text
or depends on a language grammar.

![Typed parallel operands become canonical MHP pairs](../diagrams/mhp-par-operands.svg)

## 1. Structural vocabulary

Let $`G=(V,E_{AST})`$ be the Abstract Syntax Tree (AST) overlay of a Code
Property Graph (CPG), and let $`P \subseteq V`$ be the set of typed parallel
blocks. For each $`p \in P`$, $`O_p=(o_1,\ldots,o_k)`$ is the stable sequence
of its direct AST children. Each child is one **operand root**. A root can be a
direct process node, such as a Rholang send, or a wrapper block; requiring a
particular wrapper shape would couple the analysis to one parser layout.

For an operand root $`o`$, let $`S(o)`$ contain executable sites in the AST
subtree rooted at $`o`$. libcpg includes statement nodes plus the explicit
execution-bearing kinds `Variable`, `Assignment`, `Call`, `Await`, and `Yield`.
Identifiers, literals, and structural blocks are not sites by themselves.
Traversal stops at a nested `Function`, because that node owns an independent
definition whose body is not executed merely by evaluating the enclosing
operand.

## 2. Relation definition

The unordered relation $`\mathcal{M}`$ is the union of cross-products between
different operands of every typed parallel block:

```math
\mathcal{M} =
\left\{
  \{x,y\}
  \;\middle|\;
  \exists p\in P,\; \exists i\ne j,\;
  x\in S(o_i),\; y\in S(o_j),\; x\ne y
\right\}.
```

Equivalently, two sites are related when a typed parallel block is their
nearest common AST ancestor and they lie under different direct operand
children. The cross-product construction realizes this rule without computing
an ancestor path for every site pair.

Each unordered pair is stored once as $`(\min(x,y),\max(x,y))`$. Therefore the
materialized vector is canonical, while logical membership remains symmetric:

```math
x\;\mathcal{M}\;y \iff y\;\mathcal{M}\;x.
```

Sites in one operand are not related by that parallel block. Sequential
siblings under an ordinary block are also unrelated. Nested parallel blocks
compose: the inner block contributes its own cross-operand pairs, and an outer
block relates every execution site in its nested operand to sites in the
outer block's other operands.

## 3. Literate algorithm

The implementation reads the graph in stable order and narrates its evidence as
it builds the relation:

```text
Find every Block whose typed message kind is Spawn; sort by NodeId.
For each parallel block:
    Collect and sort its direct AST children as operand roots.
    For each operand root:
        Walk its AST subtree with a visited set.
        Stop before nested Function bodies.
        Retain executable sites in sorted NodeId order.
    For each distinct operand pair:
        Form the Cartesian product of their execution sites.
        Canonicalize each site pair as (minimum id, maximum id).
        Insert the pair into one ordered set.
        If the public pair budget is exhausted, return capped evidence.
Return the sorted, deduplicated pairs and the explicit cap flag.
```

This is an out-of-graph analysis: it neither adds MHP edges nor mutates nodes.
That choice avoids confusing schedule-dependent advisory evidence with the
source-derived overlays.

## 4. Determinism and resource bounds

Let $`T=\sum_{p\in P}\sum_{o\in O_p}|\operatorname{subtree}(o)|`$ count the
visited AST positions, including repeated visits caused by nested parallel
regions, and let $`K`$ be the number of distinct pairs considered before the
cap. Ordered traversal plus ordered-set insertion costs
$`O(T+K\log K)`$ time and $`O(|V|+K)`$ transient/output space for a single
traversal frontier and the relation. The uncapped pair count can be quadratic
in the number of execution sites.

`MAX_MHP_PAIRS` bounds materialization at one million canonical pairs. When the
budget is reached, `MhpRelation::capped` is `true`. The retained result is a
deterministic under-approximation: every returned pair satisfies the structural
rule, but later valid pairs may be absent. Consumers that require complete race
candidate coverage must reject capped results rather than treating absence as
proof of sequentiality.

## 5. Interpretation

MHP answers only a concurrency question. A data-race candidate additionally
requires conflicting accesses to the same memory location, at least one write,
and no adequate synchronization. Conversely, missing `Spawn` metadata, omitted
AST edges, dynamic process creation, or the output cap can cause false
negatives. Guards and unreachable branches can cause structural false
positives.

Naumovich and Avrunin describe a conservative data-flow algorithm for finding
statement pairs that may execute in parallel [[1]](#references). libcpg adopts
the same conservative question but deliberately implements the smaller typed
parallel-structure abstraction required by this epic; it does not claim their
full interprocedural data-flow precision.

[Theory 43](43-evidence-bounded-race-and-atomicity.md) defines how the
feature-free race and atomicity APIs compose this relation with exact access
identity and locksets.

## References

1. Naumovich, G., Avrunin, G. S. (1998). *A Conservative Data Flow Algorithm
   for Detecting All Pairs of Statements That May Happen in Parallel.*
   Proceedings of the 6th ACM SIGSOFT International Symposium on Foundations
   of Software Engineering, 24–34. DOI:
   [10.1145/288195.288213](https://doi.org/10.1145/288195.288213).
