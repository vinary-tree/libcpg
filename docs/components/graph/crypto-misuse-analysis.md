# Cryptographic API-misuse component

This component is the feature-free implementation behind
`analysis::security::crypto`. It compiles one declarative policy into existing
typestate and taint analyses plus an explicit argument-provenance machine.

## Public surface

| API | Contract |
|---|---|
| `builtin_crypto_rules()` | returns owned cross-language defaults that callers may extend |
| `analyze_builtin_crypto(cpg, resolution, config)` | runs the shipped policy |
| `analyze_crypto_misuse(cpg, resolution, rules, config)` | runs caller-supplied rules |
| `CryptoArgumentRule` | named callee alternatives plus governed zero-based argument positions |
| `CryptoRuleSet` | call-order property, parameter rules, random sources, and forbidden names |
| `CryptoAnalysisConfig` | local call/origin caps plus delegated `IfdsCaps` |
| `CryptoAnalysis` | findings, delegated reports, issues, cap flag, and aggregate completeness |

All types are available with `default = []`. Policy and report types implement
Serde only when the `serde` feature is enabled.

## Data flow

![The component delegates call order and data flow while retaining local parameter evidence](../../diagrams/crypto-misuse-pipeline.svg)

1. Validate custom argument and string patterns.
2. Run the configured `PropertyFsm` through `analyze_typestate`.
3. Compile key argument rules to named taint sinks and typed literal sources.
4. Run the existing interprocedural taint report.
5. Enumerate call sites in source/identifier order.
6. Reconstruct callee paths and inspect only governed argument positions.
7. Trace literal/call origins through typed AST and DFG evidence with a queue.
8. Canonically merge local, taint, and typestate candidates.
9. Expose a clean interpretation only through `is_complete()`.

## Invariants

- The input `CodePropertyGraph` is never mutated.
- Public finding order is `(site, kind, argument_index, rule)`.
- A hardcoded-key finding is backed by `TaintAnalysis`, not an immediate-child
  heuristic.
- Literal contents are absent from `CryptoFinding`.
- A cap or delegated issue cannot become a complete empty result.
- Queue depth, not native call depth, grows with provenance depth.
- Custom rules do not execute code or invoke a parser.

## Complexity

Let $`C`$ be retained call sites and $`Q_j=(V_j,E_j)`$ the provenance subgraph
for queried argument $`j`$. Excluding delegated solver cost, the local phase is:

```math
O\!\left(C\log C + \sum_j (|V_j|+|E_j|)\log |V_j|\right)
```

The implementation caches each `(call, argument_index)` query within a run.
Heap space is $`O(\max_j |V_j|+|F|)`$, where $`F`$ is the retained finding set.

## Extension guidance

Prefer `Pattern::Exact` for stable project APIs and `Pattern::Segment` for a
reviewed terminal method family. A rule can govern multiple argument positions,
but each position should have the same semantic role. Create separate named
rules when evidence should be triaged differently.

When extending secure-random sources, verify that the matched call returns the
random value rather than merely configuring a generator. When extending the
FSM, preserve an absorbing error state and declare every allocation event
explicitly.

See [ADR-0049](../../design/0049-composed-cryptographic-api-misuse.md),
[Theory 39](../../theory/39-cryptographic-api-misuse.md), and
[Usage 22](../../usage/22-cryptographic-api-misuse.md).
