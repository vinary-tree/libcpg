# Relational abstract-domain security boundary

The numerical domains accept caller-constructed variables, coefficients,
constraints, points, and serialized values. They do not execute source code or
perform I/O, but exact arithmetic and projection can amplify small malicious
inputs into large CPU and heap costs.

## Threats and mitigations

| Threat | libcpg mitigation | Caller responsibility |
|---|---|---|
| Native-stack exhaustion | DBM closure, Fourier--Motzkin projection, redundancy, hull, and reduction use explicit heap loops; small-stack lifecycles are tested. | Keep third-party arithmetic/runtime dependencies patched; stack safety does not replace memory limits. |
| Constraint explosion | Deterministic minimum-product variable selection reduces avoidable intermediate pairs without changing exact semantics. | Bound dimension, row count, wall time, RSS, and analysis concurrency before accepting exact polyhedra. |
| Coefficient growth | Arbitrary-precision rationals avoid overflow and rounding. | Bound input numerator/denominator bit length and total serialized bytes. |
| Malformed serialized invariant | Octagon width is checked and closure restored; polyhedron row widths are checked and constraints recanonicalized. | Reject deserialization errors and use ordinary format byte/depth limits. |
| Floating-point instability | No floating point occurs in exact polyhedra or integer octagons. | Do not convert untrusted decimals through a lossy float before constructing rationals. |
| Unsound silent truncation | libcpg never caps by dropping constraint rows. Widening is an explicit sound operator that drops only moving old facets. | Reject over-budget analyses or conservatively replace the whole value with top; never keep an arbitrary prefix. |
| False proof from incomplete semantics | Domains represent only caller-supplied transfer constraints. | Qualify results against frontend, CFG, arithmetic-overflow, alias, call-resolution, and path-feasibility coverage. |
| Cross-tenant denial of service | The library keeps no global domain cache and uses caller-owned values. | Isolate tenants, limit process/cgroup resources, and avoid sharing one unrestricted solver queue. |

![The projection machine owns explicit heap state, but generated rows and exact coefficient bits remain caller-bounded resources.](../diagrams/polyhedron-projection-machine.svg)

*Figure -- resource amplification boundary. Source:
[`diagrams/polyhedron-projection-machine.puml`](../diagrams/polyhedron-projection-machine.puml).*

## Complexity boundary

For $`n`$ variables and $`m`$ input constraints, octagon closure uses
$`O(n^2)`$ storage and $`O(n^3)`$ arithmetic operations. Fourier--Motzkin
elimination can replace $`p`$ positive and $`q`$ negative rows by $`pq`$ rows
in one round. Repeating that growth is exponential in the worst case, and each
rational operation also depends on numerator/denominator bit length.

The exact polyhedron API deliberately has no silent semantic result cap.
Callers should reject an input before analysis, apply an external cancellation
or process boundary, or conservatively substitute top when their policy allows
loss of precision. Killing an isolated worker is safer than continuing after
partial constraint emission.

## Serialization boundary

`serde` encodings are representations, not trusted canonical forms. On decode:

1. an octagon checks $`(2n)^2`$ matrix length using checked arithmetic;
2. bottom is normalized to its canonical diagonal matrix;
3. non-bottom runs iterative tight closure;
4. a polyhedron checks every dense row width and reruns exact
   canonicalization.

This prevents out-of-bounds indexing and prevents a non-closed matrix or
redundant/non-normalized half-space set from bypassing constructors. Ordinary
deserializer allocation limits remain the caller's responsibility.

## References

1. Cousot, P., Halbwachs, N. (1978). *Automatic Discovery of Linear
   Restraints Among Variables of a Program.* DOI:
   [10.1145/512760.512770](https://doi.org/10.1145/512760.512770).
2. Miné, A. (2006). *The Octagon Abstract Domain.* DOI:
   [10.1007/s10990-006-8609-1](https://doi.org/10.1007/s10990-006-8609-1).
