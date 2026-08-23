# Exporting Datalog facts

Use Datalog export when an external rule engine should query the represented
CPG relations. The exporter is feature-free; parsing source still requires the
appropriate `lang-*` feature or a caller-supplied tree.

## Export in memory

```rust
use libcpg::{export_datalog, CodePropertyGraph};

fn inspect(cpg: &CodePropertyGraph) {
    let facts = export_datalog(cpg);
    println!("{} nodes, {} exact edges", facts.nodes.len(), facts.edges.len());
    for file in facts.souffle_files() {
        println!("{}.facts: {} bytes", file.relation, file.contents.len());
    }
}
```

`facts.ast_child`, `facts.cfg`, `facts.dfg`, and
the other typed vectors can be queried directly from Rust.
`facts.cpg_edges()` reconstructs the exact canonical edge snapshot.

## Write a Soufflé input directory

libcpg deliberately performs no I/O. A consumer can apply its own atomic-write
and permission policy:

```rust
use std::{fs, io, path::Path};
use libcpg::{export_datalog, CodePropertyGraph};

fn write_facts(cpg: &CodePropertyGraph, directory: &Path) -> io::Result<()> {
    fs::create_dir_all(directory)?;
    for file in export_datalog(cpg).souffle_files() {
        let path = directory.join(format!("{}.facts", file.relation));
        fs::write(path, file.contents)?;
    }
    Ok(())
}
```

The shipped rules are
[`examples/datalog/doop_lite.dl`](../../examples/datalog/doop_lite.dl)
and [`examples/datalog/ifc.dl`](../../examples/datalog/ifc.dl).
They declare `rfc4180=true` with a tab delimiter because the exporter
quotes every symbol as an RFC 4180 field. Caller-authored input relations,
including the IFC example's `high` and `low` facts, must use the same
directive and encoding.
With Soufflé installed, a typical external invocation is:

```text
souffle -F path/to/facts -D path/to/output examples/datalog/doop_lite.dl
```

The caller must bound the engine's time, memory, output directory, and rule
trust. An empty derived relation means “no consequence from the represented
facts and supplied rules,” not “the source program can never exhibit this
behavior.”

## References

1. Bravenboer, M., Smaragdakis, Y. (2009). *Strictly Declarative Specification
   of Sophisticated Points-to Analyses.* OOPSLA. DOI:
   [10.1145/1640089.1640108](https://doi.org/10.1145/1640089.1640108).
2. Jordan, H., Scholz, B., Subotić, P. (2016). *Soufflé: On Synthesis of Program
   Analyzers.* CC. DOI:
   [10.1007/978-3-319-49834-1_23](https://doi.org/10.1007/978-3-319-49834-1_23).
