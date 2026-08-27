# ADR-0073: Adopt bounded, composable approximate evidence

- **Status:** Accepted
- **Date:** 2026-08-26
- **Scope:** code-similarity candidate generation, symbol navigation and
  alignment, API-sequence comparison, weighted finite-state composition,
  dependency admission, stack and heap safety

## Context

`liblevenshtein` supplies edit-distance kernels and automata.
`duallity` exposes compatible weighted finite-state transducers (WFSTs) over
`lling-llang`. A WFST is a finite-state machine whose arcs relate input labels
to output labels and carry weights. These capabilities are useful to libcpg
because many discovery problems begin with a large, noisy candidate space:

- Type-3 and near-duplicate code detection;
- symbol spelling, navigation, and renamed-or-moved symbol alignment;
- cross-language and cross-version API migration;
- API-sequence and protocol-neighbour discovery;
- semantic-diff and vulnerability-analogue candidate generation; and
- configurable phonetic or domain-specific spelling rewrites.

Approximation changes recall and ranking, not truth. An edit-near name is not
an exact call target, an edit-near function tape is not yet a clone, and an
edit-near API trace is not a protocol violation. libcpg therefore needs a typed
evidence boundary that preserves the value of approximate search without
allowing a score to become an exact graph fact.

![Typed immutable projections pass through a bounded approximate engine and then through each consumer's exact confirmation boundary.](../diagrams/approximate-automata-adoption.svg)

*Figure -- admitted approximate-evidence flow. Source:
[`diagrams/approximate-automata-adoption.puml`](../diagrams/approximate-automata-adoption.puml).*

## Decision

Adopt approximate matching in libcpg as an opt-in, bounded candidate-generation
layer. T4.5 is authorized to implement the production adapter.

1. The adapter consumes immutable typed projections, never a mutable
   `CodePropertyGraph`.
2. Every result is an `ApproximateEvidence` value carrying the stable candidate
   identifier, consumer, method, metric semantics, semiring, raw cost,
   normalized ordering key, rank, limits, counters, completion state, and
   replayable configuration identity. A bare Boolean or floating-point score is
   insufficient.
3. Approximate evidence may rank candidates for navigation, revision
   alignment, API-sequence comparison, clone discovery, semantic diff, and
   vulnerability-analogue search. Each semantic consumer independently applies
   its exact confirmation predicate.
4. Approximation never creates or rewrites call edges, establishes node
   identity, merges equivalence classes, changes strongly connected components
   (SCCs), seeds data flow or taint, suppresses an exact candidate, or emits a
   vulnerability verdict.
5. `Incomplete` means that an explicit resource or cancellation boundary
   stopped the search. It never means complete no-match.
6. Public ordering is total and deterministic: cost first, then canonical
   consumer key, then stable identifier. It is independent of hash order,
   insertion order, cache residency, automaton state numbering, thread count,
   and traversal scheduling.
7. Use direct bounded `liblevenshtein` kernels for flat candidate scans. Use
   `duallity` composition where a consumer actually needs typed rewrite stages.
   Do not duplicate either engine inside libcpg.
8. Only dependency surfaces with constant native call depth and enforced heap
   and work bounds are reachable from the adapter. Generalized edit operations
   use the iterative distance automaton; the currently recursive generalized
   dictionary-path enumerator is not on a production call path.
9. Rust 1.85 is not a project requirement or an admission gate. The evaluated
   release graph declares Rust 1.95. libcpg may require that toolchain, and may
   use nightly when a selected PathMap, portable-SIMD, or other retained fast
   path needs nightly functionality. Capabilities are not disabled merely to
   preserve an unrequested older compiler.

## Dependency boundary

The admission experiment began from exact registry release candidates. The
production graph retains those archive checksums except for `liblevenshtein`
and `lling-llang`: published RC4 predates the bounded generated-target
correction and the paired-epsilon composition correction, so libcpg pins the
two audited root-fix commits from their canonical repositories. The revisions
are immutable and remotely resolvable; no floating branch or local path
participates in the dependency graph.

| Package | Version | Production source identity | Archive VCS commit | Declared Rust | License |
|---|---|---|---|---:|---|
| `duallity` | `4.0.0-rc.4` | crates.io SHA-256 `1864da0aa5bd1cada68bafa1c9ae160001ba93eab39b221f393a15323edb02f9` | `ce2e56b7e13a69b097cefb05dfce0b661ec5ee91` | 1.95 | Apache-2.0 |
| `liblevenshtein` | `4.0.0-rc.4` | canonical Git revision [`81c0097f093b64aafec5935a1bdeddcde090d91d`](https://github.com/vinary-tree/liblevenshtein-rust/commit/81c0097f093b64aafec5935a1bdeddcde090d91d) | same as source identity | 1.95 | Apache-2.0 |
| `libdictenstein` | `4.0.0-rc.4` | crates.io SHA-256 `3107cc65fec16a66624be386df12a0c8f4786c665c2088124bccc63350587842` | `085f61c22f714f6a9c2aee528077be180b9545d4` | 1.95 | Apache-2.0 |
| `lling-llang` | `4.0.0-rc.4` | canonical Git revision [`d34f0ec2a8b3d5728e09196114c1e22bea4272cd`](https://github.com/vinary-tree/lling-llang/commit/d34f0ec2a8b3d5728e09196114c1e22bea4272cd) | same as source identity | 1.95 | Apache-2.0 |
| `llattice` | `0.1.0` | crates.io SHA-256 `11b2da71811803fafc6a1f2aff2fb8d8f25be26ee38678a89cf4008e7374f567` | `9a35c0f08cf6dbd5f6cb8c72a431c1ccd849a095` | 1.70 | Apache-2.0 |

Production dependencies are optional and exactly pinned. Default libcpg builds
do not pay their compilation or runtime cost. A future release may replace the
Git sources only after they contain the same corrections and pass this
consumer gate. The root `[patch.crates-io]` entries redirect both libcpg's
direct requests and `duallity`'s transitive requests to those revisions;
`cargo tree -d` must therefore report no duplicate `liblevenshtein` or
`lling-llang` package identities. `duallity` disables
`lling-llang`'s default `smt-z3` feature, so approximate matching does not pull
Z3. PathMap remains a separately selected backend rather than an accidental
transitive default.

libcpg already uses optional `libdictenstein` for exact MeTTa symbol
discrimination. The production change aligns that direct dependency with the
selected release line so combined features resolve one coherent crate family.

## Evidence serialization and trust boundary

When libcpg's `serde` feature is enabled, approximate configurations,
projections, reports, individual evidence records, finite rewrite rules, and
finite extensional observations have an owned serialization form. The method
identity is owned text rather than a process-local static reference. No live
cancellation token, automaton, worklist, cache, or mutable graph is serialized.

The wire contract has three safeguards:

1. `schema_version` is emitted in every report provenance record and currently
   equals `1`. A reader must reject or explicitly migrate an unsupported
   version; it must not guess field semantics.
2. Evidence and relation paths are already canonically ordered, completion
   reasons are ordered enum values, and the serialized structures contain no
   hash maps. Repeated JSON serialization of the same report is therefore
   byte-deterministic.
3. Deserialization restores evidence, not authority. A consumer still checks
   the recorded method/configuration identity, graph or corpus namespace,
   completion state, limits, and the consumer-specific exact confirmation
   predicate before acting on a candidate.

For example, persistence and replay are deliberately explicit:

```rust,no_run
use libcpg::ApproximateReport;

# fn persist(report: &ApproximateReport) -> Result<(), serde_json::Error> {
let bytes = serde_json::to_vec(report)?;
let replayed: ApproximateReport = serde_json::from_slice(&bytes)?;
assert_eq!(&replayed, report);
# Ok(())
# }
```

The 100,000-candidate lifecycle gate performs this round trip on a 256 KiB
thread stack. The marker-typed weighted finite-state transducer (WFST)
observation gate separately proves that only extensional paths, completion,
and counters cross the wire; zero-sized tape-domain markers do not become
runtime state.

## Consumer contracts

| Consumer | Approximate proposition | Independent confirmation | Result use |
|---|---|---|---|
| developer typo and navigation | a symbol spelling is near the query | user selection or exact symbol lookup | ranked suggestions |
| revision or language alignment | names or signatures are plausible counterparts | qualified scope, typed signature, exact diff facts | alignment candidates |
| API-sequence neighbours | event tapes have low edit cost | exact API-usage graph and temporal-property evaluation | protocol-mining candidates |
| clone detection | normalized function tapes have low edit cost | Type-1/2 fingerprints, Type-3 multiset evidence, or PDG WL plus VF2 | candidate pairs only |
| semantic diff | old and new element tapes have low edit cost | identity translation, typed nodes, and exact edges | correspondence candidates |
| vulnerability analogues | a function tape is near a seed tape | joint-vocabulary PDG WL, relaxed VF2, policy checks, and review | investigation queue |
| phonetic matching | a configured rewrite plus edit model yields low cost | explicit configuration and caller confirmation | optional search ranking |

The confirmation column is part of the type-level consumer identity and the
documented API contract. Candidate generation may improve recall and reduce
the number of expensive exact comparisons; it cannot replace confirmation.

## Explicit non-consumers

The following remain exact:

- stable identifiers, declaration/reference binding, scope, overload
  resolution, and call targets;
- graph topology, SCCs, condensation, dominance, control flow, data flow,
  program dependence, name flow, heap flow, and rewrite-dependency edges;
- taint sources and sinks, declassifiers, typestate events, ownership and
  capability identities, race locations, cryptographic policy, and security
  verdicts;
- Datalog extensional facts, e-graph equivalence, cache identity, serialized
  equality, completeness claims, and authorization decisions.

## Measured engine selection

The baseline experiment compared every result with an independent bounded
distance scan before timing. Result sets and checksums were equal.

| Corpus | Terms | Ordered build | DAWG build | Direct scan | WFST query | Selection consequence |
|---|---:|---:|---:|---:|---:|---|
| qualified symbols | 256 | 16.782 us | 198.166 us | 106.052 us | 1.457064 ms | direct scan |
| qualified symbols | 4,096 | 400.250 us | 1.161493 ms | 1.599704 ms | 1.743617 ms | direct scan until reuse pays |
| Unicode identifiers | 1,024 | 93.187 us | 187.505 us | 184.310 us | 311.010 us | direct scan |
| API sequences | 1,024 | 108.045 us | 842.638 us | 141.789 us | 130.718 us | reusable WFST index after amortization |
| clone shingles | 1,024 | 88.398 us | 4.532669 ms | 728.742 us | 2.615862 ms | direct scan plus structural confirmation |

These measurements choose an engine per consumer; they do not veto the
capability. The API-sequence fixture recovers its extra dictionary construction
after roughly 67 comparable queries. Therefore index ownership and invalidation
epoch are explicit configuration, while one-shot calls use bounded direct
scans.

The 4,096-symbol locality probe recorded 11,926,591 versus 219,267,029 cache
references and 370,567 versus 45,561,229 cache misses for direct and WFST paths.
That evidence reinforces the per-consumer choice and establishes a regression
baseline for later index optimization.

## Stack, heap, and cancellation contract

The evaluated direct standard-distance path passed at 4,096 characters on a
256 KiB native stack. The standard `LevenshteinWfst` lifecycle passed at query
length 512. The RC4 generalized WFST dictionary traversal overflowed the same
stack when one operation consumed a 1,024-character dictionary path because
`collect_dictionary_paths` used native recursion.

The adapter addresses the root boundary structurally:

- standard and optimal-string-alignment scans call iterative bounded kernels;
- generalized costs call the iterative `GeneralizedAutomaton` alignment
  machine, not recursive dictionary-path enumeration;
- candidate and projection canonicalization use in-place binary heaps rather
  than recursive comparison sorting;
- top-k retention uses a bounded max-heap whose root is the current worst
  retained result, so a scan never retains more than the configured result
  count;
- iteration, ranking, serialization, cloning, and drop use flat vectors,
  arenas, heaps, and worklists;
- checked limits cover input units, candidate count, dynamic-programming cells,
  accepted results, retained bytes, and total work;
- cancellation is observed before every candidate and at bounded work
  intervals; and
- a cap or cancellation produces typed incomplete evidence.

Here *canonicalization* means arranging records by their stable identifier
before a consumer observes them. Let `n` be the number of supplied candidates,
`r` the number admitted by the candidate-count limit, and `k` the configured
result count. The implementation first charges the `n`-entry ordering vector
and the `min(k, r)`-entry retained heap against the memory budget. It then
performs the following iterative procedure:

```text
CANONICAL-TOP-K(query, candidates, k, limits)
    reject-or-report-incomplete unless storage for n ordering entries
        and min(k, r) retained entries fits limits
    ordered <- BUILD-MAX-HEAP((stable-id(candidate), candidate)
                              for candidate in candidates)
    HEAP-SORT-IN-PLACE(ordered)
    retained <- empty max-heap with capacity k
    for candidate in ordered do
        observe cancellation and work limits
        evidence <- bounded-distance(query, candidate)
        if evidence is accepted then
            PUSH(retained, evidence)
            if SIZE(retained) > k then POP-MAX(retained)
    return HEAP-SORT-IN-PLACE(retained)
```

The evidence ordering compares normalized cost, raw cost, and stable
identifier; both costs are exact rational values rather than floating-point
approximations. Therefore the max-heap root is exactly the worst retained
result, while the final in-place heap sort exposes best-first public order.
Construction and destruction are iterative, native call depth is constant,
ordering storage is `$\mathcal{O}(n)$`, and retained-result storage is
`$\mathcal{O}(\min(k,n))$`. The
integration gate exercises ordering, ranking, cloning, debug formatting, and
destruction for 100,000 reverse-ordered candidates on a 256 KiB thread stack;
the projection gate separately exercises 20,000 and 100,000 functions on the
same stack size.

The positional transition cache now selects a dense row only while one row is
at most 2,048 bytes and stores only observed transitions for longer queries.
Its independent upstream qualification covers randomized refinement against a
sparse oracle, the full default suite, executable documentation, pinned causal
benchmarks, and a 100,000-unit run on a 256 KiB thread. Future indexed consumers
must still enforce their own end-to-end budgets. Raising a cgroup limit,
enlarging a native stack, truncating input silently, or retrying after an abort
is not an implementation strategy.

## Category-theory contract

WFSTs over one semiring form a category of weighted rational relations: objects
are typed tape domains, arrows are weighted relations, identity copies its
input at multiplicative identity, and composition eliminates an intermediate
tape. libcpg uses that structure only where composition is semantically real.

Complete finite relations obey extensional left identity, right identity, and
associativity. Resource-bounded observations do not automatically obey those
laws. A truncated observation is therefore an execution report, not another
arrow; reassociation is prohibited after any stage becomes incomplete.

## Alternatives considered

### Use only existing structural algorithms

Rejected. Exact structural confirmation is necessary, but applying every
expensive verifier to every pair scales poorly and misses a reusable ranking
capability for navigation, migrations, near-duplicate code, and analogue
discovery.

### Make approximate scores exact facts

Rejected. Similarity cannot establish identity, topology, equivalence, or a
security verdict. This would conflate recall-oriented evidence with truth.

### Copy a distance implementation into libcpg

Rejected. It would fork semantics, SIMD dispatch, generalized operations,
tests, and maintenance. The selected libraries already own those concerns.

### Require Rust 1.85 or disable newer functionality

Rejected. No such compatibility requirement exists. libcpg follows the actual
toolchain needed by retained functionality and performance, including nightly
when an explicitly selected feature requires it.

### Always build a dictionary automaton

Rejected. Measurements show that a one-shot direct bounded scan wins for most
current corpora. Reusable indexes remain available when query volume amortizes
construction.

## Consequences

- T4.5 is mandatory and implements the optional production feature.
- Callers gain deterministic, resource-accounted candidate evidence for every
  named consumer while existing exact analyses remain authoritative.
- Default builds remain dependency-free with respect to approximate automata.
- The experiment remains as a reproducible oracle, algebra, supply-chain, and
  performance regression suite.
- New consumers must name their proposition, confirmation predicate, corpus,
  relevance measure, limits, and completion semantics before using the engine.

## References

1. Schulz, K. U., Mihov, S. (2002). *Fast String Correction with Levenshtein
   Automata.* International Journal on Document Analysis and Recognition 5,
   67--85. DOI:
   [10.1007/s10032-002-0082-8](https://doi.org/10.1007/s10032-002-0082-8).
2. Damerau, F. J. (1964). *A Technique for Computer Detection and Correction of
   Spelling Errors.* Communications of the ACM 7(3), 171--176. DOI:
   [10.1145/363958.363994](https://doi.org/10.1145/363958.363994).
3. Mohri, M. (1997). *Finite-State Transducers in Language and Speech
   Processing.* Computational Linguistics 23(2), 269--311.
   [ACL Anthology J97-2003](https://aclanthology.org/J97-2003/).
4. Lawvere, F. W. (1973/2002). *Metric Spaces, Generalized Logic, and Closed
   Categories.* Reprints in Theory and Applications of Categories 1, 1--37.
   [TAC reprint](http://www.tac.mta.ca/tac/reprints/articles/1/tr1.pdf).
5. Knuth, D. E. (1984). *Literate Programming.* The Computer Journal 27(2),
   97--111. DOI:
   [10.1093/comjnl/27.2.97](https://doi.org/10.1093/comjnl/27.2.97).
