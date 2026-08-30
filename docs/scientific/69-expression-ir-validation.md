# Scientific validation 69 -- Typed expression IR

## Claims under test

The validation tests six claims:

1. the heap work machine emits one deterministic postorder node per reachable
   source fact and every operand identifier precedes its parent;
2. malformed, limited, impure, unknown, or persisted evidence cannot expose a
   rewrite certificate;
3. every named law agrees with independent type/value semantics over its
   represented domain;
4. language profiles and parser-built CPG topology remain separate from the IR
   layer's typed facts;
5. serialization preserves structural bytes while revoking authority; and
6. logical depth does not become native call-stack depth across lowering,
   query, clone, equality, hash, debug, serialization, or destruction.

## Formal triangulation

[`ExpressionIr.tla`](../../formal/tla/ExpressionIr.tla) model-checks the bounded
`Enter`/`Exit` machine, shared-node reuse, cycle rejection, exact work/node
limits, backward references, and live-to-persisted transition. TLC enumerates
all reachable states for every small profile/mode/shape/cap combination.

[`ExpressionIr.v`](../../formal/rocq/ExpressionIr.v) proves universally that
eligibility implies complete lowering, pure evidence, a total profile, an
enabled law, and live provenance. It separately proves identity-only, unknown,
incomplete, and persisted denial plus flat append preservation.

[`expression_ir.smt2`](../../formal/z3/expression_ir.smt2) independently refutes
fifteen forbidden states: six certificate prerequisites and nine numeric
descriptor, shift-count, and IEEE-bit boundary contradictions.

These models specify the admission boundary. They do not prove a frontend's
type checker or a later Replete rewrite set.

## Example and property campaigns

The focused unit suite includes:

- positive and denied examples for all six laws;
- every one of ten independently missing profile dimensions;
- all 255 nonempty combinations of the eight effect bits;
- checked overflow, division/remainder by zero, signed shift counts, malformed
  integer descriptors, NaN, infinities, and both zero signs;
- resolved/external/unresolved calls and immutable/shared/mutable/volatile/
  atomic loads;
- cycles, missing facts/nodes, non-expression roots, topology, type, literal,
  binding, call, and language mismatches;
- shared subexpressions and exact work/node/operand caps; and
- alpha-renaming, lexical scope, source-text, and operator-spelling isolation.

Generated properties compare checked `i8` folding with an independent Rust
checked-arithmetic oracle, finite floating values with an IEEE-bit oracle,
masked shifts with the signed-count model, fact insertion permutations with
canonical IR equality, arbitrary unary chains with postorder invariants, and
every cap with exact expected completion.

With `serde`, generated artifacts serialize to byte-identical structure after
round-trip while every decoded artifact returns `PersistedArtifact`. A compile-
time negative trait assertion proves `RewriteCertificate` does not implement
`DeserializeOwned`.

## Parser integration

`tests/expression_ir_integration.rs` builds real CPGs through the configured
tree-sitter frontends for all nine supported profiles. The adapter locates a
normalized binary expression and supplies typed facts without reading its
operator spelling. Integer-language fixtures certify addition by zero;
JavaScript and TypeScript correctly deny the same algebraic certificate under
strict binary64 signed-zero semantics.

## Deep stack campaign

A 100,000-unary-node expression is constructed, lowered, queried, cloned,
compared, hashed, debug-formatted through a streaming sink, serialized,
deserialized, and destroyed on a worker thread with a 256 KiB native stack.
The complete arena contains 100,001 nodes. The CPG node/edge census is checked
before and after lowering to establish input immutability.

The process runs in a headless `systemd-run --user --scope` with a 4 GiB
resident-memory ceiling, swap disabled, bounded tasks, and one Cargo build job.

## Gate matrix

| Command family | Evidence |
|---|---|
| `scripts/verify-expression-ir-formal.sh all` | TLC, Rocq, and fifteen Z3 refutations |
| `cargo test --lib expression_ir::tests --features serde` | examples, generated oracles, caps, persistence, deep lifecycle |
| `cargo test --test expression_ir_integration --features <nine languages>` | parser-built profile integration |
| `cargo clippy --lib --test expression_ir_integration --features <matrix> -- -D warnings` | warning-free target surface |
| `./scripts/verify-rustdoc.sh` and `./scripts/verify-docs.sh` | API prose, links, DOI syntax, and deterministic diagrams |
| `./scripts/verify.sh` and `pgmcp bug-gate` | terminal repository and policy gates |

## Interpretation limits

Passing these campaigns supports the represented IR contract. It does not
prove completeness for unsupported languages, correctness of arbitrary
frontend facts, whole-program equivalence, saturation, global extraction
optimality, or a Replete implementation. Every new law or semantic mode must
extend the formal ledger before production code.

## References

1. Leroy, X. (2009). *Formal Verification of a Realistic Compiler.* CACM. DOI:
   [10.1145/1538788.1538814](https://doi.org/10.1145/1538788.1538814).
2. Willsey, M. et al. (2021). *egg: Fast and Extensible Equality Saturation.*
   POPL. DOI: [10.1145/3434304](https://doi.org/10.1145/3434304).
3. Knuth, D. E. (1984). *Literate Programming.* DOI:
   [10.1093/comjnl/27.2.97](https://doi.org/10.1093/comjnl/27.2.97).
