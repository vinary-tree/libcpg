# Interprocedural constant and effect summaries

`libcpg::analyze_effects` computes deterministic, read-only procedure summaries
over the current resolved call graph. It is the first concrete client of the
[IFDS/IDE summary framework](ifds-ide-framework.md): the generic engine supplies
callee-first scheduling and recursive fixed points, while this component
defines what an *effect* means.

The analysis is feature-free and language-agnostic. It consumes typed Code
Property Graph (CPG) nodes and edges; it does not parse source text, match
language syntax strings, query a database, or add a parallel graph.

![Effect-summary flow](../../diagrams/effect-summary-composition.svg)

![Typed local evaluators keep source-depth continuation state on the heap.](../../diagrams/effect-evaluation-machines.svg)

## 1. Vocabulary

- A **place** is storage whose observation matters outside one local scalar
  computation. `Place::Global` names an out-of-function variable,
  `Place::Param` names a zero-based formal parameter, and `Place::Field` names
  object or nominal state.
- **Purity** is an ordered classification: `Pure`, `ReadsGlobal`,
  `WritesGlobal`, then `Impure`. A larger value is conservatively worse.
- A **procedure summary** is a compact description of reads, writes, purity,
  return dependence, and a scalar return constant.
- **Overdefined** means available paths disagree or an operation cannot be
  folded safely. **Bottom** means no return evidence has arrived yet. They are
  deliberately distinct.
- A **continuation frame** is a typed heap value describing the work that must
  resume after one child evaluation. A **local evaluation machine** is the
  frame stack, value stack, active-path set, terminal cache, and traversal
  budget considered together.

## 2. Public API

```rust
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use libcpg::{ConstValue, NodeId};

pub enum Place {
    Global(Arc<str>),
    Param(u16),
    Field(Arc<str>),
}

pub enum Purity { Pure, ReadsGlobal, WritesGlobal, Impure }

pub struct EffectSummary {
    pub reads: BTreeSet<Place>,
    pub writes: BTreeSet<Place>,
    pub purity: Purity,
    pub param_to_return: u64,
    pub const_return: Option<ConstValue>,
    pub const_return_overdefined: bool,
    pub param_mask_truncated: bool,
    pub capped: bool,
}

// The completeness-preserving entry point.
pub fn inspect(
    cpg: &libcpg::CodePropertyGraph,
    call_graph: &libcpg::SccDecomposition,
    caps: &libcpg::IfdsCaps,
) -> libcpg::EffectAnalysis {
    libcpg::analyze_effects(cpg, call_graph, caps)
}

// The task-specified map projection.
pub fn summaries(
    cpg: &libcpg::CodePropertyGraph,
    call_graph: &libcpg::SccDecomposition,
    caps: &libcpg::IfdsCaps,
) -> BTreeMap<NodeId, libcpg::EffectSummary> {
    libcpg::effect_summaries(cpg, call_graph, caps)
}
```

Use `analyze_effects` for decisions. `effect_summaries` intentionally projects
away completeness evidence and can therefore contain a capped monotone prefix.

## 3. Evidence interpretation

| CPG evidence | Summary interpretation |
|---|---|
| Identifier whose definition is an out-of-function `Variable` | `Global(name)` read, or write when it is the assignment target |
| Identifier reaching a formal `Parameter` | `Param(index)` read/write |
| `FieldRead` / `FieldWrite` incident on `MemberAccess` | named `Field` read/write |
| `IndexRead` / `IndexWrite` | read/write of the base parameter, global, or field when represented |
| `Throw`, `Await`, or `Yield` | direct `Impure` evidence |
| one resolved callee | compose its current summary |
| external or unresolved callee | conservatively `Impure`; returned value is overdefined |
| scalar literal/unary/binary expression | the same checked folding vocabulary as SCCP |

Only evidence represented in the CPG creates a place. An identifier with no
definition or def-use evidence is not guessed to be global. This is the same
evidence-boundary discipline used by call resolution.

## 4. Composition algorithm

The following literate pseudocode names both the action and its rationale.

```text
EFFECT-SUMMARY(function, current summaries, resolver, caps)
    start with direct typed-edge reads/writes and direct control effects
        because local evidence must survive every recursive round

    for call in ascending NodeId order
        if call resolves to a represented callee with a current summary
            map callee Param(i) places through call argument i
            union mapped reads and writes
            retain the worse purity
        otherwise
            raise purity to Impure
        end if
    end for

    for each represented return expression
        run PARAMETER-DEPENDENCIES with typed heap continuation frames
        run EVALUATE-CONSTANT with typed heap continuation frames
        meet all return constants in the three-level constant lattice
    end for

    if a local traversal reaches a configured bound
        widen parameter dependence to every representable bit
        widen the constant to overdefined and set capped
    end if

    return the monotone summary
```

The generic solver visits acyclic callees once and recursive strongly
connected components (SCCs) to a bounded fixed point. Therefore callers see
stable callee facts without a second whole-program pass.

The two local machines are deliberately separate: their continuation algebras
and conservative cycle results differ. In literate pseudocode, parameter flow
is:

```text
PARAMETER-DEPENDENCIES(root)
    frames := [Enter(root, depth=0)]
    values := []
    active := {}

    while frames is not empty
        Enter(node, depth):
            charge the logical depth/work budget
            if node is active, return every parameter bit and mark capped
            add node to active
            if node directly denotes a formal, start with its bit
            choose ordered children:
                call -> arguments selected by the callee return mask
                other -> AST children followed by reaching definitions
            schedule Visit(node, children, next=0, accumulated_mask)

        Visit(node, children, next, mask):
            if one child remains, schedule Combine then Enter(child)
            otherwise schedule Exit(node, mask)

        Combine(...):
            OR the child result into the mask and resume Visit

        Exit(node, mask):
            remove node from active and return mask to the parent
```

