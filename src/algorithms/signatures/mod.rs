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

    /// Returns true if this complexity is better (smaller) than another.
    pub fn is_better_than(&self, other: &Self) -> bool {
        self.ordinal() < other.ordinal()
    }

    /// Returns a numeric ordering value.
    fn ordinal(&self) -> u32 {
        match self {
            Self::Constant => 0,
            Self::Logarithmic => 1,
            Self::Linear => 2,
            Self::Linearithmic => 3,
            Self::Quadratic => 4,
            Self::Cubic => 5,
            Self::Polynomial(k) => 5 + *k,
            Self::Exponential => 100,
            Self::Factorial => 200,
            Self::Unknown => 1000,
        }
    }
}

impl std::fmt::Display for ComplexityClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}
