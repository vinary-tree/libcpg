# Theory 37 — API-usage graphs and temporal properties

This chapter defines the two evidence relations exposed by T6.5. An
**Application Programming Interface (API) Usage Graph (AUG)** is a graph of one
represented API occurrence, its data/control context, and later calls. A
**temporal property** is a finite-state rule over the order in which events may
act on an abstract object. The first relation learns peer conventions; the
second checks an explicit caller policy. They answer different questions and
must not be conflated.

## 1. Evidence universe

Let a Code Property Graph (CPG) contain a finite node set $`V`$ and typed edge
families for the Abstract Syntax Tree (AST), Control Flow Graph (CFG), Data Flow
Graph (DFG), and Program Dependence Graph (PDG). Let $`C \subseteq V`$ be the
represented call sites. A complete call site $`c \in C`$ has:

- an enclosing function $`f(c)`$;
- a stable API identity $`a(c)`$;
- membership in the canonical CFG projection of $`f(c)`$;
- all requested local evidence within configured resource bounds.

Completeness is part of the mathematical domain. A missing identity or capped
walk does not become an unknown-valued observation inside the population; the
site is excluded and the report records why.

## 2. Centered API Usage Graph

For a complete call $`c`$, define:

```math
G_c = (V_c, E_c, \lambda_V, \lambda_E)
```

The local nodes are:

```math
V_c =
\{c\}
\cup A(c)
\cup R(c)
\cup K(c)
\cup F(c)
```

where $`A(c)`$ is the ordered argument-node set, $`R(c)`$ is the zero-or-one
receiver-node set, $`K(c)`$ is the set of direct control-dependence sources, and
$`F(c)`$ is the set of other represented calls reachable after $`c`$ in the
same function CFG.

The node-label function $`\lambda_V`$ distinguishes:

- `AnchorCall(ApiIdentity)`;
- `FollowingCall(ApiIdentity)`;
- `Data(ApiDataShape)`;
- `Control(ControlContext)`.

The data-shape abstraction preserves semantic classes such as exact literal
values, member names, and operators while abstracting identifier spelling.
This makes peer occurrences comparable without guessing a language grammar.

The edge relation is:

```math
E_c =
\{(x,c,\operatorname{Parameter}(i))\}
\cup \{(r,c,\operatorname{Receiver})\}
\cup \{(k,c,\operatorname{Selection})\}
\cup \{(c,q,\operatorname{Order})\}
```

The sets range over the corresponding local nodes. Every non-anchor node has
one edge incident to the anchor, so the projection is acyclic. Multiple typed
edges remain representable; hence the public type is a directed labeled
multigraph rather than an unlabeled simple graph.

![Typed data and control enter the anchor; represented later actions leave it](../diagrams/centered-api-usage-graph.svg)

## 3. Reachability means may-follow

Let $`\leadsto_f`$ be reflexive-transitive reachability in the CFG of function
$`f`$. A later call $`q`$ belongs to $`F(c)`$ when:

```math
c \leadsto_{f(c)} q
\quad\land\quad
q \ne c
```

The relation is existential over represented CFG paths. Therefore an `Order`
edge means “may execute later,” not “must execute later on every runtime path.”
Path-sensitive obligations belong in an explicit temporal property, not in the
population convention.

Calls with the same identity as the anchor remain graph nodes but are excluded
from `FollowingCall` pattern candidates. This avoids turning repeated calls to
the same API into a self-successor convention while preserving their source
evidence in the AUG.

## 4. Population and support

For an API identity $`\alpha`$, define its complete population:

```math
P_\alpha = \{c \mid c\text{ is complete} \land a(c)=\alpha\}
```

A fragment predicate $`\phi(c)`$ answers whether one centered graph carries a
particular following call, arity, positional data shape, receiver shape, or
control context. Its exact support is:

```math
\operatorname{support}(\phi,P_\alpha)
=
\frac{|\{c\in P_\alpha \mid \phi(c)\}|}{|P_\alpha|}
```

For configured threshold $`\tau=s/d`$, libcpg accepts the fragment when:

```math
|\{c\in P_\alpha \mid \phi(c)\}|d
\geq
|P_\alpha|s
```

Integer cross multiplication avoids rounding error. The threshold validator
also requires $`2s>d`$. For a single-valued property, suppose two distinct
values both had strict-majority support. Their disjoint supporting sets would
contain more than the whole population, a contradiction. Thus the expected
arity, argument shape at one position, or receiver shape is unique.

## 5. Deviation semantics

A frequent fragment yields one pattern with its complete supporting-site set.
A complete site $`c`$ yields an `ApiUsageMisuse` exactly when the fragment
predicate is false there. The five public deviations are:

