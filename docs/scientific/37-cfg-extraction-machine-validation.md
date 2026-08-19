# Scientific validation 37 — Stack-safe CFG extraction

This ledger tests whether the typed pushdown machine specified by
[ADR-0042](../design/0042-stack-safe-cfg-extraction.md) preserves the former
control-flow graph (CFG) equations exactly while removing abstract-syntax-tree
(AST) depth from the native call stack.

## 1. Falsifiable hypotheses

| ID | Hypothesis | Falsifying observation |
|---|---|---|
| H1 | the machine preserves the former equation's exact edge stream | any edge id, insertion position, endpoint, label, or `CfgEdgeKind` differs from the bounded oracle |
| H2 | every public configuration combination preserves its prior behavior | any of the eight Boolean configurations changes oracle equality |
| H3 | every former handler is represented by the frame algebra | the comprehensive fixture reaches a recursive production handler or differs on block/branch/loop/match/terminator/try/throw/call/sequential flow |
| H4 | malformed sharing and cycles terminate with the former path semantics | arbitrary extra `AstChild` relations hang, overflow, panic, or differ from the bounded oracle |
| H5 | irrelevant storage insertion order cannot perturb CFG construction | reversed non-AST edge insertion changes CFG edge ids, order, endpoints, kinds, entries, or exits |
| H6 | re-extraction is idempotent | a second extraction adds or reclassifies a CFG edge or duplicates an entry/exit |
| H7 | parser-backed depth 100,000 is independent of ordinary native stack size | public construction with CFG enabled overflows a 256 KiB stack or fails its anti-vacuity counts |

## 2. Bounded former-equation oracle

The test-only oracle retains the complete former recursive handler component.
It is safe only because its generated inputs are bounded to three recursive
levels and 64 generated nodes per root. Production code cannot call it because
the methods compile only under `cfg(test)`.

The generated `CfgSpec` language contains plain nodes, nested blocks, `if`
with wrapped or bare `else`, `while`, `for`, infinite `loop`, `match` arms,
`return`, `break`, `continue`, `try`/multiple catches/optional `finally`,
`throw`, resolved calls with ordered arguments, and fallback sequential nodes.
For 192 cases, each machine result is compared to the oracle using the entire
edge vector and both ordered CFG registries:

```math
E_{machine}=E_{oracle}
\land Entry_{machine}=Entry_{oracle}
\land Exit_{machine}=Exit_{oracle}.
```

Equality of `CpgEdge` includes the monotonically allocated `EdgeId`, source,
target, exact edge kind, and optional label. This is stronger than comparing an
unordered topology.

## 3. Handler-complete and malformed examples

One frozen fixture reaches every continuation family in a single graph:
nested `while`/`for`/`loop` contexts own both `break` and `continue`; an
`if` owns an `Else` wrapper; `match` has returning and empty arms; `try` owns a
resolved multi-argument call, thrown value, multiple catches, and `finally`;
plain assignment children exercise the sequential fallback. The fixture runs
under all eight configurations.

A second frozen form adds a deep `AstChild` back edge into the main function.
The production machine and oracle must still emit byte-for-byte-equivalent
edge streams. Independent named examples cover a self/cyclic AST, a node shared
by disjoint branches, nested-loop binding, outside-loop terminators, degenerate
constructs, wrapped returns, all-returning branches, exception disablement,
call disablement, loop-exit retyping, and exact idempotency.

The malformed property adds one arbitrary `AstChild` relation to every
generated graph. Depending on its endpoints, that relation is a duplicate,
self-loop, longer cycle, or second parent. Another 192 cases require exact
oracle equality under every configuration.

## 4. Determinism and conservation

The storage-order property inserts the same irrelevant `DataDependency`
relations in forward and reverse order before extraction. AST edges and their
semantic source order are unchanged. It then requires exact CFG edge-vector,
entry-vector, and exit-vector equality for 192 generated graphs.

Two further 192-case campaigns require:

- a second extraction to preserve the exact CFG edge count; and
- every emitted `Sequential` continuation to remain within loop scope while
  every `LoopExit` crosses a loop boundary and targets a basic-block leader.

The machine asserts its internal value-conservation boundary: one input region
must leave exactly one `CfgExits` value. Rust's exhaustive enum match ensures
that adding a new frame variant cannot silently skip its lifecycle equation.

## 5. Public small-stack experiment

`tests/tree_sitter_stack_safety.rs` constructs valid Rust source with 100,000
nested parenthesized expressions. A thread with a 256 KiB native stack calls
the public `TreeSitterCpgBuilder` with CFG enabled and data-flow extraction
disabled. Anti-vacuity checks require:

- at least 100,000 CPG nodes;
- exactly one CFG entry;
- a nonempty CFG exit set;
- at least 100,000 CFG edges; and
- every `AstChild` target's parent pointer to match its source.

This covers parse, stack-safe tree conversion, default pipeline dispatch, and
the CFG pushdown machine. No stack-size increase, semantic nesting cap, helper
thread escape, or alternate extraction path participates.

## 6. Reproduction

```text
cargo test --lib builder::cfg
cargo test --test tree_sitter_stack_safety --features lang-rust \
  public_mode_a_builder_and_cfg_handle_depth_100_000_on_a_small_native_stack \
  -- --exact
cargo clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --all-features --no-deps
./scripts/verify-docs.sh
```

These focused commands do not replace the campaign's one final full
verification run.

## 7. Interpretation limits

Passing proves equivalence to the former extraction semantics over the bounded
generated language, exact behavior for the handler-complete fixtures, and
tested stack independence through depth 100,000. It does not claim that a
malformed graph has meaningful language semantics; the malformed-input
contract is termination, structural validity of emitted references, and
preservation of the established conservative cycle rule.

## References

1. Reynolds, J. C. (1972). *Definitional Interpreters for Higher-Order Programming Languages.* Proceedings of the ACM Annual Conference. DOI: [10.1145/800194.805852](https://doi.org/10.1145/800194.805852)
2. Danvy, O., & Nielsen, L. R. (2001). *Defunctionalization at Work.* BRICS Report Series 8(23). DOI: [10.7146/brics.v8i23.21684](https://doi.org/10.7146/brics.v8i23.21684)
