//! Exact convex polyhedra over arbitrary-precision rational half-spaces.
//!
//! The H-representation stores inequalities `a · x <= b`. Intersection appends
//! half-spaces; subset uses exact linear-form projection; lattice join computes
//! the closed convex hull through a homogenized extended formulation. Every
//! projection uses iterative Fourier–Motzkin elimination with deterministic
//! variable selection, so logical dimension consumes heap state but never
//! native call depth.

use std::{collections::BTreeMap, fmt};

use num_bigint::BigInt;
use num_rational::BigRational;
use num_traits::{One, Signed, Zero};

use super::Lattice;

#[cfg(feature = "serde")]
use serde::{de::Error as _, Deserialize, Deserializer, Serialize};

/// Arbitrary-precision rational scalar used by the polyhedral API.
pub type Rational = BigRational;

/// One closed rational half-space `a · x <= b`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LinearConstraint {
    /// Sparse coefficients keyed by dense variable identifier.
    pub coefficients: BTreeMap<u32, Rational>,
    /// Inclusive upper bound.
    pub bound: Rational,
}

impl LinearConstraint {
    /// Constructs a constraint, combining duplicate variable coefficients and
    /// deleting exact zeros.
    pub fn new(coefficients: impl IntoIterator<Item = (u32, Rational)>, bound: Rational) -> Self {
        let mut canonical = BTreeMap::<u32, Rational>::new();
        for (variable, coefficient) in coefficients {
            *canonical.entry(variable).or_insert_with(Rational::zero) += coefficient;
        }
        canonical.retain(|_, coefficient| !coefficient.is_zero());
        Self {
            coefficients: canonical,
            bound,
        }
    }

    /// Constructs an integer-coefficient constraint without floating-point
    /// conversion.
    pub fn from_integers(coefficients: impl IntoIterator<Item = (u32, i128)>, bound: i128) -> Self {
        Self::new(
            coefficients
                .into_iter()
                .map(|(variable, coefficient)| (variable, integer(coefficient))),
            integer(bound),
        )
    }

    /// Evaluates `a · point` when every referenced coordinate exists.
    pub fn evaluate(&self, point: &[Rational]) -> Option<Rational> {
        let mut value = Rational::zero();
        for (&variable, coefficient) in &self.coefficients {
            let coordinate = point.get(usize::try_from(variable).ok()?)?;
            value += coefficient * coordinate;
        }
        Some(value)
    }

    /// Returns whether the supplied point satisfies this half-space.
    ///
    /// A point missing a referenced coordinate returns `false`.
    pub fn contains(&self, point: &[Rational]) -> bool {
        self.evaluate(point)
            .is_some_and(|value| value <= self.bound.clone())
    }

    fn required_dimension(&self) -> Result<usize, PolyhedronError> {
        self.coefficients
            .last_key_value()
            .map_or(Ok(0), |(&variable, _)| {
                usize::try_from(variable)
                    .ok()
                    .and_then(|index| index.checked_add(1))
                    .ok_or(PolyhedronError::DimensionOverflow)
            })
    }
}

