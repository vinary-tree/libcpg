# Scientific validation 19 — Martin package metrics

This record states the falsifiable claims behind `martin_metrics` and
`sdp_violations`, and identifies the example, property, integration, malformed-
input, and serialization experiments that decide them.

## Hypotheses

1. Coupling counts distinct external projection vertices and ignores internal
   edges, self-loops, weights, duplicates, and unassigned endpoints.
2. Instability is exactly $`C_e/(C_a+C_e)`$ with the isolated case defined as
   zero.
3. Abstractness counts abstract classes and traits over all class/struct/enum/
   trait declarations in each assignment bucket.
4. Distance is exactly $`|A+I-1|`$ and every scalar remains in $`[0,1]`$.
5. An SDP record exists exactly for module edges whose source instability is
   strictly lower than the target instability.
6. Stable IDs, module names, metrics, and violations are deterministic under
   assignment-pair order and projection-edge order.
7. Built-in module/source assigners, malformed public fields, serde, and parser
   integration remain total and read-only.

## Exact three-module oracle

The named unit fixture uses module edges `A -> B`, `A -> C`, `B -> C`, and
`C -> A`. `A` contains one trait; `B`, one concrete class; and `C`, one abstract
class plus one struct. The test asserts every field:

| Module | Expected `(Ca, Ce)` | Expected `I` | Expected `A` | Expected `D` |
|---|---:|---:|---:|---:|
| `A` | `(1, 2)` | $`2/3`$ | 1 | $`2/3`$ |
| `B` | `(1, 1)` | $`1/2`$ | 0 | $`1/2`$ |
| `C` | `(2, 1)` | $`1/3`$ | $`1/2`$ | $`1/6`$ |

It then requires exactly one SDP result: `C -> A`, witnessed by stable node
edge `30 -> 10`, because $`1/3<2/3`$. This fixture distinguishes afferent from
efferent direction, real abstractness from the former zero placeholder, and
strict from non-strict SDP comparison.

## Assignment examples and hostile public values

Named tests cover:

- `<root>`, nested `outer::<anonymous@9>`, and source-path buckets;
- sparse and nonconsecutive `NodeId` values;
- lexicographically canonical duplicate-node assignments;
- out-of-range bucket indices and projection nodes absent from the CPG;
- intra-module self-loops and weighted cross-module edges;
- malformed CSR offsets;
- duplicate metric names and non-finite instability; and
- serde round trips for assignment, metric, and violation values.

The expected behavior is total, not permissive: malformed global inputs produce
no verdict, while individual unmapped edges are inert.

## Generated determinism and range experiment

A 256-case property generates up to sixteen sparse-ID type nodes, four module
names, arbitrary abstract-class flags, and up to 96 weighted directed edges.
For every case it:

1. reverses assignment pairs and requires byte-equivalent canonical values;
2. reverses edge insertion and requires equal projections, metric vectors, and
   SDP records;
3. checks $`I,A,D\in[0,1]`$;
4. checks zero coupling implies zero instability; and
5. checks every reported SDP witness has positive multiplicity and satisfies
   the strict instability inequality.

This property targets iteration-order and range invariants. The named oracle
provides the independent arithmetic discrimination.

## Public and parser integrations

`tests/martin_integration.rs` has two boundaries:

- a feature-free sparse-ID graph asserts exact public crate-root values,
  lexical ordering, one SDP witness, repeat equality, and CPG non-mutation; and
- a `lang-rust` parse of nested modules asserts `api`, `api::nested`, and
  `implementation` buckets, with a trait yielding abstractness one while an
  enum and struct yield zero.

The whole-CPG robustness harness also constructs an external projection over
every malformed CPG variant, derives the module assignment, checks all scalars
and ordering, and verifies every returned violation's inequality.

## Reproduction

```text
# Feature-free examples and 256 generated cases:
cargo test --no-default-features analysis::martin

# Public feature-free and parser integrations:
cargo test --no-default-features --test martin_integration
cargo test --all-features --test martin_integration

# Serde and malformed whole-CPG campaign:
cargo test --all-features analysis::martin
cargo test --all-features --test robustness

# Repository gates and coverage:
./scripts/verify.sh
cargo cov-text
cargo +nightly cov-branch
```

In the clean maximal matrix, `src/analysis/martin.rs` measures **100.00%** line,
**99.86%** region, **100.00%** function, and **95.00%** branch coverage. The
repository totals are recorded in the [testing
guide](../engineering/02-testing.md). Coverage establishes execution, while the
exact oracle and generated invariants provide the stronger semantic evidence.
The implementation task explicitly defers a Criterion target; therefore this
record makes no throughput or scaling measurement claim. Expected asymptotic
cost and that benchmark gap are recorded in the [measurement
methodology](04-measurement-methodology.md).

## Interpretation boundary

The experiments establish exactness for the supplied directed projection,
assignment, and CPG type taxonomy. They do not establish parser completeness,
call/import-resolution completeness, change probability, ownership, runtime
coupling, or whether an SDP violation should be removed. The metric remains an
advisory structural measurement whose graph and boundary policy must accompany
the result.

See [Theory 18](../theory/18-martin-package-metrics.md), the [component
guide](../components/graph/martin-metrics.md), and [ADR
0024](../design/0024-explicit-martin-module-assignments.md).

## References

1. Martin, R. C. (1994). *OO Design Quality Metrics: An Analysis of
   Dependencies.* [Archived report
   PDF](https://condor.depaul.edu/dmumaugh/OOT/Design-Principles/oodmetrc.pdf).
   No DOI assigned.
2. Martin, R. C. (2003). *Agile Software Development: Principles, Patterns,
   and Practices.* ISBN 978-0-13-597444-5. [Publisher
   record](https://www.informit.com/store/agile-software-development-principles-patterns-and-9780135974445).
