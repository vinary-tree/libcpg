# Scientific validation 39 — Advisory bug detectors

This ledger tests four candidate classes produced by three feature-free
detectors: resource leaks, uninitialized variable uses, uncaught exceptions,
and dead catches. It validates deterministic consequences of represented CPG
evidence, not runtime path feasibility or exploitability.

## 1. Hypotheses

| ID | Hypothesis | Falsifying observation |
|---|---|---|
| H1 | release on every semantic exit produces no leak | a non-accepting terminal is emitted or the report is incomplete |
| H2 | one unreleased branch produces its exact object/exit/state witness | the branch is missed, merged with another exit, or loses allocation identity |
| H3 | expression-evaluation side sinks are not function exits | a recorded semantic exit coexists with a leak at an unrelated structural sink |
| H4 | no accepting state prevents leak semantics | the report claims completeness or guesses a release state |
| H5 | a binder-aware executable use is uninitialized exactly when its in-scope reaching-definition set is empty | a use disagrees with the independent set oracle |
| H6 | declaration and unbound callee syntax is not an executable variable read | a function/type/method spelling is reported without a variable binding |
| H7 | a definition from another function remains out-of-scope evidence | it initializes the local use or disappears from the finding |
| H8 | caught exception types do not escape their function | a matching direct or caller handler leaves the type in the summary |
| H9 | call summaries propagate escaping types through recursive-safe SCC scheduling | a unique callee type is lost or call-chain order changes the answer |
| H10 | nearest lexical first-match dispatch honors catch-all and represented subtype relations | the wrong handler is reached or a matching handler is called dead |
| H11 | root escaping types have canonical complete call/throw witnesses | an uncaught origin is missing, noncanonical, or ends before a throw |
| H12 | every applicable resource boundary and malformed evidence prevents a clean negative claim | a capped, ambiguous, unknown, malformed-catch, or missing-witness run is complete |
| H13 | reports are deterministic, read-only, and serializable | relation insertion order changes values, graph counts change, or serde round-trip differs |
| H14 | source/graph depth does not consume proportional native stack | depth 100,000 overflows a 256 KiB worker stack |

## 2. Independent leak oracle

The generated leak fixture has one allocation and two represented semantic
exits. An independent Boolean selects whether the second branch executes its
release event; another Boolean reverses AST/CFG/DFG relation insertion. The
oracle predicts one leak at that branch exactly when release is absent.

The property requires both insertion orders to return equal
`ResourceLeakAnalysis` values and the predicted object/exit/state tuple. It
does not call the production terminal projection.

## 3. Independent uninitialized-use oracle

For generated Boolean vectors, each position creates one binder-aware use and
optionally an in-scope definition. The independent oracle selects precisely the
false positions. A second input reverses relation insertion.

The property compares the exact ordered use-site set, asserts that initialized
positions are absent, and verifies insertion-order equality. Signature and
callee exclusions are separate examples so the oracle's universe contains only
executable uses.

## 4. Independent exception-chain oracle

Across 256 generated cases, the fixture contains a call chain with one terminal
throw. Independent inputs choose the thrown nominal type, whether a caller has
a typed handler, whether it catches a supertype, and relation insertion order.
The oracle performs a direct finite membership/subtype decision and predicts:

1. whether the entry summary contains the type;
2. whether the handler appears in `reached_catches`;
3. whether one canonical uncaught witness spans entry, call sites, callee
   functions, and the direct throw; and
4. whether the unused alternate handler is dead.

The oracle never calls production handler dispatch, summary transfer, or
witness reconstruction.

## 5. Example-based matrix

| Family | Examples |
|---|---|
| leak semantics | balanced exits, one leaking branch, semantic-exit versus side-sink regression, no accepting state |
| leak qualification | inherited typestate work/fact/function/callee caps, deterministic relation insertion, serde |
| variable use | use before definition, reaching definition, out-of-scope definition, unbound callee, bound callback, unbound signature |
| direct exceptions | typed uncaught throw, matching/nonmatching handler, catch-all unknown, iterative subtype closure |
| interprocedural exceptions | caller catches callee, uncaught multi-function witness, nested outer-handler dispatch |
| exception qualification | function/fact/work/SCC caps, ambiguous call, missing `Catch` edge, unknown type, serde |
| public API | all four candidate classes together without graph mutation |
| frontends | parsed Rust leak/use; Java `build` and `build_from_tree` typed exception equivalence and CFG idempotence |
| stack safety | all public detectors across 100,000 AST levels on 256 KiB |

## 6. Metamorphic relations

The suite checks properties between equivalent executions:

- reversing admissible relation insertion cannot change ordered reports;
- running analysis cannot change CPG node or edge counts;
- repeating Java CFG extraction cannot duplicate typed handler-entry edges;
- `build` and `build_from_tree` over the same Java source/tree produce equal
  throw/catch topology and detector findings;
- serde encode/decode preserves the complete report value;
- a cap event may qualify positive findings but must make `is_complete()` false.

## 7. Stack-safety experiment

The public integration gate constructs a function with 100,000 nested AST
blocks and representative detector evidence. It runs all detector report APIs
on a worker configured with a 256 KiB native stack. Success requires expected
findings, no panic or overflow, and unchanged graph counts.

This exercises scope enumeration, type/reference walks, handler ancestry,
summary scheduling, terminal witness projection, and teardown through the
crate-root API. It supports native-stack use independent of represented depth
for these paths; it does not claim constant heap memory.

## 8. Release matrix

| Configuration | Executed evidence |
|---|---|
| `--no-default-features` | 21 detector unit/property tests and two public integration tests |
| all features without serde-specific filtering | all feature-free evidence plus Rust/Java parser integrations |
| `serde` | three exact report round trips, for 24 detector tests total |
| all-feature integration | four public tests: aggregate, Rust, Java dual-mode, and 100,000-depth |
| CFG regression | 37 extractor examples/properties, including typed catch-entry differential evidence |
| strict Clippy | library and tests under no-default and all-feature configurations with warnings denied |
| rustdoc | all-feature public documentation with warnings denied |

## 9. Interpretation limits

Passing this suite supports conformance to the documented finite represented-
evidence semantics. It cannot establish runtime path feasibility, complete
dynamic dispatch, external-library behavior, whole-program ownership or alias
precision, language initialization beyond normalized bindings, exception
object identity, or vulnerability impact.

## References

1. Shadab, N., Gharat, P., Ernst, M. D., Kellogg, M., Lahiri, S. K., Lal, A.,
   Sridharan, M. (2025). *Lightweight and Modular Resource Leak Checking
   (Extended Version).* DOI:
   [10.1007/s10009-025-00804-2](https://doi.org/10.1007/s10009-025-00804-2).
2. Reps, T., Horwitz, S., Sagiv, S. (1995). *Precise Interprocedural Dataflow
   Analysis via Graph Reachability.* DOI:
   [10.1145/199448.199462](https://doi.org/10.1145/199448.199462).
3. Sinha, S., Harrold, M. J. (2000). *Analysis and Testing of Programs with
   Exception Handling Constructs.* DOI:
   [10.1109/32.877846](https://doi.org/10.1109/32.877846).
