# Diagram catalog

Every figure in the `libcpg` documentation is a **committed source** (`.puml` for PlantUML, `.dot` for Graphviz) rendered to a **committed `.svg`** that the Markdown pages embed. Sources are version-controlled so figures are reproducible and reviewable; SVGs are committed so they render on GitHub (which does not render PlantUML/Graphviz inline). This directory holds all 48 figures plus the shared theme.

## Conventions

- **Theme.** Every `.puml` begins with `!include _libcpg-theme.iuml`, which fixes the font, white background, no-shadow, and arrow style so all figures read as one system. Graphviz `.dot` files reuse the same palette by literal hex.
- **Colour → concept palette** (fill / line):

  | Concept | Fill | Line | | Concept | Fill | Line |
  |---|---|---|---|---|---|---|
  | AST layer | `#DBE4FF` | `#4C6EF5` | | Type / inheritance | `#E9ECEF` | `#868E96` |
  | CFG layer | `#D3F9D8` | `#2F9E44` | | Pattern match / highlight | `#FFE3E3` | `#E03131` |
  | DFG layer | `#FFF3BF` | `#F08C00` | | GNN / embedding | `#FCC2D7` | `#C2255C` |
  | PDG layer | `#E5DBFF` | `#7048E8` | | Import / scope | `#FFE8CC` | `#E8590C` |
  | Call graph | `#C3FAE8` | `#0CA678` | | Generic node | `#F1F3F5` | `#495057` |

- **Colouring syntax** (PlantUML build quirks pinned): activity nodes colour with a *trailing* stereotype — `:label;<<#HEX>>` (never `#HEX:label;`); classes/states/components/participants use a `#HEX` background; mindmap nodes use `*[#HEX] text`. Do not use `skinparam Padding` (deprecated). Math in labels uses `<latex>…</latex>` (JLaTeXMath).
- **Legends.** Every figure carries a self-contained legend mapping its colours to concepts.

## Rendering & the render gate

```bash
cd docs/diagrams
plantuml -tsvg -nometadata *.puml            # render all PlantUML sources
for f in *.dot; do dot -Tsvg "$f" -o "${f%.dot}.svg"; done   # render all Graphviz sources
# GATE — must print nothing:
grep -lF -e "contains errors" -e "syntax error" -e "deprecated" -e "Please use CSS" *.svg
```

Toolchain used: PlantUML (JLaTeXMath-enabled) + Graphviz `dot` 15.1.0. A figure passes the gate when its SVG contains no error/deprecation marker and is not a tiny error card (all committed SVGs are ≥ 5 KB).

## Figure index

Grouped by subject. Each row: the source (its `.svg` sibling is embedded by the listed pages).

### Core & architecture
| Source | Type | Depicts | Embedded by |
|---|---|---|---|
| `cpg-overlay.dot` | graph | AST⊕CFG⊕DFG overlays on one shared node set (hero figure) | README, docs/README, theory/00–01, architecture/graph-data-model, api/graph-reference, components/graph/overview, design/0001 |
| `cpg-vs-traditional.dot` | graph | Three separate views vs one unified CPG | theory/00, architecture/overview |
| `module-architecture.puml` | component | Crate modules and feature gates | architecture/overview, design/00 |
| `construction-pipeline.puml` | activity | parse → AST → CFG → DFG build | architecture/data-flow, components/builder/overview, api/builder-reference, usage/01 |
| `analysis-pipeline.puml` | activity | CPG → PDG/slice, VF2/GoF, algorithm, GNN | architecture/data-flow |
| `feature-flag-map.puml` | mindmap | Cargo feature taxonomy (`default = []`) | design/0005, engineering/01, usage/00 |

### Graph data model
| Source | Type | Depicts | Embedded by |
|---|---|---|---|
| `node-kind-taxonomy.puml` | mindmap | The 45 `CpgNodeKind` variants by group | theory/01, architecture/graph-data-model, api/graph-reference, components/graph/nodes |
| `edge-kind-taxonomy.puml` | mindmap | `CpgEdgeKind` grouped (AST/CFG/DFG/PDG/…) | theory/01, architecture/graph-data-model, api/graph-reference, components/graph/edges |

### Control flow, data flow, dependence
| Source | Type | Depicts | Embedded by |
|---|---|---|---|
| `cfg-example.dot` | graph | CFG of a small function with `CfgEdgeKind` labels | theory/02, components/builder/cfg |
| `cfg-control-constructs.dot` | graph | if / while / try CFG shapes | theory/02, components/graph/edges, components/builder/cfg |
| `reaching-defs-sweep.puml` | activity | AST-ordered reaching-defs sweep (strong/weak/loop) | theory/03, design/0003, components/builder/dfg, scientific/02 |
| `def-use-example.dot` | graph | Def-use / reaching-def edges | theory/03, components/graph/edges, components/builder/dfg, scientific/02 |
| `pdg-construction.puml` | activity | PDG build via reverse dominance frontier | theory/04, components/builder/pdg-and-slicing |
| `dominance-frontier.dot` | graph | Control dependence = reverse dominance frontier | theory/04, components/builder/pdg-and-slicing |
| `slice-example.dot` | graph | A backward slice highlighted over the PDG | theory/04, components/builder/pdg-and-slicing, usage/04 |
| `slicing-bfs.puml` | activity | Bounded backward/forward slice BFS | theory/04, components/builder/pdg-and-slicing, usage/04 |

