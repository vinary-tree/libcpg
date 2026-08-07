# Measurement methodology

**Honest headline: `libcpg` ships one runnable benchmark today.** The active
`scc_analysis` Criterion target measures exact call-graph SCC decomposition over
three synthetic graph families at two sizes. Other major performance surfaces —
construction, DFG/PDG analysis, VF2, and GNN propagation — still lack timing
evidence. The correctness suite and current counts are documented in
[engineering/02 — Testing](../engineering/02-testing.md).

This page documents both the shipped SCC harness and the **rigorous protocol**
for extending that evidence so measurements remain reproducible, controlled,
and auditable rather than reported ad hoc.

## 1. The current state

`criterion` is wired as a dev-dependency and the SCC benchmark is active. The
two older candidate targets remain commented out:

```toml
[dev-dependencies]
criterion = "0.5"
proptest = "1.5"

[[bench]]
name = "scc_analysis"
harness = false

# [[bench]]
# name = "cpg_construction"
# harness = false

# [[bench]]
# name = "pattern_matching"
# harness = false
```

Concretely, the gap is:

- **`benches/scc_analysis.rs` is active.** It measures 1,000- and
  10,000-function paths, rings, and cyclic clusters through the public
  `call_graph_sccs` entry point.
- **Construction, DFG/PDG, VF2, and GNN have no benchmark targets.** Their
  expected costs remain analytical rather than empirical.
- **Automated tests remain correctness tests.** They intentionally do not
  assert wall-clock durations, which would be unstable across machines.
- **`examples/` remains empty.** There are no runnable example programs to use
  as end-to-end workloads.

Timing evidence from the SCC target applies only to its named graph families and
sizes; it must not be generalized to the unbenchmarked surfaces.

![The measurement pipeline: control → measure → profile → analyze](../diagrams/measurement-methodology.svg)

*Figure — the end-to-end measurement pipeline: pin the environment, run a criterion harness once, tee the output, and profile/analyze in parallel. Source: [`diagrams/measurement-methodology.puml`](../diagrams/measurement-methodology.puml).*

## 2. Principle: measure before you optimize

The governing discipline is the scientific method applied to performance, and it forbids speculative optimization:

```text
1. Benchmark + profile FIRST — establish a reproducible baseline.
2. Analyze the profile — identify the actual bottleneck (not the guessed one).
3. Hypothesize a fix — a specific algorithmic or data-structure change.
4. Implement and re-measure under identical conditions.
5. Compare to the hypothesis: confirmed → keep; refuted → revert, re-hypothesize.
6. Record every step (input, environment, numbers) in the ledger for audit.
```

An "optimization" that is not preceded by a measured baseline and followed by a measured comparison is not an optimization; it is an untested change. The remainder of this page makes steps 1 and 4 rigorous.

## 3. Control the environment

Uncontrolled measurement is noise. Before any number is recorded, remove the two largest sources of variance — frequency scaling and scheduler migration:

```text
# 1. Hold every core at its maximum frequency (defeat DVFS / turbo variance):
sudo cpupower frequency-set -g performance

# 2. Build optimized, then PIN the bench to a specific core so the scheduler
#    cannot migrate it mid-run (CPU affinity):
cargo build --release --features lang-rust
taskset -c 3 cargo bench --features lang-rust --bench cpg_construction \
    | tee bench-construction.out
```

Additional controls, in order of impact: reserve the pinned core (`isolcpus`/`nohz_full` at boot, or at least a quiet machine), disable simultaneous multithreading on the measured core, and record the exact CPU model, microcode, kernel, and toolchain version alongside every result so a number can be reproduced. Record hardware specifications with the results — a throughput figure without its machine is not reproducible.

## 4. The Criterion harness

`criterion` supplies warm-up, statistical sampling, outlier detection, and
regression comparison across runs. The shipped SCC target can be run directly
or used for named before/after baselines:

```sh
cargo bench --bench scc_analysis --no-default-features
cargo bench --bench scc_analysis --no-default-features -- --save-baseline before
cargo bench --bench scc_analysis --no-default-features -- --baseline before
```

The benchmark constructs each CPG outside the timed loop and passes it through
`black_box`; the measured operation is the complete public SCC projection,
decomposition, cycle classification, and condensation construction.

Wiring the next performance surface is two steps.

**Step 1 — add or uncomment its bench stanza** in `Cargo.toml` (`harness =
false` hands timing to criterion rather than libtest):

```toml
[[bench]]
name = "cpg_construction"
harness = false

[[bench]]
name = "pattern_matching"
harness = false
```

