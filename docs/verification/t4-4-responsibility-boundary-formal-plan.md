# T4.4 responsibility-boundary formal plan

## Purpose and ownership

This plan governs pgmcp task `dovetail-x-t4-4`. It specifies the boundary
among three different reasoning systems before production changes are written.
A **Code Property Graph (CPG)** is libcpg's immutable typed graph of program
syntax and semantic overlays. **Datalog** derives relational consequences from
an extensional database (EDB) by a least fixed point. An **e-graph** stores
terms in congruence-closed equivalence classes, called **e-classes**. Replete
owns generic e-graph mechanics; libcpg owns program semantics, dataflow, stable
source identity, orchestration, and evidence qualification; a caller-owned
Datalog engine owns rule parsing and relational evaluation.

The proposed bridge is an owned, canonical EDB fact bundle copied from an
already checked libcpg semantic report. It is not a shared engine. It cannot
mutate the CPG or report, expose an engine-local e-class identifier, turn a
Datalog tuple into an equality proof, or conceal incomplete equality or
relational evaluation.

MeTTaIL is outside this task's write and build scope. Replete remains an
independent top-level crate. No formal or production artifact created here
requires a MeTTaIL edit, worktree, branch, format, build, or commit.

## Mathematical contract

Let $`G`$ be the immutable CPG, $`R`$ an immutable semantic report, $`P(x,y)`$
a replayed exact proof between stable source nodes $`x`$ and $`y`$, and
$`F_R`$ the owned EDB facts exported from $`R`$. Positive export requires:

```math
(x,y)\in F_R\Longrightarrow
P(x,y)\land\operatorname{Stable}(x,y)\land
\operatorname{Canonical}(x,y)\land\operatorname{ArchitectureSafe}.
```

A proof-backed positive remains valid even if later equality search or a
caller-owned relational evaluation is limited. A *negative* conclusion,
including complete no-match, requires both fixed points:

```math
\operatorname{ConclusiveAbsence}\Longrightarrow
\operatorname{EqualityComplete}\land
\operatorname{DatalogComplete}\land
\operatorname{ArchitectureSafe}.
```

The bridge reports combined completion as complete exactly when both
dimensions are complete:

```math
\operatorname{CombinedComplete}\iff
\operatorname{EqualityComplete}\land\operatorname{DatalogComplete}.
```

The one-way trust relationship is intentional. A proof can produce an EDB
tuple, but a tuple cannot produce proof authority:

```math
P(x,y)\Longrightarrow\operatorname{EDBEquivalent}(x,y),
\qquad
\operatorname{EDBEquivalent}(x,y)\not\Longrightarrow P(x,y).
```

