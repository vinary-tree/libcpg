# Usage 25 — Virtual-call type refinement

Use type refinement after C1 call resolution when a member call has represented
receiver and hierarchy evidence. Choose the least precise mode that satisfies
the downstream task, and retain the mode beside the result.

## 1. Build the required evidence

For parsed JavaScript, TypeScript, and Java, constructor syntax carries
`CpgNode::allocation_type` automatically. Hierarchy and type relations must be
present as `Inherits`, `Implements`, and optionally `TypeOf` edges. The edge
direction is subtype/implementor to parent/interface.

The current analysis does not infer hierarchy by reparsing source. A frontend,
adapter, or caller constructing CPGs manually must provide those edges and AST
ownership honestly.

## 2. Resolve C1 before refining

```rust,ignore
use libcpg::{devirtualize, resolve_calls, DevirtualizationMode, ResolveConfig};

let c1 = resolve_calls(&mut cpg, &ResolveConfig::default());
let cha = devirtualize(&cpg, &c1, DevirtualizationMode::Cha);
let rta = devirtualize(&cpg, &c1, DevirtualizationMode::Rta);
let vta = devirtualize(&cpg, &c1, DevirtualizationMode::Vta);
```

`resolve_calls` is mutable because it can materialize uniquely supported C1
topology. `devirtualize` accepts `&CodePropertyGraph` and never mutates it.

## 3. Choose a mode

| Mode | Runtime types retained | Use when |
|---|---|---|
| CHA | all represented compatible concrete subtypes | allocation/DFG evidence is unavailable or a broad conservative review set is desired |
| RTA | CHA types with any represented allocation | the analyzed CPG is a credible closed-world allocation snapshot |
| VTA | RTA types allocated in definitions reaching this receiver | receiver-local DFG evidence is credible and narrower review sets are useful |

The documented relationship is

```math
Candidates_{VTA}(c)\subseteq Candidates_{RTA}(c)\subseteq Candidates_{CHA}(c).
```

If that relationship appears violated, preserve the graph and report: it is an
implementation or evidence-model defect, not a reason to reorder results.

## 4. Inspect zero, one, and many targets

```rust,ignore
for (call, record) in &vta.resolutions {
    match record.candidates.as_slice() {
        [] => println!("{call:?}: no represented VTA target"),
        [target] => println!("{call:?}: singleton target {target:?}"),
        targets => println!("{call:?}: {} dynamic targets", targets.len()),
    }
}
```

An empty RTA/VTA set means no target exists in the represented closed world.
It does not prove that runtime loading, reflection, native code, omitted
libraries, or missing allocation metadata cannot provide a target.

## 5. Interpret inherited methods correctly

Do not filter candidate methods merely by whether their declaring classes were
allocated. If `Leaf` is allocated but does not override `Base::run`, its
dispatch target is the inherited `Base::run`. libcpg propagates dispatch before
collecting targets for allocated runtime types.

Interfaces behave similarly: an `Implements` edge can carry a represented
default interface method to an implementing concrete type unless the concrete
type overrides it.

## 6. Materialize topology only under caller policy

`devirtualize` intentionally returns out-of-graph evidence. If another layer
chooses to append `DynamicCall`/`CallSite` edges for singleton results, that
layer must record:

1. source revision and graph-build configuration;
2. selected CHA/RTA/VTA mode;
3. closed-world justification and library coverage;
4. call and target identifiers; and
5. invalidation rules when hierarchy, allocation, or DFG evidence changes.

Never delete broader call edges merely because a narrower represented result
was computed under a stronger assumption.

## 7. Operational checklist

```text
Run resolve_calls and preserve its complete C1 report.
Confirm receiver TypeInfo/TypeOf and subtype-to-parent hierarchy direction.
Audit allocation metadata before selecting RTA.
Audit reaching definitions before selecting VTA.
Run devirtualize and retain the selected mode with the report.
Check VTA subset RTA subset CHA for diagnostic runs.
Treat empty and singleton sets as closed-world evidence, not runtime proof.
Keep graph materialization a separate, append-only policy decision.
```

See the [component reference](../components/graph/type-refinement.md) and
[security boundary](../security/17-type-refinement-boundary.md).
