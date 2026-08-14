# Scientific validation 25 — Function clone detection

This page records the falsifiable claims, fixtures, and limits for the
feature-free `detect_clones` implementation. It distinguishes conformance to
the documented CPG relations from empirical clone-retrieval quality on an
external corpus.

## Claims and experiments

| Claim | Experiment | Oracle |
|---|---|---|
| verbatim semantic subtrees are Type-1 | equal identifier/literal payloads; one side adds a comment | one Type-1 class, score `1.0` |
| renaming and literal substitution are Type-2 | equal ordered tags with different payloads | one Type-2 class, score `1.0` |
| statement insertion can be Type-3 | four common named nodes plus one inserted `Return` | Jaccard $`5/6`$, Type-3 |
| reordered independent statements can be Type-4 | swap unlike AST children; use isomorphic control-dependence stars | Type-4 after WL and VF2 |
| no cap is silent | set candidate-pair or VF2 budget to zero; lower PDG cap below the fixture | typed `BudgetExceeded` with exact unit/needed/cap |
| invalid options are rejected | NaN/out-of-range thresholds and over-hard-limit options | typed `InvalidInput` |
| malformed ASTs terminate | add AST back edges to both function fixtures | finite deterministic result |
| output is deterministic | execute identical analysis twice | byte-identical debug representation |
| result is ID-permutation invariant | generate six unique IDs and compare through inverse relabeling | identical type, canonical members, and score bits |
| public API is feature-free | black-box integration test under `--no-default-features` | all four clone types and cap error pass |
| public values preserve wire shape | JSON round-trip under `serde` | exact `CloneOptions` and `CloneClass` equality |

The permutation experiment is property-based. It samples arbitrary unique
stable identifiers, reconstructs the same ordered AST relations, applies the
inverse bijection to output members, and compares exact `f64::to_bits` scores.
This directly tests the isomorphism-invariance claim rather than merely varying
edge insertion order.

## Boundary rationale

The size-ratio filter is lossless for the configured Type-3 threshold because
multiset Jaccard cannot exceed the smaller-to-larger cardinality ratio. Type-4
requires equal PDG vertex and edge counts before VF2; a complete injective
mapping between equal finite vertex sets is bijective. The VF2 matcher is
configured for one result, so its match cap changes work but not the yes/no
verdict.

The tests intentionally cover empty graphs and hand-built CPGs. The public API
does not accept a caller-selected root, so an “out-of-graph function” cannot be
passed; function roots are enumerated only from live CPG nodes. PDG construction
nevertheless retains its own selector validation for reuse outside this API.

## Reproduction

```bash
cargo test --no-default-features pattern::clones
cargo test --no-default-features --test clones_integration
cargo test --no-default-features --features serde pattern::clones
cargo clippy --all-targets --no-default-features -- -D warnings
cargo bench --bench clones --no-default-features -- --test
./scripts/verify.sh
```

Coverage measurement includes all unit and integration test binaries. The
clean maximal-feature matrices execute 848 library tests and all public/parser
integration binaries. They measure `pattern/clones.rs` at **95.28% line**
(929/975), **93.37% region** (1,366/1,463), **96.88% function** (62/64), and
**92.11% branch coverage** (70/76). Repository totals are **98.19% line**,
**98.06% region**, **98.38% function**, and **87.01% branch coverage**. Focused
green tests are not substituted for these full-matrix measurements.

## What is not established

- The suite does not estimate precision or recall on Bellon's clone corpus.
- A 64-bit fingerprint collision is possible, although fixed-vector and
  property tests protect implementation stability.
- PDG isomorphism does not prove observational program equivalence.
- The Criterion fixture families cover dense Type-1 classes, Type-3 insertion
  classes, and Type-4 reordering classes at 16 and 64 functions. They do not
  establish a universal performance claim for arbitrary clone-density or
  function-size distributions.

## References

1. Bellon et al. define a widely used evaluation framework for clone tools:
   [DOI 10.1109/TSE.2007.70725](https://doi.org/10.1109/TSE.2007.70725).
2. Baxter et al. motivate AST-based clone fingerprints:
   [DOI 10.1109/ICSM.1998.738528](https://doi.org/10.1109/ICSM.1998.738528).
3. Zou et al. study PDG-based clone matching:
   [DOI 10.1145/3324884.3416541](https://doi.org/10.1145/3324884.3416541).
4. Komondoor and Horwitz connect program slicing and duplication:
   [DOI 10.1007/3-540-47764-0_33](https://doi.org/10.1007/3-540-47764-0_33).
