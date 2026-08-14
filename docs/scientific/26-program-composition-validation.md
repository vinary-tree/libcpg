# Scientific validation 26 — Multi-file program composition

This page records the falsifiable claims and executable oracles for
`CodePropertyGraph::merge_from`, `merge_programs`, and `build_import_graph`.
The tests distinguish source-graph preservation, translated-reference totality,
and import evidence from the later topology that evidence authorizes.

## Claims and experiments

| Claim | Experiment | Oracle |
|---|---|---|
| fresh identities avoid collisions | merge graphs with sparse, non-monotone source ids into a non-empty destination | exact source-to-destination map and contiguous fresh ids |
| every typed reference is translated | fixture populates parent, children, `Call.target`, `Identifier.definition`, `Lambda.captures`, edge endpoints, CFG entries/exits | every expected image equals the map lookup and resolves in the destination |
| malformed references fail closed | source references absent id `777`; destination deliberately contains its own node `777` | merged option fields are `None`, sequences omit `777`, no accidental binding |
| merge is append-only | retain a destination node, then merge | original node and root remain; counts increase by source counts |
| equal snapshot duplicates exactly | clone graph, merge clone into original | node and edge counts are exactly doubled |
| generated merges are deterministic | merge two `arb_well_formed_cpg` values twice | exact node/edge/metadata shape equality |
| generated references remain total | inspect every merged generated node | every embedded `NodeId` resolves |
| program file scopes are complete | merge heterogeneous files with metadata | ordered scopes, exact maps, disjoint node sets, `Language::Unknown` |
| exact imports emit both granularities | `app.rs` imports `math::add` | file-root dependency, declaration import, export edge, exact evidence |
| import pass is idempotent | execute it twice | second `emitted_edges == 0`; evidence and edge count unchanged |
| ambiguity creates no topology | two `shared.rs` or `math.rs` candidates tie | `Ambiguous`, sorted candidate files, no dependency/import edge |
| absent target stays external | import has no represented alias | `External`, empty candidate files and dependencies |
| cross-file evidence disambiguates calls | `math.rs` and `rival.rs` both define `add`; app imports only `math::add` | call is `ExactViaImport` to `math::add`, candidate count one |
| repeated imports do not create false ambiguity | two syntax forms import the same declaration | one imported function candidate |
| renamed imports preserve local bindings | app imports `add as sum` while a rival declares `sum` | declaration edge label is `sum`; call resolves uniquely to imported `add` |
| grouped imports retain every binding | `use math::{add as sum, subtract}` | exact label set is `{sum, subtract}` |
| pre-`from` named imports do not leak aliases | JavaScript imports `{ add as sum }`; target also exports `sum` and `multiply` | only `add` is selected and its edge label is `sum` |
| common syntax normalizes consistently | Rust, Python, dotted-module, and C-include strings | all select the represented `math` file |
| nested declarations do not leak | top-level function/class plus class method | only top-level namespace declarations exported |
| public API is feature-free | black-box integration tests | exact cross-file call and ambiguous-alias boundary pass under `default = []` |
| serialized values preserve evidence | JSON round-trip under `serde` | `FileScope` and `ImportGraph` exact equality; graph counts equal |

Two 256-case property campaigns use `arb_well_formed_cpg`. One directly tests
`merge_from` append-only counts, contiguous identities, reference totality, and
repeat determinism. The other tests `merge_programs` over two files. A third
256-case property merges an equal snapshot and checks exact count doubling.

## Independent public integration boundary

`tests/program_integration.rs` imports only crate-root API. Its positive fixture
contains two globally same-named `add` functions, so a bare-name implementation
would be ambiguous. Passing requires the public import graph to select one file,
emit declaration evidence, and let `resolve_calls` produce
`Confidence::ExactViaImport` for the selected function. Its negative fixture
uses two equally strong `math` file aliases and asserts that no import statement
edge escapes. A third public fixture verifies renamed Python import resolution
through the local-binding edge label.

## Malformed-input discrimination

The collision test is stronger than checking for `None` on an absent id. It
pre-populates the destination through `NodeId(777)`. An implementation that
copied the malformed numeric reference would appear superficially well formed:
`contains_node(777)` would succeed. The oracle requires the reference to be
cleared, proving translation is based on membership in the source map rather
than numeric coincidence in the destination.

## Benchmark oracle

`benches/program_merge.rs` defines four Criterion fixtures:

- `merge_programs` for 16 and 64 files, each with 16 statement nodes; and
- `build_import_graph` for matching 16- and 64-file import chains.

Before timing, the merge oracle checks exact summed counts, contiguous ids,
complete file scopes, and every embedded reference. The import oracle checks
that all $`F-1`$ imports resolve exactly and produce $`F-1`$ dependencies. Timed
import iterations clone an unlinked `ProgramCpg`, so mutation does not leak
between samples.

## Reproduction

```bash
cargo test --no-default-features analysis::program
cargo test --no-default-features prop_merge
cargo test --no-default-features prop_merging_equal
cargo test --no-default-features --test program_integration
cargo test --no-default-features --features serde analysis::program
cargo clippy --all-targets --no-default-features -- -D warnings
cargo bench --bench program_merge --no-default-features -- --test
./scripts/verify.sh
```

## What is not established

- Alias resolution does not model a compiler's package manager, include search
  path, build graph, visibility checker, or conditional compilation.
- `Language::Unknown` on a heterogeneous program is metadata honesty, not a
  claim that every downstream analysis supports mixed-language semantics.
- Criterion samples describe only the stated deterministic fixtures and do not
  establish a universal performance bound.
- `Exports` identifies namespace declarations in the CPG; downstream consumers
  must not reinterpret it as a full language-specific accessibility proof.

## References

No external empirical or algorithmic result is asserted here. The normative
semantics are [ADR-0031](../design/0031-deterministic-program-composition.md),
and the equations are developed in
[Theory 24](../theory/24-program-composition.md).
