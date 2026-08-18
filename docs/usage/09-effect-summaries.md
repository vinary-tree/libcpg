# Usage 09 — Inspecting interprocedural effects

This guide shows how to compute and interpret constant/effect summaries from
an already built Code Property Graph (CPG). The API is feature-free.

## 1. Compute complete evidence

```rust
use libcpg::{
    analyze_effects, call_graph_sccs, CodePropertyGraph, EffectAnalysis,
    IfdsCaps,
};

fn effects(cpg: &CodePropertyGraph) -> EffectAnalysis {
    let call_graph = call_graph_sccs(cpg);
    analyze_effects(cpg, &call_graph, &IfdsCaps::default())
}
```

Always preserve the `EffectAnalysis` when a consumer might make a negative
claim. `is_complete()` is false when the call decomposition is malformed, the
shared solver reaches a cap, or a domain-local expression/dependence traversal
widens at a cap.

```rust
# use libcpg::{EffectAnalysis, NodeId, Purity};
fn is_proven_pure(result: &EffectAnalysis, function: NodeId) -> bool {
    result.is_complete()
        && result
            .summaries
            .get(&function)
            .is_some_and(|summary| summary.purity == Purity::Pure && !summary.capped)
}
```

Without the completeness check, `Pure` can mean only “no represented effect in
the evaluated prefix.”

## 2. Interpret one summary

```rust
# use libcpg::{EffectSummary, Place};
fn describe(summary: &EffectSummary) -> Vec<String> {
    let mut lines = Vec::new();
    lines.push(format!("purity: {:?}", summary.purity));
    for place in &summary.reads {
        lines.push(format!("reads {place:?}"));
    }
    for place in &summary.writes {
        lines.push(format!("writes {place:?}"));
    }
    if let Some(value) = &summary.const_return {
        lines.push(format!("returns constant {value:?}"));
    } else if summary.const_return_overdefined {
        lines.push("return is not one scalar constant".to_owned());
    } else {
        lines.push("return constant has no fixed-point evidence yet".to_owned());
    }
    lines
}
```

`Place::Param(i)` is deliberately not called global state. A parameter write
is remapped through each call site. A caller passing a global or field receives
that observable write; a caller passing a local scalar does not.

## 3. Read parameter-to-return dependence

```rust
# use libcpg::EffectSummary;
fn returned_parameters(summary: &EffectSummary) -> Vec<u8> {
    (0u8..64)
        .filter(|index| summary.param_to_return & (1u64 << index) != 0)
        .collect()
}
```

Check `param_mask_truncated` before treating a clear bit as absence. A function
with more than 64 parameters cannot encode every formal in the mask. When such
a callee participates in return composition, the analysis follows all caller
arguments conservatively and marks the aggregate incomplete.

## 4. Select resource bounds

```rust
use libcpg::IfdsCaps;

fn service_caps() -> IfdsCaps {
    IfdsCaps {
        max_scc_iters: 32,
        max_facts_per_node: 1_024,
        max_worklist_steps: 250_000,
        max_call_depth: 64,
        max_functions: 25_000,
    }
}
```

For this domain, `max_call_depth` is the maximum logical AST/def-use path and
`max_worklist_steps` is the maximum charged node entries across local
constant/dependence evaluation. The evaluators use typed heap continuation
frames, so neither setting configures or consumes a proportional native thread
stack. Raise `max_call_depth` when deeper represented evidence is in policy;
do not tune the process stack to change analysis semantics.

A local breach widens the parameter mask and return constant, sets the function
summary's `capped` bit, and adds the function to
`domain_capped_functions`. Failure to grow input-dependent machine state takes
the same fail-closed path. An active dependency cycle widens and caps; an
active constant cycle becomes overdefined without inventing a cap event.

Raising a cap cannot invalidate an already complete deterministic result. A
lower capped result may contain useful positive effects, but missing effects
are not a proof of purity.

## 5. Use the map-only convenience API

```rust
# use libcpg::{call_graph_sccs, effect_summaries, CodePropertyGraph, IfdsCaps, NodeId, EffectSummary};
# use std::collections::BTreeMap;
fn map_only(cpg: &CodePropertyGraph) -> BTreeMap<NodeId, EffectSummary> {
    let call_graph = call_graph_sccs(cpg);
    effect_summaries(cpg, &call_graph, &IfdsCaps::default())
}
```

This matches the original C5 task signature and is convenient for positive
display/indexing. It intentionally discards cap and structural issue evidence;
do not use it as the basis of a safety or purity verdict.

## 6. Build call evidence first

Parsed CPGs should run the call-resolution post-pass before computing the call
graph. A represented `Call::target`, `CallSite`, `StaticCall`, `DynamicCall`,
or CFG `Call` edge is accepted by the shared call graph. Ambiguous or external
calls remain topology-free and make the caller conservatively `Impure`.

```rust
# use libcpg::{resolve_calls, ResolveConfig, CodePropertyGraph};
fn resolve_before_effects(cpg: &mut CodePropertyGraph) {
    let _resolution = resolve_calls(cpg, &ResolveConfig::default());
}
```

## 7. Common mistakes

- Do not interpret `None` alone: inspect `const_return_overdefined` too.
- Do not assume a clear parameter bit is complete when
  `param_mask_truncated` or `capped` is true.
- Do not reuse an SCC decomposition after mutating call targets; the solver
  rejects stale decompositions.
- Do not infer globals from identifier spelling. Run the DFG/reference and
  call-resolution passes that create typed evidence.
- Do not treat `max_call_depth` as protection against native recursion. The
  effect evaluators are already stack-safe; the setting controls admitted
  logical evidence depth and the precision/completeness result.
- Do not mutate or sort returned maps into hash iteration order when stable
  serialization matters.

## 8. Further reading

- [Effect-summary component](../components/graph/effect-summaries.md)
- [Theory 26](../theory/26-interprocedural-effect-lattice.md)
- [ADR-0033](../design/0033-compositional-effect-summaries.md)
- [Scientific validation 28](../scientific/28-effect-summary-validation.md)
- [Security hardening](../security/01-input-and-resource-hardening.md)
