# Scientific validation 63 -- compositional heap analysis

## Claims under test

The implementation claims that typed allocation, null, dereference, release,
CFG, and resolved-call evidence yields deterministic compositional summaries;
that required violations are detected; that safe represented lifetimes remain
clean; that malformed/capped inputs cannot support a clean verdict; and that
logical depth does not become native call depth.

These tests validate the finite modeled semantics. They do not validate a
frontend's completeness, concrete aliasing, exploitability, or whole-program
memory safety.

## Example oracles

| Fixture | Independent expected result |
|---|---|
| null literal → typed dereference | one `NullDereference` with `[null, dereference]` witness |
| allocation → exit | one live-exit `Leak` |
| allocation → `free` → dereference | `UseAfterFree` with allocation/release/dereference witness |
| allocation → `free` → exit | no violation and an `Invalid(Allocation)` postcondition |
| caller passes null to callee dereference parameter | callee anti-frame substitutes to caller null finding |
| custom `destroy` release | default model leaks; extended exact model is clean |

The lifetime oracle is hand computation over the three finite may states
`Absent`, `Live`, and `Freed`. The interprocedural oracle independently maps
formal position zero to the first source-ordered actual argument.

## Property and metamorphic oracles

The generated straight-line property varies neutral CFG width through 96
nodes. Every allocation/release/exit sequence must remain complete, clean, and
bit-for-bit deterministic across two analyses.

The insertion-permutation property rebuilds one fixed-ID AST/CFG/DFG in forward
or reverse edge order and requires equal complete `HeapAnalysis` values. Its
first run found that `call_arguments` removed a child before canonical source
sorting. The verifier then exposed the dual equal-range case: sorting by stable
node ID before callee identification can misclassify an argument in a
hand-built or deserialized resolved call. The root rule now separates the two
operations. A unique direct identifier-definition matching the resolved target
identifies callee syntax; parser source order is the fallback. Only after
removing that exact node are the remaining arguments sorted. The minimized heap
regression is committed at `proptest-regressions/analysis/heap/tests.txt`, and
a generated builder property varies arity and AST-edge insertion direction
while all child ranges are equal.

## Malformed, cap, and recursive oracles

- zero work budget returns a capped, incomplete report with no summary map;
- a control-flow SCC decomposition passed as a call graph reports
  `IfdsIssue::NotCallGraph`;
- a missing overlay source returns `HeapOverlayError::UnknownSource` before any
  edge is added;
- repeated valid materialization adds zero edges; and
- a self-recursive must-invalidate summary takes multiple SCC rounds and is
  explicitly partial.

The Serde oracle round-trips `HeapAnalysis`, compares the complete value, then
requires byte-identical canonical reserialization.

## Stack-safety oracle

The public integration test creates 100,000 nested AST wrapper nodes and a
matching 100,000-edge CFG between one allocation and one release. A thread with
a 256 KiB native stack executes:

1. call-graph SCC construction;
2. function-scope discovery;
3. heap-origin fixed point;
4. CFG projection and lifetime flow;
5. summary/finding projection;
6. overlay materialization and idempotent repetition; and
7. explicit analysis and CPG drop.

The test completes cleanly. This exercises source-shaped state through the
real public pipeline rather than testing an isolated loop.

## Focused evidence at implementation checkpoint

Every command ran with `CARGO_BUILD_JOBS=1` inside a user systemd scope with
`MemoryMax=4G`, `MemorySwapMax=0`, and `TasksMax=256`.

| Command | Result |
|---|---|
| `cargo check --all-targets` | pass |
| `cargo test analysis::heap --lib` | 9 passed |
| `cargo test --test heap_integration` | 2 passed; depth-100k case included |
| `cargo test --all-features analysis::heap --lib` | 10 passed, including Serde and recursive partiality |
| `cargo clippy --all-targets --all-features -- -D warnings` | pass |
| `cargo fmt --all -- --check` | pass |

Repository-wide verifier, documentation-link, reproducible-diagram, live-DOI,
and bug-gate evidence is recorded in the final FT.4 evidence checkpoint after
the documentation tranche is rendered and audited.

## Reproduction

```text
systemd-run --user --scope \
  -p MemoryMax=4G -p MemorySwapMax=0 -p TasksMax=256 \
  env CARGO_BUILD_JOBS=1 cargo test --test heap_integration
```

Run the remaining focused commands by replacing the trailing Cargo arguments.
Do not compile in `/tmp`; this environment mounts it as memory-backed storage.

## References

1. Calcagno, C., Distefano, D., O'Hearn, P. W., Yang, H. (2011).
   DOI: [10.1145/2049697.2049700](https://doi.org/10.1145/2049697.2049700).
2. Calcagno, C., Distefano, D., O'Hearn, P. W., Yang, H. (2009).
   DOI: [10.1145/1480881.1480917](https://doi.org/10.1145/1480881.1480917).
