//! Reduced products for relationally communicating abstract domains.
//!
//! A reduced product stores two lattice elements whose concretizations describe
//! the same concrete state. A type-level [`Reduction`] policy exchanges facts
//! after construction, transfer, and lattice join. Because the generic
//! data-flow solver calls [`Lattice::join`] at CFG confluence, reduction occurs
//! at every merge without adding a second solver or a dynamic callback.

use std::{fmt, marker::PhantomData};

use super::Lattice;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// A sound closure operator for two component abstract domains.
///
/// Implementations must be deterministic, idempotent, and reductive in each
/// component's lattice order while preserving the represented concrete
/// intersection. The policy is a type rather than a stored closure, so product
/// values remain cloneable, comparable, serializable, and free of indirect
/// calls.
pub trait Reduction<A: Lattice, B: Lattice> {
    /// Exchanges facts between `first` and `second` and reports whether either
    /// component changed.
    fn reduce(first: &mut A, second: &mut B) -> bool;
}

/// The direct product: no facts are exchanged between components.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct NoReduction;

impl<A: Lattice, B: Lattice> Reduction<A, B> for NoReduction {
    fn reduce(_first: &mut A, _second: &mut B) -> bool {
        false
    }
}

/// Two abstract domains closed under a static reduction policy.
///
/// `R` defaults to [`NoReduction`], which is the ordinary direct product.
/// Construct values with [`ReducedProduct::new`] so the invariant is established
/// before the value can enter a solver.
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(bound(
        serialize = "A: Serialize, B: Serialize",
        deserialize = "A: Deserialize<'de>, B: Deserialize<'de>"
    ))
)]
pub struct ReducedProduct<A, B, R = NoReduction> {
    first: A,
    second: B,
    #[cfg_attr(feature = "serde", serde(skip))]
    reduction: PhantomData<fn() -> R>,
}

impl<A: Clone, B: Clone, R> Clone for ReducedProduct<A, B, R> {
    fn clone(&self) -> Self {
        Self {
            first: self.first.clone(),
            second: self.second.clone(),
            reduction: PhantomData,
        }
    }
}

impl<A: fmt::Debug, B: fmt::Debug, R> fmt::Debug for ReducedProduct<A, B, R> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReducedProduct")
            .field("first", &self.first)
            .field("second", &self.second)
            .finish()
    }
}

impl<A: PartialEq, B: PartialEq, R> PartialEq for ReducedProduct<A, B, R> {
    fn eq(&self, other: &Self) -> bool {
        self.first == other.first && self.second == other.second
    }
}

impl<A: Eq, B: Eq, R> Eq for ReducedProduct<A, B, R> {}

impl<A, B, R> ReducedProduct<A, B, R>
where
    A: Lattice,
    B: Lattice,
    R: Reduction<A, B>,
{
    /// Constructs a product and closes it under `R`.
    pub fn new(mut first: A, mut second: B) -> Self {
        R::reduce(&mut first, &mut second);
        Self {
            first,
            second,
            reduction: PhantomData,
        }
    }

    /// Borrows the first component.
    pub const fn first(&self) -> &A {
        &self.first
    }

    /// Borrows the second component.
    pub const fn second(&self) -> &B {
        &self.second
    }

    /// Consumes the product and returns its reduced components.
    pub fn into_components(self) -> (A, B) {
        (self.first, self.second)
    }

    /// Applies one transfer to both components, then restores the reduction
    /// invariant.
    ///
    /// The return value describes semantic component change, not whether the
    /// caller's closure happened to write an equal representation.
    pub fn transform(&mut self, operation: impl FnOnce(&mut A, &mut B)) -> bool {
        let before_first = self.first.clone();
        let before_second = self.second.clone();
        operation(&mut self.first, &mut self.second);
        R::reduce(&mut self.first, &mut self.second);
        self.first != before_first || self.second != before_second
    }

    /// Reapplies `R`, useful after deserializing or importing component values.
    pub fn reduce(&mut self) -> bool {
        R::reduce(&mut self.first, &mut self.second)
    }

    /// Widens both components with caller-supplied component operators and
    /// reduces the result.
    pub fn widen_with(
        &mut self,
        incoming: &Self,
        widen_first: impl FnOnce(&mut A, &A) -> bool,
        widen_second: impl FnOnce(&mut B, &B) -> bool,
    ) -> bool {
        let first_changed = widen_first(&mut self.first, &incoming.first);
        let second_changed = widen_second(&mut self.second, &incoming.second);
        first_changed | second_changed | R::reduce(&mut self.first, &mut self.second)
    }

    /// Narrows both components with caller-supplied component operators and
    /// reduces the result.
    pub fn narrow_with(
        &mut self,
        incoming: &Self,
        narrow_first: impl FnOnce(&mut A, &A) -> bool,
        narrow_second: impl FnOnce(&mut B, &B) -> bool,
    ) -> bool {
        let first_changed = narrow_first(&mut self.first, &incoming.first);
        let second_changed = narrow_second(&mut self.second, &incoming.second);
        first_changed | second_changed | R::reduce(&mut self.first, &mut self.second)
    }
}

