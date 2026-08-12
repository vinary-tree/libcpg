# Scientific validation 20 — reflexion-model conformance

This record identifies the falsifiable claims and executable evidence for the
feature-free path reflexion model.

## Hypotheses

1. First-match textual prefixes implement the declared low-level-to-layer map.
2. Every observed edge is classified exactly once as a convergence,
   divergence, or unlayered edge.
3. Same-layer and explicitly allowed cross-layer edges converge; every other
   fully covered edge diverges.
4. Realized permitted pairs and declared-but-unrealized absences are exact.
5. Reordering an observed edge multiset cannot change any output value.
6. Duplicate observations retain multiplicity while pair sets and absence
   output remain deduplicated and ordered.
7. Empty, uncovered, duplicate, and serialized public values remain total.

## Exact oracle

The named unit fixture declares `api -> domain` and `domain -> data`, then
supplies five edges: same-layer, allowed, two forbidden directions/pairs, and
one uncovered source. It requires exactly two convergences, two lexically
ordered divergences, one unlayered edge, realized pair `api -> domain`, and
absence `domain -> data`. Separate assertions discriminate first-prefix
precedence and all four `ReflexionVerdict` values.

## Generated experiment

A 256-case property generates up to 127 edges over two paths in `api`, one in
`domain`, one in `data`, and one uncovered path. For each multiset it reverses
the input and requires equal summaries. It also requires:

```math
|E|=C+V+U,
```

where $`C`$ is convergence count, $`V`$ divergence-vector length, and $`U`$
unlayered count; lexical divergence order; and membership of every realized
pair in the declared `allow` relation.

## Integration and serialization

`tests/reflexion_integration.rs` exercises the crate-root API without features
using sparse, nonexistent path strings, reversed observation order, every
summary field, absence derivation, and caller-input non-mutation. With `serde`,
the same boundary serializes rules and proves the decoded rules produce the
same result. Inline serde tests round-trip both rules and summary values.

## Reproduction

```text
cargo test --no-default-features analysis::reflexion
cargo test --all-features analysis::reflexion
cargo test --no-default-features --test reflexion_integration
cargo test --all-features --test reflexion_integration
./scripts/verify.sh
cargo cov-text
cargo +nightly cov-branch
```

The implementation task explicitly defers a Criterion target. This evidence
therefore establishes semantics and exercised control flow, not a throughput
claim. In the clean maximal matrix, `src/analysis/reflexion.rs` measures
**100.00%** line, **100.00%** region, **100.00%** function, and **100.00%**
branch coverage. The repository totals are **98.18%** line, **98.09%** region,
**98.40%** function, and **86.73%** branch coverage. Coverage remains supporting
evidence rather than a correctness proof.

## Interpretation boundary

The experiments establish exactness for supplied strings and rules. They do
not establish dependency-extractor completeness, filesystem identity,
architectural quality, or whether a reported mismatch warrants code or policy
change.

See [Theory 19](../theory/19-reflexion-model-conformance.md), the [component
guide](../components/graph/reflexion-model.md), and [ADR
0025](../design/0025-pure-path-reflexion-model.md).

## Reference

1. Murphy, G. C., Notkin, D., & Sullivan, K. J. (2001). Software reflexion
   models: Bridging the gap between design and implementation. *IEEE
   Transactions on Software Engineering, 27*(4), 364–380. DOI:
   [10.1109/32.917525](https://doi.org/10.1109/32.917525).
