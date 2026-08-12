# Spectral connectivity and graph bisection

Spectral graph theory turns topology into linear algebra. The key object for
libcpg is the combinatorial graph Laplacian, whose null space counts connected
components and whose first nonzero mode describes the graph's least energetic
global variation. The concrete API is documented in
[spectral connectivity and Fiedler partitioning](../components/graph/spectral-analysis.md).

## From edges to an energy

Let $`G=(V,E)`$ be a simple undirected graph with $`n=|V|`$ vertices. Its
adjacency matrix $`A`$ has $`A_{ij}=1`$ when vertices $`i`$ and $`j`$ share an
edge. The degree matrix $`D`$ is diagonal with $`D_{ii}=d_i`$. Define

```math
L=D-A.
```

For any vector $`x\in\mathbb{R}^n`$,

```math
x^{\mathsf T}Lx=\sum_{\{i,j\}\in E}(x_i-x_j)^2.
```

This identity supplies the intuition. A low-energy vector changes slowly
across most edges. The constant vector has zero energy, so zero is always an
eigenvalue. Every connected component can take its own constant value without
paying edge energy; consequently, the multiplicity of zero equals the number
of connected components.

## The constrained minimum

The constant solution is excluded by requiring zero mean and unit norm. The
Rayleigh–Ritz characterization then gives

```math
\lambda_2
=\min_{\substack{x\ne 0\\x\perp\boldsymbol{1}}}
\frac{x^{\mathsf T}Lx}{x^{\mathsf T}x}.
```

For a connected graph this minimum is positive. A minimizing vector is a
Fiedler vector. Small $`\lambda_2`$ means a unit, zero-mean signal can vary
across the graph while paying little edge energy, suggesting a sparse seam.
Large $`\lambda_2`$ means every such global variation crosses substantial
topology.

Closed forms provide useful landmarks:

| Graph | Algebraic connectivity |
|---|---:|
| Complete graph $`K_n`$ | $`n`$ |
| Path $`P_n`$ | $`2\left(1-\cos(\pi/n)\right)`$ |
| Cycle $`C_n`$ | $`2\left(1-\cos(2\pi/n)\right)`$ |
| Disconnected graph | $`0`$ |

Thus $`P_4`$ has $`\lambda_2=2-\sqrt{2}\approx0.5858`$, whereas $`K_4`$ has
$`\lambda_2=4`$.

## Why the sign reveals a seam

Suppose a graph contains two dense regions joined by few edges. The
lowest-energy zero-mean vector tends to be nearly constant within each region
and changes sign near the sparse join. Thresholding at zero turns the relaxed
real-valued solution into two vertex sets:

```math
S_+=\{v\in V\mid x_v\ge 0\},
\qquad
S_-=V\setminus S_+.
```

This sign partition is a heuristic rounding of a continuous relaxation. It is
not guaranteed to minimize an unnormalized cut, ratio cut, or normalized cut.
The value zero is deterministic and natural for a zero-mean vector, while
other thresholds can produce different balance/cut trade-offs.

Shi and Malik's normalized-cut formulation instead incorporates vertex volume
and leads to a normalized or generalized eigenproblem. libcpg deliberately
uses $`L=D-A`$ because the epic's connectivity contract and analytic oracles
are combinatorial. Calling its output “the normalized cut” would conflate two
different objectives.

## Deflated power iteration

Power iteration normally finds the largest eigenvalue. libcpg transforms the
problem with

```math
B=cI-L,
\qquad
c=2\Delta+1,
```

where $`\Delta`$ is maximum degree. Since every Laplacian eigenvalue lies in
$`[0,2\Delta]`$, $`B`$ has positive eigenvalues $`c-\lambda_i`$. The constant
mode would dominate, so each step projects it away:

```math
y\leftarrow Bx,
\qquad
y\leftarrow y-\frac{\boldsymbol{1}^{\mathsf T}y}{n}\boldsymbol{1},
\qquad
x\leftarrow\frac{y}{\lVert y\rVert_2}.
```

On the orthogonal subspace, the largest transformed eigenvalue is
$`c-\lambda_2`$. The original eigenvalue is evaluated directly through the
edge-energy identity rather than obtained by subtracting nearly equal numbers.
Sparse adjacency makes each iteration linear in graph size.

Convergence speed depends on the transformed spectral gap. When
$`\lambda_2`$ and $`\lambda_3`$ are close, unwanted components decay slowly.
This is why libcpg exposes iteration count, convergence, and residual instead
of returning a scalar with implied certainty.

## Disconnected graphs and non-uniqueness

Running an iteration is unnecessary when breadth-first search finds multiple
components: $`\lambda_2=0`$ exactly. libcpg chooses the first dense-order
component $`C`$ and returns the unit zero-mean vector

```math
x_v=
\begin{cases}
\sqrt{\dfrac{n-|C|}{|C|n}}, & v\in C,\\[6pt]
-\sqrt{\dfrac{|C|}{(n-|C|)n}}, & v\notin C.
\end{cases}
```

Every edge stays inside a constant-valued region, so $`Lx=0`$. This is one
canonical member of a multi-dimensional null space.

Non-uniqueness also occurs in connected symmetric graphs. For a cycle, for
example, $`\lambda_2`$ usually has multiplicity two. Any normalized linear
combination in that eigenspace is valid. Deterministic initialization and sign
pinning select a reproducible representative without claiming mathematical
uniqueness.

## Program-analysis interpretation

Applied to call, module, control-flow, or data-flow projections:

- $`\lambda_2=0`$ certifies disconnected projected topology;
- a small positive value suggests a weak global connection;
- Fiedler signs propose two structurally separated review regions; and
- component magnitude can show vertices near or far from the relaxed seam.

The interpretation is conditional on projection construction. Direction,
weights, unresolved calls, reflection, generated code, and dynamic dispatch are
outside this unweighted topology. Spectral evidence should guide inspection,
not automatically approve module extraction or claim runtime isolation.

## References

- Fiedler, “Algebraic Connectivity of Graphs,” 1973.
  [DOI: 10.21136/CMJ.1973.101168](https://doi.org/10.21136/CMJ.1973.101168).
- Shi and Malik, “Normalized Cuts and Image Segmentation,” 2000.
  [DOI: 10.1109/34.868688](https://doi.org/10.1109/34.868688).
