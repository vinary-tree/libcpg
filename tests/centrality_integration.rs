use libcpg::{
    betweenness_centrality, burt_constraint, closeness_centrality, dominator_tree,
    edge_betweenness, eigenvector_centrality, harmonic_centrality, hits, katz_centrality, pagerank,
    personalized_pagerank, reverse_pagerank, BetweennessOptions, CentralityBudget, GraphProjection,
    IterationOptions, KatzOptions, NodeId, PageRankOptions, ProjectionKind,
};

fn public_projection() -> GraphProjection {
    GraphProjection::from_edges(
        ProjectionKind::External,
        (10..15).map(NodeId::new),
        [
            (NodeId::new(10), NodeId::new(11), 1.0),
            (NodeId::new(10), NodeId::new(12), 1.0),
            (NodeId::new(11), NodeId::new(13), 1.0),
            (NodeId::new(12), NodeId::new(13), 1.0),
            (NodeId::new(13), NodeId::new(14), 1.0),
        ],
    )
}

#[test]
fn feature_free_crate_root_centrality_pipeline_is_aligned_and_read_only() {
    let graph = public_projection();
    let before = graph.clone();
    let page_options = PageRankOptions {
        max_iterations: 1_000,
        tolerance: 1.0e-12,
        ..PageRankOptions::default()
    };
    let iteration_options = IterationOptions {
        max_iterations: 1_000,
        tolerance: 1.0e-12,
    };
    let budget = CentralityBudget::default();

    let scalar_results = [
        pagerank(&graph, page_options),
        reverse_pagerank(&graph, page_options),
        personalized_pagerank(&graph, &[(NodeId::new(10), 1.0)], page_options)
            .expect("public projection seed belongs to the graph"),
        betweenness_centrality(&graph, BetweennessOptions::default())
            .expect("public projection is within the Brandes budget"),
        eigenvector_centrality(&graph, iteration_options),
        katz_centrality(
            &graph,
            KatzOptions {
                max_iterations: 1_000,
                tolerance: 1.0e-12,
                ..KatzOptions::default()
            },
        ),
        closeness_centrality(&graph, budget)
            .expect("public projection is within the closeness budget"),
        harmonic_centrality(&graph, budget)
            .expect("public projection is within the harmonic budget"),
        burt_constraint(&graph, budget).expect("public projection is within the Burt budget"),
    ];
    for result in scalar_results {
        assert_eq!(result.nodes, graph.nodes);
        assert_eq!(result.scores.len(), graph.node_count());
        assert!(result.scores.iter().all(|score| score.is_finite()));
    }

    let hits = hits(&graph, iteration_options);
    assert_eq!(hits.hubs.nodes, graph.nodes);
    assert_eq!(hits.authorities.nodes, graph.nodes);
    let edges = edge_betweenness(&graph, budget)
        .expect("public projection is within the edge-Brandes budget");
    assert!(edges.edges.windows(2).all(|pair| pair[0] < pair[1]));
    assert_eq!(edges.edges.len(), edges.scores.len());

    let dominators = dominator_tree(&graph, NodeId::new(10))
        .expect("public projection root belongs to the graph");
    assert!(dominators.dominates(NodeId::new(10), NodeId::new(14)));
    assert_eq!(graph, before);
}

#[test]
fn public_boundaries_reject_unknown_and_malformed_inputs_without_panicking() {
    let graph = public_projection();
    assert!(personalized_pagerank(
        &graph,
        &[(NodeId::new(99), 1.0)],
        PageRankOptions::default()
    )
    .is_err());
    assert!(dominator_tree(&graph, NodeId::new(99)).is_err());

    let mut malformed = graph;
    malformed.rev_offsets.pop();
    assert!(!pagerank(&malformed, PageRankOptions::default()).converged);
    assert!(betweenness_centrality(&malformed, BetweennessOptions::default()).is_err());
    assert!(edge_betweenness(&malformed, CentralityBudget::default()).is_err());
    assert!(closeness_centrality(&malformed, CentralityBudget::default()).is_err());
}
