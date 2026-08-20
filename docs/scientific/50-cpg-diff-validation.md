# Scientific validation 50 — Deterministic dependence-aware CPG diff

## Claims under test

For two admitted CPGs, `cpg_diff` returns one complete, deterministic edit
script under the chosen bounded bipartite GED correspondence. Logical results are
independent of graph insertion order, preserve exact typed edge multiplicity,
translate embedded node references, do not mutate either graph, and keep graph
or property depth off the native call stack.

The classification claim is narrower: `BehaviorChanging` occurs exactly when
the returned script changes a represented `ControlDependence` or
`DataDependence` edge. The tests do not establish source-language behavioral
equivalence, exact GED, or semantic identity of the heuristic node alignment.

## Exact example oracles

| Experiment | Discriminating expectation |
|---|---|
| identical graph | complete alignment, zero cost, empty edits, `Refactoring` |
| pure rename with renumbered ids | two node substitutions, no add/remove, translated definition reference, `Refactoring` |
| added guarded region | inserted control-dependence evidence, `BehaviorChanging` |
| exact parallel edges | equal occurrences consumed individually; leftover typed/labelled occurrences become canonical substitutions |
| embedded ids | call target, identifier definition, lambda captures, parent, and ordered children compare through alignment |
| property insertion permutations | one canonical closed-key/custom-key snapshot order |
| malformed cached metadata | terminates and reports exact `AstLinks` difference rather than repairing input |
| zero/caller/hard caps | complete empty inputs succeed; exceeded node, edge, or edit quantity returns a typed budget error |
| serde | options and complete result round-trip without evidence loss |

The rename fixture is especially important: old ids `10/30/70` and new ids
`101/303/707` must align without node insertions/deletions. That falsifies an
implementation that mistakes stable coordinates for cross-version identity.

## Determinism property

One 128-case `proptest` campaign generates bounded logical graph pairs and
constructs each in opposite node/property/edge insertion orders. The property
requires exact equality of alignment, alignment cost, all edit vectors, changed
field order, property order, parallel-edge multiplicity, and classification.

This targets the actual determinism hazards: petgraph indices, hash-map
enumeration, assignment ties, and multiset consumption. Comparing a result only
with itself would not exercise those hazards.

## Public and parser integration

`tests/cpg_diff_integration.rs` has two public-API experiments:

1. a feature-free hand-built pair proves a renumbered rename remains
   `Refactoring`, then adds one control-dependence edge and requires
   `BehaviorChanging`; and
2. with `lang-rust`, independently parsed unguarded and guarded functions are
   enriched through `PdgBuilder`, after which the guarded version must expose a
   changed PDG edge and classify `BehaviorChanging`.

Both record input graph shape before diffing and require immutability.

## Small-stack experiment

`tests/cpg_diff_stack_safety.rs` constructs two 128-node chains whose root owns
a logically depth-100,000 `PropertyValue`. Node ids differ by 10,000 and only
root text changes. On a worker with a 256 KiB native stack, the experiment:

1. clones the flat property into both graphs;
2. performs the complete Hungarian alignment;
3. compares and snapshots the deep property without recursive clone/drop;
4. returns exactly one `Text` node substitution; and
5. destroys the graphs and result before the worker exits.

Passing demonstrates constant native call depth across property lifecycle and
diff machinery for this admitted graph size.

## GED regression composition

The diff intentionally reuses, rather than forks, ADR-0023 alignment. The adjacent
GED suite still requires Hungarian costs to equal brute-force permutations
through order seven. Its 128 generated graph pairs require deterministic,
symmetric concrete-script upper bounds no smaller than exhaustive exact GED.
Those experiments validate the shared assignment substrate; the diff tests
separately validate correspondence exposure and exact evidence comparison.

## Feature and gate matrix

| Command family | Evidence |
|---|---|
| `cargo test --no-default-features analysis::diff --lib` | eight feature-free examples/properties |
| `cargo test --all-features analysis::diff --lib` | nine cases including serde |
| `cargo test --all-features --test cpg_diff_integration` | feature-free and Rust parser/PDG composition |
| `cargo test --no-default-features --test cpg_diff_stack_safety` | depth-100,000 property on 256 KiB |
| `cargo test --all-features pattern::ged --lib` | eight shared GED assignment regressions |
| `cargo clippy --all-targets --all-features -- -D warnings` | warning-free complete target surface |
| `./scripts/verify.sh` | format, build matrices, all tests, examples, clippy, rustdoc, docs, and diff hygiene |

## Falsification boundaries

Results are exact relative to the selected heuristic alignment and represented
node/edge payload. They do not prove the alignment is semantic identity, the
script is a globally minimal graph edit path, or two programs are behaviorally
equivalent. Container metadata is outside the experiment. A graph without
comparable PDGs cannot falsify dependence preservation. Runtime scaling lacks
an oracle-guarded Criterion target and remains a recorded empirical gap.

## References

1. Riesen, K., Bunke, H. (2009). *Approximate graph edit distance computation
   by means of bipartite graph matching.* Image and Vision Computing 27(7).
   DOI: [10.1016/j.imavis.2008.04.004](https://doi.org/10.1016/j.imavis.2008.04.004).
2. Kuhn, H. W. (1955). *The Hungarian Method for the Assignment Problem.*
   Naval Research Logistics Quarterly 2(1–2). DOI:
   [10.1002/nav.3800020109](https://doi.org/10.1002/nav.3800020109).
3. Ferrante, J., Ottenstein, K. J., Warren, J. D. (1987). *The Program
   Dependence Graph and Its Use in Optimization.* ACM TOPLAS 9(3). DOI:
   [10.1145/24039.24041](https://doi.org/10.1145/24039.24041).
