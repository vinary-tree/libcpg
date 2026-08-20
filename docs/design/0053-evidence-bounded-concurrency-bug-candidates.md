# ADR-0053: Compose exact access, MHP, and lock evidence for concurrency bugs

- **Status:** Accepted
- **Date:** 2026-08-20
- **Scope:** feature-free `analysis::concurrency::{evidence,race,atomicity}`

## Context

ADR-0038 established a bounded may-happen-in-parallel (MHP) relation between
executable statement sites. That relation answers *where overlap is possible*,
but it does not identify shared state or synchronization. A useful data-race
candidate additionally needs two accesses to one exact location, a write, and
the absence of a mutually excluding common lock. An atomicity candidate needs
still more temporal structure: a logical read-modify-write update split into
two critical sections with a parallel access able to enter between them.

The evidence resides at different CPG layers. `FieldRead`/`FieldWrite` and
`IndexRead`/`IndexWrite` touch a member or index expression; `Reference`
identifies its declaration; MHP relates the enclosing executable statement;
and `LockKind` belongs to typed call sites. Treating one node id as all four
concepts would either miss real evidence or invent precision.

The public structs in `MhpRelation` are also serializable and constructible.
Consumers therefore cannot assume hostile input preserves canonical ordering,
deduplication, or irreflexivity merely because `may_happen_in_parallel` does.

## Decision

1. Keep both detectors feature-free, immutable, advisory, and out of graph.
2. Accept a caller-supplied `MhpRelation`; never recompute or silently upgrade
   its completeness.
3. Recognize a field/index location only from a typed DFG access relation plus
   exactly one `Reference` to a `Field` or `Variable`. Recognize channel
   accesses only from typed `MsgKind` plus resolved channel identity.
4. Lift an expression to its nearest executable ancestor with an explicit
   breadth-first work queue and visited set. If malformed parent evidence gives
   more than one equally near answer, suppress the access.
5. Assign a site to its nearest `Function` or direct operand of a typed
   `MsgKind::Spawn` block. This **execution strand** is used only for lexical
   access and lock ordering; ambiguous strand evidence is suppressed.
6. Resolve lock resources to exact `Variable` or `Field` identities. Replay
   each strand's typed operations in source order. A resolved acquisition pushes
   a `HeldLock`; a resolved release removes the last matching instance; an
   unresolved release clears held state; an unresolved acquisition contributes
   no guessed protection.
7. Canonicalize and deduplicate public MHP pairs before either detector uses
   them. Reflexive pairs are discarded.
8. Report `RaceCandidate { access_a, access_b, location, witness }` exactly when
   the sites are MHP-related, their exact locations agree, at least one access
   writes, and their locksets have no common resource whose acquisition modes
   are mutually excluding. Two read-mode acquisitions alone are not excluding.
9. Report an `AtomicityCandidate` exactly when adjacent same-location accesses
   in one strand form read then write, a lock held at both endpoints is released
   and reacquired between them, another strand's same-location access is MHP
   with both endpoints, and no different continuously held acquisition excludes
   that interferer.
10. Preserve expression ids, statement ids, source ranges, access kinds,
    complete endpoint locksets, boundary operations, the interfering site, and
    the input MHP cap flag in witnesses.
11. Use ordered maps/sets, a linear merge of per-site locations for races, and a
    location index for atomicity interference. Do not hand-roll source parsers,
    recurse over source depth, or perform an all-access Cartesian product.

![Exact evidence composition for concurrency candidates](../diagrams/concurrency-bug-evidence-pipeline.svg)

## Why the two lock rules differ

A race is suppressed when the two simultaneous endpoints share an excluding
lock. An atomicity violation is about the *gap between* two protected
endpoints. Releasing and reacquiring the same guard creates such a gap, so an
interferer holding that guard is evidence for—not a rebuttal to—the candidate.
Only a distinct acquisition held continuously across both endpoints can rule
out that interleaving.

![Read-modify-write critical-section gap](../diagrams/atomicity-lock-gap.svg)

## Rejected alternatives

### Match field or lock names

Rejected. Shadowing, same-named fields, imports, and dynamic receivers make
textual identity unsound. Missing or ambiguous graph identity must remain
missing or ambiguous.

### Parse assignments, locks, or channels from source text

Rejected. The feature-free core cannot depend on sixteen grammars, and source
spelling cannot prove resolved API semantics. Frontends and semantic adapters
already provide typed CPG evidence.

### Treat a member expression as an MHP site

Rejected. The MHP execution vocabulary is statement-oriented. Iterative
lifting preserves the exact member expression in the witness while querying
MHP at the scheduling site it actually models.

### Treat every common lock as excluding

Rejected. Two read/shared acquisitions can coexist. They cannot justify
suppressing a candidate containing a write.

### Infer protection through an unresolved release

Rejected. Retaining locks after an unknown release can hide a real candidate.
Clearing the held set is the conservative direction for a detector whose clean
result must not rely on invented protection.

### Use recursive parent walks

Rejected. Valid generated source and hostile cyclic overlays can be arbitrarily
deep. Heap queues and visited sets keep native call depth constant.

### Materialize findings as CPG edges

Rejected. MHP can be capped, reachability is not proved, and alias/statement
precision is intentionally limited. Immutable reports cannot become stale
authoritative-looking topology.

## Consequences

### Positive

- Every reported location and lock has exact graph identity.
- Statement scheduling and expression identity remain visibly distinct.
- Deterministic normalization makes deserialized or manually constructed MHP
  input safe to consume.
- A linear location merge avoids a quadratic cross-product per MHP pair.
- All source-depth traversals are explicit heap machines with cycle guards.
- Witnesses are sufficient for a caller to explain or further qualify a
  candidate without mutating the CPG.

### Negative

- Missing DFG, `Reference`, `MsgKind`, `LockKind`, AST, or MHP evidence causes
  false negatives.
- Structural MHP and lexical source order do not prove runtime reachability.
- Pointer aliases, heap object sensitivity, atomics, volatile semantics,
  condition variables, and language memory models are outside this tier.
- `mhp.capped == true` means an empty or partial candidate set is incomplete.

## Verification

Example oracles cover unlocked parallel writes, a common exclusive guard,
read/read access, insufficient read-mode guards, exact field and channel
identity, ambiguous references, RMW unlock/relock gaps, absent interference,
continuous critical sections, malformed public MHP input, immutability, and
serde. Two 128-case properties reconstruct graphs in forward and reverse node
and edge insertion order. Public integration composes MHP, races, and atomicity
through crate-root APIs. A 100,000-level parent chain runs both detectors on a
256 KiB thread stack.

## References

1. Blackshear, S., Gorogiannis, N., O'Hearn, P. W., Sergey, I. (2018).
   *RacerD: Compositional Static Race Detection.* Proceedings of the ACM on
   Programming Languages 2(OOPSLA). DOI:
   [10.1145/3276514](https://doi.org/10.1145/3276514).
2. Engler, D., Ashcraft, K. (2003). *RacerX: Effective, Static Detection of Race
   Conditions and Deadlocks.* SOSP '03. DOI:
   [10.1145/945445.945468](https://doi.org/10.1145/945445.945468).
