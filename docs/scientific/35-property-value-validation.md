# Scientific validation 35 — Flat property values and CPV1

This ledger tests whether a logically recursive `PropertyValue` retains its
observable semantics while every owned lifecycle and persistence operation uses
flat heap state. The mechanism and migration are specified in
[ADR-0040](../design/0040-flat-property-value-arena.md).

## 1. Hypotheses

| ID | Hypothesis | Falsifying observation |
|---|---|---|
| H1 | flat views preserve ordered string, integer, unsigned, boolean, float, list, and null semantics | a bounded recursive oracle and the flat value disagree |
| H2 | CPV1 is canonical and bit-stable for one physical arena | encoding the same arena changes bytes or decode/re-encode changes bytes |
| H3 | each invalid tag/reference/order/parent/range/UTF-8/trailing shape is rejected | malformed input returns a value or panics |
| H4 | node, edge, string-byte, encoded-byte, and work caps are independent and monotone | an exact cap fails, a one-smaller cap passes, or the wrong dimension is reported |
| H5 | lifecycle and codec native call depth is independent of logical depth | depth 100,000 overflows a 256 KiB thread stack |
| H6 | Serde embeds CPV1 without recreating recursive visitor descent | the 100,000-depth JSON carrier overflows or changes the value |
| H7 | pgmcp's real cache uses the same invariant rather than a duplicate arena | the v2 cache fails the deep round trip or retains the old converter |

## 2. Oracle and generated corpus

The bounded test-only `OracleValue` is the former recursive equation. Its
generator chooses all seven logical forms and recursively generates ordered
lists up to six levels. For 256 cases, the suite compares the recursive debug
equation, flat clone/equality, CPV1 decode, and decode/re-encode bytes. Floats
are finite in this oracle so the expected equality relation is reflexive;
separate examples pin NaN and signed-zero behavior.

The cap property measures one generated value, sets all five limits exactly to
that observation, and requires encoding and decoding to succeed. For each
nonzero dimension it subtracts one only from that dimension and requires a
typed `LimitExceeded` naming the same dimension. This is an exact monotonicity
experiment, not a sampled “large enough” limit.

## 3. Example and adversarial matrix

| Family | Evidence |
|---|---|
| logical views | every scalar, nested and empty lists, nested null, source-order iteration |
| compatibility | historical constructor spellings and enum-shaped debug output |
| floating point | negative zero bits, positive zero bits, infinity, NaN payload identity, old `f64` equality |
| canonical bytes | one fixed CPV1 golden and exact re-encode identity |
| malformed bytes | prefix/version, truncation, unknown tag, invalid bool, invalid UTF-8, forward/self reference, redundant root null, trailing bytes |
| canonical tree | child order, one parent per non-root node, final root, edge-range partition |
| resources | exact and one-smaller limits for nodes, edges, strings, encoded bytes, work |
| lifecycle | construct, borrow, clone, equality, debug, validate, encode, decode, Serde, and drop at depth 100,000 |
| integration | pgmcp cache envelope v2 with `property-cpv1` signature on a 256 KiB stack |

## 4. Resource interpretation

For node tape $`N`$ and edge tape $`E`$, validation performs
$`|N|+|E|`$ bounded visits and uses a parent-count vector of $`|N|`$ cells.
The deep unary fixture has $`|N|=100001`$ and $`|E|=100000`$. Its anti-vacuity
checks count exactly 100,000 list views and one terminal null. Width and depth
are represented in heap tapes; neither becomes native call depth.

## 5. Reproduction commands

```text
cargo test --lib --features serde graph::property_value::tests
cargo test --test property_value_stack_safety
cargo test --test property_value_stack_safety --features serde
cargo clippy --tests --features serde -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --features serde
```

In the pgmcp feature worktree:

```text
cargo test --lib cpg::cache::tests
```

These focused commands do not replace the epic's final all-feature verification
campaign. Their role is to isolate semantic, structural, malformed-input, and
native-stack failures at the property boundary.

## 6. Interpretation limits

Passing this suite establishes the documented `PropertyValue` alphabet and
CPV1 implementation under the tested allocator and Serde backends. It does not
make arbitrary surrounding user types stack-safe, guarantee successful
allocation up to a configured maximum, or make an old unversioned recursive
Serde payload automatically migratable. The owning persistence format must
declare its version transition, as pgmcp cache v2 does.

## References

1. Reynolds, J. C. (1972). *Definitional Interpreters for Higher-Order
   Programming Languages.* DOI:
   [10.1145/800194.805852](https://doi.org/10.1145/800194.805852).
2. Knuth, D. E. (1984). *Literate Programming.* DOI:
   [10.1093/comjnl/27.2.97](https://doi.org/10.1093/comjnl/27.2.97).