/// Failure to construct or project an exact polyhedral value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PolyhedronError {
    /// A dense dimension could not represent the requested variable identifier
    /// or extended projection space.
    #[error("polyhedral dimension exceeds this target's addressable index space")]
    DimensionOverflow,
    /// Concrete points did not share one dimension.
    #[error("polyhedral point {point} has dimension {actual}, expected {expected}")]
    PointDimension {
        /// Zero-based point index.
        point: usize,
        /// Required coordinate count.
        expected: usize,
        /// Observed coordinate count.
        actual: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
struct DenseConstraint {
    coefficients: Vec<Rational>,
    bound: Rational,
}

impl DenseConstraint {
    fn from_sparse(constraint: &LinearConstraint, dimension: usize) -> Self {
        let mut coefficients = vec![Rational::zero(); dimension];
        for (&variable, coefficient) in &constraint.coefficients {
            if let Ok(index) = usize::try_from(variable) {
                if let Some(slot) = coefficients.get_mut(index) {
                    *slot = coefficient.clone();
                }
            }
        }
        Self {
            coefficients,
            bound: constraint.bound.clone(),
        }
    }

    fn to_sparse(&self) -> LinearConstraint {
        LinearConstraint::new(
            self.coefficients
                .iter()
                .enumerate()
                .filter(|(_, coefficient)| !coefficient.is_zero())
                .filter_map(|(variable, coefficient)| {
                    u32::try_from(variable)
                        .ok()
                        .map(|variable| (variable, coefficient.clone()))
                }),
            self.bound.clone(),
        )
    }

    fn expanded(&self, dimension: usize) -> Self {
        let mut result = self.clone();
        result.coefficients.resize(dimension, Rational::zero());
        result
    }

    fn normalized(mut self) -> Self {
        let Some(first) = self
            .coefficients
            .iter()
            .find(|coefficient| !coefficient.is_zero())
        else {
            return self;
        };
        let scale = first.abs();
        for coefficient in &mut self.coefficients {
            *coefficient /= &scale;
        }
        self.bound /= scale;
        self
    }
}

struct Normalized {
    constraints: Vec<DenseConstraint>,
    infeasible: bool,
}

enum Maximum {
    Empty,
    Finite(Rational),
    Unbounded,
}

/// A canonical closed convex rational polyhedron.
#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Polyhedron {
    dimension: usize,
    empty: bool,
    constraints: Vec<DenseConstraint>,
}

impl fmt::Debug for Polyhedron {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Polyhedron")
            .field("dimension", &self.dimension)
            .field("empty", &self.empty)
            .field("constraints", &self.constraints)
            .finish()
    }
}

impl PartialEq for Polyhedron {
    fn eq(&self, other: &Self) -> bool {
        self.semantic_leq(other) && other.semantic_leq(self)
    }
}

impl Eq for Polyhedron {}

impl Default for Polyhedron {
    fn default() -> Self {
        Self::top(0)
    }
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for Polyhedron {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Encoded {
            dimension: usize,
            empty: bool,
            constraints: Vec<DenseConstraint>,
        }

        let encoded = Encoded::deserialize(deserializer)?;
        if encoded
            .constraints
            .iter()
            .any(|constraint| constraint.coefficients.len() != encoded.dimension)
        {
            return Err(D::Error::custom(
                "polyhedral constraint width disagrees with its dimension",
            ));
        }
        if encoded.empty {
            Ok(Self::bottom(encoded.dimension))
        } else {
            Ok(Self::from_dense(encoded.dimension, encoded.constraints))
        }
    }
}

impl Polyhedron {
    /// Constructs the unconstrained polyhedron in `dimension` variables.
    pub const fn top(dimension: usize) -> Self {
        Self {
            dimension,
            empty: false,
            constraints: Vec::new(),
        }
    }

    /// Constructs the empty polyhedron in `dimension` variables.
    pub const fn bottom(dimension: usize) -> Self {
        Self {
            dimension,
            empty: true,
            constraints: Vec::new(),
        }
    }

    /// Constructs and canonicalizes an intersection of half-spaces.
    ///
    /// The explicit dimension grows to admit every referenced variable, making
    /// out-of-domain sparse identifiers total.
    pub fn from_constraints(
        dimension: usize,
        constraints: impl IntoIterator<Item = LinearConstraint>,
    ) -> Result<Self, PolyhedronError> {
        let constraints: Vec<_> = constraints.into_iter().collect();
        let mut required = dimension;
        for constraint in &constraints {
            required = required.max(constraint.required_dimension()?);
        }
        Ok(Self::from_dense(
            required,
            constraints
                .iter()
                .map(|constraint| DenseConstraint::from_sparse(constraint, required))
                .collect(),
        ))
    }

    /// Constructs the singleton polyhedron containing one rational point.
    pub fn point(point: &[Rational]) -> Result<Self, PolyhedronError> {
        let mut constraints = Vec::new();
        for (variable, value) in point.iter().enumerate() {
            let variable =
                u32::try_from(variable).map_err(|_| PolyhedronError::DimensionOverflow)?;
            constraints.push(LinearConstraint::new(
                [(variable, Rational::one())],
                value.clone(),
            ));
            constraints.push(LinearConstraint::new(
                [(variable, -Rational::one())],
                -value,
            ));
        }
        Self::from_constraints(point.len(), constraints)
    }

