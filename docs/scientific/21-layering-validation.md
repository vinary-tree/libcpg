# Scientific validation 21 — architectural layering inference

This record states the falsifiable claims and executable evidence for
feature-free longest-path architectural layering.

## Hypotheses

1. Reversing source-oriented levels produces bottom-first architectural layers
   with a direct component lookup.
2. Every unflagged condensation edge descends exactly one layer.
3. A descent greater than one is reported as `SkipLayer`; a zero or negative
   descent is reported as `Upward`.
4. Every cyclic SCC contributes exactly one `CyclicComponent` witness without
   inventing a contracted internal edge.
5. Equal public inputs always produce equal ordered reports.
6. Empty, malformed, capped, hostile-level, and serialized values remain total.
7. Inference does not mutate the source CPG, SCC decomposition, or metrics.

## Exact oracle

The unit oracle uses four components and edges `0 -> 1`, `1 -> 2`, `2 -> 3`,
and `0 -> 2`. Source levels `[0, 1, 2, 3]` must become layers
`[[3], [2], [1], [0]]`. Component `1` is cyclic, so the complete violation set
is its cyclic witness plus the two-layer `0 -> 2` skip. Iterating every other
edge must satisfy

```math
a(u)=a(v)+1.
```

A second oracle supplies bounded stale levels `[1, 1, 0]` for edges `0 -> 1`
and `1 -> 2`; it requires one lateral and one ascending `Upward` witness. This
separates violation classification from the guarantee made by directly
computed longest-path metrics.

## Generated experiment

A 256-case property generates up to 20 components, arbitrary forward DAG edges,
and arbitrary cyclic singleton markers. It computes real condensation metrics,
runs `infer_layering` twice, and requires equal complete reports. For every
cross-component edge without a skip/upward witness it checks the one-step
equation above. This simultaneously exercises empty, isolated, chain, fork,
join, transitive-skip, and mixed-cycle-marker shapes.

## Hostile-input experiment

Public struct tests inject an edge endpoint outside the component domain, an
incomplete capped report, `u32::MAX` as a level, duplicate canonical edges, and
a multi-node component falsely marked acyclic. Every case must return an empty
incomplete report without panic or large allocation. The empty valid
decomposition remains distinguishable as complete.

## Integration and serialization

`tests/layering_integration.rs` builds a module graph whose facade modules form
an SCC, whose service and repository occupy lower levels, and whose facade-to-
repository edge skips the service. The crate-root pipeline must find both exact
violation kinds and leave graph counts unchanged. With `serde`, a public report
serializes stable `CyclicComponent` discriminants; the inline test round-trips
the complete report value.

## Reproduction

```text
cargo test --no-default-features analysis::layering
cargo test --all-features analysis::layering
cargo test --no-default-features --test layering_integration
cargo test --all-features --test layering_integration
./scripts/verify.sh
cargo cov-text
cargo +nightly cov-branch
```

The task explicitly defers a Criterion target. Complexity follows the bounded
linear condensation validation and edge scan; this record makes no throughput
claim. In the clean maximal matrix, `src/analysis/layering.rs` measures
**100.00%** line, **100.00%** region, **100.00%** function, and **100.00%**
branch coverage. The repository totals are **98.20%** line, **98.10%** region,
**98.41%** function, and **86.91%** branch coverage. Coverage remains supporting
evidence rather than proof.

## Interpretation boundary

These experiments establish deterministic transformation and classification
for supplied SCC/metric values. They do not establish that the projected
dependencies are complete, that a reported dependency is undesirable, or that
the longest-path grouping matches a team's intended architecture.

See [Theory 20](../theory/20-longest-path-architectural-layering.md), the
[component guide](../components/graph/layering-analysis.md), and [ADR
0026](../design/0026-longest-path-architectural-layering.md).

## Reference

1. Coffman, E. G., Jr., & Graham, R. L. (1972). Optimal scheduling for two-
   processor systems. *Acta Informatica, 1*, 200–213. DOI:
   [10.1007/BF00288685](https://doi.org/10.1007/BF00288685).
