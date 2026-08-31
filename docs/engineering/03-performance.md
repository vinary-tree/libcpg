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
| Node id → index map | `rustc_hash::FxHashMap` | The CPG keeps one `NodeId → NodeIndex` map for O(1)-expected lookup. `EdgeId` remains in each edge weight; because there is no edge-id lookup API, no duplicate edge map is maintained. `FxHash` is a fast, non-cryptographic hasher—a good fit because keys are library-assigned `u32` ids, not attacker-controlled strings. (Security implications: [`security/01`](../security/01-input-and-resource-hardening.md).) |
| Lazy exact-edge projection cache | three CSR edge-index orders behind `OnceLock<Box<_>>` | A first stable edge read groups exact edges by source/layer, target/layer, and source/target. Warm outgoing, incoming, CFG, DFG, and endpoint views borrow deterministic exact-size rows without allocating. Every public topology mutation invalidates the derived box; mutation-oriented builders use native petgraph adjacency. |
| Per-node child lists | `smallvec::SmallVec<[NodeId; 4]>` | `CpgNode::children` stores up to four child ids **inline**, with no heap allocation — the common case for AST fan-out — spilling to the heap only for wider nodes. |
| Cold metadata | `Option<Box<FxHashMap<…>>>` / `Option<Box<str>>` | Empty node properties and unlabeled edges retain pointer-sized optional fields; maps and label text allocate only when present. |
| Shared mapped text | `std::sync::Arc<str>` plus a build-local `FxHashSet` | Names, types, operators, literals, imports, node text, and allocation types remain immutable `Arc<str>` values. During tree-sitter construction, equal contents in one build share one allocation; the temporary pool is dropped before CFG/DFG work. |
| Source positions | `text-size` `TextRange` | `SourceRange` stores six `u32` offsets compactly; `to_text_range()` converts to `text_size::TextRange` for callers that use that ecosystem. |
| Error handling | `thiserror` | Derives the [`Error`](../GLOSSARY.md#code-property-graph-cpg) enum without hand-written boilerplate. |

The upshot: a CPG is one contiguous petgraph arena of compact
`CpgNode`/`CpgEdge` values, with small inline child vectors, cold boxed metadata,
refcounted text, one fast node-id map, and an optional read-optimized exact-edge
index.

## Parallel execution and the remaining unused dependency

Rayon is wired only into explicit parallel entry points. Other dependencies in
this table still have **no call sites in `src/`**; documenting both states keeps
the performance story bounded to implemented behavior:

| Dependency | Declared for | Actual state in `src/` |
| --- | --- | --- |
| `rayon` | explicit parallel analysis | **Wired.** `CfgExtractor::extract_parallel`, `DfgExtractor::extract_parallel`, `PdgBuilder::build_all_parallel`, and `Vf2Matcher::find_matches_many_parallel` use indexed parallel collection. Serial methods and the default builder configuration remain serial. |
| `regex` | pattern matching | **Unused as a crate.** The only `"regex"` occurrences in `src/` are node-kind string literals mapped to a `Literal` kind, not calls into the `regex` crate. |

Parallel workers borrow one immutable graph and produce heap-owned plans. A
single ordered emitter applies graph mutations, so the implementation needs no
graph clone, shared mutation, or scheduling-sensitive edge assignment. The
controlled worker-count sweep and profiler evidence are reported in
[scientific validation 54](../scientific/54-parallel-analysis-validation.md).
Build-local mapped-string storage uses the existing `rustc-hash` dependency;
controlled string and integer-key comparisons rejected `ahash`, and H11 removed
both `ahash` and `string_cache`. See
[scientific validation 55](../scientific/55-string-storage-validation.md).

## Preallocation

Preallocation (sizing a collection up front when the element count is known) is
used at the major structural boundaries. `build_from_tree` treats tree-sitter's
`descendant_count` as an upper bound and constructs the CPG with node and AST
edge capacity before mapping. Graph projection, continuation machines, and
analysis result builders use local exact or conservative capacities where their
input cardinality is already known. Collections whose retained size depends on
semantic filtering still grow dynamically rather than reserving an unrelated
worst case. New reservations must be measured because over-reservation can
increase peak memory without reducing allocator traffic.

## Asymptotic cost of key operations

The table gives *expected* costs. Symbols: $`n`$ = number of tree-sitter /
AST nodes, $`B`$ = total UTF-8 bytes across mapped string occurrences, $`u`$ =
distinct mapped string contents in one build, $`V`$ and $`E`$ = node and edge counts of the (sub)graph an
operation touches, $`L`$ = GNN layers (`num_layers`), $`d`$ = embedding
dimension, $`k`$ = whole-graph similarity WL iterations (3), and
$`q=\min(\mathtt{iterations},16)`$ = projection-WL rounds,
$`c=\min(\mathtt{max\_iterations},10{,}000)`$ = centrality rounds,
$`p=\min(\mathtt{max\_passes},50)`$ = Louvain sweeps per level, and
$`h=\min(\mathtt{max\_levels},10)`$ = Louvain aggregation levels. For Martin
metrics, $`T`$ is CPG nodes and $`K`$ is unique module names.
For graph edit distance, $`N=V_1+V_2`$ is the square assignment dimension.
For reflexion conformance, $`R`$ is observed path edges, $`H`$ layers, $`P`$
prefixes per layer, and $`A`$ allowed pairs.
For code metrics, $`T`$ is nominal types, $`M`$ methods in one type, $`F`$
fields, and $`A_f`$ retained method-field accesses; $`N_f,E_f`$ describe one
function scope.

| Operation | Expected cost | Notes |
| --- | --- | --- |
| CPG construction (AST walk) | expected $`O(n+B)`$ time and $`O(n+u)`$ temporary traversal/pool handles | One explicit heap-stack pass over the parse tree, one `CpgNode` per kept tree-sitter node, and one borrowed-content Fx lookup per mapped string. Native call depth is constant with respect to source depth. |
| Lazy exact-edge cache build / read | build $`O(V+E+\sum_r d_r\log d_r)`$ time and $`O(V+E)`$ retained space; warm row $`O(1)`$ plus output; layer/endpoint selection $`O(\log d_r)`$ plus output | Three counting/scatter CSR orders preserve exact parallel edges. The first qualifying read pays construction; warm `CpgEdges`, `CfgNeighbors`, and `DfgNeighbors` allocate nothing. Mutation discards the cache. |
| CFG extraction (per parser-built function) | $`O(n + e_c)`$ | Typed heap continuations visit each AST node once and emit $`e_c`$ unique control-flow edges; heap state is $`O(d+x)`$ for depth $`d`$ and live exit frontier $`x`$, while native call depth is constant. Shared hand-built DAG regions retain path-replay semantics. |
| DFG reaching-defs (per function) | $`O(W)`$ structural work and $`O(V+E)`$ conservative traversal heap | $`W = O(V\,2^d)`$ for $`V`$ AST nodes at maximum loop nesting $`d`$: one [AST-ordered sweep](../GLOSSARY.md#ast-ordered-reaching-definitions), with every loop body revisited once per enclosing-pass context. Typed `Enter`/`Bind`/`Exit` frames and an active path make native call depth constant. The public 100,000-level 256 KiB gate completes in 0.75 s; retained `max_iterations` is not used by this live algorithm. |
| DFG field/index auxiliary pass | $`O((D+F+M)h + A)`$ expected time, $`O(D+F)`$ index space | One immutable exact-name index is shared across a full extraction: $`D`$ nominal declarations, $`F`$ fields, $`M`$ member accesses, maximum AST-parent height $`h`$, and $`A`$ member/index/assignment sites. Hash lookup is expected constant time; ambiguity retains all candidates and emits no reference. |
| Dominator/post-dominator tree | worst case $`O(VE)`$ time, $`O(V + E)`$ auxiliary space | Deterministic CHK fixed point with a $`V + 2`$ pass cap, Cytron frontier, and iterative Euler intervals; ancestry queries are $`O(1)`$. |
| PDG construction (per function) | worst case $`O(VE)`$ for control dependence, plus a DFG scan | Reuses the public post-dominator tree's reverse frontier, then emits deduplicated control and data dependence edges. |
| CFG/call-graph SCC decomposition | $`O(V + E)`$ after adjacency normalization | Iterative Tarjan plus condensation construction; dense indices avoid traversal hash lookups, and explicit stacks avoid native-stack growth with path depth. |
| Condensation metrics | $`O(M + E\log E + (C+E)\log C)`$ time, $`O(C+E)`$ auxiliary space | $`C`$ components, $`E`$ condensation edges, and $`M`$ SCC members. Validation checks the public membership map, adjacency is normalized, and a min-id Kahn heap drives levels and weighted critical paths. |
| Coffman–Graham layering | worst case $`O(M + E\log E + C^2\log C)`$ time, $`O(C+E)`$ auxiliary space | Predecessor-label vectors make heap comparison linear in $`C`$ in the worst case; ordered available layers enforce the requested width. The component cap is $`10^6`$. |
| Architectural layering | $`O(M+C+E\log E)`$ validation/inference time and $`O(M+C+E)`$ temporary/output space | Reuses condensation validation, reverses one bounded level per component, and scans canonical edges once; inherits the $`10^6`$ component ceiling. |
| Architectural smells | $`O((V+E)\log(V+K)+R\log R\cdot L)`$ time and $`O(V+E+S)`$ temporary/output space | Validates assignments/Martin metrics, recomputes SCCs, and scans edges once for SCC evidence, degree, and instability shares. $`K`$ is module count, $`R`$ is finding count, $`L`$ is the largest finding-node vector compared during sorting, and $`S`$ is total emitted node evidence. No independent cap: bound the input projection at ingress. |
| DSM visibility and core–periphery | $`O(V(V+E))`$ time and $`O(V+E)`$ working space after $`O(V+E\log d_{max})`$ validation | One iterative BFS per vertex computes non-reflexive fan-out/fan-in; shared iterative SCC analysis supplies the cyclic core. Inputs above 50,000 vertices return a total capped result before traversal. |
| Martin package metrics / SDP | $`O(E\log V+T\log K)`$ metrics and $`O(E\log K)`$ SDP time; $`O(V+K)`$ auxiliary space | Ordered sets count distinct external vertices; CPG type declarations supply abstractness; malformed projections and invalid comparison metrics return no verdict. |
| CK / LCOM class metrics | current worst case $`O(T(N+E+M^2+MF))`$ time and $`O(N+E+MF)`$ temporary/output space | Each class report retains real method-access-field evidence, scans resolved calls/type edges, compares method pairs, and condenses inheritance cycles; stable ordered sets remove insertion-order effects. |
| Halstead / cyclomatic / cognitive / MI | $`O(N_f+E_f)`$ time and $`O(N_f)`$ temporary space | One iterative semantic function-scope walk plus ordered operator/operand vocabularies and merged comment intervals; no source reparse or convergence loop. |
| Reflexion-model conformance | $`O(RHP+R\log R+A\log A)`$ time and $`O(R+A)`$ output/auxiliary space | First textual prefix mapping, ordered allowed/realized pairs, and path-sorted divergences; no parser, graph search, or I/O. |
| Variable liveness | conventional $`O(H(B + E_b)W)`$ solve plus $`O(NW)`$ report replay | $`B`$ blocks, $`E_b`$ block edges, $`N`$ reachable CFG nodes, lattice height $`H \le F`$ for $`F`$ variables, and $`W = \lceil F/64 \rceil`$ machine words per state. |
| Available / very-busy expressions | shared postorder fact construction is $`O((N_e+O_e)\log C)`$ for physical expression nodes $`N_e`$, operands $`O_e`$, and exact structural classes $`C`$; each solve has conventional $`O(H(B + E_b)W)`$ plus $`O(NW)`$ report replay; advisory candidate filtering adds dominance and bounded kill-reachability checks | $`F`$ structural expressions, $`H \le F`$, and $`W = \lceil F/64 \rceil`$; arena ownership is linear in physical syntax, while standalone CEK1 correctly charges occurrence-expanded logical size. |
| Dense bit-set union/intersection | $`O(W)`$ time and $`O(1)`$ auxiliary space | Scalar for $`W<8`$; runtime AVX2 processes four `u64` words per iteration on supported x86/x86-64, then a scalar tail. Subset remains scalar with early exit. Kernel choice changes constants, not semantics or asymptotic cost. |
| Interval value ranges | at most 64 visits per block plus two narrowing sweeps; $`O(D)`$ local expression heap state | Pointwise sorted environments drive the fixed point. Typed `Enter/Unary/BinaryLeft/BinaryRight/Exit` frames retain expression depth $`D`$ on the heap with constant native call depth. The exact public 100,000-level 256 KiB regression completed in 1.17 s on the recorded development host. |
| Loop induction | at most $`C+1`$ invariant rounds after candidate-use summary construction; $`O(D)`$ expression heap state | Typed postorder frames memoize ordinary nested use summaries independent of node-id order; malformed cycles use exact iterative reachability. Typed affine/sign continuations retain logical depth $`D`$ on the heap, while the affine semantic budget remains three. A parent-before-child public 100,000-operator step completes on 256 KiB. |
| Evidence-aware complexity | provider cost plus $`O(L + R)`$ composition | Computes the legacy AST fallback, validated function scope, loop forest/dominance, and induction report; then formats $`L`$ loops and combines $`R`$ recursion sites. Provider worst cases are documented with loop and induction analysis. |
| [`backward_slice`](../GLOSSARY.md#backward-slice--forward-slice) / `forward_slice` | $`O(V + E)`$, capped at `max_nodes` | Breadth-first over PDG edges; returns as soon as the slice reaches `max_nodes`. |
| [VF2](../GLOSSARY.md#vf2) `find_matches` | worst case $`O(N!\,N)`$ time; $`O(P+T+E_P+E_T)`$ auxiliary heap on a live branch | $`P`$ and $`T`$ are pattern and target nodes. Lazy dense metadata and mappings are linear; flat terminal deltas are edge-linear; cursor-only search frames are $`O(P)`$. Fenwick selection is logarithmic, and distinct directed-degree pruning is necessary but cannot bound a no-match search. `max_matches` bounds only emitted results. Native call depth is constant; the public 100,000-node path gate completes on a 256 KiB stack. See [`theory/05-subgraph-isomorphism-vf2.md`](../theory/05-subgraph-isomorphism-vf2.md). |
| [Jaccard](../GLOSSARY.md#jaccard-similarity) similarity | $`O(V_1 + V_2)`$ | Over the two graphs' node-kind multisets. |
| Weisfeiler-Lehman similarity | $`O(k(V + E))`$ | $`k = 3`$ label-refinement rounds. |
| Bipartite GED upper bound | $`O(N^3+E_1+E_2)`$ time, $`O(N^2+E_1+E_2)`$ space | Two deterministic Hungarian solves plus concrete typed-edge script evaluation; each input is capped at 512 nodes, beyond which a flagged linear surrogate is returned. |
| Deterministic CPG diff | $`O(N^3+M\log M+P)`$ time, $`O(N^2+M+P)`$ temporary/output space | Reuses the two bounded Hungarian assignments, then sorts/indexes exact edge occurrences and snapshots changed payload $`P`$; 512-node, 2,000,000-edge, and 2,000,000-edit hard ceilings. |
| Projection WL labels / clone classes | $`O(q(V\log V+E+\sum_v d(v)\log d(v)))`$ time, $`O(V+d_{max})`$ temporary space | Directed-degree seeds; sorted incident-label multisets; sorted raw-label ranks each round. Two call-local dense matrices and canonical/incident vectors retain capacity across rounds, removing repeated allocations without cache state or native recursion; clone grouping adds $`O(V\log V)`$ time and $`O(V)`$ space. |
| Function clone detection | $`O(N+PF+W)`$ before VF2; exponential verifier worst case | $`N`$ is total function AST size, $`P\le100{,}000`$ size-qualified pairs, $`F`$ is compared bag vocabulary, and $`W`$ is bounded WL work. Type-4 PDGs have at most 300 vertices, VF2 stops after one match, and invocation count is explicit. |
| Multi-file merge | $`O(N\log N+E\log E+R)`$ time, $`O(N+E)`$ temporary space | Sorts source identities, translates $`R`$ embedded references, and appends fresh nodes/edges plus CFG metadata. |
| Import graph construction | conservative $`O(DN+ITA)`$ time | Scans $`D`$ namespace declarations, then matches $`I`$ imports of at most $`T`$ tokens against finite alias buckets totaling $`A`$ entries; exact matches append idempotent overlay edges. |
| IFDS/IDE summary solving | $`O(W + I\,S)`$ bounded domain evaluations | $`W`$ is scheduler work, $`I`$ the configured recursive-SCC round cap, and $`S`$ the cost of one domain summary; every unbounded dimension has an `IfdsCaps` ceiling. |
| Effect summaries | $`O(W + I(F+E_c+E_d))`$ bounded work; $`O(D+B+E_d)`$ local heap state | Each domain evaluation scans a function scope of $`F`$ nodes, represented call edges $`E_c`$, and relevant DFG/AST evidence $`E_d`$; typed continuation machines retain logical depth $`D`$, maximum child frontier $`B`$, and terminal cache state on the heap, keep native call depth constant, and widen visibly on logical-depth/work/allocation exhaustion. The 100,000-level 256 KiB public regression gate completed in 1.43 s on the recorded development host. |
| Taint and maybe-null summaries | $`O(W + I(N_f+E_f)D_f\log D_f)`$ bounded work | Each local relaxation propagates at most $`D_f`$ retained facts over typed nodes and admitted DFG edges; shared and domain-local caps qualify partial reports. |
| Typestate analysis | $`O(V_d+E_d+K(N+E))`$ work before ordered-container logarithms | $`V_d,E_d`$ are object-flow size, $`N,E`$ are canonical CFG size, and $`K\le |O||Q|`$ is admitted object/state facts per point. Ordered heap worklists make native stack independent of graph depth; public depth 100,000 completes on 256 KiB. `IfdsCaps` bounds functions, facts, work, and callee depth. |
| Resource-leak projection | $`O(|T|)`$ after typestate | $`T`$ is canonical typestate terminal evidence. Leak projection retains exactly terminals whose states are outside the accepting set; all typestate bounds and witnesses are reused. |
| Uninitialized-use projection | $`O(F+U+R)`$ before ordered-container logarithms | $`F`$ is scoped function evidence, $`U`$ binder-aware uses, and $`R`$ represented reaching definitions. Scope, signature, and callee filtering use iterative graph queries. |
| Exception-flow analysis | $`O(I(F+C)(T+H)\log T)`$ bounded summary/dispatch work | $`I`$ is SCC rounds, $`F`$ functions, $`C`$ call sites, $`T`$ admitted exception types, and $`H`$ handler/type-hierarchy evidence. Finite sets, SCC scheduling, inheritance closure, and witnesses are heap-backed; `IfdsCaps` bounds functions, facts, work, and recursive rounds. |
| Missing-check population inference | $`O((U+A+E_c)\log U+P)`$ before output sorting | $`U`$ is calls, $`A`$ admitted guard-AST evidence, $`E_c`$ call-neighborhood evidence, and $`P`$ same-API site/fingerprint comparisons. `max_check_nodes` and `max_callee_depth` qualify local traversals; flat postorder frames keep native stack independent of source depth. |
| Deviant-behavior inference | $`O((U+R+D)\log U+B)`$ before output sorting | $`R`$ is aggregate bounded CFG reachability, $`D`$ aggregate forward value reachability, and $`B`$ API/belief population comparisons. Per-call CFG and return-evidence caps qualify excluded samples; queues and visited sets are heap-backed. |
| Inconsistent-clone advisories | $`O(P(A_LA_R+G))`$ within configured pair/tape/alignment/GED caps | $`P`$ is admitted exact Type-2/3 pairs, $`A_L,A_R`$ are tape sizes for a non-isomorphic alignment, and $`G`$ is bounded pair-local GED. Same-shape alignment is linear; all tapes/matrices are heap-backed. |
| Infeasible-path advisories | $`O(F(S+I)+V+K)`$ within condition state/step caps | $`F`$ functions, $`S`$ SCCP cost, $`I`$ interval cost, $`V`$ condition-arena nodes, and $`K`$ explicit machine transitions. Boolean alternatives can grow exponentially, so state and transition caps qualify the affected condition. |
| API-usage and temporal mining | $`O(\sum_{c\in C}(N_{f(c)}+E_{f(c)}+A_c))`$ before ordered-container factors, plus delegated typestate cost | Each admitted anchor walks its function CFG, builds one flat AUG of size $`A_c`$, and contributes constant-family population fragments. Call, per-site CFG, AUG node/edge, property, and typestate caps bound work; every traversal is heap-backed. |
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
| Bounded simple-cycle enumeration | output-sensitive exponential worst case, with `O(V+E)` SCC preprocessing | Iterative canonical DFS is limited to length 32 and 100,000 retained cycles and reports its first omitted witness. |
| Directed motif census | `O(V³ + V⁴)` time, $`O(V\lceil V/64\rceil)`$ words | Complete triple/quadruple enumeration uses dense directed bit rows; hard ceilings are 512 triad vertices and 50 graphlet vertices. |
| GNN `propagate` | $`O(L \cdot (V + E) \cdot d)`$ | Each layer means neighbour vectors over the AST/CFG/DFG overlays, then ReLU. |

Two of these deserve emphasis. VF2 is worst-case super-exponential, which is why
its `max_matches` cap and strict-matching toggles matter for untrusted input
([`security/01`](../security/01-input-and-resource-hardening.md)). The live DFG
sweep is structural rather than a CFG fixed point: ordinary regions are visited
once, loop bodies twice, and a path set cuts malformed AST cycles.

## Benchmark coverage and remaining gaps

Thirty-six explicitly declared Criterion targets are active. Most groups are
feature-free; DFG, PDG, corpus, and reanalysis groups request their parser
features, while `induction_analysis` additionally registers an
`algorithm-detection`-gated group. Every active group constructs and checks a
deterministic oracle before Criterion enters the timed loop:

- `scc_analysis` measures public call-graph decomposition over 1,000- and
  10,000-function paths, rings, and cyclic clusters;
- `condensation` measures public metrics with and without Coffman–Graham
  layering over 1,024- and 8,192-component paths and eight independent chains;
  each fixture checks exact levels, critical path, height, width, coverage, and
  edge orientation before timing;
- `cpg_construction` measures deterministic graph-arena construction over
  1,000- and 10,000-node paths and, with `lang-rust`, complete parser-backed
  construction over committed 49- and 514-line Rust modules;
- `traversal_primitives` measures `node`, `ast_children`, `cfg_successors`,
  `dfg_successors`, and `edges_between` over exact three-overlay paths at 1,024
  and 8,192 nodes;
- `dfg_extraction` measures DFG extraction over parser-built, DFG-free Rust
  templates after requiring represented data-flow evidence;
- `pdg_construction` measures PDG construction for every function in those
  parser-built Rust templates after requiring represented dependence evidence;
- `dominator_analysis` measures direct dominator and post-dominator construction
  over 256- and 2,048-stage diamond ladders after checking exact tree,
  post-dominator, exit, and control-dependence cardinalities;
- `pattern_matching` measures bounded VF2 matching over 256- and 4,096-node
  paths;
- `pattern_vf2` is the T9 canonical VF2 search benchmark over the same size
  classes, with an exact one-match cap oracle; its Criterion fixture and
  search-only profile retain constructed graphs so candidate-state cost is not
  obscured by construction;
- `parallel_analysis` measures serial and 1/2/4/8/16/32-worker CFG/DFG/PDG
  pipelines over 512 functions and ordered VF2 batches over 128 patterns. Every
  worker-count fixture must equal the exact serial edge or match sequence before
  timing; each Rayon pool uses a 256 KiB worker stack;
- `dataflow_liveness` measures backward-May liveness over 512- and 4,096-node
  chains, 64- and 512-rung diamond ladders, and 128- and 1,024-node nested-loop
  families;
- `bitset_domain` isolates union, intersection, and subset over 17 widths from
  zero through 65,536 bits. Exact full/striped-set oracles run before timing;
  the 511--769-bit cases expose vector/tail crossover, while subset is the
  early-exit scalar control;
- `approximate_matching` separates exact symbol lookup from bounded edit
  search, tracks call/byte allocation in the measured closure, and measures
  reversible token-index candidate retrieval under an explicit feature gate;
- `expression_equivalence` separates public pair-output scaling at 2/8/32
  candidates from AC-normalization width at 4/16/64 operands; every fixture
  enters through public typed IR lowering and requires complete analysis;
- `semantic_expression_flow` separates exact-proof plus forward/backward flow
  over 2/8/32 linear occurrences from one cross-revision constant fold; each
  fixture requires the expected proof-qualified report before timing;
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
- `similarity_wl` is the T9 canonical similarity baseline for labels and clone
  classes over 1,024- and 8,192-node rings;
- `analyze_corpus` measures parser/build, call resolution, call projection,
  SCC, and WL end to end over committed Rust modules in approximately 50,
  500, and 5,000-line classes plus Python, JavaScript, Go, and C;
- `clones` measures Type-1 equality, Type-3 statement insertion, and Type-4
  PDG-preserving reordering over 16- and 64-function corpora; exact taxonomy,
  membership, and similarity oracles run before timing.
- `advisory_bugs` measures exact Type-2 guard divergence and contradictory
  interval guards over 8- and 32-function corpora. Complete reports, nonempty
  clone findings, and exactly one contradiction per feasibility function are
  asserted before timing.
- `api_usage_mining` measures combined centered-AUG inference and named
  release-after-acquire checking over 20- and 100-peer corpora. Exactly one
  missing-following-call and one non-accepting exit per twenty peers are
  asserted before timing.
- `cwe_templates` measures the complete ten-template registry over 20- and
  100-endpoint request-to-SQL corpora. Complete reports and exactly one CWE-89
  candidate per endpoint are asserted before timing.
- `program_merge` measures collision-free merge and import-graph construction
  over 16- and 64-file programs with 16 statement nodes per file. Exact summed
  counts, contiguous ids, embedded-reference totality, file scopes, resolution
  classes, and dependency counts are asserted before timing.
- `ifds_summaries` measures one-pass acyclic and bounded recursive
  procedure-summary solving over 256- and 1,024-function call graphs. Complete
  first-function transitive closure and nonempty per-function summaries are
  asserted before timing; call-graph SCC construction remains outside the timed
  region.
- `ifds_taint` measures advisory taint summaries over path, one-SCC ring, and
  clustered recursive call graphs with 32 and 128 workers. The exact
  source-to-sink pair and witness endpoints are asserted before timing;
  call-graph SCC construction remains outside the timed closure.
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
- `motifs` measures complete triad and selected graphlet censuses over directed
  paths and out-stars at 24/50 vertices and complete bidirected graphs at 16/30
  vertices. Exact binomial class/graphlet counts, conservation, and repeated
  evaluation are asserted before timing.
- `ged` measures identical paths, fully relabeled isolates, and paths whose
  complete edge kind changes at 16 and 48 nodes. Exact costs `0`, `n`, and
  `n - 1` are asserted before timing.

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
name = "bitset_domain"
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

[[bench]]
name = "motifs"
harness = false

[[bench]]
name = "ged"
harness = false

[[bench]]
name = "clones"
harness = false

[[bench]]
name = "api_usage_mining"
harness = false

[[bench]]
name = "cwe_templates"
harness = false

[[bench]]
name = "program_merge"
harness = false

[[bench]]
name = "traversal_primitives"
harness = false

[[bench]]
name = "dfg_extraction"
harness = false
required-features = ["lang-rust"]

[[bench]]
name = "pdg_construction"
harness = false
required-features = ["lang-rust"]

[[bench]]
name = "pattern_vf2"
harness = false

[[bench]]
name = "similarity_wl"
harness = false

[[bench]]
name = "analyze_corpus"
harness = false
required-features = ["lang-rust", "lang-python", "lang-javascript", "lang-go", "lang-c"]
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
./scripts/bench.sh bitset_domain --save-baseline before
./scripts/bench.sh induction_analysis --save-baseline before
cargo bench --bench dsm --no-default-features
cargo bench --bench wl --no-default-features
cargo bench --bench centrality --no-default-features
cargo bench --bench community --no-default-features
cargo bench --bench cohesion --no-default-features
cargo bench --bench spectral --no-default-features
cargo bench --bench cycles --no-default-features
cargo bench --bench motifs --no-default-features
cargo bench --bench ged --no-default-features
cargo bench --bench clones --no-default-features
cargo bench --bench api_usage_mining --no-default-features
cargo bench --bench cwe_templates --no-default-features
cargo bench --bench program_merge --no-default-features
cargo bench --bench ifds_taint --no-default-features
cargo bench --bench traversal_primitives --no-default-features
cargo bench --bench pattern_vf2 --no-default-features
cargo bench --bench parallel_analysis --no-default-features
cargo bench --bench similarity_wl --no-default-features
cargo bench --bench expression_equivalence --no-default-features
cargo bench --bench approximate_matching --features approximate-matching
cargo bench --bench dfg_extraction --features lang-rust
cargo bench --bench pdg_construction --features lang-rust
cargo bench --bench dominator_analysis --no-default-features
cargo bench --bench analyze_corpus \
  --features lang-rust,lang-python,lang-javascript,lang-go,lang-c
cargo bench --no-default-features --features algorithm-detection \
  --bench induction_analysis -- --test
```

Effect summaries, Martin metrics, CK/LCOM and function metrics, reflexion conformance,
architectural layering, architectural-smell detection, and deterministic CPG
differencing do not yet have dedicated Criterion targets. GNN propagation remains
a broader runnable-benchmark gap. Every other
implemented architecture-analysis operation in this epic has an oracle-guarded
target before optimization work begins.

The complete pre-optimization timing, RSS, allocation, counter, and hot-path
record is the [optimization ledger](optimization-ledger.md).

### Expression-equivalence baseline

The T4.2 smoke baseline keeps pair-output growth separate from
associative-commutative (AC) width. Criterion `--quick` estimated 6.8291 us,
48.236 us, and 652.47 us for 2, 8, and 32 identical candidates. The 4-, 16-,
and 64-operand AC cases estimated 236.26 us, 5.5132 ms, and 196.93 ms.

The directly executed quick binary completed in 0.24 seconds at 43,076 KiB
maximum resident set size with no swap. For the 64-operand case, headless
Massif observed about 1.454 MiB peak total, and DHAT observed 1,424,796 live
bytes in 6,362 blocks at its global maximum. Perf-stat recorded 253.12 ms task
clock, 2,794,582,388 instructions, and 6,740,506 cache misses. These are local
smoke/profile observations, not portable latency or complexity claims. The
full interpretation and profiler limitations are recorded in
[Scientific 70](../scientific/70-expression-equivalence-validation.md).

### Semantic expression-flow baseline

The T4.3 Criterion `--quick` smoke run measured complete proof-plus-flow
analysis for 2, 8, and 32 equal linear occurrences at midpoint estimates of
14.208 us, 72.533 us, and 920.57 us, respectively. The corresponding midpoint
throughputs were 140.77, 110.29, and 34.761 Kelem/s. Cross-revision comparison
of `1 + 2` with `3` measured a 44.401 us midpoint.

The linear fixture includes equality proof construction/replay, both must
fixed points, candidate checks, and report materialization. Public proof-pair
output grows with the number of equal occurrences, so this smoke series is a
regression baseline rather than a claim of linear scaling. The run was
headless, limited to 4 GiB resident memory with swap disabled, and used one
Cargo job. Full test and interpretation evidence is in
[Scientific 71](../scientific/71-semantic-expression-flow-validation.md).

### Liveness replay finding

A controlled diagnostic run on 2026-08-10 pinned Criterion to CPU 2 under the
`performance` governor. The first implementation replayed every node from its
block boundary independently and measured about 445 ms for the 4,096-node
chain. The profile implied repeated prefix work, matching a quadratic
$`O(N^2W)`$ replay. Replacing it with one reverse replay per block measured
about 4.80 ms in the same harness and environment: approximately 92.6 times the
throughput. The measurement supports the algorithmic diagnosis for this named
shape and machine; it is not generalized to other inputs or hardware.

### Runtime-dispatched bit-set finding

The H14A primitive target showed that direct four-word AVX2 union and
intersection amortize their machinery at eight storage words. A four-word
minimum was rejected because a five-word input regressed union 12.75% and
intersection 9.74%. The retained path improved the 8,192-bit operations by
roughly 22--33% and the matched primitive profile by about 48% elapsed/cycles,
while exact DHAT allocation remained identical.

The established liveness families moved from -2.25% to +1.28%, all neutral or
modestly favorable under the measurement threshold. A 100,000-variable
profile was dominated by `variable_facts`, ordered maps, and the worklist. This
is why `bitset_domain` is the kernel diagnosis and `dataflow_liveness` is the
end-to-end acceptance control. See
[validation 58](../scientific/58-runtime-dispatched-bitset-validation.md) and
[ADR-0063](../design/0063-runtime-dispatched-bitset-kernels.md).

### Exact resolver symbol-index finding

The `resolve_calls` target now measures standalone symbol construction and
complete exact, unresolved, ambiguous, and deep-scope resolution. Every case
asserts its complete report and graph topology before timing. Headless
workloads expose the same four causal paths at 32,768 symbols.

A 100,000-symbol controlled comparison rejected external DAT and DynamicDawg
indices for the cold resolver. DAT build was 6.31 times slower; build plus one
exact pass regressed about 30%, RSS rose 26.6%, and total allocation rose
81.6%. DynamicDawg was 2.74 times slower to build and roughly twice as slow for
exact and absent passes. For prefix work, the existing ordered lower-bound
range was 8.70 times faster than the DAT zipper.

An ordered fuzzy automaton did beat a complete bounded scan by roughly 880
times, but fuzzy spelling is not a resolver semantic and its long-query dense
transition table retained quadratic heap. The production outcome is therefore
the exact call-local ordered map with no new dependency. See
[validation 59](../scientific/59-resolver-symbol-index-validation.md) and
[ADR-0064](../design/0064-retain-ordered-resolver-symbol-index.md).

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
# Pin to two physical cores on one CCD and write captures under ignored target/.
mkdir -p target/verification
taskset -c 0,1 perf record --call-graph lbr \
  -o target/verification/perf.data -- \
  cargo bench --features lang-rust \
  > target/verification/bench-run.log 2>&1

# Analyze the recorded profile without re-running the workload.
perf report --stdio -i target/verification/perf.data \
  > target/verification/perf-report.txt
```

Guidelines that apply specifically to `libcpg` measurements:

- **Isolate and fix the clock.** Pin the benchmark to known physical cores in
  one cache/CCD domain (`taskset`/CPU affinity) and hold those cores at maximum
  frequency so results are reproducible.
- **Record once, analyze many.** `tee` the run to a file and derive every figure
  from that single capture instead of re-running the workload for each number;
  generate and read the `perf` report in parallel with other analysis.
- **Baseline before optimizing.** Establish a criterion baseline, change one thing,
  and compare — the discipline the rayon/preallocation opportunities above are
  waiting on.
- **Keep temporary output off memory-backed filesystems.** The wrappers write
  benchmark logs and profiler captures beneath `target/verification`, never
  `/tmp`.
- **Stay headless.** `./scripts/profile.sh` uses perf, DHAT, Massif, `ms_print`,
  and the VTune command-line interface; it never starts a profiler GUI.

The same methodology, framed as a scientific protocol, is in
[`scientific/04-measurement-methodology.md`](../scientific/04-measurement-methodology.md).

## Typed expression-IR resource model

For $`n`$ reachable source facts and $`m`$ ordered operand occurrences, the
lowerer emits exactly $`n`$ arena nodes and $`m`$ operand identifiers when
complete. The explicit work machine processes two frames per newly emitted
node plus one `Enter` frame per shared-source revisit. Ordered active/source
indexes make the conservative bound $`O((n+m)\log n)`$ time and $`O(n+m)`$
heap space; named-law checks are $`O(k)`$ in the selected node arity $`k`$.

`ExpressionIrLimits` independently caps nodes, operands, and work before the
next unit is admitted. The 100,000-level lifecycle demonstrates constant native
call-stack depth, not a universal latency or resident-memory claim. A later
optimization may change index structures only after an oracle-guarded benchmark
and profile show an end-to-end gain while preserving canonical output.

## Tree-sitter parent-context scaling

Tree-sitter nodes do not store a direct public parent pointer. Calling
`Node::parent()` reconstructs context through descendant and child iteration;
doing so once per node in a depth-$`d`$ chain admits $`O(d^2)`$ work. The
preorder builder already observes the direct edge when scheduling each child,
so its heap frame now carries raw syntax parent $`r`$ separately from effective
CPG parent $`e`$. Child scheduling is constant-time context propagation, while
the overall AST walk remains linear in visited nodes plus grammar-owned child
iteration.

A capped debug-profile run of the unchanged 100,000-depth/50,000-width test did
not complete after more than 41 minutes before the repair. A 120-second
headless `perf` sample attributed the hot path to `ts_node_parent`,
`ts_node_child_with_descendant`, and child iteration. After commit `c8dfcb0`,
the same test completed in 1.37 seconds of test time (1.50 seconds wall), with
86,916 KiB maximum RSS and zero swaps. These figures describe that recorded
host/run; they are regression evidence, not a universal latency guarantee.

## Related pages

- [Build and features](01-build-and-features.md) — which dependencies each feature
  pulls in.
- [Testing](02-testing.md) — correctness tests and benchmark coverage.
- [`security/01`](../security/01-input-and-resource-hardening.md) — the bounds that
  keep worst-case costs finite on hostile input.
