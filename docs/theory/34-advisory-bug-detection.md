# Theory 34 — Advisory resource, definition, and exception analysis

This chapter gives the common mathematical model for libcpg's resource,
definition, and exception detector families. [Theory 35](35-population-anomaly-inference.md)
defines the separate population-based missing-check and deviant-behavior
families; [Theory 36](36-clone-consistency-and-abstract-feasibility.md) defines
the clone-consistency and path-feasibility families; [Theory
37](37-api-usage-graphs-and-temporal-properties.md) defines centered API-usage
and named temporal evidence. A **candidate** is a
reproducible consequence of represented Code
Property Graph (CPG) evidence. It is advisory: it neither proves runtime
feasibility nor assigns security impact. A report supports a negative claim
only under its documented completeness predicate.

![Normalized evidence fans into three deterministic advisory analyses](../diagrams/bug-detector-pipeline.svg)

## 1. Shared evidence boundary

Let $`G=(V,E)`$ be the finite CPG. Nodes in $`V`$ have stable identifiers and
typed syntax roles; edges in $`E`$ belong to typed Abstract Syntax Tree (AST),
Control Flow Graph (CFG), Data Flow Graph (DFG), call, type, scope, or
inheritance relations. Each detector is a read-only function of $`G`$ and an
explicit policy or cap value:

```math
D:G\times P\times B\longrightarrow (F,Q).
```

Here $`P`$ is caller policy when one is required, $`B`$ is a finite resource
budget, $`F`$ is ordered finding evidence, and $`Q`$ is a qualification report.
The equation does not claim that $`G`$ models every runtime behavior. It states
that equal normalized evidence and policy produce equal values.

## 2. Resource leaks as terminal typestate projection

A **resource protocol** is a finite-state property whose allocation events
create abstract objects and whose accepting states describe legal function
exit. [Theory 33](33-finite-state-typestate-properties.md) defines its state set
$`Q`$, accepting subset $`A\subseteq Q`$, object set $`O`$, and CFG transfer.

Typestate returns terminal evidence
$`T\subseteq O\times X\times Q\times V^*`$, where $`X`$ is the set of semantic
function exits and $`V^*`$ is a finite node witness. Resource-leak candidates
are exactly the non-accepting projection:

```math
L=\{(o,x,q,w)\in T\mid q\notin A\}.
```

This definition separates two defects. A missing ordinary transition is a
typestate violation at an event; a legal but non-accepting state at an exit is a
leak candidate. `return` and `throw` are semantic exits. Builder-recorded
function exits are authoritative; a structural sink is used only for a legacy
graph with no represented semantic exit. Expression-evaluation side sinks are
therefore not mislabeled as function termination.

