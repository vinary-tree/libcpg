# Input and Resource Hardening

The [threat model](00-threat-model.md) identifies denial-of-service via oversized
or pathological input as the primary risk to an application embedding `libcpg`.
This page is the operator's manual for the bounds that keep the work finite: what
each one defaults to, where it is enforced, how to tighten it, and what to do about
the gaps (notably the Mode-B size-check bypass and the absence of a built-in parser
timeout).

![Resource bounds that keep libcpg finite on large or untrusted input](../diagrams/resource-bounds.svg)

*Figure — finite-work safeguards for construction, call resolution, VF2
matching, monotone solving, condensation/DSM/Martin/code-metric/smell/centrality analytics, the structural DFG
sweep, and slicing.
Source: [`diagrams/resource-bounds.puml`](../diagrams/resource-bounds.puml).*

## The bounds at a glance

| Bound | Default | Enforced in | Effect when exceeded |
| --- | --- | --- | --- |
| `CpgBuilderConfig::max_file_size` | 10 MB | `build` only (not `build_from_tree`) | `build` returns `Error::Construction` before parsing |
| Program-composition admission | caller-selected file/node/edge/path-byte limits | `merge_programs`; `build_import_graph` | finite and non-recursive, but intentionally has no independent cap; reject an oversized aggregate before composition |
| `ResolveConfig::max_scope_depth` | `128` | scope and callee-name walks | traversal stops at the configured depth or the first repeated node |
| `ResolveConfig::max_candidates_per_name` | `64` | lexical/global symbol buckets | full ambiguity count is preserved, but excess candidate IDs are not retained |
| `ResolveConfig::max_calls` | `100_000` | sorted call-node enumeration | higher `NodeId` calls remain untouched after the cap |
| `IfdsCaps::{max_functions,max_facts_per_node,max_worklist_steps,max_scc_iters,max_call_depth}` | `100_000`, `4_096`, `1_000_000`, `64`, `128` | `solve_summaries`; default callee resolver | returns ordered partial evidence, never a false completeness claim; fact overflow stops before solving and recursive overflow stops before callers |
| `Vf2Matcher::max_matches` | `0` (unlimited) | `pattern::Vf2Matcher` | search short-circuits once the match count is reached |
| `Vf2Matcher` strict toggles | `strict_kinds`/`strict_edges` off | `pattern::Vf2Matcher` | tighter feasibility test prunes branches earlier |
| Clone pair / PDG / VF2 budgets | `100_000` pairs, `300` PDG nodes, `10_000` VF2 calls | `pattern::detect_clones` | returns `AnalysisError::BudgetExceeded` before omitting a required comparison; hard maxima are `100_000`, `300`, and `100_000` |
| DFG structural guards | always on | `DfgExtractor` | loop bodies receive exactly two passes; an AST path set cuts malformed cycles |
| `backward_slice` / `forward_slice` `max_nodes` | *(caller-supplied argument)* | crate-root slice fns | BFS returns as soon as the slice reaches `max_nodes` |
| AST traversal guards | *(always on)* | `ast_descendants` / `ast_ancestors` / `ast_depth`, SCC scope discovery, `CfgExtractor`, DFG reaching-defs | a cyclic `AstChild` graph terminates instead of looping or overflowing the stack |
| CFG reachability guard | *(always on)* | `unreachable_code` | iterative DFS expands each function-scoped CFG node at most once; `Call` edges and nested functions are excluded |
| Basic-block traversal guard | *(always on)* | `block_cfg` | every scoped node is assigned once; iterative DFS avoids native-stack growth; `Call` edges and nested functions are excluded |
| `SolverConfig::max_visits_per_block` | `256` | `solve` | returns `AnalysisError::IterationLimitExceeded` before another extraction of that block |
| `SolverConfig::narrow_passes` | `0` | `solve` | performs exactly the configured number of post-widening deterministic sweeps |
| SCCP event cap | `4 * (scope nodes + DefUse edges)` | `sccp` | returns `AnalysisError::IterationLimitExceeded` if malformed dependencies exceed the finite-lattice schedule bound |
| `CondensationOptions::max_components` | `1_000_000` hard maximum | `condensation_metrics`; `coffman_graham` always uses the hard maximum | the total metrics API returns an incomplete capped report; the direct CG API returns `AnalysisError::BudgetExceeded` |
| Layering structural/metric validation | inherits condensation hard maximum | `infer_layering` | incomplete/capped or malformed SCC values, noncanonical edges, impossible/nondense level domains, and inconsistent height/width return an empty incomplete report |
| `MAX_DSM_NODES` | `50_000` hard maximum | `analyze_dsm`; independently rechecked by `classify_core_periphery` | returns an incomplete capped result with the stable node universe and no partial visibility/class vectors |
| Martin structural validation | always on | `martin_metrics`; `sdp_violations` | malformed projections return no result; invalid assignment entries are inert; duplicate/non-finite/out-of-range instability values suppress all SDP verdicts |
| Code-metric structural guards | always on; caller caps CPG size | `method_field_graph`; `ck_metrics`; `function_metrics` | finite visited traversals, exact field/call evidence, inheritance SCC condensation, saturating counts, and defined zero denominators prevent cycles and non-finite results; invalid function selectors return typed errors |
| Architectural-smell bundle validation | always on; caller caps projection size | `architecture_smells` | malformed CSR, stale SCC, invalid options/metrics, partial assignment, or missing/duplicate metrics return no findings before detection |
| Reflexion input admission | caller-selected | `reflexion` | pure finite iteration, but callers must normalize path identity and cap layers, prefixes, path bytes, allowed pairs, and observations at ingress |
| `MAX_WL_ITERATIONS` | `16` hard maximum | `wl_labels`; inherited by `structural_clone_classes` | clamps the request before refinement; malformed projections return empty vectors before indexed traversal |
| `MAX_CENTRALITY_ITERATIONS` | `10_000` hard maximum | PageRank, eigenvector, Katz, and HITS | clamps every requested round count; invalid numerical options return a non-converged result without iteration |
| `CentralityBudget::max_work` | `2_000_000_000` default | edge betweenness, closeness, harmonic, and Burt constraint | rejects before all-sources/neighborhood traversal with `AnalysisError::BudgetExceeded` |
| `BetweennessOptions::max_work` | `2_000_000_000` default | directed vertex betweenness and `accumulate_for_sources` | rejects before Brandes traversal when `source_count * edge_count` exceeds the cap |
| `MAX_LOUVAIN_PASSES` / `MAX_LOUVAIN_LEVELS` | `50` / `10` hard maxima | `louvain` | clamps both repetition dimensions; zero requests return the singleton partition without local moving |
| `MAX_K_TRUSS_EDGES` | `200_000` hard maximum | `k_truss_decomposition` | rejects before triangle-support peeling with `AnalysisError::BudgetExceeded` |
| `MAX_MIN_CUT_NODES` | `1_024` hard maximum | `global_min_cut` | rejects before allocating the dense Stoer–Wagner matrix |
| `MAX_ATTACK_STEPS` | `10_000` hard maximum | `attack_simulation` | rejects an excessive step request; an intentional shorter prefix is explicitly marked `truncated` |
| `MAX_CYCLE_LENGTH` / `MAX_SIMPLE_CYCLES` | `32` / `100_000` hard maxima | `simple_cycles` | clamps path depth and retained count; stops inside DFS at the first over-cap unique cycle and returns that omitted witness |
| `MAX_TRIAD_NODES` / `MAX_GRAPHLET_NODES` | `512` / `50` hard maxima | `motif_census` | rejects before cubic/quartic enumeration with `AnalysisError::BudgetExceeded`; never returns a partial census |
| `MAX_GED_NODES` | `512` hard maximum | `ged_upper_bound`; `SimilarityMetric::GraphEdit` | bounds the cubic Hungarian matrix; direct results set `capped = true` and clearly label the linear compatibility fallback |
| Loop-induction structural bounds | at most `candidate_count + 1` invariant rounds per loop; affine depth `3` | `loop_induction` | reaches a finite fixed point; malformed AST cycles are cut by visited sets and arithmetic overflow becomes unknown evidence |
| Complexity-evidence bounds | validated finite function scope plus loop/induction provider bounds | `estimate_time_complexity_with_evidence` (feature `algorithm-detection`) | provider failure or absent CFG evidence returns the exact legacy total estimate; justification grows with loop count |

