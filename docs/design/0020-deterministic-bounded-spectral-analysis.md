# ADR-0020: Deterministic bounded spectral analysis

- **Status:** Accepted
- **Date:** 2026-08-11
- **Decision owners:** libcpg maintainers
- **Scope:** feature-free projection analysis

## Context

libcpg needs algebraic connectivity and a Fiedler bipartition that compose with
stable `GraphProjection` identifiers. The source pgmcp kernel establishes a
useful deterministic sparse-iteration shape, but libcpg additionally needs
complete public-CSR validation, exact behavior for disconnected graphs,
observable numerical convergence, an explicit hard iteration ceiling, stable
public ordering, feature-free operation, and results outside the source graph.

The requested analytic identities—$`K_n\mapsto n`$,
$`P_4\mapsto2-\sqrt{2}`$, and
$`C_n\mapsto2(1-\cos(2\pi/n))`$—select the combinatorial Laplacian. They do not
select the normalized Laplacian used by normalized-cut formulations.

## Decision

libcpg will:

1. expose total `algebraic_connectivity` and checked
   `try_algebraic_connectivity` functions over `&GraphProjection`;
2. return stable node alignment, algebraic connectivity, a unit zero-mean
   Fiedler vector, its sign partition, iteration count, convergence state, and
   residual norm;
3. validate complete forward/reverse CSR shape and require a finite
   nonnegative tolerance;
4. derive a canonical simple undirected topology by forgetting direction,
   deduplicating reciprocal or parallel relations, ignoring loops, and ignoring
   numeric weights;
5. use breadth-first connected components before numerical work and return an
   exact canonical component-indicator eigenvector when disconnected;
6. use sparse deflated power iteration on $`(2\Delta+1)I-L`$ for connected
   graphs;
7. evaluate at most eight cyclic shifts of a fixed centered linear vector,
   select the lowest Rayleigh result with stable residual/vector ties, reduce
   in dense order, and make the first nonzero component positive;
8. clamp each start to `MAX_SPECTRAL_ITERATIONS == 1_000` and the start family
   to `MAX_SPECTRAL_STARTS == 8`;
9. stop only when both eigenvalue change and vector change meet the configured
   tolerance;
10. return capped approximations with `converged == false` and a residual
    instead of presenting them as certified; and
11. keep all outputs immutable and outside both projection and CPG storage.

## Rationale

The simple unweighted topology gives the public function one precise meaning
across call, module, control-flow, data-flow, and external projections. Exact
component detection both avoids needless iteration and correctly handles a
zero eigenvalue with multiplicity greater than one.

The shift $`2\Delta+1`$ is strictly above the combinatorial Laplacian spectrum,
so power iteration on the constant-orthogonal subspace targets $`\lambda_2`$.
Sparse multiplication avoids a dense $`O(|V|^2)`$ matrix. A fixed iteration
ceiling bounds valid adversarial inputs; a residual and explicit convergence
flag preserve numerical honesty at that boundary.

Canonical sign does not make a repeated-eigenvalue eigenspace unique. A single
fixed vector can also be orthogonal to the Fiedler eigenspace and converge to a
higher eigenpair. A bounded family of cyclic linear shifts reduces this
symmetry failure while retaining sparse linear storage; choosing the lowest
Rayleigh quotient is the best variational upper estimate among the starts.
Fixed starts, adjacency, reduction, and tie order make the chosen representative
reproducible for identical topology.

## Alternatives considered

### Dense eigendecomposition

Rejected for the production kernel. It needs quadratic storage and commonly
cubic work, which is inappropriate for large sparse projections. An
independently implemented dense Jacobi method remains valuable as a small-graph
test oracle.

### Iteration without a component pre-pass

Rejected. Orthogonality only to the all-ones vector does not remove the
additional zero modes of a disconnected graph. The iteration could then return
an arbitrary null-space combination after unnecessary work.

### Normalized Laplacian

Rejected for this API because it would contradict the specified closed forms
and change the meaning on irregular-degree projections. A future normalized
analysis needs a distinct name, formulas, and isolated-vertex contract.

### Use edge weights

Rejected for this API. Projection weights may represent counts, confidences,
or caller-specific measurements. Treating them as conductances without an
explicit weighted contract would silently alter semantics and introduce
non-finite/negative numeric policy.

### Return only the eigenvalue

Rejected. The aligned Fiedler vector and partition answer where the weak seam
lies, while iteration count, convergence, and residual let callers qualify the
approximation.

### Use only one linear start

Rejected after property-based coverage produced a connected five-node graph
whose primary linear seed is exactly a $`\lambda=3`$ eigenvector orthogonal to
the true $`\lambda_2=1`$ space. Exact residual and immediate convergence do not
certify that an eigenpair is the second one. Bounded cyclic starts retain the
requested deterministic linear initialization while escaping that symmetry.

### Run until convergence

Rejected. A difficult spectrum or zero tolerance could consume unbounded CPU.
The hard ceiling makes worst-case iteration work explicit and testable.

## Consequences

### Positive

- The API is available with `default = []`.
- Disconnectedness is exact and iteration-free.
- Connected iteration uses linear sparse storage and is stack-safe.
- Non-unique signs and insertion order cannot perturb public bytes.
- Callers can enforce their own residual/convergence acceptance policy.
- Optional serde round trips preserve deterministic result structure.

### Trade-offs

- Direction and numeric weights are deliberately discarded.
- One or more power starts may reach the cap when the relevant spectral gap is
  small; total sparse multiplications are bounded by 8,000 rounds.
- A capped eigenpair is approximate and must be interpreted with its residual.
- The zero-threshold sign heuristic is not an exact minimum-cut or normalized-
  cut optimizer.
- Repeated Fiedler eigenvalues admit other equally valid vectors and cuts.

## Validation

Independent dense Jacobi eigendecomposition checks generated graphs through
nine vertices. A minimized symmetry regression proves multi-start selection
escapes a higher eigenpair. Closed forms cover complete graphs, paths, and cycles; exact
component indicators cover disconnected graphs. Tests also exercise reversed
insertion order, reciprocal/parallel edges, loops, arbitrary numeric weights,
malformed CSR, invalid tolerances, zero and excessive iteration requests,
optional serialization, a 25,000-vertex path, an external public projection,
and a parsed Rust call path. Six Criterion smoke cases validate analytic stars,
complete bipartite graphs, and disconnected stars before timing. See
[spectral-analysis validation](../scientific/15-spectral-validation.md).

## References

- Fiedler 1973.
  [DOI: 10.21136/CMJ.1973.101168](https://doi.org/10.21136/CMJ.1973.101168).
- Shi and Malik 2000.
  [DOI: 10.1109/34.868688](https://doi.org/10.1109/34.868688).