This is consistent with Datalog's least-fixed-point semantics and congruence
closure: both systems add knowledge monotonically, but their judgments have
different meanings. Nelson and Oppen formalize congruence closure
([DOI 10.1145/322186.322198](https://doi.org/10.1145/322186.322198)); Jordan,
Scholz, and Subotić describe Soufflé's Datalog execution model
([DOI 10.1007/978-3-319-41540-6_23](https://doi.org/10.1007/978-3-319-41540-6_23)).

## Dependency contract

The permitted source dependency direction is:

```text
caller-owned Datalog program
    consumes owned canonical fact files
libcpg semantic bridge
    consumes immutable libcpg semantic reports
libcpg expression adapter
    consumes Replete through its public generic API
Replete
    depends on neither libcpg nor a Datalog runtime
```

The base `analysis::datalog` module remains independent of Replete and semantic
proof internals. The bridge may depend on public report types, not on Replete
IDs, mutable e-graphs, proof constructors, or Datalog runtime APIs. libcpg does
not gain an `egg`, `egglog`, or Soufflé runtime dependency. Any future egglog
evaluation is a separate experiment and cannot silently replace either engine.

## Executable verification

| Artifact | Verification role | Required result |
|---|---|---|
| `formal/tla/ResponsibilityBoundary.tla` | Exhaustively varies proof/completion/identity and every forbidden architecture request, then checks publication, absence, completion, ownership, and immutability. | TLC generates and checks all 2,048 distinct reachable states at depth 2 with every invariant true. |
| `formal/rocq/ResponsibilityBoundary.v` | Proves positive/negative evidence implications, completion honesty, rejection of each architecture violation, canonical source pairs, tuple non-authority, and immutable export. | All sixteen printed theorem groups close under the global context with no axioms or admissions. |
| `formal/z3/responsibility_boundary.smt2` | Independently refutes unsafe positive/negative, completion, dependency, runtime, identity-leak, and mutation states. | All twenty forbidden queries are `unsat`; none is `sat` or `unknown`. |

Run all three with:

```bash
./scripts/verify-responsibility-boundary-formal.sh
```

Campaign execution places the command inside a `systemd-run --user --scope`
with `MemoryMax=4G`, `MemorySwapMax=0`, one TLC worker, and disk-backed output.
The driver keeps temporary proof output below `target/` and deletes it on exit.

## Invariants extracted into tests

| ID | Formal invariant | Required implementation witness |
|---|---|---|
| RB-01 | Every exported exact semantic relation is backed by an independently replayed proof. | Corrupt or omit every proof form and require typed rejection/no tuple. |
| RB-02 | Positive proof facts survive unrelated incomplete equality or Datalog dimensions. | Exhaust all completion pairs and preserve the canonical positive prefix. |
| RB-03 | Conclusive absence requires equality and Datalog completion. | Exhaust the four completion pairs; only complete/complete permits a negative conclusion. |
| RB-04 | Combined completion is exact and never turns a limit into no-match. | Property-test the completion truth table and serialized labels. |
| RB-05 | Public facts contain stable source `NodeId`s, never Replete e-class or proof-arena IDs. | Compile-time API/privacy checks plus runtime range/provenance assertions. |
| RB-06 | All relation rows and fact files use deterministic canonical order. | Shuffle classes, occurrences, candidates, and maps; require byte-identical files. |
| RB-07 | Export owns its tuples and shares no mutable engine state. | Drop source reports after export, clone/serialize facts, and prove outputs remain usable and equal. |
| RB-08 | Export does not mutate the CPG, semantic report, or structural Datalog facts. | Snapshot all three before/after complete and limited paths. |
| RB-09 | A Datalog tuple cannot construct or deserialize equality authority. | Compile-fail/private-constructor checks and one-way public API tests. |
| RB-10 | Replete has no libcpg/Datalog-runtime dependency and libcpg embeds no Datalog/e-graph alternative runtime. | Repository dependency script checks manifests, Cargo metadata/tree, and source-module boundaries. |
| RB-11 | Base CPG EDB export remains feature-free and semantically unchanged. | Existing Datalog unit/integration/property tests plus exact pre/post fixtures. |
| RB-12 | Semantic relation files use the same canonical RFC 4180 tab-separated encoding contract. | Quoting, empty, singleton, parallel-context, and round-trip file examples. |
| RB-13 | Large exports use iterative flat loops and constant native-stack depth. | 100,000 class members through export/clone/format/serialize/drop on a 256 KiB stack. |
| RB-14 | Invalid indices, duplicate identities, malformed completion, and oversized work fail closed. | Adversarial constructors/caps and typed errors; no panics on untrusted input. |
| RB-15 | MeTTaIL remains untouched. | Dirty-worktree and dependency audits record no MeTTaIL write/build/format/branch/commit. |

## Planned implementation shape

```text
EXPORT-SEMANTIC-EDB(report):
    borrow report immutably
    replay every proof referenced by a public semantic relation
        because an e-class or tuple alone has no equality authority
    copy stable source IDs into owned relation rows
    sort and deduplicate every relation by its declared total order
    copy exact completion dimensions into typed completion rows
        because a caller must distinguish limited from complete absence
    render canonical in-memory SouffleFactFile buffers on request
    return the owned bundle without retaining a CPG, report, Replete, or
    Datalog-engine reference

COMBINE-COMPLETION(equality, datalog):
    if equality is complete and datalog is complete:
        return Complete
    return Limited with both original dimensions
```

Production code must not be written until the executable formal gate is green.
The bounded TLC model is not a proof about arbitrary heap sizes; Rocq proves
the implications for arbitrary inputs, while Rust properties, compile-fail
checks, dependency checks, serialization tests, and small-stack lifecycles
validate refinement and ownership.

## References

1. Kildall, G. A. (1973). *A Unified Approach to Global Program Optimization.*
   DOI: [10.1145/512927.512945](https://doi.org/10.1145/512927.512945).
2. Nelson, G., Oppen, D. C. (1980). *Fast Decision Procedures Based on
   Congruence Closure.* DOI:
   [10.1145/322186.322198](https://doi.org/10.1145/322186.322198).
3. Jordan, H., Scholz, B., Subotić, P. (2016). *Soufflé: On Synthesis of
   Program Analyzers.* DOI:
   [10.1007/978-3-319-41540-6_23](https://doi.org/10.1007/978-3-319-41540-6_23).
