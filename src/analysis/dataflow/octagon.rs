//! Feature-free integer octagons over constraints of the form
//! `±x ± y <= c`.
//!
//! The domain stores a coherent, integer-tight difference-bound matrix (DBM)
//! over two signed forms per variable. Lattice operations scan the dense matrix
//! iteratively; closure uses heap-resident rows and never maps variable or graph
//! depth to native call depth.

use std::fmt;

use super::{reduced_product::Reduction, Bound as IntervalBound, Interval, IntervalEnv, Lattice};

#[cfg(feature = "serde")]
use serde::{de::Error as _, Deserialize, Deserializer, Serialize};

/// Sign of one variable in an octagonal expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum OctagonSign {
    /// `+x`.
    Positive,
    /// `-x`.
    Negative,
}

impl OctagonSign {
    const fn flipped(self) -> Self {
        match self {
            Self::Positive => Self::Negative,
            Self::Negative => Self::Positive,
        }
    }
}

/// One signed variable term in an octagonal constraint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct OctagonTerm {
    /// Dense variable identifier.
    pub variable: u32,
    /// Coefficient, restricted to `+1` or `-1`.
    pub sign: OctagonSign,
}

impl OctagonTerm {
    /// Constructs `+x_variable`.
    pub const fn positive(variable: u32) -> Self {
        Self {
            variable,
            sign: OctagonSign::Positive,
        }
    }

    /// Constructs `-x_variable`.
    pub const fn negative(variable: u32) -> Self {
        Self {
            variable,
            sign: OctagonSign::Negative,
        }
    }

    const fn flipped(self) -> Self {
        Self {
            variable: self.variable,
            sign: self.sign.flipped(),
        }
    }
}

/// One integer octagonal inequality.
///
/// A missing `right` term denotes `left <= bound`. Otherwise the constraint is
/// `left + right <= bound`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct OctagonConstraint {
    /// First signed term.
    pub left: OctagonTerm,
    /// Optional second signed term.
    pub right: Option<OctagonTerm>,
    /// Inclusive integer upper bound.
    pub bound: i128,
}

impl OctagonConstraint {
    /// Constructs `term <= bound`.
    pub const fn unary(term: OctagonTerm, bound: i128) -> Self {
        Self {
            left: term,
            right: None,
            bound,
        }
    }

    /// Constructs `left + right <= bound`.
    pub const fn binary(left: OctagonTerm, right: OctagonTerm, bound: i128) -> Self {
        Self {
            left,
            right: Some(right),
            bound,
        }
    }
}

/// One DBM entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum OctagonBound {
    /// A finite inclusive difference bound.
    Finite(i128),
    /// No represented upper bound.
    Infinity,
}

/// Failure to represent an exact requested octagonal operation.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OctagonError {
    /// Doubling a public unary bound exceeded `i128`.
    #[error("octagon unary bound {bound} cannot be doubled exactly")]
    UnaryBoundOverflow {
        /// Bound that could not be encoded.
        bound: i128,
    },
    /// Exact closure would require a finite value below `i128::MIN`.
    #[error("octagon closure underflowed its exact i128 DBM representation")]
    ClosureUnderflow,
    /// Concrete points did not share one dimension.
    #[error("octagon point {point} has dimension {actual}, expected {expected}")]
    PointDimension {
        /// Zero-based point index.
        point: usize,
        /// Required coordinate count.
        expected: usize,
        /// Observed coordinate count.
        actual: usize,
    },
}

/// A canonical integer octagon represented by a coherent, tight DBM.
#[derive(Clone)]
#[cfg_attr(feature = "serde", derive(Serialize))]
pub struct Octagon {
    variables: usize,
    empty: bool,
    matrix: Vec<OctagonBound>,
}

impl fmt::Debug for Octagon {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Octagon")
            .field("variables", &self.variables)
            .field("empty", &self.empty)
            .field("matrix", &self.matrix)
            .finish()
    }
}

