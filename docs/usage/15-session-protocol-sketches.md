# Usage 15 — Inspect Rholang session-protocol sketches

`session_sketches` requires the `rholang` feature. It returns deterministic
per-channel endpoint traces and an advisory three-valued duality result. The
analyzer consumes the CPG; it does not parse Rholang source itself.

## 1. Query sketches

```rust
// requires: features = ["rholang"]
use libcpg::{session_sketches, CodePropertyGraph, SessionConformance};

fn inspect_sessions(cpg: &CodePropertyGraph) {
    for sketch in session_sketches(cpg) {
        println!(
            "channel {:?}: {}",
            sketch.channel,
            sketch.conformance.as_str()
        );
        for endpoint in &sketch.endpoints {
            println!("  process {:?}", endpoint.process);
            for action in &endpoint.actions {
                println!(
                    "    {} {:?} {:?}",
                    action.direction.as_str(),
                    action.payload_types,
                    action.range
                );
            }
        }
        if sketch.conformance != SessionConformance::Conformant {
            for issue in &sketch.issues {
                println!("  {}: {}", issue.kind.as_str(), issue.detail);
            }
        }
    }
}
```

Every `channel`, `process`, and `site` is a `NodeId` from the analyzed graph.
Results are sorted by channel, process, and event site and do not modify the
graph.

## 2. Build from a real Rholang tree

Mode B keeps parser ownership with the caller:

```rust
// requires: features = ["rholang"]
use libcpg::{session_sketches, Language, Result, SessionSketch, TreeSitterCpgBuilder};

fn analyze_rholang(
    tree: &tree_sitter::Tree,
    source: &str,
) -> Result<Vec<SessionSketch>> {
    let cpg = TreeSitterCpgBuilder::new()
        .build_from_tree(tree, source, Language::Rholang)?;
    Ok(session_sketches(&cpg))
}
```

For this source:

```rholang
contract @"main"() = {
  new channel in {
    channel!(42) |
    for (@value <- channel) { Nil }
  }
}
```

both actions belong to one top-level contract, so the result is
`Inconclusive` with `EndpointCardinality`. Parallel syntax does not by itself
invent two binary endpoint processes.

## 3. Read the verdict

| Verdict | Consumer action |
|---|---|
| `Conformant` | retain as positive shallow evidence; do not label it deadlock-free |
| `Nonconformant` | inspect mismatch issues and source sites; confirm runtime reachability |
| `Inconclusive` | surface missing/non-linear/non-binary evidence; do not treat it as a pass |

Concrete mismatch issue kinds are `LengthMismatch`, `DirectionMismatch`,
`PayloadArityMismatch`, and `PayloadTypeMismatch`. Uncertainty kinds are
`EndpointCardinality`, `UnknownPayloadType`, and `NonLinearReceive`. A concrete
mismatch wins when both kinds of evidence occur.

## 4. Payload-type interpretation

Literals map to shallow categories. Common explicit primitive type names such
as `Int`, `String`, and `Bool` normalize to the same categories; custom names
remain `Named`. Untyped binders, unresolved identifier types, and compound
expressions are `Unknown`. The analysis does not infer a type from source text
or a variable name.

## 5. Serialize evidence

With both `rholang` and `serde`, every public session structure implements
`Serialize` and `Deserialize`:

```rust
// requires: features = ["rholang", "serde"]
use libcpg::{session_sketches, CodePropertyGraph, SessionSketch};

fn json(cpg: &CodePropertyGraph) -> serde_json::Result<String> {
    let sketches: Vec<SessionSketch> = session_sketches(cpg);
    serde_json::to_string_pretty(&sketches)
}
```

See [Theory 32](../theory/32-session-protocol-duality.md),
[ADR-0039](../design/0039-advisory-session-sketches.md), the
[component guide](../components/graph/session-analysis.md), and the
[security boundary](../security/07-session-analysis.md).