    /// Constructs the singleton polyhedron containing one integer point.
    pub fn integer_point(point: &[i128]) -> Result<Self, PolyhedronError> {
        let rational: Vec<_> = point.iter().copied().map(integer).collect();
        Self::point(&rational)
    }

    /// Constructs the least closed convex polyhedron containing every rational
    /// point. The empty set maps to bottom.
    pub fn from_points(points: &[Vec<Rational>]) -> Result<Self, PolyhedronError> {
        let Some(first) = points.first() else {
            return Ok(Self::bottom(0));
        };
        for (point, values) in points.iter().enumerate() {
            if values.len() != first.len() {
                return Err(PolyhedronError::PointDimension {
                    point,
                    expected: first.len(),
                    actual: values.len(),
                });
            }
        }
        let mut result = Self::point(first)?;
        for point in &points[1..] {
            result = result.closed_convex_hull(&Self::point(point)?)?;
        }
        Ok(result)
    }

    /// Constructs the least closed convex polyhedron containing every integer
    /// point.
    pub fn from_integer_points(points: &[Vec<i128>]) -> Result<Self, PolyhedronError> {
        let rational: Vec<Vec<_>> = points
            .iter()
            .map(|point| point.iter().copied().map(integer).collect())
            .collect();
        Self::from_points(&rational)
    }

    /// Returns the number of dense variables in the represented space.
    pub const fn dimension(&self) -> usize {
        self.dimension
    }

    /// Returns whether this element represents no concrete state.
    pub const fn is_bottom(&self) -> bool {
        self.empty
    }

    /// Returns whether this element imposes no half-space.
    pub fn is_top(&self) -> bool {
        !self.empty && self.constraints.is_empty()
    }

    /// Returns a canonical sparse snapshot of the irredundant half-spaces.
    pub fn constraints(&self) -> Vec<LinearConstraint> {
        self.constraints
            .iter()
            .map(DenseConstraint::to_sparse)
            .collect()
    }

    /// Returns whether an exact rational point belongs to this polyhedron.
    ///
    /// Dimension mismatch returns `false`.
    pub fn contains(&self, point: &[Rational]) -> bool {
        !self.empty
            && point.len() == self.dimension
            && self.constraints.iter().all(|constraint| {
                constraint
                    .coefficients
                    .iter()
                    .zip(point)
                    .fold(Rational::zero(), |sum, (coefficient, value)| {
                        sum + coefficient * value
                    })
                    <= constraint.bound.clone()
            })
    }

    /// Returns whether an integer point belongs to this polyhedron.
    pub fn contains_integers(&self, point: &[i128]) -> bool {
        let rational: Vec<_> = point.iter().copied().map(integer).collect();
        self.contains(&rational)
    }

    /// Intersects this polyhedron with one half-space.
    pub fn assume(&mut self, constraint: LinearConstraint) -> Result<bool, PolyhedronError> {
        let other = Self::from_constraints(self.dimension, [constraint])?;
        let next = self.intersection(&other);
        let changed = next != *self;
        *self = next;
        Ok(changed)
    }

    /// Computes exact intersection.
    pub fn intersection(&self, other: &Self) -> Self {
        let dimension = self.dimension.max(other.dimension);
        if self.empty || other.empty {
            return Self::bottom(dimension);
        }
        let left = self.expanded(dimension);
        let right = other.expanded(dimension);
        let mut constraints = Vec::new();
        constraints.extend(left.constraints);
        constraints.extend(right.constraints);
        Self::from_dense(dimension, constraints)
    }

