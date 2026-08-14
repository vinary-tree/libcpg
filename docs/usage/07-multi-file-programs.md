# Building and resolving a multi-file program

Use the program API when analyses need calls or dependencies across multiple
per-file Code Property Graphs (CPGs). Composition is feature-free: each input
may come from an enabled internal grammar, caller-supplied Mode-B tree, a cache,
or hand-built graph.

## End-to-end workflow

```rust
use libcpg::{
    build_import_graph, merge_programs, resolve_calls, CodePropertyGraph,
    ImportGraph, ProgramCpg, ResolveConfig,
};

fn compose_and_resolve(
    files: Vec<CodePropertyGraph>,
) -> (ProgramCpg, ImportGraph) {
    // 1. Translate every file into one collision-free NodeId space.
    let mut program = merge_programs(files);

    // 2. Add exact represented file/declaration import evidence.
    let imports = build_import_graph(&mut program);

    // 3. Resolve calls after imports exist, so ExactViaImport can use them.
    let calls = resolve_calls(&mut program.cpg, &ResolveConfig::default());
    println!("resolved {} calls", calls.resolutions.len());

    (program, imports)
}
```

The order matters: `resolve_calls` can use cross-file declarations only after
`build_import_graph` emits declaration-level `Imports` edges. Calling it before
that pass is safe but may produce `BareNameUnique`, `BareNameAmbiguous`,
`External`, or `Unresolved` from weaker evidence.

Renamed imports are retained on the overlay itself. For example,
`from math import add as sum` creates an import-to-`add` edge labelled `sum`, so
a later call to `sum()` resolves as `ExactViaImport` without changing the
declaration's actual name.

## Translating saved node ids

If a caller kept a node id from file $`i`$, translate it through that file's
scope before querying the program graph:

```rust
use libcpg::{NodeId, ProgramCpg};

fn program_id(
    program: &ProgramCpg,
    file_index: usize,
    file_node: NodeId,
) -> Option<NodeId> {
    program.files.get(file_index)?.translated(file_node)
}
```

Never use a per-file `NodeId` directly against `program.cpg`; equal numeric ids
from different files name different syntax before translation.

## Interpreting import results

```rust
use libcpg::{ImportGraph, ImportResolutionKind};

fn print_imports(graph: &ImportGraph) {
    for resolution in graph.resolutions.values() {
        match resolution.kind {
            ImportResolutionKind::Exact => println!(
                "{} -> file {:?}, declarations {:?}",
                resolution.path,
                resolution.target_files,
                resolution.imported_nodes,
            ),
            ImportResolutionKind::Ambiguous => println!(
                "ambiguous {} among files {:?}",
                resolution.path,
                resolution.target_files,
            ),
            ImportResolutionKind::External => {
                println!("external or unrepresented: {}", resolution.path)
            }
        }
    }
}
```

`External` is local to the supplied program: it means no input `FileScope`
matched. It does not report that a dependency is unavailable on disk or through
a package manager. `Ambiguous` intentionally adds no file/import dependency
edge; add better source paths or module nodes to the per-file CPGs if the caller
has stronger identity evidence.

## Repeated construction

`build_import_graph` is idempotent. Re-running it is useful after appending
additional typed evidence to the existing program graph; unchanged inputs add
zero edges. `merge_from` is different: it explicitly appends a new disjoint copy
on every call, so invoke it only for files that should become additional program
units.

## Mixed-language programs

A homogeneous program retains its language on `program.cpg`. A heterogeneous
program uses `Language::Unknown` at graph level and retains the exact language
inside each `FileScope`. Analyses that depend only on node and edge kinds can
still run; callers must consult each analysis's documented language boundary.

## Further reading

- [Component semantics](../components/graph/program-composition.md)
- [Theory 24](../theory/24-program-composition.md)
- [Scientific validation 26](../scientific/26-program-composition-validation.md)
- [Call resolution](../components/graph/call-resolution.md)

## References

This usage guide describes the shipped API and makes no external
literature-dependent claim.