impl PartialEq for Octagon {
    fn eq(&self, other: &Self) -> bool {
        self.semantic_leq(other) && other.semantic_leq(self)
    }
}

impl Eq for Octagon {}

impl Default for Octagon {
    fn default() -> Self {
        Self::top(0)
    }
}

#[cfg(feature = "serde")]
impl<'de> Deserialize<'de> for Octagon {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Encoded {
            variables: usize,
            empty: bool,
            matrix: Vec<OctagonBound>,
        }

        let encoded = Encoded::deserialize(deserializer)?;
        let signed = encoded
            .variables
            .checked_mul(2)
            .ok_or_else(|| D::Error::custom("octagon signed dimension overflow"))?;
        let expected = signed
            .checked_mul(signed)
            .ok_or_else(|| D::Error::custom("octagon matrix dimension overflow"))?;
        if encoded.matrix.len() != expected {
            return Err(D::Error::custom(
                "octagon matrix length disagrees with its variable count",
            ));
        }

        let mut octagon = Self {
            variables: encoded.variables,
            empty: encoded.empty,
            matrix: encoded.matrix,
        };
        if octagon.empty {
            octagon.matrix.fill(OctagonBound::Infinity);
            for index in 0..signed {
                octagon.matrix[index * signed + index] = OctagonBound::Finite(0);
            }
        } else {
            octagon.close().map_err(D::Error::custom)?;
        }
        Ok(octagon)
    }
}

impl Octagon {
    /// Constructs the unconstrained element over `variables` dense variables.
    pub fn top(variables: usize) -> Self {
        let signed = variables.saturating_mul(2);
        let mut matrix = vec![OctagonBound::Infinity; signed.saturating_mul(signed)];
        for index in 0..signed {
            matrix[index * signed + index] = OctagonBound::Finite(0);
        }
        Self {
            variables,
            empty: false,
            matrix,
        }
    }

    /// Constructs the empty element over `variables` dense variables.
    pub fn bottom(variables: usize) -> Self {
        let mut result = Self::top(variables);
        result.empty = true;
        result
    }

    /// Returns the number of dense variables explicitly admitted so far.
    pub const fn variable_count(&self) -> usize {
        self.variables
    }

    /// Returns whether this element represents no concrete state.
    pub const fn is_bottom(&self) -> bool {
        self.empty
    }

    /// Returns whether this element imposes no constraint.
    pub fn is_top(&self) -> bool {
        if self.empty {
            return false;
        }
        let signed = self.signed_width();
        (0..signed).all(|row| {
            (0..signed).all(|column| {
                self.matrix[self.slot(row, column)]
                    == if row == column {
                        OctagonBound::Finite(0)
                    } else {
                        OctagonBound::Infinity
                    }
            })
        })
    }

    /// Intersects this element with one inequality.
    ///
    /// Variable identifiers beyond the current dense dimension grow the DBM
    /// with unconstrained rows, so out-of-domain terms are total rather than
    /// indexing failures.
    pub fn assume(&mut self, constraint: OctagonConstraint) -> Result<bool, OctagonError> {
        self.assume_all([constraint])
    }

    /// Intersects this element with all inequalities and closes once.
    pub fn assume_all(
        &mut self,
        constraints: impl IntoIterator<Item = OctagonConstraint>,
    ) -> Result<bool, OctagonError> {
        if self.empty {
            return Ok(false);
        }
        let constraints: Vec<_> = constraints.into_iter().collect();
        let required = constraints
            .iter()
            .flat_map(|constraint| {
                std::iter::once(constraint.left.variable)
                    .chain(constraint.right.map(|term| term.variable))
            })
            .max()
            .and_then(|variable| usize::try_from(variable).ok())
            .and_then(|variable| variable.checked_add(1))
            .unwrap_or(self.variables)
            .max(self.variables);

        let before = self.clone();
        let mut candidate = self.expanded(required);
        for constraint in constraints {
            candidate.insert_raw(constraint)?;
            if candidate.empty {
                break;
            }
        }
        if !candidate.empty {
            candidate.close()?;
        }
        let changed = candidate != before;
        *self = candidate;
        Ok(changed)
    }

