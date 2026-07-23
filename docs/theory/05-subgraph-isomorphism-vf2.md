# Subgraph Isomorphism and the VF2 Algorithm

> Theory pillar · file 05. Prerequisite for [design‑pattern detection](07-design-pattern-detection.md); the graded, approximate counterpart is [graph similarity](06-graph-similarity.md).

Exact structural search is the sharpest question we can ask of a [Code Property Graph](../GLOSSARY.md#code-property-graph-cpg): *does this specific shape occur inside this program?* "Return a value that was just read from a field", "an `if` whose branch throws without releasing a lock", "a class that owns a static instance of itself" — each is a small **pattern graph** that either embeds into the target CPG or does not. This page develops the theory behind that question ([subgraph isomorphism](../GLOSSARY.md#isomorphism--subgraph-isomorphism)) and the algorithm `libcpg` uses to answer it in practice ([VF2](../GLOSSARY.md#vf2), implemented in the always‑on `pattern` module).

The distinction between `pattern` (the always‑on VF2 / similarity module, this page) and `patterns` (the feature‑gated Gang‑of‑Four detectors, [file 07](07-design-pattern-detection.md)) matters throughout `libcpg`: the two are never the same module.

---

## 1. What "subgraph isomorphism" means

Write a graph as $`G = (V, E)`$ with vertex set $`V`$ and edge set $`E`$, and let each vertex and edge carry a **kind** label (`CpgNodeKind`, `CpgEdgeKind`).

- A **[graph isomorphism](../GLOSSARY.md#isomorphism--subgraph-isomorphism)** between $`G_1`$ and $`G_2`$ is a bijection $`\mu : V_1 \to V_2`$ that preserves edges in both directions: $`(u,v) \in E_1 \iff (\mu(u), \mu(v)) \in E_2`$. It says the two graphs are *the same graph up to renaming*.
- **Subgraph isomorphism** relaxes the whole‑graph requirement: given a small **pattern** $`P = (V_P, E_P)`$ and a large **target** $`G = (V_G, E_G)`$, find an injection $`\mu : V_P \to V_G`$ that maps every pattern edge onto an edge of the target. When only the "forward" direction is required — every pattern edge must have a compatible target edge, but the target may carry *extra* edges among the matched nodes — the map is a **monomorphism** (a non‑induced embedding). This is exactly what `libcpg` computes: a pattern is found when it embeds into the target, regardless of whatever else the target does around the matched nodes.

Formally, `libcpg` searches for an injection $`\mu`$ such that

```math
\forall (u,v) \in E_P : \exists\, e \in E_G \text{ with } \operatorname{src}(e)=\mu(u),\ \operatorname{tgt}(e)=\mu(v),\ \text{and } \operatorname{compatible}\big(\operatorname{kind}(u,v),\, \operatorname{kind}(e)\big),
```

together with $`\operatorname{compatible}\big(\operatorname{kind}(u),\operatorname{kind}(\mu(u))\big)`$ at every vertex. The predicate $`\operatorname{compatible}`$ is the tunable part (Section 4): strict equality of kinds, or looser category‑level agreement.

### Why it is hard

The decision problem "does $`P`$ embed into $`G`$?" is **NP‑complete** in general (it contains, e.g., the clique and Hamiltonian‑path problems as special cases). There is no known polynomial‑time algorithm; a naïve search tries every injection from $`V_P`$ into $`V_G`$, of which there are $`\frac{|V_G|!}{(|V_G| - |V_P|)!}`$ — astronomically many. The engineering goal is therefore not to beat NP‑completeness but to **prune aggressively** so the search stays cheap on the sparse, labelled graphs that real code produces. VF2 is built for precisely that regime.

---

## 2. VF2 as a state‑space search

VF2 (Cordella, Foggia, Sansone, Vento [[1]](#references)) frames matching as a depth‑first walk over **partial mappings**. A state $`s`$ holds a partial injection $`\mu_s`$ covering some pattern nodes; the search grows $`\mu_s`$ one pair $`(p, t)`$ at a time, checks that the extension stays consistent (a *feasibility* test), recurses, and **backtracks** by undoing the extension when a branch is exhausted. A state whose mapping covers *all* of $`V_P`$ is a complete match and is emitted.

`libcpg`'s state is the `Vf2State` struct. Its fields correspond one‑to‑one with the classical VF2 state description:

```rust
pub struct Vf2State<'a> {
    pattern: &'a CodePropertyGraph,          // P
    target:  &'a CodePropertyGraph,          // G
    mapping:          FxHashMap<NodeId, NodeId>, // μ  : pattern → target
    reverse_mapping:  FxHashMap<NodeId, NodeId>, // μ⁻¹: target  → pattern
    unmapped_pattern: FxHashSet<NodeId>,     // pattern nodes not yet in μ
    unused_target:    FxHashSet<NodeId>,     // target nodes not yet in μ
    pattern_terminal: FxHashSet<NodeId>,     // fringe on the pattern side
    target_terminal:  FxHashSet<NodeId>,     // fringe on the target side
    order:            Vec<Vf2Frame>,         // explicit push‑order stack (undo log)
}
```

The `order` stack is `libcpg`'s correctness backbone; Section 5 explains why it must exist.

![VF2 state machine: initialise, generate candidate pairs from the terminal fringe, test feasibility, push a mapping and recurse, or backtrack via the push‑order stack, emitting a match when the mapping is complete.](../diagrams/vf2-state-machine.svg)

*Figure — the VF2 control loop as a state machine: candidate generation → feasibility → push/recurse → pop, with completion emitting a `PatternMatch`. Source: [`diagrams/vf2-state-machine.puml`](../diagrams/vf2-state-machine.puml).*

### 2.1 Terminal sets: search the fringe first

The single most important pruning idea is to **grow the mapping along edges** rather than jumping to arbitrary nodes. The [terminal sets](../GLOSSARY.md#terminal-set-vf2) `pattern_terminal` and `target_terminal` are the "fringes": pattern (resp. target) nodes that are *adjacent to an already‑mapped node but not yet mapped themselves*. Candidate generation prefers them:

- if both fringes are non‑empty, pick one pattern fringe node $`p`$ and pair it with **every** target fringe node $`t`$;
- otherwise (the mapping is empty, or a connected component has been fully consumed), fall back to any unmapped pattern node paired with every unused target node.

Preferring the fringe keeps the partial mapping **connected**, which means each new pair is immediately constrained by edges to already‑mapped nodes — so infeasible pairs are rejected early instead of deep in the tree.

---

## 3. The matching algorithm in literate form

The recursion is small enough to read whole. In `libcpg` it is `Vf2Matcher::vf2_search`; here it is as literate pseudocode.

```text
Algorithm VF2‑Match(P, G, max_matches):
  s       ← empty state over (P, G)         # μ = ∅, both fringes empty
  results ← [ ]
  Search(s, results)
  return results

procedure Search(s, results):
  ❶ if max_matches > 0 and |results| >= max_matches: return     # honour the bound
  ❷ if s.unmapped_pattern = ∅:                                  # μ covers all of V_P
        results.append( snapshot(s.mapping) as PatternMatch )    # a complete embedding
        return
  ❸ for (p, t) in CandidatePairs(s):        # fringe pairs first, else unmapped × unused
        if Feasible(s, p, t):               # Section 4
            Push(s, p, t)                    # extend μ; record an undo frame
            Search(s, results)               # recurse one level deeper
            Pop(s)                           # EXACT inverse of Push (Section 5)
            if max_matches > 0 and |results| >= max_matches: return
```

Three properties are worth stating explicitly, because the correctness of *every* pattern query depends on them:

1. **Completeness.** With `max_matches = 0` the loop at ❸ tries every feasible candidate for the chosen pattern node, so `Search` enumerates **every** embedding — not just the first. (`with_max_matches(k)` for $`k>0`$ stops after $`k`$.)
2. **Soundness.** A mapping is emitted at ❷ only after each incremental `Push` passed `Feasible`, so the accumulated injection satisfies the edge‑ and kind‑constraints of Section 1 by construction.
3. **No graph copying.** The search mutates one shared state and undoes each step; it never clones $`P`$ or $`G`$. Memory stays $`O(|V_P| + |V_G|)`$ — VF2's headline advantage over Ullmann‑style matchers.

The public surface that drives this is deliberately tiny:

```rust
use libcpg::pattern::{Vf2Matcher, SubgraphMatcher}; // SubgraphMatcher brings find_matches into scope

let matcher = Vf2Matcher::new()
    .with_strict_kinds(true)   // exact node‑kind equality (Section 4)
    .with_strict_edges(true)   // exact edge‑kind equality
    .with_max_matches(0);      // 0 = enumerate every embedding

let matches: Vec<libcpg::PatternMatch> = matcher.find_matches(&pattern, &target);
```

`find_matches` is the one required method of the `SubgraphMatcher` trait; `Vf2Matcher` is its VF2 implementation. Each returned [`PatternMatch`](../GLOSSARY.md#confidence-pattern-match) exposes `node_mapping` (pattern `NodeId` → target `NodeId`), `match_size()`, and `root` (one representative matched target node).

---

## 4. Feasibility: the pruning rules

`Feasible(s, p, t)` decides whether mapping pattern node $`p`$ to target node $`t`$ can possibly extend to a full match. `libcpg` applies two checks — a **semantic** check on kinds and a **syntactic** check on incident edges.

### 4.1 Node compatibility (strict vs. relaxed)

- **Strict** (`with_strict_kinds(true)`): the [node kinds](../GLOSSARY.md#node-kind--edge-kind) must be *identical* up to their tag, i.e. `NodeKindTag::from_kind(p) == NodeKindTag::from_kind(t)`. An `If` matches only an `If`.
- **Relaxed** (`with_strict_kinds(false)`, the default): agreement at the level of **category** is enough. Two nodes are compatible if they are both declarations, both expressions, or both statements — or, failing that, share the exact tag. The categories are fixed:

  | Category | `CpgNodeKind` members |
  |---|---|
  | Declaration | `Module`, `Class`, `Struct`, `Enum`, `Trait`, `Function`, `Variable`, `Field` |
  | Expression | `BinaryOp`, `UnaryOp`, `Call`, `MemberAccess`, `IndexAccess`, `Identifier`, `Literal`, `Lambda` |
  | Statement | `Return`, `If`, `While`, `For`, `Loop`, `Match`, `Break`, `Continue` |

  Relaxed matching is what lets a template written with a `Class` node match a target `Struct`, or an interface‑role `Trait` match a concrete `Class` — the basis of [design‑pattern detection](07-design-pattern-detection.md), which runs VF2 in this mode for recall.

### 4.2 Edge consistency

For the candidate $`(p,t)`$, every pattern edge that touches $`p`$ **and whose other endpoint is already mapped** must have a compatible partner in the target:

- **Outgoing:** for each pattern edge $`p \to q`$ with $`q`$ already mapped to $`\mu(q)`$, the target must contain a compatible edge $`t \to \mu(q)`$.
- **Incoming:** for each pattern edge $`r \to p`$ with $`r`$ already mapped to $`\mu(r)`$, the target must contain a compatible edge $`\mu(r) \to t`$.

Edges are compared by `edges_compatible`: under `with_strict_edges(true)` the `CpgEdgeKind` values must be equal; under the relaxed default, both being AST, both CFG, both DFG, or both call edges suffices (via `is_ast` / `is_cfg` / `is_dfg` / `is_call`). Because only pattern edges are required to have target partners, the search accepts non‑induced embeddings, as promised in Section 1.

> **Scope note.** `libcpg` implements VF2's core **syntactic feasibility** rule (consistency with the already‑mapped neighbourhood) and uses the terminal sets to *order* candidates. It does not add VF2's optional *k*‑look‑ahead cardinality cuts (the $`R_{term}`$/$`R_{new}`$ rules that compare fringe sizes). Those cuts prune more but are not required for correctness; on the sparse graphs of real code the consistency rule plus connected growth already keep the tree small.

![Pattern graph on the left, target CPG on the right, with the matched subgraph and the node mapping highlighted.](../diagrams/vf2-pattern-target.svg)

*Figure — a pattern graph embedded into a larger target CPG; highlighted nodes/edges show one satisfying injection $`\mu`$. Source: [`diagrams/vf2-pattern-target.dot`](../diagrams/vf2-pattern-target.dot).*

---

## 5. Backtracking and the pop‑order correctness fix

Depth‑first search is only correct if **`Pop` is the exact inverse of `Push`**. When the search abandons a branch it must restore the state — mapping, reverse mapping, unmapped/unused sets, *and both terminal fringes* — to precisely what it was before the corresponding `Push`. Get this wrong and the search silently drops valid embeddings.

`libcpg` guarantees exact inversion with an explicit **push‑order stack** of undo frames:

```rust
struct Vf2Frame {
    pattern_node: NodeId,          // the pair this push mapped …
    target_node:  NodeId,
    pattern_was_terminal: bool,    // … its prior fringe membership …
    target_was_terminal:  bool,
    pattern_terminal_added: Vec<NodeId>, // … and the fringe entries THIS push inserted
    target_terminal_added:  Vec<NodeId>,
}
```

`Push` records, in the frame, only the fringe entries it actually inserted (a neighbour already on the fringe is justified by *another* mapped node and must survive the later `Pop`). `Pop` then reverses exactly those insertions, removes the mapped pair, and restores the pair's own prior fringe membership — nothing more, nothing less.

### Why this is not a formality

The struct exists to fix two real bugs that a straight‑line target masks but a branching one exposes:

1. **`Pop` removed an arbitrary pair.** The earlier implementation deleted `mapping.iter().last()` — an *arbitrary* `FxHashMap` entry — instead of the last‑pushed one. On a path target this is harmless (all nodes are mapped before any backtrack, so *N* arbitrary pops empty the mapping just like *N* correct pops). On a branching target it corrupts the partial mapping mid‑search.
2. **Terminal‑set removal was a silent no‑op**, so the fringes drifted and candidate generation went wrong after the first backtrack.

The discriminating regression is the **diamond**:

```text
Pattern P (a directed path):        Target G (a diamond):

     p0                                       t0
      │                                      ╱  ╲
      ▼                                    t1    t2
     p1                                      ╲  ╱
      │                                       t3
      ▼
     p2                     with edges t0→t1, t0→t2, t1→t3, t2→t3
```

The path $`p_0 \to p_1 \to p_2`$ has **exactly two** embeddings into the diamond, and they diverge at the *middle* node:

```math
\mu_A = \{\, p_0\!\mapsto t_0,\ p_1\!\mapsto t_1,\ p_2\!\mapsto t_3 \,\}, \qquad
\mu_B = \{\, p_0\!\mapsto t_0,\ p_1\!\mapsto t_2,\ p_2\!\mapsto t_3 \,\}.
```

After committing $`p_0 \mapsto t_0`$, the middle node $`p_1`$ has two feasible targets ($`t_1`$ and $`t_2`$). Finding the *second* embedding requires a **partial** backtrack: pop only $`p_2`$ and $`p_1`$ while **retaining** $`p_0 \mapsto t_0`$, then retry $`p_1 \mapsto t_2`$. Exactly this is what the buggy `Pop` could not do; with the frame stack the matcher returns both $`\mu_A`$ and $`\mu_B`$, no more and no fewer. The inline test `test_multi_embedding_backtracking` in `src/pattern/vf2.rs` asserts this, and it is the anchor for the completeness discussion in [`scientific/03-vf2-completeness.md`](../scientific/03-vf2-completeness.md).

---

## 6. Complexity

Let $`N = |V_P|`$ be the number of pattern nodes (and assume the target is of comparable order). VF2's search tree has depth $`N`$; without pruning the number of partial mappings is bounded by the number of injections, $`O(N!)`$, and each feasibility test inspects the incident edges in $`O(N)`$ time. Hence the classical worst case:

```math
T_{\text{VF2}}(N) = O\!\left(N!\,N\right).
```

This bound is unavoidable in the worst case — subgraph isomorphism is NP‑complete — but it is pessimistic for code. Two structural facts make the practical cost far smaller:

- **Sparsity.** CPGs are near‑linear in their edges; a node's neighbourhood is small, so the edge‑consistency rule rejects most candidates immediately.
- **Connected growth.** Terminal‑set ordering keeps the partial mapping connected, so after the first pair each new candidate is pinned by an edge to something already mapped — collapsing the branching factor.

Reported behaviour (from `libcpg`'s own doc‑comments and the VF2 literature [[1]](#references)) is roughly $`O(N^2 M)`$ in the typical case, where $`M`$ is the target size, and closer to $`O(N)`$ when strong labels prune early. Two knobs trade completeness for latency:

| Knob | Effect |
|---|---|
| `with_max_matches(k)`, $`k>0`$ | stop after $`k`$ embeddings — e.g. $`k = 1`$ for a Boolean "does it occur?" query |
| `with_strict_kinds(true)` / `with_strict_edges(true)` | fewer compatible pairs ⇒ smaller tree (higher precision, lower recall) |

Memory is $`O(N + M)`$ throughout: the mapping tables, the two fringes, and the undo stack, all over shared references.

---

## 7. From exact matching to everything else

VF2 is the exact, all‑or‑nothing end of `libcpg`'s pattern facilities. It underpins two higher‑level capabilities and has one graded sibling:

- **Design‑pattern detection** ([file 07](07-design-pattern-detection.md)) runs `Vf2Matcher` in relaxed mode against a library of Gang‑of‑Four template graphs, then scores each embedding by template completeness — trading precision for recall.
- **Pattern templates** (`PatternTemplate` → `to_pattern_graph`) let callers describe a pattern declaratively (node/edge constraints) and compile it to a pattern CPG that VF2 consumes; see the API in [`components/patterns/vf2-matching.md`](../components/patterns/vf2-matching.md).
- **[Graph similarity](06-graph-similarity.md)** ([file 06](06-graph-similarity.md)) answers the *fuzzy* version of the same question — "how alike are these two graphs?" — with scores in $`[0,1]`$ (Jaccard, cosine, Weisfeiler‑Lehman, approximate graph‑edit distance) when an exact embedding is too rigid.

When you need certainty about a precise shape, use VF2; when you need a tolerance dial, use similarity; when you need a named catalogue, use the GoF detectors.

---

## References

1. Cordella, L. P., Foggia, P., Sansone, C., Vento, M. (2004). *A (Sub)graph Isomorphism Algorithm for Matching Large Graphs.* IEEE TPAMI 26(10). DOI: [10.1109/TPAMI.2004.75](https://doi.org/10.1109/TPAMI.2004.75)
