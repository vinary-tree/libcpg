//! Public-API stack-safety gate for the iterative VF2 search machine.

use libcpg::pattern::Vf2Matcher;
use libcpg::{
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, Language, NodeId, SourceRange,
    SubgraphMatcher,
};

#[test]
fn public_vf2_matches_a_depth_100_000_path_on_a_small_stack() {
    const DEPTH: usize = 100_000;

    std::thread::Builder::new()
        .name("vf2-search-small-stack".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut graph = CodePropertyGraph::new(Language::Rust);
            let mut nodes = Vec::new();
            nodes
                .try_reserve_exact(DEPTH)
                .expect("reserve deep path node ids");
            for _ in 0..DEPTH {
                nodes.push(graph.add_node(CpgNode::new(
                    NodeId::new(0),
                    CpgNodeKind::If,
                    SourceRange::default(),
                )));
            }
            for pair in nodes.windows(2) {
                graph.connect(pair[0], pair[1], CpgEdgeKind::AstChild);
            }

            let before = (graph.node_count(), graph.edge_count());
            let matches = Vf2Matcher::new()
                .with_strict_kinds(true)
                .with_strict_edges(true)
                .with_max_matches(1)
                .find_matches(&graph, &graph);

            assert_eq!(matches.len(), 1);
            assert_eq!(matches[0].root, nodes[0]);
            assert_eq!(matches[0].node_mapping.len(), DEPTH);
            assert_eq!(matches[0].node_mapping.get(&nodes[0]), Some(&nodes[0]));
            assert_eq!(
                matches[0].node_mapping.get(&nodes[DEPTH - 1]),
                Some(&nodes[DEPTH - 1])
            );
            assert_eq!((graph.node_count(), graph.edge_count()), before);
        })
        .expect("spawn VF2 small-stack test")
        .join()
        .expect("iterative VF2 search completes on a small native stack");
}
