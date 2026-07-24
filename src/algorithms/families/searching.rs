//! Searching algorithm patterns.

/// Known searching algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchingAlgorithm {
    /// Binary search.
    BinarySearch,
    /// Linear search.
    LinearSearch,
    /// Interpolation search.
    InterpolationSearch,
    /// Exponential search.
    ExponentialSearch,
    /// Jump search.
    JumpSearch,
    /// Hash-based lookup.
    HashLookup,
    /// Tree-based search.
    TreeSearch,
    /// Unknown searching algorithm.
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_searching_algorithm_variants() {
        let all = [
            SearchingAlgorithm::BinarySearch,
            SearchingAlgorithm::LinearSearch,
            SearchingAlgorithm::InterpolationSearch,
            SearchingAlgorithm::ExponentialSearch,
            SearchingAlgorithm::JumpSearch,
            SearchingAlgorithm::HashLookup,
            SearchingAlgorithm::TreeSearch,
            SearchingAlgorithm::Unknown,
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
