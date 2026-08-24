# Relational abstract domains

libcpg exposes three interoperable numerical lattices through the data-flow
framework: feature-free intervals, feature-free integer octagons, and optional
exact rational convex polyhedra. `ReducedProduct` composes any two
`Lattice` implementations through a static reduction policy.

![The public domains trade cost for relational expressiveness and meet again through a statically reduced product.](../../diagrams/relational-domain-precision.svg)

*Figure -- public domain selection and composition. Source:
[`diagrams/relational-domain-precision.puml`](../../diagrams/relational-domain-precision.puml).*

## Feature placement

| Surface | Cargo feature | Arithmetic | Intended role |
|---|---|---|---|
| `Interval`, `IntervalEnv` | none | checked `i128` bounds | inexpensive non-relational ranges |
| `Octagon` | none | checked `i128` DBM | signed pairwise relations |
| `ReducedProduct<A,B,R>` | none | component-defined | static fact exchange at solver merges |
| `Polyhedron` | `abstract-domains` | arbitrary-precision rationals | exact general linear relations |

The optional feature adds `num-bigint`, `num-rational`, and `num-traits`.
Enabling `serde` propagates serialization only when those optional dependencies
are present. `full` includes `abstract-domains`; `default = []` remains free of
the arbitrary-precision dependencies.

## Octagon API

```rust
use libcpg::{
    Octagon, OctagonConstraint, OctagonTerm,
};

let mut relation = Octagon::top(2);
relation.assume_all([
    // x - y <= 3
    OctagonConstraint::binary(
        OctagonTerm::positive(0),
        OctagonTerm::negative(1),
        3,
    ),
    // x >= 0
    OctagonConstraint::unary(OctagonTerm::negative(0), 0),
])?;

assert!(relation.contains(&[5, 2]));
assert!(!relation.contains(&[5, 1]));
# Ok::<(), libcpg::OctagonError>(())
```

`assume_all` grows the dense DBM to admit every referenced `u32` variable,
inserts all constraints, and performs one iterative tight closure. Unary bounds
whose exact doubled DBM representation overflows return
`OctagonError::UnaryBoundOverflow`; exact negative closure overflow returns
`ClosureUnderflow`. A mismatched concrete point returns `false`.

## Exact polyhedron API

```rust
// requires: features = ["abstract-domains"]
use libcpg::{Lattice, LinearConstraint, Polyhedron};

let triangle = Polyhedron::from_constraints(
    2,
    [
        LinearConstraint::from_integers([(0, -1)], 0), // x >= 0
        LinearConstraint::from_integers([(1, -1)], 0), // y >= 0
        LinearConstraint::from_integers([(0, 1), (1, 1)], 4),
    ],
)?;
assert!(triangle.contains_integers(&[1, 2]));
assert!(!triangle.contains_integers(&[3, 3]));

let left = Polyhedron::integer_point(&[0, 0])?;
let right = Polyhedron::integer_point(&[2, 2])?;
let hull = left.closed_convex_hull(&right)?;
assert!(hull.contains_integers(&[1, 1]));
assert!(!hull.contains_integers(&[1, 0]));
assert!(left.leq(&hull));
# Ok::<(), libcpg::PolyhedronError>(())
```

`LinearConstraint::new` accepts arbitrary-precision rational coefficients;
`from_integers` avoids floating-point conversion. Equal sparse variables are
summed and exact zero coefficients disappear. `from_constraints` grows the
dimension for an out-of-domain sparse variable and returns a canonical,
irredundant H-representation.

| Method | Semantic operation | Failure/totality behavior |
|---|---|---|
| `contains` / `contains_integers` | exact point membership | wrong dimension is `false` |
| `assume` / `intersection` | exact conjunction/meet | contradiction becomes bottom |
| `closed_convex_hull` | exact lattice join in rational polyhedra | dimension arithmetic can return `DimensionOverflow` |
| `Lattice::join` | same hull through an infallible trait | a representation-dimension overflow conservatively returns top |
| `widen` | retain only stable old facets | finite stabilization by facet deletion |
| `narrow` | exact intersection after widening | bottom and mixed dimensions are total |

![Projection state is explicit and heap resident from input constraints through canonical output.](../../diagrams/polyhedron-projection-machine.svg)

*Figure -- Fourier--Motzkin implementation flow. Source:
[`diagrams/polyhedron-projection-machine.puml`](../../diagrams/polyhedron-projection-machine.puml).*

## Reduced products

```rust
use libcpg::{
    Bound, Interval, IntervalEnv, IntervalOctagonReduction, Octagon,
    ReducedProduct,
};

type State = ReducedProduct<IntervalEnv, Octagon, IntervalOctagonReduction>;
let state = State::new(
    IntervalEnv::from_vars([(
        0,
        Interval::new(Bound::Finite(0), Bound::Finite(4)),
    )]),
    Octagon::top(2),
);
assert_eq!(state.first().get(0), Some(Interval::new(
    Bound::Finite(0),
    Bound::Finite(4),
)));
```

Implementations of `Reduction<A,B>` must be deterministic, idempotent,
reductive in both lattice orders, and preserve the concrete intersection.
`ReducedProduct::transform`, `join`, `widen_with`, and `narrow_with` restore the
policy before returning. The generic Kildall solver therefore reduces every
CFG confluence without a dynamic callback or second fixed-point engine.

## Determinism, cost, and stack safety

Octagon closure costs $`O(n^3)`$ time and $`O(n^2)`$ matrix space for $`n`$
variables. Exact Fourier--Motzkin projection has exponential worst-case work
and can grow arbitrary-precision coefficients. Canonical direction scaling,
ordered maps, stable variable selection, and exact rational comparisons remove
hash-order and floating-tolerance nondeterminism.

All DBM scans, projection rounds, pair products, redundancy checks, product
reductions, and point-hull folds are iterative. Logical dimension and CFG depth
consume heap storage, not recursive native frames. This is stack safety, not a
memory promise: callers must establish size and time policy before selecting
the exact domain.

## Related documentation

- [Theory](../../theory/50-relational-abstract-domains.md)
- [ADR-0067](../../design/0067-exact-relational-abstract-domains.md)
- [Usage guide](../../usage/35-relational-abstract-domains.md)
- [Security boundary](../../security/25-relational-abstract-domain-boundary.md)
- [Scientific validation](../../scientific/62-relational-abstract-domain-validation.md)

## References

1. Cousot, P., Halbwachs, N. (1978). *Automatic Discovery of Linear
   Restraints Among Variables of a Program.* DOI:
   [10.1145/512760.512770](https://doi.org/10.1145/512760.512770).
2. Miné, A. (2006). *The Octagon Abstract Domain.* DOI:
   [10.1007/s10990-006-8609-1](https://doi.org/10.1007/s10990-006-8609-1).
