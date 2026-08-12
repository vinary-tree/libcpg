# Cohesion-analysis validation

This report records the falsifiable hypotheses, independent oracles, boundary
experiments, and remaining limits for libcpg's deterministic cohesion suite.
The implementation contract is in
[cohesion and structural robustness](../components/graph/cohesion-analysis.md).

## Hypotheses and rejection rules

| Hypothesis | Independent evidence | Reject when |
|---|---|---|
| H1: core numbers are exact | naïve threshold-by-threshold vertex peeling on generated graphs | any node's core number differs |
| H2: trussness is exact | independent fixed-point edge peeling for every `k` on generated graphs | any canonical edge or trussness differs |
| H3: cut vertices and bridges are exact | brute deletion and component recount | any reported or omitted object differs |
| H4: two-edge components are exact | brute bridges followed by an independent breadth-first partition | any node is missing, duplicated, or differently grouped |
| H5: minimum-cut weight is global | exhaustive enumeration of all nontrivial bipartitions through ten vertices | reported weight or returned side's crossing weight differs by more than `1e-10` |
| H6: assortativity is finite and bounded | generated simple topologies plus analytic star | result leaves `[-1,1]`, is non-finite, or star differs from `-1` |
| H7: attack traces measure the requested removals | independent adjacency construction and breadth-first LCC after each generated removal | any trace size, remaining count, LCC, AUC, or truncation flag differs |
| H8: determinism removes storage-order effects | exact repeated values and reversed edge-insertion sequence | any public field differs |
| H9: public APIs align and do not mutate inputs | external projection and parsed Rust call-graph integration | node alignment, canonical order, expected blocks, or graph shape changes |
| H10: malformed and excessive work is rejected | corrupted CSR, invalid/overflowing weights, unknown/duplicate removal ids, and cap fixtures | indexed traversal panics, a plausible result is returned, or a cap is exceeded |
| H11: deep low-link traversal is stack-safe | 25,000-vertex path | process stack overflows or counts differ from path closed forms |
| H12: optional serialization is reproducible | repeated JSON bytes and round trips | bytes differ or decoding changes a field |
| H13: timing never masks invalid semantics | sixteen Criterion smoke cases with pre-timing analytic oracles | an oracle fails before a timed loop begins |

## Independent oracle designs

### Core oracle

For each threshold $`k=0,\ldots,|V|`$, the test starts with every vertex alive
and repeatedly removes all live vertices whose live degree is below $`k`$.
Vertices left at convergence receive at least core number $`k`$. This repeated
fixed-point construction does not use bins or the production mutable-degree
invariant.

### Truss oracle

For every $`k`$, the test starts with every canonical edge alive. It repeatedly
counts third vertices whose two companion edges are still alive and removes
edges with fewer than $`k-2`$ such triangles until stable. Surviving edges
receive trussness $`k`$. This intentionally uses repeated full scans instead of
production support bins and incremental triangle updates.

### Cut oracle

The test first counts components in the original graph. It then deletes each
vertex and each edge in turn and recounts components with breadth-first search.
An increased count is the definition of articulation or bridge truth; no
discovery-time or low-link state is shared with production.

### Minimum-cut oracle

For every generated graph with two through ten vertices, the test enumerates
all masks except the empty and full masks. It sums weights whose endpoint mask
bits differ and chooses the smallest sum. It separately recomputes the crossing
weight of the production partition, so a correct scalar paired with an invalid
side also fails.

### Attack oracle

The test rebuilds undirected adjacency from canonical expected edges. After
each requested deletion it runs its own breadth-first component scan, compares
the LCC and remaining count, and recomputes trapezoidal area from those values.

## Analytic examples

### Cliques, paths, triangles, and bridges

- Every vertex of $`K_6`$ has coreness five.
- Every vertex of a nontrivial path has coreness one.
- Every edge of $`K_4`$ has trussness four.
- Every triangle edge has trussness three.
- An edge outside all triangles has trussness two.
- Two triangles joined by edge `(2,3)` have articulation vertices 2 and 3,
  bridge `(2,3)`, and two three-vertex two-edge-connected blocks.
- An isolated seventh vertex becomes a canonical singleton block.

### Weighted seam

Two weight-five triangles are joined by reciprocal weights `0.4` and `0.6`.
The undirected seam is exactly one. A weight-1000 self-loop cannot cross a cut
and therefore does not change the result. Equal three-vertex sides select the
lexicographically smaller side.

### Assortativity and attack

A five-vertex star has assortativity `-1`. Removing its center first leaves four
singleton components; removing one leaf next leaves three. For a two-step
trace, trapezoidal integration gives area `0.4`.

### Parsed program graph

A Rust fixture constructs two three-function call cycles joined by one call.
Tree-sitter construction, evidence-bounded call resolution, call-graph
projection, low-link analysis, bridge deletion, and minimum cut run end-to-end.
The joining call is the only bridge, the two cycles are the two robust blocks,
the minimum cut has weight one, and the CPG node/edge shape remains unchanged.

