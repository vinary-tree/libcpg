# ADR-0041: Share expression facts through a flat postorder arena

- **Status:** Accepted
- **Date:** 2026-08-18
- **Scope:** `ExprKey`, `ExprUniverse`, `expression_facts`, available expressions, very-busy expressions, and Serde

## Context

The former expression key was a recursive owned equation:

```rust,ignore
struct ExprKey {
    operator: Arc<str>,
    operands: Vec<ExprOperand>,
}

enum ExprOperand {
    // scalar cases omitted
    Expression(Box<ExprKey>),
}
```

This encoded source depth directly in Rust ownership. Construction,
variable collection, generated `Clone`, comparison, ordering, hashing,
formatting, Serde, and destruction could consume one native frame per nested
operator. Flattening each key independently would remove those stack frames but
would not solve the complete problem: a unary chain with $`n`$ expression
occurrences contains $`1+2+\cdots+n = n(n+1)/2`$ copied logical nodes if every
occurrence owns its own flat subtree.

The root cause therefore has two parts: recursive ownership and repeated
subtree ownership. Analysis caps, a larger stack, `stacker`, a recursive
compatibility wrapper, or one flat copy per occurrence leaves at least one part
intact.

## Decision

Expression facts use one shared arena per derivation:

- a **node tape** stores operator records in postorder;
- an **operand tape** stores scalar operands and `Expression(node_id)` links in
  source order;
- an `ExprKey` owns an `Arc` arena handle, one root id, cached logical metrics,
  and no recursive field;
- `ExprUniverse` deduplicates roots with exact bottom-up structural class ids;
  and
- borrowed `ExprKeyRef`, `ExprOperands`, and `ExprOperandRef` values project the
  logical tree without materializing it.

![CPG operators enter an explicit postorder machine, share one flat arena, and cross a canonical CEK1 persistence boundary.](../diagrams/expression-key-flat-arena.svg)

For every stored nested-expression link $`(p,c)`$, $`c<p`$. The shared arena may
be a directed acyclic graph: two ordered operands may deliberately reference
the same completed child. A public key's logical metrics count that child once
per occurrence, so resource bounds describe the value a caller observes rather
than only the compact physical storage.

### Exact structural classes and dense fact ids

When node $`p`$ is appended, the builder interns this exact descriptor:

```text
Class(p) := intern(
  operator(p),
  map operands(p):
    scalar payload       -> exact tagged scalar
    Expression(child_id) -> Class(child_id)
)
```

The child class already exists because children precede parents. Equal
descriptors receive the same class id; distinct descriptors cannot collide
because the interner compares the complete typed descriptor rather than a
finite digest. `ExprUniverse` enumerates distinct classes in deterministic
postorder-class order and resolves each occurrence without re-comparing a deep
tree. This fixed the measured 100,000-level integration path from more than one
minute of repeated deep binary searches to 1.64--1.73 seconds across the Serde
and all-feature debug-profile gates on the recorded development host.

Class ids are arena-local accelerators, not public semantic hashes. Equality,
ordering, and hashing between independent arenas still use the full logical
operator/operand equation through explicit cursors. Consequently decoding a
standalone key cannot change its equality, ordering, or hash meaning.

### Iterative CPG extraction

`expression_facts_with_limits` traverses each operator at most once with
explicit `Frame { node, children, next, operands, variables }` records. A frame
either consumes the next scalar, reuses a completed expression id, pushes an
unseen child frame, or assembles a completed node. An edge to an operator
already on the active path becomes `Opaque(NodeId)`, preserving the previous
conservative cycle behavior without recursive descent.

In literate pseudocode:

```text
for root in sorted function-scope operators:
  if root is complete: continue
  push enter(root)
  while frames is not empty:
    frame := top(frames)
    if frame has another child:
      scalar child: append exact scalar operand
      completed operator: append Expression(completed_id)
      active operator: append Opaque(child_id)
      unseen operator: push enter(child)
    else:
      pop frame
      id := append_postorder(frame.operator, frame.operands)
      remember id and its variable set
      append Expression(id) to its parent, if any
```

All input-depth state lives in heap vectors and sets. Cap failure aborts the
whole derivation; a partial universe could make a Must analysis report an
unsound absence.

### Public construction and lifecycle

`ExprKey::try_new` accepts owned `ExprOperand` values. The
`ExprOperand::Expression` payload is another flat handle, not a box. When there
is one uniquely owned canonical child arena, the constructor consumes and
extends that arena; a 100,000-level mixed unary chain is therefore linear rather
than repeatedly copied. Multiple or shared children are copied with an
explicit postorder worklist.

Logical comparison uses counted key/operand frames. Hashing and debug formatting
use explicit event tapes. Clone copies an `Arc`; destruction decrements it and,
for the final owner, drops flat vectors. No lifecycle operation materializes a
recursive compatibility tree.

### Canonical `CEK1` persistence

Standalone persistence uses `CEK` plus version byte `1`, little-endian `u64`
node and operand counts, node records, and an operand tape. A node record is
`operator_length`, UTF-8 operator bytes, `first_operand`, and `operand_count`.
The operand alphabet is closed:

