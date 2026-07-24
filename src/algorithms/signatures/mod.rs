//! Algorithm signatures and complexity types.
//!
//! Defines structures for representing algorithm characteristics
//! and computational complexity.

pub mod loops;
pub mod recursion;

pub use loops::{LoopStructure, LoopType, LoopKind as SigLoopKind, LoopBounds};
pub use recursion::{RecursionPattern, RecursionKind as SigRecursionKind, ReductionPattern};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Signature characterizing an algorithm.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AlgorithmSignature {
    /// Loop structure characteristics.
    pub loop_structure: Option<LoopStructure>,
    /// Recursion pattern (if any).
    pub recursion_pattern: Option<RecursionPattern>,
    /// Estimated time complexity.
    pub time_complexity: Option<ComplexityEstimate>,
    /// Estimated space complexity.
    pub space_complexity: Option<ComplexityEstimate>,
    /// Feature vector for ML-based classification.
    pub feature_vector: Vec<f32>,
}

impl AlgorithmSignature {
    /// Creates a new empty signature.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the loop structure.
    pub fn with_loop_structure(mut self, structure: LoopStructure) -> Self {
        self.loop_structure = Some(structure);
        self
    }

    /// Sets the recursion pattern.
    pub fn with_recursion(mut self, pattern: RecursionPattern) -> Self {
        self.recursion_pattern = Some(pattern);
        self
    }

    /// Sets the time complexity estimate.
    pub fn with_time_complexity(mut self, estimate: ComplexityEstimate) -> Self {
        self.time_complexity = Some(estimate);
        self
    }

    /// Sets the space complexity estimate.
    pub fn with_space_complexity(mut self, estimate: ComplexityEstimate) -> Self {
        self.space_complexity = Some(estimate);
        self
    }
}

/// Estimated computational complexity.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ComplexityEstimate {
    /// The complexity class.
    pub class: ComplexityClass,
    /// Confidence in this estimate (0.0 to 1.0).
    pub confidence: f64,
    /// Justification/explanation.
    pub justification: String,
}

/// Big-O complexity classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ComplexityClass {
    /// O(1) - Constant time.
    Constant,
    /// O(log n) - Logarithmic.
    Logarithmic,
    /// O(n) - Linear.
    Linear,
    /// O(n log n) - Linearithmic.
    Linearithmic,
    /// O(n^2) - Quadratic.
    Quadratic,
    /// O(n^3) - Cubic.
    Cubic,
    /// O(n^k) - Polynomial (general).
    Polynomial(u32),
    /// O(2^n) - Exponential.
    Exponential,
    /// O(n!) - Factorial.
    Factorial,
    /// Unknown complexity.
    #[default]
    Unknown,
}

impl ComplexityClass {
    /// Returns the complexity as a string.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Constant => "O(1)",
            Self::Logarithmic => "O(log n)",
            Self::Linear => "O(n)",
            Self::Linearithmic => "O(n log n)",
            Self::Quadratic => "O(n²)",
            Self::Cubic => "O(n³)",
            Self::Polynomial(_) => "O(n^k)",
            Self::Exponential => "O(2^n)",
            Self::Factorial => "O(n!)",
            Self::Unknown => "Unknown",
        }
    }

    /// Returns true if this complexity is better (asymptotically smaller) than
    /// another.
    ///
    /// This is a strict weak order: irreflexive, asymmetric, and transitive.
    /// Two classes denoting the *same* growth rate compare equal (neither is
    /// better) — e.g. `Cubic` and `Polynomial(3)`, or `Constant` and
    /// `Polynomial(0)`.
    pub fn is_better_than(&self, other: &Self) -> bool {
        self.rank() < other.rank()
    }

    /// Returns a total, growth-faithful ordering key.
    ///
    /// `Polynomial(k)` denotes `O(n^k)`, so small `k` collapse onto the named
    /// classes (`0→Constant`, `1→Linear`, `2→Quadratic`, `3→Cubic`); for
    /// `k ≥ 4` the key sits strictly between `Cubic` and `Exponential` and
    /// increases with `k` — a polynomial is always asymptotically better than
    /// any exponential, regardless of how large `k` is.
    fn rank(&self) -> f64 {
        match self {
            Self::Constant => 0.0,
            Self::Logarithmic => 1.0,
            Self::Linear => 2.0,
            Self::Linearithmic => 3.0,
            Self::Quadratic => 4.0,
            Self::Cubic => 5.0,
            Self::Polynomial(k) => match k {
                0 => 0.0, // O(n^0) = O(1)
                1 => 2.0, // O(n)
                2 => 4.0, // O(n^2)
                3 => 5.0, // O(n^3)
                // k >= 4: in (5, 6), strictly increasing in k, always < Exponential.
                _ => 6.0 - 1.0 / (*k as f64),
            },
            Self::Exponential => 10.0,
            Self::Factorial => 20.0,
            Self::Unknown => 1000.0,
        }
    }
}