    /// Computes the exact closed convex hull used as lattice join.
    pub fn closed_convex_hull(&self, other: &Self) -> Result<Self, PolyhedronError> {
        let dimension = self.dimension.max(other.dimension);
        if self.empty {
            return Ok(other.expanded(dimension));
        }
        if other.empty {
            return Ok(self.expanded(dimension));
        }
        if self.is_top() || other.is_top() {
            return Ok(Self::top(dimension));
        }
        let left = self.expanded(dimension);
        let right = other.expanded(dimension);
        let extended = dimension
            .checked_mul(3)
            .and_then(|value| value.checked_add(1))
            .ok_or(PolyhedronError::DimensionOverflow)?;
        let lambda = extended - 1;
        let mut constraints = Vec::new();

        for variable in 0..dimension {
            let mut positive = vec![Rational::zero(); extended];
            positive[variable] = Rational::one();
            positive[dimension + variable] = -Rational::one();
            positive[2 * dimension + variable] = -Rational::one();
            constraints.push(DenseConstraint {
                coefficients: positive.clone(),
                bound: Rational::zero(),
            });
            for coefficient in &mut positive {
                *coefficient = -coefficient.clone();
            }
            constraints.push(DenseConstraint {
                coefficients: positive,
                bound: Rational::zero(),
            });
        }

        for constraint in left.constraints {
            let mut coefficients = vec![Rational::zero(); extended];
            for (variable, coefficient) in constraint.coefficients.into_iter().enumerate() {
                coefficients[dimension + variable] = coefficient;
            }
            coefficients[lambda] = -constraint.bound;
            constraints.push(DenseConstraint {
                coefficients,
                bound: Rational::zero(),
            });
        }
        for constraint in right.constraints {
            let mut coefficients = vec![Rational::zero(); extended];
            for (variable, coefficient) in constraint.coefficients.into_iter().enumerate() {
                coefficients[2 * dimension + variable] = coefficient;
            }
            coefficients[lambda] = constraint.bound.clone();
            constraints.push(DenseConstraint {
                coefficients,
                bound: constraint.bound,
            });
        }

        let mut lambda_lower = vec![Rational::zero(); extended];
        lambda_lower[lambda] = -Rational::one();
        constraints.push(DenseConstraint {
            coefficients: lambda_lower,
            bound: Rational::zero(),
        });
        let mut lambda_upper = vec![Rational::zero(); extended];
        lambda_upper[lambda] = Rational::one();
        constraints.push(DenseConstraint {
            coefficients: lambda_upper,
            bound: Rational::one(),
        });

        let auxiliaries: Vec<_> = (dimension..extended).collect();
        let projected = eliminate_variables(constraints, &auxiliaries);
        if projected.infeasible {
            return Ok(Self::bottom(dimension));
        }
        Ok(Self::from_dense(
            dimension,
            projected
                .constraints
                .into_iter()
                .map(|constraint| DenseConstraint {
                    coefficients: constraint.coefficients[..dimension].to_vec(),
                    bound: constraint.bound,
                })
                .collect(),
        ))
    }

    /// Applies Cousot–Halbwachs widening.
    ///
    /// An old half-space is retained exactly when it contains the incoming
    /// polyhedron; every moving facet is dropped. Repetition therefore
    /// stabilizes after at most the old finite facet count.
    pub fn widen(&mut self, incoming: &Self) -> bool {
        if self.empty {
            let dimension = self.dimension.max(incoming.dimension);
            let next = incoming.expanded(dimension);
            let changed = next != *self;
            *self = next;
            return changed;
        }
        if incoming.empty {
            return false;
        }
        let dimension = self.dimension.max(incoming.dimension);
        let before = self.clone();
        let old = self.expanded(dimension);
        let next = incoming.expanded(dimension);
        let retained = old
            .constraints
            .into_iter()
            .filter(|constraint| implies(&next.constraints, dimension, constraint))
            .collect();
        *self = Self::from_dense(dimension, retained);
        *self != before
    }

    /// Narrows by exact intersection with the incoming descending iterate.
    pub fn narrow(&mut self, incoming: &Self) -> bool {
        let before = self.clone();
        *self = self.intersection(incoming);
        *self != before
    }

    fn from_dense(dimension: usize, constraints: Vec<DenseConstraint>) -> Self {
        let canonical = canonicalize(dimension, constraints);
        if canonical.infeasible {
            Self::bottom(dimension)
        } else {
            Self {
                dimension,
                empty: false,
                constraints: canonical.constraints,
            }
        }
    }

