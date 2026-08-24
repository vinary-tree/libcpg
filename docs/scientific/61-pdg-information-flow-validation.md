# Scientific validation 61 -- PDG information-flow evidence

This record tests whether libcpg's feature-free information-flow and
constant-time queries exactly classify represented dependence paths while
preserving determinism, graph immutability, and constant native call depth.

## 1. Falsifiable hypotheses

| ID | Claim | Failure evidence |
|---|---|---|
| H1 | Every represented High-to-Low data/summary path is reported as explicit. | A reachable pair or summary edge is absent or misclassified. |
| H2 | Every represented High-to-Low path containing control dependence is reported as implicit independently of an explicit path. | A path class suppresses the other or a witness lacks the required class. |
| H3 | Declassifiers are hard cut vertices and missing labels are total. | A witness enters a cut, or an out-of-graph label changes output or panics. |
| H4 | Every reachable conditional CFG edge and `IndexAccess` is reported exactly once per distinct mechanism and secret. | A site is missing, duplicated, or assigned to the wrong secret/target/kind. |
| H5 | Equal stable graphs produce byte-for-value equal findings across insertion permutations. | Finding order, classification, or witness differs. |
| H6 | Every witness is a contiguous replay of represented dependence edges. | A source/target discontinuity or invented edge occurs. |
| H7 | Analysis and complete witness lifecycle have constant native call depth. | A 100,001-node path fails on a 256 KiB worker stack. |
| H8 | Public and parser-built calls are read-only. | Node/edge counts change or repeated output differs. |

## 2. Independent oracles

The non-interference property test generates up to seven nodes, forty
data/control-dependence edges, arbitrary High/Low/declassifier masks, and both
edge insertion orders. An independent iterative fixed-point repeatedly applies
every edge until no `(node, has_control_step)` bit changes. It compares the
complete source/sink-to-path-class map with `noninterference`.

The constant-time property independently generates up to twenty-four typed
conditional CFG edges, an index-node mask, and a secret mask. A separate
single-state fixed point computes dependence reachability. A sorted map of
generated branch mechanisms and index nodes then produces the exact expected
`(secret, site, kind)` vector. The production witnesses are additionally
replayed against the CPG's represented dependence edges.

Both properties run 128 deterministic cases and compare forward with reverse
insertion. Neither oracle calls the production search or witness
reconstruction.

## 3. Exact examples and integration

Seven focused unit tests cover:

- distinct explicit and implicit witnesses for one result set;
- `ExplicitAndImplicit` when both path classes connect one pair;
- a declassification cut plus out-of-graph source, sink, declassifier, and
  secret labels;
- labeled SDG summary dependence classified as explicit;
- separate true-edge, false-edge, and index findings;
- both independent 128-case generated oracles;
- a 100,001-node data-dependence chain whose High-to-Low and secret-to-index
  witnesses each contain 100,000 steps on a 256 KiB stack.

Two public integration tests exercise crate-root exports. The feature-free
fixture combines explicit, implicit, declassified, branch, index, missing-label,
determinism, witness, and immutable-graph behavior. The `lang-rust` fixture
parses a function, builds all PDGs, and selects real `Parameter`,
`DataDependence`, conditional CFG, `ControlDependence`, and `IndexAccess`
nodes before calling both analyses.

## 4. Focused results

The all-feature unit/property/deep-stack run passed all seven tests, including
256 generated oracle cases. The public/parser integration run passed both
tests. Strict all-target/all-feature Clippy, formatting, staged-diff integrity,
and the unfinished-marker scan passed.

The enlarged all-feature unit-test binary was first linked with four Cargo
jobs inside a 4 GiB cgroup. The scope reached exactly `MemoryMax=4G` and the
kernel safely killed it twice without exhausting the host. Repeating with one
Cargo job completed under the same `MemoryMax=4G`, `MemorySwapMax=0` ceiling.
One job is therefore the evidence-backed validation setting for this binary;
the semantic tests themselves completed in less than one second after linking.

## 5. Reproduction

```text
systemd-run --user --scope \
  -p MemoryMax=4G -p MemorySwapMax=0 -p TasksMax=256 \
  env CARGO_BUILD_JOBS=1 \
  cargo test -j 1 --all-features analysis::security::ifc::tests

systemd-run --user --scope \
  -p MemoryMax=4G -p MemorySwapMax=0 -p TasksMax=256 \
  env CARGO_BUILD_JOBS=1 \
  cargo test -j 1 --all-features --test ifc_integration

systemd-run --user --scope \
  -p MemoryMax=4G -p MemorySwapMax=0 -p TasksMax=256 \
  env CARGO_BUILD_JOBS=1 \
  cargo clippy -j 1 --all-targets --all-features -- -D warnings
```

Validation output is captured under disk-backed `target/verification`,
inspected, and removed by exact path. No build or dependency output is placed
under `/tmp`.

## 6. Interpretation boundary

H1--H8 are supported for the represented CPG relation. The tests do not prove
source-language or system-level non-interference, completeness of PDG/SDG
construction, path feasibility, or microarchitectural constant time. Those are
explicitly excluded claims, not untested assumptions.

## References

1. Hammer, C., Snelting, G. (2009). *Flow-sensitive, context-sensitive, and
   object-sensitive information flow control based on program dependence
   graphs.* DOI:
   [10.1007/s10207-009-0086-1](https://doi.org/10.1007/s10207-009-0086-1).
2. Barthe, G., Betarte, G., Campo, J. D., Luna, C., Pichardie, D. (2014).
   *System-level non-interference for constant-time cryptography.* DOI:
   [10.1145/2660267.2660283](https://doi.org/10.1145/2660267.2660283).
