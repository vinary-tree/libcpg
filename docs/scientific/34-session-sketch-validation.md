# Scientific validation 34 — Session-protocol sketches

This ledger tests whether normalized Rholang CPG evidence yields deterministic
per-channel endpoint traces and conservative binary-duality verdicts. It does
not test an operational Rholang semantics, recursive session inference,
progress, or deadlock freedom.

## 1. Hypotheses

| ID | Hypothesis | Falsifying observation |
|---|---|---|
| H1 | two equal-length linear endpoint traces with opposite directions and equal known payloads are conformant | the typed oracle is rejected or carries an issue |
| H2 | a direction/order, length, arity, or known-type mismatch is nonconformant | any concrete mismatch is accepted or merely called inconclusive |
| H3 | unknown payloads, non-linear receives, and non-binary participation remain inconclusive unless another concrete mismatch exists | uncertain evidence is guessed conformant/nonconformant, or uncertainty masks a concrete mismatch |
| H4 | payload inference excludes the resolved channel operand and nested event/function syntax | the channel or nested event payload leaks into an action vector |
| H5 | every channel/process/site is input-backed and analysis is read-only | output contains a dangling id or graph counts change |
| H6 | node/edge insertion order cannot change sketches or issues | equivalent generated graphs produce unequal output |
| H7 | the real Rholang frontend supplies sufficient normalized evidence without analyzer source parsing | the Mode-B oracle produces no sketch or depends on source-string inspection |
| H8 | public evidence round-trips under `rholang,serde` | decoded output differs from encoded output |

## 2. Independent generated oracle

For each of 128 cases, the generator chooses a non-empty vector of directions
and shallow payload types. Endpoint $`A`$ receives that vector directly;
endpoint $`B`$ receives the pointwise direction complement with the same type:

```math
d_i^B =
\begin{cases}
\mathtt{receive} & d_i^A=\mathtt{send},\\
\mathtt{send} & d_i^A=\mathtt{receive},
\end{cases}
\qquad
\tau_i^B=\tau_i^A.
```

The oracle is known conformant by construction. The property builds the same
CPG twice with reversed node and edge insertion order and requires equal
`SessionSketch` values, `Conformant`, no issues, and input-backed evidence. It
does not call the production comparison helper to compute the expected result.

## 3. Example-based matrix

| Family | Examples |
|---|---|
| positive | send/receive `Integer`, then receive/send `String`; sync send is still send direction |
| concrete negatives | same-direction step, unequal trace length, unequal payload arity, unequal known type |
| uncertainty | untyped payload, persistent receive, one participating process |
| payload evidence | all nine literal categories, custom named type, identifier-to-typed-definition resolution |
| robustness | nested event boundary, malformed AST cycle, stable sorted evidence, graph immutability |
| compatibility | conditional JSON round-trip for the complete public structures |
| integration | crate-root typed binary oracle and real `new`/send/linear-receive Rholang tree |

The real Rholang example has send and receive in one top-level contract. It is
therefore expected to return `EndpointCardinality` and `Inconclusive`, which
checks that the analyzer does not invent a second binary endpoint from parallel
syntax alone.

## 4. Feature and release matrix

| Configuration | Evidence |
|---|---|
| `--no-default-features` | gated module and exports disappear without regressing the feature-free crate |
| `--features rholang` | unit/property and public/real-frontend integration suites execute |
| `--features rholang,serde` | public result round-trip executes |
| maximal and `--all-features` | interaction with all language and analysis features remains warning-free |

Strict Clippy, warning-free rustdoc, diagram rendering, documentation checks,
formatting, diff checks, and staged pgmcp bug-gate complete the release evidence.

## 5. Interpretation limits

Passing this suite supports conformance to the documented shallow, ordered CPG
abstraction. It cannot establish runtime pairing of dynamically created channel
instances, scheduling order, recursive protocol fidelity, global/multiparty
consistency, progress, or deadlock freedom. Those claims require independent
operational or type-theoretic oracles.

## References

1. Honda, K., Vasconcelos, V. T., Kubo, M. (1998). *Language Primitives and
   Type Discipline for Structured Communication-Based Programming.* DOI:
   [10.1007/BFb0053567](https://doi.org/10.1007/BFb0053567).
2. Mordido, A., Pérez, J. A. (2025). *Deadlock-free Context-free Session
   Types.* DOI:
   [10.48550/arXiv.2506.20356](https://doi.org/10.48550/arXiv.2506.20356).
