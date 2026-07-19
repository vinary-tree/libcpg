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
