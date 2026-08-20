# Theory 43 — Evidence-bounded race and atomicity candidates

A **data race** is a pair of conflicting accesses to one shared location that
can overlap without adequate mutual exclusion. An **atomicity violation** is a
broader interference problem: operations intended to act as one logical update
can be observed or changed between their steps. libcpg reports *candidates* for
these conditions. It proves facts about represented CPG evidence, not a
reachable execution under a complete language memory model.

This chapter defines the exact relations implemented by `race_candidates` and
`atomicity_candidates`. May-happen-in-parallel is abbreviated **MHP** only after
this definition: an MHP pair is two executable sites whose typed parallel AST
permits overlap.

![Exact evidence composition for concurrency candidates](../diagrams/concurrency-bug-evidence-pipeline.svg)

## 1. Access evidence

Let $`G=(V,E)`$ be a Code Property Graph (CPG). Let $`S\subseteq V`$ be its
represented executable sites, $`L\subseteq V`$ exact location declarations,
and $`K=\{\mathsf{R},\mathsf{W},\mathsf{RW}\}`$ the read, write, and
read-write access kinds.

An access fact is a tuple $`a=(s,e,\ell,k)`$ where:

- $`s\in S`$ is the executable statement queried against MHP;
- $`e\in V`$ is the exact member, index, or channel expression retained for
  explanation;
- $`\ell\in L`$ is one uniquely referenced `Field` or `Variable`; and
- $`k\in K`$ comes from typed DFG or `MsgKind` evidence.

For fields and indexes, $`e`$ must touch `FieldRead`, `FieldWrite`, `IndexRead`,
or `IndexWrite`, and exactly one incident `Reference` must identify $`\ell`$.
For channels, typed sends are writes and typed receives/peeks are reads. No name
comparison creates an access fact.

The expression-to-site map is the unique nearest executable AST ancestor. A
cycle-guarded breadth-first walk computes it. Zero answers mean incomplete
evidence; multiple equally near answers mean malformed ambiguity. Both cases
suppress the fact.

## 2. Locksets and exclusion

An **execution strand** is the unique nearest containing function or direct
operand of a typed parallel block. Its lock events are sorted by
`(range.start, range.end, NodeId)` and replayed by a finite state machine whose
state is a heap vector of acquisition instances.

For access $`a`$, let $`H(a)`$ be the resulting held-lock multiset. A held lock
records resource $`q`$, mode $`m\in\{\mathsf{Read},\mathsf{Write}\}`$, and the
acquisition site. Two held locks exclude each other when they name one resource
and at least one acquisition is exclusive:

```math
\operatorname{excludes}(h_1,h_2)
\iff
q(h_1)=q(h_2)
\land
\bigl(m(h_1)=\mathsf{Write}\lor m(h_2)=\mathsf{Write}\bigr).
```

The common-exclusion predicate is therefore:

```math
\operatorname{guarded}(a,b)
\iff
\exists h_a\in H(a),h_b\in H(b):
\operatorname{excludes}(h_a,h_b).
```

This definition matters: a read-mode/read-mode pair can run concurrently and
must not suppress evidence containing a write.

## 3. Race relation

Let $`\mathcal{M}\subseteq S\times S`$ be the symmetric MHP relation, and let
$`\operatorname{writes}(k)`$ hold for $`\mathsf{W}`$ and $`\mathsf{RW}`$.
The candidate relation is:

```math
\mathcal{R}=
\left\{
(a,b)\;\middle|\;
s(a)\;\mathcal{M}\;s(b),
\ell(a)=\ell(b),
\operatorname{writes}(k(a))\lor\operatorname{writes}(k(b)),
\neg\operatorname{guarded}(a,b)
\right\}.
```

Each pair is canonicalized by statement id. Locations attached to the two
sites are already sorted, so libcpg intersects them with a two-cursor linear
merge. It does not multiply every access on one statement by every access on
the other.

## 4. Atomicity relation

Consider adjacent accesses $`r,w`$ to location $`\ell`$ in one strand's
source order, where $`r`$ reads and $`w`$ writes. A **split boundary** for lock
$`q`$ exists when:

1. $`q`$ is held at both $`r`$ and $`w`$;
2. a resolved release of $`q`$ occurs after $`r`$ and before $`w`$; and
3. a resolved reacquisition of $`q`$ follows that release before $`w`$.

