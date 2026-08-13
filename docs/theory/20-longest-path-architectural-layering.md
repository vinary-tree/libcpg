# Theory 20 — longest-path architectural layering

Architectural layering asks a directional question: which components sit above
others, and which dependencies bypass or oppose that ordering? `libcpg` answers
it over a module graph after [strongly connected components
(SCCs)](../GLOSSARY.md#strongly-connected-component-scc) have been contracted.
The resulting [condensation graph](../GLOSSARY.md#condensation-graph--condensation-metrics)
is a directed acyclic graph (DAG), so a longest-path level exists for every
component.

## 1. From source depth to architectural height

Let $`G_c=(V_c,E_c)`$ be the SCC condensation and let $`pred(v)`$ be the
immediate predecessors of component $`v`$. The source-oriented longest-path
level from [Theory 02](02-control-flow-and-complexity.md) is

```math
\ell(v)=
\begin{cases}
0, & pred(v)=\varnothing,\\
1+\max_{u\in pred(v)}\ell(u), & \text{otherwise}.
\end{cases}
```

Define height $`H=1+\max_{v\in V_c}\ell(v)`$ for non-empty input and the
bottom-first architectural layer

```math
a(v)=(H-1)-\ell(v).
```

Layer zero is lowest. Larger values are higher, matching the convention used
by pgmcp's architectural recommendations. For every valid condensation edge
$`(u,v)`$, longest-path monotonicity gives $`\ell(v)\ge\ell(u)+1`$ and therefore
$`a(u)>a(v)`$: dependencies point downward.

This reversal is not the width-bounded Coffman–Graham (CG) schedule. Both are
sink-oriented presentations, but this analysis preserves the exact longest-
path levels already computed by `CondensationMetrics`; it does not rebalance
components to meet a width limit. Coffman and Graham's precedence layering is
the scheduling context for the direction convention [[1]](#reference).

## 2. Three violation classes

For a cross-component edge $`(u,v)`$, define its signed descent
$`d(u,v)=a(u)-a(v)`$:

```math
\operatorname{class}(u,v)=
\begin{cases}
\text{healthy}, & d(u,v)=1,\\
\text{skip-layer}, & d(u,v)>1,\\
\text{upward}, & d(u,v)\le 0.
\end{cases}
```

A **skip-layer dependency** reaches past at least one intervening layer. An
**upward violation** includes a lateral edge because neither case points
strictly downward. A genuine `CondensationMetrics` value for the same
decomposition cannot produce the third case; retaining it makes stale or
foreign deserialized level vectors diagnosable instead of silently healthy.

An SCC marked `cyclic` contains at least one internal edge participating in a
cycle. Condensation deliberately removes internal edges, so the precise
remaining witness is the component itself. `CyclicComponent` records that
component once with identical source/target component and layer. It never
fabricates an original edge or multiplicity that the input no longer retains.

## 3. Literate algorithm

```text
INFER_LAYERING(decomposition, metrics):
  require complete, uncapped metrics
  validate the SCC partition and acyclic condensation
  require one bounded level per component, dense levels 0..H-1,
          and height/width consistent with that level vector

  for each component v in component-id order:
    layer_of[v] := (H - 1) - metrics.levels[v]
    append v to layers[layer_of[v]]

  for each cyclic component v:
    emit one CyclicComponent witness for v

  for each canonical condensation edge (u, v):
    descent := layer_of[u] - layer_of[v]
    if descent > 1: emit SkipLayer
    if descent <= 0: emit Upward

  sort witnesses by kind and component/layer fields
  return layers, direct lookup, witnesses, and completion state
```

For $`c=|V_c|`$, $`e=|E_c|`$, and $`n`$ original projected nodes, structural
validation plus inference takes $`O(n+c+e)`$ time and $`O(n+c+e)`$ temporary
space because the shared validator rebuilds a canonical condensation
projection. The returned layer and violation storage is $`O(c+e)`$. There is
no recursion, numerical iteration, random state, or mutation of the Code
Property Graph (CPG).

## 4. Determinism and interpretation

Component ids, level-vector positions, inner layer members, and condensation
edges are canonical. Violations have a total ordering. The report is therefore
byte-stable for equal public inputs. An empty decomposition yields an empty,
complete report. Malformed public structs yield an empty, incomplete report;
an upstream cap is preserved as `capped == true`.

The result is structural evidence, not an architectural decree. A skip can be
an intentional facade-to-storage shortcut; a cycle can be a cohesive recursive
subsystem; an upward edge can reveal stale analytics rather than source code.
Consumers should retain the SCC membership and original dependency evidence
when recommending a change.

See the [component guide](../components/graph/layering-analysis.md), [ADR
0026](../design/0026-longest-path-architectural-layering.md), and [validation
record](../scientific/21-layering-validation.md).

## Reference

1. Coffman, E. G., Jr., & Graham, R. L. (1972). Optimal scheduling for two-
   processor systems. *Acta Informatica, 1*, 200–213. DOI:
   [10.1007/BF00288685](https://doi.org/10.1007/BF00288685).
