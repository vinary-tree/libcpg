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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_graph_algorithm_variants() {
        let all = [
            GraphAlgorithm::BFS,
            GraphAlgorithm::DFS,
            GraphAlgorithm::Dijkstra,
            GraphAlgorithm::BellmanFord,
            GraphAlgorithm::FloydWarshall,
            GraphAlgorithm::Prim,
            GraphAlgorithm::Kruskal,
            GraphAlgorithm::TopologicalSort,
            GraphAlgorithm::SCC,
            GraphAlgorithm::ArticulationPoints,
            GraphAlgorithm::Unknown,
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
