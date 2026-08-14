# Theory 29 — Channel topology and linear consumption

A message-passing program coordinates processes through **channels**. A send
produces a message; a receive observes or consumes one. libcpg turns these
operations into a deterministic, statement-precise channel topology without
parsing source inside the analysis. Grammar knowledge stops at the Tree-sitter
frontend, which attaches a language-neutral `MsgKind` to the existing CPG node.

The analysis is inspired by the place/transition intuition of Petri nets: a
channel buffer behaves like a place, while sends and receives behave like
transitions that add or remove availability. It deliberately does not enumerate
markings or prove runtime reachability. Its output is structural evidence for
review.

![Typed channel-event extraction from frontend metadata and def-use evidence](../diagrams/channel-event-extraction.svg)

## 1. Vocabulary

A **process** is one top-level `Function` node. Rholang contracts are normalized
to `Function`, so the same definition applies to ordinary functions and
contracts. Nested functions are separate definitions and are not folded into an
enclosing process.

A **channel identity** is the `NodeId` of the `Variable` that defines the
channel. Identity is therefore lexical and statement-precise: two shadowed
variables with the same spelling are different channels, while differently
spelled uses connected to one definition are the same channel.

An **event** is the tuple

```math
e=(k,c,s,r),
```

where $`k`$ is a `MsgKind`, $`c`$ is an optional channel identity, $`s`$ is the
event-site `NodeId`, and $`r`$ is its `SourceRange`. `Select` and `Spawn` carry
no channel. A channel-using event can also carry `None` when the represented CPG
lacks enough evidence to resolve it; the extractor retains uncertainty instead
of guessing by name.

## 2. Normalized message kinds

| `MsgKind` | Role | Rholang syntax | Consumes a message? | May identify a channel? |
|---|---|---|---|---|
| `Send` | asynchronous producer | `c!(v)` and method send | no | yes |
| `SendSync` | producer with waiting continuation | `c!?(v).` | no | yes |
| `Recv` | linear receiver | `x <- c` | yes | yes |
| `RecvPersistent` | persistent receiver | `x <= c` | no control-token consumption | yes |
| `Peek` | observing receiver | `x <<- c` | no | yes |
| `Select` | non-deterministic branch choice | `select { ... }` | determined by selected bind | no |
| `Spawn` | concurrent process composition | `P \| Q` | no | no |

`RecvPersistent` remaining armed and `Peek` leaving a message available are
different semantics. Both are excluded from the linear starvation and
consumption-contention counts, but both count as observers when deciding
whether a send has any receiver.

## 3. Typed extraction, not source parsing

For a syntax node $`v`$, the frontend performs one grammar-specific mapping

```math
\operatorname{normalize}(v)\in
\{\textsf{Send},\textsf{SendSync},\textsf{Recv},
\textsf{RecvPersistent},\textsf{Peek},\textsf{Select},\textsf{Spawn}\}.
```

The value is stored as `CpgNode::message_kind`. The feature-free analyzer sees
only `CpgNode`, `AstChild`, `Reference`, and DFG edge kinds. It never scans
punctuation such as `<-`, never applies a regular expression to source, and
never imports the Rholang grammar.

Within one event subtree, candidate `Identifier` nodes resolve through:

1. `Identifier::definition`;
2. incoming `DataFlow(DefUse | ReachingDef)` edges; or
3. an incident `Reference` edge.

Only targets whose kind is `Variable` are channel identities. Candidates are
ordered by source start, source end, identifier id, and definition id. Nested
message-event subtrees form new statement boundaries and are not searched.
This selects a send's target before its payload uses and a receive's channel
while excluding receive-bound `Parameter` nodes.

Literate pseudocode makes the boundary explicit:

```text
ExtractEvents(CPG):
    result := ordered map
    for process in top-level Functions ordered by NodeId:
        events := []
        for site in process scope ordered by NodeId:
            if site.message_kind is absent: continue
            channel := None
            if site.message_kind uses a channel:
                channel := first Variable definition supported by
                           typed Identifier/DefUse/Reference evidence
            append (kind, channel, site, range(site)) to events
        result[process] := events
    return result
```

## 4. Channel topology predicates

For channel $`c`$, let:

- $`S_c`$ be its `Send` and `SendSync` event sites;
- $`R_c`$ be its linear `Recv` event sites;
- $`P_c`$ be its persistent `RecvPersistent` sites; and
- $`K_c`$ be its `Peek` sites.

The F1 topology findings are

```math
\operatorname{OrphanSend}(c)
\iff |S_c|>0 \land |R_c\cup P_c\cup K_c|=0,
```

and

```math
\operatorname{BlockedRecv}(c)
\iff |R_c|>0 \land |S_c|=0.
```

Persistent receivers and peeks do not produce `BlockedRecv`; that finding is
specifically the starvation signal for consuming linear receives.

The F4 linearity warning is

```math
\operatorname{Contention}(c)
\iff |R_c|>0 \land (|S_c|\ge 2 \lor |R_c|\ge 2).
```

It reports that matching and consumption can be non-deterministic. Counts are
statement counts rather than distinct process counts: two producer sites or
two receiver sites in one process still compete operationally.

![Per-channel topology classification for orphan, blocked, and contended linear operations](../diagrams/channel-topology-classification.svg)

## 5. Determinism and complexity

The process map, channel tally, process evidence, and site evidence all use
ordered collections. Events are explicitly sorted by site `NodeId`. Reversing
edge insertion therefore cannot change event order, finding order, or evidence
order.

Let $`n`$ be the number of nodes in top-level process scopes, $`m`$ the typed
edges inspected while resolving candidates, and $`q`$ the number of extracted
events. Extraction takes $`O(n+m+q\log q)`$ time including stable sorting and
$`O(n+q)`$ temporary space. Tallying is $`O(q\log q)`$ in the worst case because
channels are keyed by a `BTreeMap`. No state-space product is constructed.

## 6. Evidence boundary

The report is exact for the represented event metadata and identity edges, not
for all runtime schedules. Missing definitions yield unresolved events rather
than cross-name matches. Aliasing, channel extrusion, reflection, guards,
pattern success, control reachability, and dynamic process creation can make
the runtime behavior more or less constrained than this structural topology.

Accordingly, `ChannelFinding` is advisory. It contains the resolved channel,
sorted process ids, sorted event sites, and a descriptive explanation so a
consumer can inspect the evidence. An empty report is not a proof of deadlock
freedom, determinism, or liveness.

## References

1. Meredith, L. G., Radestock, M. (2005). *A Reflective Higher-order Calculus.*
   Electronic Notes in Theoretical Computer Science 141(5), 49–67. DOI:
   [10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016).
