# Advisory architectural-smell detection

An **architectural smell** is a dependency shape that merits review because it
often accompanies architectural erosion. It is evidence, not a proof of a
defect: a deliberate protocol cycle or gateway may be valid in its context.
`libcpg::architecture_smells` therefore returns immutable, numerical findings
and never rewrites the Code Property Graph (CPG).

The analysis follows the graph-based Arcan family of detectors [1] and the
instability-smell work of Fontana et al. [2]. It composes four existing libcpg
abstractions: a directed `GraphProjection`, its exact strongly connected
components (SCCs), a `ModuleAssignment`, and Martin package metrics.

![A dependency projection, SCC decomposition, module assignment, and Martin metrics pass through validation into four evidence-bearing smell families.](../diagrams/architecture-smells-pipeline.svg)

*Figure 1 — end-to-end evidence flow. Source:
[`architecture-smells-pipeline.puml`](../diagrams/architecture-smells-pipeline.puml).*

## Common notation

Let the directed projection be $`G=(V,E)`$, where $`V`$ is the sorted set of
stable node identifiers and $`E`$ is the deduplicated edge set. Weights are
irrelevant to these structural detectors. Let $`M(v)`$ map a node to a logical
module and let $`I(m)`$ be Martin instability for module $`m`$:

```math
I(m)=\frac{C_e(m)}{C_a(m)+C_e(m)}.
```

Here $`C_a`$ is afferent coupling and $`C_e`$ is efferent coupling.
An isolated module has instability zero. Population mean and standard deviation
for observations $`x_1,\ldots,x_n`$ are:

```math
\mu_x=\frac{1}{n}\sum_{i=1}^{n}x_i,
\qquad
\sigma_x=\sqrt{\frac{1}{n}\sum_{i=1}^{n}(x_i-\mu_x)^2}.
```

Population statistics are appropriate because the analyzed projection is the
complete population selected by the caller, not a sample from a larger graph.

## Detector 1: cyclic dependency

A cyclic-dependency smell is emitted for every SCC $`S`$ with at least two
nodes:

```math
|S|\ge 2.
```

Singleton self-loops are excluded by design because the T2.14 contract defines
this detector over multi-node components. The finding contains all sorted SCC
members. Severity is $`|S|`$; evidence also records the number of projected
edges whose two endpoints lie inside $`S`$.

## Detector 2: hub-like dependency

For node $`v`$, fan-in $`d^-(v)`$ counts distinct non-self predecessors and
fan-out $`d^+(v)`$ counts distinct non-self successors. With configured
non-negative multipliers $`k_-`$ and $`k_+`$, the
two-sided Arcan-style Total-Dependence rule is:

```math
d^-(v)>\mu_-+k_-\sigma_-
\quad\land\quad
d^+(v)>\mu_++k_+\sigma_+.
```

Both inequalities are strict. A one-sided authority or fan-out dispatcher is
not a hub-like finding. Severity is the weaker of the two positive z-scores:

```math
\operatorname{severity}(v)=
\min\!\left(
\frac{d^-(v)-\mu_-}{\sigma_-},
\frac{d^+(v)-\mu_+}{\sigma_+}
\right).
```

Strict outlier selection implies both standard deviations are positive before
the division is evaluated.

## Detector 3: unstable dependency

For source module $`a`$, let $`E_a`$ contain its cross-module
projection edges. A target is *less stable* when its Martin instability is
greater: $`I(a)<I(M(v))`$. The violation share is:

```math
p_a=\frac{
|\{(u,v)\in E_a:I(a)<I(M(v))\}|
}{|E_a|}.
```

A finding is emitted when $`p_a>\tau`$, where $`\tau`$ is the configured
threshold in $`[0,1]`$. Severity is $`p_a`$. The node list
contains the sorted endpoints of violating edges, so every finding retains
concrete projection evidence rather than only a module name.

This direction is Martin's Stable Dependencies Principle: a more stable source
should not depend on a less stable target. The rule is structural and distinct
from churn-based notions of runtime or organizational instability.

## Detector 4: god component

The public signature has no source-line table, so the portable size measure is
the number of assigned projection nodes per module. For module $`m`$ with size
$`s_m`$, the high-side outlier rule is:

```math
s_m>\mu_s+k_s\sigma_s.
```

Severity is the positive node-count z-score. This is deliberately named
`GodComponent`, not `GodClass`: the selected projection determines what a node
means. A module graph may have one vertex per module, while a call graph grouped
by module measures functions per module.

## Literate algorithm

```text
ARCHITECTURE-SMELLS(view, scc, martin, assignment, options)
  reject invalid thresholds or malformed projection
  recompute SCC(view); reject if it differs from scc
  map every projected node to exactly one module
  validate one finite bounded Martin record per used module

  emit each multi-node SCC with internal-edge evidence
  compute fan-in/fan-out population statistics; emit two-sided outliers
  group cross-module edges by source; emit high less-stable shares
  compute assigned node counts per module; emit high z-score outliers

  sort by (smell family, descending severity, smallest node, node vector)
  return findings
```

The validation-first structure is important. Mixing an SCC decomposition from
one graph with metrics or assignments from another could produce convincing
but false evidence. libcpg fails closed with an empty advisory result.

## Determinism and interpretation

`GraphProjection` canonicalizes vertices and edges. Ordered maps/sets preserve
module and evidence order, SCC identity is recomputed, floating-point sorting
uses `total_cmp`, and every output node list is ascending. Edge insertion order,
parallel-edge order, and assignment-pair order therefore cannot change output.

Absence of a smell is not a proof of architectural quality. Results depend on
the selected projection, module boundary, and thresholds. Conversely, a
finding should begin review, not authorize automatic refactoring.

## References

1. Fontana, F. A., Pigazzini, I., Roveda, R., Tamburri, D. A., Zanoni, M., &
   Di Nitto, E. (2017). *Arcan: A Tool for Architectural Smells Detection*.
   2017 IEEE International Conference on Software Architecture Workshops,
   282–285. DOI:
   [10.1109/ICSAW.2017.16](https://doi.org/10.1109/ICSAW.2017.16).
2. Fontana, F. A., Pigazzini, I., Roveda, R., & Zanoni, M. (2016).
   *Automatic Detection of Instability Architectural Smells*. 2016 IEEE
   International Conference on Software Maintenance and Evolution, 433–437.
   DOI: [10.1109/ICSME.2016.33](https://doi.org/10.1109/ICSME.2016.33).
