# Community-detection validation

This report records what the deterministic community suite demonstrates, how
the oracles are independent, and which conclusions remain outside the evidence.
The implementation contract is in
[deterministic community detection](../components/graph/community-detection.md).

## Hypotheses and rejection rules

| Hypothesis | Evidence | Reject when |
|---|---|---|
| H1: a planted pair of weighted cliques is recovered | analytic eight-node fixture | a clique is split, the bridge is absorbed incorrectly, or reported modularity is not positive |
| H2: reported modularity describes the returned partition | separate full-formula oracle over the public undirected projection | absolute disagreement exceeds `1e-9` on a generated graph |
| H3: aggregation is real and weight preserving | ring of eight cliques plus direct aggregate-weight check | multi-level output fails to coarsen the one-level result, lowers its score, or changes total edge weight |
| H4: deterministic boundaries remove storage order | repeated runs and reversed edge insertion sequences | any `CommunityAssignment` field differs |
| H5: canonical labels are complete | generated partitions | identifiers are not contiguous, not smallest-member ordered, or disagree with `num_communities` |
| H6: local moving does not worsen the singleton baseline | generated weighted graphs and resolutions | returned modularity is below independently computed singleton modularity beyond `1e-12` |
| H7: refinement guarantees ordinary connectivity | generated graphs and arbitrary canonical input partitions | breadth-first search cannot reach every member inside an output community |
| H8: refinement preserves score semantics | independent modularity oracle at inherited resolution | refined reported modularity disagrees beyond `1e-10` |
| H9: public boundaries are read-only and language-integrated | external projection test, parsed Rust call graph, shared corruption harness | a graph/CPG mutates, alignment breaks, or a boundary panics |
| H10: caps and invalid inputs are explicit | zero work, oversized requests, malformed CSR, negative/non-finite weights | a request exceeds 50 passes/10 levels, invalid data yields a plausible partition, or zero work performs a move |
| H11: optional serialization preserves the deterministic result | repeated JSON byte comparison and round trips for assignments/options | repeated bytes differ or decoding changes a public field |
| H12: performance fixtures cannot time an invalid result | Criterion pre-timing oracles for Louvain and connected refinement | repeatability, alignment, canonicality, independent score, component count, or connectivity fails before measurement |

## Independent modularity oracle

Production code stores canonical undirected edges, weighted degree, and
community totals. The test oracle instead traverses the public
`UndirectedProjection`, selects one orientation with `target >= source`, and
reconstructs total weight, per-community degree, and internal weight. It then
evaluates

```math
Q_\gamma=\sum_C\left[
\frac{L_C}{m}-\gamma\left(\frac{D_C}{2m}\right)^2
\right].
```

The oracle does not call the production modularity helper or inspect its
private level graph. Agreement therefore catches self-loop convention,
reciprocal-weight, final-label, and stale-score defects.

## Example-based experiments

### Planted blocks

Two four-vertex cliques are connected by a weight-`0.1` bridge. Standard
resolution recovers exactly `[0,0,0,0,1,1,1,1]` and reports modularity above
`0.4`. A six-vertex public integration fixture independently checks two
triangles with the same weak bridge.

### Complete graph and self-loops

The complete graph on four vertices becomes one community with standard
modularity zero. Two isolated self-loop vertices remain separate. Their score
is checked against the independent formula, locking the “loop once in edge
mass, twice in degree” convention.

### Multi-level witness

Eight three-vertex cliques are arranged in a ring. One level reaches a
vertex-local partition. Allowing aggregation produces fewer communities and a
score no lower than the one-level result. A second fixture directly contracts
two weighted triangles and proves that total edge weight is unchanged.

### Connected refinement

An intentionally invalid one-community assignment covers two disconnected
components, a two-vertex path and a three-vertex path. Refinement returns
`[0,0,1,1,1]`, inherits resolution `0.75`, and reports the independently
recomputed score.

### Parser integration

A Rust source fixture declares two three-function collaboration clusters with
one cross-cluster call. Tree-sitter construction, call resolution,
`call_graph_projection`, Louvain, and stable function lookup run end-to-end.
The two clusters are recovered and the CPG node/edge counts remain unchanged.

### Benchmark fixtures

The Criterion target uses rings of eight-vertex cliques at 256 and 1,024
vertices for Louvain, plus disconnected eight-vertex paths at 512 and 2,048
vertices for refinement. It invokes independent modularity and connectivity
oracles before timing, so a performance run cannot silently measure a malformed
or semantically inconsistent result.

## Property-based experiments

The Louvain property generates up to eight vertices, 31 directed weighted
edges, reciprocal and self edges, duplicate insertion candidates, and
resolutions from `0.25` to `2.5`. It compares two runs and a reversed insertion
sequence, validates canonical labels, invokes the independent modularity
formula, and compares the result with the singleton baseline.

The refinement property generates up to nine vertices, 35 edges, and arbitrary
raw labels. Labels are canonicalized before the checked boundary. An
independent breadth-first search then proves every output community connected
and rechecks canonical numbering.

`proptest` shrinking retains the smallest counterexample when any invariant
fails. The properties complement, rather than replace, the analytic fixtures.

## Malformed and adversarial cases

- malformed forward CSR is rejected before indexed traversal;
- negative, infinite, and not-a-number weights are rejected;
- overflowing reciprocal-weight sums are rejected;
- invalid resolution is rejected;
- noncanonical or misaligned assignments are rejected by
  `try_refine_connected`;
- the total refinement wrapper returns a documented singleton fallback;
- empty and edgeless graphs remain aligned and finite;
- zero passes or levels perform no local moving;
- `usize::MAX` pass/level requests equal the hard-ceiling request; and
- the shared ten-corruption CPG campaign exercises Louvain and refinement on
  every resulting call projection.

## Reproduction

```sh
cargo test --all-features community::tests
cargo test --all-features --test community_integration
cargo bench --bench community --no-default-features -- --test
./scripts/verify.sh
cargo cov-text
cargo +nightly cov-branch
```

Validation output is captured before inspection and removed after the stable
commit, following the repository engineering protocol.

## Measured coverage

The clean maximal stable and nightly runs each executed **689 library tests**
and **47 integration/robustness cases**. For `analysis/community.rs`, the stable
run measured **97.58 % line**, **98.34 % region**, and **100.00 % function**
coverage; the nightly run additionally measured **83.62 % branch** coverage.
The whole repository measured **97.98 % line**, **97.92 % region**, **98.21 %
function**, and **85.08 % branch** coverage in the nightly run.

These are maximal-matrix measurements, not figures from the ten-test focused
filter. No community source file or assertion is excluded from the measurement;
the behavioral oracles above, rather than coverage percentage alone, decide the
stated hypotheses.

## Limits of the evidence

The suite does not prove global modularity optimality, eliminate modularity's
resolution limit, validate an architectural interpretation against human
ground truth, or implement full Leiden. It does prove the stated deterministic
algorithm, score consistency, multi-level contraction, connected-refinement
invariant, caps, error boundary, and integration behavior on the tested domain.

## References

- Blondel et al. 2008.
  [DOI: 10.1088/1742-5468/2008/10/P10008](https://doi.org/10.1088/1742-5468/2008/10/P10008).
- Traag, Waltman, and van Eck 2019.
  [DOI: 10.1038/s41598-019-41695-z](https://doi.org/10.1038/s41598-019-41695-z).