    /// Returns the strongest represented bound for one expression.
    pub fn implied_bound(
        &self,
        left: OctagonTerm,
        right: Option<OctagonTerm>,
    ) -> Option<OctagonBound> {
        if self.empty {
            return None;
        }
        let required = left.variable.max(right.map_or(0, |term| term.variable));
        if usize::try_from(required)
            .ok()
            .is_none_or(|variable| variable >= self.variables)
        {
            return Some(OctagonBound::Infinity);
        }
        let source = self.term_index(left);
        let target = match right {
            Some(term) => self.term_index(term.flipped()),
            None => source ^ 1,
        };
        Some(match (right, self.matrix[self.slot(source, target)]) {
            (None, OctagonBound::Finite(bound)) => OctagonBound::Finite(bound.div_euclid(2)),
            (None, OctagonBound::Infinity) => OctagonBound::Infinity,
            (Some(_), bound) => bound,
        })
    }

    /// Projects one variable to its tight represented integer interval.
    ///
    /// An unadmitted variable is unconstrained. Bottom has no interval.
    pub fn interval(&self, variable: u32) -> Option<Interval> {
        if self.empty {
            return None;
        }
        let Some(variable_index) = usize::try_from(variable).ok() else {
            return Some(Interval::top());
        };
        if variable_index >= self.variables {
            return Some(Interval::top());
        }
        let positive = self.term_index(OctagonTerm::positive(variable));
        let negative = positive ^ 1;
        let upper = match self.matrix[self.slot(positive, negative)] {
            OctagonBound::Finite(bound) => IntervalBound::Finite(bound.div_euclid(2)),
            OctagonBound::Infinity => IntervalBound::PosInf,
        };
        let lower = match self.matrix[self.slot(negative, positive)] {
            OctagonBound::Finite(bound) => IntervalBound::Finite(-bound.div_euclid(2)),
            OctagonBound::Infinity => IntervalBound::NegInf,
        };
        Interval::new(lower, upper)
    }

    /// Returns whether one concrete integer point belongs to this octagon.
    ///
    /// Dimension mismatch returns `false`. Coordinates use `i64` so every
    /// signed difference is exactly representable in the internal `i128` DBM.
    pub fn contains(&self, point: &[i64]) -> bool {
        if self.empty || point.len() != self.variables {
            return false;
        }
        let signed = self.signed_width();
        for row in 0..signed {
            let row_value = signed_value(point, row);
            for column in 0..signed {
                let OctagonBound::Finite(bound) = self.matrix[self.slot(row, column)] else {
                    continue;
                };
                if row_value - signed_value(point, column) > bound {
                    return false;
                }
            }
        }
        true
    }

    /// Constructs the least octagon containing every supplied point.
    ///
    /// The empty point set maps to bottom. All points must have equal dimension.
    pub fn from_points(points: &[Vec<i64>]) -> Result<Self, OctagonError> {
        let Some(first) = points.first() else {
            return Ok(Self::bottom(0));
        };
        let variables = first.len();
        for (point, values) in points.iter().enumerate() {
            if values.len() != variables {
                return Err(OctagonError::PointDimension {
                    point,
                    expected: variables,
                    actual: values.len(),
                });
            }
        }
        let mut result = Self::top(variables);
        let signed = result.signed_width();
        for row in 0..signed {
            for column in 0..signed {
                let mut bound = signed_value(first, row) - signed_value(first, column);
                for point in &points[1..] {
                    bound = bound.max(signed_value(point, row) - signed_value(point, column));
                }
                let slot = result.slot(row, column);
                result.matrix[slot] = OctagonBound::Finite(bound);
            }
        }
        result.close()?;
        Ok(result)
    }

