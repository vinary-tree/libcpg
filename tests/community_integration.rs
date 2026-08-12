//! Public-API integration tests for deterministic community detection.

use libcpg::{
    louvain, refine_connected, try_refine_connected, CommunityAssignment, CommunityKind,
    GraphProjection, LouvainOptions, NodeId, ProjectionKind,
};

fn two_clusters() -> GraphProjection {
    GraphProjection::from_edges(
        ProjectionKind::External,
        (10..16).map(NodeId::new),
        [
            (NodeId::new(10), NodeId::new(11), 1.0),
            (NodeId::new(11), NodeId::new(12), 1.0),
            (NodeId::new(12), NodeId::new(10), 1.0),
            (NodeId::new(13), NodeId::new(14), 1.0),
            (NodeId::new(14), NodeId::new(15), 1.0),
            (NodeId::new(15), NodeId::new(13), 1.0),
            (NodeId::new(12), NodeId::new(13), 0.1),
        ],
    )
}

#[test]
fn feature_free_root_api_is_aligned_deterministic_and_read_only() {
    let graph = two_clusters();
    let before = graph.clone();
    let first = louvain(&graph, LouvainOptions::default()).expect("finite weighted projection");
    let second = louvain(&graph, LouvainOptions::default()).expect("repeat projection");

    assert_eq!(first, second);
    assert_eq!(first.nodes, graph.nodes);
    assert_eq!(first.community, vec![0, 0, 0, 1, 1, 1]);
    assert_eq!(first.num_communities, 2);
    assert!(first.modularity > 0.0);

    let disconnected = CommunityAssignment {
        kind: CommunityKind::Louvain { resolution: 1.0 },
        nodes: graph.nodes.clone(),
        community: vec![0; graph.node_count()],
        num_communities: 1,
        modularity: 0.0,
    };
    let refined = try_refine_connected(&graph, &disconnected).expect("aligned assignment");
    assert_eq!(refined.community, vec![0, 0, 0, 0, 0, 0]);
    assert_eq!(
        refine_connected(&graph, &first),
        first_with_refinement_kind(first)
    );
    assert_eq!(graph, before);
}

fn first_with_refinement_kind(mut assignment: CommunityAssignment) -> CommunityAssignment {
    assignment.kind = CommunityKind::ConnectedRefinement { resolution: 1.0 };
    assignment
}

#[cfg(feature = "lang-rust")]
#[test]
fn parsed_call_graph_recovers_two_function_clusters_without_mutating_the_cpg() {
    use libcpg::{
        call_graph_projection, CpgBuilder, CpgBuilderConfig, Language, TreeSitterCpgBuilder,
    };

    let source = r#"
        fn a() { b(); c(); }
        fn b() { a(); c(); }
        fn c() { a(); b(); d(); }
        fn d() { e(); f(); }
        fn e() { d(); f(); }
        fn f() { d(); e(); }
    "#;
    let cpg =
        TreeSitterCpgBuilder::with_config(CpgBuilderConfig::default().with_import_resolution(true))
            .build(source, Language::Rust)
            .expect("build and resolve the Rust call graph");
    let shape = (cpg.node_count(), cpg.edge_count());
    let projection = call_graph_projection(&cpg);
    let result = louvain(&projection, LouvainOptions::default())
        .expect("call graph projection has finite unit weights");

    let cluster_of = |name: &str| {
        let function = cpg
            .functions()
            .find(|node| node.name() == Some(name))
            .map(|node| node.id)
            .expect("named parsed function");
        result.community[projection
            .dense_index(function)
            .expect("projected function") as usize]
    };
    assert_eq!(cluster_of("a"), cluster_of("b"));
    assert_eq!(cluster_of("b"), cluster_of("c"));
    assert_eq!(cluster_of("d"), cluster_of("e"));
    assert_eq!(cluster_of("e"), cluster_of("f"));
    assert_ne!(cluster_of("a"), cluster_of("d"));
    assert_eq!(result.num_communities, 2);
    assert_eq!((cpg.node_count(), cpg.edge_count()), shape);
}
