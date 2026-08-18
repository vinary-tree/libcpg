# Scientific validation 36 — Shared flat expression facts and CEK1

This ledger tests whether the shared expression arena specified by
[ADR-0041](../design/0041-shared-flat-expression-facts.md) preserves shallow
structural semantics, rejects malformed persistence, remains practically
linear on deep syntax, and keeps native call depth independent of source depth.

## 1. Falsifiable hypotheses

| ID | Hypothesis | Falsifying observation |
|---|---|---|
| H1 | borrowed flat views preserve operator spelling, operand tags, scalar payloads, and operand order | a hand-computed key differs from its view |
| H2 | exact structural classes merge equal roots and never merge unequal roots | repeated `x + 1` has multiple fact ids or distinct ordered expressions share one id |
| H3 | a shared CPG subexpression occupies one physical arena node while retaining two logical operand occurrences | the two borrowed child ids differ or canonical encoding loses one occurrence |
| H4 | iterative extraction preserves conservative cycle and unsupported-node semantics | an active cycle recurses, hangs, or becomes a structural equality claim instead of `Opaque` |
| H5 | `CEK1` is canonical and exact for every scalar payload | decode/re-encode changes bytes or float bits change |
| H6 | malformed ranges, references, order, tags, scalars, UTF-8, truncation, and trailing bytes fail closed | any malformed case returns a key or panics |
| H7 | all five caps are independent and monotone | an exact cap fails, a one-smaller cap passes, or the wrong dimension is reported |
| H8 | equality, ordering, and hashing agree across independently owned arenas | equal decoded/original keys compare non-equal or hash differently |
| H9 | a 100,000-level key and expression universe fit a 256 KiB native stack | construction, query, lifecycle, codec, Serde, or drop overflows |
| H10 | shared construction avoids quadratic occurrence ownership and lookup | the 100,000-expression public gate does not complete within the recorded linear-scale envelope |

## 2. Example and generated evidence

The example matrix covers all twelve operand tags, unary, binary, n-ary,
ordered permutation, an unsupported return node, an active self-cycle, and a
child referenced twice by one parent. The sharing example requires logical
metrics of three nodes—parent plus two child occurrences—while both borrowed
operands compare as the same arena node. Encoding expands those occurrences to
one canonical three-node tree, and decoding remains structurally equal.

The generated property starts with an arbitrary integer leaf and applies up to
96 mixed operators. Each step independently chooses the operator, scalar, and
whether the scalar precedes or follows the nested expression. Two independently
constructed keys must satisfy:

```math
k_1 = k_2
\land \operatorname{cmp}(k_1,k_2)=\mathrm{Equal}
\land \operatorname{hash}(k_1)=\operatorname{hash}(k_2).
```

It then requires `decode(encode(k)) = k` and byte-idempotent re-encoding. The
separate permutation example changes only operand order and requires unequal
keys, unequal ordering, and unequal canonical bytes.

## 3. Adversarial persistence matrix

| Family | Rejected shape |
|---|---|
| version | wrong `CEK1` magic/version; empty arena |
| bounds | declared count above node/operand/work cap; encoded input above byte cap |
| node tape | nonpartitioned range; arithmetic overflow; unused operand suffix |
| graph | forward/self reference; noncontiguous postorder child subtrees; duplicate/missing parent |
| scalar | unknown tag; boolean other than 0/1; surrogate or out-of-range character; invalid UTF-8 |
| completeness | truncated payload; trailing bytes |

The decoder reads counts and enforces count/work/encoded bounds before
reserving the corresponding tapes. Complete canonical validation occurs before
the key becomes observable. No old recursive shape is attempted after failure.

## 4. Resource and stack experiments

For physical arena nodes $`N_p`$, physical operands $`O_p`$, and one logical
root with occurrence-expanded counts $`N_l`$ and $`O_l`$, storage is
$`O(N_p+O_p)`$ while standalone encoding is $`O(N_l+O_l)`$. This distinction
is necessary for a shared DAG: compact storage must not hide the bytes or work
needed to persist the logical value.

The key experiment alternates unary and mixed unary-plus-scalar parents for
100,000 levels. On a 256 KiB stack it performs construction, borrow/query,
clone, equality, ordering through decode comparison, hash, debug, encode,
decode, optional JSON-backed Serde, and destruction.

The public integration constructs a 100,000-operator CPG chain, derives all
100,000 expression facts, queries the outer root and variable kill set,
clones/compares `ExprFacts`, and drops both facts and graph on a 256 KiB stack.
The anti-vacuity checks require exactly 100,000 universe entries and an outer
metric of 100,000 nodes. After exact structural-class lookup replaced repeated
deep binary searches, this gate completed in 1.64--1.73 seconds across the
Serde and all-feature debug-profile gates on the development host; that number
is a regression datum, not a portable benchmark claim.

## 5. Reproduction

```text
cargo test --lib analysis::dataflow::expr_key::tests::
cargo test --lib analysis::dataflow::facts::tests::
cargo test --test integration \
  expression_dataflow_analysis::public_expression_facts_build_depth_100_000_on_a_small_stack \
  -- --exact
cargo test --lib --features serde \
  analysis::dataflow::expr_key::tests::depth_100_000_lifecycle_and_codec_fit_a_small_ordinary_stack
cargo clippy --lib --tests --features serde -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --features serde
```

These focused experiments do not replace the epic's one final maximal
verification run.

## 6. Interpretation limits

Passing establishes the documented expression alphabet, exact class interner,
CPG extraction semantics, `CEK1` boundary, and tested small-stack behavior. It
does not prove that moving or eliminating a reported expression is safe under
language-specific effects, traps, volatile access, floating-point environment,
or concurrency. Available-expression and PRE outputs remain advisory evidence.

## References

1. Reynolds, J. C. (1972). *Definitional Interpreters for Higher-Order
   Programming Languages.* DOI:
   [10.1145/800194.805852](https://doi.org/10.1145/800194.805852).
2. Kildall, G. A. (1973). *A Unified Approach to Global Program Optimization.*
   DOI: [10.1145/512927.512945](https://doi.org/10.1145/512927.512945).
3. Knuth, D. E. (1984). *Literate Programming.* DOI:
   [10.1093/comjnl/27.2.97](https://doi.org/10.1093/comjnl/27.2.97).
