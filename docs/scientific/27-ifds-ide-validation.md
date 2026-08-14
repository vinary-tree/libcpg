# Scientific validation 27 — IFDS/IDE summary solving

This page states falsifiable claims for the feature-free `analysis::ifds`
framework and maps each claim to an independent oracle. The framework tests
whole-program scheduling and bounds; analysis-specific IFDS/IDE domains must
add their own local-flow, distributivity, and semantic oracles.

## Claims and executable experiments

| Claim | Experiment | Oracle |
|---|---|---|
| acyclic functions run once after callees | three-function call chain | exact processed order `leaf, middle, entry`; every component has one round |
| transitive summaries are correct | generated directed call graphs with one to six functions | independent breadth-first reachability from every function |
| edge insertion does not affect output | rebuild every generated graph with reversed call insertion | exact `IfdsOutput` equality |
| mutual recursion converges | two-function cycle | both summaries equal the hand-computed two-function closure |
| self recursion converges | singleton self-loop | exact singleton closure in two completed rounds |
| recursive iteration is bounded | same mutual cycle with zero, one, and two rounds | zero/one are capped with exact events; two is complete |
| iteration-cap completeness is monotone | generated rings and ordered cap pairs | complete at the lower cap implies complete and equal at the higher cap |
| work-cap completeness is monotone | generated acyclic paths and ordered cap pairs | same implication for scheduler work |
| summary lattice obeys its stated laws | generated finite sets | commutativity, associativity, and idempotence |
| broken lattice contracts are visible | `join` falsely reports no change | exact `LatticeLaw` issue; no completeness claim |
| facts are canonical and bounded | duplicated, permuted facts over one point | deduplicate, retain least facts, exact observed count, no solve after truncation |
| seed points stay in function scope | unknown and represented out-of-scope nodes | distinct `UnknownSeedNode` and `SeedOutsideFunction` evidence |
| empty input is a valid boundary | empty CPG with every cap zero | complete empty output |
| function and work caps stop before unbudgeted work | one function with caps zero/one | exact event and work count |
| resolver distinguishes evidence classes | definite, named unresolved, supplied external, and supplied out-of-graph targets | exact `CalleeRef` set; no panic or indexing |
| resolver label depth is bounded | unresolved call with nested name and retained fallback text | depth zero selects fallback; depth one selects child name |
| conflicting definite evidence is not guessed | target field and call edge disagree | one named `Unresolved` result |
| stale/malformed decompositions never run a domain | mutate projection, ids, membership, members, condensation order/endpoints/cycle | ordered issue set and empty processed list |
| public root API is sufficient | black-box recursive and external fixtures | complete exact closure and total external boundary under `default = []` |
| serde preserves order and evidence | recursive output JSON round trip | exact equality under `serde` |

The property module uses four 256-case campaigns: reachability plus insertion
permutation, recursive-iteration cap monotonicity, work-cap monotonicity, and
finite-set semilattice laws. Generated reachability uses an independently
implemented breadth-first search rather than the summary solver.

## Oracle separation

The reachability domain deliberately has minimal semantics: every function
summarizes itself, every resolved direct callee, and every fact already present
in that callee's summary. On a finite call graph, the expected result is exactly
the reflexive transitive closure. The oracle constructs adjacency directly from
the generated edge matrix and runs breadth-first search. It does not call
`call_graph_sccs`, use component order, or reuse `Lattice::join`.

This oracle catches three distinct defects:

- caller-first evaluation misses transitive callees in acyclic graphs;
- incomplete recursive iteration misses members of an SCC closure; and
- unstable member/ready ordering changes traces or cap behavior under
  insertion permutations.

## Malformed-input discrimination

The hostile SCC fixtures mutate one invariant at a time and in combination.
Exact fresh-decomposition comparison detects semantic staleness, while
fine-grained variants preserve diagnostic value. A separate cycle fixture adds
the reverse condensation edge but keeps the edge vector sorted, proving cycle
detection is not an accidental consequence of noncanonical ordering.

Seed validation distinguishes an unknown `NodeId` from a real node owned by a
different function. This prevents a domain bug from laundering cross-function
facts into a seemingly well-formed ordered map.

## Public integration boundary

`tests/ifds_integration.rs` imports only crate-root types. Its first fixture
contains an entry function calling a mutually recursive worker/leaf pair and
asserts the exact closure of all three summaries. Its second fixture supplies an
`External` resolver for an unresolved call and proves that out-of-program
evidence stays total without inventing a function summary.

## Benchmark oracle

`benches/ifds_summaries.rs` measures 256- and 1,024-function path and
eight-function-clustered recursive graphs. The SCC decomposition is prepared
outside the timed region so the target measures summary scheduling and domain
work. Before timing, every fixture must be complete, the first function must
reach all represented functions, and no function summary may be empty.

The benchmark uses `black_box`, fixed sample/warm-up/measurement intervals, and
`Throughput::Elements(function_count)`. These fixtures establish regression
comparability, not a universal performance bound.

## Reproduction

```bash
cargo test --no-default-features --lib analysis::ifds::tests
cargo test --no-default-features --test ifds_integration
cargo test --no-default-features --features serde --lib analysis::ifds::tests
cargo clippy --all-targets --no-default-features -- -D warnings
cargo bench --bench ifds_summaries --no-default-features -- --test
./scripts/verify.sh
```

## What is not established

- The framework cannot prove a client `SummaryDomain` monotone or
  distributive; each instantiation needs semantic tests.
- The default resolver models represented call evidence, not dynamic dispatch,
  reflection, foreign-function semantics, or package availability.
- A capped may-analysis result is not evidence that an unobserved flow is
  absent.
- The generic engine does not by itself label sources, sinks, sanitizers,
  effects, or typestate transitions.
- Benchmark timings generalize only to the documented fixtures and measurement
  environment.

## References

1. Reps, T., Horwitz, S., Sagiv, M. (1995). *Precise Interprocedural Dataflow
   Analysis via Graph Reachability.* DOI:
   [10.1145/199448.199462](https://doi.org/10.1145/199448.199462).
2. Sagiv, M., Reps, T., Horwitz, S. (1996). *Precise Interprocedural Dataflow
   Analysis with Applications to Constant Propagation.* DOI:
   [10.1016/0304-3975(96)00072-2](https://doi.org/10.1016/0304-3975(96)00072-2).
