# Usage 13 — Communication and lock-order deadlock candidates

`communication_deadlocks` is feature-free and returns deterministic,
statement-precise advisory findings. It consumes the channel metadata described
in [Usage 12](12-channel-topology.md) and typed lock metadata attached by a
frontend or semantic API adapter.

## 1. Run the analysis

```rust
use libcpg::{communication_deadlocks, CodePropertyGraph, DeadlockFindingKind};

fn print_deadlock_candidates(cpg: &CodePropertyGraph) {
    for finding in communication_deadlocks(cpg) {
        let label = match finding.kind {
            DeadlockFindingKind::CommunicationCycle => "communication wait",
            DeadlockFindingKind::LockOrderCycle => "lock order",
        };
        println!(
            "{label}: processes={:?} resources={:?}: {}",
            finding.processes, finding.resources, finding.detail,
        );
        for witness in finding.witnesses {
            println!("  {witness:?}");
        }
    }
}
```

Results list communication SCCs first, then lock-order SCCs. Components and all
evidence vectors use stable `NodeId` order.

## 2. Attach typed lock evidence

Keep the ordinary call role and add orthogonal synchronization semantics:

```rust
use libcpg::{CpgNode, CpgNodeKind, LockKind, NodeId, SourceRange};

let acquire = CpgNode::new(
    NodeId::new(0),
    CpgNodeKind::Call {
        target: None,
        is_method: true,
    },
    SourceRange::from_bytes(40, 52),
)
.with_lock_kind(LockKind::AcquireWrite);
```

Make the lock expression's `Identifier` an AST descendant of the call and link
it to a `Variable` using `Identifier::definition`, `DefUse`, `ReachingDef`, or
`Reference`. `AcquireRead` represents a shared acquisition; `Release` removes
the most recent matching resource from the held stack.

Do not annotate calls by substring inside the analyzer. Resolve the library API
in a frontend/adapter, then attach `LockKind` as typed evidence.

## 3. Communication interpretation

A process is initially blocked only when its first relevant event is a resolved
linear `Recv`. `Send` and `SendSync` are progress-producing. Persistent receive,
peek, select, and spawn do not establish a linear initial wait.

A wait edge is omitted when any known producer is not initially blocked. This
is why a send-first process or an alternative ready producer breaks a candidate.

## 4. Understand witnesses

```rust
use libcpg::DeadlockWitness;

fn describe(witness: DeadlockWitness) {
    match witness {
        DeadlockWitness::Communication {
            waiter,
            producer,
            channel,
            wait_site,
            producer_site,
            ..
        } => println!(
            "{waiter:?} waits at {wait_site:?} on {channel:?}; \
             {producer:?} would send at {producer_site:?}"
        ),
        DeadlockWitness::LockOrder {
            held_lock,
            acquired_lock,
            held_site,
            acquired_site,
            via_call,
            ..
        } => println!(
            "{held_lock:?} at {held_site:?} precedes {acquired_lock:?} \
             at {acquired_site:?}; call={via_call:?}"
        ),
    }
}
```

For an interprocedural lock edge, `holder` and `held_site` are caller-side;
`acquirer` and `acquired_site` identify the representative callee acquisition;
`via_call` is the immediate caller-side call site.

## 5. Rholang Mode B

The same `TreeSitterCpgBuilder::build_from_tree` flow shown in Usage 12 works for
communication cycles. The DFG now seeds contracts from lexically preceding
outer definitions, so multiple contracts enclosed by one `new` can resolve the
same channel definitions without analyzer-side source parsing.

## 6. Review checklist

- Confirm metadata and def-use/call-target completeness.
- Inspect CFG reachability around each witness.
- Check for external producers or runtime-created resources.
- Confirm lock mode, lifetime, reentrancy, and release behavior.
- Preserve the advisory/candidate qualifier in UI, SARIF, and policy output.
- Rebuild stale serialized graphs after frontend semantic changes.

See [Theory 30](../theory/30-cyclic-wait-and-lock-order.md), the
[component guide](../components/graph/deadlock-analysis.md), and the
[security boundary](../security/05-deadlock-analysis.md).