### Patterns & similarity
| Source | Type | Depicts | Embedded by |
|---|---|---|---|
| `vf2-state-machine.puml` | state | VF2 search states + backtracking | theory/05, design/0004, components/patterns/vf2-matching |
| `vf2-pattern-target.dot` | graph | Pattern vs target with one embedding | theory/05, api/pattern-reference, components/patterns/vf2-matching, scientific/03 |
| `vf2-diamond.dot` | graph | Diamond target, exactly two embeddings | components/patterns/vf2-matching, scientific/03 |
| `similarity-metrics.puml` | mindmap | Jaccard / Cosine / WL / GraphEdit | theory/06, components/patterns/vf2-matching |
| `wl-kernel.dot` | graph | Weisfeiler-Lehman label refinement | theory/06, components/patterns/vf2-matching |
| `pattern-detection-pipeline.puml` | activity | GoF detect: template → relaxed VF2 → confidence | theory/07, design/0004, components/patterns/overview, usage/03 |
| `gof-taxonomy.puml` | mindmap | 23 GoF patterns by category | theory/07, api/pattern-reference, components/patterns/gang-of-four |
| `gof-singleton.puml` | class | Singleton structure & detection signature | theory/07, components/patterns/gang-of-four |
| `gof-factory-method.puml` | class | Factory Method structure | components/patterns/gang-of-four |
| `gof-observer.puml` | class | Observer structure | components/patterns/gang-of-four |
| `gof-strategy.puml` | class | Strategy structure | components/patterns/gang-of-four |
| `gof-command.puml` | class | Command structure | components/patterns/gang-of-four |
| `gof-decorator.puml` | class | Decorator structure | components/patterns/gang-of-four |
| `gof-adapter.puml` | class | Adapter structure | components/patterns/gang-of-four |
| `dpml-flow.puml` | activity | DPML template → pattern graph | components/patterns/dpml |
| `classification-flow.puml` | activity | FeatureVector → ClassificationMode → label | components/patterns/classification |

### Algorithms & complexity
| Source | Type | Depicts | Embedded by |
|---|---|---|---|
| `algorithm-detection-pipeline.puml` | activity | Per-function detection pipeline | theory/08, components/algorithms/overview |
| `algorithm-family-taxonomy.puml` | mindmap | `AlgorithmFamily` heuristic detectors | components/algorithms/families |
| `complexity-ladder.dot` | graph | `ComplexityClass` ordering with Big-O | theory/08, api/pattern-reference, components/algorithms/complexity |
| `complexity-heuristics.puml` | activity | Structure → complexity class (Master Theorem) | theory/08, components/algorithms/complexity |

### Graph neural network
| Source | Type | Depicts | Embedded by |
|---|---|---|---|
| `gnn-architecture.puml` | component | `CpgGnn` init → layers → embeddings | theory/09, components/gnn/overview |
| `gnn-message-passing.dot` | graph | Mean aggregation over AST/CFG/DFG neighbours | theory/09, components/gnn/message-passing |
| `gnn-receptive-field.dot` | graph | Receptive-field growth per layer | theory/09, components/gnn/overview, components/gnn/message-passing |
| `embedding-space.dot` | graph | Subgraph embeddings & cosine similarity | components/gnn/embeddings |

### Language frontends (incl. F1R3FLY Mode B)
| Source | Type | Depicts | Embedded by |
|---|---|---|---|
| `language-frontend-pipeline.puml` | component | ParserRegistry / Mode-B tree → NodeMapper dispatch | architecture/language-frontends, design/0002, components/builder/node-mapper, engineering/04 |
| `mode-b-sequence.puml` | sequence | `build_from_tree` with a caller-supplied grammar | architecture/language-frontends, design/0002, api/builder-reference, usage/01, usage/06 |
| `rholang-mapping.dot` | graph | Rholang constructs → CPG node kinds | architecture/language-frontends, components/builder/node-mapper, usage/06 |
| `metta-mapping.dot` | graph | MeTTa S-expressions → CPG node kinds | architecture/language-frontends, components/builder/node-mapper, usage/06 |

### Engineering, science, security
| Source | Type | Depicts | Embedded by |
|---|---|---|---|
| `build-equivalence.puml` | activity | `build` ≡ `build_from_tree` node/edge equality | scientific/01, engineering/02 |
| `measurement-methodology.puml` | activity | Rigorous benchmarking method (a documented gap) | scientific/04, engineering/03 |
| `threat-model-dataflow.puml` | activity | Untrusted input flow & trust boundaries | security/00 |
| `resource-bounds.puml` | mindmap | The construction/analysis resource caps | security/01 |
