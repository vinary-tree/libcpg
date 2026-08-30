# Tree-sitter parent-context formal verification plan

## Purpose

This plan governs pgmcp bug `dovetail-x-tree-parent-quadratic`. The explicit
preorder Abstract Syntax Tree (AST) work machine already knows a child's direct
raw tree-sitter parent when it schedules that child. Carrying this context must
remove repeated `Node::parent()` reconstruction without changing CPG topology,
transparent-node behavior, ownership classification, traversal order, or stack
safety.

## State and invariants

A work frame contains syntax node $`n`$, raw parent $`r`$, and effective CPG
parent $`e`$. For tree parent function $`p`$, included-node predicate $`I`$,
and scheduled child $`c`$, the transition is:

```math
r_c=n,\qquad
e_c=\begin{cases}
n & I(n),\\
e_n & \neg I(n).
\end{cases}
```

The proof obligations are:

1. the root has no raw or effective parent;
2. every scheduled child carries its direct raw parent exactly;
3. an included node becomes the effective parent of its children;
4. a transparent node preserves the prior effective parent;
5. every mapped CPG parent equals the nearest included syntax ancestor;
6. preorder visits each syntax node once; and
7. replacing an oracle parent with an equal carried parent preserves ownership
   classification.

Raw and effective parents are deliberately separate. Conflating them would
break CPG flattening when a grammar wrapper is omitted.

## Triangulation

| Method | Question |
|---|---|
| TLA+ / TLC | Does every included/transparent configuration of the bounded branching tree preserve raw/effective context, unique preorder visitation, and terminal coverage? |
| Rocq | Are child scheduling, transparent/included effective-parent transitions, and ownership-classification substitution universally valid? |
| Z3 | Can any of four forbidden raw-parent, effective-parent, or observational-equivalence states exist? |

Only after all three gates pass may production traversal frames or mapper APIs
change. Implementation tests must then cover root, direct assignment/value,
non-value, borrow, transparent wrapper, generated source shapes, exact CPG
parent/`AstChild` equivalence, and the unchanged 100,000-depth/50,000-width
256 KiB-stack regression. Before/after profiling remains headless and is
captured beneath `target/verification` under RSS/no-swap limits.
