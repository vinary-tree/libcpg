//! Graph algorithm patterns.

/// Known graph algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphAlgorithm {
    /// Breadth-first search.
    BFS,
    /// Depth-first search.
    DFS,
    /// Dijkstra's shortest path.
    Dijkstra,
    /// Bellman-Ford shortest path.
    BellmanFord,
    /// Floyd-Warshall all pairs shortest path.
    FloydWarshall,
    /// Prim's MST.
    Prim,
    /// Kruskal's MST.
    Kruskal,
    /// Topological sort.
    TopologicalSort,
    /// Strongly connected components.
    SCC,
    /// Articulation points.
    ArticulationPoints,
    /// Unknown graph algorithm.
    Unknown,
}
