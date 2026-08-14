# Defining and solving an IFDS/IDE summary domain

Use `analysis::ifds` when an analysis needs procedure summaries across resolved
calls, including direct or mutual recursion. The framework is feature-free and
read-only: construct or resolve the CPG first, compute its current call-graph
SCCs, then solve one domain.

## Complete reachability example

This compilable domain summarizes the represented functions reachable from
each function. It is intentionally simple, but exercises the same public
contract used by taint, effects, and typestate domains.

```rust
use std::collections::BTreeSet;

use libcpg::{
    call_graph_sccs, solve_summaries, CalleeRef, CodePropertyGraph,
    CpgNodeKind, IfdsCaps, IfdsLattice, NodeId, SummaryCtx, SummaryDomain,
};

#[derive(Clone, Default)]
struct ReachSummary(BTreeSet<NodeId>);

impl IfdsLattice for ReachSummary {
    fn join(&mut self, other: &Self) -> bool {
        let before = self.0.len();
        self.0.extend(other.0.iter().copied());
        self.0.len() != before
    }

    fn subsumes(&self, other: &Self) -> bool {
        self.0.is_superset(&other.0)
    }
}

struct Reachability;

impl SummaryDomain for Reachability {
    type Fact = NodeId;
    type Summary = ReachSummary;

    fn seed_facts(
        &self,
        _cpg: &CodePropertyGraph,
        function: NodeId,
    ) -> Vec<(NodeId, Self::Fact)> {
        vec![(function, function)]
    }

    fn summarize(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
        ctx: &SummaryCtx<'_, Self>,
    ) -> Self::Summary {
        let mut result = ReachSummary(BTreeSet::from([function]));
        let mut scope = cpg.ast_descendants(function);
        scope.sort_unstable();

        for call in scope.into_iter().filter(|&node| {
            cpg.node(node)
                .is_some_and(|n| matches!(n.kind, CpgNodeKind::Call { .. }))
        }) {
            if let CalleeRef::Resolved(callee) = (ctx.resolve)(call) {
                result.0.insert(callee);
                if let Some(summary) = ctx.summaries.get(&callee) {
                    result.0.extend(summary.0.iter().copied());
                }
            }
        }
        result
    }
}

fn reachable_functions(
    cpg: &CodePropertyGraph,
) -> Option<Vec<(NodeId, Vec<NodeId>)>> {
    let call_graph = call_graph_sccs(cpg);
    let output = solve_summaries(
        cpg,
        &Reachability,
        &call_graph,
        &IfdsCaps::default(),
    );
    if !output.is_complete() {
        return None;
    }

    Some(
        output
            .summaries
            .into_iter()
            .map(|(function, summary)| {
                (function, summary.0.into_iter().collect())
            })
            .collect(),
    )
}
```

`ast_descendants` is sufficient for this example because nested functions are
not inserted into an enclosing function's call sites by the fixture. A
production domain should use its own explicit function-scope traversal and stop
at nested functions, matching call-graph semantics.

## Construction order

The SCC value must describe the current call graph:

```text
build or merge CPGs
    -> build import evidence when multi-file
    -> resolve definite calls
    -> call_graph_sccs
    -> solve_summaries
```

If call resolution changes after the SCC step, the solver returns
`StaleCallGraph` instead of running. Recompute `call_graph_sccs`; do not remove
the diagnostic or hand-edit its public fields.

## Modeling unknown and external calls

The default resolver only promotes one uniquely represented target. Supply a
resolver when the caller owns stronger foreign-function or package evidence:

```rust
use libcpg::{
    call_graph_sccs, solve_summaries_with_resolver, CalleeRef,
    CodePropertyGraph, IfdsCaps, NodeId, SummaryDomain,
};

fn solve_with_ffi<D: SummaryDomain>(
    cpg: &CodePropertyGraph,
    domain: &D,
    known_ffi_call: NodeId,
) -> libcpg::IfdsOutput<D> {
    let resolver = |site: NodeId| {
        if site == known_ffi_call {
            CalleeRef::External("trusted-ffi-model".into())
        } else {
            CalleeRef::Unresolved(format!("call@{}", site.as_u32()).into())
        }
    };
    solve_summaries_with_resolver(
        cpg,
        domain,
        &call_graph_sccs(cpg),
        &IfdsCaps::default(),
        &resolver,
    )
}
```

The example labels an external boundary; it does not define its effect. A taint
domain might apply a catalog model or conservatively taint the return. An
effect domain might use a top/unknown effect. Treating either `External` or
`Unresolved` as no effect is a domain policy that requires explicit
justification.

## Choosing caps

Start from `IfdsCaps::default()` and adjust against measured program sizes:

```rust
use libcpg::IfdsCaps;

let caps = IfdsCaps {
    max_scc_iters: 128,
    max_facts_per_node: 8_192,
    max_worklist_steps: 2_000_000,
    max_call_depth: 128,
    max_functions: 200_000,
};
```

Do not treat a higher cap as a precision algorithm. It only permits more of the
same monotone computation. If `output.capped` is true, inspect
`output.cap_events` and either report partial evidence, increase a justified
bound, or reduce the represented program. Never turn a capped may-analysis into
a clean verdict.

## Diagnosing incomplete output

```rust
use libcpg::{IfdsOutput, SummaryDomain};

fn report<D: SummaryDomain>(output: &IfdsOutput<D>) {
    if output.is_complete() {
        println!("{} complete summaries", output.summaries.len());
        return;
    }
    for issue in &output.issues {
        eprintln!("invalid input/domain contract: {issue:?}");
    }
    for event in &output.cap_events {
        eprintln!("partial result: {event:?}");
    }
}
```

Formatting the concrete `IfdsIssue` and `IfdsCapEvent` values, as above, has no
extra domain bound.

## Domain test checklist

In addition to framework tests, every new domain should provide:

- hand-computed normal, recursive, external, unresolved, and clean examples;
- an independent local-flow or concrete-execution oracle;
- join-semilattice algebra properties;
- transfer monotonicity and, for IFDS/IDE, distributivity properties;
- insertion/permutation determinism;
- cap monotonicity and explicit partial-result behavior;
- malformed CPG/scope/call evidence;
- public API integration and `serde` round trips when enabled; and
- a Criterion fixture with an oracle asserted before timing.

## Further reading

- [Component guide](../components/graph/ifds-ide-framework.md)
- [Theory 25](../theory/25-ifds-ide-summary-solving.md)
- [ADR-0032](../design/0032-deterministic-ifds-ide-summaries.md)
- [Scientific validation 27](../scientific/27-ifds-ide-validation.md)

## References

1. Reps, T., Horwitz, S., Sagiv, M. (1995). *Precise Interprocedural Dataflow
   Analysis via Graph Reachability.* DOI:
   [10.1145/199448.199462](https://doi.org/10.1145/199448.199462).
2. Sagiv, M., Reps, T., Horwitz, S. (1996). *Precise Interprocedural Dataflow
   Analysis with Applications to Constant Propagation.* DOI:
   [10.1016/0304-3975(96)00072-2](https://doi.org/10.1016/0304-3975(96)00072-2).
