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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dp_pattern_variants() {
        let all = [
            DPPattern::Linear,
            DPPattern::Matrix,
            DPPattern::Interval,
            DPPattern::Tree,
            DPPattern::Digit,
            DPPattern::Bitmask,
            DPPattern::Knapsack,
            DPPattern::Subsequence,
            DPPattern::Unknown,
        ];
        for (i, a) in all.iter().enumerate() {
            let copied = *a;
            assert_eq!(copied, *a);
            assert!(!format!("{a:?}").is_empty());
            for (j, b) in all.iter().enumerate() {
                assert_eq!(i == j, a == b);
            }
        }
    }
}