The underlying bounds are on the **feature-free surface**. The final complexity
consumer requires `algorithm-detection`, but it reuses those providers and adds
no parser or unbounded search.

## Construction: `max_file_size`

`build` checks the source length against `CpgBuilderConfig::max_file_size` (default
`10 * 1024 * 1024` bytes) *before* handing anything to the parser, and rejects
oversized input:

```rust
// requires: features = ["lang-rust"]  (the build call needs a grammar)
use libcpg::{CpgBuilder, CpgBuilderConfig, TreeSitterCpgBuilder, Language};

// Tighten the cap well below the 10 MB default for hostile input.
let config = CpgBuilderConfig::new().with_max_file_size(256 * 1024); // 256 KiB
let builder = TreeSitterCpgBuilder::with_config(config);

let result = builder.build(oversized_source, Language::Rust);
assert!(result.is_err()); // Error::Construction, before parsing
```

This is exactly what the shipped `test_file_size_limit` test asserts (with a 10-byte
cap and a longer source). Because the guard runs before parsing, it caps the size of
the parse tree, the CPG, and every downstream pass in one stroke — making it the
single most effective DoS control for the internal-parse path.

### The Mode-B bypass

`build_from_tree` (Mode B) runs the identical post-parse pipeline but **does not
apply the `max_file_size` check** — the caller already parsed the source and owns
the tree. If your Mode-B input is untrusted, you must bound it yourself before
parsing (next section). This is a deliberate design property, not an oversight; see
[`design/0002-mode-b-build-from-tree.md`](../design/0002-mode-b-build-from-tree.md).

## Program composition: aggregate admission and evidence safety

[`merge_programs`](../components/graph/program-composition.md) clones the finite
node, edge, metadata, and retained-source inventories supplied by the caller.
It performs no parser, filesystem, package-manager, or network access, and all
walks are iterative or finite sorted scans. It deliberately has no built-in
aggregate cap because trusted build systems may compose programs much larger
than one source file. At an untrusted boundary, cap the number of files, summed
nodes and edges, source-path bytes, and retained-source bytes before calling it.

Malformed embedded `NodeId` values are translated only when they belong to the
source graph; otherwise optional references are cleared and sequence members
are omitted. A numeric collision with an existing destination node therefore
cannot forge a reference. `build_import_graph` creates topology only for one
uniquely strongest represented file alias. Ties and absent files remain
ordered evidence with no dependency edge. These rules protect semantic
integrity; caller admission limits protect memory and sorting work.

## Call resolution: scope, candidate, and call caps

[`resolve_calls`](../components/graph/call-resolution.md) processes calls in
ascending `NodeId` order and exposes three independent work/evidence bounds.
The defaults are suitable for ordinary source; applications analyzing hostile
hand-built or deserialized CPGs can tighten them:

```rust
use libcpg::{
    resolve_calls, CallGraphResolution, CodePropertyGraph, ResolveConfig,
};

fn resolve_bounded(cpg: &mut CodePropertyGraph) -> CallGraphResolution {
    let config = ResolveConfig {
        max_scope_depth: 32,
        max_candidates_per_name: 16,
        max_calls: 10_000,
        resolve_methods: false,
    };
    resolve_calls(cpg, &config)
}
```

`max_scope_depth` bounds lexical parent walks and callee-subtree depth; visited
sets stop parent/child cycles earlier. `max_candidates_per_name` limits only
retained identifiers: the resolver preserves the full distinct count, so
truncation cannot turn an ambiguous name into a unique one. `max_calls` is a
hard prefix budget over deterministically sorted calls. Calls beyond it are not
recorded or mutated. These controls bound resolver work without weakening the
evidence boundary: ambiguous, external, and unresolved calls never create
topology.

## IFDS/IDE summaries: whole-program and recursion caps

