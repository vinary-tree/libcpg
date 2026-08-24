# ADR-0070: Analyze MeTTa rewriting with flat terms and explicit worklists

- **Status:** Accepted
- **Date:** 2026-08-24
- **Scope:** MeTTa, term rewriting, dependency pairs, termination, critical
  pairs, confluence, unification, strongly connected components, stack safety

## Context

FT.6 needs termination and confluence evidence for MeTTa rules already
normalized into the Code Property Graph (CPG). The analysis must not introduce
a second source parser, a second graph identity, input-shaped native recursion,
an unsound binary verdict, or a fuzzy rule lookup that can omit an exact
candidate. It must reuse libcpg's existing iterative strongly connected
component (SCC) implementation and preserve every established serialized and
machine-learning identifier.

## Decision

1. Consume only normalized MeTTa CPG nodes. A rule is a `Function`; its
   left-hand side (LHS) binders are `Parameter` nodes; right-hand side (RHS)
   uses resolve through rule-local typed binder identity. Operator leaves retain
   their normalized lexeme, so `+`, `*`, `=`, and `:=` remain distinguishable
   without reparsing source.
2. Extract every term into one preorder `FlatTerm` arena. Each `FlatNode`
   stores symbol, source `NodeId`, arity, and an exclusive `subtree_end` offset.
   Extraction, unification, occurs checks, instantiation, matching, rewriting,
   comparison, hashing, and destruction use loops and heap worklists.
3. Index exact rule-root symbols with `libdictenstein`'s double-array trie.
   Variable-root rules occupy an explicit wildcard bucket; every returned
   candidate still passes exact first-order unification.
4. Build dependency pairs from defined-symbol calls in every RHS subterm.
   Project rule-to-rule edges as `ProjectionKind::RewriteDependency` and pass
   that projection to the existing iterative `projection_sccs` implementation.
5. Use the fixed polynomial interpretation whose constant is the number of
   non-variable nodes and whose variable coefficients are occurrence counts.
   A certificate requires every rewrite rule to be weakly oriented and every
   dependency pair internal to a cyclic SCC to be strictly oriented.
6. Return a three-valued `TerminationVerdict`: a complete reduction-pair
   certificate, an alpha-equivalent root self-cycle witness, or `Unknown` with
   every unoriented component and extraction/resource issue. Never infer
   nontermination from a graph cycle alone.
7. Generate critical overlaps only at non-variable LHS positions, rename rule
   variables apart, compute a most-general unifier with an iterative occurs
   check, and preserve one shared variable identity across both reducts and all
   later rewrite states. Bidirectional breadth-first search proves joinability
   on intersection or non-joinability only after both finite reachable spaces
   are exhausted.
8. Mark non-left-linear rules explicitly incomplete for confluence. This
   implementation omits variable-position overlaps, so the Critical Pair Lemma
   is invoked only for its admitted left-linear fragment. Termination analysis
   remains available for those rules.
9. Make all six resource ceilings caller-selectable through
   `RewriteAnalysisCaps`. A reached cap is a typed issue; no truncated prefix is
   presented as a complete negative result.
10. Materialize `RewriteDep::{DependencyPair,CriticalConflict}` only after
    validating every endpoint as an existing rule function. Mutation is
    transactional and idempotent.
11. Append serialized variants and statistics, ML columns 73--74, and relation
    classes 60--61. Preserve all earlier discriminants, 73 feature columns, and
    60 relation classes exactly.
12. Keep `metta` grammar-free but let it activate the canonical optional
    `libdictenstein` dependency. The caller still owns the Tree-sitter grammar;
    the new dependency is an exact dictionary index, not a parser.

![Normalized MeTTa evidence flows through flat terms, exact dependency analysis, qualified proofs, and optional transactional overlay materialization.](../diagrams/metta-rewrite-analysis-pipeline.svg)

*Figure -- accepted end-to-end analysis boundary. Source:
[`diagrams/metta-rewrite-analysis-pipeline.puml`](../diagrams/metta-rewrite-analysis-pipeline.puml).*

## Consequences

The native call depth is constant in source-term depth and rewrite-search
depth. Deterministic ordering comes from stable IDs, ordered maps/sets, binary
heaps, canonical flat structural keys, and the existing SCC contract. Exact
positive and negative evidence is replayable at existing rule and call-site
nodes.

The procedure is deliberately incomplete. The fixed interpretation does not
discover every reduction order; the concrete nontermination detector covers
alpha-equivalent root self-loops; infinite joinability spaces reach a cap; and
non-left-linear systems do not receive a confluence certificate. Each case is
`Unknown` or `complete == false`, never a guessed proof.

The current ML schema grows from 73 to 75 node columns, 60 to 62 relation
classes, and eleven to twelve coarse layers. The two new columns and classes
are appended, so existing numeric consumers remain stable while schema-aware
consumers must admit the new suffix.

## Alternatives considered

### Hand-write a MeTTa parser in the analyzer

Rejected. Tree-sitter and `NodeMapper` already own syntax. A second parser
would drift from the grammar and erase the typed-evidence boundary.

### Use fuzzy rule-head matching

Rejected. Approximate matching is useful for search and advisory similarity,
not for a proof obligation. Exact trie lookup plus unification has no false
negative from an edit-distance threshold.

### Treat every dependency cycle as nontermination

Rejected. A dependency graph is a candidate-chain abstraction. Only a concrete
repeating rewrite is a refutation; an unoriented cycle is honestly unknown.

### Use recursive term algorithms

Rejected. Adversarial symbolic terms routinely exceed native stack depth. Flat
arenas and explicit continuation/work queues preserve semantics without
process-stack dependence.

### Add another SCC implementation

Rejected. The shared iterative Tarjan engine already defines canonical libcpg
cycle and condensation semantics.

## References

1. Arts, T., Giesl, J. (2000). *Termination of Term Rewriting Using Dependency
   Pairs.* DOI:
   [10.1016/S0304-3975(99)00207-8](https://doi.org/10.1016/S0304-3975(99)00207-8).
2. Giesl, J. et al. (2017). *Analyzing Program Termination and Complexity
   Automatically with AProVE.* DOI:
   [10.1007/s10817-016-9388-y](https://doi.org/10.1007/s10817-016-9388-y).
3. Newman, M. H. A. (1942). *On Theories with a Combinatorial Definition of
   Equivalence.* DOI: [10.2307/1968867](https://doi.org/10.2307/1968867).
4. Robinson, J. A. (1965). *A Machine-Oriented Logic Based on the Resolution
   Principle.* DOI:
   [10.1145/321250.321253](https://doi.org/10.1145/321250.321253).
5. Aoe, J. (1989). *An Efficient Digital Search Algorithm by Using a
   Double-Array Structure.* DOI:
   [10.1109/32.31365](https://doi.org/10.1109/32.31365).
