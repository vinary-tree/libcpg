# Usage 12 — Channel topology and linearity reports

This guide extracts normalized message events and reports structural channel
matching/consumption concerns. Both APIs are feature-free. A language feature
is needed only when a frontend must normalize source syntax while building the
CPG.

## 1. Inspect events

```rust
use libcpg::{extract_events, CodePropertyGraph};

fn print_events(cpg: &CodePropertyGraph) {
    for (process, events) in extract_events(cpg) {
        for event in events {
            println!(
                "process={process:?} site={:?} kind={:?} channel={:?}",
                event.site, event.kind, event.channel,
            );
        }
    }
}
```

The outer map is ordered by process `NodeId`; each vector is ordered by event
site `NodeId`. `channel = None` is meaningful: either `Select`/`Spawn` has no
single channel, or the CPG did not contain sufficient typed identity evidence.

## 2. Produce findings

```rust
use libcpg::{channel_report, ChannelFindingKind, CodePropertyGraph};

fn print_channel_report(cpg: &CodePropertyGraph) {
    for finding in channel_report(cpg) {
        let severity = match finding.kind {
            ChannelFindingKind::OrphanSend => "review dropped output",
            ChannelFindingKind::BlockedRecv => "review possible starvation",
            ChannelFindingKind::NondeterministicConsumption => {
                "review competing linear consumption"
            }
        };
        println!(
            "{}: channel={:?} processes={:?} sites={:?}: {}",
            severity,
            finding.channel,
            finding.processes,
            finding.sites,
            finding.detail,
        );
    }
}
```

Findings are already ordered by channel and classification. Do not reorder
them with an unordered map if a stable report or cache key matters.

## 3. Build feature-free fixtures or imported graphs

Caller-built and deserialized graphs must attach normalized metadata to each
event site:

```rust
use libcpg::{CpgNode, CpgNodeKind, MsgKind, NodeId, SourceRange};

let send = CpgNode::new(
    NodeId::new(0),
    CpgNodeKind::Call { target: None, is_method: false },
    SourceRange::from_bytes(12, 17),
).with_message_kind(MsgKind::Send);
```

Then make the channel-use `Identifier` an AST descendant of `send` and provide
at least one of:

- `Identifier { definition: Some(channel_variable), .. }`;
- `channel_variable --DataFlow(DefUse/ReachingDef)--> identifier`; or
- an incident `Reference` between the identifier and variable.

The definition node must be `CpgNodeKind::Variable`. A mere equal spelling is
not enough.

## 4. Parse Rholang through Mode B

The `rholang` feature enables normalization but deliberately does not bundle a
grammar. The application owns the parser and passes its tree to libcpg:

```rust,ignore
// application dependencies: libcpg features=["rholang"] and a compatible
// rholang-tree-sitter grammar crate
use libcpg::{extract_events, Language, TreeSitterCpgBuilder};

let source = r#"contract @"main"() = {
  new c in { c!(1) | for (@x <- c) { Nil } }
}"#;
let mut parser = tree_sitter::Parser::new();
parser.set_language(&tree_sitter_rholang::LANGUAGE.into())?;
let tree = parser.parse(source, None).expect("parse");
let cpg = TreeSitterCpgBuilder::new()
    .build_from_tree(&tree, source, Language::Rholang)?;
let events = extract_events(&cpg);
```

The Tree-sitter mapper records the message kind while walking the tree. The
later concurrency analysis does not parse `source` again.

## 5. Interpret the classifications

| Finding | Structural meaning | Required follow-up |
|---|---|---|
| `OrphanSend` | producer exists; no linear, persistent, or peek receiver is represented | inspect external consumers, extrusion, and reachability |
| `BlockedRecv` | linear consuming receive exists; no producer is represented | inspect initialization, external sends, guards, and unreachable code |
| `NondeterministicConsumption` | a linear receive coexists with two producer sites or two linear receiver sites | decide whether race/non-determinism is intended |

`RecvPersistent` alone is not a starvation finding. `Peek` observes a send but
does not count as a linear consumer. `SendSync` is a producer for all topology
rules.

## 6. Safe operational use

- Treat `None` channel identities as incomplete evidence, not a shared unnamed
  channel.
- Preserve `processes`, `sites`, and `SourceRange` values when presenting a
  finding.
- Rebuild old serialized CPGs if they predate `message_kind`; the serde default
  is intentionally `None`.
- Combine the report with CFG reachability, pattern/guard reasoning, and later
  [`communication_deadlocks`](13-deadlock-candidates.md) evidence before making
  a runtime claim.
- Do not interpret an empty report as proof of determinism or liveness.

## 7. Focused verification

```sh
cargo test --no-default-features analysis::concurrency
cargo test --no-default-features --test concurrency_integration
cargo test --no-default-features --features rholang --test concurrency_integration
```

See [Theory 29](../theory/29-channel-topology-and-linearity.md), the
[component guide](../components/graph/channel-topology.md),
[ADR-0036](../design/0036-typed-channel-topology.md), and
[Scientific validation 31](../scientific/31-channel-topology-validation.md).

## References

1. Meredith, L. G., Radestock, M. (2005). *A Reflective Higher-order Calculus.*
   DOI:
   [10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016).
