# ADR-0054: Export deterministic CPG evidence for external ML systems

- **Status:** Accepted
- **Date:** 2026-08-20
- **Scope:** feature-free `analysis::ml`

## Context

External machine-learning (ML) systems need a stable numerical view of a Code
Property Graph (CPG), but libcpg's typed graph is richer than one adjacency
matrix. It contains 45 node kinds, 47 edge relations, nine coarse overlay
layers, source identities, parallel relations, malformed-input possibilities,
and optional dependence slices. A consumer must be able to map every numerical
row and edge back to exact CPG evidence.

Path-based code models need a second view. code2vec represents a method through
terminal-to-terminal Abstract Syntax Tree (AST) paths, while line-level
vulnerability models combine syntax, dependence, and graph context
[[1]](#references), [[2]](#references), [[3]](#references). Reconstructing
those paths after export from only a lossy tensor would discard child order,
exact ancestors, and stable node identity.

The existing optional `gnn` module is not this boundary. It owns a CPG, uses
random initialization, and performs an untrained mean propagation when the
`gnn` feature is enabled. A canonical interchange format must be feature-free,
read-only, deterministic, serializable when `serde` is enabled, and independent
of PyTorch, PyTorch Geometric (PyG), `ndarray`, random-number generators, or a
model runtime.

## Decision

1. Add a feature-free `analysis::ml` module with two independent public views:
   `PygExport` for graph tensors and `PathContext` for exact AST paths.
2. Canonicalize dense node ids by ascending `NodeId`. Align `node_features`,
   `node_ids`, and `source_ranges` one-for-one in that order.
3. Freeze a 69-column node schema: 45 node-kind indicators; incoming and
   outgoing degrees for nine overlay layers; AST depth and a separate
   depth-known bit; loop-nesting depth; forward and reverse PageRank; and one
   optional backward-PDG-slice membership bit.
4. Freeze a contiguous 47-class edge schema covering every current
   `CpgEdgeKind`, `CfgEdgeKind`, and `DfgEdgeKind`. Export sorted
   `(source_dense, target_dense, relation_class)` tuples and preserve parallel
   edge multiplicity.
5. Compute PageRank on a canonical unweighted whole-CPG topology. Parallel
   typed edges remain in `edge_index`, but the structural centrality projection
   merges repeated endpoint pairs.
6. Compute AST depth and loop depth with a root-seeded breadth-first worklist.
   A node has known depth only when its `AstChild` parent relation is unique and
   reachable from an AST root. Multiple-parent or cyclic regions receive zero
   numeric depth plus `ast.depth_known = 0`; zero is never presented as known.
7. Let `export_pyg` export the whole CPG without localization. Let
   `export_pyg_with_options` optionally compute a complete backward Program
   Dependence Graph (PDG) slice before checking the caller's cap. If the full
   slice exceeds the cap, return `BudgetExceeded` rather than a partial negative
   example.
8. Extract AST contexts only inside one validated function, stopping before
   nested functions. A terminal is a non-function scoped node without an
   `AstChild` successor. Sort terminals by source range and stable id.
9. Reconstruct every terminal-to-function chain iteratively. Suppress chains
   with a missing, ambiguous, or cyclic parent relation. For each unordered
   terminal pair, find the lowest common ancestor, enforce path-length and
   source-ordered child-width filters, and retain the complete node path,
   aligned node-kind classes, ancestor position, and terminal tokens.
10. Expose caller caps and compile-time hard ceilings for terminals, pairs,
    contexts, retained path nodes, and ancestor steps. Every cap failure is an
    error before a partial vector is returned.
11. Use binary heaps, ordered maps, queues, generation marks, and heap vectors
    for all input-sized traversal and ordering state. Never recurse over source
    depth or parse source spellings.
12. Keep both exports immutable and out of graph. Optional serde derives encode
    the complete evidence records but do not change computation.

![Canonical CPG evidence exported for external ML consumers](../diagrams/ml-export-pipeline.svg)

![Exact terminal-to-terminal AST path reconstruction](../diagrams/ast-path-context.svg)

## Rejected alternatives

### Make PyTorch or PyG a libcpg dependency

Rejected. libcpg owns graph evidence and canonical interchange, not a Python
runtime, device allocator, training loop, or tensor ABI. Ordinary vectors keep
the feature-free core portable; a consumer can transpose `edge_index` and
construct its preferred tensor type.

### Reuse the optional `CpgGnn` embeddings

Rejected. Those vectors are randomly initialized and model-runtime-specific.
They are useful as an experimental structural embedder, but not as reproducible
ground truth or a stable interchange schema.

### Collapse all edges to an untyped adjacency matrix

Rejected. AST, CFG, DFG, PDG, call, type, reference, scope, and import relations
have different semantics. Relation classes preserve the evidence a relational
model needs, while coarse degree columns summarize each layer separately.

### Use a truncated dependence slice

Rejected. Marking only an arbitrary prefix can teach a model that omitted
dependencies are negative evidence. The complete slice is computed first and
then admitted or rejected as a unit.

### Assign a plausible depth to malformed AST nodes

Rejected. Choosing one of several parents or breaking a cycle by node id would
invent syntax. The paired depth-known bit preserves the difference between a
real root depth of zero and unknown depth.

### Export only endpoints and an ancestor label

Rejected. A downstream consumer could not verify adjacency, recover the
ancestor position, or apply a different path encoder. `PathContext::path`
retains every exact CPG node on both branches.

### Sort with recursive comparison machinery or recurse through parents

Rejected. Generated and hostile graphs can be arbitrarily deep. Explicit
heaps and worklists keep native call depth constant and make cycle behavior
visible.

## Consequences

### Positive

- The numerical representation is stable across node and edge insertion order.
- Every row, range, edge endpoint, path node, and slice member maps back to one
  stable `NodeId`.
- Typed parallel edges survive export even though centrality intentionally uses
  simple unweighted topology.
- Exact paths support code2vec/code2seq-style encoders without reparsing source.
- Missing AST depth and over-budget localization cannot masquerade as valid
  negative evidence.
- No ML runtime or parser dependency enters the feature-free core.

### Negative

- The frozen schemas must be versioned deliberately when node or edge enums
  grow.
- A dense `Vec<Vec<f32>>` favors interoperability over packed tensor memory.
- PageRank is global, unweighted, and topology-only; it does not encode relation
  importance or convergence as a proof of semantic relevance.
- Multiple-parent/cyclic terminal chains are suppressed rather than reported as
  contexts, so an empty path vector is not an AST-validity certificate.
- Path-pair enumeration is quadratic in the terminal count and therefore
  requires explicit caps.

## Verification

Exact examples pin all 47 edge classes, the 69-column schema, dense alignment,
parallel-edge preservation, degree/depth/loop/slice columns, reconstructable
AST paths, malformed-chain suppression, and every budget boundary. A 128-case
property rebuilds complete graphs in forward and reverse insertion order and
requires identical graph and path exports. Public integration composes both
APIs through crate-root re-exports. A 100,000-node AST runs both views on a
256 KiB native stack. Feature-free, all-feature serde, clippy, rustdoc, and the
complete libcpg verification matrix pass.

## References

1. Alon, U., Zilberstein, M., Levy, O., Yahav, E. (2019). *code2vec: Learning
   Distributed Representations of Code.* Proceedings of the ACM on Programming
   Languages 3(POPL). DOI:
   [10.1145/3290353](https://doi.org/10.1145/3290353).
2. Hin, D., Kan, A., Chen, H., Babar, M. A. (2022). *LineVD: Statement-level
   Vulnerability Detection using Graph Neural Networks.* MSR 2022. DOI:
   [10.1145/3524842.3527949](https://doi.org/10.1145/3524842.3527949).
3. Li, Y., Wang, S., Nguyen, T. N. (2021). *Vulnerability Detection with Fine-
   grained Interpretations.* ASE 2021. DOI:
   [10.1145/3468264.3468597](https://doi.org/10.1145/3468264.3468597).
4. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and
   Discovering Vulnerabilities with Code Property Graphs.* IEEE Symposium on
   Security and Privacy. DOI:
   [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44).
