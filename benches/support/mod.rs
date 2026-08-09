use libcpg::{CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, Language, SourceRange};

/// Construct a deterministic directed path using only the feature-free graph API.
pub fn linear_graph(node_count: usize) -> CodePropertyGraph {
    let mut graph = CodePropertyGraph::new(Language::Rust);
    let nodes: Vec<_> = (0..node_count)
        .map(|_| {
            graph.add_node(CpgNode::new(
                0.into(),
                CpgNodeKind::Return,
                SourceRange::default(),
            ))
        })
        .collect();

    for endpoints in nodes.windows(2) {
        graph.connect(endpoints[0], endpoints[1], CpgEdgeKind::AstChild);
    }

    graph
}

/// Assert the exact path shape before a benchmark enters its timed loop.
pub fn assert_linear_graph(graph: &CodePropertyGraph, node_count: usize) {
    assert_eq!(graph.node_count(), node_count, "path node oracle");
    assert_eq!(
        graph.edge_count(),
        node_count.saturating_sub(1),
        "path edge oracle"
    );

    let mut ids: Vec<_> = graph.node_ids().collect();
    ids.sort_unstable();
    for endpoints in ids.windows(2) {
        assert_eq!(
            graph.ast_children(endpoints[0]),
            vec![endpoints[1]],
            "path adjacency oracle"
        );
    }
}
