# Spectral-analysis validation

This report states falsifiable hypotheses, independent oracles, boundary
experiments, and limits for libcpg's deterministic spectral analysis. The API
contract is in
[spectral connectivity and Fiedler partitioning](../components/graph/spectral-analysis.md).

## Hypotheses and rejection rules

| Hypothesis | Independent evidence | Reject when |
|---|---|---|
| H1: connected algebraic connectivity is numerically correct | independent dense symmetric Jacobi eigendecomposition on generated graphs | absolute eigenvalue error exceeds `1e-3` at the fixed 1,000-iteration cap |
| H2: closed-form graph families are recovered | complete graphs, $`P_4`$, and cycles | value differs from its analytic expression beyond the fixture tolerance |
| H3: disconnectedness is exact | independently disconnected examples | value is not exactly zero, iteration occurs, or the vector is not a zero-energy component contrast |
| H4: vectors satisfy the eigenpair contract | independent mean, norm, sign, partition, and residual checks | alignment, orthogonality, unit norm, sign pin, or residual condition fails |
| H5: determinism removes storage-order effects | exact repeated result and reversed insertion sequence | any public field differs |
| H6: topology semantics are isolated from numeric metadata | reciprocal, parallel, loop, negative, infinite, and not-a-number edge fixtures | result differs from the equivalent simple path |
| H7: numerical work is bounded and observable | zero, exact-cap, and `usize::MAX` iteration requests | work exceeds 1,000 steps or capped request differs from exact-cap request |
| H8: malformed inputs are rejected without panic | corrupted reverse CSR and invalid tolerances | checked API returns a plausible value or total API panics |
| H9: public APIs align and do not mutate inputs | external projection and parsed Rust call-graph integration | identifiers, partition shape, expected path spectrum, or source graph shape changes |
| H10: sparse traversal is stack-safe | 25,000-vertex path | process stack overflows or output alignment changes |
| H11: optional serialization is reproducible | repeated JSON bytes and round trips | bytes differ or decoding changes options/results |
| H12: timing never masks invalid semantics | six Criterion smoke cases with pre-timing analytic oracles | an oracle fails before measurement starts |
| H13: symmetric initialization cannot masquerade as the Fiedler pair | minimized connected five-node graph plus independent Jacobi spectrum | selected value is the primary start's $`3`$ instead of true $`1`$ |

## Independent dense oracle

Generated topologies have between two and nine vertices. The oracle constructs
the full combinatorial Laplacian independently from canonical expected edge
pairs. A symmetric Jacobi rotation repeatedly selects the largest off-diagonal
entry and annihilates it with an orthogonal plane rotation. When every
off-diagonal magnitude is at most `1e-13`, sorted diagonal entries approximate
the spectrum and the second entry is the reference $`\lambda_2`$.

This oracle shares neither sparse adjacency, deflated power iteration,
component handling, nor Rayleigh code with production. The `1e-3` rejection
envelope reflects the production contract's fixed 1,000-step ceiling, not the
Jacobi stopping precision. A converged production result must additionally have
residual at most `1e-5`; a nonconverged one must report exactly the cap.

### Symmetry regression

Coverage generated a connected five-node topology on which the primary
centered linear start is exactly an eigenvector for $`\lambda=3`$ and has no
component in the $`\lambda_2=1`$ eigenspace. Residual-only validation therefore
cannot detect the wrong eigenvalue. The retained minimized regression requires
the bounded cyclic-start family to select value one while separately proving
that the primary start still yields three. The dense Jacobi oracle was not
weakened.

## Analytic examples

- For every $`2\le n\le8`$, $`K_n`$ returns $`\lambda_2=n`$.
- $`P_4`$ returns $`2-\sqrt{2}`$ and separates its two middle vertices.
- For every $`3\le n\le12`$, $`C_n`$ returns
  $`2(1-\cos(2\pi/n))`$.
- A six-vertex graph with three connected components returns exact zero and a
  positive first-component indicator without iteration.
- A nonconsecutive-id external $`P_4`$ preserves identifier alignment and does
  not mutate its projection.
- Four parsed Rust functions calling in a chain produce the $`P_4`$ spectrum
  and a two-by-two sign cut without changing CPG node or edge counts.

Every eigenpair example checks zero mean, unit norm, canonical positive first
nonzero component, sign/partition equivalence, finite residual, and exact node
alignment.