| Tag | Operand | Payload |
|---:|---|---|
| 0 | variable | `u64` length and UTF-8 bytes |
| 1 | integer | little-endian `i64` |
| 2 | float | exact little-endian `u64` IEEE-754 bits |
| 3 | boolean | exactly byte 0 or 1 |
| 4 | string | `u64` length and UTF-8 bytes |
| 5 | character | valid little-endian Unicode-scalar `u32` |
| 6, 7, 8 | null, array, object | none |
| 9 | regular expression | `u64` length and UTF-8 bytes |
| 10 | nested expression | preceding `u64` node id |
| 11 | opaque operand | little-endian CPG `NodeId` as `u32` |

A shared arena is projected to one canonical postorder tree before encoding;
shared child occurrences are emitted separately. Decoding rejects empty
arenas, invalid tags/scalars/UTF-8, forward references, cycles, malformed or
nonpartitioned ranges, noncontiguous child subtrees, duplicate or missing
parents, truncation, and trailing bytes. Thus every accepted `CEK1` byte string
has one physical interpretation. Serde carries those bytes with
`serialize_bytes` and a bounded byte visitor rather than a recursive data-model
visitor.

### Independent bounds

`ExprKeyLimits` names five independent dimensions: logical/physical nodes,
logical/physical operands, aggregate string bytes, canonical encoded bytes,
and work steps. Defaults are one million nodes, four million operands, 64 MiB
of strings, 256 MiB encoded, and five million steps. Checked arithmetic and
fallible vector reservations return `ExprKeyError`; no nesting cutoff or stack
size participates in semantics.

## API migration

| Previous use | Current use |
|---|---|
| `ExprKey { operator, operands }` | `ExprKey::try_new(operator, operands)?` |
| `ExprOperand::Expression(Box::new(key))` | `ExprOperand::Expression(key)` |
| `key.operator` | `key.operator()` |
| `&key.operands` | `key.operands()` and its borrowed iterator |
| `expression_facts(cpg, function)` returning a value | returns `Result<ExprFacts, ExprKeyError>` |
| `FactUniverse<ExprKey>` | specialized shared `ExprUniverse` with the same dense lookup surface |

`available_expressions` and `very_busy_expressions` map expression failures to
`AnalysisError::ExpressionKey`; they never continue with incomplete facts.

## Rejected alternatives

### Hand-write a recursive parser with a larger recursion limit

Rejected. Tree-sitter already supplies syntax. The problem is post-parse fact
ownership and traversal, not token recognition. A recursion limit would also
turn valid deep syntax into missing facts.

### Flat-copy every occurrence

Rejected. It is stack safe but requires quadratic ownership for a deep chain.

### Store only a finite structural digest

Rejected. A collision could merge unequal expressions and make CSE/PRE
evidence unsound. Structural class interning compares complete typed
descriptors; hashing remains an API operation, not semantic identity.

### Keep the old recursive type as a compatibility adapter

Rejected. Constructing, comparing, serializing, or dropping the adapter would
restore source-controlled native recursion.

### Extract a generic arena crate now

Deferred under the same criterion as ADR-0040. Property trees use a strict
single-parent node/edge tape; expression facts use a shared DAG, arena-local
structural classes, `Opaque(NodeId)`, and analysis-specific failure semantics.
The MeTTa/f1r3node pushdown machines demonstrate reusable transformation
techniques, but not yet one stable semantic crate shared by two unrelated
public data models.

## Consequences

### Positive

- Native call depth is constant across extraction, lifecycle, and persistence.
- One shared arena makes a deep expression-fact universe linear in physical
  syntax size.
- Exact structural classes avoid both collisions and repeated deep searches.
- Ordered operands, exact float bits, conservative opaque identity, and public
  CSE/PRE semantics remain intact.
- Typed bounds fail closed before a Must analysis can consume partial facts.

### Negative

- Public struct literals and boxed nested operands require migration.
- `expression_facts` is now fallible.
- A shared DAG must be expanded to a canonical tree for standalone `ExprKey`
  persistence; logical encoded-byte limits account for that expansion.
- Structural class descriptors add one flat typed descriptor per physical node.

## Verification

Examples cover every operand, unary/binary/n-ary order, exact floats, opaque and
missing/unsupported nodes, shared subexpressions, and malformed AST cycles.
Generated chains check canonical round trips and equality/order/hash laws.
Adversarial bytes cover prefix, counts, ranges, references, tags, boolean,
character, UTF-8, truncation, and trailing data. Exact and one-smaller tests
cover every cap. A public integration builds and queries 100,000 unary
expressions, clones/compares the fact universe, and drops the CPG and facts on a
256 KiB thread stack. A separate key gate constructs a 100,000-level mixed
tree and exercises clone, comparison, hash, debug, `CEK1`, Serde, and drop on
the same stack size.

## References

1. Reynolds, J. C. (1972). *Definitional Interpreters for Higher-Order
   Programming Languages.* DOI:
   [10.1145/800194.805852](https://doi.org/10.1145/800194.805852).
2. Knuth, D. E. (1984). *Literate Programming.* DOI:
   [10.1093/comjnl/27.2.97](https://doi.org/10.1093/comjnl/27.2.97).
3. f1r3node-rust-mettail, stable reference
   `dcc4600d4560a32671302841c79e2a1637091f83`, including schema-driven
   encoder/decoder continuations under `models/src/rust/rholang/`.
4. mettail-rust, stable reference
   `1d2d8ce8c902399cbeb567294f0b9acc93d4c07c`, including mutually recursive
   lifecycle flattening under `ast/src/language/lifecycle.rs` and
   `prattail/src/*/lifecycle.rs`.