    fn expanded(&self, dimension: usize) -> Self {
        if dimension <= self.dimension {
            return self.clone();
        }
        if self.empty {
            return Self::bottom(dimension);
        }
        Self {
            dimension,
            empty: false,
            constraints: self
                .constraints
                .iter()
                .map(|constraint| constraint.expanded(dimension))
                .collect(),
        }
    }

    fn semantic_leq(&self, other: &Self) -> bool {
        if self.empty {
            return true;
        }
        if other.empty {
            return false;
        }
        let dimension = self.dimension.max(other.dimension);
        let left = self.expanded(dimension);
        let right = other.expanded(dimension);
        right
            .constraints
            .iter()
            .all(|constraint| implies(&left.constraints, dimension, constraint))
    }
}

impl Lattice for Polyhedron {
    fn join(&mut self, other: &Self) -> bool {
        let before = self.clone();
        let dimension = self.dimension.max(other.dimension);
        *self = self
            .closed_convex_hull(other)
            .unwrap_or_else(|_| Self::top(dimension));
        *self != before
    }

    fn leq(&self, other: &Self) -> bool {
        self.semantic_leq(other)
    }
}

fn integer(value: i128) -> Rational {
    Rational::from_integer(BigInt::from(value))
}

fn normalize(constraints: Vec<DenseConstraint>) -> Normalized {
    let mut unique = BTreeMap::<Vec<Rational>, Rational>::new();
    for constraint in constraints {
        let constraint = constraint.normalized();
        if constraint.coefficients.iter().all(Rational::is_zero) {
            if constraint.bound.is_negative() {
                return Normalized {
                    constraints: Vec::new(),
                    infeasible: true,
                };
            }
            continue;
        }
        match unique.entry(constraint.coefficients) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(constraint.bound);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if constraint.bound < *entry.get() {
                    entry.insert(constraint.bound);
                }
            }
        }
    }
    Normalized {
        constraints: unique
            .into_iter()
            .map(|(coefficients, bound)| DenseConstraint {
                coefficients,
                bound,
            })
            .collect(),
        infeasible: false,
    }
}

fn eliminate_one(constraints: Vec<DenseConstraint>, variable: usize) -> Normalized {
    let mut positive = Vec::new();
    let mut negative = Vec::new();
    let mut zero = Vec::new();
    for constraint in constraints {
        match constraint.coefficients[variable].cmp(&Rational::zero()) {
            std::cmp::Ordering::Greater => positive.push(constraint),
            std::cmp::Ordering::Less => negative.push(constraint),
            std::cmp::Ordering::Equal => zero.push(constraint),
        }
    }
    let pair_count = positive.len().saturating_mul(negative.len());
    let mut projected = Vec::with_capacity(zero.len().saturating_add(pair_count));
    projected.extend(zero);
    for upper in &positive {
        let upper_scale = upper.coefficients[variable].clone();
        for lower in &negative {
            let lower_scale = -lower.coefficients[variable].clone();
            let mut coefficients = Vec::with_capacity(upper.coefficients.len());
            for (upper_coefficient, lower_coefficient) in
                upper.coefficients.iter().zip(&lower.coefficients)
            {
                coefficients
                    .push(upper_coefficient * &lower_scale + lower_coefficient * &upper_scale);
            }
            coefficients[variable] = Rational::zero();
            projected.push(DenseConstraint {
                coefficients,
                bound: &upper.bound * &lower_scale + &lower.bound * &upper_scale,
            });
        }
    }
    normalize(projected)
}