    /// Intersects two octagons and restores tight closure.
    pub fn meet(&self, other: &Self) -> Result<Self, OctagonError> {
        if self.empty || other.empty {
            return Ok(Self::bottom(self.variables.max(other.variables)));
        }
        let variables = self.variables.max(other.variables);
        let left = self.expanded(variables);
        let right = other.expanded(variables);
        let mut result = left;
        for (into, from) in result.matrix.iter_mut().zip(&right.matrix) {
            *into = (*into).min(*from);
        }
        result.close()?;
        Ok(result)
    }

    /// Applies the standard DBM widening.
    ///
    /// An old bound is retained only when the incoming bound does not move
    /// outward. Removed bounds become infinity; closure then recovers only
    /// consequences of the stable bounds.
    pub fn widen(&mut self, incoming: &Self) -> Result<bool, OctagonError> {
        if self.empty {
            let variables = self.variables.max(incoming.variables);
            let next = incoming.expanded(variables);
            let changed = next != *self;
            *self = next;
            return Ok(changed);
        }
        if incoming.empty {
            return Ok(false);
        }
        let variables = self.variables.max(incoming.variables);
        let before = self.clone();
        let old = self.expanded(variables);
        let next = incoming.expanded(variables);
        let mut widened = Self::top(variables);
        for index in 0..widened.matrix.len() {
            widened.matrix[index] = if next.matrix[index] <= old.matrix[index] {
                old.matrix[index]
            } else {
                OctagonBound::Infinity
            };
        }
        for diagonal in 0..widened.signed_width() {
            let slot = widened.slot(diagonal, diagonal);
            widened.matrix[slot] = OctagonBound::Finite(0);
        }
        widened.close()?;
        let changed = widened != before;
        *self = widened;
        Ok(changed)
    }

    /// Descends after widening by restoring incoming bounds only where the
    /// current DBM is infinite.
    pub fn narrow(&mut self, incoming: &Self) -> Result<bool, OctagonError> {
        if self.empty || incoming.empty {
            if incoming.empty && !self.empty {
                *self = Self::bottom(self.variables.max(incoming.variables));
                return Ok(true);
            }
            return Ok(false);
        }
        let variables = self.variables.max(incoming.variables);
        let before = self.clone();
        let old = self.expanded(variables);
        let next = incoming.expanded(variables);
        let mut narrowed = old.clone();
        for index in 0..narrowed.matrix.len() {
            if narrowed.matrix[index] == OctagonBound::Infinity {
                narrowed.matrix[index] = next.matrix[index];
            }
        }
        narrowed.close()?;
        let changed = narrowed != before;
        *self = narrowed;
        Ok(changed)
    }

    fn signed_width(&self) -> usize {
        self.variables * 2
    }

    fn slot(&self, row: usize, column: usize) -> usize {
        row * self.signed_width() + column
    }

    fn term_index(&self, term: OctagonTerm) -> usize {
        term.variable as usize * 2
            + match term.sign {
                OctagonSign::Positive => 0,
                OctagonSign::Negative => 1,
            }
    }

    fn expanded(&self, variables: usize) -> Self {
        if variables <= self.variables {
            return self.clone();
        }
        let mut result = if self.empty {
            Self::bottom(variables)
        } else {
            Self::top(variables)
        };
        let old_width = self.signed_width();
        let new_width = result.signed_width();
        for row in 0..old_width {
            for column in 0..old_width {
                result.matrix[row * new_width + column] = self.matrix[row * old_width + column];
            }
        }
        result
    }

    fn insert_raw(&mut self, constraint: OctagonConstraint) -> Result<(), OctagonError> {
        let left = self.term_index(constraint.left);
        let target = match constraint.right {
            Some(right) => self.term_index(right.flipped()),
            None => left ^ 1,
        };
        let bound = match constraint.right {
            Some(_) => constraint.bound,
            None => constraint
                .bound
                .checked_mul(2)
                .ok_or(OctagonError::UnaryBoundOverflow {
                    bound: constraint.bound,
                })?,
        };
        if left == target {
            if bound < 0 {
                self.empty = true;
            }
            return Ok(());
        }
        self.tighten(left, target, OctagonBound::Finite(bound));
        self.tighten(target ^ 1, left ^ 1, OctagonBound::Finite(bound));
        Ok(())
    }

