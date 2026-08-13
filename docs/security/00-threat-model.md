# Threat Model

`libcpg` is a static-analysis library: its entire purpose is to read code — often
code you did not write and do not trust — and answer structural questions about it.
[Code Property Graphs](../GLOSSARY.md#code-property-graph-cpg) were introduced by
Yamaguchi et al. to *discover vulnerabilities* in exactly such untrusted code [[1]](#references),
so the analyzer must itself be robust to hostile input. This page states what is
being protected, from whom, and where the trust boundaries lie. The concrete knobs
that enforce these boundaries are catalogued in
[input and resource hardening](01-input-and-resource-hardening.md).

## Assets

| Asset | Why it matters |
| --- | --- |
| Availability of the host process | The most realistic attack is denial of service: a crafted input that makes construction or analysis consume unbounded time or memory and starve the embedding application. |
| Integrity of the analysis result | A downstream tool may route decisions on `libcpg`'s output; a graph deserialized from an untrusted source could be crafted to mislead. |
| The embedding application's process boundary | `libcpg` runs in-process; a memory-safety failure would compromise the whole host. Rust's safety guarantees and tree-sitter's no-execution parsing are the primary defences here. |

Note what is **not** among the assets: `libcpg` does not manage secrets, open
network connections, spawn processes, or touch the filesystem beyond the one
convenience method `build_file`, which reads a path the caller supplied.

## Threat actors

- **A supplier of untrusted source code.** Anyone who can influence the bytes
  passed to `build`, `build_file`, or `build_from_tree` — for instance a
  code-review bot analyzing an attacker's pull request, or a service that indexes
  arbitrary repositories.
- **A supplier of untrusted serialized graphs.** With the `serde` feature, anyone
  who can influence the bytes handed to a deserializer that reconstructs a
  `CodePropertyGraph`.

Both are *remote, unprivileged* actors: they provide data, not code that `libcpg`
executes.

## Trust boundaries and data flow

There are two independent boundaries. The primary one is the parse-and-analyze
pipeline; the secondary one is graph deserialization.

![Threat-model dataflow and trust boundaries](../diagrams/threat-model-dataflow.svg)

*Figure — untrusted source bytes pass the `max_file_size` guard (in `build`),
through a no-execution tree-sitter parse, into bounded analyses; deserialization is
a separate boundary. Source:
[`diagrams/threat-model-dataflow.puml`](../diagrams/threat-model-dataflow.puml).*

### Boundary 1 — untrusted source bytes

`build(source, language)` is the guarded entry point. Before parsing it rejects
input larger than `CpgBuilderConfig::max_file_size` (default 10 MB), returning
`Error::Construction`. This is the first-line denial-of-service guard: it caps the
size of the tree, and therefore the size of the CPG and the cost of every
downstream pass, at construction time.

The critical caveat — visible in the diagram's note — is that **Mode B skips this
check**. [`build_from_tree`](../GLOSSARY.md#mode-b--build_from_tree) accepts a tree
the caller already parsed and runs the identical post-parse pipeline *without* the
`max_file_size` test, because at that point the potentially-expensive parse has
already happened and the caller owns the tree. Consequently, **a Mode-B caller
must impose its own input bound** (size, and ideally a parse timeout) before
calling `build_from_tree`. See
[input and resource hardening](01-input-and-resource-hardening.md#parser-timeouts-are-the-callers-job).

### tree-sitter parses; it does not execute

A recurring worry with "run a tool over attacker code" is that the code might
*run*. It does not. [tree-sitter](../GLOSSARY.md#tree-sitter) is a parser: it reads
bytes and produces a concrete syntax tree. It never evaluates, compiles, or
otherwise executes the program under analysis, and `libcpg`'s mappers only read
node kinds and source spans off that tree. There is no `eval`, no macro expansion,
no build step, and no code-loading anywhere in the construction path. The residual
parser risks are resource-exhaustion (bounded above) and parser bugs in the
grammar, not code execution.

### Boundary 2 — untrusted serialized graphs

With the `serde` feature, the graph types derive `Serialize`/`Deserialize` (with
custom helpers for `Arc<str>` fields). There is **no bespoke, validated import
function** — round-tripping is done with the caller's own `serde_json` (or similar),
as described in [`usage/05-serialization.md`](../usage/05-serialization.md).
Deserialization is therefore its own trust boundary: a `CodePropertyGraph`
reconstructed from bytes is only as trustworthy as those bytes. Treat deserializing
a graph like deserializing any other complex structure — **load only graphs from
sources you trust**, because a crafted payload could encode a misleading or
internally inconsistent graph that later analyses take at face value.

## Results are advisory, not authoritative

Several of `libcpg`'s outputs are **heuristics**, and must not be treated as
security guarantees:

- [Design-pattern](../GLOSSARY.md#design-pattern) detection scores structural
  matches by [confidence](../GLOSSARY.md#confidence-pattern-match); a high score is
  evidence, not proof, that a pattern is present.
- [Algorithm-family](../GLOSSARY.md#algorithm-family) detection infers intent
  from structural signatures and can be wrong. Evidence-aware
  [complexity-class](../GLOSSARY.md#complexity-class--big-o) estimation cites
  validated CFG/DFG loop evidence when available, but its $`O(n^d)`$ model is
  still advisory rather than a symbolic cost or termination proof (see
  [`components/algorithms/complexity.md`](../components/algorithms/complexity.md)).
- [GNN](../GLOSSARY.md#graph-neural-network-gnn) embeddings summarise structure;
  similarity is a soft signal.
- [Martin package metrics](../GLOSSARY.md#martin-package-metrics) are exact for
  the supplied dependency projection and module assignment, but those inputs
  are architectural policy. A stable-dependency violation is evidence for
  review, not authorization to remove or invert a dependency.
- [Reflexion-model](../GLOSSARY.md#reflexion-model) categories are exact for
  the supplied prefix policy and dependency paths. They do not validate
  filesystem identity or authorize a layering change.
- [Architectural-layering](../GLOSSARY.md#architectural-layer--layering-violation)
  categories are exact for supplied SCC and level values. They do not prove
  architectural intent or authorize moving, deleting, or inverting code.

Use these to *prioritise* human attention, not to *gate* a security decision on
their own. Exact structural analyses are exact with respect to their input
projection: SCC decomposition exactly partitions the projected CFG or resolved
call graph, and [program slicing](../GLOSSARY.md#program-slicing) exactly traverses
the modelled PDG. Neither claim fills in facts absent from the CPG: unresolved
calls are omitted, and the PDG reflects the dependencies produced by libcpg's
[AST-ordered](../GLOSSARY.md#ast-ordered-reaching-definitions) data flow rather
than a full fixpoint. These results are not guarantees of runtime behaviour.

## Scope

**In scope.** Denial-of-service via oversized or pathological input; the
deserialization trust boundary; being clear about the advisory nature of
heuristics; keeping the parse path free of code execution.

**Out of scope.** Sandboxing the host process; authenticating or encrypting inputs;
supply-chain integrity of the tree-sitter grammars and other dependencies;
side-channel resistance. These are the responsibility of the embedding application
and its build pipeline.

## Mitigations at a glance

| Threat | Mitigation | Where |
| --- | --- | --- |
| Oversized source (DoS) | `max_file_size` guard in `build` (10 MB default) | [hardening §max_file_size](01-input-and-resource-hardening.md#construction-max_file_size) |
| Oversized source via Mode B | Caller must bound size/time before `build_from_tree` | [hardening §parser timeouts](01-input-and-resource-hardening.md#parser-timeouts-are-the-callers-job) |
| Expensive/adversarial call resolution | Scope-depth, retained-candidate, and sorted-call caps plus cycle guards | [hardening §call resolution](01-input-and-resource-hardening.md#call-resolution-scope-candidate-and-call-caps) |
| Explosive pattern search | VF2 `max_matches` cap + strict toggles | [hardening §VF2](01-input-and-resource-hardening.md#pattern-matching-vf2) |
| Non-terminating data flow | Structural AST sweep, two loop passes, and a malformed-cycle path guard | [hardening §DFG](01-input-and-resource-hardening.md#data-flow-structural-sweep-bounds) |
| Unbounded slice | `backward_slice` / `forward_slice` `max_nodes` cap | [hardening §slicing](01-input-and-resource-hardening.md#slicing-max_nodes) |
| Expensive or divergent centrality | hard iteration ceiling, explicit traversal budgets, finite-option validation, and complete projection validation | [hardening §centrality](01-input-and-resource-hardening.md#centrality-iteration-ceilings-traversal-budgets-and-safe-numerics) |
| Adversarial community search or weights | 50-pass/10-level ceilings plus complete CSR, resolution, weight, and overflow validation | [hardening §community detection](01-input-and-resource-hardening.md#community-detection-pass-level-and-numeric-bounds) |
| Adversarial cohesion topology, weighted cut, or removal trace | 200,000-edge truss, 1,024-node minimum-cut, and 10,000-step attack ceilings; iterative traversals; complete CSR and weight validation | [hardening §cohesion](01-input-and-resource-hardening.md#cohesion-edge-matrix-traversal-and-numeric-bounds) |
| Expensive or slow-converging spectral topology | 1,000-iteration ceiling, iterative sparse traversal, complete CSR/options validation, and explicit convergence/residual evidence | [hardening §spectral analysis](01-input-and-resource-hardening.md#spectral-analysis-iteration-and-numerical-evidence) |
| Exponential directed-cycle family | length 32 and retained-count 100,000 hard ceilings, SCC pruning, iterative reusable DFS state, and a concrete first-omission witness | [hardening §simple cycles](01-input-and-resource-hardening.md#simple-cycles-exponential-output-boundaries) |
| Combinatorial motif enumeration | 512-node triad and 50-node graphlet hard ceilings, complete CSR validation, dense bounded bit rows, and all-or-error results | [hardening §motifs](01-input-and-resource-hardening.md#directed-motifs-combinatorial-bounds) |
| Cubic graph-edit assignment | caller-selectable node budget with a 512 hard ceiling, finite-cost validation, and an explicit `capped` compatibility result | [hardening §GED](01-input-and-resource-hardening.md#graph-edit-distance-cubic-assignment-cap) |
| Forged module assignment or instability values | projection validation, inert invalid bucket indices, canonical assignments, and all-or-none SDP metric validation | [hardening §Martin metrics](01-input-and-resource-hardening.md#martin-metrics-assignment-and-comparison-validation) |
| Path-prefix confusion or oversized reflexion policy | pure lexical semantics; caller normalization and ingress caps for layers, prefixes, path bytes, allowed pairs, and observations | [hardening §reflexion](01-input-and-resource-hardening.md#reflexion-model-path-policy-and-input-bounds) |
| Forged or oversized layering inputs | inherited condensation ceiling, full SCC validation, bounded dense levels, and consistent height/width checks | [hardening §layering](01-input-and-resource-hardening.md#architectural-layering-inherited-cap-and-level-validation) |
| Untrusted serialized graph | Deserialize only trusted bytes | [`usage/05-serialization.md`](../usage/05-serialization.md) |
| Over-trusting heuristics, rankings, partitions, edit bounds, architecture metrics, cut/robustness evidence, bounded cycle prefixes, or motif frequencies | Treat pattern/algorithm/GNN, GED, Martin, reflexion, layering, centrality, community, cohesion, spectral, truncated-cycle, and motif output as advisory | this page |

## References

1. Yamaguchi, F., Golde, N., Arp, D., Rieck, K. (2014). *Modeling and Discovering Vulnerabilities with Code Property Graphs.* 2014 IEEE Symposium on Security and Privacy. DOI: [10.1109/SP.2014.44](https://doi.org/10.1109/SP.2014.44)
