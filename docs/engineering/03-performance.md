# Performance

This page explains the data-structure choices behind `libcpg`, the asymptotic
cost of its core operations, and the current state of benchmarking. It draws a
firm line between dependencies that are
**actually wired into `src/`** and dependencies that are **declared in
`Cargo.toml` but not yet used**, so you can reason about real behaviour rather
than aspiration.

## Data structures that are wired in

These are the choices the code actually makes, and why:

| Concern | Choice | Why |
| --- | --- | --- |
| Graph storage | [`petgraph`](../GLOSSARY.md#petgraph) `DiGraph<CpgNode, CpgEdge>` | Mature, well-tested directed-graph store with stable `NodeIndex`/`EdgeIndex` handles. One store holds all four [overlays](../GLOSSARY.md#code-property-graph-cpg); exact analyses use normalized `GraphProjection` CSR views. |
| Id → index maps | `rustc_hash::FxHashMap` / `FxHashSet` | The CPG keeps `NodeId → NodeIndex` (and edge) maps for O(1)-expected lookup. `FxHash` is a fast, non-cryptographic hasher — a good fit because keys are library-assigned `u32` ids, not attacker-controlled strings. (Security implications: [`security/01`](../security/01-input-and-resource-hardening.md).) |
| Per-node child lists | `smallvec::SmallVec<[NodeId; 4]>` | `CpgNode::children` stores up to four child ids **inline**, with no heap allocation — the common case for AST fan-out — spilling to the heap only for wider nodes. |
| Shared source text | `std::sync::Arc<str>` | `CpgNode::text`, operator strings, and string/regex literal payloads are `Arc<str>`: immutable, shared, and cloned in O(1) by bumping a refcount instead of copying bytes. |
| Source positions | `text-size` `TextRange` | `SourceRange` stores six `u32` offsets compactly; `to_text_range()` converts to `text_size::TextRange` for callers that use that ecosystem. |
| Error handling | `thiserror` | Derives the [`Error`](../GLOSSARY.md#code-property-graph-cpg) enum without hand-written boilerplate. |

The upshot: a CPG is one contiguous petgraph arena of `CpgNode`/`CpgEdge` values,
with small inline child vectors and refcounted text, indexed by fast hash maps.

## Dependencies declared but not yet wired

Several dependencies appear in `Cargo.toml` but have **no call sites in `src/`**.
They are documented here so the performance story is not oversold:

| Dependency | Declared for | Actual state in `src/` |
| --- | --- | --- |
| `rayon` | parallel processing (the crate-level doc comment lists it under "Parallel Processing (planned)") | **Not wired.** There is no `par_iter` / `into_par_iter` / `par_bridge` anywhere in `src/`. Analysis is currently single-threaded. |
| `ahash` | an alternative fast hasher | **Unused.** The code hashes with `rustc_hash::FxHash`; `ahash` is never referenced. |
| `string_cache` | interned string labels | **Unused.** Node labels/text are stored as `Arc<str>`, not `string_cache` atoms. |
| `regex` | pattern matching | **Unused as a crate.** The only `"regex"` occurrences in `src/` are node-kind string literals mapped to a `Literal` kind, not calls into the `regex` crate. |

Wiring rayon into the naturally-parallel passes (per-function CFG/DFG extraction,
independent VF2 searches) is a clear future optimization; per the project's
data-driven-optimization discipline it should be **measured before and after**
in an analysis-specific harness.

## Preallocation

Preallocation (sizing a collection up front when the element count is known) is a
project-wide best practice, but its current coverage in `libcpg` is **sparse** —
there is essentially one `with_capacity` site, in the PDG builder. Most collections
grow dynamically. Introducing preallocation at the hot construction sites — for
example, reserving `CpgNode`/`CpgEdge` capacity from the tree-sitter node count
before the AST walk — is a safe, mechanical optimization and a good contribution;
see [contributing](04-contributing.md).

## Asymptotic cost of key operations

The table gives *expected* costs. Symbols: $`n`$ = number of tree-sitter /
AST nodes, $`V`$ and $`E`$ = node and edge counts of the (sub)graph an
operation touches, $`L`$ = GNN layers (`num_layers`), $`d`$ = embedding
dimension, $`k`$ = whole-graph similarity WL iterations (3), and
$`q=\min(\mathtt{iterations},16)`$ = projection-WL rounds,
$`c=\min(\mathtt{max\_iterations},10{,}000)`$ = centrality rounds,
$`p=\min(\mathtt{max\_passes},50)`$ = Louvain sweeps per level, and
$`h=\min(\mathtt{max\_levels},10)`$ = Louvain aggregation levels.

| Operation | Expected cost | Notes |
| --- | --- | --- |
| CPG construction (AST walk) | $`O(n)`$ | One depth-first pass over the parse tree, one `CpgNode` per kept tree-sitter node. |
| CFG extraction (per function) | $`O(n)`$ | A per-construct walk over the function's AST children. |
| DFG reaching-defs (per function) | $`O(n + \sum_L |L|)`$ structural visits | One [AST-ordered sweep](../GLOSSARY.md#ast-ordered-reaching-definitions), with each loop body revisited once; $`|L|`$ is the size of loop region $`L`$. The retained `max_iterations` config is not used by this live algorithm. |
| Dominator/post-dominator tree | worst case $`O(VE)`$ time, $`O(V + E)`$ auxiliary space | Deterministic CHK fixed point with a $`V + 2`$ pass cap, Cytron frontier, and iterative Euler intervals; ancestry queries are $`O(1)`$. |
| PDG construction (per function) | worst case $`O(VE)`$ for control dependence, plus a DFG scan | Reuses the public post-dominator tree's reverse frontier, then emits deduplicated control and data dependence edges. |
| CFG/call-graph SCC decomposition | $`O(V + E)`$ after adjacency normalization | Iterative Tarjan plus condensation construction; dense indices avoid traversal hash lookups, and explicit stacks avoid native-stack growth with path depth. |
| Condensation metrics | $`O(M + E\log E + (C+E)\log C)`$ time, $`O(C+E)`$ auxiliary space | $`C`$ components, $`E`$ condensation edges, and $`M`$ SCC members. Validation checks the public membership map, adjacency is normalized, and a min-id Kahn heap drives levels and weighted critical paths. |
| Coffman–Graham layering | worst case $`O(M + E\log E + C^2\log C)`$ time, $`O(C+E)`$ auxiliary space | Predecessor-label vectors make heap comparison linear in $`C`$ in the worst case; ordered available layers enforce the requested width. The component cap is $`10^6`$. |
| DSM visibility and core–periphery | $`O(V(V+E))`$ time and $`O(V+E)`$ working space after $`O(V+E\log d_{max})`$ validation | One iterative BFS per vertex computes non-reflexive fan-out/fan-in; shared iterative SCC analysis supplies the cyclic core. Inputs above 50,000 vertices return a total capped result before traversal. |
| Variable liveness | conventional $`O(H(B + E_b)W)`$ solve plus $`O(NW)`$ report replay | $`B`$ blocks, $`E_b`$ block edges, $`N`$ reachable CFG nodes, lattice height $`H \le F`$ for $`F`$ variables, and $`W = \lceil F/64 \rceil`$ machine words per state. |
| Available / very-busy expressions | each has conventional $`O(H(B + E_b)W)`$ solve plus $`O(NW)`$ report replay; advisory candidate filtering adds dominance and bounded kill-reachability checks | $`F`$ structural expressions, $`H \le F`$, and $`W = \lceil F/64 \rceil`$; kill indexing is linear in operand memberships and definitions. |
| Evidence-aware complexity | provider cost plus $`O(L + R)`$ composition | Computes the legacy AST fallback, validated function scope, loop forest/dominance, and induction report; then formats $`L`$ loops and combines $`R`$ recursion sites. Provider worst cases are documented with loop and induction analysis. |
| [`backward_slice`](../GLOSSARY.md#backward-slice--forward-slice) / `forward_slice` | $`O(V + E)`$, capped at `max_nodes` | Breadth-first over PDG edges; returns as soon as the slice reaches `max_nodes`. |
| [VF2](../GLOSSARY.md#vf2) `find_matches` | worst case $`O(N!\,N)`$; pruned in practice | Feasibility rules and `max_matches` cut the state space dramatically; see [`theory/05-subgraph-isomorphism-vf2.md`](../theory/05-subgraph-isomorphism-vf2.md). |
| [Jaccard](../GLOSSARY.md#jaccard-similarity) similarity | $`O(V_1 + V_2)`$ | Over the two graphs' node-kind multisets. |
| Weisfeiler-Lehman similarity | $`O(k(V + E))`$ | $`k = 3`$ label-refinement rounds. |
| Projection WL labels / clone classes | $`O(q(V\log V+E+\sum_v d(v)\log d(v)))`$ time, $`O(V+d_{max})`$ temporary space | Directed-degree seeds; sorted incident-label multisets; sorted raw-label ranks each round; clone grouping adds $`O(V\log V)`$ time and $`O(V)`$ space. |
| PageRank / reverse / personalized | $`O(c(V+E))`$ time, $`O(V)`$ working space | Dense forward/reverse CSR and stable floating reduction; hard 10,000-round ceiling. |
| Vertex/edge betweenness | $`O(V(V+E))`$ time, $`O(V+E)`$ working space | Directed vertex Brandes or undirected canonical edge Brandes; preflight work budgets; serial source partitions are public. |
| Eigenvector / Katz / HITS | $`O(c(V+E))`$ time, $`O(V+E)`$ working space | Shifted spectral iteration, sufficient Katz row-sum bound, and paired HITS vectors. |
| Closeness / harmonic | $`O(V(V+E))`$ time, $`O(V+E)`$ working space | One undirected BFS per vertex with an explicit adjacency-visit budget. |
| Burt constraint | $`O(E+\sum_v d(v)^2)`$ time, $`O(V+E)`$ working space | Nonnegative weighted undirected adjacency; exact modeled-neighborhood budget. |
| Multi-level Louvain | worst case $`O(h[p(V+E\log V)+E\log E])`$ time, $`O(V+E)`$ working space | Dense-order local moving sorts touched community ids; deterministic aggregation groups canonical weighted edges. Graph sizes do not increase across levels. |
| Connected-community refinement | $`O(V+E)`$ time, $`O(V)`$ working space after projection validation | Breadth-first search is restricted to each input community; components and output labels are canonicalized by smallest member. |
| K-core decomposition | $`O(V+E)`$ time, $`O(V+E)`$ working space | Batagelj–Zaversnik degree bins with constant-time moves. |
| K-truss decomposition | $`O(E^{3/2})`$ simple-graph neighborhood work, $`O(V+E)`$ space | Support bins and smaller-endpoint triangle discovery; hard 200,000-edge ceiling. |
| Articulation/bridges and two-edge components | $`O(V+E)`$ time, $`O(V+E)`$ space | Iterative low-link DFS; bridge-deletion BFS adds another linear pass. |
| Global minimum cut | $`O(V^3)`$ time, $`O(V^2)`$ space | Dense Stoer–Wagner matrix; hard 1,024-node ceiling and checked finite sums. |
| Degree assortativity | $`O(V+E)`$ time and space including simple projection | One canonical edge pass after degree construction. |
| Attack simulation | $`O(s(V+E))`$ time, $`O(V+E)`$ space | One iterative component traversal after each of at most 10,000 requested removals. |
| Algebraic connectivity / Fiedler vector | $`O(sc(V+E))`$ time, $`O(V+E)`$ space | Exact component pre-pass; at most $`s\le8`$ deterministic linear starts each have hard $`c\le1000`$ ceiling and expose selected convergence plus residual. |
| GNN `propagate` | $`O(L \cdot (V + E) \cdot d)`$ | Each layer means neighbour vectors over the AST/CFG/DFG overlays, then ReLU. |

Two of these deserve emphasis. VF2 is worst-case super-exponential, which is why
its `max_matches` cap and strict-matching toggles matter for untrusted input
([`security/01`](../security/01-input-and-resource-hardening.md)). The live DFG
sweep is structural rather than a CFG fixed point: ordinary regions are visited
once, loop bodies twice, and a path set cuts malformed AST cycles.

## Benchmark coverage and remaining gaps

Thirteen Criterion targets are active. Their twenty-two baseline operation groups are
feature-free; `induction_analysis` also registers a twenty-third,
`algorithm-detection`-gated operation. Every active group constructs and checks a deterministic oracle
before Criterion enters the timed loop:

- `scc_analysis` measures public call-graph decomposition over 1,000- and
  10,000-function paths, rings, and cyclic clusters;
- `condensation` measures public metrics with and without Coffman–Graham
  layering over 1,024- and 8,192-component paths and eight independent chains;
  each fixture checks exact levels, critical path, height, width, coverage, and
  edge orientation before timing;
- `cpg_construction` measures deterministic graph-arena construction over
  1,000- and 10,000-node paths;
- `pattern_matching` measures bounded VF2 matching over 256- and 4,096-node
  paths;
- `dataflow_liveness` measures backward-May liveness over 512- and 4,096-node
  chains, 64- and 512-rung diamond ladders, and 128- and 1,024-node nested-loop
  families;
- `induction_analysis` measures DFG-backed invariant and induction analysis on
  single counted loops with 64, 512, and 2,048 invariant definitions; with
  `algorithm-detection`, its `complexity_evidence` group measures the public
  evidence consumer on the same sizes after checking class, confidence,
  provenance nodes, and non-mutation.
- `dsm` measures repeated-BFS visibility and median/SCC classification over
  256- and 1,024-vertex paths and rings, with exact count, cost, class, and
  cyclic-core oracles;
- `wl` measures four-round labels and clone grouping over 1,024- and
  8,192-vertex rings and outward stars, with exact symmetry, distinction,
  membership, and ordering oracles.
- `centrality` measures PageRank and shifted eigenvector iteration over 256-
  and 1,024-vertex paths, rings, and clustered graphs, plus directed Brandes
  and undirected closeness over the same shapes at 128 and 256 vertices. Its
  pre-timing oracles also exercise HITS, edge betweenness, harmonic centrality,
  and Burt constraint.
- `community` measures multi-level Louvain on 256- and 1,024-vertex rings of
  eight-vertex cliques and connected refinement on 512- and 2,048-vertex
  disconnected path families. Independent modularity, exact repeatability,
  canonical labels, component counts, and induced connectivity are checked
  before timing.
- `cohesion` measures k-core on 1,024- and 8,192-vertex ring lattices, k-truss
  on those sizes of high-degree stars, k-truss and iterative connectivity on
  512- and 2,048-vertex clique chains, weighted
  minimum cut on 64- and 128-vertex seams, and attack/assortativity on 1,024-
  and 8,192-vertex stars. Analytic core, truss, bridge, component, cut,
  correlation, fragmentation, and repeatability oracles run before timing.
- `spectral` measures sparse iteration over 1,024- and 8,192-vertex stars,
  $`K_{32,32}`$ and $`K_{64,64}`$, plus exact disconnected handling over pairs
  of stars with 1,024 and 8,192 total vertices. Closed-form connectivity,
  convergence/residual, alignment, sign, partition, and repeatability oracles
  run before timing.
- `cycles` measures SCC rejection on 1,024- and 8,192-vertex acyclic paths,
  complete enumeration on 1,023- and 8,190-vertex disjoint-triangle families,
  and explicit 1,000-cycle truncation on complete directed graphs of 8 and 10
  vertices. Exact counts, edge closure, canonical starts, repeatability, caps,
  and first-omission evidence are asserted before timing.

```toml
[[bench]]
name = "scc_analysis"
harness = false

[[bench]]
name = "condensation"
harness = false

[[bench]]
name = "cpg_construction"
harness = false

[[bench]]
name = "pattern_matching"
harness = false

[[bench]]
name = "dataflow_liveness"
harness = false

[[bench]]
name = "induction_analysis"
harness = false

[[bench]]
name = "dsm"
harness = false

[[bench]]
name = "wl"
harness = false

[[bench]]
name = "centrality"
harness = false

[[bench]]
name = "community"
harness = false

[[bench]]
name = "cohesion"
harness = false

[[bench]]
name = "spectral"
harness = false

[[bench]]
name = "cycles"
harness = false
```

Run it directly, or save and compare named baselines:

```sh
cargo bench --bench scc_analysis --no-default-features
cargo bench --bench scc_analysis --no-default-features -- --save-baseline before
cargo bench --bench scc_analysis --no-default-features -- --baseline before
./scripts/bench.sh condensation --save-baseline before
./scripts/bench.sh cpg_construction --save-baseline before
./scripts/bench.sh pattern_matching --save-baseline before
./scripts/bench.sh dataflow_liveness --save-baseline before
./scripts/bench.sh induction_analysis --save-baseline before
cargo bench --bench dsm --no-default-features
cargo bench --bench wl --no-default-features
cargo bench --bench centrality --no-default-features
cargo bench --bench community --no-default-features
cargo bench --bench cohesion --no-default-features
cargo bench --bench spectral --no-default-features
cargo bench --bench cycles --no-default-features
cargo bench --no-default-features --features algorithm-detection \
  --bench induction_analysis -- --test
```

DFG/PDG construction, whole-corpus analysis, and GNN propagation remain the
runnable-benchmark gaps. Every implemented architecture-analysis operation in
this epic has an oracle-guarded target before optimization work begins.

### Liveness replay finding

A controlled diagnostic run on 2026-08-10 pinned Criterion to CPU 2 under the
`performance` governor. The first implementation replayed every node from its
block boundary independently and measured about 445 ms for the 4,096-node
chain. The profile implied repeated prefix work, matching a quadratic
$`O(N^2W)`$ replay. Replacing it with one reverse replay per block measured
about 4.80 ms in the same harness and environment: approximately 92.6 times the
throughput. The measurement supports the algorithmic diagnosis for this named
shape and machine; it is not generalized to other inputs or hardware.

### How to add a benchmark

1. **Add a `[[bench]]` target** in `Cargo.toml` and its matching file under
   `benches/`:

   ```toml
   # Cargo.toml
   [[bench]]
   name = "cpg_construction"
   harness = false
   ```

   ```rust
   // benches/cpg_construction.rs — requires: features = ["lang-rust"] when running
   use criterion::{criterion_group, criterion_main, Criterion};
   use libcpg::{CpgBuilder, TreeSitterCpgBuilder, Language};

   fn bench_build(c: &mut Criterion) {
       let builder = TreeSitterCpgBuilder::new();
       let source = "fn main() { let x = 1; let y = x; }";
       c.bench_function("build_rust_small", |b| {
           b.iter(|| builder.build(source, Language::Rust).expect("build"));
       });
   }

   criterion_group!(benches, bench_build);
   criterion_main!(benches);
   ```

   Run it with the grammar enabled: `cargo bench --features lang-rust`.

2. **Follow a rigorous measurement protocol.** The figure below captures it.

![Benchmarking methodology](../diagrams/measurement-methodology.svg)

*Figure — pin CPU affinity and max frequency, warm up, record once with
`perf record --call-graph lbr` (or criterion), tee the output, and generate/analyze
the report in parallel. Source:
[`diagrams/measurement-methodology.puml`](../diagrams/measurement-methodology.puml).*

Concretely:

```sh
# Pin to an isolated core and record a call-graph profile in one pass.
taskset -c 2 perf record --call-graph lbr -- \
  cargo bench --features lang-rust 2>&1 | tee bench-run.log

# Analyze the recorded profile without re-running the workload.
perf report --stdio | tee perf-report.txt
```

Guidelines that apply specifically to `libcpg` measurements:

- **Isolate and fix the clock.** Pin the benchmark to a dedicated core
  (`taskset`/CPU affinity) and hold the cores at maximum frequency so results are
  reproducible.
- **Record once, analyze many.** `tee` the run to a file and derive every figure
  from that single capture instead of re-running the workload for each number;
  generate and read the `perf` report in parallel with other analysis.
- **Baseline before optimizing.** Establish a criterion baseline, change one thing,
  and compare — the discipline the rayon/preallocation opportunities above are
  waiting on.

The same methodology, framed as a scientific protocol, is in
[`scientific/04-measurement-methodology.md`](../scientific/04-measurement-methodology.md).

## Related pages

- [Build and features](01-build-and-features.md) — which dependencies each feature
  pulls in.
- [Testing](02-testing.md) — correctness tests and benchmark coverage.
- [`security/01`](../security/01-input-and-resource-hardening.md) — the bounds that
  keep worst-case costs finite on hostile input.