    fn tighten(&mut self, row: usize, column: usize, bound: OctagonBound) {
        let slot = self.slot(row, column);
        self.matrix[slot] = self.matrix[slot].min(bound);
    }

    fn close(&mut self) -> Result<(), OctagonError> {
        if self.empty {
            return Ok(());
        }
        self.floyd_warshall()?;
        if self.has_negative_diagonal() {
            self.empty = true;
            return Ok(());
        }

        let signed = self.signed_width();
        for row in 0..signed {
            let column = row ^ 1;
            let slot = self.slot(row, column);
            if let OctagonBound::Finite(bound) = self.matrix[slot] {
                self.matrix[slot] = OctagonBound::Finite(bound.div_euclid(2) * 2);
            }
        }
        self.floyd_warshall()?;
        if self.has_negative_diagonal() {
            self.empty = true;
            return Ok(());
        }

        for row in 0..signed {
            for column in 0..signed {
                let left = self.matrix[self.slot(row, row ^ 1)];
                let right = self.matrix[self.slot(column ^ 1, column)];
                let candidate = half_sum(left, right)?;
                self.tighten(row, column, candidate);
            }
        }
        self.enforce_coherence();
        if self.has_negative_diagonal() {
            self.empty = true;
        }
        Ok(())
    }

    fn floyd_warshall(&mut self) -> Result<(), OctagonError> {
        let signed = self.signed_width();
        for pivot in 0..signed {
            for row in 0..signed {
                let left = self.matrix[self.slot(row, pivot)];
                if left == OctagonBound::Infinity {
                    continue;
                }
                for column in 0..signed {
                    let right = self.matrix[self.slot(pivot, column)];
                    let candidate = add_bounds(left, right)?;
                    self.tighten(row, column, candidate);
                }
            }
        }
        self.enforce_coherence();
        Ok(())
    }

    fn enforce_coherence(&mut self) {
        let signed = self.signed_width();
        for row in 0..signed {
            for column in 0..signed {
                let mirror_row = column ^ 1;
                let mirror_column = row ^ 1;
                let common = self.matrix[self.slot(row, column)]
                    .min(self.matrix[self.slot(mirror_row, mirror_column)]);
                let slot = self.slot(row, column);
                let mirror = self.slot(mirror_row, mirror_column);
                self.matrix[slot] = common;
                self.matrix[mirror] = common;
            }
        }
    }

    fn has_negative_diagonal(&self) -> bool {
        (0..self.signed_width()).any(|index| {
            matches!(
                self.matrix[self.slot(index, index)],
                OctagonBound::Finite(bound) if bound < 0
            )
        })
    }

    fn semantic_leq(&self, other: &Self) -> bool {
        if self.empty {
            return true;
        }
        if other.empty {
            return false;
        }
        let variables = self.variables.max(other.variables);
        let left = self.expanded(variables);
        let right = other.expanded(variables);
        left.matrix
            .iter()
            .zip(&right.matrix)
            .all(|(left, right)| left <= right)
    }
}

impl Lattice for Octagon {
    fn join(&mut self, other: &Self) -> bool {
        if self.empty {
            if other.empty {
                return false;
            }
            *self = other.expanded(self.variables.max(other.variables));
            return true;
        }
        if other.empty {
            return false;
        }
        let variables = self.variables.max(other.variables);
        let before = self.clone();
        let left = self.expanded(variables);
        let right = other.expanded(variables);
        let mut joined = left;
        for (into, from) in joined.matrix.iter_mut().zip(&right.matrix) {
            *into = (*into).max(*from);
        }
        let changed = joined != before;
        *self = joined;
        changed
    }

    fn leq(&self, other: &Self) -> bool {
        self.semantic_leq(other)
    }
}