Constant folding uses the same enter/exit discipline but gives each suspended
operation its own frame:

```text
EVALUATE-CONSTANT(root)
    frames := [Enter(root, depth=0)]
    values := []
    active := {}

    while frames is not empty
        Enter(node, depth):
            charge the logical depth/work budget
            if node is active, return Overdefined
            literal/call/missing -> schedule terminal Exit
            identifier -> schedule JoinNext over sorted unique definitions
            binary -> schedule BinaryLeft, then Enter(left)
            unary -> schedule Unary, then Enter(operand)
            assignment/variable/return -> schedule Forward, then Enter(value)

        JoinNext / JoinCombine:
            evaluate definitions in order and join in the constant lattice
        BinaryLeft / BinaryRight:
            retain the left value, evaluate right, then use checked SCCP fold
        Unary / Forward:
            fold or forward the returned child value
        Exit(node, value):
            remove node from active and return value to the parent
```

Only terminal results are cached. Every cache hit still performs `Enter` and
charges one step, so cached and uncached executions have identical cap and work
semantics. Required machine-owned frame, value, and active-path growth is
fallible; failure widens and sets visible cap evidence. The cache is an
optimization, so failure to reserve a cache entry simply omits that entry.

## 5. Lattice semantics

`EffectSummary` is a product lattice. Reads and writes use set union; the
parameter mask uses bitwise OR; Boolean evidence uses OR; purity uses the worse
ordered value. The constant coordinate is

```math
\bot_C \sqsubset c \sqsubset \top_C,
```

where $`c`$ is one exact `ConstValue`, distinct constants join to
$`\top_C`$, and `const_return_overdefined` distinguishes $`\top_C`$ from the
`None` representation of $`\bot_C`$. This extra bit prevents a conflicting
constant from incorrectly becoming constant again in a later recursive round.

The `u64` parameter mask represents formal parameters 0 through 63.
`param_mask_truncated` records a wider signature. Transitive use of a truncated
callee conservatively visits every argument and makes the result incomplete.

## 6. Purity and place remapping

A write to `Param(i)` alone remains `Pure`: the summary says the function can
mutate its caller-provided place, but it does not yet know whether that place is
externally visible. At a call, parameter places are remapped through actual
arguments. Passing a global to such a callee turns the caller into
`WritesGlobal`; passing a local scalar does not fabricate a global effect.

This delayed remapping is what makes summaries reusable and compositional.

## 7. Determinism, bounds, and malformed input

- functions, calls, returns, facts, places, and evidence use sorted vectors or
  `BTreeMap`/`BTreeSet`;
- the exact current call-graph SCC decomposition is validated before analysis;
- recursive rounds and shared scheduler work use `IfdsCaps`;
- expression/dependence machines use `max_call_depth` as logical path depth,
  `max_worklist_steps` as node-entry work, and active-path cycle guards;
- input depth grows bounded heap frame state, never the native call stack;
- exact-cap acceptance and one-smaller rejection are part of the tested API
  behavior, independent of thread-stack size;
- a domain-local limit sets `EffectSummary::capped`, rolls up through
  `EffectAnalysis::domain_capped_functions`, widens dependencies, and prevents
  `is_complete()` from succeeding;
- required machine-state allocation failure follows the same fail-closed
  widening path, while an unavailable optional cache remains semantically inert;
- the source CPG is never mutated.

An incomplete may-summary can still prove a positive effect. It cannot justify
the negative claim “this function never writes that place.”

## 8. Complexity

Let $`F`$ be functions, $`C`$ call sites, $`A`$ inspected AST/def-use evidence,
$`I`$ recursive rounds, $`D`$ the maximum admitted local path depth, and $`B`$
the maximum ordered child frontier. Ordered-set operations add logarithmic
factors. The bounded worst-case work is

```math
O\!\left(I(A+C)\log P\right),
```

where $`P`$ is the number of distinct places retained. Acyclic components have
$`I=1`$. A local machine retains $`O(D+B)`$ heap continuation/value/path state
and $`O(A)`$ terminal cache state in the worst case. Its native call depth is
$`O(1)`$. The configured work and logical-depth caps provide operational
ceilings.

## 9. See also

- [Theory 26](../../theory/26-interprocedural-effect-lattice.md)
- [ADR-0033](../../design/0033-compositional-effect-summaries.md)
- [Scientific validation 28](../../scientific/28-effect-summary-validation.md)
- [Usage 09](../../usage/09-effect-summaries.md)
- [Input and resource hardening](../../security/01-input-and-resource-hardening.md)

## References

1. Reps, T., Horwitz, S., Sagiv, M. (1995). *Precise Interprocedural Dataflow
   Analysis via Graph Reachability.* POPL. DOI:
   [10.1145/199448.199462](https://doi.org/10.1145/199448.199462).
2. Wegman, M. N., Zadeck, F. K. (1991). *Constant Propagation with Conditional
   Branches.* ACM TOPLAS 13(2), 181–210. DOI:
   [10.1145/103135.103136](https://doi.org/10.1145/103135.103136).
3. Reynolds, J. C. (1972). *Definitional Interpreters for Higher-Order
   Programming Languages.* DOI:
   [10.1145/800194.805852](https://doi.org/10.1145/800194.805852).
