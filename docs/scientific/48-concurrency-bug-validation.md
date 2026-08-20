# Scientific validation 48 — Race and atomicity candidates

## Claim under test

For the represented CPG model, the implementation should report exactly those
same-location MHP conflicts that contain a write and lack a common excluding
lock. It should report an RMW atomicity candidate only when a represented guard
is released/reacquired between adjacent same-location read/write endpoints, an
other-strand access is MHP with both, and no continuously held lock excludes the
interferer.

The claim is intentionally structural and advisory. It is not a claim of
runtime reachability, alias completeness, or language-memory-model soundness.

## Oracle matrix

| Fixture | Expected result | Boundary exercised |
|---|---|---|
| two MHP writes, one exact field, no locks | one race | positive F6 oracle |
| same writes under one common write-mode lock | no race | mutual exclusion |
| two MHP reads | no race | conflict predicate |
| read/write under two read-mode acquisitions | one race | shared mode is not exclusion |
| same field spelling but ambiguous `Reference` targets | no access fact | no name guessing |
| two typed sends to one resolved channel | one candidate | channel identity path |
| field-valued lock resource | same suppression as variable lock | generalized exact lock identity |
| malformed reversed/duplicate/reflexive MHP pairs | canonical single result | public-input hardening |
| read, unlock, lock, write plus other-strand access | one atomicity candidate | positive F7 oracle |
| read/write in one continuous critical section | no atomicity candidate | no split boundary |
| split RMW without represented interferer | no atomicity candidate | MHP requirement |
| empty MHP | no candidate | evidence-bounded absence |

Each positive assertion also verifies exact location ids, endpoint ids, source
ranges or boundary sites, access kinds, lock identity, and immutability.

## Property-based validation

Two independent 128-case campaigns rebuild complete fixtures with forward and
reverse node/edge insertion order:

1. the race property varies the number of exact shared locations from one to
   eight and the MHP cap flag, then requires byte-for-byte equal candidate
   vectors, canonical endpoint ids, exact output cardinality, and preserved cap
   state; and
2. the atomicity property reconstructs the access and lock timeline in both
   orders, then requires identical boundary/interferer witnesses and cap state.

These properties target the actual determinism hazard—graph insertion order—
rather than sorting two already-computed outputs.

## Public integration

`tests/concurrency_bugs_integration.rs` uses only crate-root APIs. One parallel
operand performs a lock/read/unlock/relock/write sequence; the other writes the
same exact field. Public MHP feeds both detectors. The oracle requires two race
candidates, one atomicity candidate, exact boundary/interferer identities, and
unchanged graph node/edge counts.

## Stack-safety experiment

`tests/concurrency_bugs_stack_safety.rs` places the first operand's evidence
beneath 100,000 nested blocks and runs MHP plus both detectors on a worker with a
256 KiB native stack. It still produces two races and one atomicity candidate.
The test exercises expression lifting, strand discovery, lock replay, MHP
collection, report construction, and destruction. Passing demonstrates that
represented source depth is heap state rather than native recursion depth.

## Feature matrix

| Command family | Evidence |
|---|---|
| `cargo test --no-default-features analysis::concurrency --lib` | feature-free examples and 128-case properties |
| `cargo test --all-features analysis::concurrency --lib` | feature-free behavior plus serde and optional session coexistence |
| `cargo test --no-default-features --test concurrency_bugs_integration` | public F6/F7 composition |
| `cargo test --no-default-features --test concurrency_bugs_stack_safety` | 100,000 levels on 256 KiB |
| `cargo clippy --all-targets --all-features -- -D warnings` | warning-free full target surface |

## Falsification boundaries

The tests do not establish precision for pointer aliases, object-sensitive heap
locations, runtime scheduling, unreachable branches, atomics, volatile memory,
barriers, condition variables, or a language-specific memory model. A future
implementation adding one of these must introduce a corresponding clean/dirty
oracle and preserve every evidence-bounded negative above.

## Literature grounding

The candidate shape follows the scalable static race-detection tradition of
RacerD and RacerX, while libcpg deliberately limits its claim to evidence
already represented in the CPG [[1]](#references), [[2]](#references).

## References

1. Blackshear, S., Gorogiannis, N., O'Hearn, P. W., Sergey, I. (2018).
   *RacerD: Compositional Static Race Detection.* DOI:
   [10.1145/3276514](https://doi.org/10.1145/3276514).
2. Engler, D., Ashcraft, K. (2003). *RacerX: Effective, Static Detection of Race
   Conditions and Deadlocks.* DOI:
   [10.1145/945445.945468](https://doi.org/10.1145/945445.945468).