Let $`x`$ be an access to $`\ell`$ in another strand. It is a represented
interferer when both endpoint pairs are in MHP:

```math
s(r)\;\mathcal{M}\;s(x)
\land
s(w)\;\mathcal{M}\;s(x).
```

Let $`C(r,w)`$ contain locks whose *same acquisition instance* is present at
both endpoints. These are continuous guards; a released and reacquired lock
has different acquisition sites and is deliberately absent. The atomicity
candidate predicate is:

```math
\operatorname{atomicity\_candidate}(r,w,x)
\iff
\operatorname{split}(r,w,q)
\land
s(r)\;\mathcal{M}\;s(x)
\land
s(w)\;\mathcal{M}\;s(x)
\land
\neg\exists h_c\in C(r,w),h_x\in H(x):
\operatorname{excludes}(h_c,h_x).
```

![Read-modify-write critical-section gap](../diagrams/atomicity-lock-gap.svg)

An interferer holding the split lock $`q`$ is not suppression evidence: it can
acquire $`q`$ precisely during the gap. A different lock held continuously
across the logical update can suppress the candidate when it excludes $`x`$.

## 5. Literate algorithms

The shared evidence pass narrates identity before classification:

```text
For each typed field/index DFG edge:
    Resolve exactly one referenced Field or Variable.
    Lift its expression to one nearest executable site.
For each typed channel operation:
    Resolve its exact channel and classify send/receive access kind.
For each access and typed lock event:
    Find one execution strand with an iterative parent machine.
For each strand in stable order:
    Replay acquisitions and releases in source order.
    Snapshot the held acquisition instances at every access.
Return sorted access facts, per-site indexes, and lock boundaries.
```

Race classification then performs a merge join:

```text
Canonicalize and deduplicate every supplied MHP pair.
For each pair (left, right):
    Merge the two sorted per-site location vectors.
    For each shared location:
        Emit when one access writes and no common lock excludes the pair.
Sort and deduplicate complete witnesses.
```

Atomicity classification uses source-adjacent updates:

```text
Group accesses by (strand, location); sort each group by source position.
Index all accesses by location.
For every adjacent read-then-write pair:
    Find a resolved release/reacquire boundary for one endpoint lock.
    Examine only other-strand accesses from the same-location index.
    Require MHP with both endpoints.
    Suppress when a continuously held lock excludes the interferer.
    Emit the complete boundary and lockset witness.
```

Every work queue, access vector, held-lock stack, ordered set, and grouping map
is heap allocated. Source depth does not increase native call depth.

## 6. Complexity

Let $`A`$ be the number of exact access facts, $`B`$ typed lock boundaries,
$`P`$ supplied MHP pairs, $`H`$ the maximum represented parent height, and
$`R`$ the emitted race count. Evidence lifting costs
$`O((A+B)H+|E|)`$ in the worst malformed/deep case and $`O(A+B+|E|)`$ space.
MHP normalization costs $`O(P\log P)`$ time and $`O(P)`$ space.

For one MHP pair, the location merge is linear in the two sites' exact access
counts. Atomicity grouping costs $`O(A\log A)`$ overall; interference work is
restricted to same-location vectors rather than all graph accesses. Output
storage is $`O(R+Q)`$, where $`Q`$ is the atomicity-candidate count.

## 7. Interpretation and limits

The reports are high-signal structural advisories in the RacerD/RacerX lineage
[[1]](#references), [[2]](#references), but they are not equivalent to either
system. libcpg does not currently model pointer aliases, object-sensitive heap
locations, all control-flow order, atomic/volatile operations, condition
variables, barriers, lock poisoning, reentrant runtime semantics, or a formal
language memory model. Structural MHP can include unreachable sites, while a
capped MHP relation can omit candidates.

Consequently:

- presence means “all represented prerequisites hold”;
- absence with complete evidence means only “no candidate under this model”;
- absence with missing identity or capped MHP is inconclusive; and
- every finding requires engineering review before being called a defect.

## References

1. Blackshear, S., Gorogiannis, N., O'Hearn, P. W., Sergey, I. (2018).
   *RacerD: Compositional Static Race Detection.* DOI:
   [10.1145/3276514](https://doi.org/10.1145/3276514).
2. Engler, D., Ashcraft, K. (2003). *RacerX: Effective, Static Detection of Race
   Conditions and Deadlocks.* DOI:
   [10.1145/945445.945468](https://doi.org/10.1145/945445.945468).