**Step 2 — add the bench file.** A construction benchmark over Mode A needs the
grammar. This remains a proposed file, not a shipped target:

```rust
// PROPOSED benches/cpg_construction.rs — NOT shipped (see §1). Requires the
// [[bench]] stanza above and dev-dependency criterion = "0.5".
// requires: features = ["lang-rust"]
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use libcpg::{CpgBuilder, Language, TreeSitterCpgBuilder};

fn bench_construction(c: &mut Criterion) {
    let source = "fn main() { let x = 1; let y = x; }";
    let builder = TreeSitterCpgBuilder::new();
    c.bench_function("build_rust_small", |b| {
        b.iter(|| {
            let cpg = builder
                .build(black_box(source), Language::Rust)
                .expect("build should succeed");
            black_box(cpg.node_count())
        });
    });
}

criterion_group!(benches, bench_construction);
criterion_main!(benches);
```

A pattern-matching benchmark can be **feature-free** — it builds the graphs by hand, so it needs no grammar and times the [VF2](../GLOSSARY.md#vf2) search in isolation (reusing the discriminating diamond from [03](03-vf2-completeness.md#3-experiment-e2--completeness-with-exact-backtracking-the-diamond)):

```rust
// PROPOSED benches/pattern_matching.rs — NOT shipped; feature-free.
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use libcpg::{
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, Language, NodeId, SourceRange,
};
// `find_matches` is a method of the `SubgraphMatcher` trait, so the trait must
// be in scope; `Vf2Matcher` is the concrete implementation.
use libcpg::pattern::{SubgraphMatcher, Vf2Matcher};

fn if_node(g: &mut CodePropertyGraph) -> NodeId {
    g.add_node(CpgNode::new(NodeId::new(0), CpgNodeKind::If, SourceRange::default()))
}

fn bench_vf2(c: &mut Criterion) {
    let mut pattern = CodePropertyGraph::new(Language::Rust);
    let p: Vec<NodeId> = (0..3).map(|_| if_node(&mut pattern)).collect();
    pattern.connect(p[0], p[1], CpgEdgeKind::AstChild);
    pattern.connect(p[1], p[2], CpgEdgeKind::AstChild);

    let mut target = CodePropertyGraph::new(Language::Rust);
    let t: Vec<NodeId> = (0..4).map(|_| if_node(&mut target)).collect();
    target.connect(t[0], t[1], CpgEdgeKind::AstChild);
    target.connect(t[0], t[2], CpgEdgeKind::AstChild);
    target.connect(t[1], t[3], CpgEdgeKind::AstChild);
    target.connect(t[2], t[3], CpgEdgeKind::AstChild);

    let matcher = Vf2Matcher::new();
    c.bench_function("vf2_diamond", |b| {
        b.iter(|| {
            let m = matcher.find_matches(black_box(&pattern), black_box(&target));
            black_box(m.len())
        });
    });
}

criterion_group!(benches, bench_vf2);
criterion_main!(benches);
```

`black_box` prevents the optimizer from hoisting or eliding the work being timed — without it, a pure function whose result is discarded can be optimized away, and the benchmark measures nothing.

## 5. Profiling: find the bottleneck, don't guess it

Once a baseline exists, profile the hot path rather than reasoning about it. Record with Last-Branch-Record call graphs — cheap and accurate on modern hardware — and, crucially, **generate and analyze the report in parallel** with the next measurement so profiling does not serialize the workflow:

```text
# Record LBR call graphs of the optimized bench binary, pinned to the core:
taskset -c 3 perf record --call-graph lbr -- \
    target/release/deps/cpg_construction-<hash> --bench --profile-time 10

# Generate the report AND analyze it in the background (parallel), so the next
# run can proceed while this one is being read:
perf report -i perf.data > perf-construction.txt &
```

Complementary tools, matched to the question:

- **Memory profile** — `valgrind --tool=massif` for heap growth over time (relevant to the preallocation strategy documented in [`engineering/03-performance.md`](../engineering/03-performance.md)).
- **Deeper CPU attribution** — the Intel VTune profiler for microarchitectural detail when `perf` is not enough.
- **Blocking / on-CPU stacks** — the `bcc` / `bcc-libbpf-tools` eBPF tools to see where threads sleep or spend wall-time, useful if [rayon](../GLOSSARY.md#petgraph)-parallel passes are added.

## 6. Discipline: run once, tee, analyze from the file

Two rules keep a measurement campaign honest and cheap:

- **Tee once, analyze many.** Pipe each bench/profile run to a file (`| tee bench.out`) and extract every metric from that file. Never re-run a benchmark to read a different column of its output — re-running under even slightly different conditions invalidates the comparison and wastes the controlled window.
- **Parallelize the analysis, not the measurement.** Report generation, plotting, and reading happen in the background *while the measured core stays dedicated* to the next run. The measurement itself is always single-pinned-core and serialized; only the post-processing is parallel.

Preallocation is treated as a *best practice, not a premature optimization*: where the size of a collection is known (node/edge counts during construction), reserving capacity up front avoids reallocation churn and is applied by default rather than deferred to a profiling result.

## 7. What to measure, and the expected cost of each operation

The candidate benchmarks below are ordered by measurement priority. The "expected cost" column is the *analytical* complexity to test the empirical numbers against — a benchmark whose scaling departs from the expected class is itself a finding.

| Operation | Entry point | Expected cost | Why it needs measuring |
|---|---|---|---|
| **SCC decomposition (active)** | `call_graph_sccs` | $`O(V + E)`$ after normalization | The shipped path/ring/cluster benchmark checks scaling across many singleton SCCs, one large SCC, and a condensation DAG of small SCCs. |
| CPG construction | `build` / `build_from_tree` | $`O(n)`$ in tree size | Baseline for every downstream analysis; dominated by the tree-walk. |
| Reaching-defs sweep | `DfgExtractor::extract` | ~$`O(n)`$ per function (loop bodies swept twice) | The P7c sweep is the DFG's hot path; see [02](02-reaching-defs-validation.md). |
| PDG + slicing | `PdgBuilder::build`, `backward_slice` | dominator computation + bounded BFS | On-demand and per-function; slice bounds cap the traversal. |
| **VF2 matching** | `pattern::Vf2Matcher::find_matches` | worst case $`O(N!\,N)`$ (Cordella et al. [[4]](#references)) | **Highest variance** — the factorial worst case makes this the operation whose real-world cost is least predictable from the input size, and therefore the one most in need of empirical bounds and pruning-effectiveness data. |
| GNN propagation | `gnn::CpgGnn::propagate` | $`O(\text{layers} \cdot |E| \cdot d)`$ | Mean aggregation over three overlays; scales with edges and embedding dim. |

VF2's factorial worst case is exactly why the pattern-matching bench in §4 is feature-free and reuses a fixed, discriminating graph: it isolates the search cost from parsing and lets the pruning effectiveness be measured directly.

## 8. Deterministic oracles you can already assert

Several outputs are exactly reproducible and make excellent regression oracles
to pair with benchmarks — they let a performance change be validated for
*correctness* at the same time it is measured for *speed*:

- **SCC partition** — every projected node occurs in exactly one component,
  components and members retain stable node-id ordering, and the condensation
  graph is acyclic. The SCC property suite compares the partition with
  `petgraph`'s independent Tarjan implementation.
- **Shape equivalence** — `node_count()` / `edge_count()` agreement between `build` and `build_from_tree` (proved in [01](01-cpg-invariants-and-equivalence.md#4-the-equivalence-theorem-build--build_from_tree)); a construction optimization that changes a count has changed the graph and is wrong.
- **[Cyclomatic complexity](../GLOSSARY.md#cyclomatic-complexity)** — `cyclomatic_complexity()` computes $`M = E - N + 2`$ over the CFG (McCabe [[6]](#references)); it is a closed-form integer, ideal as a fast, exact correctness check embedded in a construction benchmark.

Pairing an exact oracle with each timing run means a faster build that silently corrupts the CPG cannot masquerade as a win.

See also [`engineering/03-performance.md`](../engineering/03-performance.md) for the data-structure rationale ([petgraph](../GLOSSARY.md#petgraph), `rustc-hash`, `smallvec`, `Arc<str>`) that these benchmarks would exercise, and [`engineering/02-testing.md`](../engineering/02-testing.md) for how the inline correctness suite is run.

## References

4. Cordella, L. P., Foggia, P., Sansone, C., Vento, M. (2004). *A (Sub)graph Isomorphism Algorithm for Matching Large Graphs.* IEEE TPAMI 26(10). DOI: [10.1109/TPAMI.2004.75](https://doi.org/10.1109/TPAMI.2004.75)
6. McCabe, T. J. (1976). *A Complexity Measure.* IEEE Transactions on Software Engineering SE-2(4). DOI: [10.1109/TSE.1976.233837](https://doi.org/10.1109/TSE.1976.233837)
