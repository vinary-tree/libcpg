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

VF2 (Cordella, Foggia, Sansone, Vento [[1]](#references)) frames matching as a depth‑first walk over **partial mappings**. A state $`s`$ holds a partial injection $`\mu_s`$ covering some pattern nodes; the search grows $`\mu_s`$ one pair $`(p, t)`$ at a time, checks that the extension stays consistent (a *feasibility* test), and **backtracks** by undoing the extension when a branch is exhausted. `libcpg` realizes that depth-first control flow as typed heap frames rather than recursive calls. A state whose mapping covers *all* of $`V_P`$ is a complete match and is emitted.

`libcpg`'s `Vf2State` represents the classical state over an immutable dense
node domain. Ascending stable `NodeId` order defines each index:

```rust
pub struct Vf2State<'a> {
    pattern: &'a CodePropertyGraph,     // P; authoritative exact edges
    target:  &'a CodePropertyGraph,     // G; authoritative exact edges
    pattern_topology: Vf2Topology<'a>,  // ascending ids + unique degrees
    target_topology:  Vf2Topology<'a>,
    mapping: Vec<u32>,                  // dense P index → dense G index
    reverse_mapping: Vec<u32>,          // dense G index → dense P index
    unmapped_pattern: DenseSet,         // initially full ordered set
    unused_target: DenseSet,            // initially full ordered set
    pattern_terminal: DenseSet,         // fringe ordered set
    target_terminal: DenseSet,
    pattern_terminal_count: Vec<u32>,   // mapped-neighbour justifications
    target_terminal_count: Vec<u32>,
    pattern_terminal_delta: Vec<u32>,   // flat exact-undo logs
    target_terminal_delta: Vec<u32>,
    order: Vec<Vf2UndoFrame>,           // compact offsets into those logs
}
```

`DenseSet` combines membership bits with a [Fenwick tree](../GLOSSARY.md#fenwick-tree),
so it selects the first member or the first member at/after a cursor in logarithmic time.
It neither sorts hash entries nor scans an ever-growing mapped prefix. The `order` stack is
`libcpg`'s correctness backbone; Section 5 explains why it must exist. It is distinct from
`Vf2Matcher`'s private `Vec<Vf2SearchFrame>`: `order` restores state, whereas a search
frame stores only a pattern index, target cursor, terminal/full-set selector, and ownership
bit. Defunctionalizing the recursive continuation this way makes native call-stack
consumption independent of $`|V_P|`$.

![VF2 state machine: initialise a root search frame, advance its candidate cursor, test feasibility, push a mapping and child frame, or exhaust and undo the child, emitting a match when the mapping is complete.](../diagrams/vf2-state-machine.svg)

*Figure — the VF2 control loop as a heap state machine: candidate generation → feasibility → push child frame → pop-and-restore, with completion emitting a `PatternMatch`. Source: [`diagrams/vf2-state-machine.puml`](../diagrams/vf2-state-machine.puml).*

### 2.1 Terminal sets: search the fringe first

The single most important pruning idea is to **grow the mapping along edges** rather than jumping to arbitrary nodes. The [terminal sets](../GLOSSARY.md#terminal-set-vf2) `pattern_terminal` and `target_terminal` are the "fringes": pattern (resp. target) nodes that are *adjacent to an already‑mapped node but not yet mapped themselves*. Candidate generation prefers them:

- if both fringes are non‑empty, pick the smallest pattern-fringe `NodeId` $`p`$ and pair it with every target-fringe node $`t`$ in ascending `NodeId` order;
- otherwise (the mapping is empty, or a connected component has been fully consumed), fall back to the smallest unmapped pattern `NodeId`, paired with every unused target in ascending order.

Preferring the fringe keeps the partial mapping **connected**, which means each new pair is immediately constrained by edges to already‑mapped nodes — so infeasible pairs are rejected early instead of deep in the tree.
The dense view is sorted once when first requested. Fenwick selection then defines the
canonical depth-first sequence without a per-frame allocation or sort: graph insertion
order cannot change the result order, representative match root, or prefix selected by a
positive result cap.

---

## 3. The matching algorithm in literate form

`Vf2Matcher::vf2_search` is an iterative pushdown machine. A
`Frame(pattern_index, target_cursor, terminal_only, owns_mapping)` is the
defunctionalized continuation of one depth-first candidate loop: `target_cursor` is where
ordered-set selection resumes, and `owns_mapping` says whether exhausting the frame must
undo one extension.

```text
Algorithm VF2‑Match(P, G, max_matches):
  s       ← empty state over (P, G)         # μ = ∅, both fringes empty
  results ← [ ]
  frames  ← [CursorFrame(s, owns_mapping = false)]

  while frames is not empty:
    ❶ if max_matches > 0 and |results| >= max_matches: break
    ❷ f ← frames.last
    ❸ candidate ← NextEligibleAtOrAfter(s, f.target_cursor)
       if candidate does not exist:
          exhausted ← frames.pop
          if exhausted.owns_mapping: Pop(s)              # exact inverse of Push
          continue
    ❹ (p, t) ← candidate
       f.target_cursor ← DenseIndex(t) + 1                # suspended resume point
    ❺ if not Feasible(s, p, t): continue                  # Section 4
    ❻ Push(s, p, t)                                       # extend μ; log exact undo
    ❼ if s.unmapped_pattern = ∅:
          results.append(snapshot(s.mapping) as PatternMatch)
          Pop(s)                                          # leaf owns no child frame
       else:
          frames.push(CursorFrame(s, owns_mapping = true))

  for f in reverse(frames):                               # cap-safe unwind
    if f.owns_mapping: Pop(s)
  return results
```

Three properties are worth stating explicitly, because the correctness of *every* pattern query depends on them:

1. **Completeness.** With `max_matches = 0`, cursor advancement at ❹ tries every feasible candidate for the chosen pattern node, so the machine enumerates **every** embedding—not just the first. `with_max_matches(k)` for $`k>0`$ returns the first $`k`$ embeddings in canonical order.
2. **Soundness.** A mapping is emitted at ❼ only after each incremental `Push` passed `Feasible`, so the accumulated injection satisfies the edge- and kind-constraints of Section 1 by construction.
3. **Exact restoration.** Normal frame exhaustion and positive-cap unwinding both perform exactly one `Pop` for every live child frame, so the caller-visible state is empty after matching.
4. **No graph copying or native recursion.** The search mutates one shared state and undoes each step; it never clones $`P`$ or $`G`$, materializes a candidate vector for an internal frame, or allocates one native call frame per pattern node. The search PDA uses $`O(|V_P|)`$ compact heap frames.

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

`Feasible(s, p, t)` decides whether mapping pattern node $`p`$ to target node $`t`$ can possibly extend to a full match. `libcpg` applies three necessary checks: a **semantic** check on kinds, a distinct directed-degree check, and a **syntactic** check on incident edges.

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

Before reading exact edges, the dense topology rejects $`(p,t)`$ when $`p`$
has more distinct successors or more distinct predecessors than $`t`$. Every
distinct pattern neighbour needs a distinct image under an injection, so this
is a necessary condition. Counting unique endpoints rather than raw edges is
essential: current parallel-edge semantics require a compatible target edge
existentially and do not require equal multiplicity.

For the candidate $`(p,t)`$, every pattern edge that touches $`p`$ **and whose other endpoint is already mapped** must have a compatible partner in the target:

- **Outgoing:** for each pattern edge $`p \to q`$ with $`q`$ already mapped to $`\mu(q)`$, the target must contain a compatible edge $`t \to \mu(q)`$.
- **Incoming:** for each pattern edge $`r \to p`$ with $`r`$ already mapped to $`\mu(r)`$, the target must contain a compatible edge $`\mu(r) \to t`$.

Edges are compared by `edges_compatible`: under `with_strict_edges(true)` the `CpgEdgeKind` values must be equal; under the relaxed default, both being AST, both CFG, both DFG, or both call edges suffices (via `is_ast` / `is_cfg` / `is_dfg` / `is_call`). Because only pattern edges are required to have target partners, the search accepts non‑induced embeddings, as promised in Section 1.

> **Scope note.** The distinct-degree rule is label-independent look-ahead; it
> is not VF2++ or VF3 candidate ordering. `libcpg` does not add the classical
> $`R_{term}`$/$`R_{new}`$ fringe-cardinality cuts. Stronger cuts need a separate
> proof against this non-induced typed multigraph relation before they can enter
> production.

![Pattern graph on the left, target CPG on the right, with the matched subgraph and the node mapping highlighted.](../diagrams/vf2-pattern-target.svg)

*Figure — a pattern graph embedded into a larger target CPG; highlighted nodes/edges show one satisfying injection $`\mu`$. Source: [`diagrams/vf2-pattern-target.dot`](../diagrams/vf2-pattern-target.dot).*

---

## 5. Backtracking and the pop‑order correctness fix

Depth‑first search is only correct if **`Pop` is the exact inverse of `Push`**. When the search abandons a branch it must restore the state — mapping, reverse mapping, unmapped/unused sets, *and both terminal fringes* — to precisely what it was before the corresponding `Push`. Get this wrong and the search silently drops valid embeddings.

`libcpg` guarantees exact inversion with an explicit **push‑order stack** of undo frames:

```rust
struct Vf2UndoFrame {
    pattern_index: u32,        // the dense pair this push mapped
    target_index: u32,
    pattern_delta_start: u32,  // suffixes added to shared flat logs
    target_delta_start: u32,
}
```

`Push` increments one dense justification count per distinct incident neighbour
and appends the changed index to a flat delta log. A neighbour already on the
fringe may be justified by another mapped node and must survive a later `Pop`.
`Pop` replays precisely the two suffixes named by the compact frame, decrements
their counts, removes the mapped pair, and restores its membership—nothing more
and nothing less.

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
- **Necessary degree pruning.** A pair whose target has too few distinct
  predecessors or successors cannot extend to an injection and is rejected
  before exact incident-edge scans.

Reported behaviour (from `libcpg`'s own doc‑comments and the VF2 literature [[1]](#references)) is roughly $`O(N^2 M)`$ in the typical case, where $`M`$ is the target size, and closer to $`O(N)`$ when strong labels prune early. Two knobs reduce work after preserving their documented semantics:

| Knob | Effect |
|---|---|
| `with_max_matches(k)`, $`k>0`$ | stop after the first $`k`$ canonical embeddings—e.g. $`k = 1`$ for a Boolean "does it occur?" query; no positive result means the full search may still run |
| `with_strict_kinds(true)` / `with_strict_edges(true)` | fewer compatible pairs ⇒ smaller tree (higher precision, lower recall) |

The dense mappings, ordered sets, terminal counts, and lazy node metadata use
$`O(N+M)`$ memory over shared graph references. Flat terminal deltas can retain
$`O(|E_P|+|E_G|)`$ indices along a live branch. The explicit search stack uses
$`O(N)`$ compact cursor frames rather than $`O(NM)`$ candidate vectors. Native
call-stack use is $`O(1)`$ with respect to $`N`$, $`M`$, degree, and depth.

---

## 7. From exact matching to everything else

VF2 is the exact, all‑or‑nothing end of `libcpg`'s pattern facilities. It underpins two higher‑level capabilities and has one graded sibling:

- **Design‑pattern detection** ([file 07](07-design-pattern-detection.md)) runs `Vf2Matcher` in relaxed mode against a library of Gang‑of‑Four template graphs, then scores each embedding by template completeness — trading precision for recall.
- **Pattern templates** (`PatternTemplate` → `to_pattern_graph`) let callers describe a pattern declaratively (node/edge constraints) and compile it to a pattern CPG that VF2 consumes; see the API in [`components/patterns/vf2-matching.md`](../components/patterns/vf2-matching.md).
- **[Graph similarity](06-graph-similarity.md)** ([file 06](06-graph-similarity.md)) answers the *fuzzy* version of the same question — "how alike are these two graphs?" — with scores in $`[0,1]`$ (Jaccard, cosine, Weisfeiler‑Lehman, or a bounded bipartite graph-edit upper bound) when an exact embedding is too rigid.

When you need certainty about a precise shape, use VF2; when you need a tolerance dial, use similarity; when you need a named catalogue, use the GoF detectors.

---

## References

1. Cordella, L. P., Foggia, P., Sansone, C., Vento, M. (2004). *A (Sub)graph Isomorphism Algorithm for Matching Large Graphs.* IEEE TPAMI 26(10). DOI: [10.1109/TPAMI.2004.75](https://doi.org/10.1109/TPAMI.2004.75)
