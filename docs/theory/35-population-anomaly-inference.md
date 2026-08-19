# Theory 35 — Exact population anomaly inference

Missing checks and unusual API use are relational defects: one call site is
suspicious because comparable sites normally carry evidence that it lacks.
This chapter defines libcpg's feature-free I6 missing-check and I7
deviant-behavior analyses. Both are deterministic, read-only advisory
functions over a represented Code Property Graph (CPG), not correctness or
vulnerability proofs.

![Typed evidence becomes exact population beliefs and qualified deviations](../diagrams/population-anomaly-pipeline.svg)

## 1. Evidence universe

Let $`G=(V,E)`$ be a finite CPG. A call occurrence $`u\in U\subseteq V`$ has:

- an owner function $`f(u)`$ found through represented AST scope;
- an API identity $`a(u)`$;
- zero or more represented control checks $`C(u)`$;
- an intraprocedural CFG position; and
- optional forward value evidence into a guard.

An **API identity** is either the sorted set of represented function targets,
with a human-readable name, or a typed callee path when no target exists.
Resolved target sets are authoritative: equal spelling does not merge distinct
represented overloads or dispatch alternatives.

A **check fingerprint** is a flat postorder tape. Every tape item records a
normalized semantic node kind and its direct-child count. Local identifiers
and declaration names are abstracted; operators, literals, member and API
names, nominal types, imports, attributes, macros, and opaque frontend kinds
remain exact. Thus `value > 0` and `candidate > 0` can be peers, while
`value > 0` and `value != 0` are distinct.

## 2. Exact support

For a finite comparable population $`P`$ and behavior predicate $`b`$, define:

```math
\operatorname{support}(b,P)
=\frac{|\{u\in P\mid b(u)\}|}{|P|}.
```

`PopulationFraction` stores the two integers without reduction. The candidate
occurrence remains in the denominator, so four guarded uses and one unguarded
use produce exactly `4/5`. A policy threshold $`s/d`$ accepts observed support
$`n/p`$ precisely when:

```math
n d \geq s p.
```

The implementation widens each product to `u128`. It never converts the
decision to floating point; `PopulationFraction::ratio()` exists only for
presentation.

![The acceptance oracles retain the deviating occurrence in each denominator](../diagrams/population-support-oracles.svg)

## 3. I6 missing-check inference

The caller-owned `TaintCatalog` determines which callee paths are sensitive
sinks. An optional class set narrows those rules. For each admitted call,
control checks are represented `ControlDependence` predecessors of the call or
one of its AST ancestors. A call used directly as a branching guard also
counts as a check.

For sensitive site $`u`$, let $`P(u)`$ contain same-API sites selected by one
of two policies:

- `WholeProgram`: all complete same-identity sensitive sites;
- `CallGraph { max_distance: r }`: sites whose owners lie within undirected
  call-graph distance $`r`$ of $`f(u)`$.

For each fingerprint $`c`$ found in $`P(u)`$, I6 reports it as expected at
$`u`$ when:

```math
|P(u)| \geq m
\quad\land\quad
\operatorname{support}(c,P(u)) \geq \tau
\quad\land\quad
c\notin C(u),
```

where $`m`$ is `minimum_population` and $`\tau`$ is the exact threshold.
Each site contributes at most one vote per fingerprint. A `MissingCheck`
retains the site, function, API, absent fingerprints, exact fractions, peer
sites, and representative guard nodes.

```text
collect complete sensitive calls in stable node order
for each call:
    collect represented controlling guards
    normalize each guard with a heap-owned postorder frame machine
for each call and each peer-supported fingerprint:
    if the call lacks that fingerprint, emit evidence and witnesses
```

This follows the comparative condition lineage of Chucky and the semantic and
contextual peer reasoning of CRIX [[1]](#references), [[2]](#references).

## 4. I7 implicit beliefs and deviations

Following Engler et al., a high-support program behavior is treated as an
implicit programmer belief, and a violating occurrence becomes review
evidence [[3]](#references). libcpg infers two belief families.

### 4.1 Call order

For an occurrence of predecessor API $`A`$, let $`F(u)`$ be the set of API
identities appearing on a later CFG node reachable within the same function.
For every $`B`$ with threshold support among complete $`A`$ occurrences, infer
`CallOrderBelief(A, B)`. An occurrence with $`B\notin F(u)`$ produces
`MissingFollowingCall { expected: B }`.

This relation is reachability, not immediate adjacency. It respects branches
represented by the CFG and deliberately makes no claim about runtime-feasible
path conditions.

### 4.2 Return-value checking

For call $`u`$, define $`checked(u)`$ when a forward walk from its result over
represented DFG or PDG data-dependence edges reaches a guard. `UseDef` edges
are excluded because they point from a use back to a definition and would
reverse the intended value-flow question.

If the exact checked fraction for API $`A`$ meets the policy threshold, infer
`ReturnCheckBelief(A)`. Every complete unchecked occurrence then yields
`UncheckedReturn` with the exact population support and supporting sites.

## 5. Completeness semantics

Only complete occurrences vote. The reports retain typed issues for:

- calls outside represented function scope;
- absent or capped callee identity;
- cyclic, missing, oversized, or unrepresentable check AST evidence;
- calls absent from their owner's CFG; and
- exhausted CFG or return-evidence node budgets.

Excluding an incomplete occurrence prevents it from silently changing one
side of a fraction. `is_complete()` is true exactly when the issue set is
empty. Convenience finding vectors discard that qualification and therefore
cannot justify a negative claim.

## 6. Determinism, termination, and stack safety

All populations, fingerprints, identities, beliefs, findings, witnesses, and
issues use canonical ordering. Exact integer arithmetic removes boundary
rounding. Reordering equivalent graph insertions cannot change a report.

Every input-sized traversal uses a heap worklist, visited set, or flat tape:
AST fingerprint construction is an explicit postorder frame machine; call
neighborhoods, CFG reachability, and value reachability are iterative. Cycles
terminate through visited-state checks, and explicit budgets terminate large
valid traversals. Represented source or graph depth therefore does not become
native Rust call depth.

## 7. Interpretation limits

A dominant behavior may itself be wrong, an uncommon behavior may be correct,
and unrelated contexts may share an API identity. Missing frontend, resolver,
CFG, DFG, PDG, call, or catalog evidence can hide behavior entirely. These
analyses rank exact represented deviations for review; they do not prove path
feasibility, policy correctness, exploitability, or the absence of defects.

## References

1. Yamaguchi, F., Wressnegger, C., Gascon, H., Rieck, K. (2013). *Chucky:
   Exposing Missing Checks in Source Code for Vulnerability Discovery.* ACM
   CCS, 499–510. DOI:
   [10.1145/2508859.2516665](https://doi.org/10.1145/2508859.2516665).
2. Lu, K., Pakki, A., Wu, Q. (2019). *Detecting Missing-Check Bugs via
   Semantic- and Context-Aware Criticalness and Constraints Inferences.* 28th
   USENIX Security Symposium, 1769–1786.
   [USEN proceedings page](https://www.usenix.org/conference/usenixsecurity19/presentation/lu).
3. Engler, D., Chen, D. Y., Hallem, S., Chou, A., Chelf, B. (2001). *Bugs as
   Deviant Behavior: A General Approach to Inferring Errors in Systems Code.*
   SOSP, 57–72. DOI:
   [10.1145/502034.502041](https://doi.org/10.1145/502034.502041).
