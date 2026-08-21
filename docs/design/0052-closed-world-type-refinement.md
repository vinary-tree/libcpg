# ADR-0052: Closed-world virtual-call type refinement

- **Status:** Accepted
- **Date:** 2026-08-20
- **Scope:** feature-free `analysis::resolve_types`

## Context

ADR-0006 resolves a call from lexical, import, existing-binding, or unique
bare-name evidence. That C1 pass intentionally cannot answer the object-oriented
question “which override runs for this receiver?” A member name can be shared by
a declared class, many subclasses, and interface implementations. Treating one
same-named method as definite would fabricate call topology; retaining every
same-named method ignores useful type evidence.

Three classical analyses occupy useful precision points. Class-hierarchy
analysis (CHA) admits compatible concrete subtypes [[1]](#references).
Rapid-type analysis (RTA) narrows that population to instantiated types
[[2]](#references). Variable-type analysis (VTA) narrows further using types
assigned to the receiver [[3]](#references). libcpg needs those refinements
without a full points-to analysis, source-text heuristics, native recursion, or
implicit graph mutation.

The graph previously represented constructor syntax only as an ordinary
`Call`. Once that normalization discarded allocation identity, downstream RTA
and VTA could recover it only by reparsing source—a layering violation and an
unreliable workaround.

## Decision

### 1. Preserve allocation semantics at the frontend boundary

`CpgNode::allocation_type: Option<Box<TypeInfo>>` records the normalized nominal
type allocated by a constructor expression. JavaScript and TypeScript
`new_expression` nodes read the grammar's `constructor` field; Java
`object_creation_expression` nodes read its `type` field. Other syntax remains
`None` until its frontend supplies an equally typed mapping.

The box bounds the inline size of every node and is transparent to Serde's wire
shape; callers use `allocation_type.as_deref()` for `Option<&TypeInfo>`. The
field is orthogonal to `CpgNodeKind::Call`, just as `message_kind` and
`lock_kind` preserve semantic roles without destroying shared call structure.
It defaults to `None` in legacy serde input.

### 2. Refine C1 records without mutating topology

```rust,ignore
pub fn devirtualize(
    cpg: &CodePropertyGraph,
    resolution: &CallGraphResolution,
    mode: DevirtualizationMode,
) -> CallGraphResolution;
```

The CPG is borrowed immutably. `devirtualize` copies non-method and insufficiently
typed records unchanged. For an admitted member call it replaces only the
out-of-graph candidate record. A singleton is exposed as `target`; no
`DynamicCall`, `CallSite`, or `Reference` edge is created or removed.

### 3. Use an explicit closed-world target lattice

Let $`T_{CHA}(c)`$ be the represented concrete subtype closure of call $`c`$'s
declared receiver type. Let $`A`$ be the represented nominal types named by
allocation metadata, and $`V(c)`$ the allocation types found in definitions
reaching the receiver. The runtime-type sets are

```math
T_{RTA}(c)=T_{CHA}(c)\cap A,
```

```math
T_{VTA}(c)=T_{RTA}(c)\cap V(c).
```

Consequently,

```math
T_{VTA}(c)\subseteq T_{RTA}(c)\subseteq T_{CHA}(c).
```

Dispatch selects a type's own non-static methods with the requested name;
otherwise it propagates inherited/default methods through `Inherits` and
`Implements` edges. Multiple-inheritance ambiguity is retained as a candidate
union, never guessed away. The same subset relation holds for target sets.

### 4. Fail closed at nominal boundaries

A `TypeInfo` name is admitted only when it identifies exactly one represented
`Class`, `Struct`, `Enum`, or `Trait`. Ambiguous and absent names do not trigger
terminal-name guessing, source parsing, or arbitrary selection. Hierarchy
edges are consumed only when both endpoints are represented nominal types and
use the direction subtype/implementor to supertype/interface.

Methods are owned by their nearest nominal ancestor or a uniquely resolved
`Impl::for_type`. Static and nested functions are not virtual candidates.

### 5. Keep VTA deliberately below points-to analysis

VTA reads normalized allocations in the receiver definition and in
`DefUse`/`ReachingDef` sources (plus reversed `UseDef` evidence). Each definition
subtree is traversed iteratively, stopping at nested function boundaries. It
does not model heap fields, interprocedural aliases, reflection, dynamic
loading, generic substitution, or context sensitivity. “VTA” here names this
documented represented assignment-type refinement, not a claim of full
flow-sensitive points-to precision.

### 6. Represent continuations as heap worklists

![Typed graph evidence flows through deterministic heap worklists to an immutable refined result.](../diagrams/type-refinement-pipeline.svg)

Subtype closure uses a `VecDeque`; assignment discovery uses an explicit
depth-first vector; dispatch propagation uses a queue and ordered sets. Visited
and queued sets terminate malformed AST and hierarchy cycles. No hierarchy or
definition depth becomes native call depth.

## Confidence contract

The ordered evidence tiers add `ClassHierarchy` (score 0.75), `RapidType`
(0.80), and `VariableType` (0.85) below `ExactViaBinding` and above
`BareNameUnique`. A singleton typed result is strong enough to identify one
closed-world target, but this API deliberately leaves materialization to a
separate caller decision.

An empty RTA or VTA set is a represented closed-world result, not an indexable
singleton and not proof about code absent from the CPG.

## Consequences

### Positive

- CHA, RTA, and VTA are available under `default = []`.
- Constructor evidence is normalized once, where grammar knowledge belongs.
- Candidate order and results are independent of edge insertion order.
- The subset lattice is explicit and property tested.
- Inherited methods survive allocation pruning correctly.
- Empty sets, malformed cycles, ambiguous names, and legacy serialization are
  total, observable cases.
- A 100,000-level hierarchy completes on a 256-KiB native stack.

### Trade-offs and limits

- The analysis assumes the represented graph is a closed world.
- Missing allocation, hierarchy, type, AST ownership, or DFG evidence can
  over- or under-prune the represented target set.
- RTA is allocation filtering, not whole-program reachability discovery.
- VTA is assignment-type filtering, not a points-to graph.
- Reflection, runtime loading, foreign dispatch, and language-specific method
  selection rules remain outside the model.
- Conservative unions can retain multiple inherited defaults.

### Rejected alternatives

- **Reparse constructor source in the analysis.** This duplicates grammar
  semantics downstream and fails on aliases and formatting.
- **Infer allocations from every ordinary call.** That fabricates runtime types.
- **Mutate dynamic-call edges during refinement.** The caller must inspect the
  closed-world assumption before materializing topology.
- **Filter methods only by their declaring type.** An allocated subtype without
  an override still dispatches to an inherited method.
- **Recursive hierarchy traversal.** User-controlled inheritance depth must not
  control native stack depth.
- **Claim points-to precision.** The represented evidence does not justify it.

## Verification

Example tests cover all three modes, inherited/default methods, `TypeOf`,
def-use assignments, empty allocations, ambiguous names, malformed cycles,
serde, JavaScript/TypeScript/Java constructor normalization, and graph
immutability. A 128-case property campaign checks insertion-order determinism
and $`VTA\subseteq RTA\subseteq CHA`$. Public integration starts with
`resolve_calls`; the stack gate uses 100,000 hierarchy levels on 256 KiB.

## References

1. Dean, J., Grove, D., Chambers, C. (1995). *Optimization of Object-Oriented
   Programs Using Static Class Hierarchy Analysis.* ECOOP 1995, 77–101. DOI:
   [10.1007/3-540-49538-X_5](https://doi.org/10.1007/3-540-49538-X_5).
2. Bacon, D. F., Sweeney, P. F. (1996). *Fast Static Analysis of C++ Virtual
   Function Calls.* OOPSLA 1996, 324–341. DOI:
   [10.1145/236337.236371](https://doi.org/10.1145/236337.236371).
3. Sundaresan, V., Hendren, L., Razafimahefa, C., Vallée-Rai, R., Lam, P.,
   Gagnon, E., Godin, C. (2000). *Practical Virtual Method Call Resolution for
   Java.* OOPSLA 2000, 264–280. DOI:
   [10.1145/353171.353189](https://doi.org/10.1145/353171.353189).
