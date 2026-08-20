# Scientific validation 49 — Deterministic ML export

## Claims under test

The graph-export claim is that every numerical row, source range, typed edge,
and optional localization bit maps exactly to stable CPG evidence, independent
of insertion order. The path-export claim is that every returned context is an
exact source-ordered terminal-to-lowest-common-ancestor-to-terminal AST path,
with no invented parent relation and no source-depth native recursion.

These are representation claims. The validation does not claim that the
features train an accurate model, reproduce a published vulnerability detector,
or predict runtime behavior.

## Exact graph oracle

The hand-built graph contains a function, loop, binary expression, identifier,
and literal with stable non-dense ids. Its overlay includes AST, CFG, DFG, PDG,
and reference relations, including two identical parallel typed DFG edges.

| Observation | Exact expectation |
|---|---|
| dense identity | ascending ids `[10, 20, 30, 40, 50]` |
| row/range alignment | each range contains its corresponding stable id offset |
| kind family | exactly one of the first 45 columns is one per row |
| edge relation | every dense tuple and all ten edge occurrences match the graph |
| multiplicity | both parallel `DefUse` tuples remain present |
| AST depth | `[0, 1, 2, 3, 3]`, all known |
| loop depth | `[0, 1, 1, 1, 1]` |
| layer degrees | exact AST, DFG, and reference counts at selected nodes |
| numerical validity | every one of 69 values in every row is finite |
| immutability | node and edge counts are unchanged |

The class-registry oracle enumerates all four AST relations, all fourteen CFG
subtypes, all thirteen DFG subtypes, and the sixteen remaining relations. Their
classes must be exactly the contiguous vector `0..47`. Exhaustive `match`
expressions make a future enum addition a compile failure until the schema is
updated deliberately.

## Complete localization oracle

Two incoming `DataDependence` edges form a chain into criterion 50. With a cap
of three, the exact sorted slice is `[20, 30, 50]`, and only those rows have the
localization bit. With a cap of two, the operation returns a budget error whose
required size is three. An absent criterion returns `UnknownNode`.

This three-way oracle distinguishes complete positive membership, complete
negative membership, and refusal to manufacture a partial label.

## Exact AST-path oracle

The same graph has two terminals under a binary expression. Its sole context
must have:

- path `[identifier, binary, literal]`;
- ancestor equal to the binary node at index one;
- aligned kind classes `[Identifier, BinaryOp, Literal]`;
- exact source text `lhs` for the first token and represented literal `7` for
  the second; and
- one real `AstChild` relation in either direction for every adjacent pair.

Reducing `max_path_length` to one filters the context without error. Separate
fixtures prove `UnknownNode` and `NotAFunction`, all five allocation caps, and
the compile-time path-length ceiling.

## Malformed-graph oracle

A terminal is made the child of two branches while the branches also form a
cycle. `ast_paths` terminates and emits no invented context. `export_pyg`
retains all nodes but clears `ast.depth_known` throughout the ambiguous/cyclic
region. This pins the distinction between conservative suppression and guessed
syntax.

## Property-based validation

One 128-case campaign varies terminal count from two through thirteen and adds
a varying subset of DFG/PDG edges. Each case constructs two independent CPGs:
nodes and edges enter one in forward order and the other in reverse order.

The property requires exact `PygExport` equality, including floating-point
centralities and convergence flags. It also requires exact equality of all AST
contexts under caps sized from the generated terminal count. This targets the
actual determinism hazard—construction order—rather than sorting one result
twice.

## Public integration

`tests/ml_export_integration.rs` uses only crate-root APIs. A four-node
function graph is exported with a two-node complete PDG slice. The test verifies
stable row ids, feature/range cardinality, typed edge cardinality, the public
feature-name lookup, the exact three-node AST path, exact terminal tokens, and
CPG immutability.

## Stack-safety experiment

`tests/ml_export_stack_safety.rs` constructs 100,000 nested AST blocks followed
by one binary expression and two terminals. On a worker with a 256 KiB native
stack it:

1. reconstructs both terminal-to-function chains and emits the exact
   three-node terminal path;
2. exports every node and AST edge;
3. computes root-reachable depth, whole-graph forward/reverse PageRank, and all
   aligned feature rows; and
4. confirms the deepest terminal depth and source graph immutability.

The constrained stack exercises construction, parent chains, depth traversal,
centrality, row assembly, result checks, and destruction. Passing demonstrates
that represented source depth is heap state rather than native call depth.

## Feature and gate matrix

| Command family | Evidence |
|---|---|
| `cargo test --no-default-features analysis::ml --lib` | feature-free exact examples and 128-case property |
| `cargo test --all-features analysis::ml --lib` | same behavior plus serde round trips and deterministic JSON bytes |
| `cargo test --no-default-features --test ml_export_integration` | public graph/path/localization composition |
| `cargo test --no-default-features --test ml_export_stack_safety` | 100,000 nodes on 256 KiB |
| `cargo clippy --all-targets --all-features -- -D warnings` | warning-free complete target surface |
| `./scripts/verify.sh` | formatting, default/maximal/all-feature tests, examples, clippy, rustdoc, docs, and diff checks |

## Falsification boundaries

The suite does not validate a trained model, a data-set split, GPU conversion,
Python package compatibility, feature normalization, relation weighting,
vulnerability labels, or model accuracy. It does not claim malformed-chain
suppression is reported as an explicit count. A future change to any schema,
centrality topology, token precedence, path ordering, or completeness behavior
must update exact oracles and preserve insertion-order and small-stack
properties.

## Literature grounding

code2vec grounds terminal-path contexts, while LineVD and IVDetect motivate
line-aligned syntax/dependence/centrality features for downstream vulnerability
models [[1]](#references), [[2]](#references), [[3]](#references). The tests
validate libcpg's representation, not equivalence to those systems.

## References

1. Alon, U., Zilberstein, M., Levy, O., Yahav, E. (2019). *code2vec: Learning
   Distributed Representations of Code.* DOI:
   [10.1145/3290353](https://doi.org/10.1145/3290353).
2. Hin, D., Kan, A., Chen, H., Babar, M. A. (2022). *LineVD: Statement-level
   Vulnerability Detection using Graph Neural Networks.* DOI:
   [10.1145/3524842.3527949](https://doi.org/10.1145/3524842.3527949).
3. Li, Y., Wang, S., Nguyen, T. N. (2021). *Vulnerability Detection with Fine-
   grained Interpretations.* DOI:
   [10.1145/3468264.3468597](https://doi.org/10.1145/3468264.3468597).
