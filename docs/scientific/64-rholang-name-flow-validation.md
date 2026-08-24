# Scientific validation 64 -- Rholang NameFlow and level analysis

## Claims under test

The implementation claims that normalized fresh-name, definition, reflection,
and communication evidence yields deterministic NameFlow provenance; that
payload scope extrusion reaches receive binders; that receive-before-send
cycles are found through the shared strongly connected component (SCC) engine;
that acyclic dependencies receive canonical strict ranks; that malformed and
capped inputs cannot support a clean verdict; and that logical depth does not
become native call depth.

These tests validate the finite CPG model. They do not validate the complete
Rholang operational semantics or prove whole-program deadlock freedom.

## Example oracles

| Fixture | Independent expected result |
|---|---|
| fresh binder beneath restriction | one `Creates` relation |
| fresh name under quote and drop | exact `Quotes` and `Drops` relations |
| carrier sends a fresh payload to receive binder | payload origin reaches binder and later typed use |
| process receives `left`, then sends `right` | dependency `left -> right` |
| two processes impose `left -> right` and `right -> left` | two cyclic channels, `None` levels, precise SCC witnesses |
| processes send before receiving | no dependency, finite level zero, no witness |
| typed DFG definition with unrelated spelling | identity still resolves without text matching |

## Property oracles

The insertion-permutation property reverses the complete AST-edge insertion
order of a fixed-ID cyclic program and requires equal `NameFlowAnalysis` and
`DeadlockFreedomAnalysis` values. This tests canonical ordering independent of
graph construction history.

The generated-chain property constructs between two and eleven fresh channels.
Each process receives on channel $`i`$ and then sends on $`i+1`$. The
independent oracle requires no cycle, equal capability/obligation values for
each channel, and strictly increasing ranks across every adjacent pair.

## Malformed and cap oracles

- zero origin capacity produces `OriginAssignmentLimitExceeded` and an
  incomplete report;
- conflicting typed definitions produce `AmbiguousDefinition`;
- missing restrictions and unresolved channels are explicit issues;
- a payload/binder arity difference is retained as `PayloadArityMismatch`;
- an unknown overlay target fails before any edge is appended; and
- successful repeated materialization adds zero edges.

With feature `serde`, the full qualified report round-trips and reserializes to
the same canonical bytes.

## Integration and stack-safety oracle

The public integration suite parses a real Rholang mutual-wait program through
the caller-owned grammar and Mode B. It requires the same cyclic channel
evidence as the hand-built independent oracle. A separate real public overlay
case verifies graph queries, idempotency, statistics, and inferred levels.

The depth test constructs 100,000 logical AST levels and runs, on a 256 KiB
native stack:

1. NameFlow origin propagation;
2. deadlock-level inference and iterative SCC decomposition;
3. transactional overlay materialization;
4. NameFlow graph queries and statistics; and
5. explicit report and CPG destruction.

No input-shaped Rust recursion participates in that path.

## Focused evidence at implementation checkpoint

Every command ran with `CARGO_BUILD_JOBS=1` inside a user systemd scope with
`MemoryMax=4G`, `MemorySwapMax=0`, and `TasksMax=256`.

| Command | Result |
|---|---|
| focused `--features rholang` unit and property tests | 9 passed |
| all-feature NameFlow, property, and Serde tests | 10 passed |
| `cargo test --features rholang --test name_flow_integration` | 3 passed, including the real parser and depth-100k case |
| deterministic ML-export unit and integration tests | 8 passed after updating the appended feature offset |
| `cargo clippy --all-targets --all-features -- -D warnings` | pass |
| `cargo test --features rholang` | pass, including the existing integration matrix and doctests |

## Final repository evidence

The first repository-wide verifier stopped at its first gate because one
receive-registration guard differed from canonical rustfmt layout. Commit
`67daa43` applied only that formatter output; the clean rerun then completed
the entire matrix under the same bounded systemd scope.

| Gate | Result |
|---|---|
| `./scripts/verify.sh` | pass: formatting; default, maximal, and all-feature test matrices; examples; warnings-denied Clippy; rustdoc and doctests; documentation; and diff audit |
| documentation links | 4,819 total; 962 unique; 4,054 checked successfully; zero errors; 765 explicitly excluded |
| DOI syntax and resolution | 119 unique canonical DOI links passed syntax policy and all 119 resolved through `doi.org` |
| reproducible diagrams | 148 sources rendered headlessly twice with byte-identical committed SVGs |
| `pgmcp bug-gate` | no open bug anchored to the 851 files changed from epic base `71ab1a7cbe3fd9710b7c98626e994bd602e5c600` |

The verification commands used `MemoryMax=4G`, `MemorySwapMax=0`,
`TasksMax=256`, and `CARGO_BUILD_JOBS=1`. The three new SVGs were rasterized
headlessly onto a white background for visual inspection; no GUI process was
opened.

## Reproduction

```text
systemd-run --user --scope \
  -p MemoryMax=4G -p MemorySwapMax=0 -p TasksMax=256 \
  env CARGO_BUILD_JOBS=1 cargo test --features rholang \
  --test name_flow_integration
```

Do not compile in `/tmp` on systems where it is memory-backed.

## References

1. Kobayashi, N. (2006). DOI:
   [10.1007/11817949_16](https://doi.org/10.1007/11817949_16).
2. Kobayashi, N., Sangiorgi, D. (2010). DOI:
   [10.1145/1745312.1745313](https://doi.org/10.1145/1745312.1745313).
3. Meredith, L. G., Radestock, M. (2005). DOI:
   [10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016).
