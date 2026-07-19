//! Dynamic programming patterns.

/// Known DP problem patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DPPattern {
    /// Linear DP (1D state).
    Linear,
    /// 2D DP (matrix-based).
    Matrix,
    /// Interval DP.
    Interval,
    /// Tree DP.
    Tree,
    /// Digit DP.
    Digit,
    /// Bitmask DP.
    Bitmask,
    /// Knapsack-type.
    Knapsack,
    /// LCS/LIS type.
    Subsequence,
    /// Unknown pattern.
    Unknown,
}
