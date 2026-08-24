# Scientific validation 60 -- deterministic Datalog export

This record tests whether libcpg can expose complete represented graph
relations without adding a query-engine dependency or sacrificing determinism,
overlay multiplicity, graph immutability, or stack safety.

## Hypotheses

| ID | Falsifiable claim | Failure evidence |
|---|---|---|
| H1 | Every represented edge is recoverable exactly from `cpg_edge`. | Missing/extra edge, changed kind/label/id, or lost parallel multiplicity. |
| H2 | Equal stable graphs yield equal typed facts and bytes across insertion permutations. | Any relation or `.facts` byte differs. |
| H3 | Relation-specific views classify every public overlay kind correctly. | A tuple is absent or appears in the wrong relation. |
| H4 | The shipped Doop-lite equations yield the hand-computed points-to closure. | A missing, extra, or wrongly propagated pair. |
| H5 | Export and rendering have constant native call depth. | A 100,001-node chain fails on a 256 KiB stack. |
| H6 | Parser-built use is deterministic and immutable. | Repeated output differs or graph counts change. |

## Method and results

The focused unit suite contains six examples/properties:

- all seventeen relation families are exercised and the exact `CpgEdge`
  vector is reconstructed;
- a rejected edge whose target is outside the graph never enters either the
  lossless master relation or a relation-specific fact vector;
- RFC 4180 symbol rendering quotes every symbol, doubles internal quotes, and
  preserves control separators inside the field;
- an independent queue-based oracle derives exactly three expected
  `VarPointsTo` pairs from two allocations and three assignments;
- 128 generated stable-id graphs compare forward and reverse node/edge
  insertion, typed bundle equality, and byte-identical file buffers;
- a 100,001-node AST path is exported and rendered on a 256 KiB stack.

All six passed. The parser-built Rust integration parsed a conditional
function, exported it twice, observed non-empty AST/CFG relations, preserved
node/edge counts, and obtained equal bundles. Strict all-target/all-feature
Clippy and warning-denied all-feature rustdoc passed.

The first compilation exposed a test-fixture error: the private capacity
constructor requires an explicit `Language`. The second exposed a
missing public `CpgBuilder` trait import in the integration test.
Both were corrected at the call sites; no production behavior or compatibility
workaround changed.

## Reproduction

```text
cargo test --all-features --lib analysis::datalog::tests
cargo test --all-features --test datalog_integration
cargo clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps --all-features
```

Potentially memory-intensive commands ran with `MemoryMax=4G`,
`MemorySwapMax=0`, `TasksMax=512`, and four Cargo build
jobs. Captures stayed under disk-backed `target/verification`.

Soufflé was not installed on the validation host. The shipped rules explicitly
select RFC 4180 tab-separated input and were checked as committed syntax
assets; their Doop-lite semantics were tested by an independent worklist
oracle. This result does not claim an external-engine integration run.

## Interpretation boundary

H1--H6 are supported. The result proves exact export of represented libcpg
facts, not completeness of the frontend model, soundness of an arbitrary
ruleset, or bounded resource use by an external Datalog engine.

## References

1. Bravenboer, M., Smaragdakis, Y. (2009). *Strictly Declarative Specification
   of Sophisticated Points-to Analyses.* OOPSLA. DOI:
   [10.1145/1640089.1640108](https://doi.org/10.1145/1640089.1640108).
2. Jordan, H., Scholz, B., Subotić, P. (2016). *Soufflé: On Synthesis of Program
   Analyzers.* CC. DOI:
   [10.1007/978-3-319-41540-6_23](https://doi.org/10.1007/978-3-319-41540-6_23).