fn eliminate_variables(constraints: Vec<DenseConstraint>, variables: &[usize]) -> Normalized {
    let mut normalized = normalize(constraints);
    let mut remaining: Vec<_> = variables.to_vec();
    remaining.sort_unstable();
    remaining.dedup();
    while !normalized.infeasible && !remaining.is_empty() {
        let mut selected = 0usize;
        let mut selected_cost = usize::MAX;
        for (position, &variable) in remaining.iter().enumerate() {
            let mut positive = 0usize;
            let mut negative = 0usize;
            for constraint in &normalized.constraints {
                match constraint.coefficients[variable].cmp(&Rational::zero()) {
                    std::cmp::Ordering::Greater => positive += 1,
                    std::cmp::Ordering::Less => negative += 1,
                    std::cmp::Ordering::Equal => {}
                }
            }
            let cost = positive.saturating_mul(negative);
            if cost < selected_cost {
                selected = position;
                selected_cost = cost;
            }
        }
        let variable = remaining.remove(selected);
        normalized = eliminate_one(normalized.constraints, variable);
    }
    normalized
}

fn feasible(constraints: &[DenseConstraint], dimension: usize) -> bool {
    let variables: Vec<_> = (0..dimension).collect();
    !eliminate_variables(constraints.to_vec(), &variables).infeasible
}

fn maximize(
    constraints: &[DenseConstraint],
    dimension: usize,
    coefficients: &[Rational],
) -> Maximum {
    let Some(extended) = dimension.checked_add(1) else {
        return Maximum::Unbounded;
    };
    let target = dimension;
    let mut system: Vec<_> = constraints
        .iter()
        .map(|constraint| constraint.expanded(extended))
        .collect();

    let mut positive = vec![Rational::zero(); extended];
    let mut negative = vec![Rational::zero(); extended];
    for variable in 0..dimension {
        positive[variable] = -coefficients[variable].clone();
        negative[variable] = coefficients[variable].clone();
    }
    positive[target] = Rational::one();
    negative[target] = -Rational::one();
    system.push(DenseConstraint {
        coefficients: positive,
        bound: Rational::zero(),
    });
    system.push(DenseConstraint {
        coefficients: negative,
        bound: Rational::zero(),
    });

    let variables: Vec<_> = (0..dimension).collect();
    let projected = eliminate_variables(system, &variables);
    if projected.infeasible {
        return Maximum::Empty;
    }
    let mut upper = None::<Rational>;
    for constraint in projected.constraints {
        let coefficient = &constraint.coefficients[target];
        if coefficient.is_positive() {
            let candidate = constraint.bound / coefficient;
            if upper.as_ref().is_none_or(|current| candidate < *current) {
                upper = Some(candidate);
            }
        }
    }
    upper.map_or(Maximum::Unbounded, Maximum::Finite)
}

fn implies(constraints: &[DenseConstraint], dimension: usize, target: &DenseConstraint) -> bool {
    match maximize(constraints, dimension, &target.coefficients) {
        Maximum::Empty => true,
        Maximum::Finite(maximum) => maximum <= target.bound.clone(),
        Maximum::Unbounded => false,
    }
}