impl std::fmt::Display for ComplexityClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeId;

    /// Every `ComplexityClass` variant, used to exhaustively exercise `as_str`
    /// and `Display`. `Polynomial` is represented by a couple of `k` values.
    fn all_named_classes() -> Vec<ComplexityClass> {
        vec![
            ComplexityClass::Constant,
            ComplexityClass::Logarithmic,
            ComplexityClass::Linear,
            ComplexityClass::Linearithmic,
            ComplexityClass::Quadratic,
            ComplexityClass::Cubic,
            ComplexityClass::Polynomial(4),
            ComplexityClass::Exponential,
            ComplexityClass::Factorial,
            ComplexityClass::Unknown,
        ]
    }

    #[test]
    fn test_as_str_every_variant() {
        assert_eq!(ComplexityClass::Constant.as_str(), "O(1)");
        assert_eq!(ComplexityClass::Logarithmic.as_str(), "O(log n)");
        assert_eq!(ComplexityClass::Linear.as_str(), "O(n)");
        assert_eq!(ComplexityClass::Linearithmic.as_str(), "O(n log n)");
        assert_eq!(ComplexityClass::Quadratic.as_str(), "O(n²)");
        assert_eq!(ComplexityClass::Cubic.as_str(), "O(n³)");
        // Polynomial always renders the generic form regardless of `k`.
        assert_eq!(ComplexityClass::Polynomial(0).as_str(), "O(n^k)");
        assert_eq!(ComplexityClass::Polynomial(2).as_str(), "O(n^k)");
        assert_eq!(ComplexityClass::Polynomial(7).as_str(), "O(n^k)");
        assert_eq!(ComplexityClass::Exponential.as_str(), "O(2^n)");
        assert_eq!(ComplexityClass::Factorial.as_str(), "O(n!)");
        assert_eq!(ComplexityClass::Unknown.as_str(), "Unknown");
    }

    #[test]
    fn test_display_matches_as_str() {
        for c in all_named_classes() {
            assert_eq!(format!("{}", c), c.as_str());
        }
        // Display also delegates to as_str for Polynomial.
        assert_eq!(format!("{}", ComplexityClass::Polynomial(5)), "O(n^k)");
    }

    #[test]
    fn test_is_better_than_named_ladder() {
        // Strictly ascending growth: each is better than the next.
        assert!(ComplexityClass::Constant.is_better_than(&ComplexityClass::Logarithmic));
        assert!(ComplexityClass::Logarithmic.is_better_than(&ComplexityClass::Linear));
        assert!(ComplexityClass::Linear.is_better_than(&ComplexityClass::Linearithmic));
        assert!(ComplexityClass::Linearithmic.is_better_than(&ComplexityClass::Quadratic));
        assert!(ComplexityClass::Quadratic.is_better_than(&ComplexityClass::Cubic));
        assert!(ComplexityClass::Cubic.is_better_than(&ComplexityClass::Exponential));
        assert!(ComplexityClass::Exponential.is_better_than(&ComplexityClass::Factorial));
        assert!(ComplexityClass::Factorial.is_better_than(&ComplexityClass::Unknown));
        // Reverse never holds.
        assert!(!ComplexityClass::Linear.is_better_than(&ComplexityClass::Constant));
        assert!(!ComplexityClass::Exponential.is_better_than(&ComplexityClass::Cubic));
    }

    #[test]
    fn test_is_better_than_irreflexive_spot() {
        for c in all_named_classes() {
            assert!(!c.is_better_than(&c), "{c:?} must not be better than itself");
        }
    }

    #[test]
    fn test_polynomial_collapses_onto_named_ladder() {
        // Small k collapse exactly onto the named classes: neither is better.
        let pairs = [
            (ComplexityClass::Polynomial(0), ComplexityClass::Constant),
            (ComplexityClass::Polynomial(1), ComplexityClass::Linear),
            (ComplexityClass::Polynomial(2), ComplexityClass::Quadratic),
            (ComplexityClass::Polynomial(3), ComplexityClass::Cubic),
        ];
        for (poly, named) in pairs {
            assert!(!poly.is_better_than(&named), "{poly:?} vs {named:?} must tie");
            assert!(!named.is_better_than(&poly), "{named:?} vs {poly:?} must tie");
        }
    }

    #[test]
    fn test_polynomial_high_k_between_cubic_and_exponential() {
        // k >= 4 sits strictly between Cubic and Exponential.
        assert!(ComplexityClass::Cubic.is_better_than(&ComplexityClass::Polynomial(4)));
        assert!(ComplexityClass::Polynomial(4).is_better_than(&ComplexityClass::Exponential));
        // A polynomial is always better than exponential, however large k is.
        assert!(ComplexityClass::Polynomial(1000).is_better_than(&ComplexityClass::Exponential));
        // Strictly increasing in k for k >= 4: smaller k is better.
        assert!(ComplexityClass::Polynomial(4).is_better_than(&ComplexityClass::Polynomial(5)));
        assert!(!ComplexityClass::Polynomial(5).is_better_than(&ComplexityClass::Polynomial(4)));
    }

    #[test]
    fn test_algorithm_signature_builders() {
        let sig = AlgorithmSignature::new();
        assert!(sig.loop_structure.is_none());
        assert!(sig.recursion_pattern.is_none());
        assert!(sig.time_complexity.is_none());
        assert!(sig.space_complexity.is_none());
        assert!(sig.feature_vector.is_empty());

        let time = ComplexityEstimate {
            class: ComplexityClass::Linear,
            confidence: 0.8,
            justification: "single loop".to_string(),
        };
        let space = ComplexityEstimate {
            class: ComplexityClass::Constant,
            confidence: 0.7,
            justification: "no allocation".to_string(),
        };

        let sig = sig
            .with_loop_structure(LoopStructure::new().with_max_depth(2))
            .with_recursion(RecursionPattern::new(SigRecursionKind::Tail))
            .with_time_complexity(time)
            .with_space_complexity(space);

        assert_eq!(
            sig.loop_structure.as_ref().expect("loop structure set").max_depth,
            2
        );
        assert_eq!(
            sig.recursion_pattern.as_ref().expect("recursion set").kind,
            SigRecursionKind::Tail
        );
        assert_eq!(
            sig.time_complexity.as_ref().expect("time set").class,
            ComplexityClass::Linear
        );
        assert_eq!(
            sig.space_complexity.as_ref().expect("space set").class,
            ComplexityClass::Constant
        );
    }

    #[test]
    fn test_recursion_pattern_via_signature_reexports() {
        // Exercise the re-exported LoopType / SigLoopKind / LoopBounds path.
        let lt = LoopType {
            header: NodeId::new(3),
            kind: SigLoopKind::CountedFor,
            depth: 1,
            bounds: LoopBounds::LinearN,
            has_early_exit: false,
        };
        let ls = LoopStructure::new().with_loop(lt).with_independence(true);
        assert_eq!(ls.loop_count, 1);
        assert_eq!(ls.loop_types.len(), 1);
        assert!(ls.loops_independent);
        let rp = RecursionPattern::new(SigRecursionKind::Direct)
            .with_reduction(ReductionPattern::Division(2));
        assert!(matches!(rp.reduction, ReductionPattern::Division(2)));
    }

    #[test]
    fn test_signature_default_matches_new() {
        let a = AlgorithmSignature::default();
        let b = AlgorithmSignature::new();
        assert_eq!(a.feature_vector.len(), b.feature_vector.len());
        assert!(a.loop_structure.is_none());
        assert!(b.loop_structure.is_none());
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    /// A strategy over every `ComplexityClass`, including `Polynomial(k)` for a
    /// spread of `k` that straddles the named-ladder collapse (`k <= 3`) and the
    /// strictly-between-Cubic-and-Exponential regime (`k >= 4`).
    fn arb_complexity_class() -> impl Strategy<Value = ComplexityClass> {
        prop_oneof![
            Just(ComplexityClass::Constant),
            Just(ComplexityClass::Logarithmic),
            Just(ComplexityClass::Linear),
            Just(ComplexityClass::Linearithmic),
            Just(ComplexityClass::Quadratic),
            Just(ComplexityClass::Cubic),
            (0u32..=8).prop_map(ComplexityClass::Polynomial),
            Just(ComplexityClass::Exponential),
            Just(ComplexityClass::Factorial),
            Just(ComplexityClass::Unknown),
        ]
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// `is_better_than` is a strict weak order: irreflexive, asymmetric, and
        /// transitive. (No trichotomy: equal-growth classes such as `Cubic` and
        /// `Polynomial(3)` tie, so neither is better.)
        #[test]
        fn prop_is_better_than_strict_weak_order(
            a in arb_complexity_class(),
            b in arb_complexity_class(),
            c in arb_complexity_class(),
        ) {
            // Irreflexive.
            prop_assert!(!a.is_better_than(&a));
            prop_assert!(!b.is_better_than(&b));
            prop_assert!(!c.is_better_than(&c));

            // Asymmetric: a<b implies not b<a.
            if a.is_better_than(&b) {
                prop_assert!(!b.is_better_than(&a));
            }

            // Transitive: a<b and b<c implies a<c.
            if a.is_better_than(&b) && b.is_better_than(&c) {
                prop_assert!(a.is_better_than(&c));
            }
        }

        /// `as_str` and `Display` always agree, for every class incl. Polynomial.
        #[test]
        fn prop_display_agrees_with_as_str(c in arb_complexity_class()) {
            prop_assert_eq!(format!("{}", c), c.as_str());
        }
    }
}
