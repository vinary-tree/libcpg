# Scientific validation 66 -- Rust place-capability analysis

## Claims under test

The implementation claims that typed Rust ownership metadata plus exact CPG
identity reconstruct deterministic move/borrow/reborrow relations, that its
advisory findings obey represented CFG and DFG evidence, that optional
materialization is transactional and idempotent, and that input depth does not
become native call depth.

These tests validate libcpg's represented model. They do not validate rustc's
complete trait solver, Mid-level Intermediate Representation (MIR), non-lexical
lifetime (NLL), Polonius, or borrow-checking semantics.

## Example oracles

| Fixture | Independent expected result |
|---|---|
| move an owning `String`, then reach a later DFG use | one `Move` relation and one `UseAfterMove` witness |
| shared loan followed by live mutable loan of the same source | one `ConflictingBorrow` witness |
| two live shared loans | no conflict |
| field operand with exact `Reference` | field source and preserved reference-evidence pair |
| borrow through a mutable reference | `MutableReborrow`, not a root mutable borrow |
| known primitive value use | copy classification suppresses a move relation |
| unknown nominal type | possible move plus `UnknownCopySemantics` qualification |
| malformed overlay endpoint | typed error and zero mutation |
| repeated valid materialization | second call adds zero edges |

## Property oracle

The generated insertion-order campaign permutes AST, CFG, DFG, and ownership
fixture edges independently. It requires equal `Pcg` reports and equal
`BorrowViolation` vectors after normalization. This targets construction-order
independence rather than sorting one result twice.

The edge-kind registry oracle enumerates all five `PlaceCapability` subtypes
and requires every edge to inhabit exactly one top-level classifier family.
The PyG registry oracle requires a contiguous class vector through class 66,
with all pre-existing columns/classes unchanged and only suffix identities
appended.

## Public integration and stack-safety oracles

`tests/place_capability_integration.rs` uses only crate-root APIs and a real
Rust parser. It checks parser-normalized move/borrow metadata, structured
reference types, relation reconstruction, and advisory findings.

`tests/place_capability_stack_safety.rs` builds 100,000 AST levels in a public
CPG and runs `place_capability_graph` on a 64 KiB worker stack. It exercises
scope discovery, source evidence, destination ancestor walking, report
construction, and destruction without source-depth recursion.

## Focused implementation evidence

Every compiler/test command runs with one Cargo build job inside a user
systemd scope with `MemoryMax=4G`, `MemorySwapMax=0`, and `TasksMax=256`.

| Gate | Evidence |
|---|---|
| focused unit/example/property suite | move, loan, field, reborrow, transaction, and insertion-order oracles pass |
| public parser integration | real Rust parser-to-PCG-to-report path passes |
| public depth gate | 100,000 levels on 64 KiB passes |
| default library suite | 939 tests pass |
| default and all-feature compilation | both configurations pass |
| strict Clippy | all targets/all features pass with warnings denied |
| complete all-feature/all-target matrix | 1,536 tests across 76 test targets and 260 Criterion test-mode cases pass; zero failures |
| rustdoc and doctests | default/all-feature rustdoc pass with warnings denied; both three-example doctest sets pass |
| documentation structure | 154 diagram sources render headlessly twice byte-identically; 126 canonical DOI links, 994 unique links, anchors, and MathJax delimiters pass policy |

The complete matrix includes the parser-built ownership oracle and the public
100,000-depth/64 KiB PCG analysis, materialization, and destruction oracle. It
also replays every pre-existing all-feature unit, property, integration,
stack-safety, example, and Criterion test-mode target so the append-only graph,
serialization, Datalog, and machine-learning changes cannot silently regress
another analysis family.

## Reproduction

```text
systemd-run --user --scope \
  -p MemoryMax=4G -p MemorySwapMax=0 -p TasksMax=256 \
  -E CARGO_BUILD_JOBS=1 cargo test --all-features
```

Do not compile in `/tmp` on systems where it is memory-backed.

## References

1. Grannan, D. et al. (2026). DOI:
   [10.1145/3763122](https://doi.org/10.1145/3763122).
2. Matsushita, Y. et al. (2020). DOI:
   [10.1007/978-3-030-44914-8_18](https://doi.org/10.1007/978-3-030-44914-8_18).
3. Crichton, W. et al. (2022). DOI:
   [10.1145/3519939.3523445](https://doi.org/10.1145/3519939.3523445).
