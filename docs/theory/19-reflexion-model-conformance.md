# Theory 19 — reflexion-model conformance

A **software reflexion model** compares a developer's intended architecture
with dependencies recovered from an implementation. “Reflexion” is the term
used by Murphy, Notkin, and Sullivan; it is not a spelling of runtime
reflection. The comparison exposes agreement, disagreement, and missing
realization without pretending that source structure alone reveals intent.

## 1. Two directed relations and one mapping

Let $`H`$ be the set of high-level architectural layers and $`L`$ the set of
low-level artifacts, represented here by project-relative paths. The declared
model is a directed relation $`D\subseteq H\times H`$. The implementation is a
multiset of directed dependencies $`E\in\mathcal{M}(L\times L)`$. An ordered
prefix policy induces a partial mapping $`\mu:L\rightharpoonup H`$:

```math
\mu(p)=h_i\quad\text{for the least }i\text{ whose layer has a prefix of }p.
```

The first-match rule is intentionally observable. It lets a narrow prefix such
as `src/api/internal/` precede the broader `src/api/` prefix. `libcpg` applies
`str::starts_with` exactly: it does not canonicalize separators, collapse `..`,
resolve symlinks, inspect the filesystem, or infer a language module.

Same-layer dependencies are always permitted. For an observed edge $`(s,t)`$:

```math
\operatorname{class}(s,t)=
\begin{cases}
\text{unlayered} & \mu(s)\text{ or }\mu(t)\text{ is undefined},\\
\text{convergence} & \mu(s)=\mu(t),\\
\text{convergence} & (\mu(s),\mu(t))\in D,\\
\text{divergence} & \text{otherwise}.
\end{cases}
```

A **convergence** is observed and permitted. A **divergence** is observed but
not permitted. An **absence** is declared but not realized:

```math
A=D\setminus\{(\mu(s),\mu(t))\mid(s,t)\in E\land\mu(s)\ne\mu(t)\}.
```

Only explicitly allowed cross-layer pairs participate in $`A`$; same-layer
convergences do not manufacture declarations.

## 2. Literate algorithm

The implementation separates classification from reporting so a caller can
inspect one edge or summarize a complete dependency relation:

```text
REFLEXION(rules, observed_edges):
  allowed := ordered set of rules.allow
  summary := empty counters, divergences, and realized-pairs set

  for each (source_path, target_path):
    source_layer := first matching layer, if any
    target_layer := first matching layer, if any

    if either layer is absent:
      increment unlayered
    else if the layers are equal:
      increment convergences
    else if (source_layer, target_layer) is in allowed:
      increment convergences
      insert the pair into realized_pairs
    else:
      append the concrete path edge and both layers to divergences

  sort divergences by (source_path, target_path)
  return summary
```

For $`e=|E|`$, $`a=|D|`$, $`h=|H|`$, and at most $`p`$ prefixes per layer,
the current implementation takes $`O(ehp+e\log e+a\log a)`$ time and
$`O(e+a)`$ output/auxiliary space in the worst case. There is no graph search;
work is bounded by caller-supplied collections.

## 3. Determinism and multiplicity

`realized_pairs` is a `BTreeSet`, absences are lexical, and divergences are
sorted. Reordering observed edges therefore leaves the summary value
unchanged. Duplicate observed edges remain duplicate observations: they
increase convergence/unlayered counts or produce duplicate divergence
witnesses. This retains input multiplicity while deduplicating only the
question “was this allowed layer pair realized at least once?”

Duplicate `allow` declarations collapse in absence reporting. Duplicate layer
names are not rejected; first matching layer position still owns a path, while
the name is the high-level identity used in relations. Empty prefixes match
every path and should be treated as an explicit catch-all policy.

## 4. Interpretation

The result is exact for the supplied strings, prefix ordering, allowed pairs,
and dependency multiset. It is not proof that the declared architecture is
good, that an absence is a defect, or that a divergence should be removed.
Generated files, test code, adapters, platform ports, and unresolved imports
often require explicit policy. A useful report preserves the rules, dependency
extractor, path normalization performed by the caller, and concrete divergence
witnesses.

See the [component guide](../components/graph/reflexion-model.md), [ADR
0025](../design/0025-pure-path-reflexion-model.md), and [validation record](../scientific/20-reflexion-model-validation.md).

## Reference

1. Murphy, G. C., Notkin, D., & Sullivan, K. J. (2001). Software reflexion
   models: Bridging the gap between design and implementation. *IEEE
   Transactions on Software Engineering, 27*(4), 364–380. DOI:
   [10.1109/32.917525](https://doi.org/10.1109/32.917525).
