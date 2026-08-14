# Scientific validation 32 — Cyclic-wait and lock-order candidates

This validation plan separates three claims: event/resource evidence is
correctly preserved, projected cycles are classified deterministically, and
public witnesses refer only to the input CPG. It does not validate runtime
deadlock reachability, which the API explicitly does not claim.

## 1. Hypotheses

| ID | Hypothesis | Falsifying observation |
|---|---|---|
| H1 | mutual initial receives whose only producers are initially blocked form one communication SCC | no finding or invalid process/channel witness |
| H2 | a send-first or alternative unblocked producer breaks the candidate | a communication finding remains |
| H3 | opposite A/B and B/A conflicting lock orders form one lock SCC | missing/duplicated candidate or wrong resource ids |
| H4 | releases and all-read ordering do not create conflicting lock candidates | a false positive is returned |
| H5 | resolved calls propagate acquisition summaries through recursion | a reachable callee acquisition is absent or iteration fails to terminate |
| H6 | results are deterministic under node and edge insertion order | generated equivalent graphs produce unequal results |
| H7 | every process/resource/site/range is input-backed and analysis is read-only | dangling evidence or changed graph counts |
| H8 | outer lexical definitions reach nested functions without sibling-local leakage | missing Rholang channel identity or cross-function false edge |

## 2. Independent oracles

For communication examples, the oracle is a hand-written process table listing
the first relevant event and producer set for each channel. For lock examples,
the oracle is the explicit held-stack trace:

```text
left:  acquire A; acquire B  => A -> B
right: acquire B; acquire A  => B -> A
expected resource SCC: {A, B}
```

The implementation under test creates `GraphProjection::External`; the already
independently tested `projection_sccs` establishes cyclicity. Tests assert the
resulting component and evidence, rather than reusing private helper state as
the oracle.

## 3. Example-based matrix

| Family | Examples |
|---|---|
| communication positive | two-process mutual receive; single-process receive-before-own-send |
| communication negative | send-first; external/unblocked alternative producer; persistent and peek only; unresolved identity |
| lock positive | direct write A/B cycle; mixed read/write cycle; resolved interprocedural A/B cycle |
| lock negative | consistent order; release before second acquire; unresolved release; all-read cycle; re-acquisition of same resource |
| summaries | transitive and recursive resolved calls reach a finite fixed point |
| evidence | exact source ranges, existing ids, immutable node/edge counts, stable kind strings |
| compatibility | legacy serde nodes default missing `lock_kind` to `None` |
| frontend | real two-contract Rholang Mode-B tree captures outer `new` channels |
| DFG regression | outer lexical capture succeeds; preceding sibling function local does not leak |

## 4. Property-based tests

Each property runs 128 generated cases:

1. reversing event/node and AST/DFG edge insertion leaves communication findings identical;
2. reversing lock-fixture node and edge insertion leaves lock findings identical; and
3. an $`n`$-process receive-before-send ring is cyclic exactly when no process
   is changed to send first.

The ring generator varies $`2 \le n < 8`$ and optionally selects a breaker.
This checks more topologies than a fixed two-process example while retaining a
simple independent expected predicate.

## 5. Feature matrix

| Configuration | Purpose |
|---|---|
| `--no-default-features` | proves the analysis core and typed fixture API are feature-free |
| `--features rholang` | exercises real grammar normalization plus outer lexical DFG capture |
| `--features serde` | round-trips lock metadata, modes, witnesses, and findings |
| maximal and `--all-features` | detects integration and optional-dependency regressions |

Strict Clippy, warning-free rustdoc, the repository documentation checker,
diagram rendering, diff checks, and the staged pgmcp bug-gate complete the
release evidence.

## 6. Interpretation limits

Passing the suite supports deterministic conformance to the documented
structural abstraction. It does not establish completeness for dynamic channel
creation, reflection, external actors, lock upgrades, condition variables, or
runtime scheduling. Kobayashi's type-based analysis is explicitly a richer
future abstraction [[1]](#references).

## References

1. Kobayashi, N. (2006). *A New Type System for Deadlock-Free Processes.* DOI:
   [10.1007/11817949_16](https://doi.org/10.1007/11817949_16).