## Property campaign

Ninety-six generated cases include isolated vertices, self-loops, reciprocal
relations, duplicate insertion candidates, connected graphs, disconnected
graphs, trees, cycles, and denser topologies. Each case:

1. compares production $`\lambda_2`$ with the dense Jacobi oracle;
2. checks the 1,000-step bound and convergence/residual relationship;
3. checks zero disconnected starts or at most eight connected starts;
4. checks unit norm, zero mean, canonical sign, and aligned partition;
5. requires an exact repeated result; and
6. rebuilds edges in reverse order and requires the same exact result.

`proptest` shrinking retained a small slow-convergence graph that reaches the
iteration cap, the five-node higher-eigenpair symmetry, and a disconnected
start-count boundary. Keeping these regression seeds ensures future changes
honor both the approximate capped contract and exact component bypass.

## Boundary and adversarial experiments

- empty and singleton projections return `Ok(None)`;
- incomplete reverse CSR returns `AnalysisError::InvalidInput`;
- not-a-number, infinite, and negative tolerances are rejected;
- the total wrapper maps checked rejection to `None` without panic;
- zero iterations return the deterministic initial estimate with
  `converged == false`;
- `usize::MAX` is clamped and exactly matches a 1,000-step request per start;
- reciprocal, duplicate, and self-loop topology cannot change the simple graph;
- negative, infinite, and not-a-number edge weights are intentionally inert;
- a 25,000-vertex path executes one sparse iteration without recursive depth;
  and
- the shared ten-corruption CPG robustness campaign invokes the checked
  analysis over every derived call projection without mutation or panic.

## Benchmark experiments

`benches/spectral.rs` contains six smoke cases:

- stars with 1,024 and 8,192 vertices, whose algebraic connectivity is one;
- complete bipartite graphs $`K_{32,32}`$ and $`K_{64,64}`$, whose algebraic
  connectivity is respectively 32 and 64; and
- pairs of disconnected stars with 1,024 and 8,192 total vertices, whose
  algebraic connectivity is exactly zero with no iteration.

Before Criterion measures a kernel, each connected case checks the analytic
value, node alignment, convergence, residual, canonical sign, and exact repeat.
Disconnected cases check exact zero, zero iterations, convergence, residual,
and canonical equal-size partition.

## Reproduction

```sh
cargo test --no-default-features spectral::tests
cargo test --all-features spectral::tests
cargo test --no-default-features --test spectral_integration
cargo test --features lang-rust --test spectral_integration
cargo test --all-features --test robustness
cargo bench --bench spectral --no-default-features -- --test
./scripts/verify.sh
cargo cov-text
cargo +nightly cov-branch
```

Validation output is captured for inspection and removed after the stable
commit, as required by the repository engineering protocol.

## Measured coverage

A clean stable maximal-matrix run executed **714 library tests** and **51
integration/robustness tests**, for **765 executable cases**. A separate clean
nightly run executed the same matrix with LLVM branch instrumentation. The
measured results are:

| Scope | Lines | Regions | Functions | Branches |
| --- | ---: | ---: | ---: | ---: |
| `analysis/spectral.rs` | **100.00%** | **100.00%** | **100.00%** | **95.45%** |
| Repository total | **98.07%** | **97.99%** | **98.31%** | **86.07%** |

The spectral module clears the campaign's 90% line and 80% branch thresholds.
The independent dense oracle, retained regressions, analytic families, and
residual invariants—not a percentage alone—decide correctness; complete
line/region/function coverage demonstrates that those experiments traverse the
entire implementation boundary.

## Limits of the evidence

The campaign establishes agreement on bounded generated domains, analytic
families, public integration, malformed boundaries, determinism, and resource
caps. It is not a proof of convergence for every connected graph. The
1,000-step output can be approximate, repeated eigenvalues can admit different
valid eigenvectors, and a sign split does not solve exact minimum-cut or
normalized-cut optimization. No structural result proves a runtime boundary,
module ownership, or security consequence.

## References

- Fiedler 1973.
  [DOI: 10.21136/CMJ.1973.101168](https://doi.org/10.21136/CMJ.1973.101168).
- Shi and Malik 2000.
  [DOI: 10.1109/34.868688](https://doi.org/10.1109/34.868688).
