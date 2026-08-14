# Scientific validation 31 — Channel topology and linearity

This page states falsifiable claims for `analysis::concurrency`, identifies the
independent oracle for each claim, and separates normalization correctness from
runtime scheduling claims.

## 1. Hypotheses

| Hypothesis | Discriminating experiment | Oracle |
|---|---|---|
| a producer-less linear receive is blocked | one `Recv`, no send | exactly `BlockedRecv` |
| a receiver-less send is orphaned | one `Send`, no receiver | exactly `OrphanSend` |
| one send and one linear receive match cleanly | two processes, one channel definition | empty report |
| persistent receive never produces starvation | one `RecvPersistent`, no producer | empty report |
| peek observes without consuming | one send plus one `Peek` | no orphan and no contention |
| two producers contend for one linear receive | two send sites plus one `Recv` | one `NondeterministicConsumption` |
| two linear receivers contend | one send plus two `Recv` sites | one contention finding |
| spelling does not define identity | two differently named uses linked to one variable | same `ChannelId` and clean match |
| missing links do not trigger name guessing | same spelling, no typed relation | event retained with `None` |
| nested definitions do not leak into a process | event under nested function | absent from parent event vector |
| analysis is out of graph | record counts before/after | identical node and edge counts |
| all evidence ids are valid | inspect every finding/event id | every id belongs to input CPG |
| Rholang syntax is normalized by Tree-sitter | parse all seven forms through Mode B | exact `MsgKind` coverage and one channel id |
| serialization preserves semantics | `serde` round trips | structural equality |

## 2. Independent classification oracle

For generated producer count $`p`$ and linear-receiver count $`r`$ on one
channel, the test computes expected booleans directly from:

```math
\begin{aligned}
O &= (p>0)\land(r=0),\\
B &= (r>0)\land(p=0),\\
N &= (r>0)\land(p\ge 2\lor r\ge 2).
\end{aligned}
```

The implementation output is then reduced to finding-kind membership and
compared with $`O`$, $`B`$, and $`N`$. This is independent of the internal tally
representation and exercises the overlapping blocked-plus-contention case
when $`p=0`$ and $`r\ge2`$.

## 3. Property-based campaigns

Three 128-case properties are used:

1. reverse every `AstChild`, `DefUse`, and `Reference` insertion and require an
   identical `BTreeMap<NodeId, Vec<ChannelEvent>>`;
2. reverse a multi-channel graph's edge insertion and require byte-for-byte
   equal ordered `ChannelFinding` vectors; and
3. generate $`0\le p,r<5`$ and compare classification with the equations above.

The first two properties discriminate explicit canonicalization from accidental
petgraph adjacency order. The third covers 25 structural count classes on every
campaign and shrinks failures to the smallest counterexample.

## 4. Real-grammar integration oracle

`tests/concurrency_integration.rs`, under `rholang`, parses one contract through
the caller-supplied `rholang-tree-sitter` grammar and hands the tree to
`TreeSitterCpgBuilder::build_from_tree`. The fixture contains:

```rholang
new c in {
  c!(1) | c!?(2). |
  for (@x <- c) { Nil } |
  for (@y <= c) { Nil } |
  for (@z <<- c) { Nil } |
  select { @w <- c => Nil }
}
```

The oracle requires every `MsgKind` variant to occur and every channel-using
event to resolve to the same new-bound `Variable`. The analyzer never sees this
source string; it consumes only the constructed CPG. This test would fail if
the frontend erased one operator distinction, if DFG extraction lost `c`, or if
the extractor guessed identity from text.

## 5. Coverage matrix

| Dimension | Evidence |
|---|---|
| normal | matched send/receive and all Rholang forms |
| negative | orphan, blocked, unresolved identity |
| consumption | persistent and peek exclusions |
| contention | multiple producers and receivers |
| identity | `definition`, `DefUse`, and `Reference` paths |
| scope | top-level process and nested-function exclusion |
| determinism | reverse-insertion generated equality |
| mutation | exact node/edge counts before and after |
| integration | crate-root hand graph and real Mode-B parse |
| serialization | message metadata, events, and findings under `serde` |

## 6. Claim boundary

These experiments validate graph extraction and structural predicates. They do
not validate runtime reachability, fairness, message-pattern satisfiability,
guard truth, or whole-program channel aliasing. Consequently, a positive result
is a review candidate and an empty result is not a liveness proof.

## 7. Reproduction

```sh
cargo test --no-default-features analysis::concurrency
cargo test --no-default-features --test concurrency_integration
cargo test --no-default-features --features rholang --test concurrency_integration
cargo test --no-default-features --features serde analysis::concurrency
cargo clippy --no-default-features --all-targets -- -D warnings
```

## References

1. Meredith, L. G., Radestock, M. (2005). *A Reflective Higher-order Calculus.*
   DOI:
   [10.1016/j.entcs.2005.05.016](https://doi.org/10.1016/j.entcs.2005.05.016).
