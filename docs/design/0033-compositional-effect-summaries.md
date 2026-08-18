# ADR-0033: Compositional interprocedural effect summaries

- **Status:** Accepted
- **Date:** 2026-08-13
- **Amended:** 2026-08-18
- **Scope:** feature-free `analysis::effects`

## Context

Taint, System Dependence Graph (SDG), and optimization clients need to know
whether a represented function reads or writes observable state, returns an
argument, or returns one constant. Re-running whole-body analysis at every
call duplicates work and fails on recursion. A source-text classifier would
also violate libcpg's language-agnostic evidence boundary.

T3.2 already supplies validated call-graph SCC scheduling and bounded monotone
fixed points. T3.5 must instantiate that framework without weakening its
determinism or completeness contracts.

The original local return evaluators nevertheless encoded AST and
reaching-definition depth as mutually recursive Rust calls. A caller could
raise `IfdsCaps::max_call_depth` to analyze a valid deep expression, yet the
native thread stack could fail before the documented semantic cap. The same
problem existed in the index-base walk used to map an actual argument to a
place. A logical depth check is resource policy; it is not a stack-safety
mechanism.

## Decision

1. Represent observable storage as `Global`, `Param`, or `Field` places.
2. Build direct effects only from typed CPG node/edge evidence.
3. Compose resolved callees in deterministic `NodeId` order; map callee
   parameter places through the shared call-argument normalizer.
4. Treat unresolved/external calls and represented throw/await/yield effects
   conservatively as `Impure`.
5. Reuse SCCP's checked scalar fold functions for local and interprocedural
   constant returns.
6. Store constants in a lawful bottom/constant/overdefined coordinate. Preserve
   the ergonomic `Option<ConstValue>` plus an explicit overdefined bit.
7. Offer `analyze_effects` as the completeness-preserving API and retain the
   task-specified `effect_summaries` map projection as a documented convenience.
8. Make domain-local caps explicit in each summary and in the aggregate result.
9. Defunctionalize parameter-dependence and constant evaluation into separate
   typed continuation machines. Store frames, intermediate values, and the
   active path on the heap; use an iterative cursor for index-base descent.
10. Preserve the recursive specification's left-to-right child order, logical
    depth, work accounting, conservative cycle behavior, lattice joins, and
    call-summary boundaries exactly.
11. Cache only terminal evaluations. A cache hit still consumes the same node
    entry and logical-depth check, so enabling the cache cannot change cap
    behavior. Nonterminal results remain path-context-sensitive and are not
    memoized.
12. Use fallible growth for required machine-owned frame, value, and active-path
    storage. Its allocation failure widens dependencies to every bit or
    constants to overdefined, marks the traversal capped, and never publishes a
    false complete result. Failure to reserve the optional terminal cache merely
    skips cache publication.

## Typed local machines

The **parameter-dependence machine** carries `Enter`, `Visit`, `Combine`, and
`Exit` frames. `Visit` schedules the next ordered AST/def-use/call child;
`Combine` ORs its returned bit mask. The **constant machine** carries `Enter`,
`JoinNext`, `JoinCombine`, `BinaryLeft`, `BinaryRight`, `Unary`, `Forward`, and
`Exit` frames. Those variants make every suspended recursive continuation and
intermediate lattice value explicit.

![Effect return evaluators use typed heap continuation machines rather than source-depth native recursion.](../diagrams/effect-evaluation-machines.svg)

In both machines, `on_path` represents only active evaluation ancestors.
`Exit` removes the current node before returning its value. A repeated active
dependency node widens to `u64::MAX` and marks the traversal capped; a repeated
active constant node evaluates to overdefined, matching the prior conservative
constant-cycle semantics. Completed terminal values may enter a local cache,
but results assembled from children do not, because their meaning can depend on
which ancestors are active.

For logical path depth $`D`$, visited work $`W`$, and maximum frontier width
$`B`$, native call depth is $`O(1)`$ while heap machine state is
$`O(D+B)`$. `max_call_depth` still bounds $`D`$ and
`max_worklist_steps` still bounds $`W`$; neither value selects a thread-stack
size.

## Rejected alternatives

### Parse source or match language spellings

Rejected because it duplicates frontend parsers, disagrees across languages,
and can fabricate evidence absent from the CPG.

### Use `Option<ConstValue>` as both bottom and overdefined

Rejected because it is not a lattice. After two constants conflict, a later
join could incorrectly revive a constant.

### Classify every parameter mutation as globally impure

Rejected because the callee does not know what actual place its caller passes.
Delayed parameter remapping is both more precise and compositional.

### Return only a map

Rejected as the sole API because caps and malformed SCC evidence would be
lost. The map remains only as a convenience projection.

### Keep native recursion behind the existing depth cap

Rejected because a semantic cap that a caller may raise cannot guarantee that
the native stack reaches the cap. It would also make identical analysis depend
on thread-stack size.

### Increase the thread stack or use a stack-switching helper

Rejected because it moves the failure threshold without changing the recursive
ownership of continuation state. The analysis already has a natural finite
continuation algebra, so explicit frames address the root cause directly.

### Memoize every completed subtree

Rejected because a nonterminal result can be conservative specifically due to
an ancestor on the active path. Reusing that result in another path would make
cache policy observable. Terminal caching is context independent and retains
identical entry/work accounting.

## Consequences

- Recursive effects converge through the existing generic solver.
- Callers can distinguish reads, writes, constants, parameter-return flow,
  truncation, and incomplete results.
- No graph nodes or edges are created or modified.
- Field/index precision is limited by existing typed DFG/reference evidence.
- The 64-bit return-dependence mask has an explicit truncation indicator.
- Local evaluator native call depth is constant for arbitrarily deep accepted
  AST/DFG paths; source depth is represented only by bounded heap state.
- Required machine-state allocation failure is conservative, visible cap
  evidence rather than a partial negative result; optional cache allocation is
  allowed to degrade to uncached evaluation.
- T3.3 taint and T3.4 SDG can refine external/call summaries without acquiring
  a second interprocedural scheduler.

## Verification

Example tests cover pure leaves, globals, fields, parameter mutation,
unresolved and impure calls, scalar folding, disagreement, recursion,
transitive definitions, malformed SCCs, exact caps, cycles, missing nodes,
terminal-cache equivalence, 10,000-way joins, and width boundaries. Six
256-case properties add a bounded recursive specification oracle for both
machines, reaching-definition insertion permutation, and local-cap monotonicity
to the product-lattice, transitive-purity, and SCC-cap campaigns. A public test
runs a 100,000-level expression through `analyze_effects` on a 256 KiB thread
stack. Feature-free, Serde, and maximal-feature gates exercise the exported
contract.

## References

1. Reps, Horwitz, Sagiv (1995), DOI
   [10.1145/199448.199462](https://doi.org/10.1145/199448.199462).
2. Wegman, Zadeck (1991), DOI
   [10.1145/103135.103136](https://doi.org/10.1145/103135.103136).
3. Reynolds, J. C. (1972). *Definitional Interpreters for Higher-Order
   Programming Languages.* DOI:
   [10.1145/800194.805852](https://doi.org/10.1145/800194.805852).