fn add_bounds(left: OctagonBound, right: OctagonBound) -> Result<OctagonBound, OctagonError> {
    match (left, right) {
        (OctagonBound::Infinity, _) | (_, OctagonBound::Infinity) => Ok(OctagonBound::Infinity),
        (OctagonBound::Finite(left), OctagonBound::Finite(right)) => {
            match left.checked_add(right) {
                Some(sum) => Ok(OctagonBound::Finite(sum)),
                None if left.is_negative() && right.is_negative() => {
                    Err(OctagonError::ClosureUnderflow)
                }
                None => Ok(OctagonBound::Infinity),
            }
        }
    }
}

fn half_sum(left: OctagonBound, right: OctagonBound) -> Result<OctagonBound, OctagonError> {
    match (left, right) {
        (OctagonBound::Finite(left), OctagonBound::Finite(right)) => add_bounds(
            OctagonBound::Finite(left.div_euclid(2)),
            OctagonBound::Finite(right.div_euclid(2)),
        ),
        _ => Ok(OctagonBound::Infinity),
    }
}

fn signed_value(point: &[i64], signed: usize) -> i128 {
    let value = i128::from(point[signed / 2]);
    if signed & 1 == 0 {
        value
    } else {
        -value
    }
}

/// Reduction between the existing interval environment and an integer octagon.
///
/// Finite interval endpoints become unary octagonal constraints. Tight unary
/// consequences from the closed DBM are then intersected back into every
/// represented interval. Relational closure can therefore refine a marginal
/// interval before and after every CFG merge.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct IntervalOctagonReduction;

