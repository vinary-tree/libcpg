# Component — Race and atomicity candidate detection

## Responsibility

`analysis::concurrency::{evidence,race,atomicity}` composes exact shared-
location identity, typed access kinds, MHP, and typed lock operations into
immutable advisory candidates. It owns neither parsing nor a runtime scheduler,
and it never changes the CPG.

![Concurrency candidate evidence flow](../../diagrams/concurrency-bug-evidence-pipeline.svg)

## Public surface

| API/type | Contract |
|---|---|
| `race_candidates(&CodePropertyGraph, &MhpRelation)` | sorted exact-location conflicts that are MHP and lack a common excluding lock |
| `RaceCandidate` | canonical statement pair, exact location, and `RaceWitness` |
| `RaceWitness` | expressions, access kinds, ranges, endpoint locksets, and MHP cap state |
| `atomicity_candidates(&CodePropertyGraph, &MhpRelation)` | sorted RMW lock-gap candidates with a represented interferer |
| `AtomicityCandidate` | read site, write site, exact location, and `AtomicityWitness` |
| `AtomicityWitness` | endpoint expressions/locksets, release/reacquire boundary, interfering access, and cap state |
| `ConcurrentAccessKind` | `Read`, `Write`, or `ReadWrite` typed behavior |
| `HeldLock` | resource, mode, acquisition site, and acquisition range |

All types are feature-free. Public result types derive serialization only with
the crate's existing `serde` feature.

## Internal boundary

The private `evidence` module exists so both classifiers share one semantics:

1. typed DFG relations classify field/index reads and writes;
2. one exact `Reference` identifies a `Field` or `Variable` location;
3. typed channel sends/receives add channel access facts;
4. an iterative nearest-parent machine lifts expressions to executable sites;
5. a second iterative machine assigns execution strands;
6. a source-ordered state machine snapshots held lock acquisitions; and
7. ordered indexes expose accesses by site, strand/location, and location.

The expression and executable site are intentionally both retained. The former
explains *what* accessed state; the latter is the scheduling unit represented by
MHP.

## Detector invariants

1. Every location and lock resource is an existing exact declaration id.
2. Ambiguous location, site, or strand evidence produces no guessed fact.
3. Every race has one write and a canonical MHP pair.
4. Read/read pairs are clean; read-mode/read-mode guards do not hide writes.
5. Every atomicity candidate has read then write, a resolved same-lock
   release/reacquire gap, and an interferer MHP with both endpoints.
6. A continuously held excluding lock suppresses atomicity; the split lock does
   not.
7. Malformed public MHP pairs are canonicalized, deduplicated, and stripped of
   self-pairs before use.
8. Findings are sorted, deduplicated, read-only, and out of graph.
9. Parent walks and lock replay use heap state and terminate on cycles.

## Failure and uncertainty

| Missing/hostile evidence | Behavior |
|---|---|
| typed DFG or `MsgKind` absent | no access kind is invented |
| zero or multiple referenced locations | access suppressed |
| zero or multiple nearest sites/strands | access or lock event suppressed |
| unresolved acquisition | no protection invented |
| unresolved release | held state cleared conservatively |
| malformed MHP order/duplicates | normalized before classification |
| `mhp.capped == true` | known pairs remain usable; witnesses preserve incompleteness |
| AST cycles/depth | visited heap work queues terminate without native recursion |

## Complexity-sensitive implementation choices

Per-site access vectors are ordered by location, so race classification uses a
merge join rather than a Cartesian product. Atomicity classification builds a
location index before searching for interferers. These choices preserve exact
semantics while avoiding avoidable quadratic work.

## Tests

Feature-free and all-feature tests cover unlocked/protected races, access-mode
boundaries, exact fields/channels/locks, ambiguity, hostile MHP values, RMW
gaps, missing interference, continuous protection, serde, and immutability.
Two 128-case properties compare independently built forward/reverse insertion
orders. Public integration composes both APIs with MHP. The stack regression
runs a 100,000-level parent chain on a 256 KiB thread stack.

See [Theory 43](../../theory/43-evidence-bounded-race-and-atomicity.md),
[Usage 26](../../usage/26-concurrency-bug-candidates.md), the [scientific
validation dossier](../../scientific/48-concurrency-bug-validation.md), and the
[security boundary](../../security/18-concurrency-bug-detection.md).
