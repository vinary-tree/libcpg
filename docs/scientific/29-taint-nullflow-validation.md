# Scientific validation 29 — Taint and maybe-null summaries

This page states falsifiable claims for `analysis::taint`, identifies an
independent oracle for each claim, and separates implementation correctness
from advisory security interpretation. Passing these experiments supports only
the represented typed CPG and catalog semantics.

## 1. Hypotheses and discriminating experiments

| Hypothesis | Discriminating experiment | Oracle |
|---|---|---|
| Direct taint reaches a matching sink | environment source to command argument | exact endpoints, classes, CWE-78, and typed witness |
| Interprocedural laundering is summarized | source passed to helper parameter that reaches sink | exact cross-function source/sink and parameter edge |
| Untainted constants remain clean | literal argument to sink | empty finding set |
| Sanitizers are transfer barriers | source through sanitizer result to sink | empty finding set |
| Opaque models have distinct semantics | passthrough, sink, sanitizer, source fixtures | exact finding presence/class/confidence |
| Recursion converges | self-recursive and mutually recursive call SCCs | complete fixed point and exact parameter-to-sink relation |
| Return summaries compose | parameter-return and source-return helpers | exact mask/site and caller flow |
| Catalogs are replaceable | same CPG under empty and custom catalogs | zero versus one exact finding, unchanged graph shape |
| Caps remain observable | zero work, fact pressure, bounded typed call paths, and 65 parameters | `capped`, incomplete report, marked retained flow, truncation bit |
| Null origins reach dereference | literal, uninitialized variable, nullable return | exact origin kind, endpoint, and witness |
| Safe null counterexamples remain clean | initialized non-null variable, nonnullable return | empty finding set |
| Serialization preserves evidence | serde JSON round trip | structural equality |

## 2. Ported pgmcp scenarios

Twelve named example tests preserve the original defensive scenarios while
generalizing closed categories to open classes:

1. direct source-to-sink;
2. laundering through a represented helper;
3. constant argument not flagged;
4. sanitizer killing;
5. external passthrough;
6. external sink;
7. external sanitizer;
8. self recursion;
9. mutual recursion;
10. parameter-to-return summary;
11. source-returning helper; and
12. external source with a project-defined class.

Each fixture is hand-built from public or internal typed CPG constructors. No
test supplies source text to the analysis.

## 3. Independent reachability oracle

Generated acyclic graphs contain one planted source, zero or more value nodes,
and one sink argument or dereference. An independent breadth-first search (BFS)
over the generated edge list computes whether the final node is reachable from
the source. The production analysis is then required to satisfy

```math
\operatorname{hasFinding}(G)
=
\operatorname{BFSReachable}(G,s,t).
```

The BFS implementation does not call the summary solver, token transfer,
catalog classifier, or witness code. Taint and null-flow each execute this
comparison for 256 generated cases.

## 4. Algebraic and determinism properties

Three `proptest!` blocks contain eight 256-case properties:

- catalog classification and external models repeat exactly for arbitrary
  paths;
- segment matching agrees with a direct boundary construction;
- taint summary join is associative, commutative, idempotent, and an upper
  bound under `subsumes`;
- taint findings equal the independent BFS oracle;
- reversing DFG edge insertion leaves the entire `TaintAnalysis` equal;
- null summary join satisfies the same lattice laws;
- null findings equal the independent BFS oracle; and
- reversing DFG edge insertion leaves the entire `NullAnalysis` equal.

The properties execute 2,048 generated cases at their configured default.
Any minimized counterexample is retained by proptest's normal regression-file
mechanism.

## 5. Witness oracle

Every generated positive finding checks each adjacent witness pair against the
admitted typed transfer relation: a forward DFG edge or AST value-composition
boundary. Interprocedural examples additionally require concrete `Parameter`
and `ReturnValue` edges where summaries cross functions. The first and last
nodes must equal the reported source and sink.

This validates that a witness is not a disconnected list selected solely by
classification. It does not claim path feasibility under unmodeled branch
conditions.

## 6. Public integration boundary

`tests/taint_integration.rs` imports only crate-root exports. Its three tests
exercise:

1. source and sink paths reconstructed from nested typed `MemberAccess` nodes,
   both report and vector APIs, CWE mapping, and graph-shape preservation;
2. an unknown-language CPG whose finding changes only when a custom catalog is
   supplied; and
3. both maybe-null public APIs plus explicit empty capped evidence.

The typed member-path fixture is a regression guard against accidental source
parsing: it contains no retained source buffer.

## 7. Benchmark oracle and shapes

`benches/ifds_taint.rs` constructs three call-graph families:

- **path:** acyclic wrapper chain ending at a sink;
- **ring:** one recursive SCC; and
- **clustered:** bounded recursive clusters connected by forward bridges.

For 32 and 128 worker functions, the benchmark first runs the full analysis
outside Criterion's timed closure and requires a complete exact source-to-sink
finding with the reported source as the first witness node. Only then is the
same analysis timed. This protects performance measurements from rewarding an
incorrect empty result.

## 8. Coverage matrix

| Dimension | Evidence |
|---|---|
| normal | direct flow, helper flow, return composition, three null origins |
| negative | constant, sanitizer, initialized variable, nonnullable return |
| boundary | same-class origins, open classes, 65 parameters, recursive SCCs |
| capped | zero shared work, local fact pressure, typed call-path depth, fixed-width mask |
| algebraic | taint and null product-lattice properties |
| deterministic | exact report equality under reversed edge insertion |
| oracle | two independent BFS campaigns |
| witness | every generated positive path checks typed adjacency |
| integration | all four public entry points and typed member paths |
| serialization | catalog, taint report, and null report round trips |
| performance | six oracle-gated path/ring/clustered benchmark inputs |

The feature-free focused suite contains 35 tests; enabling `serde` adds three
round-trip tests. The public integration target adds three black-box tests.

## 9. Limitations and future falsifiers

- Dynamic dispatch and opaque native behavior remain limited by current call
  resolution and explicit external models.
- Alias and heap precision await points-to analysis; a future implementation
  should add differential alias fixtures without weakening existing results.
- Catalog sanitizers are policy assumptions. Concrete validation of sanitizer
  semantics belongs to a language/application-specific consumer.
- Null-flow does not prove branch feasibility or ownership safety.
- CWE association classifies the configured sink; it does not establish a CWE
  instance or exploitability.
- Coverage measurement belongs to the campaign-wide coverage gate; test counts
  alone are not line or branch coverage claims.

## 10. Reproduction

```sh
cargo test --no-default-features --lib taint
cargo test --no-default-features --test taint_integration
cargo test --no-default-features --features serde --lib taint
cargo bench --bench ifds_taint -- --test
```

## References

1. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and
   Discovering Vulnerabilities with Code Property Graphs.* DOI:
   [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44).
2. Reps, T., Horwitz, S., Sagiv, M. (1995). *Precise Interprocedural Dataflow
   Analysis via Graph Reachability.* DOI:
   [10.1145/199448.199462](https://doi.org/10.1145/199448.199462).
3. Sagiv, M., Reps, T., Horwitz, S. (1996). *Precise Interprocedural Dataflow
   Analysis with Applications to Constant Propagation.* DOI:
   [10.1016/0304-3975(96)00072-2](https://doi.org/10.1016/0304-3975(96)00072-2).