Shadab et al. describe modular resource-leak checking as a static-analysis
problem and motivate explicit acquisition/release reasoning
[[1]](#references). libcpg deliberately leaves the protocol catalog to its
caller and exposes the underlying typestate qualifications rather than claiming
a built-in whole-language ownership model.

## 3. Uninitialized use as empty scoped reaching definitions

For a represented function $`f`$, let $`S_f\subseteq V`$ be its lexical scope,
$`U_f\subseteq V`$ its binder-aware executable identifier uses, and
$`R(u)\subseteq V`$ the definitions reaching use $`u`$ through represented DFG
evidence. A use is a candidate exactly when no reaching definition belongs to
the same function:

```math
U_{\mathrm{uninit}}=
\{u\in U_f\mid R(u)\cap S_f=\varnothing\}.
```

Definitions in $`R(u)\setminus S_f`$ are retained as evidence: they explain why
a name may appear globally connected while remaining uninitialized in the
current function. The use set excludes unbound function/method callee syntax
and unbound identifiers in the declaration signature before the body block.
Bound function-valued variables and executable default expressions remain
eligible.

Kildall's finite monotone framework supplies the classical fixed-point basis
for reaching definitions [[2]](#references). Reps, Horwitz, and Sagiv include
possibly-uninitialized variables among the finite distributive interprocedural
problems covered by graph reachability [[3]](#references). libcpg's detector is
intentionally narrower: it queries the normalized reaching-definition relation
already stored in $`G`$ instead of solving another private data-flow graph.

## 4. Exception summaries

An **exception summary** maps a function to the finite set of nominal exception
types that may leave it. Let $`\mathcal T`$ be the set of normalized nominal
types plus `Unknown`, $`H_f(t)`$ mean that a lexical handler in function $`f`$
accepts type $`t`$, $`D_f`$ be direct throws, and $`C_f`$ be represented call
sites. For call $`c`$, $`g_c`$ is its unique represented callee. The transfer is:

```math
E_f=
\{t\in D_f\mid\neg H_f(t)\}
\cup
\bigcup_{c\in C_f}
\{t\in E_{g_c}\mid\neg H_{f,c}(t)\}.
```

The lattice is $`(2^{\mathcal T},\subseteq,\cup,\varnothing)`$. It is finite,
and union is associative, commutative, and idempotent. The shared SCC solver
evaluates acyclic call components once in callee-first order and recursive
components by bounded Kleene iteration:

```math
E_f^{(k+1)}=E_f^{(k)}\cup F_f(E^{(k)}).
```

This is the same finite-summary foundation described by Reps, Horwitz, and
Sagiv [[3]](#references), specialized to exception-type sets. Mutual recursion
is an SCC fixed point, not a recursive Rust call.

![Exception types propagate through call SCC summaries and lexical handler dispatch](../diagrams/exception-flow-summary.svg)

## 5. Handler semantics

A **handler layer** is the source-ordered catch list attached to one lexically
enclosing `try` whose protected body contains the throw or call site. Dispatch
examines the nearest layer first and chooses its first accepting catch. If no
catch accepts, it continues to the next enclosing layer.

A typed catch accepting $`a`$ handles thrown $`t`$ when $`t=a`$ or a finite
walk over represented `Inherits`/`Implements` edges reaches $`a`$. An untyped
catch accepts every type, including `Unknown`. Cycles in malformed inheritance
evidence terminate through a visited set.

A **root call component** is an SCC with zero incoming edges in the call
condensation graph. Every function in such a component is an entry candidate.
An escaping type at such a function yields `UncaughtException` with a canonical
shortest call/throw witness. A catch absent from the reached-handler map yields
`DeadCatch`.

Sinha and Harrold show why exception constructs must participate explicitly in
control-flow and interprocedural program representations [[4]](#references).
libcpg accordingly emits normalized `Throw`/`Catch` nodes and a typed `Catch`
handler-entry CFG edge rather than reconstructing exception syntax in the
analysis.

## 6. Literate algorithms

The resource and use detectors tell their evidence story directly:

```text
To project resource leaks:
    Validate and run the caller's finite typestate property.
    For each canonical terminal object/exit/state witness:
        Retain it exactly when the state is not accepting.
    Return leak candidates together with the entire typestate report.

To find uninitialized uses:
    Enumerate functions in stable node order.
    Build the shared binder-aware variable facts for each function.
    Remove declaration-signature and unbound callee syntax from executable uses.
    For each remaining use:
        Partition reaching definitions by membership in the function scope.
        Emit the use exactly when the in-scope partition is empty.
```

The exception detector composes finite summaries and evidence reconstruction:

```text
Normalize direct throw types and represented call targets.
Build and validate the call-graph SCC decomposition.
Solve finite escaping-type summaries in callee-first component order.
For every processed function and each direct/callee exception origin:
    Search lexical handler layers from nearest to farthest.
    Record the first accepting catch, if any.
    Otherwise, at a root call component, reconstruct the canonical throw witness.
Enumerate every represented catch not reached by any origin as dead.
Return summaries, findings, reached handlers, work, caps, and issues.
```

All queues, visited sets, predecessor paths, and inheritance frontiers live on
the heap. Stable ordered collections define tie-breaking without an input-sized
native-stack sort.

## 7. Termination and cost

Leak projection is linear in the typestate terminal count after typestate's own
bounded fixed point. Uninitialized-use projection scans function scopes,
variable facts, and reaching definitions once, modulo ordered-container costs.

For exceptions, let $`F`$ be represented functions, $`C`$ represented call
sites, $`T`$ admitted exception types, $`I`$ SCC rounds, and $`H`$ total handler
and inheritance evidence examined per domain evaluation. The bounded shape is:

```math
O(I(F+C)(T+H)\log T)
```

plus canonical witness search. `IfdsCaps` bounds functions, facts per function,
summary work, and SCC rounds. Call-depth is not consumed because the algorithm
propagates summaries over SCCs rather than recursively expanding call paths.

## 8. Advisory interpretation

- `ResourceLeakAnalysis::is_complete()` requires a nonempty accepting policy
  and complete underlying typestate evidence.
- `UninitializedUseAnalysis::is_complete()` is always true for the finite
  represented relation; it makes no claim about omitted runtime semantics.
- `ExceptionFlowAnalysis::is_complete()` requires no shared cap and no solver,
  type, call, catch-edge, or witness issue.

Convenience projections discard these qualifications. They are suitable for
positive searches, not for asserting absence.

## References

1. Shadab, N., Gharat, P., Ernst, M. D., Kellogg, M., Lahiri, S. K., Lal, A.,
   Sridharan, M. (2025). *Lightweight and Modular Resource Leak Checking
   (Extended Version).* DOI:
   [10.1007/s10009-025-00804-2](https://doi.org/10.1007/s10009-025-00804-2).
2. Kildall, G. A. (1973). *A Unified Approach to Global Program Optimization.*
   POPL. DOI: [10.1145/512927.512945](https://doi.org/10.1145/512927.512945).
3. Reps, T., Horwitz, S., Sagiv, S. (1995). *Precise Interprocedural Dataflow
   Analysis via Graph Reachability.* POPL. DOI:
   [10.1145/199448.199462](https://doi.org/10.1145/199448.199462).
4. Sinha, S., Harrold, M. J. (2000). *Analysis and Testing of Programs with
   Exception Handling Constructs.* DOI:
   [10.1109/32.877846](https://doi.org/10.1109/32.877846).