[`solve_summaries`](../components/graph/ifds-ide-framework.md) validates the
supplied call-graph SCC value against a fresh decomposition before any domain
evaluation. Public component fields therefore cannot redirect indexed
traversal, omit a newly resolved callee, or introduce a cyclic condensation
without an ordered `IfdsIssue`.

`IfdsCaps` bounds five independent resources. `max_functions` rejects an
oversized represented program before seed or summary allocation.
`max_facts_per_node` deduplicates and deterministically truncates seed evidence,
then stops before summary solving because a may analysis with missing seeds is
incomplete. `max_worklist_steps` counts both condensation removals and domain
summary evaluations. `max_scc_iters` bounds complete rounds inside each
recursive component; on exhaustion, the solver retains the bottom-based
monotone prefix but does not evaluate callers that depend on it.
`max_call_depth` bounds the default resolver's cycle-safe callee-label descent
and is passed to domains for their own call traversals.

Every event sets `IfdsOutput::capped`; `is_complete()` additionally requires an
empty structural/domain issue set. A capped may-analysis result can contain real
positive evidence, but absence from that result is not evidence of safety.
Domain implementations remain caller code: the framework cannot preempt an
unbounded loop inside `SummaryDomain::summarize`, so every domain must honor the
shared caps or stricter documented local limits.

## Pattern matching: VF2

