# Scientific validation 67 -- Heterogeneous export and fuzzing distance

## Claims under test

The heterogeneous-export claim is exact alignment of stable nodes, normalized
tokens, 109 feature columns, and all 67 typed relation multisets independent of
construction order. The distance claim is exact harmonic aggregation of
stored-direction CFG/call shortest paths, with explicit zero and infinity
semantics. Both analyses claim constant native call depth in graph depth.

The validation does not claim model accuracy, collision-free token encoding,
path feasibility, compiler basic-block equivalence, fuzzing coverage, or an
implementation of AFLGo's scheduler.

## Exhaustive relation oracle

A two-node CPG contains one edge occurrence of every `CpgEdgeKind` subtype:
four AST, fourteen CFG, thirteen DFG, two PDG, three call, four type, three
reference, two scope, two import, six Heap, seven NameFlow, two RewriteDep, and
five PlaceCapability relations.

The oracle requires:

- exactly 67 buckets and names in class order;
- one exact dense pair `(0, 1)` in every corresponding bucket;
- ascending non-dense stable ids `[10, 20]`;
- exact node classes, tokens, source ranges, and 109-column rows;
- one exact one-hot value in columns 0--44; and
- at least one token-hash contribution in columns 45--108.

A second graph pins empty relation buckets and two identical parallel
`DefUse` coordinates. This distinguishes schema completeness from graph
multiplicity.

## Hand-calculated distance oracle

The exact fixture gives one join node path lengths two and three to targets.
Its expected distance is `12/5`. A call-site predecessor has lengths three and
four, so its result is `24/7`; the preceding dynamic-call node has lengths four
and five, so its result is `40/9`.

Both targets are exactly zero. An AST-only predecessor is positive infinity,
proving that non-CFG/call overlays do not shorten distance. Duplicate CFG edges
do not alter the result. Empty/all-unknown targets yield infinity everywhere,
and duplicate/reordered/unknown target lists equal the unique valid list.
Node/edge counts before and after prove immutability.

## Property-based validation

The ML campaign runs 128 generated cases. It varies terminal count and a
subset of extra DFG/PDG edges, constructs independent forward/reverse node and
edge insertion permutations, and requires exact `HeteroGnnExport` equality.
Under `serde`, it also requires byte-identical JSON.

The distance campaign runs 256 generated cases over optional CFG, static-call,
dynamic-call, call-site, irrelevant AST, and duplicate edges. It reverses node,
edge, and target order, optionally duplicates a target, includes an unknown id,
and requires exact map equality including infinity.

## Public and parser integration

`tests/heterogeneous_distance_integration.rs` uses crate-root APIs. A manual
graph crosses CFG, call-site, call-return, and static-call boundaries, checks
one exact typed relation bucket, proves an irrelevant PDG predecessor remains
unreachable, and verifies graph immutability.

With `lang-rust`, the same target builds a real Rust CPG with CFG and DFG
enabled. The test requires non-empty typed CFG buckets, a zero target exit, and
at least one positive finite predecessor distance.

## Stack-safety experiment

The public integration constructs a 100,000-node sequential CFG on a worker
with a 64 KiB native stack. It computes the first node's exact distance
`99_999`, builds all 100,000 aligned feature rows, and retains all 99,999
sequential relation coordinates. Construction, analysis, assertions, result
destruction, and CPG destruction complete on that stack.

This demonstrates that represented depth resides in binary heaps, vectors,
ordered maps, and FIFO queues rather than native recursive frames. The run is
not a resident-memory bound; validation processes separately use a 4 GiB
no-swap scope.

## Gate matrix

| Command family | Evidence |
|---|---|
| `cargo test --lib analysis::ml::tests` | exhaustive relations, token/features, parallel/empty buckets, 128-case property |
| `cargo test --lib analysis::distance::tests` | harmonic examples, unknown/duplicate/empty targets, 256-case property |
| `cargo test --test heterogeneous_distance_integration` | public inter-overlay and 100,000-depth/64 KiB composition |
| `cargo test --all-features --lib analysis::ml::tests` | serde round trip and byte-identical insertion permutations |
| `cargo test --all-features --test heterogeneous_distance_integration` | parser-built Rust plus public/depth cases |
| `cargo clippy --all-features --all-targets -- -D warnings` | warning-free complete target surface |
| `./scripts/verify.sh` | terminal repository matrix after documentation completion |

Every build/test command runs with `MemoryMax=4G`, `MemorySwapMax=0`,
`TasksMax=256`, and `CARGO_BUILD_JOBS=1`. Validation output is captured under
`target/verification`, inspected, and exactly deleted.

## Literature grounding

R-GCN and HGT establish relation-specific convolution/attention over
heterogeneous graphs [[1]](#references), [[2]](#references). AFLGo establishes
directed greybox fuzzing with target-distance-guided scheduling
[[3]](#references). The suite validates libcpg's structural interchange and
distance equations, not behavioral equivalence to those complete systems.

## References

1. Schlichtkrull, M. et al. (2018). *Modeling Relational Data with Graph
   Convolutional Networks.* DOI:
   [10.1007/978-3-319-93417-4_38](https://doi.org/10.1007/978-3-319-93417-4_38).
2. Hu, Z., Dong, Y., Wang, K., Sun, Y. (2020). *Heterogeneous Graph
   Transformer.* DOI:
   [10.1145/3366423.3380027](https://doi.org/10.1145/3366423.3380027).
3. Böhme, M., Pham, V.-T., Nguyen, M.-D., Roychoudhury, A. (2017). *Directed
   Greybox Fuzzing.* DOI:
   [10.1145/3133956.3134020](https://doi.org/10.1145/3133956.3134020).
