# Scientific validation 62 -- relational abstract domains

This record tests whether libcpg's octagon, exact rational polyhedron, and
reduced-product domains satisfy their stated lattice, soundness, determinism,
composition, serialization, and stack-safety claims.

## 1. Falsifiable hypotheses

| ID | Claim | Failure evidence |
|---|---|---|
| H1 | Every supplied concrete point belongs to its octagon or polyhedron abstraction. | Any generated source point fails membership. |
| H2 | Join contains both operands, is deterministic under operand/point order, and respects lattice ordering. | Reordered joins differ or an operand is not below the result. |
| H3 | Exact polyhedral intersection equals concrete conjunction. | Any point in the bounded independent grid disagrees. |
| H4 | Octagons retain $`x-y\leq c`$ when marginal intervals cannot, and exact polyhedra retain a non-octagonal affine facet. | A violating point is admitted or a valid point rejected. |
| H5 | Reduced products exchange facts after construction, transfer, real CFG merge, widening, and narrowing. | A component remains less precise than a sound reduction consequence. |
| H6 | Widening stabilizes and narrowing restores sound incoming precision. | A finite old facet oscillates, a concrete incoming point is lost, or narrowing excludes the descending operand. |
| H7 | Out-of-domain sparse variables, wrong-dimensional points, arithmetic limits, and malformed serialized matrices are total or typed errors. | Panic, invalid indexing, silent arithmetic wrap, or invariant bypass. |
| H8 | Closure, projection, hull, redundancy, and reduction have constant native call depth. | The deep/dimensional lifecycle overflows a 256 KiB worker stack. |

## 2. Example and property methods

The octagon suite uses exact examples for relational closure, contradiction,
interval projection, interval reduction, join/widen/narrow, out-of-domain
growth, and malformed dimensions. A 128-case property generates up to fifteen
three-dimensional integer points, reverses their order, checks every point,
splits the set, joins both operand orders, and verifies both lattice lower
bounds.

The polyhedron suite uses exact examples for triangle feasibility, an affine
line-segment hull, rational thirds and halves, mixed-dimension bottom identity,
widening/narrowing, out-of-domain growth, and serde recanonicalization. Two
96-case properties provide independent observations:

- generated point hulls are compared with reversed point order, both join
  orders, operand ordering, and every source point;
- generated rational boxes are intersected in both orders and compared against
  concrete Boolean conjunction at every integer point in
  $`[-7,7]\times[-7,7]`$.

The public integration suite instantiates
`ReducedProduct<IntervalEnv, Octagon, IntervalOctagonReduction>` as a real
`DataflowAnalysis` domain. Two CFG branches constrain $`x`$ to zero or four
under $`x=y`$; the merge must reduce both interval coordinates to $`[0,4]`$
while the octagon still rejects $`(0,4)`$. A feature-gated external API test
also places an exact line hull and octagon in the generic direct product and
checks componentwise order.

## 3. Stack and resource method

The octagon lifecycle closes a 96-variable relation chain on a 256 KiB native
stack. The polyhedron lifecycle constructs, canonicalizes, queries, and orders
a 24-dimensional rational box on the same stack size. These dimensions are
logical heap sizes: neither test spawns recursive frames.

All builds and tests run with one Cargo job inside
`MemoryMax=4G`, `MemorySwapMax=0`, and `TasksMax=256`. Captures stay under
disk-backed `target/verification`, are inspected, and are removed by exact
path. The generated hull property was initially allowed up to six points; it
used three CPU minutes because exact repeated hull projection is combinatorial.
Restricting each of the same 96 cases to at most three points preserved every
oracle and reduced focused runtime to about two seconds.

## 4. Focused results

Nine exact-polyhedron example/property/depth/serde tests passed with
`abstract-domains,serde`. The 96-case hull property and 96-case concrete
intersection property both passed. The octagon malformed-serde regression
passed, as did both public integration tests. Strict all-target Clippy with
`abstract-domains,serde` and warnings denied passed under the same resource
scope.

The repository-wide FT.3 verifier and base-aware bug gate are recorded after
the documentation tranche; this section must be updated with their exact
counts before the work item is claimed done.

## 5. Reproduction

```text
systemd-run --user --scope \
  -p MemoryMax=4G -p MemorySwapMax=0 -p TasksMax=256 \
  env CARGO_BUILD_JOBS=1 \
  cargo test --features abstract-domains,serde --lib polyhedra::tests::

systemd-run --user --scope \
  -p MemoryMax=4G -p MemorySwapMax=0 -p TasksMax=256 \
  env CARGO_BUILD_JOBS=1 \
  cargo test --features abstract-domains \
    --test abstract_domains_integration

systemd-run --user --scope \
  -p MemoryMax=4G -p MemorySwapMax=0 -p TasksMax=256 \
  env CARGO_BUILD_JOBS=1 \
  cargo clippy --all-targets --features abstract-domains,serde \
    -- -D warnings
```

## 6. Interpretation boundary

The focused results support H1--H8 for the generated and exact cases above.
They validate domain algebra and implementation behavior, not completeness of a
language transfer, machine-integer overflow semantics, scalability to
unbounded constraints, or precision after an arbitrary widening schedule.

## References

1. Cousot, P., Halbwachs, N. (1978). *Automatic Discovery of Linear
   Restraints Among Variables of a Program.* DOI:
   [10.1145/512760.512770](https://doi.org/10.1145/512760.512770).
2. Cousot, P., Cousot, R. (1979). *Systematic Design of Program Analysis
   Frameworks.* DOI:
   [10.1145/567752.567778](https://doi.org/10.1145/567752.567778).
3. Miné, A. (2006). *The Octagon Abstract Domain.* DOI:
   [10.1007/s10990-006-8609-1](https://doi.org/10.1007/s10990-006-8609-1).
