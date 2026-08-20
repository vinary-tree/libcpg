# Virtual-call type refinement

`analysis::resolve_types` is the feature-free, immutable component that refines
C1 method-call candidates with class hierarchy, allocation, and receiver
assignment evidence. It does not parse source text and does not modify the Code
Property Graph (CPG).

## Public surface

| API | Role |
|---|---|
| `devirtualize` | Refines a `CallGraphResolution` from one selected evidence mode. |
| `DevirtualizationMode::Cha` | Uses all represented compatible concrete subtypes. |
| `DevirtualizationMode::Rta` | Intersects CHA types with represented allocations. |
| `DevirtualizationMode::Vta` | Intersects RTA types with allocations reaching the receiver. |
| `Confidence::{ClassHierarchy,RapidType,VariableType}` | Records the selected typed evidence tier. |
| `CpgNode::allocation_type` | Frontend-normalized nominal constructor result used by RTA/VTA. |

`serde` adds serialization for the mode, confidence variants, allocation
metadata, and existing resolution records without changing the algorithm.

## Input contract

The strongest input supplies:

- a C1 `CallGraphResolution` from `resolve_calls`;
- member calls whose first AST child is `MemberAccess` and whose first child is
  the receiver;
- receiver `TypeInfo`, `TypeOf`, definition, or `self`/`this` ownership;
- `Inherits` and `Implements` edges directed subtype to parent;
- methods nested beneath nominal nodes or uniquely named `Impl::for_type`;
- normalized `allocation_type` on constructor calls; and
- `DefUse`, `ReachingDef`, or reversed `UseDef` evidence for VTA.

Absent evidence is not synthesized. Ambiguous nominal names leave the C1
record unchanged.

## Pipeline

![The component indexes typed evidence, computes hierarchy dispatch, applies the selected refinement, and returns an immutable record.](../../diagrams/type-refinement-pipeline.svg)

```text
DEVIRTUALIZE(cpg, c1, mode):
    index := build sorted nominal, hierarchy, method, and allocation indices
    cache := empty dispatch table per method name
    output := empty CallGraphResolution

    for (call, record) in c1.resolutions ordered by NodeId:
        if call is not a typed member call:
            copy record; continue

        static_types := exact represented receiver types
        if static_types is empty:
            copy record; continue

        cha := iterative concrete descendant closure(static_types)
        runtime := choose mode:
            CHA => cha
            RTA => cha intersect all allocated types
            VTA => cha intersect all allocated types intersect receiver assignments

        dispatch := cached iterative dispatch table(record.callee_name)
        candidates := sorted union of dispatch[type] for type in runtime
        replace candidate fields and typed confidence
        target := the sole candidate, only when exactly one exists

    return output with topology_edges = 0
```

## Result semantics

| Candidate count | Meaning in the selected closed world | `target` |
|---:|---|---|
| 0 | no represented runtime type dispatches to a represented method | `None` |
| 1 | one represented target remains | that `NodeId` |
| 2 or more | dynamic dispatch remains ambiguous | `None` |

`candidate_count` is exact for the computed type mode and
`candidates_truncated` is false. `topology_edges` is zero because this immutable
pass emits none. Aggregate ambiguous/unresolved/external counts are recomputed
from the returned records.

## Stack and determinism contract

- subtype closure: `VecDeque` plus an ordered visited set;
- dispatch fixed point: queue plus ordered per-type target sets;
- VTA definition traversal: explicit vector plus visited set;
- owner walks: iterative parent chains with cycle detection;
- stable input/output order: `BTreeMap`, `BTreeSet`, sorted/deduplicated edges.

Malformed cycles terminate. The public adversarial gate crosses 100,000
inheritance levels on a 256-KiB native stack.

## Cross-links

See [Theory 42](../../theory/42-closed-world-type-refinement.md),
[ADR-0052](../../design/0052-closed-world-type-refinement.md),
[Usage 25](../../usage/25-virtual-call-type-refinement.md), the
[scientific dossier](../../scientific/47-type-refinement-validation.md), and the
[security boundary](../../security/17-type-refinement-boundary.md).
