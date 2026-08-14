# Scientific validation 28 — Interprocedural effect summaries

This page states the falsifiable claims behind `analysis::effects` and the
experiments that test them. A passing test is evidence for the named claim, not
a claim about source semantics absent from the CPG.

## Hypotheses and oracles

| Hypothesis | Discriminating experiment | Oracle |
|---|---|---|
| A leaf with no observable write is pure | literal-return leaf | exact empty place sets and `Pure` |
| Typed access evidence is preserved | global read/write, parameter write, field read/write | exact `BTreeSet<Place>` |
| Parameter effects compose through calls | callee writes `Param(0)`, caller passes global | caller must write that global |
| Purity is transitive | caller reaches throwing or unresolved callee | exact `Impure` result |
| Constant returns compose | local checked binary expression and returned call | exact `ConstValue` |
| Disagreement is irreversible | two distinct return literals, then another join | flat-lattice overdefined state |
| Recursive summaries terminate | mutual call SCC | complete bounded fixed point |
| Caps remain visible | zero shared work and zero domain depth | exact cap evidence and incomplete result |
| Input values are trusted only after validation | wrong SCC projection | ordered issue, no evaluation, no mutation |
| Serialization preserves evidence | serde JSON round trip | structural equality |

## Generated experiments

One `proptest!` block contains three 256-case campaigns:

1. independently generated summary coordinates satisfy commutativity,
   associativity, idempotence, and upper-bound subsumption;
2. generated directed call graphs through six functions compare transitive
   purity with an independent depth-first reachability oracle, then reverse
   call insertion and require the entire `EffectAnalysis` to remain equal;
3. generated recursive rings verify that raising the SCC-iteration cap cannot
   invalidate a result already established complete.

The graph oracle does not invoke the production summary solver. It computes
the reachable function set directly and classifies a caller `Impure` exactly
when that set contains a planted direct impure operation.

## Public integration boundary

`tests/effects_integration.rs` imports only crate-root exports. One test checks
the completeness-preserving `analyze_effects` and map-only
`effect_summaries` paths on a constant leaf. The second constructs a callee
parameter write and proves public call-argument remapping reaches a caller
global.

## Coverage matrix

| Dimension | Evidence |
|---|---|
| normal | pure leaf, constants, transitive calls |
| boundary | no return, 65 parameters, recursive SCC |
| malformed | stale/wrong SCC projection, graph non-mutation |
| capped | shared scheduler work, local expression depth |
| algebraic | product lattice properties |
| deterministic | full output equality under reversed edge insertion |
| integration | two feature-free crate-root tests |
| serialization | one serde-gated round trip |

The focused feature-free module has 13 tests; the serde matrix has 14. Three
properties each execute 256 generated cases.

## Limitations and future falsifiers

- Alias and heap effects are only as precise as represented DFG/reference
  evidence. Future points-to analysis should add cross-check fixtures.
- The constant evaluator covers the scalar SCCP vocabulary; aggregate constants
  intentionally remain overdefined.
- A 64-bit parameter-return mask exposes, rather than hides, wider signatures.
- `effect_summaries` omits completeness metadata by design; verdict-making code
  must test `EffectAnalysis::is_complete()`.
- A future T3.3 taint instantiation should test that effect-derived external
  refinement never removes a flow when effect evidence is incomplete.

## Reproduction

```sh
cargo test --no-default-features analysis::effects::tests
cargo test --no-default-features --test effects_integration
cargo test --no-default-features --features serde analysis::effects::tests
```

## References

1. Reps, Horwitz, Sagiv (1995), DOI
   [10.1145/199448.199462](https://doi.org/10.1145/199448.199462).
2. Wegman, Zadeck (1991), DOI
   [10.1145/103135.103136](https://doi.org/10.1145/103135.103136).
