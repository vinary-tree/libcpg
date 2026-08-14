# Scientific validation 33 — May-happen-in-parallel analysis

This ledger tests whether typed parallel operand structure produces exactly the
documented may-happen-in-parallel (MHP) pairs, independently of insertion order
and without mutating the source graph. It does not claim schedule feasibility,
shared-memory conflict, or race detection.

## 1. Hypotheses

| ID | Hypothesis | Falsifying observation |
|---|---|---|
| H1 | execution sites in different direct operands of a typed `Spawn` block are related symmetrically | a cross-operand pair is absent or one orientation queries false |
| H2 | sites in one operand and sites under an ordinary sequential block are not related | either negative fixture produces a pair |
| H3 | direct process roots and wrapper-block roots have identical operand semantics | the real Rholang tree or direct-root unit fixture returns no pair |
| H4 | nested parallel regions compose while nested function bodies are excluded | a required inner/outer pair is absent or a function-body site leaks |
| H5 | output is sorted, deduplicated, input-backed, and independent of node/edge insertion order | equivalent generated graphs produce unequal relations or dangling ids |
| H6 | the cap returns only valid pairs and reports truncation exactly when required | an invalid retained pair, excess pair, or incorrect `capped` flag appears |
| H7 | the analysis is feature-free, read-only, and serde-compatible when enabled | a feature-free build fails, graph counts change, or round-trip differs |

## 2. Independent oracle

Generated fixtures choose two non-empty operand site sets $`L`$ and $`R`$.
The test oracle constructs the canonical Cartesian product directly:

```math
E(L,R)=\{(\min(l,r),\max(l,r))\mid l\in L,\;r\in R,\;l\ne r\}.
```

The production implementation discovers operands and sites by traversing the
CPG. The oracle instead knows the generated fixture's site identifiers, so it
does not reuse private traversal helpers.

## 3. Example-based matrix

| Family | Examples |
|---|---|
| positive | one-vs-many wrapped operands; direct call roots; nested inner and outer `par` |
| negative | two calls in one operand; ordinary sequential siblings; nested function body |
| API | symmetric `contains`, sorted `partners`, canonical `first < second`, empty/unknown queries |
| resource bound | zero, one-pair, and complete limits preserve valid deterministic prefixes |
| evidence | every returned id exists; node and edge counts remain unchanged |
| compatibility | `MhpRelation` and `MhpPair` round-trip under `serde` |
| frontend | a real Rholang `left!(1) | right!?(2)` Mode-B tree relates both send sites |

The frontend example originally falsified the wrapper-only assumption: the
typed `par` block's direct children were send `Call` roots. Generalizing direct
children to operand roots made the unit and real-tree oracles agree.

## 4. Property-based experiment

For 128 generated cases, the test varies the sizes of two operand site sets,
constructs the graph in forward and reversed node/edge insertion order, and
asserts:

1. equality with the independent Cartesian-product oracle;
2. equality between insertion orders;
3. strict pair ordering and global lexicographic sorting;
4. symmetric membership for every expected pair; and
5. no same-operand pair.

This property exercises many operand cardinalities while retaining a simple,
auditable expected relation.

## 5. Feature and release matrix

| Configuration | Evidence |
|---|---|
| `--no-default-features` | core implementation, unit/property suite, and public API require no optional grammar |
| `--features rholang` | real parser tree preserves typed `Spawn` and direct operand roots |
| `--features serde` | relation and pair serialization round-trip |
| maximal and `--all-features` | optional language/analysis combinations do not regress the API |

Strict Clippy, warning-free rustdoc, documentation/diagram checks, formatting,
diff checks, and the staged pgmcp bug-gate form the remaining release evidence.

## 6. Interpretation limits

Passing these experiments supports conformance to the typed structural
abstraction. It does not prove that both sites are reachable in one execution,
that they overlap under a concrete scheduler, or that they race. Missing
frontend metadata and capped output can produce false negatives; unreachable
branches can produce false positives.

## References

1. Naumovich, G., Avrunin, G. S. (1998). *A Conservative Data Flow Algorithm
   for Detecting All Pairs of Statements That May Happen in Parallel.* DOI:
   [10.1145/288195.288213](https://doi.org/10.1145/288195.288213).
