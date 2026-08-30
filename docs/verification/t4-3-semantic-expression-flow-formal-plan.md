# T4.3 semantic expression-flow formal plan

## Purpose and decision boundary

This plan governs pgmcp task `dovetail-x-t4-3`. It specifies how libcpg may
combine proof-producing expression equivalence with control-flow facts before
production code is written. The analysis produces evidence and optimization
*candidates*; it does not rewrite source code, mutate the Code Property Graph
(CPG), or claim whole-program behavioral equivalence.

Three ideas must remain separate:

- An *e-class* is a set of typed terms connected by replayable equality
  evidence.
- *Availability* says that a semantic expression class has already been
  computed on every control-flow path to a program point and has not since
  been killed.
- *Anticipability*, also called very-busy expression analysis, says that every
  path from a program point evaluates the class before a kill or exit.

Equality saturation adds equal terms without selecting a destructive rewrite,
as introduced for optimization by Tate et al.
([DOI 10.1145/1480881.1480915](https://doi.org/10.1145/1480881.1480915)).
Congruence closure supplies equality reasoning for applications with equal
operators and operands
([DOI 10.1145/322186.322198](https://doi.org/10.1145/322186.322198)).
Neither result contains control-flow path information. The path facts remain
the responsibility of libcpg's monotone dataflow framework, following the
fixed-point foundation established by Kildall
([DOI 10.1145/512927.512945](https://doi.org/10.1145/512927.512945)).

## Mathematical contract

### Semantic classes and resolved dependencies

Let $`C`$ be a semantic expression class and $`n`$ a CPG node. A class is
generated at $`n`$ only when a complete, live, pure `ExpressionIr` root at
$`n`$ has an exact replayable proof into $`C`$. A *resolved dependency* is a
`BindingIdentity`, consisting of a definition node and scope, that occurs in
the flat expression representation. A declaration, parameter, or assignment
kills $`C`$ exactly when its resolved target is among $`C`$'s dependencies.
Textual identifier equality is not sufficient.

For a path $`\pi`$ from the function entry to program point $`p`$, define
$`G(C,\pi,p)`$ to mean that $`\pi`$ contains a generation of $`C`$ after its
last matching kill and before $`p`$. Must-availability is:

```math
\operatorname{Avail}(C,p)
  \iff
  \bigwedge_{\pi\in\operatorname{Paths}(\operatorname{entry},p)}
  G(C,\pi,p).
```

For a path $`\rho`$ from $`p`$ to an exit, define $`U(C,p,\rho)`$ to mean
that $`\rho`$ evaluates $`C`$ before a matching kill or exit. Must-
anticipability is:

```math
\operatorname{Busy}(C,p)
  \iff
  \bigwedge_{\rho\in\operatorname{Paths}(p,\operatorname{exit})}
  U(C,p,\rho).
```

The implementation computes these meet-over-all-path facts with deterministic
iterative work queues over the finite block control-flow graph. It never
enumerates paths in production. The executable model uses small explicit paths
only as an independent oracle for the fixed-point implementation.

### Common-subexpression-elimination candidates

Common-subexpression elimination (CSE) may report producer $`x`$ and later
use $`y`$ only when all of the following hold:

```math
\begin{aligned}
\operatorname{CSE}(x,y) \Longrightarrow {} &
  \operatorname{ExactProof}(x,y) \\
  &\land \operatorname{Avail}([x],y) \\
  &\land \operatorname{Dom}(x,y) \\
  &\land \operatorname{NoKill}(x,y) \\
  &\land \operatorname{CompleteDataflow}.
\end{aligned}
```

Here $`[x]`$ is the semantic class containing $`x`$, and
$`\operatorname{Dom}(x,y)`$ means that the single selected producer dominates
the use. This extra dominance premise is essential: in a diamond, both
branches can generate the same class, making it available at the join, while
neither branch occurrence dominates the join. Equality plus availability
therefore does not identify a reusable producer.

### Partial-redundancy-elimination candidates

Partial redundancy elimination (PRE) identifies an expression redundant on
some, but not necessarily all, incoming paths. Morel and Renvoise introduced
the global formulation
([DOI 10.1145/359060.359069](https://doi.org/10.1145/359060.359069)).
This task reports a conservative placement candidate $`p`$; it does not move
code. The required implication is:

```math
\begin{aligned}
\operatorname{PRE}(C,p) \Longrightarrow {} &
  \operatorname{ExactProof}(C) \\
  &\land \operatorname{Busy}(C,p) \\
  &\land \operatorname{DomAll}(p,\operatorname{Occurrences}(C)) \\
  &\land |\operatorname{Occurrences}(C)|\ge 2 \\
  &\land \operatorname{CompleteDataflow}.
\end{aligned}
```

Calls, exceptions, unknown side effects, unsupported profiles, unresolved
bindings, and incomplete flow facts prevent a candidate. They never become an
assumption that motion is safe.

### Four-way semantic diff

Semantic diff is an additive report with four statuses:

| Status | Exact meaning |
|---|---|
| `ProvenEquivalent` | Exact or qualified alpha equality has a complete, independently replayed proof. |
| `ProvenDifferent` | Both closed terms are inside the bounded evaluator domain and iterative evaluation yields unequal typed values. |
| `Limited` | No decisive witness exists and at least one relevant saturation, provenance, dataflow, cancellation, or resource boundary is incomplete. |
| `Unknown` | The checked evidence is complete, but neither equality nor inequality was proved. |

The decision order is intentional:

```text
classify(evidence):
    if replay_exact_or_qualified_alpha_proof(evidence):
        return ProvenEquivalent
    if closed_values_are_exact_and_unequal(evidence):
        return ProvenDifferent
    if any_relevant_dimension_is_limited(evidence):
        return Limited
    return Unknown
```

A positive proof remains useful if a later saturation frontier reaches a cap.
Likewise, an exact unequal closed evaluation remains useful when no equality
proof exists. If a replayed equality proof and an unequal evaluation ever
coexist, the report is internally contradictory and must fail closed rather
than choose either result.

## Executable verification

| Artifact | Verification role | Required result |
|---|---|---|
| `formal/tla/SemanticExpressionFlow.tla` | Enumerates admission, proof modes, completeness dimensions, must-flow premises, evaluator outcomes, and reporting precedence. | TLC explores all 391,168 reachable states with every invariant true. |
| `formal/rocq/SemanticExpressionFlow.v` | Proves forward transfer and backward anticipation laws, path-universal must facts, PRE/CSE prerequisites, diff classification soundness, evidence consistency, and CPG immutability. | Twenty printed theorem groups are closed under the global context without axioms or admissions. |
| `formal/z3/semantic_expression_flow.smt2` | Refutes forbidden candidate, classification, binding-context, ordering, contradiction, and mutation states. | All twenty-four queries are `unsat`; none is `sat` or `unknown`. |

Run all three with:

```bash
./scripts/verify-semantic-expression-flow-formal.sh
```

Campaign runs execute this command under `systemd-run` with a fixed memory cap,
one TLC worker, no swap, and captured output below the disk-backed repository's
`target/verification/` directory. The proof driver's own temporary files also
live below `target/` and are deleted on exit.

The bounded TLA+ state space does not prove arbitrary graph sizes. Rocq proves
the logical implications for arbitrary finite action lists, while Rust
property tests compare iterative fixed points against exhaustive small-graph
path oracles. Small-native-stack tests validate the implementation refinement
at 100,000 nodes.

## Verified invariants and required tests

| ID | Invariant extracted from the models | Implementation and test witness |
|---|---|---|
| SF-01 | Exact e-class equality never substitutes for availability or anticipability. | Equal terms on disjoint branches without a dominating producer produce no CSE candidate. |
| SF-02 | Availability is a forward must fact over every predecessor path. | Diamonds, back edges, unreachable blocks, and predecessor-order permutations match exhaustive path oracles. |
| SF-03 | Anticipability is a backward must fact over every successor path. | Branches containing exits, kills, calls, exceptions, and later uses match the independent oracle. |
| SF-04 | Kills compare resolved `BindingIdentity` values, not identifier spelling. | Shadowing and same-name/different-scope examples never cross-kill; true redefinitions always kill. |
| SF-05 | CSE requires exact proof, complete flow, must availability, one dominating producer, and no intervening kill. | Delete each premise independently and assert rejection; generate all Boolean premise combinations. |
| SF-06 | PRE requires exact proof, complete flow, at least two occurrences, must anticipability, and a dominating placement. | Loops, diamonds, critical branch shapes, and every missing premise are covered. |
| SF-07 | Impure, effectful, unresolved, unsupported, malformed, or incomplete expressions never become PRE/CSE candidates. | Calls, assignments, exception edges, unknown operators, deserialized IR, and incomplete profiles fail closed. |
| SF-08 | `ProvenEquivalent` requires independent proof replay. | Corrupt every proof-tape field and ensure rejection; e-class membership alone is insufficient. |
| SF-09 | `ProvenDifferent` requires two exact closed evaluator results with unequal typed values. | Equal values, open terms, overflow, type errors, unsupported operations, and evaluator caps are never reported different. |
| SF-10 | `Limited` and `Unknown` are distinct, and neither is equality or inequality. | Exhaust every completion bit and assert the four-way truth table. |
| SF-11 | Decisive evidence takes precedence over unrelated later limits. | Add each cap/limit after a valid proof or unequal evaluation and preserve the decisive result. |
| SF-12 | A proof/evaluator contradiction fails closed. | Inject a contradictory test fixture and require a typed internal-consistency error. |
| SF-13 | Public pairs and candidates use stable canonical `NodeId` order; engine-local IDs never escape. | Shuffle blocks, nodes, rules, candidates, and hash insertion order; serialized reports remain byte-identical. |
| SF-14 | The CPG and legacy structural-diff report are observationally immutable. | Snapshot before/after every complete and limited path; predecessor diff fixtures remain byte-identical. |
| SF-15 | Composition consumes heap work tapes, not native recursion. | Build, analyze, compare, format, clone, serialize, deserialize, and drop 100,000-node fixtures on a 256 KiB thread. |
| SF-16 | Bounded results preserve exact completion reasons. | Exercise work, node, match, iteration, proof, evaluator, cancellation, and malformed-input bounds separately. |
| SF-17 | The semantic report is additive and advisory. | API tests prove no optimizer mutation path exists and old `ChangeKind` behavior is unchanged. |
| SF-18 | MeTTaIL remains outside the implementation boundary. | Dependency and dirty-worktree audits show no MeTTaIL write, worktree, build, format, branch, or commit. |

## Independent oracle strategy

The tests must not merely restate the production transfer functions.

1. Generate every directed graph with up to a bounded number of blocks, then
   enumerate simple paths with an explicit heap work list.
2. Interpret `Generate`, `Kill`, and `Preserve` directly on each path.
3. Meet the concrete path results with logical conjunction.
4. Compare those facts with the production forward and backward fixed points.
5. Independently enumerate all PRE/CSE premise combinations and compare the
   public candidate report with SF-05 and SF-06.
6. Evaluate bounded closed expression trees with a second typed interpreter,
   then compare all four semantic-diff outcomes.

The production algorithm and oracle must share only public input types and
result types. They must not share transfer helpers, work-queue code, dominance
helpers, evaluator implementation, or decision-classification functions.

## Stack-safe implementation shape

The required production state machine is:

```text
prepare:
    lower complete pure roots to flat ExpressionIr tapes
    replay equivalence proofs
    assign deterministic semantic classes by minimum stable NodeId
    collect exact BindingIdentity dependencies

solve-forward:
    seed a deterministic block work queue
    intersect predecessor facts
    apply resolved kills, then generations
    enqueue changed successors until the fixed point

solve-backward:
    seed the reverse deterministic block work queue
    intersect successor facts
    apply generations and resolved kills in reverse program order
    enqueue changed predecessors until the fixed point

report:
    join proof and flow evidence without graph mutation
    select CSE and PRE candidates only when every premise holds
    classify semantic diffs with explicit completion evidence
```

Every collection is finite and bounded by caller-visible limits. Work queues,
IR tapes, proof tapes, evaluator continuations, serialization, formatting, and
destruction remain iterative. A graph edge or expression depth must never map
to native call-stack depth.

## Proof-construction ledger

The following rejected attempts are retained so later work does not repeat
them:

1. The first Rocq classification proof asked `discriminate` to reduce an
   undecided nested Boolean branch. The corrected proof explicitly destructs
   every decision Boolean and closes only constructor contradictions.
2. The first captured verification invocation piped through `tee` without
   enabling shell `pipefail`, so the logger's success hid Rocq's failure. All
   subsequent captured commands propagate the left-hand verifier status.
3. The first Z3 canonical-pair query asserted both orderings inside the query,
   proving only arithmetic contradiction. The accepted model asserts
   canonical order as a global consequence of a CSE candidate, then asks Z3
   whether a reversed candidate can satisfy that contract.

These were proof-harness defects, not counterexamples to the semantic
contract. The corrected suite completes all three verification systems before
production implementation begins.

## Scope and repository boundary

T4.3 changes only the dedicated libcpg feature worktree and consumes the
standalone Replete crate through its pinned dependency. Replete remains an
independent top-level crate, not a libcpg submodule or copied implementation.

MeTTaIL is entirely read-only and outside this campaign's implementation
scope. No MeTTaIL worktree, manifest, source, test, documentation, generated
language, runtime, branch, or commit may be changed. Separately authorized
top-level externalizations such as Prattail, Replete, and Rigail do not grant
permission to refactor their MeTTaIL-internal predecessors.
