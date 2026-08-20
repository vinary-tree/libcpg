# Usage 26 — Race and atomicity candidates

The concurrency bug APIs are feature-free and consume an MHP relation you have
already computed. They return immutable advisory records; they do not add edges
or modify node properties.

## Basic composition

```rust,ignore
use libcpg::{
    atomicity_candidates, may_happen_in_parallel, race_candidates,
    CodePropertyGraph,
};

fn review_concurrency(cpg: &CodePropertyGraph) {
    let mhp = may_happen_in_parallel(cpg);

    for race in race_candidates(cpg, &mhp) {
        println!(
            "race candidate at {:?} / {:?}, location {:?}",
            race.access_a, race.access_b, race.location
        );
    }

    for issue in atomicity_candidates(cpg, &mhp) {
        println!(
            "RMW gap {:?} -> {:?}, interferer {:?}",
            issue.read, issue.write, issue.witness.interfering_access
        );
    }

    if mhp.capped {
        eprintln!("candidate coverage is incomplete because MHP was capped");
    }
}
```

The snippet compiles in any libcpg build; the `ignore` marker avoids pretending
it is a standalone executable without a caller-provided CPG.

## Prerequisites

Useful results require all relevant layers:

1. a typed parallel `Block` carrying `MsgKind::Spawn`;
2. executable sites beneath distinct direct operands;
3. typed `FieldRead`/`FieldWrite` or `IndexRead`/`IndexWrite` plus exact
   `Reference`, or typed channel `MsgKind` plus resolved identity; and
4. typed `LockKind` metadata when synchronization should suppress or explain a
   candidate.

`LockKind` is semantic metadata. A frontend or adapter should set it only after
resolving the synchronization operation. The detector does not recognize
method names such as `lock`, `unlock`, or `read` from text.

## Reading a race witness

`RaceCandidate::access_a` and `access_b` are executable MHP sites. The
`expression_a` and `expression_b` fields identify the member/index/channel
expressions used to prove exact location identity. `kind_a` and `kind_b` expose
read/write behavior. `locks_a` and `locks_b` are complete represented locksets
at those sites.

A reported pair can contain the same lock in both locksets when both modes are
`Read`; shared acquisitions do not exclude one another. Any common resource
with at least one `Write` mode suppresses the race.

## Reading an atomicity witness

An atomicity candidate means one strand reads a location, releases a represented
guard, reacquires it, and then writes the same location while another strand's
same-location access is MHP with both endpoints.

Inspect these fields together:

- `read`, `write`, and `location` identify the logical update;
- `release_site`, `reacquire_site`, and `boundary_lock` identify the gap;
- `interfering_access`, `interfering_expression`, and `interfering_kind`
  identify possible interference; and
- `read_locks`, `write_locks`, and `interfering_locks` explain why no separate
  continuously held lock excluded it.

The split lock may appear on the interferer's lockset. That is expected: the
interferer can acquire it during the release/reacquire gap.

## Completeness policy

Treat `mhp.capped` as part of the result contract:

| MHP state | Safe interpretation |
|---|---|
| not capped, exact identities present | no candidate means none under this structural model |
| capped | findings are valid, but missing findings are possible |
| identities or typed access metadata missing | absence is inconclusive |

The same cap flag is copied into every `RaceWitness` and `AtomicityWitness` so
serialized findings retain their completeness context.

## What to do with a candidate

1. Verify the parallel region is reachable in the application.
2. Confirm the exact `location` denotes shared mutable state in the language's
   runtime model.
3. Inspect both source ranges and locksets.
4. Check atomics, barriers, condition variables, message semantics, and aliases
   that this analysis tier does not model.
5. For atomicity, decide whether the read and write are intended to form one
   transaction.

Do not automatically rewrite code or label a security vulnerability from this
advisory alone.

## Serialization

With `features = ["serde"]`, candidate vectors and all nested witness types can
be serialized normally. The source CPG is not embedded. Persist the relevant
CPG or stable node-to-source mapping alongside reports if later consumers need
to render source context.

See the [component guide](../components/graph/concurrency-bug-detection.md),
[Theory 43](../theory/43-evidence-bounded-race-and-atomicity.md), and the
[security boundary](../security/18-concurrency-bug-detection.md).
