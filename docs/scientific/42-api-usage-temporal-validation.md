# Scientific validation 42 — API-usage and temporal evidence

This ledger makes T6.5 falsifiable. It separates semantic tests from controlled
performance observations and names the exact oracle behind each claim.
An **oracle** is an independently stated expected result; a benchmark is not a
correctness oracle merely because it terminates.

## 1. Hypotheses

| ID | Falsifiable hypothesis |
|---|---|
| H1 | If exactly 19 of 20 complete `lock` occurrences can reach `unlock`, mining at threshold 19/20 emits that exact pattern and flags only the twentieth occurrence. |
| H2 | Exact argument, receiver, and control shapes yield the corresponding fragment/deviation kinds without source parsing. |
| H3 | A caller FSM reports a missing release as one non-accepting exit and a second release as one forbidden transition with its typestate witness. |
| H4 | Repeated runs and CFG-edge insertion permutations produce equal reports. |
| H5 | Malformed evidence and every operational cap surface as invalid input or typed incomplete evidence; no capped site contributes partial population facts. |
| H6 | CFG depth 100,000 does not consume proportional native call-stack depth. |
| H7 | Public feature-free, serde, and parser-backed Rust consumers observe the same documented semantics. |

## 2. Example-based oracle matrix

The unit module `src/analysis/bugs/api_mining/tests.rs` contains fourteen tests
when `serde` is enabled.

| Scenario | Deciding assertions |
|---|---|
| 19/20 call order | support equals `PopulationFraction::new(19, 20)`; one `MissingFollowingCall`; site/witness equal the omitted anchor |
| argument mismatch | one Boolean `true` shape has 19/20 support; only the `false` site deviates |
| release omission | every unreleased allocation yields `NonAcceptingExit(acquired)` |
| double release | second `unlock` yields `ForbiddenTransition(released, unlock)` and witness contains allocation plus both events |
| graph schema | typed parameter, selection, and order edges exist; graph passes iterative acyclicity |
| method receiver | typed member syntax yields an `Identifier` receiver pattern |
| malformed/capped evidence | missing identity, CFG cap, graph cap, invalid thresholds/budgets/names are explicit |
| inventory overflow | `CallSiteCap` leaves graphs, patterns, findings, and admitted sites empty |
| serialization | full config/report round-trip under `serde` |

These tests assert the full qualified report rather than only the positive
convenience projection.

## 3. Property-based oracles

Two tests run 128 generated cases each.

### P1 — independent population oracle

Generate a Boolean vector whose `true` entries are a strict majority and whose
`false` entries represent omitted `unlock` calls. Let:

```math
S = \{i \mid b_i = \mathrm{false}\}
```

The independently expected finding-site set is the lock anchors at indices in
$`S`$. The test configures the exact generated support fraction and asserts set
equality with mined `MissingFollowingCall` sites.

### P2 — determinism under edge permutation

Build equal CPGs with forward and reversed CFG-edge insertion order. Analyze the
forward graph twice and the reversed graph once. The test asserts:

```math
R_{\mathrm{forward},1}
=
R_{\mathrm{forward},2}
=
R_{\mathrm{reverse}}
```

It also checks every produced AUG with the public iterative acyclicity method.

## 4. Public integration evidence

`tests/api_usage_mining_integration.rs` exercises only exported libcpg APIs.
The feature-free test combines the 19/20 usage population with explicit DFG
links from each successful `unlock` to its `lock` allocation. One run must
produce:

- 39 complete usage graphs: 20 `lock` plus 19 `unlock` anchors;
- one exact 19/20 following-call pattern;
- one missing-following-call advisory at the twentieth lock;
- one non-accepting temporal exit for that same object;
- no CPG node/edge-count mutation;
- equality between the convenience projection and full-report findings.

With `lang-rust`, a generated source corpus is parsed through
`TreeSitterCpgBuilder`. The test asserts that only function `peer_19` violates
the learned `lock`→`unlock` pattern. This validates parser, AST, CFG, call
identity, mining, and public-report integration without adding parser logic to
the detector.

## 5. Stack-safety experiment

The unit gate constructs one function with a 100,000-node sequential CFG after
an anchor call, then runs the public analyzer on a thread whose native stack is
64 KiB. It asserts a complete one-graph report and successful acyclicity.

The test falsifies input-depth native recursion in:

- per-site CFG reachability;
- centered graph construction;
- output ordering;
- graph acyclicity checking;
- destruction of returned public values.

It does not establish a universal memory bound; heap work is separately limited
by configuration.

## 6. Controlled benchmark

`benches/api_usage_mining.rs` creates 20- and 100-peer corpora. Exactly every
twentieth peer omits `unlock`; every other peer carries explicit DFG object-flow
evidence. Before Criterion times anything, the benchmark asserts:

```math
|\operatorname{usage\ findings}| =
|\operatorname{temporal\ findings}| =
\frac{\operatorname{population}}{20}
```

One local release-mode observation on 2026-08-19 produced:

| Population | Measured interval | Throughput interval |
|---:|---:|---:|
| 20 | 181.78–189.69 µs | 105.44–110.03 Kpeer/s |
| 100 | 1.4982–1.5009 ms | 66.625–66.748 Kpeer/s |

These values are a smoke observation, not a portable performance promise. CPU,
load, compiler, allocator, and repository revision affect them. The benchmark's
durable scientific value is its named fixture and pre-timing semantic oracle.

## 7. Reproduction

```text
# Feature-free unit and public integration semantics:
cargo test --no-default-features analysis::bugs::api_mining::tests
cargo test --no-default-features --test api_usage_mining_integration

# Parser-backed public integration:
cargo test --no-default-features --features lang-rust \
  --test api_usage_mining_integration

# Serialization contract:
cargo test --no-default-features --features serde \
  analysis::bugs::api_mining::tests

# Executable example and oracle-guarded benchmark:
cargo run --no-default-features --example api_usage_protocols
cargo bench --no-default-features --bench api_usage_mining

# Strict static and documentation checks:
cargo clippy --no-default-features --all-targets -- -D warnings
cargo doc --all-features --no-deps
./scripts/verify-docs.sh
```

The leaf gate additionally runs the repository's complete `./scripts/verify.sh`
matrix and `pgmcp bug-gate` over the exact T6.5 diff.

## 8. Interpretation boundary

Passing H1–H7 proves conformance to the represented CPG, configured thresholds,
FSMs, and resource budgets. It does not prove runtime path feasibility, the
correctness of a dominant convention, complete dynamic dispatch/reflection,
absence of frontend omissions, or safety of an automated repair.

See [Theory 37](../theory/37-api-usage-graphs-and-temporal-properties.md), the
[security boundary](../security/12-api-usage-protocol-mining.md), and
[ADR-0047](../design/0047-evidence-bounded-api-usage-and-temporal-mining.md).

## References

1. Nielebock, S., Blockhaus, P., Krüger, J., Ortmeier, F. (2024).
   *ASAP-Repair: API-Specific Automated Program Repair Based on API Usage
   Graphs.* DOI:
   [10.1145/3643788.3648011](https://doi.org/10.1145/3643788.3648011).
2. Song, Y., Gao, X., Li, W., Chin, W.-N., Roychoudhury, A. (2024).
   *ProveNFix: Temporal Property-Guided Program Repair.* DOI:
   [10.1145/3643737](https://doi.org/10.1145/3643737).
