# Scientific validation 45 — Secret flow and access policy

This plan separates **implementation correctness**—conformance to libcpg's
documented represented relation—from **external validity**—whether candidates
predict real secret leakage or authorization defects in deployed systems.

## 1. Hypotheses

| ID | Hypothesis | Oracle |
|---|---|---|
| H1 | a high-entropy typed literal reaching authentication is reported | direct source-to-positional-sink CPG |
| H2 | a low-entropy literal under a hinted binding reaching a network sink is reported | binding + DFG + sink fixture |
| H3 | ordinary low-entropy literals remain clean | paired unhinted fixture |
| H4 | positional policy excludes an otherwise qualifying argument | two-argument crypto-key fixture |
| H5 | four guarded of five identical mutations report only the fifth | exact `4/5` population oracle |
| H6 | unrelated peer checks are not relabeled as access control | quota-guard negative control |
| H7 | reports are deterministic | two 128-case repeated-run property campaigns |
| H8 | depth does not consume native stack | 100,000-node binding/fingerprint tapes on 64 KiB |
| H9 | invalid/capped evidence cannot become complete | policy, literal, context, and population issue fixtures |
| H10 | public parser composition preserves both relations | Rust AST/CFG/DFG/PDG corpus |

## 2. Secret-flow example matrix

The example suite distinguishes qualifier and propagation dimensions:

| Qualifier | Reaches governed sink | Expected |
|---|---:|---|
| high entropy | yes | finding with `HighEntropy` |
| typed binding hint | yes | finding with `NameHint` |
| neither | yes | no finding |
| high entropy | no | no finding |
| high entropy | wrong argument position | no finding |

Fixtures assert concrete source/sink IDs, family, position, evidence variant,
witness, completeness, and unchanged node/edge counts. No assertion needs to
copy the secret into a finding.

## 3. Access-policy differential structure

The exact oracle constructs five sites with one `ApiIdentity`. Four carry the
same normalized `authz::authorize` guard; the fifth has none. At threshold
$`4/5`$, only the fifth site is expected, with all five peers and four concrete
guard witnesses.

A paired population replaces the guard with `quota::within_limit`. The shared
missing-check engine still infers the generic convention, but the authZ
specialization must return no finding. This differential proves the new filter
is semantic, not a projection of every missing check.

## 4. Property-based campaigns

The entropy campaign generates 128 alphanumeric strings through length 96 and
requires exact repeated measurement and the mathematical bound
$`0\le 1000H(s)\le 8000`$.

The secret report campaign generates strings through length 48, forces a
permissive threshold, and requires structural equality of repeated aggregate
reports. The access campaign generates one through seven guarded peers plus one
unguarded peer, sets the exact generated threshold, and likewise requires full
report equality.

These properties cover ordering, integer threshold decisions, witness
canonicalization, issue translation, and delegated report stability. They do
not estimate detector recall or precision.

## 5. Stack-safety experiments

On a 64-KiB native stack:

- J3 walks 100,000 nested typed wrapper nodes to a binding hint using explicit
  parent-loop state;
- J4 scans a 100,001-node flat guard fingerprint containing one terminal typed
  authZ call.

The shared taint and missing-check engines have their own independent deep
worklist/tape tests. No experiment enlarges the stack, catches overflow in a
subprocess, or truncates the semantic input silently.

## 6. Integration levels

1. Feature-free unit tests isolate policy, qualifier, filter, cap, and issue semantics.
2. Serde tests round-trip both policies and aggregate reports.
3. Feature-free integration consumes only crate-root exports and composes J3/J4.
4. `lang-rust` integration builds a real CPG, adds PDG evidence, and repeats both oracles.
5. The executable example verifies documented behavior and value-minimizing output.
6. Repository gates cover formatting, strict Clippy, rustdoc, links, diagrams,
   default/all-feature builds, and source-policy checks.

## 7. External-validity protocol

A future empirical study should use a versioned multilingual corpus with:

- reviewed live/revoked/test/non-secret labels kept in a protected environment;
- explicit API and framework policy versions;
- separate source-only and source-to-sink measurements;
- missing-auth ground truth derived from authoritative application policy;
- precision, recall, confidence intervals, and per-family error analysis; and
- blinded comparison against established secret scanners and access-control
  review baselines.

Passing current tests supports implementation correctness only. It does not
establish production precision, recall, severity calibration, or policy
completeness.

## References

1. Meli, M., McNiece, M. R., Reaves, B. (2019). *How Bad Can It Git?
   Characterizing Secret Leakage in Public GitHub Repositories.* DOI:
   [10.14722/ndss.2019.23418](https://doi.org/10.14722/ndss.2019.23418).
2. Yamaguchi, F., Wressnegger, C., Gascon, H., Rieck, K. (2013). *Chucky:
   Exposing Missing Checks in Source Code for Vulnerability Discovery.* DOI:
   [10.1145/2508859.2516665](https://doi.org/10.1145/2508859.2516665).
3. Shannon, C. E. (1948). *A Mathematical Theory of Communication.* DOI:
   [10.1002/j.1538-7305.1948.tb01338](https://doi.org/10.1002/j.1538-7305.1948.tb01338).
