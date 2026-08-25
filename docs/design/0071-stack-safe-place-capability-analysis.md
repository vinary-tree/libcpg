# ADR-0071: Reconstruct place capabilities from typed CPG evidence

- **Status:** Accepted
- **Date:** 2026-08-24
- **Scope:** Rust ownership, places, moves, borrows, reborrows, advisory borrow
  checking, typed overlays, stack safety

## Context

FT.7 needs a feature-free ownership view over a Rust Code Property Graph
(CPG). A **place** is represented storage such as a parameter, local, field,
assignment result, or temporary. A **capability** is permission carried from
one place to another by a move or borrow. The result is a place-capability
graph (PCG): an analysis report derived from existing CPG nodes and evidence.

The CPG does not contain rustc's Mid-level Intermediate Representation (MIR),
trait solver, or borrow-checker facts. The public surface must therefore expose
useful, replayable evidence without pretending to implement the Rust compiler.
It must also avoid a second Rust parser, input-depth recursion, competing node
identity, or implicit graph mutation.

## Decision

1. Rust frontends attach `OwnershipOperationKind::{Move, SharedBorrow,
   MutableBorrow}` to existing syntax nodes from Tree-sitter node kinds and
   typed fields. The analyzer never parses source text.
2. Structured reference types populate `TypeInfo::is_reference` and
   `TypeInfo::is_mutable`. Copy classification is deliberately closed over
   represented primitives, raw pointers, shared references, mutable
   references, and selected standard owning containers. Unknown nominal trait
   evidence remains `PcgIssue::UnknownCopySemantics`.
3. `place_capability_graph(cpg, function)` derives source identity from typed
   `DefUse`/`ReachingDef` Data Flow Graph (DFG) edges or field `Reference`
   edges, bounds ownership operations by function Abstract Syntax Tree (AST)
   scope, and uses Control Flow Graph (CFG) reachability for ordering.
4. The report uses only existing `NodeId` values. It creates no graph node.
   Every relation retains function scope, operand, and its exact DFG or
   `Reference` witness.
5. `borrow_check` is advisory. Move-after-use requires a later reachable DFG
   use. Conflicting loans require the same represented source, at least one
   mutable loan, CFG order, and a later use that keeps the earlier loan live.
6. A borrow's modeled live extent runs from its operation through a represented
   DFG use of the destination reference. This approximates non-lexical
   lifetime (NLL) liveness without claiming rustc completeness.
7. Every AST ancestor walk, operand traversal, use scan, and reachability query
   uses an explicit heap vector, ordered set/map, or `VecDeque`. Native call
   depth is constant in source and graph depth.
8. `materialize_place_capability_overlay` validates every endpoint before the
   first mutation, then appends idempotent typed edges over existing nodes.
9. Append serialized variants and statistics, PyTorch Geometric (PyG) columns
   75--76, relation classes 62--66, and a thirteenth coarse layer. Every older
   discriminant and numeric identity remains unchanged.
10. Keep the analysis feature-free. Parser-backed examples may enable
    `lang-rust`, but graph analysis itself has no language-grammar dependency.

![Typed Rust ownership evidence becomes an immutable PCG report and optional validated overlay without introducing a second parser or node identity.](../diagrams/place-capability-analysis-pipeline.svg)

*Figure -- accepted evidence pipeline. Source:
[`diagrams/place-capability-analysis-pipeline.puml`](../diagrams/place-capability-analysis-pipeline.puml).*

## Consequences

The API can identify represented move-after-use and incompatible-live-borrow
evidence deterministically. `Pcg::is_complete()` means only that each
represented transfer has a source and decisive represented copy evidence. It
does not mean that rustc would accept or reject the function, nor that every
source-language ownership event was represented.

Repeated CFG reachability queries favor a small, auditable implementation over
an implicit recursive oracle. In the worst case, borrow-pair checks cost
quadratic many reachability passes. Heap allocation prevents native stack
overflow but does not cap resident memory; service operators must still limit
graph size, time, memory, swap, and task count.

## Alternatives considered

### Parse Rust expressions inside the analyzer

Rejected. The Tree-sitter frontend already owns syntax and typed fields. A
second parser would drift from the grammar and weaken the evidence boundary.

### Claim compiler-equivalent borrow checking

Rejected. Compiler equivalence requires MIR places, trait resolution, full NLL
or Polonius origin facts, projection overlap, and path-sensitive semantics that
the current CPG does not represent.

### Infer ordering only from source offsets

Rejected. Source order does not represent branches and loops. CFG reachability
is the minimum graph-backed ordering evidence for an advisory finding.

### Recursively traverse expressions or graph paths

Rejected. Adversarial nesting can exceed the native stack. Explicit heap
worklists preserve the logical traversal without input-shaped call depth.

### Always materialize the PCG

Rejected. Immutable reports compose safely with other analyses. Mutation is a
separate, explicit, transactional operation.

## References

1. Grannan, D. et al. (2026). *Place Capability Graphs: A General-Purpose
   Abstraction for the Rust Borrow Checker.* DOI:
   [10.1145/3763122](https://doi.org/10.1145/3763122); preprint:
   [arXiv:2503.21691](https://arxiv.org/abs/2503.21691).
2. Matsushita, Y. et al. (2020). *RustHorn: CHC-Based Verification for Rust
   Programs.* DOI:
   [10.1007/978-3-030-44914-8_18](https://doi.org/10.1007/978-3-030-44914-8_18).
3. Crichton, W. et al. (2022). *Flowistry: An IDE Plugin for Rust to Focus on
   Relevant Code.* DOI:
   [10.1145/3519939.3523445](https://doi.org/10.1145/3519939.3523445).