impl Reduction<IntervalEnv, Octagon> for IntervalOctagonReduction {
    fn reduce(intervals: &mut IntervalEnv, octagon: &mut Octagon) -> bool {
        let before_intervals = intervals.clone();
        let before_octagon = octagon.clone();
        if intervals.is_bottom() {
            *octagon = Octagon::bottom(octagon.variable_count());
            return *octagon != before_octagon;
        }
        if octagon.is_bottom() {
            *intervals = IntervalEnv::bottom();
            return *intervals != before_intervals;
        }

        let mut constraints = Vec::with_capacity(intervals.vars.len() * 2);
        for &(variable, interval) in &intervals.vars {
            if let IntervalBound::Finite(upper) = interval.hi {
                if upper.checked_mul(2).is_some() {
                    constraints.push(OctagonConstraint::unary(
                        OctagonTerm::positive(variable),
                        upper,
                    ));
                }
            }
            if let IntervalBound::Finite(lower) = interval.lo {
                if let Some(negated) = lower.checked_neg() {
                    if negated.checked_mul(2).is_some() {
                        constraints.push(OctagonConstraint::unary(
                            OctagonTerm::negative(variable),
                            negated,
                        ));
                    }
                }
            }
        }
        let mut refined_octagon = octagon.clone();
        if refined_octagon.assume_all(constraints).is_ok() {
            *octagon = refined_octagon;
        }
        if octagon.is_bottom() {
            *intervals = IntervalEnv::bottom();
            return *intervals != before_intervals || *octagon != before_octagon;
        }

        let current = intervals.vars.clone();
        for (variable, interval) in current {
            let Some(octagonal) = octagon.interval(variable) else {
                intervals.vars.clear();
                *octagon = Octagon::bottom(octagon.variable_count());
                break;
            };
            match interval.meet(octagonal) {
                Some(refined) => intervals.set(variable, refined),
                None => {
                    intervals.vars.clear();
                    *octagon = Octagon::bottom(octagon.variable_count());
                    break;
                }
            }
        }
        *intervals != before_intervals || *octagon != before_octagon
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use proptest::prelude::*;

    use super::*;
    use crate::analysis::dataflow::{ReducedProduct, SolverConfig};

    #[test]
    fn relational_constraints_are_tight_and_detect_contradictions() {
        let mut octagon = Octagon::top(2);
        octagon
            .assume_all([
                OctagonConstraint::binary(OctagonTerm::positive(0), OctagonTerm::negative(1), 3),
                OctagonConstraint::unary(OctagonTerm::negative(0), 0),
                OctagonConstraint::unary(OctagonTerm::positive(1), 10),
            ])
            .expect("close consistent octagonal constraints");
        assert!(octagon.contains(&[5, 3]));
        assert!(!octagon.contains(&[10, 0]));
        assert_eq!(
            octagon.interval(0),
            Interval::new(IntervalBound::Finite(0), IntervalBound::Finite(13))
        );

        octagon
            .assume(OctagonConstraint::unary(OctagonTerm::positive(0), -1))
            .expect("close contradictory octagonal constraint");
        assert!(octagon.is_bottom());
        assert!(!octagon.contains(&[0, 0]));
    }

    #[test]
    fn octagon_retains_a_relation_that_marginal_intervals_cannot_express() {
        let mut octagon = Octagon::top(2);
        octagon
            .assume(OctagonConstraint::binary(
                OctagonTerm::positive(0),
                OctagonTerm::negative(1),
                0,
            ))
            .expect("assume x minus y");
        assert_eq!(octagon.interval(0), Some(Interval::top()));
        assert_eq!(octagon.interval(1), Some(Interval::top()));
        assert!(!octagon.contains(&[1, 0]));
        assert!(octagon.contains(&[0, 1]));
    }

    #[test]
    fn interval_reduction_propagates_relational_bounds_at_product_join() {
        type Product = ReducedProduct<IntervalEnv, Octagon, IntervalOctagonReduction>;

        let equality = [
            OctagonConstraint::binary(OctagonTerm::positive(0), OctagonTerm::negative(1), 0),
            OctagonConstraint::binary(OctagonTerm::positive(1), OctagonTerm::negative(0), 0),
        ];
        let mut left_octagon = Octagon::top(2);
        left_octagon
            .assume_all(equality)
            .expect("close left equality");
        let mut right_octagon = Octagon::top(2);
        right_octagon
            .assume_all(equality)
            .expect("close right equality");

        let mut left = Product::new(
            IntervalEnv::from_vars([(0, Interval::singleton(0)), (1, Interval::top())]),
            left_octagon,
        );
        let right = Product::new(
            IntervalEnv::from_vars([(0, Interval::singleton(4)), (1, Interval::top())]),
            right_octagon,
        );
        assert_eq!(left.first().get(1), Some(Interval::singleton(0)));
        assert_eq!(right.first().get(1), Some(Interval::singleton(4)));

        assert!(left.join(&right));
        let expected = Interval::new(IntervalBound::Finite(0), IntervalBound::Finite(4));
        assert_eq!(left.first().get(0), expected);
        assert_eq!(left.first().get(1), expected);
        assert!(!left.second().contains(&[0, 4]));
        assert!(left.second().contains(&[3, 3]));
    }

    #[test]
    fn join_widen_and_narrow_are_sound_and_convergent() {
        let point_zero = Octagon::from_points(&[vec![0]]).expect("abstract zero");
        let point_one = Octagon::from_points(&[vec![1]]).expect("abstract one");
        let point_two = Octagon::from_points(&[vec![2]]).expect("abstract two");

        let mut joined = point_zero.clone();
        assert!(joined.join(&point_one));
        assert!(joined.contains(&[0]));
        assert!(joined.contains(&[1]));
        assert!(!joined.join(&joined.clone()));

        let mut widened = joined.clone();
        assert!(widened.widen(&point_two).expect("widen octagon"));
        assert!(widened.contains(&[i64::MAX]));
        assert!(!widened.widen(&point_two).expect("stable widening"));

        assert!(widened.narrow(&point_two).expect("narrow octagon"));
        assert!(widened.contains(&[2]));
        assert!(!widened.contains(&[3]));
        assert!(point_two.leq(&widened));
    }

    #[test]
    fn out_of_domain_terms_grow_totally_and_point_dimensions_are_checked() {
        let mut octagon = Octagon::top(0);
        octagon
            .assume(OctagonConstraint::unary(OctagonTerm::positive(7), 9))
            .expect("grow octagon to variable seven");
        assert_eq!(octagon.variable_count(), 8);
        assert_eq!(
            octagon.interval(7),
            Interval::new(IntervalBound::NegInf, IntervalBound::Finite(9))
        );
        assert!(!octagon.contains(&[0; 7]));
        assert!(matches!(
            Octagon::from_points(&[vec![0], vec![0, 1]]),
            Err(OctagonError::PointDimension { .. })
        ));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn point_abstraction_is_sound_deterministic_and_obeys_join_laws(
            raw in prop::collection::vec(prop::collection::vec(-20i8..=20, 3), 1..16),
        ) {
            let points: Vec<Vec<i64>> = raw
                .into_iter()
                .map(|point| point.into_iter().map(i64::from).collect())
                .collect();
            let mut reverse = points.clone();
            reverse.reverse();
            let abstraction = Octagon::from_points(&points).expect("equal point dimensions");
            let reversed = Octagon::from_points(&reverse).expect("equal reversed dimensions");
            prop_assert_eq!(&abstraction, &reversed);
            for point in &points {
                prop_assert!(abstraction.contains(point));
            }

            let split = points.len() / 2;
            let left_points = &points[..split.max(1)];
            let right_points = &points[split.max(1)..];
            let left = Octagon::from_points(left_points).expect("left abstraction");
            let right = if right_points.is_empty() {
                Octagon::bottom(3)
            } else {
                Octagon::from_points(right_points).expect("right abstraction")
            };
            let mut left_join_right = left.clone();
            left_join_right.join(&right);
            let mut right_join_left = right.clone();
            right_join_left.join(&left);
            prop_assert_eq!(&left_join_right, &right_join_left);
            prop_assert!(left.leq(&left_join_right));
            prop_assert!(right.leq(&left_join_right));
            for point in &points {
                prop_assert!(left_join_right.contains(point));
            }
        }
    }

    #[test]
    fn closure_lifecycle_is_stack_safe_on_a_small_native_stack() {
        std::thread::Builder::new()
            .name("octagon-small-stack".into())
            .stack_size(256 * 1024)
            .spawn(|| {
                let variables = 96u32;
                let mut constraints = Vec::with_capacity((variables - 1) as usize);
                for variable in 0..variables - 1 {
                    constraints.push(OctagonConstraint::binary(
                        OctagonTerm::positive(variable),
                        OctagonTerm::negative(variable + 1),
                        1,
                    ));
                }
                let mut octagon = Octagon::top(0);
                octagon
                    .assume_all(constraints)
                    .expect("close deep relation chain");
                assert_eq!(octagon.variable_count(), variables as usize);
                assert!(octagon.contains(&vec![0; variables as usize]));
                assert_eq!(
                    octagon.implied_bound(
                        OctagonTerm::positive(0),
                        Some(OctagonTerm::negative(variables - 1))
                    ),
                    Some(OctagonBound::Finite(i128::from(variables - 1)))
                );
            })
            .expect("spawn octagon small-stack lifecycle")
            .join()
            .expect("join octagon small-stack lifecycle");
    }

    #[test]
    fn imported_solver_types_remain_feature_free() {
        let _config = SolverConfig::default();
        let _variables = BTreeSet::<u32>::new();
        assert!(Octagon::top(0).is_top());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn serde_round_trip_preserves_the_semantic_element() {
        let octagon =
            Octagon::from_points(&[vec![1, 2], vec![3, 4]]).expect("abstract serde points");
        let encoded = serde_json::to_string(&octagon).expect("serialize octagon");
        let decoded: Octagon = serde_json::from_str(&encoded).expect("deserialize octagon");
        assert_eq!(decoded, octagon);

        let malformed = r#"{"variables":2,"empty":false,"matrix":[]}"#;
        assert!(serde_json::from_str::<Octagon>(malformed).is_err());
    }
}
