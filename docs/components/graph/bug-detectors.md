# Component — Advisory bug detectors

## Responsibility

`analysis::bugs` turns normalized CPG evidence into deterministic resource-leak,
uninitialized-use, uncaught-exception, and dead-catch candidates. It owns only
the detector composition and report surfaces. Language parsing belongs to the
frontend; typestate transfer, variable facts, reaching definitions, call SCCs,
and bounded summary scheduling remain in their shared analysis components.

This page covers those evidence-composed detector families. The same module's
missing-check and deviant-behavior surfaces are specified separately in the
[population anomaly component](population-anomaly-detectors.md).
I8 inconsistent-clone and I9 infeasible-path evidence are specified by the
[clone/feasibility component](clone-feasibility-bug-detectors.md), which
composes clone/GED, SCCP, and interval providers rather than duplicating them.

![Inputs, shared analyses, findings, and completeness evidence](../../diagrams/bug-detector-pipeline.svg)

## Feature boundary

Every detector and report type is available with `default = []`. A `lang-*`
feature is needed only when libcpg itself parses source. Callers using
`build_from_tree` may supply their own grammar. Enabling `serde` adds ordered
report serialization without changing analysis semantics.

## Public surface

| API/type | Contract |
|---|---|
| `analyze_resource_leaks(cpg, fsm, caps)` | qualified leak projection plus complete underlying typestate evidence |
| `resource_leaks(cpg, fsm, caps)` | positive-only `Vec<ResourceLeak>` projection |
| `ResourceLeak` | allocation object, semantic exit, non-accepting state, canonical witness, and cap qualifier |
| `ResourceLeakAnalysis` | ordered leaks, `TypestateAnalysis`, detector issues, and `is_complete()` |
| `analyze_uninitialized_uses(cpg)` | qualified binder-aware empty-reaching-definition report |
| `uninitialized_uses(cpg)` | positive-only `Vec<UninitializedUse>` projection |
| `UninitializedUse` | function, executable use site, canonical variable name, and out-of-scope definitions |
| `UninitializedUseAnalysis` | ordered uses, processed functions, and complete represented-relation predicate |
| `analyze_exception_flow(cpg, caps)` | bounded summaries, uncaught entries, reached/dead catches, work, caps, and issues |
| `uncaught_exceptions(cpg, caps)` | positive-only uncaught projection |
| `dead_catches(cpg, caps)` | positive-only dead-handler projection |
| `ExceptionType` | ordered `Named(Arc<str>)` or explicit `Unknown` type |
| `ExceptionSummary` | finite ordered set of escaping types for one function |
| `ExceptionFlowAnalysis` | all summaries/findings and shared solver qualifications |

All output identifiers refer to nodes in the input CPG. The detector does not
append nodes or edges.

## Internal composition

### Resource leaks

1. Run `analyze_typestate` with the caller's `PropertyFsm` and `IfdsCaps`.
2. Reject leak semantics with `NoAcceptingState` when the policy defines no
   legal terminal state.
3. Project each canonical `TypestateTerminal` whose state is not accepting to a
   `ResourceLeak`.
4. Preserve typestate issues, caps, work, violations, and terminal evidence in
   the report instead of weakening them to a Boolean.

### Uninitialized uses

1. Enumerate functions and function-scope nodes in stable identifier order.
2. Reuse `variable_facts` to obtain binder-aware variable uses and names.
3. Exclude unbound identifiers in a represented declaration signature and the
   callee/member-name syntax of unbound calls.
4. Query `reaching_definitions(use_site)` and partition definitions by current
   function scope.
5. Emit one finding per canonical variable fact only when the in-scope
   partition is empty; retain the other partition as diagnostic evidence.

### Exception flow

1. Reuse `call_graph_sccs` and `solve_summaries` with an
   `ExceptionSummary` finite-set lattice.
2. Recover direct throw types from typed node, definition, reference, `TypeOf`,
   and relevant DFG evidence with an ordered heap worklist.
3. Join escaping callee summary types at represented unique call targets.
4. Dispatch each origin through lexically enclosing `Try`/`Catch` layers and
   the finite inheritance relation.
5. Record reached catches. Emit uncaught evidence at source call components and
   dead catches absent from the reached map.
6. Reconstruct a canonical shortest function/call/throw witness with a
   heap-backed ordered breadth-first search.

![Finite summaries, handler dispatch, and canonical evidence](../../diagrams/exception-flow-summary.svg)

## Frontend contract

| Evidence | Required representation |
|---|---|
| explicit throw | `CpgNodeKind::Throw` with thrown value as AST child |
| catch clause | `CpgNodeKind::Catch` with parameter/type and body AST children |
| handler entry | `ControlFlow(CfgEdgeKind::Catch)` from catch to body |
| call target | one represented function through embedded target and/or call edge |
| nominal type | `TypeAnnotation`, typed declaration, class/struct/enum/trait, or connected type evidence |
| subtype | `Inherits` or `Implements` edge from subtype to supertype |
| variable use | shared fact-universe use backed by identifier/binding evidence |
| reaching definition | normalized DFG/reference/definition relation |

`build` and `build_from_tree` must produce equal semantics for the same parsed
tree. Re-running CFG extraction must not duplicate or retype catch entry.

## Invariants

1. The CPG node and edge counts are unchanged by all detector APIs.
2. Every leak object/exit and every witness element exists in the input CPG.
3. A leak exists exactly for a non-accepting typestate terminal.
4. A reported uninitialized use has no reaching definition in its function
   scope; every retained out-of-scope definition lies outside that scope.
5. Unbound declaration and callee syntax is never reported as a variable read.
6. Exception summaries grow only by finite set union.
7. The nearest first matching catch handles an origin; caught types do not
   escape that function summary.
8. A dead catch has no entry in `reached_catches`.
9. Findings and maps are independent of input edge insertion order.
10. Every graph-shaped traversal is iterative and heap-backed.
11. An empty convenience vector never licenses a negative claim by itself.

## Bounds and failure semantics

Resource leaks inherit all typestate caps and issues. `NoAcceptingState`
prevents candidate projection because no release policy has been defined.
Uninitialized-use projection has no truncation mode; it is finite over the
represented function/use/reaching-definition relation.

Exception flow uses these shared caps:

| `IfdsCaps` field | Exception-flow role |
|---|---|
| `max_functions` | admits represented functions before summary solving |
| `max_facts_per_node` | bounds direct and summarized exception types per function |
| `max_worklist_steps` | bounds shared summary-domain evaluations |
| `max_scc_iters` | bounds recursive call-component Kleene rounds |
| `max_call_depth` | inapplicable; SCC summaries replace recursive call-path expansion |

Unknown throw types, ambiguous represented calls, missing typed catch-entry
edges, and missing canonical origins appear as `ExceptionFlowIssue` values.
Shared solver structural errors are wrapped as `ExceptionFlowIssue::Ifds`.

## Tests

Feature-free example and property tests cover all invariants, each issue/cap,
deterministic ordering, independent oracles, 256 generated exception chains,
and report serialization. Public integration covers all three APIs together,
real Rust leak/use evidence, Java `build`/`build_from_tree` exception topology,
idempotent CFG extraction, graph immutability, and 100,000 AST levels on a
256 KiB native stack.
