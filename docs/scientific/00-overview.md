# Scientific validation — overview

This pillar documents **what `libcpg` claims to be true, and the experiments that decide whether those claims hold.** It treats the library's inline test suite as a *scientific ledger*: every property the analyses depend on is stated as a falsifiable hypothesis, encoded as a deterministic test, and settled by an assertion that either passes (the hypothesis survives) or fails (the hypothesis is refuted and the code is wrong).

`libcpg` is a [Code Property Graph](../GLOSSARY.md#code-property-graph-cpg) (CPG) library: it merges a program's [Abstract Syntax Tree](../GLOSSARY.md#abstract-syntax-tree-ast), [Control Flow Graph](../GLOSSARY.md#control-flow-graph-cfg), [Data Flow Graph](../GLOSSARY.md#data-flow-graph-dfg), and — on demand — its [Program Dependence Graph](../GLOSSARY.md#program-dependence-graph-pdg) onto one shared node set, following Yamaguchi et al. [[1]](#references). Downstream consumers (pattern detection, slicing, embeddings, and the `pgmcp` indexer that drives Rholang/MeTTa via [Mode B](../GLOSSARY.md#mode-b--build_from_tree)) trust that graph. If the graph is malformed — duplicated edges, a use wired to the wrong definition, a pattern search that silently drops a match — every analysis built on top inherits the error. Validation is therefore not an afterthought; it is the contract.

## 1. Why a static-analysis library must be validated, not just tested

A CPG library makes three kinds of assertion, and each demands a different standard of evidence:

| Kind of claim | Example | Standard of evidence | Where it lives |
|---|---|---|---|
| **Structural invariant** | "Re-running an extractor adds no duplicate edges." | A deterministic property that must hold on *every* input; validated by construction + regression tests. | [01 — Invariants & equivalence](01-cpg-invariants-and-equivalence.md) |
| **Semantic correctness** | "A use resolves to its latest reaching definition, not a killed one." | Matches the textbook data-flow answer on a corpus of hand-audited scenarios. | [02 — Reaching-defs validation](02-reaching-defs-validation.md) |
| **Algorithmic completeness** | "The subgraph matcher finds *all* embeddings, no more and no fewer." | An exact count on a *discriminating* input that would expose a partial or over-eager search. | [03 — VF2 completeness](03-vf2-completeness.md) |

These are distinct from *performance* claims ("construction is fast", "VF2 prunes well"), which require measurement rather than assertion. `libcpg` does **not** yet ship runnable benchmarks, so those claims are currently unproven; the rigorous method to establish them once benches are added is specified separately in [04 — Measurement methodology](04-measurement-methodology.md).

## 2. Tests as experiments: the scientific method applied

Every validation in this pillar follows the same loop, recorded so it can be audited and reconstructed:

```text
Hypothesis   A precise, falsifiable statement about the CPG or an algorithm
             (e.g. "build and build_from_tree produce graphs of identical shape").
     │
     ▼
Experiment   A deterministic inline #[cfg(test)] test with a fixed input, a fixed
             grammar/build path, and no randomness in the assertion path.
     │
     ▼
Result       The test's assertions. A pass corroborates the hypothesis; a failing
             assertion refutes it and localizes the defect.
     │
     ▼
Conclusion   Either the invariant holds (and the test guards it against regression),
             or a new hypothesis is derived and the loop repeats.
```

Two properties of the suite make this rigorous rather than merely suggestive:

- **Determinism.** The grounding tests fix their inputs and their build path. The one search that could in principle vary its *order* — VF2 — is pinned by a test that asserts an *exact set* of embeddings, not just a count, so a re-ordering cannot mask a dropped match (see [03](03-vf2-completeness.md)).
- **Discrimination.** A good experiment can *fail*. The VF2 regression deliberately uses a diamond target rather than a path, precisely because a path would pass even with a broken backtracker; only the diamond forces the mid-search retry that exposes the bug. A test that cannot distinguish correct from incorrect behaviour is not evidence.

## 3. The evidence base

All correctness evidence is **inline**: `libcpg` ships **99** `#[cfg(test)]` tests compiled into the crate under `#[cfg(test)]` modules across the `src/` tree. There are *no* separate integration tests and *no* example programs — the `tests/` and `examples/` directories exist but are empty, and the `[[bench]]` targets in `Cargo.toml` are commented out. This is a deliberate, honestly-documented shape: the correctness contract is fully specified by the inline suite, while the performance contract is a known gap ([04](04-measurement-methodology.md)).

Because `default = []`, many tests are **feature-gated**. The end-to-end reaching-defs and PDG scenarios that parse real Rust are annotated `#[cfg(feature = "lang-rust")]`; the graph-algebra and VF2 tests are feature-free (they build CPGs by hand). The map below notes which gate each grounding test sits behind.

## 4. Map of the validation pillar

| Page | Property validated | Anchoring inline test(s) | Gate |
|---|---|---|---|
| [01 — Invariants & equivalence](01-cpg-invariants-and-equivalence.md) | Single shared node set; graph-assigned unique IDs; AST child/parent consistency; idempotent extractors; `build` ≡ `build_from_tree` (identical node/edge counts + language) | `test_build_from_tree_matches_build`, `parsed_shadowing_and_idempotent`, `def_use_backward_slice`, `parsed_nested_use_resolves_and_chains` | `lang-rust` (equivalence, DFG) / feature-free (slice) |
| [02 — Reaching-defs validation](02-reaching-defs-validation.md) | Nested-expression use resolution; latest-definition (strong update); non-flow of unrelated variables; shadowing scope; idempotency | `parsed_nested_use_resolves_and_chains`, `parsed_reassignment_uses_latest_definition`, `parsed_unrelated_variable_does_not_flow`, `parsed_shadowing_and_idempotent` | `lang-rust` |
| [03 — VF2 completeness](03-vf2-completeness.md) | Soundness floor (empty pattern → no match); kind-feasibility pruning; **all** embeddings found (diamond → exactly 2) with exact backtracking | `test_empty_pattern`, `test_single_node_match`, `test_multi_embedding_backtracking` | feature-free |
| [04 — Measurement methodology](04-measurement-methodology.md) | *(none yet — documented gap)* the rigorous protocol to establish performance claims when benchmarks are added | — | — |

## 5. What is proven, and what is advisory

Scientific honesty requires separating the two:

- **Proven (guarded by the tests above).** The CPG's structural invariants, the reaching-definition semantics on the audited corpus, and VF2's completeness on the discriminating diamond. These are deterministic and regression-locked.
- **Advisory (heuristic by design).** [Design-pattern detection](../GLOSSARY.md#design-pattern), [algorithm-family](../GLOSSARY.md#algorithm-family) recognition, and [GNN](../GLOSSARY.md#graph-neural-network-gnn) embeddings are *heuristics*: they score likelihood, they do not prove identity. Their tests check that the machinery runs and returns well-formed, plausibly-ranked results — not that a classification is correct in the sense a theorem is correct. Treat their output as evidence to be reviewed, never as ground truth.
- **Unproven (a gap, not a defect).** Performance. No timing is asserted anywhere in the crate today; any speed claim must be earned by the protocol in [04](04-measurement-methodology.md).

## 6. Reproducing the experiments

The suite is the executable form of this documentation. To re-run every corroborating experiment:

```text
# All correctness tests, every analysis compiled in:
cargo test --all-features

# The reaching-defs and PDG end-to-end corpus (needs the Rust grammar):
cargo test --features lang-rust reaching       # dfg.rs scenarios
cargo test --features lang-rust                 # + pdg.rs parsed tests

# The feature-free graph-algebra and VF2 evidence (no grammar needed):
cargo test vf2
cargo test def_use_backward_slice
```

Each subsequent page names its tests, quotes the exact assertions that decide the hypothesis, and cross-links the theory (`../theory/`) and builder-component (`../components/builder/`) pages that explain the mechanism under test.

## References

1. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and Discovering Vulnerabilities with Code Property Graphs.* 2014 IEEE Symposium on Security and Privacy. DOI: [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44)