| Pattern | Deviating observation |
|---|---|
| following call $`\beta`$ | $`\beta`$ is absent from reachable distinct APIs |
| argument count $`k`$ | observed arity differs from $`k`$ |
| argument shape at $`i`$ | position is absent or shape differs |
| receiver shape | receiver is absent or shape differs |
| control context | direct context is absent |

The term “misuse” names an advisory data structure. Frequency does not establish
the API provider's specification, path necessity, security impact, or a safe
repair.

## 6. Temporal property semantics

A finite-state machine (FSM) is:

```math
M = (Q,\Sigma,\delta,q_{\mathrm{err}},I,F)
```

Here $`Q`$ is a finite state set, $`\Sigma`$ is the event alphabet,
$`\delta:Q\times\Sigma\rightharpoonup Q`$ is a partial transition function,
$`q_{\mathrm{err}}`$ is the absorbing error state, $`I`$ maps allocation events
to initial states, and $`F\subseteq Q`$ is the accepting-state set.

The typestate engine associates each allocation call site with an abstract
object identity. DFG/reference evidence relates later event arguments or
receivers to that object. Along represented CFG paths:

- a defined transition advances state;
- a missing transition produces `ForbiddenTransition`;
- an exit fact whose state is not in $`F`$ produces `NonAcceptingExit`.

For release-after-acquire, `lock` allocates state `acquired`, `unlock` maps it
to accepting state `released`, and an exit in `acquired` is a temporal
violation. Unlike the learned 19/20 convention, this conclusion follows from an
explicit property supplied by the caller.

## 7. Determinism and stack safety

All public order is canonical:

- source ranges and stable node identifiers order sites;
- ordered maps/sets aggregate identities and fragments;
- patterns and findings are sorted and deduplicated;
- property names order independent temporal analyses.

CFG reachability is breadth-first with a heap queue. AUG acyclicity is checked
by Kahn elimination with heap vectors and a queue. Typestate is a monotone heap
worklist. Consequently, native call-stack depth is independent of CFG depth,
population size, and graph depth.

## 8. Completeness theorem for the represented domain

For a validated configuration, if `ApiUsageMiningAnalysis::is_complete()` is
true, then:

1. every represented call site admitted by the inventory bound either produced
   a complete centered graph or would have created an issue;
2. every eligible population fragment was counted exactly;
3. every complete site deviating from an accepted fragment appears in the
   ordered finding set;
4. every delegated temporal report is complete under its FSM and typestate
   bounds.

The theorem is deliberately relative to represented CPG evidence. It says
nothing about calls omitted by a frontend, runtime reflection, infeasible CFG
paths, or developer intent.

## 9. Literate algorithm

The implementation follows this narrative:

```text
ANALYZE-API-USAGE(cpg, properties, config):
    Validate every threshold, cap, property name, and property FSM.
    Evaluate named temporal properties with the shared typestate worklist.
    Inventory calls; if the global cap is exceeded, report it and stop mining.

    For each call in canonical source order:
        Resolve its function owner and typed API identity.
        Project the owner's CFG.
        Traverse reachable nodes with a heap queue.
        Gather arguments, receiver, controllers, and later calls.
        If any evidence is absent or capped, record an issue and exclude site.
        Otherwise construct and retain its centered AUG.

    Partition complete observations by anchor API identity.
    For each sufficiently large population:
        Count each fragment family in ordered maps.
        Accept fragments meeting the exact strict-majority threshold.
        Emit one advisory for every complete non-supporting occurrence.

    Canonically sort and deduplicate every public collection.
    Return findings, supporting evidence, temporal analyses, and issues.
```

See [ADR-0047](../design/0047-evidence-bounded-api-usage-and-temporal-mining.md),
the [component contract](../components/graph/api-usage-protocol-mining.md), and
[Usage 20](../usage/20-api-usage-protocol-mining.md).

## References

1. Nielebock, S., Blockhaus, P., Krüger, J., Ortmeier, F. (2024).
   *ASAP-Repair: API-Specific Automated Program Repair Based on API Usage
   Graphs.* DOI:
   [10.1145/3643788.3648011](https://doi.org/10.1145/3643788.3648011).
2. Song, Y., Gao, X., Li, W., Chin, W.-N., Roychoudhury, A. (2024).
   *ProveNFix: Temporal Property-Guided Program Repair.* DOI:
   [10.1145/3643737](https://doi.org/10.1145/3643737).
3. Strom, R. E., Yemini, S. (1986). *Typestate: A Programming Language
   Concept for Enhancing Software Reliability.* DOI:
   [10.1109/TSE.1986.6312929](https://doi.org/10.1109/TSE.1986.6312929).
