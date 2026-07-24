//! Algorithm family definitions.
//!
//! Defines categories of common algorithms and their characteristics.

pub mod sorting;
pub mod searching;
pub mod graph;
pub mod dp;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Algorithm families.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum AlgorithmFamily {
    /// Sorting algorithms (quicksort, mergesort, etc.).
    Sorting,
    /// Searching algorithms (binary search, linear search, etc.).
    Searching,
    /// Graph traversal (BFS, DFS, etc.).
    GraphTraversal,
    /// Shortest path algorithms (Dijkstra, Bellman-Ford, etc.).
    ShortestPath,
    /// Minimum spanning tree (Prim, Kruskal, etc.).
    MinimumSpanningTree,
    /// Dynamic programming.
    DynamicProgramming,
    /// Divide and conquer.
    DivideAndConquer,
    /// Greedy algorithms.
    Greedy,
    /// Backtracking algorithms.
    Backtracking,
    /// String matching (KMP, Rabin-Karp, etc.).
    StringMatching,
    /// Tree algorithms.
    TreeAlgorithm,
    /// Hashing.
    Hashing,
    /// Mathematical/numerical.
    Mathematical,
    /// Other/unknown.
    Other,
}

impl AlgorithmFamily {
    /// Returns the family name as a string.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Sorting => "Sorting",
            Self::Searching => "Searching",
            Self::GraphTraversal => "Graph Traversal",
            Self::ShortestPath => "Shortest Path",
            Self::MinimumSpanningTree => "Minimum Spanning Tree",
            Self::DynamicProgramming => "Dynamic Programming",
            Self::DivideAndConquer => "Divide and Conquer",
            Self::Greedy => "Greedy",
            Self::Backtracking => "Backtracking",
            Self::StringMatching => "String Matching",
            Self::TreeAlgorithm => "Tree Algorithm",
            Self::Hashing => "Hashing",
            Self::Mathematical => "Mathematical",
            Self::Other => "Other",
        }
    }

    /// Returns typical complexity for well-implemented algorithms in this family.
    pub fn typical_complexity(&self) -> &'static str {
        match self {
            Self::Sorting => "O(n log n)",
            Self::Searching => "O(log n) to O(n)",
            Self::GraphTraversal => "O(V + E)",
            Self::ShortestPath => "O(E log V) to O(V³)",
            Self::MinimumSpanningTree => "O(E log V)",
            Self::DynamicProgramming => "Varies",
            Self::DivideAndConquer => "O(n log n)",
            Self::Greedy => "O(n log n)",
            Self::Backtracking => "O(k^n) worst case",
            Self::StringMatching => "O(n + m)",
            Self::TreeAlgorithm => "O(n)",
            Self::Hashing => "O(1) average",
            Self::Mathematical => "Varies",
            Self::Other => "Unknown",
        }
    }
}

impl std::fmt::Display for AlgorithmFamily {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every `AlgorithmFamily` variant.
    fn all_families() -> Vec<AlgorithmFamily> {
        vec![
            AlgorithmFamily::Sorting,
            AlgorithmFamily::Searching,
            AlgorithmFamily::GraphTraversal,
            AlgorithmFamily::ShortestPath,
            AlgorithmFamily::MinimumSpanningTree,
            AlgorithmFamily::DynamicProgramming,
            AlgorithmFamily::DivideAndConquer,
            AlgorithmFamily::Greedy,
            AlgorithmFamily::Backtracking,
            AlgorithmFamily::StringMatching,
            AlgorithmFamily::TreeAlgorithm,
            AlgorithmFamily::Hashing,
            AlgorithmFamily::Mathematical,
            AlgorithmFamily::Other,
        ]
    }

    #[test]
    fn test_name_every_variant() {
        assert_eq!(AlgorithmFamily::Sorting.name(), "Sorting");
        assert_eq!(AlgorithmFamily::Searching.name(), "Searching");
        assert_eq!(AlgorithmFamily::GraphTraversal.name(), "Graph Traversal");
        assert_eq!(AlgorithmFamily::ShortestPath.name(), "Shortest Path");
        assert_eq!(AlgorithmFamily::MinimumSpanningTree.name(), "Minimum Spanning Tree");
        assert_eq!(AlgorithmFamily::DynamicProgramming.name(), "Dynamic Programming");
        assert_eq!(AlgorithmFamily::DivideAndConquer.name(), "Divide and Conquer");
        assert_eq!(AlgorithmFamily::Greedy.name(), "Greedy");
        assert_eq!(AlgorithmFamily::Backtracking.name(), "Backtracking");
        assert_eq!(AlgorithmFamily::StringMatching.name(), "String Matching");
        assert_eq!(AlgorithmFamily::TreeAlgorithm.name(), "Tree Algorithm");
        assert_eq!(AlgorithmFamily::Hashing.name(), "Hashing");
        assert_eq!(AlgorithmFamily::Mathematical.name(), "Mathematical");
        assert_eq!(AlgorithmFamily::Other.name(), "Other");
    }

    #[test]
    fn test_typical_complexity_every_variant() {
        assert_eq!(AlgorithmFamily::Sorting.typical_complexity(), "O(n log n)");
        assert_eq!(AlgorithmFamily::Searching.typical_complexity(), "O(log n) to O(n)");
        assert_eq!(AlgorithmFamily::GraphTraversal.typical_complexity(), "O(V + E)");
        assert_eq!(AlgorithmFamily::ShortestPath.typical_complexity(), "O(E log V) to O(V³)");
        assert_eq!(AlgorithmFamily::MinimumSpanningTree.typical_complexity(), "O(E log V)");
        assert_eq!(AlgorithmFamily::DynamicProgramming.typical_complexity(), "Varies");
        assert_eq!(AlgorithmFamily::DivideAndConquer.typical_complexity(), "O(n log n)");
        assert_eq!(AlgorithmFamily::Greedy.typical_complexity(), "O(n log n)");
        assert_eq!(AlgorithmFamily::Backtracking.typical_complexity(), "O(k^n) worst case");
        assert_eq!(AlgorithmFamily::StringMatching.typical_complexity(), "O(n + m)");
        assert_eq!(AlgorithmFamily::TreeAlgorithm.typical_complexity(), "O(n)");
        assert_eq!(AlgorithmFamily::Hashing.typical_complexity(), "O(1) average");
        assert_eq!(AlgorithmFamily::Mathematical.typical_complexity(), "Varies");
        assert_eq!(AlgorithmFamily::Other.typical_complexity(), "Unknown");
    }

    #[test]
    fn test_display_matches_name_and_nonempty() {
        for fam in all_families() {
            assert_eq!(format!("{}", fam), fam.name());
            assert!(!fam.name().is_empty());
            assert!(!fam.typical_complexity().is_empty());
        }
    }

    #[test]
    fn test_family_equality_and_copy() {
        let a = AlgorithmFamily::Sorting;
        let b = a; // Copy
        assert_eq!(a, b);
        assert_ne!(AlgorithmFamily::Sorting, AlgorithmFamily::Searching);
    }
}
