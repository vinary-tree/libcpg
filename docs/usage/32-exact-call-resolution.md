# Usage 32 -- exact evidence-bounded call resolution

Use `resolve_calls` when a CPG should gain call topology only from one exact,
represented target. The resolver is deliberately not a spelling corrector or
code-search interface.

## Direct post-pass

```rust
use libcpg::{resolve_calls, CodePropertyGraph, Confidence, ResolveConfig};

fn resolve_exact(cpg: &mut CodePropertyGraph) {
    let report = resolve_calls(cpg, &ResolveConfig::default());

    for (call, evidence) in &report.resolutions {
        match evidence.confidence {
            Confidence::ExactInFile
            | Confidence::ExactViaImport
            | Confidence::ExactViaBinding
            | Confidence::BareNameUnique => {
                assert!(evidence.target.is_some());
                println!("resolved {call:?} as {:?}", evidence.target);
            }
            Confidence::BareNameAmbiguous
            | Confidence::External
            | Confidence::Unresolved => {
                assert!(evidence.target.is_none());
            }
            Confidence::ClassHierarchy
            | Confidence::RapidType
            | Confidence::VariableType => {
                unreachable!("resolve_calls does not emit refinement tiers");
            }
        }
    }
}
```

`topology_edges` counts newly emitted `StaticCall`/`DynamicCall` and `CallSite`
edges. A second invocation reports the same per-call evidence and adds no
duplicate edges.

## Builder integration

Enable the same pass between control-flow graph (CFG) and data-flow graph (DFG)
construction:

```rust
use libcpg::{CpgBuilderConfig, Language, TreeSitterCpgBuilder};

fn build_with_exact_calls(source: &str) -> libcpg::CodePropertyGraph {
    TreeSitterCpgBuilder::with_config(
        CpgBuilderConfig::new().with_import_resolution(true),
    )
    .build(source, Language::Rust)
    .expect("valid Rust source must build")
}
```

The option defaults to `false`; enabling it intentionally adds uniquely
supported call/reference topology before DFG parameter and return edges.

## Configure resource bounds

```rust
use libcpg::ResolveConfig;

let config = ResolveConfig {
    max_scope_depth: 256,
    max_candidates_per_name: 32,
    max_calls: 50_000,
    resolve_methods: true,
};
```

The candidate cap limits retained identifiers, not the true count. If ten
functions share `run` and only one identifier is retained, the result is still
ambiguous with `candidate_count == 10` and `candidates_truncated == true`.
Never interpret the retained first candidate as a target.

## Exact-name examples

| Represented situation | Lookup result |
|---|---|
| nearest lexical scope contains one `parse` | `ExactInFile`, topology emitted |
| nearest lexical scope contains two `parse` functions | `BareNameAmbiguous`, no topology |
| `from math import add as sum` points to one declaration | call `sum` is `ExactViaImport` |
| no local/import match; one global `run` | `BareNameUnique` |
| no represented declaration for imported `remote` | `External` |
| graph contains `resolver`, call spells `resolvor` | `Unresolved`; no fuzzy correction |
| graph contains `Render`, call spells `render` | exact miss; no case folding |
| canonically equivalent Unicode spellings have different bytes | distinct names unless the frontend normalized both |

Qualification, shadowing, and import aliases are resolved by represented scope
and edge evidence, not by parsing delimiter characters out of the name string.

## What prefix and fuzzy measurements mean

The H14B evaluation measured prefix ranges and distance-two dictionary
intersection to decide whether a third-party index belonged in this exact
resolver. It did not add those operations to the API.

```text
RESOLVE_EXACT(call, spelling):
    candidates <- nearest exact lexical/import/global bucket
    if candidates.full_count = 1 and tier authorizes topology:
        emit the represented target
    else:
        emit no target

    never replace an empty bucket with PREFIX(spelling)
    never replace an empty bucket with EDIT_DISTANCE(spelling)
```

Use a separate caller-owned search/advisory component if a user interface needs
suggestions. Its result type must expose the metric, distance/work/candidate
caps, completeness, and the fact that candidates are advisory. Do not feed
suggestions back into `Call::target`.

## Repeated analysis and ownership

`resolve_calls` builds a deterministic call-local symbol table and drops it
when the invocation ends. Repeated calls do not share a hidden global index.
This keeps graph ownership and invalidation unambiguous: mutate the graph, then
invoke the resolver again to obtain a report for the new graph.

If an application needs multiple downstream analyses, retain the returned
`CallGraphResolution` for that same immutable graph snapshot. Do not combine a
report from one graph revision with another graph.

## Failure handling

- `BareNameAmbiguous`, `External`, and `Unresolved` are useful evidence, not
  exceptions.
- `candidates_truncated` means the report intentionally omits identifiers; the
  full count remains authoritative.
- A low `max_calls` processes canonical lowest `NodeId` calls and leaves the
  rest unchanged; it does not guess their targets.
- Cyclic malformed AST/scope edges terminate through visited sets.
- Long names and deep scopes use heap-backed iteration rather than recursive
  native calls.

See [the component guide](../components/graph/call-resolution.md) for the full
evidence algorithm, [API reference](../api/graph-reference.md) for signatures,
[Security 22](../security/22-resolver-symbol-index-boundary.md) for trust and
availability, and [ADR-0064](../design/0064-retain-ordered-resolver-symbol-index.md)
for the measured index decision.