## Property campaigns

Ninety-six generated graphs through twenty vertices compare core numbers with
naïve threshold peeling. A separate ninety-six-case campaign through twelve
vertices compares truss, articulation, bridge, and component results against
their independent oracles. Both campaigns check exact repeated results and
reversed insertion order. Another ninety-six weighted graphs through ten
vertices compare Stoer–Wagner to exhaustive bipartitions. Sixty-four generated
attack cases compare every trace step and AUC to independent breadth-first
reconstruction, assert finite bounded assortativity, and repeat both analyses
over reversed edge insertion.

Generated inputs include empty graphs, isolated vertices, self-loops,
reciprocal relations, duplicate insertion candidates, disconnected graphs,
zero weights, and tied cuts. `proptest` shrinking retains the smallest
counterexample if an invariant fails.

## Malformed, cap, and numeric experiments

- a missing reverse-CSR entry is rejected by every public function;
- k-truss rejects exactly 200,001 simple edges before triangle work;
- minimum cut rejects 1,025 vertices before matrix allocation;
- minimum cut rejects negative, not-a-number, and infinite weights;
- reciprocal `f64::MAX` weights prove accumulated-overflow rejection;
- attack simulation rejects a 10,001-step limit;
- attack simulation rejects unknown and duplicate identifiers;
- topological algorithms intentionally accept arbitrary numeric weights
  because they use edge presence only; and
- the shared ten-corruption CPG campaign exercises every function over the
  resulting call projections without panic or mutation.

## Benchmark experiments

`benches/cohesion.rs` has sixteen smoke cases:

- k-core on 1,024- and 8,192-vertex 4-regular ring lattices;
- k-truss on 512- and 2,048-vertex chains of eight-vertex cliques;
- k-truss on 1,024- and 8,192-vertex high-degree stars, guarding the
  smaller-neighborhood support bound against degree-skew regressions;
- articulation/bridge and two-edge-component kernels on both clique chains;
- minimum cut on 64- and 128-vertex weighted two-block seams; and
- center-first attack plus assortativity on 1,024- and 8,192-vertex stars.

Before Criterion times a kernel, analytic invariants establish expected core
number, clique trussness, bridge and component counts, seam weight, star
assortativity, center-removal fragmentation, node alignment, and exact
repeatability.

## Reproduction

```sh
cargo test --all-features cohesion::tests
cargo test --no-default-features --test cohesion_integration
cargo test --features lang-rust --test cohesion_integration
cargo test --all-features --test robustness
cargo bench --bench cohesion --no-default-features -- --test
./scripts/verify.sh
cargo cov-text
cargo +nightly cov-branch
```

Validation output is captured for inspection and removed after the stable
commit, as required by the repository engineering protocol.

## Measured coverage

At the T2.3 cohesion checkpoint, a clean stable maximal-matrix run executed
**703 library tests** and **49 integration/robustness tests**, for **752
executable cases**. A separate clean nightly run executed that same checkpoint
with LLVM branch instrumentation. The measured results are:

| Scope | Lines | Regions | Functions | Branches |
| --- | ---: | ---: | ---: | ---: |
| `analysis/cohesion.rs` | **99.82%** | **99.43%** | **100.00%** | **97.94%** |
| Repository total | **98.04%** | **97.96%** | **98.27%** | **85.89%** |

The cohesion module clears the campaign's 90% line and 80% branch thresholds.
The independent oracles above, not a percentage alone, decide each correctness
hypothesis; coverage shows that those experiments traverse nearly the entire
implementation boundary.

## Limits of the evidence

The tests prove agreement on the generated bounded domains and analytic
fixtures; they are not machine-checked proofs for every graph size. They do not
validate a human architectural interpretation, infer a privileged attack
order, preserve directed variants of the measures, or make a runtime-failure
claim. They do establish deterministic implementations, independent
correctness boundaries, explicit caps, malformed-input behavior, public
composition, and stack safety for the stated contracts.

## References

- Seidman 1983.
  [DOI: 10.1016/0378-8733(83)90028-X](https://doi.org/10.1016/0378-8733(83)90028-X).
- Batagelj and Zaveršnik 2003.
  [arXiv:cs/0310049](https://arxiv.org/abs/cs/0310049).
- Cohen 2008, National Security Agency Technical Report. No DOI assigned.
- Hopcroft and Tarjan 1973.
  [DOI: 10.1145/362248.362272](https://doi.org/10.1145/362248.362272).
- Tarjan 1972.
  [DOI: 10.1137/0201010](https://doi.org/10.1137/0201010).
- Stoer and Wagner 1997.
  [DOI: 10.1145/263867.263872](https://doi.org/10.1145/263867.263872).
- Newman 2003.
  [DOI: 10.1103/PhysRevE.67.026126](https://doi.org/10.1103/PhysRevE.67.026126).
- Holme et al. 2002.
  [DOI: 10.1103/PhysRevE.65.056109](https://doi.org/10.1103/PhysRevE.65.056109).
