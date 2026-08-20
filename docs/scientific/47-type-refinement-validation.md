# Scientific validation 47 — Virtual-call type refinement

This dossier states which T8.1 claims are tested, the oracles used, and which
runtime-dispatch claims remain outside the evidence.

## 1. Claim ledger

| Claim | Evidence | Strength |
|---|---|---|
| CHA includes represented concrete base/subtype overrides | three-level exact hierarchy oracle | example-tested |
| RTA prunes unallocated compatible types | `Base`/`Mid`/`Leaf` allocation oracle | example-tested |
| VTA prunes types not assigned to the receiver | declaration-initializer and `DefUse` assignment oracles | example-tested |
| $`VTA\subseteq RTA\subseteq CHA`$ | 128-case mode/insertion campaign | property-tested |
| Edge insertion order cannot change a record | forward/reverse hierarchy construction over 128 cases | property-tested |
| Inherited/default methods survive pruning | subclass-without-override and `Implements` fixtures | example/integration-tested |
| Ambiguous nominal names add no invented evidence | duplicate `Base` fixture | regression-tested |
| Empty RTA target sets are total | zero-allocation regression that exposed eager singleton indexing | regression-tested |
| Malformed hierarchy cycles terminate deterministically | explicit cycle, repeated-result oracle | robustness-tested |
| Constructor types survive frontend normalization | JavaScript, TypeScript, and Java parser tests | parser integration-tested |
| Allocation metadata is backward compatible | full-field and missing-field serde tests | serialization-tested |
| Public C1 records refine without graph mutation | `resolve_calls` to all three public modes with node/edge snapshot | integration-tested |
| Hierarchy depth does not consume native call depth | 100,000 levels on a 256-KiB native stack | adversarial stack-safety gate |

## 2. Independent hierarchy oracle

The core fixture defines `Base <- Mid <- Leaf`, where the arrow points from
subtype to parent in the CPG. Every type overrides `run`. The declared receiver
type is `Base`; `Mid` and `Leaf` are allocated; only `Leaf` initializes the
receiver. Therefore the exact represented oracle is

```math
C_{CHA}=\{Base::run,Mid::run,Leaf::run\},
```

```math
C_{RTA}=\{Mid::run,Leaf::run\},
```

```math
C_{VTA}=\{Leaf::run\}.
```

The VTA singleton must set `target = Leaf::run`; the other records must not
invent a unique target. The property campaign rebuilds the same logical graph
with reversed hierarchy insertion and selects each mode across 128 generated
cases. It compares complete records and checks both subset inclusions.

## 3. Dispatch oracle

The inherited-dispatch test removes `Mid::run` and `Leaf::run` while retaining
their allocations. Correct RTA still returns `Base::run`; filtering methods by
allocated declaring class would incorrectly return empty. A second public
fixture gives concrete `Worker` an `Implements` edge to abstract `Runner`, whose
default `run` method must dispatch for an allocated `Worker`.

These tests distinguish runtime-type filtering from method-owner filtering.

## 4. VTA evidence oracle

One fixture finds an allocation beneath the receiver's declaration. Another
removes that initializer and adds an assignment node as an incoming `DefUse`
source; the allocation is on the assignment's right-hand AST subtree. VTA must
select exactly that type. Declared `TypeInfo` alone is insufficient for VTA but
continues to seed CHA compatibility.

The explicit `TypeOf` test removes a variable's declared `TypeInfo`, connects
the definition to the represented base nominal node, and requires the same CHA
population. This separates static receiver evidence from assignment evidence.

## 5. Boundary matrix

| Boundary | Expected behavior |
|---|---|
| non-method call | C1 record copied unchanged |
| method syntax without `MemberAccess` receiver | copied unchanged |
| absent receiver type | copied unchanged |
| ambiguous nominal type name | copied unchanged |
| abstract/trait static type | compatible represented concrete descendants admitted |
| no represented allocations under RTA | typed confidence, empty candidates, no target |
| no receiver-reaching allocations under VTA | typed confidence, empty candidates, no target |
| one candidate | exact candidate count and singleton `target` |
| many candidates | sorted exact set and no unique target |
| hierarchy endpoint not nominal | edge ignored |
| nested function under nominal type | not indexed as a method |
| hierarchy or AST cycle | termination through visited/queued state |

## 6. Stack-safety validation

The public gate constructs 100,000 concrete classes in a single-inheritance
chain, one inherited base method, a typed receiver, and one CHA record. It runs
inside a thread with a 256-KiB native stack. Completion with the inherited
singleton and unchanged graph counts establishes that index construction,
subtype closure, and dispatch propagation do not recurse with hierarchy depth.

The gate does not claim constant memory. Heap indices and worklists grow with
represented graph size, as required to return exact candidate sets.

## 7. Limits of validation

The tests validate libcpg's documented finite closed-world equations. They do
not validate the completeness of a caller's hierarchy, allocations, types, or
DFG; JVM/C++/JavaScript runtime loading; reflection; proxy dispatch;
language-specific multiple-inheritance linearization; or equivalence to a full
points-to analysis. No benchmark is claimed: the adversarial test is a
correctness/stack gate, not a representative performance study.

## 8. Reproduction

```console
cargo test --no-default-features analysis::resolve_types
cargo test --no-default-features --test devirtualization_integration
cargo test --no-default-features --test devirtualization_stack_safety
cargo test --features serde devirtualization_modes_round_trip_through_serde
cargo test --features lang-javascript javascript_new_expression_retains_allocation_type
cargo test --features lang-typescript typescript_new_expression_retains_allocation_type
cargo test --features lang-java java_object_creation_retains_allocation_type
cargo clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS=-Dwarnings cargo doc --all-features --no-deps
```

## References

1. Dean, J., Grove, D., Chambers, C. (1995). *Optimization of Object-Oriented
   Programs Using Static Class Hierarchy Analysis.* DOI:
   [10.1007/3-540-49538-X_5](https://doi.org/10.1007/3-540-49538-X_5).
2. Bacon, D. F., Sweeney, P. F. (1996). *Fast Static Analysis of C++ Virtual
   Function Calls.* DOI:
   [10.1145/236337.236371](https://doi.org/10.1145/236337.236371).
3. Sundaresan, V., et al. (2000). *Practical Virtual Method Call Resolution for
   Java.* DOI:
   [10.1145/353171.353189](https://doi.org/10.1145/353171.353189).
