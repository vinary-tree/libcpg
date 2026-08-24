# Scientific validation 65 -- MeTTa rewriting analysis

## Claims under test

The implementation claims that normalized MeTTa rules yield deterministic
dependency pairs, existing iterative SCC decomposition, sound certificates for
the fixed polynomial reduction pair, concrete self-loop refutations, exact
left-linear critical overlaps, and proven conflicts only after exhaustive
finite joinability search. It also claims that every incomplete condition is
visible and term depth does not become native call depth.

These tests validate the represented first-order rewrite model. They do not
validate the complete Hyperon evaluator or prove that the available reduction
pair decides every terminating system.

## Example oracles

| Fixture | Independent expected result |
|---|---|
| `f(S(x)) -> g(x)` and `g(S(x)) -> f(x)` | one cyclic SCC; both pairs strict; all rules weak; `Terminates` |
| `loop(x) -> loop(x)` | repeated rule cycle and exact RHS call-site witness |
| growing unoriented mutual recursion | `Unknown`, never false refutation |
| unrelated variable-duplicating rule plus decreasing cycle | fixed interpretation cannot certify the global weak obligation; `Unknown` |
| `pick(x) -> A` and `pick(x) -> B` | proven non-joinable critical conflicts |
| one branch rewrites to the other's normal form | no conflict after BFS intersection |
| swapped shared variables | distinct branches remain distinct before and after successor rewriting |
| repeated LHS variable | one logical binder; termination remains available; confluence report is incomplete |
| unsupported language | no accidental rule analysis; typed issue |

## Property oracle

The generated rule-permutation campaign varies chain length and construction
priority. It parses a canonical and a permuted rule order independently and
requires equal semantic rule-name dependency pairs, complete reports, and
canonical SCC behavior. This targets source/construction ordering rather than
sorting one result twice.

The edge-kind property includes both RewriteDep subtypes and requires every
edge to inhabit at most one of the nine top-level classifier families. The ML
registry oracle enumerates every edge subtype and requires the contiguous
class vector `0..62`, with old classes `0..60` unchanged.

## Cap and malformed-input oracles

- zero rule capacity yields `RuleLimitExceeded`;
- zero aggregate term capacity yields `TermNodeLimitExceeded`;
- zero dependency capacity yields `DependencyPairLimitExceeded`;
- zero unification capacity yields `UnificationLimitExceeded`;
- zero critical-pair capacity yields `CriticalPairLimitExceeded`;
- zero joinability capacity yields `JoinabilityLimitExceeded` without a false
  conflict;
- dependency and critical-pair capacities are independent;
- non-left-linear input yields `NonLeftLinearRule`; and
- invalid/non-rule overlay endpoints fail before any mutation, while repeated
  successful materialization adds zero edges.

## Public integration and stack-safety oracles

`tests/rewriting_integration.rs` uses only crate-root APIs and a real
caller-owned MeTTa parser. It proves a decreasing mutual recursion, finds a
non-confluent parser-built overlap, materializes RewriteDep, and observes exact
statistics.

`tests/rewriting_stack_safety.rs` builds a 100,000-level term in a public CPG
and runs dependency extraction and termination on a 64 KiB worker stack. An
internal 64 KiB test independently unifies, alpha-compares, and fingerprints
100,000-level flat terms. Together they exercise parser-independent
construction, flat extraction, unification, comparison, hashing, SCC-facing
results, and destruction without source-depth recursion.

## Focused implementation evidence

Every compiler/test command runs with one Cargo build job inside a user
systemd scope with `MemoryMax=4G`, `MemorySwapMax=0`, and `TasksMax=256`.

| Gate | Evidence |
|---|---|
| focused rewrite unit/property suite | 20 examples/properties pass |
| public integration | parser-to-proof/conflict/overlay flow passes |
| public depth gate | 100,000 levels on 64 KiB passes |
| complete `--features metta` matrix | library, integration, stack, and doctest targets pass |
| strict Clippy | all MeTTa-enabled targets pass with warnings denied |

Repository-wide all-feature, documentation, DOI, reproducible-diagram, and
bug-gate evidence is recorded only after the terminal FT6 verification pass.

## Reproduction

```text
systemd-run --user --scope \
  -p MemoryMax=4G -p MemorySwapMax=0 -p TasksMax=256 \
  env CARGO_BUILD_JOBS=1 cargo test --features metta
```

Do not compile in `/tmp` on systems where it is memory-backed.

## References

1. Arts, T., Giesl, J. (2000). DOI:
   [10.1016/S0304-3975(99)00207-8](https://doi.org/10.1016/S0304-3975(99)00207-8).
2. Giesl, J. et al. (2017). DOI:
   [10.1007/s10817-016-9388-y](https://doi.org/10.1007/s10817-016-9388-y).
3. Newman, M. H. A. (1942). DOI:
   [10.2307/1968867](https://doi.org/10.2307/1968867).
4. Robinson, J. A. (1965). DOI:
   [10.1145/321250.321253](https://doi.org/10.1145/321250.321253).