[VF2](../GLOSSARY.md#vf2) subgraph isomorphism is worst-case super-exponential —
$`O(N!\,N)`$ — so on adversarial graphs the search space is the concern. Two
controls bound it:

- **`with_max_matches(n)`** stops the state-space search after `n` matches (`0`
  means unlimited, the default). Set it to a small number — or `1` for a pure
  existence check — to guarantee early termination.
- **`with_strict_kinds(true)` / `with_strict_edges(true)`** tighten the feasibility
  test so infeasible branches are pruned sooner, shrinking the search.

```rust
// requires: no features (pattern module is always compiled)
use libcpg::pattern::Vf2Matcher;

let matcher = Vf2Matcher::new()
    .with_max_matches(100)     // never enumerate more than 100 embeddings
    .with_strict_kinds(true)   // prune on node kind …
    .with_strict_edges(true);  // … and on incident edge kinds
let matches = matcher.find_matches(&pattern, &target);
```

Note that the [Gang-of-Four detector](../GLOSSARY.md#gang-of-four-gof) uses a
*relaxed* matcher by design (category-level kind/edge matching); when you run it on
untrusted input, bound the surrounding work instead — cap the number of patterns
searched and the size of the CPG (via `max_file_size`).

### Function clone search

[`detect_clones`](../components/patterns/clone-detection.md) contains VF2 behind
three all-or-error work gates. `max_pairs` counts size-qualified candidate
pairs before strongest-relation classification, `max_pdg_nodes` limits either semantic candidate graph to at most
300 vertices, and `max_vf2_invocations` limits verifier calls. The first needed
unit beyond a configured cap returns `BudgetExceeded`; no partial classes are
returned. Thresholds must be finite values in $`[0,1]`$, and requests above the
hard ceilings are `InvalidInput`.

The Type-1/2 fingerprint walk is iterative, skips nested functions, and cuts
malformed AST cycles. The Type-4 matcher uses `with_max_matches(1)`, so a
successful existential check stops immediately. For untrusted CPGs, lower all
three budgets together; lowering only `max_pairs` does not constrain pairs that
reach Type-1/2 by direct fingerprint equality.

## Data flow: structural sweep bounds

The [reaching-definitions](../GLOSSARY.md#reaching-definition) analysis is an
[AST-ordered sweep](../GLOSSARY.md#ast-ordered-reaching-definitions) with a loop
double-pass. It does not iterate to a data-flow fixed point: ordinary AST
regions receive one structural visit, loop bodies receive exactly two, and a
path set cuts malformed AST cycles. Consequently termination follows from the
finite graph and explicit structural bounds.

`DfgExtractorConfig::max_iterations` is retained for source compatibility with
the retired CFG fixed-point implementation and is not consulted by the live
AST-ordered sweep. It must not be relied upon as a live security control.

## Monotone solving: visit and narrowing bounds

The separate [`solve`](../components/graph/dataflow-framework.md) framework
uses an explicit worklist over `BlockCfg`; it does not recurse through the CFG.
Every extraction increments `DataflowSolution::visits[block]`. When that count
would exceed `SolverConfig::max_visits_per_block` (default `256`), solving
returns `AnalysisError::IterationLimitExceeded` with the analysis name and cap.

`widen_after` is disabled by default. When enabled, widening is eligible only
at headers identified by the supplied `LoopForest`; it does not weaken the hard
visit cap. `narrow_passes` runs exactly the requested number of bounded sweeps.
For hostile or client-constructed domains, lower the visit cap. The library
cannot prove a client's lattice laws, monotonicity, or transfer cost, so the cap
is the termination boundary rather than a correctness certificate.

The built-in liveness, available-expression, and very-busy-expression analyses
use finite dense bit domains and the default visit cap. Expression reports can
still be large: each reachable node may contain every structural expression, so
the worst-case output is $`O(NF)`$ facts for $`N`$ nodes and $`F`$ expressions.
Bound source/CPG size before analysis when accepting hostile input. Treat CSE
and PRE candidates as advisory; applying them without language-aware checks for
volatile access, traps, floating-point state, or concurrency can compromise the
semantic integrity of transformed code.

The built-in [`value_ranges`](../components/graph/interval-analysis.md)
instance uses a stricter fixed policy: at most 64 visits per block, widening
after the third visit only at a `LoopForest` header, and exactly two narrowing
sweeps. Unsupported expressions become top rather than triggering recursive or
language-specific evaluation. A visited stack cuts malformed AST expression
cycles, and an empty branch meet remains environment bottom. The report may
still contain $`O(BV)`$ interval pairs for $`B`$ blocks and $`V`$ variables, so
source/graph size remains the caller's memory boundary.

## SCCP: coupled event bound

[`sccp`](../components/graph/sccp-analysis.md) maintains separate ordered
worklists for executable CFG sources and definition/expression value cells. A
constant cell can rise at most twice (`Unknown` to one constant, then to
`Overdefined`), and an executable edge is inserted once. The solver still
enforces the explicit cap

```math
4(N + D)
```

for $`N`$ function-scope nodes and $`D`$ scoped `DefUse` edges. Exceeding it
returns `AnalysisError::IterationLimitExceeded { analysis: "sccp", ... }`.
Visited sets separately cut AST cycles while finding execution owners and guard
expressions. SCCP borrows the CPG immutably, so even a malformed input cannot
cause partial graph mutation before an error.

The cap bounds scheduling, not operator cost or output size. String
concatenation allocates a result proportional to its two constant operands, and
the public maps can grow with the scoped graph. Continue to enforce source and
CPG size limits for hostile input.

## Condensation analytics: component cap and structural validation

[`condensation_metrics`](../components/graph/condensation-analysis.md) clamps
the caller's `max_components` option to the hard
`MAX_CONDENSATION_COMPONENTS = 1_000_000` ceiling. A decomposition above the
effective limit returns `complete = false`, `capped = true`, and empty metric
vectors. The direct `coffman_graham` API uses the hard ceiling and returns
`AnalysisError::BudgetExceeded` instead. A zero CG width is invalid.

The input is a public `SccDecomposition`, so the analytics do not assume that
it came from libcpg's SCC engine. Validation requires positional component ids,
nonempty and exactly mapped membership, in-range non-self edges, and an acyclic
normalized condensation graph. Malformed input produces an incomplete total
report or `AnalysisError::InvalidInput`; it never causes partial mutation or
recursive traversal. Work uses explicit heaps and vectors. The cap limits the
number of components, while total SCC membership and edge storage still scale
with the supplied decomposition, so callers must retain source/graph size
limits for hostile inputs.

## Architectural layering: inherited cap and level validation

[`infer_layering`](../components/graph/layering-analysis.md) invokes the
condensation engine's complete SCC/DAG validation before indexing either public
input. It therefore inherits `MAX_CONDENSATION_COMPONENTS = 1_000_000` and the
membership, endpoint, and acyclicity checks above. It additionally requires
canonical distinct condensation-edge order, marks a multi-node component with
`cyclic == false` invalid, and rejects incomplete or capped metric values.

The consumed level vector must have exactly one entry per component, remain
below the component count, occupy every level from zero through its maximum,
and agree with `height` and `width`. These checks happen before allocating the
returned layer vectors, so `u32::MAX` cannot trigger an attacker-sized
allocation. Shape-consistent stale levels are deliberately classified: their
lateral or ascending edges become `Upward` witnesses. The result is advisory,
read-only, iterative, and out of graph.

The inherited hard cap is not a latency target. The validator and report still
scale with total SCC membership and condensation edges; interactive or hostile-
repository services should configure a substantially smaller condensation cap
before computing the metrics supplied to layering.

## DSM visibility: quadratic-work cap and CSR validation

[`analyze_dsm`](../components/graph/dsm-analysis.md) performs one forward
breadth-first search per projected vertex, with worst-case time
$`O(n(n+m))`$. It checks `n > MAX_DSM_NODES` before validating CSR or
allocating traversal state. An input above the 50,000-vertex hard ceiling
returns `complete = false`, `capped = true`, retains only stable node
provenance, and reports no partial visibility vectors or scalar inference.

For an in-budget public `GraphProjection`, the analysis validates strictly
sorted nodes, forward/reverse offset shape and monotonicity, in-range and
strictly sorted targets, optional-weight alignment, equal edge counts, and
forward/reverse agreement before calling any indexing accessor. Malformed CSR
returns an uncapped incomplete result. `classify_core_periphery` repeats graph
validation and requires exact `NodeId` provenance, node count, vector lengths,
per-node count bounds, equal fan-in/fan-out totals, finite and count-consistent
propagation cost, and a complete source analysis before median or SCC work.
This prevents mixing results from different same-sized projections and rejects
caller-forged inconsistent fields before classification.

The hard ceiling is a denial-of-service boundary, not a latency guarantee:
quadratic work can be expensive far below 50,000. Applications processing
hostile or interactive workloads should enforce a tighter input-specific node
limit before calling the fixed-signature API. Edge weights and payload sizes do
not affect visibility topology, but the source projection and returned SCC
decomposition still occupy $`O(n+m)`$ storage.

## Martin metrics: assignment and comparison validation

[Martin package metrics](../components/graph/martin-metrics.md) validate the
complete forward/reverse `GraphProjection` before any indexed edge traversal.
`ModuleAssignment::module_of` treats absent nodes and bucket indices outside
`buckets` as unassigned; caller-supplied pairs are canonicalized by sorted name
and stable node ID. `sdp_violations` rejects the complete metric slice if module
names repeat or an instability is non-finite or outside $`[0,1]`$, rather than
selecting a contradictory value and fabricating a policy verdict.

The passes have no independent hard cap because their work is bounded by the
supplied graph and CPG: metrics take
$`O(m\log n+t\log k)`$ time and SDP aggregation takes
$`O(m\log k)`$, where $`n`$ is projection vertices, $`m`$ is edges, $`t`$ is
CPG nodes, and $`k`$ is module names. A hostile-input service must still bound
CPG/projection size and module-name bytes at its ingress. Treat the assignment
policy and dependency-edge semantics as untrusted metadata when they cross a
serialization or service boundary; validation guarantees total execution, not
architectural truth.

## Code metrics: finite evidence and ingress cap

[`method_field_graph`, `ck_metrics`, and `function_metrics`](../components/graph/code-metrics.md)
perform no parser call, source evaluation, I/O, recursion, or unbounded fixed
point. Function/member discovery uses visited iterative stacks; inheritance
cycles are collapsed by the shared iterative SCC engine before DIT longest-path
evaluation; ordered sets bound each declaration/access/call to finite storage.
Integer aggregation saturates at `u32::MAX`, Halstead zero denominators are
defined as zero, logarithm inputs are floored at one, and maintainability index
is clamped to a finite $`[0,100]`$ result.

The fixed-signature API has no independent node/edge cap because it analyzes an
already admitted finite CPG. Full CK evaluation can repeat evidence scans per
nominal type and LCOM pair comparison is quadratic in a class's method count.
Hostile or interactive services must therefore cap CPG nodes, edges, methods per
type, string payload bytes, and total nominal types at ingestion. The Mode-A
`max_file_size` guard is the broadest control; Mode-B and deserialized graphs
require caller policy.

Structural safety is not semantic trust. A forged deserialized graph can attach
a plausible `Reference`, `FieldRead`, call target, or source range and thereby
produce a deterministic but misleading metric. Persist builder, DFG, resolver,
frontend, and `CkOptions` policy beside reports. Never use a threshold as an
automatic authorization to move/delete code or as a security gate.

## Architectural smells: validated bundle and ingress cap

[`architecture_smells`](../components/graph/architecture-smells.md) treats its
projection, SCC decomposition, module assignment, Martin metrics, and options as
one untrusted snapshot. It validates complete CSR invariants, recomputes SCCs
with the iterative shared engine and requires exact equality, resolves every
projected node to an in-range assignment bucket, and requires one finite bounded
Martin record per used module. Every option must be finite and in its documented
domain. Any failure returns no findings before detector traversal.

The function has no independent hard node/edge cap because it is a
caller-selected feature-free analysis over an already admitted projection. Its
graph work is near-linear apart from ordered-map lookups and canonical output
sorting, but a hostile service must still cap vertices, edges, module/name
bytes, and serialized evidence at ingress. The SCC recomputation is a
consistency defense, not a latency limit. Preserve projection meaning,
assignment policy, Martin inputs, and options with the output; deterministic
evidence does not establish architectural intent.

## Reflexion model: path policy and input bounds

[`reflexion`](../components/graph/reflexion-model.md) performs no parsing,
graph traversal, recursion, I/O, or filesystem lookup. It treats every path as
an opaque UTF-8 string and assigns it to the first declared layer with a
`starts_with` prefix match. Consequently it remains total for empty, duplicate,
unknown, and overlapping public values, but it must not be used as a path
authorization check. Normalize separators, relative roots, `.`/`..`, and any
symlink policy before the call when path identity has security consequences.

Work is finite and bounded by supplied collections, but the fixed signature has
no hard admission cap. A hostile-input service should bound the number of
layers, prefixes per layer, `allow` pairs, observed edges, and bytes per name or
path before allocating `Arc<str>` values. Persist the dependency extractor and
normalization policy with results; deterministic output does not establish the
truth or completeness of caller metadata.

## WL refinement: round cap and total projection validation

[`wl_labels`](../components/graph/wl-analysis.md) clamps every request to
`MAX_WL_ITERATIONS = 16`. Each round reads the forward and reverse adjacency,
sorts one incident-label multiset per vertex, and sorts the graph-wide raw-label
vocabulary. The ceiling bounds caller-controlled repeated work even when the
request is `usize::MAX`; the implementation is iterative and cannot consume
native stack proportional to graph depth.

WL shares `GraphProjection::is_well_formed` with DSM. Before any indexed
access, that validator checks stable-node ordering, both CSR shapes, target
bounds/order, weight alignment, edge counts, and forward/reverse agreement.
Malformed public or deserialized state yields empty labels and clone classes.
This safe failure is distinguishable from a valid nonempty result because a
successful label vector is exactly aligned with `node_count`.

The round cap does not bound graph size. Sorting and storage still scale with
the supplied projection, so hostile-input applications must cap source, node,
and edge counts before projection construction. Equal 64-bit labels are
candidate evidence and must not authorize security-sensitive equivalence or
code transformation without exact semantic validation.

## Centrality: iteration ceilings, traversal budgets, and safe numerics

[Projection centrality](../components/graph/centrality-analysis.md) validates
the same complete `GraphProjection` contract before indexed work. Fallible
kernels return `AnalysisError::InvalidInput` for malformed CSR. Total scalar
kernels return an empty, non-converged result, which cannot be mistaken for a
valid nonempty aligned vector.

PageRank, eigenvector, Katz, and HITS clamp requests to
`MAX_CENTRALITY_ITERATIONS = 10_000`. Damping and tolerance must be finite and
inside their documented ranges. Katz additionally requires finite attenuation
and baseline plus the sufficient row-sum convergence bound; an invalid option
returns without iterating. These controls prevent NaN-driven non-convergence or
hostile machine-maximum round counts.

Personalized PageRank rejects finite seed weights when their grouped or total
mass overflows. Weighted spectral/Burt kernels reject non-finite edge weights;
negative finite weights are clamped to zero by their documented nonnegative
matrix policy. Brandes rescales each breadth-first layer by one common positive
factor, preserving dependency ratios while preventing exponentially many
shortest paths from becoming `inf/inf`.

Directed Brandes checks `source_count * edge_count`. Edge betweenness,
closeness, and harmonic centrality check `node_count *
undirected_adjacency_entries`. Burt constraint checks the exact modeled
$`\sum_i d(i)^2`$ neighborhood comparisons. All use caller-visible budgets and
fail before a partial ranking can escape. The two-billion default is a hard
finite ceiling, not an interactive-latency promise; applications should choose
substantially lower limits for hostile workloads.

Centrality remains advisory. A high score must not authorize code execution,
access control, deletion, or automated refactoring. Call-graph results inherit
the call resolver's evidence boundary, and every ranking inherits the selected
projection's semantic completeness.

## Community detection: pass, level, and numeric bounds

[Community detection](../components/graph/community-detection.md) validates the
complete projection contract before indexed work. `louvain` rejects a
non-finite or negative resolution, every negative or non-finite directed edge
weight, a non-finite reciprocal sum, and any degree or total-weight overflow.
It never returns a partial plausible partition after one of those failures.

`max_passes` is clamped to `MAX_LOUVAIN_PASSES = 50` independently at every
level. `max_levels` is clamped to `MAX_LOUVAIN_LEVELS = 10`. A zero value for
either option is a defined no-work request that returns the canonical singleton
partition. Aggregation cannot increase the number of vertices or canonical
edges, so these two ceilings bound every repeated phase even on a graph whose
local moves keep changing through exact-score ties.

`try_refine_connected` checks projection structure, exact node alignment,
canonical contiguous labels, and stored resolution before breadth-first search.
The total `refine_connected` wrapper converts malformed caller-controlled public
fields to an aligned singleton partition rather than panicking or presenting an
unchecked grouping as refined.

The limits bound algorithmic repetition, not input size. Hosts processing
hostile projections should also cap node and edge counts to their latency and
memory budgets. A community assignment is advisory structural evidence; it
must not authorize access, ownership, deletion, or automatic refactoring.

## Cohesion: edge, matrix, traversal, and numeric bounds

[Cohesion and structural robustness](../components/graph/cohesion-analysis.md)
validates the complete projection contract before indexed traversal. Core,
truss, low-link, component, assortativity, and attack operations derive simple
undirected topology: direction and numeric weights do not affect reachability.
Minimum cut separately rejects negative, non-finite, or overflowing weights.

K-truss checks `MAX_K_TRUSS_EDGES = 200_000` before triangle-support work. Its
storage remains proportional to nodes and simple edges; it does not materialize
an attacker-controlled list of every triangle. Global minimum cut checks
`MAX_MIN_CUT_NODES = 1_024` before allocating its quadratic matrix or entering
the cubic Stoer–Wagner phases. These are safety ceilings, not latency targets;
interactive services should select smaller caller-side graph limits.

Attack simulation requires a distinct, fully projected `NodeId` order and
rejects `max_steps > MAX_ATTACK_STEPS = 10_000`. It performs bounded iterative
breadth-first traversal after each processed removal. `truncated = true` means
the caller deliberately requested a prefix shorter than the validated order;
it never disguises a library-enforced cap. Low-link DFS is iterative, so even a
deep valid path cannot consume unbounded native stack.

Every result is advisory structural evidence. A high core/truss value, light
cut, articulation, bridge, assortativity value, or attack area must not by
itself authorize deletion, isolation, access control, or automated refactoring.

## Spectral analysis: iteration and numerical evidence

[Spectral connectivity](../components/graph/spectral-analysis.md) validates the
complete projection contract and finite nonnegative tolerance before indexed
work. It treats direction and numeric edge values as metadata, deduplicates
topology, and ignores self-loops; non-finite weights therefore never enter
floating arithmetic.

The component pre-pass and every matrix-vector multiply are iterative and use
sparse storage. `MAX_SPECTRAL_ITERATIONS = 1_000` clamps each start even for a
request of the machine maximum, while `MAX_SPECTRAL_STARTS = 8` bounds the
deterministic cyclic linear family. Numerical repetition is therefore bounded
by $`O(8000(|V|+|E|))`$. Hosts must still cap projection node/edge counts to their
own memory and latency budget. A capped result declares `converged == false`
and includes $`\lVert Lx-\lambda x\rVert_2`$; consumers must not interpret an
unchecked approximation or sign partition as an authorization, isolation
boundary, or exact minimum cut.

## Simple cycles: exponential-output boundaries

[Bounded simple-cycle analysis](../components/graph/cycle-analysis.md) validates
the complete forward/reverse CSR contract before SCC decomposition or indexed
DFS. `MAX_CYCLE_LENGTH = 32` limits every path, and
`MAX_SIMPLE_CYCLES = 100_000` limits retained results even when the caller
requests the machine maximum. A zero count does not silently claim acyclicity:
the first discovered cycle is returned as `first_dropped_cycle`.

The DFS receives only remaining capacity plus one, so its local discovery
buffer cannot grow beyond the public result boundary on a dense SCC. Path,
frame, and membership storage is allocated once and reused. Traversal is
iterative, and `projection_sccs` skips every acyclic component. These controls
bound stack and output memory, but hosts must still cap input node/edge counts
and latency: the family of simple cycles is exponential even at bounded depth.
A truncated `CycleSet` contains valid witnesses but is not evidence that no
other cycles exist.

## Directed motifs: combinatorial bounds

[Directed motif analysis](../components/graph/motif-analysis.md) validates the
complete forward/reverse CSR contract before allocating its adjacency bit rows.
`MAX_TRIAD_NODES = 512` bounds cubic enumeration and
`MAX_GRAPHLET_NODES = 50` bounds quartic enumeration. Smaller caller limits are
honored and larger values clamp to the hard ceilings. Because one call requests
both families, exceeding either effective limit returns
`AnalysisError::BudgetExceeded`; zero counts therefore always mean a complete
admitted census, not skipped work.

The dense topology uses a predictable
`O(n ceil(n/64))` words, ignores loops and numeric weights, and has no
input-dependent recursion. Hosts should still choose smaller limits when their
latency budget cannot tolerate exhaustive `O(n³ + n⁴)` work. Motif frequencies
are advisory structural evidence and must not independently authorize code
changes, isolation, or access-control decisions.

## Graph-edit distance: cubic assignment cap

[`ged_upper_bound`](../components/patterns/ged-analysis.md) constructs a square
matrix of side $`|V_1|+|V_2|`$ and runs cubic Hungarian assignment twice. The
effective per-input limit is `min(GedOptions::max_nodes, MAX_GED_NODES)`, where
`MAX_GED_NODES = 512`. This limit prevents an untrusted caller from requesting
an arbitrarily large quadratic allocation or cubic solve. Hosts with tighter
latency or memory budgets should choose a smaller `max_nodes`.

When either input exceeds the effective limit, the API does not allocate the
matrix. It returns the linear legacy size/Jaccard compatibility distance and
sets `GedBound::capped = true`. The facade `SimilarityMetric::GraphEdit`
remains total, but it does not expose that bit; security-sensitive callers must
use `ged_upper_bound` directly and reject capped evidence.

All costs are checked for finiteness and nonnegativity, and derived
normalization/matrix arithmetic is checked before assignment. This prevents
NaN, infinity, negative-cost cycles, and overflow from masquerading as valid
distances. An uncapped GED upper bound remains advisory: node equality uses
coarse `NodeKindTag` labels, and the bipartite mapping is approximate. It must
not independently authorize a code change or suppress a security finding.

## Loop induction: finite structural bounds

[`loop_induction`](../components/graph/induction-analysis.md) computes each
loop's invariant set monotonically over a finite candidate list. A round can
only add candidates, so the implementation performs at most
$`candidate\_count + 1`$ rounds before stopping. Basic-IV recognition scans the
finite loop region; derived affine recognition descends through at most three
operator levels. Checked `i64` arithmetic maps negation, coefficient, or step
overflow to unknown evidence rather than panicking or wrapping.

Region, candidate-use, initialization, and guard discovery all use explicit
work vectors plus visited sets. Malformed `AstChild` cycles therefore terminate,
and a 25,000-node region is regression-tested for native-stack safety. The
caller-supplied `LoopForest` is not trusted blindly: every header, entry, latch,
body member, exit source, and exit target must resolve inside the selected
function's reachable CFG projection or the call returns
`AnalysisError::UnknownNode`.

The fixed point bounds termination, not output size or transformation safety.
Nested loop regions may overlap, worst-case candidate rescanning is polynomial,
and `hoistable`/`Counted` results remain language-neutral advisory evidence.
Continue to bound source/CPG size and apply language-specific overflow, trap,
aliasing, volatile/atomic, and concurrency policy before consuming them.

## Complexity evidence: bounded providers and total fallback

`ComplexityAnalyzer::estimate_time_complexity_with_evidence` computes an exact
legacy AST estimate first, then accepts only a validated intraprocedural CFG
scope, loop forest, and induction report. The provider bounds above therefore
bound the stronger path: iterative graph walks cut cycles, dominance has its
pass cap, and induction has finite fixed-point and expression-depth limits. If
scope, loop, or induction validation fails, the method returns the already
computed fallback rather than retrying, mutating the graph, or parsing source.

The result string lists every loop header, IV, comparison, and bound. Its size
is therefore $`O(L + I)`$ references for $`L`$ loops and $`I`$ reported IVs;
source/CPG size remains the caller's memory boundary. A confidence of `1.0`
means that every reducible trip has evidence under the documented structural
model. It does not prove termination, language overflow safety, an exact
symbolic bound, or suitability for a security decision.

## Dominance: structural convergence cap

[`dominator_tree`](../components/graph/dominator-analysis.md) and
`post_dominator_tree` run the deterministic CHK fixed point for at most the
analyzed vertex count plus two passes. A non-converging analysis returns
`AnalysisError::IterationLimitExceeded`; it cannot spin indefinitely. Reverse
postorder and Euler intervals use explicit work stacks, so an adversarially deep
CFG grows heap storage instead of the native call stack. The worst-case work is
still $`O(VE)`$, so callers processing untrusted inputs should continue to cap
source and CPG size.

## Slicing: `max_nodes`

[Backward and forward slices](../GLOSSARY.md#backward-slice--forward-slice) are
breadth-first traversals over PDG edges; both take a **required** `max_nodes`
argument that bounds the result. The traversal returns as soon as the slice reaches
`max_nodes` nodes (and returns empty when `max_nodes == 0` or the criterion node is
absent):

```rust
// requires: no features (PdgBuilder + slices are always compiled)
use libcpg::{PdgBuilder, backward_slice};

PdgBuilder::new().build(&mut cpg, function); // add ControlDependence + DataDependence
let slice = backward_slice(&cpg, criterion, 512); // FxHashSet<NodeId>, at most 512 nodes
```

The functions return an `FxHashSet<NodeId>`; pick a `max_nodes` proportional to how
large a slice your UI or downstream step can handle, so a huge function cannot
produce an unbounded result.

## Parser timeouts are the caller's job

`libcpg` does **not** configure a parse timeout. In the internal-parse path,
`build` constructs a `tree_sitter::Parser` itself and calls `parse` with no
cancellation, so parse time is bounded only indirectly by `max_file_size`. For a
*hard* wall-clock bound on parsing untrusted input, use Mode B: parse with your own
`tree_sitter::Parser` — on which you can set tree-sitter's own cancellation/timeout
controls — and then call `build_from_tree`. In that arrangement you own both the
size bound (which Mode B skips) and the time bound:

```rust
// requires: a grammar you link yourself (Mode B — feature-free in libcpg)
use libcpg::{TreeSitterCpgBuilder, Language};

// 1. Enforce your own size cap (Mode B does not).
if source.len() > MY_MAX_BYTES {
    return; // reject before doing any work
}

// 2. Parse with your own Parser, on which you may set tree-sitter's
//    cancellation/timeout facilities, then build from the tree.
let tree = my_parser.parse(source, None).expect("parse");
let cpg = TreeSitterCpgBuilder::new()
    .build_from_tree(&tree, source, Language::Rust)
    .expect("build_from_tree");
```

## Memory considerations

The size of a CPG is roughly linear in the number of retained AST nodes, which is
in turn bounded by the parsed source size. There is no separate cap on the *total*
node/edge count beyond `max_file_size` (internal-parse path) or the caller's own
bound (Mode B). The graph is a single [petgraph](../GLOSSARY.md#petgraph) arena, so
its footprint is that arena plus the per-node child vectors and `Arc<str>` text; see
[performance](../engineering/03-performance.md) for the data-structure details.

## Malformed graphs: traversal guards

The bounds above cap work on *well-formed* input. A separate hazard is input
that is not well-formed at all.

`CodePropertyGraph` is an **open** data structure: `add_node`, `connect`, and
`node_mut` are public, and `connect` deliberately wires only the petgraph edge —
it does **not** maintain `node.parent` / `node.children`, which the caller is
responsible for. A consumer that assembles a graph by hand (or a language
frontend with a bug, or a deserialized graph from an untrusted source) can
therefore hand the analyses a shape no builder would produce:

- a `parent` pointer to a node that was never added;
- a `children` entry with no corresponding edge, or an edge with no pointers;
- an `AstChild` **cycle**, so the "tree" is not a tree;
- a `Call` whose `target` names a node that does not exist.

Several analyses walk the AST recursively, and an `AstChild` cycle turns such a
walk into an unbounded descent. Two of them — the CFG extractor's mutually
recursive `process_*` handlers and the DFG's reaching-definitions sweep — would
overflow the stack and **abort the process**, which is a denial of service
reachable from any caller that builds a graph by hand.

### The contract

For structurally corrupt input the library guarantees **robustness, not
correctness**: an analysis may return a meaningless answer, but it must

1. **terminate**,
2. **not panic**, and
3. **not overflow the stack**.

### How it is enforced

| Traversal | Guard |
| --- | --- |
| `ast_descendants`, `ast_ancestors`, `ast_depth` | a **visited set** — each node is expanded at most once, so the walk is bounded by the node count |
| `ast_ancestors` | additionally stops at a parent pointer that names a node not in the graph, rather than returning an id `node()` cannot resolve |
| `control_flow_sccs` / `call_graph_sccs` | a visited set guards AST scope discovery; dense discovery/low-link arrays and explicit DFS/component stacks make iterative Tarjan visit every projected vertex and normalized edge without consuming native stack per path element |
| `analyze_dsm` / `classify_core_periphery` | validates public CSR and exact result provenance, caps vertices before work, and uses iterative queues plus the iterative shared SCC engine; no path depth consumes native stack |
| `martin_metrics` / `sdp_violations` | validate projection CSR before indexed traversal; invalid assignment indices are inert; enclosing-module walks inherit bounded `ast_ancestors`; invalid metric slices produce no verdict |
| `loop_forest` | the shared visited function scope, iterative dominator/SCC/DFS walks, iterative reverse-body closure, and iterative nesting walk avoid recursion; a 25,000-node natural loop is regression-tested |
| `unreachable_code` | visited function-scope and dense CFG sets bound discovery; iterative DFS is stack-safe on a 25,000-node reachable/disconnected graph |
| `block_cfg` / `solve` | iterative CFG traversal and work queues avoid native-stack growth; `solve` also caps extractions per block and ignores inconsistent public loop-header mappings |
| `value_ranges` | expression visited stack, loop-header-only widening, 64 visits per block, and two bounded narrowing sweeps |
| `loop_induction` | validates every supplied loop reference; finite invariant rounds, depth-three affine recognition, checked arithmetic, and iterative visited-set AST walks; a 25,000-node region is regression-tested |
| `build_symbol_table` / `resolve_calls` | visited sets cut parent and callee-subtree cycles; deterministic scope-depth, candidate-retention, and call-count caps bound configured work |
| `CfgExtractor::process_node` (and every `process_*` handler) | a **path set** on `CfgContext` — a node already on the current recursion path becomes its own exit instead of being re-entered |
| DFG reaching definitions (`visit_reaching`) | a **path set** — same rule |
| DFG member-to-field resolution | one immutable declaration index; every owner/scope walk uses cycle-bounded `ast_ancestors`; exact lexical/type/field names and uniqueness are required, so malformed, missing, or ambiguous evidence emits no `Reference` |
| `method_field_graph` / `ck_metrics` / `function_metrics` | iterative visited function/member walks, declaration uniqueness, typed field/call evidence, inheritance SCC condensation, ordered sets, saturating counts, and finite arithmetic edge cases; nested functions remain separate scopes |

A *path* set rather than a global visited set is used in the two extractors so
that a node legitimately reachable from two disjoint branches is still processed
on each path; only a genuine cycle is cut. Tree input is therefore unaffected.

Field resolution has a different threat: a hostile graph can repeat names or
forge parent pointers to induce a false declaration link. The resolver therefore
compares lexical scope by stable owner `NodeId`, never by display name alone,
requires exactly one type and field candidate, stops parent walks at dangling or
cyclic pointers through `ast_ancestors`, and never removes an existing edge.

These guarantees are pinned by `tests/robustness.rs`, which injects ten distinct
corruptions into a plausible function graph and drives **every** public analysis
over each one — see
[`engineering/02-testing.md`](../engineering/02-testing.md#what-the-properties-assert).

## Hardening checklist for hostile input

1. **Set a strict `max_file_size`** far below the 10 MB default (e.g. tens to a few
   hundred KiB) for the internal-parse path.
2. **On the Mode-B path, enforce your own size and parse-time bounds** before
   `build_from_tree` — it applies neither.
3. **Cap `Vf2Matcher::max_matches`** (and enable the strict toggles) whenever you
   match patterns against untrusted graphs.
4. **Tighten all three `ResolveConfig` caps** for direct resolution of hostile
   hand-built or deserialized graphs.
5. **Pass a sensible `max_nodes`** to every slice call.
6. **Lower `SolverConfig::max_visits_per_block`** for untrusted graphs or
   third-party analysis domains, and enable widening only with a matching loop
   forest.
7. **Treat interval, SCCP, and loop-induction results as conservative
   evidence**, validating language-specific widths, overflow, termination,
   traps, effects, aliasing, and concurrency before any transformation.
8. **Deserialize only trusted graphs** (see the [threat model](00-threat-model.md)
   and [`usage/05-serialization.md`](../usage/05-serialization.md)).
9. **Treat detection output as advisory**, never as a security gate.
10. **Validate graphs you did not build yourself** (deserialized or caller-assembled) if you need *meaningful* results — the traversal guards guarantee termination, not that the answer means anything.
11. **Preflight DSM latency below its hard cap** for interactive or hostile
    workloads; 50,000 prevents unbounded input but does not make
    $`O(n(n+m))`$ work inexpensive.
12. **Bound Martin projection/assignment size at ingress** and retain its
    boundary policy with every result; structural validation is not a latency
    cap or proof that the selected modules represent the intended architecture.
13. **Bound architectural-smell input at ingress** and treat an empty result as
    “no advisory verdict”; the compact API also uses that result for invalid or
    mismatched evidence.
14. **Tighten centrality budgets and iteration requests** for interactive or
    hostile graphs, and require `converged` before presenting an iterative
    ranking as complete.
15. **Cap projection size before community detection** for hostile workloads;
    the 50-pass and 10-level ceilings bound repetition but not one sweep's edge
    volume. Treat every partition as advisory.
16. **Cap CPG/type size before code metrics**, persist frontend/DFG/resolver and
    `CkOptions` policy with the report, and inspect method-field witnesses before
    interpreting a cohesion or coupling threshold.

## Related pages

- [Threat model](00-threat-model.md) — the assets and boundaries these bounds
  defend.
- [Performance](../engineering/03-performance.md) — the asymptotic costs these
  bounds cap.
- [`usage/04-program-slicing.md`](../usage/04-program-slicing.md) — using slices in
  anger.
