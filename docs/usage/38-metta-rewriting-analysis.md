# Usage 38 -- Analyze MeTTa termination and critical pairs

Enable feature `metta`, build the Code Property Graph (CPG) from a
caller-owned MeTTa Tree-sitter tree through Mode B, and retain qualified
results whenever absence matters. The feature activates exact
`libdictenstein` rule indexing but does not link a MeTTa grammar.

## Build and decide termination

```rust,ignore
use libcpg::{
    terminates, Language, TerminationVerdict, TreeSitterCpgBuilder,
};

fn analyze(source: &str) -> Result<(), libcpg::Error> {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_metta::LANGUAGE.into())
        .expect("load caller-owned MeTTa grammar");
    let tree = parser.parse(source, None).expect("parse MeTTa");
    let cpg = TreeSitterCpgBuilder::new()
        .build_from_tree(&tree, source, Language::MeTTa)?;

    match terminates(&cpg) {
        TerminationVerdict::Terminates(proof) => {
            println!("proved with {:?}", proof.reduction_pairs);
        }
        TerminationVerdict::DoesNotTerminate(witness) => {
            println!("repeating rule/call sites: {witness:?}");
        }
        TerminationVerdict::Unknown(reason) => {
            println!("unoriented or incomplete: {reason:?}");
        }
    }
    Ok(())
}
```

A decreasing mutual recursion such as
`f(S(x)) -> g(x), g(S(x)) -> f(x)` satisfies the fixed reduction pair. A rule
`loop(x) -> loop(x)` yields a concrete self-loop witness. Other cyclic systems
may correctly return `Unknown`.

## Inspect dependency SCCs

```rust,ignore
use libcpg::{dependency_pairs, CodePropertyGraph};

fn print_cycles(cpg: &CodePropertyGraph) {
    let graph = dependency_pairs(cpg);
    for component in graph.sccs.cyclic_components() {
        println!("cyclic rules: {:?}", component.nodes);
    }
    if !graph.complete {
        eprintln!("partial dependency graph: {:?}", graph.issues);
    }
}
```

`DpGraph::projection` is the canonical
`ProjectionKind::RewriteDependency` CSR view. Its SCC decomposition is produced
by libcpg's existing iterative engine.

## Analyze confluence evidence

```rust,ignore
use libcpg::{analyze_critical_pairs, CodePropertyGraph};

fn print_conflicts(cpg: &CodePropertyGraph) {
    let report = analyze_critical_pairs(cpg);
    for pair in &report.pairs {
        println!("non-joinable critical overlap: {pair:?}");
    }
    if !report.complete {
        eprintln!("no negative conclusion: {:?}", report.issues);
    }
}
```

Two rules `pick(x) -> A` and `pick(x) -> B` produce a conflict when `A` and
`B` are distinct normal forms. A join rule can make the peak joinable.
`critical_pairs(cpg)` returns only positive conflicts; use the qualified report
for any empty-result interpretation. A confluence argument additionally needs
a termination proof and the documented left-linear represented fragment.

## Materialize RewriteDep

```rust,ignore
use libcpg::{
    analyze_critical_pairs, dependency_pairs, materialize_rewrite_dep_overlay,
    CodePropertyGraph, RewriteDepOverlayError,
};

fn add_rewrite_edges(
    cpg: &mut CodePropertyGraph,
) -> Result<usize, RewriteDepOverlayError> {
    let dependencies = dependency_pairs(cpg);
    let critical = analyze_critical_pairs(cpg);
    materialize_rewrite_dep_overlay(cpg, &dependencies, &critical)
}
```

Every endpoint is validated before mutation. Repeating the call adds zero
edges. Query `rewrite_dep_edges`, `rewrite_dep_successors`, or
`rewrite_dep_predecessors` afterward.

## Set service bounds

```rust
use libcpg::RewriteAnalysisCaps;

let caps = RewriteAnalysisCaps {
    max_rules: 50_000,
    max_term_nodes: 500_000,
    max_dependency_pairs: 500_000,
    max_unification_steps: 2_000_000,
    max_critical_pairs: 100_000,
    max_joinability_states: 250_000,
};
assert_eq!(caps.max_joinability_states, 250_000);
```

Pass the value to the three `_with_caps` functions. A reached cap makes the
result incomplete or unknown; never translate that into “terminating” or
“confluent.” Heap-backed stack safety does not bound resident memory, so hosts
must also enforce process memory and time policy.

## Related documentation

- [Theory](../theory/53-dependency-pairs-and-critical-pair-confluence.md)
- [Component contract](../components/graph/metta-rewriting-analysis.md)
- [ADR-0070](../design/0070-stack-safe-metta-rewriting-analysis.md)
- [Security boundary](../security/28-metta-rewriting-boundary.md)
- [Validation](../scientific/65-metta-rewriting-validation.md)