fn canonicalize(dimension: usize, constraints: Vec<DenseConstraint>) -> Normalized {
    let mut normalized = normalize(constraints);
    if normalized.infeasible || !feasible(&normalized.constraints, dimension) {
        return Normalized {
            constraints: Vec::new(),
            infeasible: true,
        };
    }
    let mut index = 0usize;
    while index < normalized.constraints.len() {
        let mut others = Vec::with_capacity(normalized.constraints.len() - 1);
        others.extend_from_slice(&normalized.constraints[..index]);
        others.extend_from_slice(&normalized.constraints[index + 1..]);
        if implies(&others, dimension, &normalized.constraints[index]) {
            normalized.constraints.remove(index);
        } else {
            index += 1;
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn constraint(coefficients: &[(u32, i128)], bound: i128) -> LinearConstraint {
        LinearConstraint::from_integers(coefficients.iter().copied(), bound)
    }

    fn fraction(numerator: i128, denominator: i128) -> Rational {
        Rational::new(BigInt::from(numerator), BigInt::from(denominator))
    }

    #[test]
    fn triangle_membership_intersection_and_infeasibility_are_exact() {
        let triangle = Polyhedron::from_constraints(
            2,
            [
                constraint(&[(0, -1)], 0),
                constraint(&[(1, -1)], 0),
                constraint(&[(0, 1), (1, 1)], 4),
            ],
        )
        .expect("construct rational triangle");
        assert!(triangle.contains_integers(&[1, 2]));
        assert!(!triangle.contains_integers(&[3, 3]));
        assert!(!triangle.contains_integers(&[1]));

        let mut empty = triangle.clone();
        empty
            .assume(constraint(&[(0, 1)], -1))
            .expect("intersect contradictory x bound");
        assert!(empty.is_bottom());
        assert!(empty.leq(&triangle));
    }

    #[test]
    fn closed_convex_hull_preserves_the_exact_affine_relation() {
        let left = Polyhedron::integer_point(&[0, 0]).expect("left point");
        let right = Polyhedron::integer_point(&[2, 2]).expect("right point");
        let hull = left
            .closed_convex_hull(&right)
            .expect("project exact closed hull");
        assert!(hull.contains_integers(&[0, 0]));
        assert!(hull.contains_integers(&[1, 1]));
        assert!(hull.contains_integers(&[2, 2]));
        assert!(!hull.contains_integers(&[1, 0]));
        assert!(left.leq(&hull));
        assert!(right.leq(&hull));
    }

    #[test]
    fn rational_hulls_and_mixed_dimension_bottom_are_exact() {
        let left = Polyhedron::point(&[fraction(1, 3)]).expect("one third point");
        let right = Polyhedron::point(&[fraction(2, 3)]).expect("two thirds point");
        let hull = left
            .closed_convex_hull(&right)
            .expect("exact fractional hull");
        assert!(hull.contains(&[fraction(1, 2)]));
        assert!(!hull.contains(&[fraction(3, 4)]));

        let expanded = Polyhedron::bottom(3)
            .closed_convex_hull(&Polyhedron::integer_point(&[2]).expect("integer point"))
            .expect("bottom identity in joined dimension");
        assert_eq!(expanded.dimension(), 3);
        assert!(expanded.contains_integers(&[2, -17, 99]));
        assert!(!expanded.contains_integers(&[3, -17, 99]));
    }

    #[test]
    fn widening_drops_only_moving_facets_and_then_stabilizes() {
        let zero = Polyhedron::integer_point(&[0]).expect("zero point");
        let zero_to_one =
            Polyhedron::from_constraints(1, [constraint(&[(0, -1)], 0), constraint(&[(0, 1)], 1)])
                .expect("zero to one");
        let zero_to_two =
            Polyhedron::from_constraints(1, [constraint(&[(0, -1)], 0), constraint(&[(0, 1)], 2)])
                .expect("zero to two");

        let mut widened = zero;
        assert!(widened.widen(&zero_to_one));
        assert!(widened.contains_integers(&[i128::MAX]));
        assert!(!widened.contains_integers(&[-1]));
        assert!(!widened.widen(&zero_to_two));

        assert!(widened.narrow(&zero_to_two));
        assert!(widened.contains_integers(&[2]));
        assert!(!widened.contains_integers(&[3]));
    }

    #[test]
    fn out_of_domain_variables_grow_and_mismatched_points_are_total() {
        let polyhedron = Polyhedron::from_constraints(0, [constraint(&[(7, 1)], 9)])
            .expect("grow polyhedron to variable seven");
        assert_eq!(polyhedron.dimension(), 8);
        assert!(!polyhedron.contains_integers(&[0; 7]));
        assert!(polyhedron.contains_integers(&[0; 8]));
        assert!(matches!(
            Polyhedron::from_integer_points(&[vec![0], vec![0, 1]]),
            Err(PolyhedronError::PointDimension { .. })
        ));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(96))]

        #[test]
        fn generated_point_hulls_are_sound_deterministic_and_lattice_ordered(
            raw in prop::collection::vec(prop::collection::vec(-6i8..=6, 2), 1..4),
        ) {
            let points: Vec<Vec<i128>> = raw
                .into_iter()
                .map(|point| point.into_iter().map(i128::from).collect())
                .collect();
            let mut reversed_points = points.clone();
            reversed_points.reverse();
            let abstraction =
                Polyhedron::from_integer_points(&points).expect("point hull abstraction");
            let reversed = Polyhedron::from_integer_points(&reversed_points)
                .expect("reversed point hull abstraction");
            prop_assert_eq!(&abstraction, &reversed);
            for point in &points {
                prop_assert!(abstraction.contains_integers(point));
            }

            let split = points.len().div_ceil(2);
            let left =
                Polyhedron::from_integer_points(&points[..split]).expect("left point hull");
            let right = if split == points.len() {
                Polyhedron::bottom(2)
            } else {
                Polyhedron::from_integer_points(&points[split..]).expect("right point hull")
            };
            let mut left_join_right = left.clone();
            left_join_right.join(&right);
            let mut right_join_left = right.clone();
            right_join_left.join(&left);
            prop_assert_eq!(&left_join_right, &right_join_left);
            prop_assert!(left.leq(&left_join_right));
            prop_assert!(right.leq(&left_join_right));
            for point in &points {
                prop_assert!(left_join_right.contains_integers(point));
            }
        }

        #[test]
        fn generated_box_intersection_matches_concrete_conjunction(
            first in (-6i8..=6, -6i8..=6, -6i8..=6, -6i8..=6),
            second in (-6i8..=6, -6i8..=6, -6i8..=6, -6i8..=6),
        ) {
            let box_from = |raw: (i8, i8, i8, i8)| {
                let x_lo = i128::from(raw.0.min(raw.1));
                let x_hi = i128::from(raw.0.max(raw.1));
                let y_lo = i128::from(raw.2.min(raw.3));
                let y_hi = i128::from(raw.2.max(raw.3));
                Polyhedron::from_constraints(
                    2,
                    [
                        constraint(&[(0, -1)], -x_lo),
                        constraint(&[(0, 1)], x_hi),
                        constraint(&[(1, -1)], -y_lo),
                        constraint(&[(1, 1)], y_hi),
                    ],
                )
                .expect("bounded rational box")
            };
            let left = box_from(first);
            let right = box_from(second);
            let intersection = left.intersection(&right);
            let reverse = right.intersection(&left);
            prop_assert_eq!(&intersection, &reverse);
            prop_assert!(intersection.leq(&left));
            prop_assert!(intersection.leq(&right));
            for x in -7i128..=7 {
                for y in -7i128..=7 {
                    let point = [x, y];
                    prop_assert_eq!(
                        intersection.contains_integers(&point),
                        left.contains_integers(&point) && right.contains_integers(&point)
                    );
                }
            }
        }
    }

    #[test]
    fn projection_and_redundancy_lifecycle_is_stack_safe() {
        std::thread::Builder::new()
            .name("polyhedron-small-stack".into())
            .stack_size(256 * 1024)
            .spawn(|| {
                let dimension = 24u32;
                let mut constraints = Vec::with_capacity(dimension as usize * 2);
                for variable in 0..dimension {
                    constraints.push(constraint(&[(variable, -1)], 0));
                    constraints.push(constraint(&[(variable, 1)], 10));
                }
                let box_polyhedron = Polyhedron::from_constraints(dimension as usize, constraints)
                    .expect("construct high-dimensional rational box");
                assert!(box_polyhedron.contains_integers(&vec![5; dimension as usize]));
                assert!(!box_polyhedron.contains_integers(&vec![11; dimension as usize]));
                assert!(box_polyhedron.leq(&Polyhedron::top(dimension as usize)));
            })
            .expect("spawn polyhedron small-stack lifecycle")
            .join()
            .expect("join polyhedron small-stack lifecycle");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trip_recanonicalizes_the_polyhedron() {
        let polyhedron = Polyhedron::from_constraints(
            2,
            [
                constraint(&[(0, -1)], 0),
                constraint(&[(1, -1)], 0),
                constraint(&[(0, 1), (1, 1)], 4),
            ],
        )
        .expect("construct serde polyhedron");
        let encoded = serde_json::to_string(&polyhedron).expect("serialize polyhedron");
        let decoded: Polyhedron = serde_json::from_str(&encoded).expect("deserialize polyhedron");
        assert_eq!(decoded, polyhedron);
    }
}
