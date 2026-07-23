# Design Decisions (ADR index)

This directory holds `libcpg`'s **Architecture Decision Records** (ADRs): short,
durable notes that capture *why* a load-bearing design choice was made, not just
*what* the code does. The API reference in [`../api/`](../api/graph-reference.md)
tells you the shapes; the [component guides](../components/) tell you how to use
them; the [theory pillar](../theory/00-overview.md) explains the science. An ADR
answers the remaining question — **"why this and not the obvious alternative?"** —
so that a future reader (or a rewrite from scratch) inherits the reasoning and
does not silently undo a decision that was made for a good reason.

## What an ADR is

An ADR records **one** decision. Following Michael Nygard's original format, each
record here has a fixed skeleton so it can be skimmed:

| Section | Question it answers |
| :--- | :--- |
| **Status** | Is this decision proposed, accepted, superseded? |
| **Context** | What forces (requirements, constraints, prior art) made a decision necessary? |
| **Decision** | What did we choose, stated plainly? |
| **Consequences** | What do we gain and what do we pay — the honest pro/con ledger? |
| **Alternatives considered** | What else was on the table, and why was it rejected? |

The value is in the last two sections: a decision without its rejected
alternatives is folklore, and a decision without its costs is marketing.

## How to read one

1. Start with **Status** and the one-sentence **Decision** to know what you are
   looking at.
2. Read **Context** only if you need to understand the pressure behind it.
3. Consult **Consequences** before you build on the decision — the *Negative*
   column is where the sharp edges live (traversal must filter by edge kind,
   the data-flow sweep over-approximates in branches, relaxed matching trades
   precision for recall, and so on).
4. Follow the cross-links. Each ADR points back into
   [`../theory/`](../theory/00-overview.md) for the formal grounding and into
   [`../architecture/`](../architecture/overview.md) for where the decision
   lives in the module map, and links every term to the
   [glossary](../GLOSSARY.md).

## Index

| ADR | Decision | Status |
| :--- | :--- | :--- |
| [0001 — Unified overlay graph on petgraph](0001-unified-overlay-graph.md) | One shared node set; AST / CFG / DFG / PDG are typed *edge overlays* over it, stored in a `petgraph` `DiGraph`. | Accepted |
| [0002 — Mode B: `build_from_tree`](0002-mode-b-build-from-tree.md) | Accept a caller-parsed tree-sitter tree; keep the `rholang` / `metta` features empty `cfg` toggles; match grammar pins to pgmcp to avoid duplicate C symbols. | Accepted |
| [0003 — AST-ordered reaching definitions](0003-ast-ordered-reaching-defs.md) | Build the DFG from a single AST-ordered, flow-sensitive reaching-definitions sweep rather than SSA or a CFG fixed point. | Accepted |
| [0004 — Relaxed VF2 detection](0004-relaxed-vf2-detection.md) | Detect Gang-of-Four patterns with a *relaxed* (category-level) VF2 matcher plus a completeness-scaled confidence, not strict isomorphism or pure ML. | Accepted |
| [0005 — Feature-flag taxonomy](0005-feature-flag-taxonomy.md) | `default = []`: every grammar and every analysis is opt-in via Cargo features, grouped into `lang-*` sets and an umbrella `full`. | Accepted |

These five are not independent. ADR-0001 fixes the substrate that all analyses
write onto; ADR-0002 and ADR-0005 together explain how a language reaches that
substrate and what you must switch on to get there; ADR-0003 and ADR-0004 are
two analyses *over* the substrate (one always-on, one feature-gated) that each
chose a pragmatic algorithm over a textbook-maximal one. Read 0001 first.

![Module architecture of libcpg, showing the graph core and the analysis modules layered over it](../diagrams/module-architecture.svg)

*Figure — the module map these decisions inhabit: the `graph` core (ADR-0001) with the `builder`, `pattern`/`patterns`, `algorithms`, and `gnn` modules layered on top. Source: [`diagrams/module-architecture.puml`](../diagrams/module-architecture.puml).*

## Conventions

- **Numbering.** Records are numbered in the order they were accepted
  (`0001`…). Numbers are never reused; a reversed decision is marked
  *Superseded* and a new record supersedes it, so history stays legible.
- **Status lifecycle.** `Proposed → Accepted → (Superseded by NNNN | Deprecated)`.
  All five current records are **Accepted** and reflect the shipped `v0.1.1`
  code.
- **Grounding.** Every claim here is traceable to source: the
  [`Cargo.toml`](../engineering/01-build-and-features.md) feature block, the
  `#[cfg(any())]` retired code paths in `src/builder/dfg.rs`, and the inline
  `#[cfg(test)]` tests that pin each behaviour. Where a record cites external
  literature it lists it verbatim under **References**; where a decision is a
  pure engineering trade-off with no literature behind it, that is stated
  plainly.

## References

This index cites no external literature; each decision record below carries its
own **References** section listing only the works it cites. Terminology is
defined once in the [glossary](../GLOSSARY.md) and linked from every record.
