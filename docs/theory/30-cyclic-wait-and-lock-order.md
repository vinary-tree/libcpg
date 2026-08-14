# Theory 30 — Cyclic wait and lock-order deadlock candidates

A **deadlock** is a state in which every participant in a non-empty set waits
for an action that only another participant in that set can perform. Static
analysis rarely knows the complete runtime state, so libcpg reports
**deadlock candidates**: cyclic structural evidence that warrants review, never
a claim that a particular schedule reaches the cycle.

This chapter develops two related graphs:

- a process **wait-for graph** for message-passing code; and
- a resource **lock-order graph** for typed acquire/release calls.

Both are deterministic projections over existing CPG `NodeId` values. Both use
the shared strongly connected component (SCC) implementation instead of a
private cycle detector.

Parallel composition is a separate question: [Theory 31](31-may-happen-in-parallel.md)
derives a symmetric statement relation from typed operand subtrees without
using SCCs.

![Initially blocked processes become a wait-for SCC](../diagrams/wait-for-deadlock.svg)

## 1. Evidence vocabulary

Let $`P`$ be the set of top-level process/function nodes and $`C`$ the set of
resolved channel definitions. For a process $`p \in P`$,
`extract_events` returns an ordered sequence $`E_p`$. A **relevant initial
operation** is the first element of $`E_p`$ whose kind is `Send`, `SendSync`, or
linear `Recv`.

The partial function $`w : P \rightharpoonup C`$ records an initial wait:

```math
w(p) = c
\quad\Longleftrightarrow\quad
\operatorname{first}_{\{Send,SendSync,Recv\}}(E_p)
= Recv(c).
```

An unresolved linear receive does not invent $`c`$. A send as the first
relevant operation means the process can make structural progress and is not in
the domain of $`w`$. `RecvPersistent`, `Peek`, `Select`, and `Spawn` do not
establish this *linear* initial wait.

Let $`S(c)`$ be the processes with a resolved `Send` or `SendSync` on $`c`$.
The wait-for relation is:

```math
W = \{(p,q) \mid w(p)=c \land q\in S(c)
     \land S(c)\ne\varnothing
     \land S(c)\subseteq\operatorname{dom}(w)\}.
```

The last condition matters. If any known producer of $`c`$ is not initially
blocked, it can break the wait, so libcpg emits no edge for that channel. A
self-edge is retained: `recv(c); send(c)` in one process is a genuine
wait-before-own-production candidate.

## 2. SCCs as communication candidates

Construct $`G_W=(P_W,W)`$ from the endpoints of $`W`$. A cyclic SCC is a
maximal set in which each process is reachable from every other by wait edges.
It therefore contains circular-wait evidence. A singleton is cyclic only when
it has a self-edge.

The implication is deliberately one-way:

```math
\operatorname{cyclicScc}(G_W) \Longrightarrow
\operatorname{communicationCandidate},
```

not a proof of runtime deadlock. Guards, pattern matching, external producers,
dynamic channel extrusion, or missing graph evidence can change the operational
result. Kobayashi's type system reasons about richer process behavior and is the
documented upgrade path, not something this structural pass claims to
implement [[1]](#references).

## 3. Lock-order graph

Let $`L`$ be resolved lock-resource definitions. A typed acquisition has mode
`Read` or `Write`; a release removes the most recent matching acquisition from
the held stack. For every acquisition of $`b`$ while $`a`$ is held, add
$`a \rightarrow b`$ to the lock-order relation $`O`$. Re-acquiring the same
resource does not add a self-edge because reentrancy policy is not represented.

![Direct and interprocedural lock order feed the shared SCC engine](../diagrams/lock-order-deadlock.svg)

The classic A/B pattern produces $`A\rightarrow B`$ in one process and
$`B\rightarrow A`$ in another. Their SCC is a circular-wait candidate. An SCC
whose internal witnesses are all `Read`→`Read` is excluded: shared reads do not
mutually exclude one another under the represented read/write-lock model.

## 4. Interprocedural acquisition summaries

For process $`p`$, let $`D_p`$ be its directly acquired lock resources and
$`Call(p)`$ its resolved callees. The reachable-acquisition summary is the least
fixed point:

```math
A_p = D_p \cup \bigcup_{q\in Call(p)} A_q.
```

libcpg computes this finite monotone fixed point over ordered maps. At a call
site reached while holding $`a`$, every $`b\in A_q`$ contributes a may-order
edge $`a\rightarrow b`$. The witness retains the caller's held-lock site, the
ultimate callee acquisition site, both source ranges, and the immediate call
site. Recursive call SCCs converge because summaries only add members from the
finite resource universe.

## 5. Literate algorithm

The implementation follows this sequence:

```text
Extract typed, definition-resolved channel and lock events.
For each process, classify its first relevant message operation.
Create wait edges only when every known producer is initially blocked.
Project process ids and wait edges into GraphProjection::External.
Run projection_sccs and retain cyclic components with their internal edges.

Compute direct lock acquisitions and resolved call sites per process.
Iterate reachable-acquisition summaries to a finite fixed point.
Simulate each ordered acquire/release/call skeleton to create lock-order edges.
Project resource ids and order edges into GraphProjection::External.
Run projection_sccs; discard all-read components; attach precise witnesses.
```

No step parses source or compares resource spellings. `Variable` definition
identity and typed metadata are the semantic boundary.

## 6. Determinism, complexity, and limitations

Ordered maps, sets, site sorting, canonical CSR construction, and deterministic
SCC ordering make output independent of CPG node and edge insertion order. If $`n`$ is
the projected vertex count and $`m`$ the projected edge count, SCC decomposition
is $`O(n+m)`$. The lock-summary fixed point is polynomial in process, call, and
resource counts; it never enumerates schedules or Petri-net markings.

The report is incomplete when definitions, call targets, lock annotations, or
external participants are missing. It can over-approximate when a represented
branch or call is unreachable. Consumers must preserve the word *candidate*.

## References

1. Kobayashi, N. (2006). *A New Type System for Deadlock-Free Processes.*
   CONCUR 2006, LNCS 4137, 233–247. DOI:
   [10.1007/11817949_16](https://doi.org/10.1007/11817949_16).
2. Meredith, L. G., Radestock, M. (2005). *A Reflective Higher-order Calculus.*
   DOI:
   [10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016).
