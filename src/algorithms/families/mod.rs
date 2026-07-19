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