impl<A, B, R> Default for ReducedProduct<A, B, R>
where
    A: Lattice + Default,
    B: Lattice + Default,
    R: Reduction<A, B>,
{
    fn default() -> Self {
        Self::new(A::default(), B::default())
    }
}

impl<A, B, R> Lattice for ReducedProduct<A, B, R>
where
    A: Lattice,
    B: Lattice,
    R: Reduction<A, B>,
{
    fn join(&mut self, other: &Self) -> bool {
        let first_changed = self.first.join(&other.first);
        let second_changed = self.second.join(&other.second);
        first_changed | second_changed | R::reduce(&mut self.first, &mut self.second)
    }

    fn leq(&self, other: &Self) -> bool {
        self.first.leq(&other.first) && self.second.leq(&other.second)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    struct Upper(u8);

    impl Lattice for Upper {
        fn join(&mut self, other: &Self) -> bool {
            let next = self.0.max(other.0);
            let changed = next != self.0;
            self.0 = next;
            changed
        }

        fn leq(&self, other: &Self) -> bool {
            self.0 <= other.0
        }
    }

    struct Equalize;

    impl Reduction<Upper, Upper> for Equalize {
        fn reduce(first: &mut Upper, second: &mut Upper) -> bool {
            let common = first.0.min(second.0);
            let changed = first.0 != common || second.0 != common;
            first.0 = common;
            second.0 = common;
            changed
        }
    }

    #[test]
    fn construction_transfer_and_join_restore_the_reduction_invariant() {
        let mut left = ReducedProduct::<_, _, Equalize>::new(Upper(3), Upper(7));
        assert_eq!((left.first().0, left.second().0), (3, 3));

        assert!(left.transform(|first, second| {
            first.0 = 8;
            second.0 = 5;
        }));
        assert_eq!((left.first().0, left.second().0), (5, 5));

        let right = ReducedProduct::<_, _, Equalize>::new(Upper(9), Upper(6));
        assert!(left.join(&right));
        assert_eq!((left.first().0, left.second().0), (6, 6));
        assert!(right.leq(&left));
        assert!(!left.join(&left.clone()));
    }

    #[test]
    fn widening_and_narrowing_delegate_then_reduce() {
        let incoming = ReducedProduct::<_, _, Equalize>::new(Upper(9), Upper(7));
        let mut product = ReducedProduct::<_, _, Equalize>::new(Upper(2), Upper(4));
        assert!(product.widen_with(
            &incoming,
            |left, right| left.join(right),
            |left, right| left.join(right)
        ));
        assert_eq!((product.first().0, product.second().0), (7, 7));

        assert!(product.narrow_with(
            &ReducedProduct::new(Upper(3), Upper(5)),
            |left, right| {
                let changed = left.0 != right.0;
                *left = *right;
                changed
            },
            |left, right| {
                let changed = left.0 != right.0;
                *left = *right;
                changed
            },
        ));
        assert_eq!((product.first().0, product.second().0), (3, 3));
    }
}
