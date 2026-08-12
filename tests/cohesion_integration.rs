//! Public-API integration tests for deterministic cohesion analyses.

use libcpg::{
    attack_simulation, cut_vertices_and_bridges, degree_assortativity, global_min_cut,
    k_core_decomposition, k_truss_decomposition, two_edge_connected_components, GraphProjection,
    NodeId, ProjectionKind,
};

fn bridged_triangles() -> GraphProjection {
    GraphProjection::from_edges(
        ProjectionKind::External,
        [10, 20, 30, 40, 50, 60].into_iter().map(NodeId::new),
        [
            (NodeId::new(10), NodeId::new(20), 1.0),
            (NodeId::new(20), NodeId::new(30), 1.0),
            (NodeId::new(30), NodeId::new(10), 1.0),
            (NodeId::new(30), NodeId::new(40), 0.25),
            (NodeId::new(40), NodeId::new(30), 0.75),
            (NodeId::new(40), NodeId::new(50), 1.0),
            (NodeId::new(50), NodeId::new(60), 1.0),
            (NodeId::new(60), NodeId::new(40), 1.0),
        ],
    )
}

#[test]
fn feature_free_root_api_is_deterministic_aligned_and_read_only() {
    let graph = bridged_triangles();
    let before = graph.clone();

    let core = k_core_decomposition(&graph).expect("valid external projection");
    assert_eq!(core.nodes, graph.nodes);
    assert_eq!(core.coreness, vec![2; 6]);
    assert_eq!(core.max_core, 2);
    assert_eq!(core, k_core_decomposition(&graph).expect("repeat core"));

    let truss = k_truss_decomposition(&graph).expect("small external projection");
    assert_eq!(truss.edges.len(), 7);
    assert_eq!(truss.max_truss, 3);
    assert_eq!(
        truss
            .edges
            .iter()
            .find(|edge| edge.source == NodeId::new(30) && edge.target == NodeId::new(40))
            .expect("bridge edge")
            .trussness,
        2
    );

    let cuts = cut_vertices_and_bridges(&graph).expect("valid external projection");
    assert_eq!(
        cuts.articulation_points,
        vec![NodeId::new(30), NodeId::new(40)]
    );
    assert_eq!(cuts.bridges, vec![(NodeId::new(30), NodeId::new(40))]);

    let components = two_edge_connected_components(&graph).expect("valid external projection");
    assert_eq!(components.component, vec![0, 0, 0, 1, 1, 1]);
    assert_eq!(components.components.len(), 2);

    let minimum = global_min_cut(&graph)
        .expect("finite nonnegative weights")
        .expect("six-node projection");
    assert!((minimum.weight - 1.0).abs() <= 1.0e-12);
    assert_eq!(
        minimum.partition,
        vec![NodeId::new(10), NodeId::new(20), NodeId::new(30)]
    );

    let coefficient = degree_assortativity(&graph).expect("valid external projection");
    assert!((-1.0..=1.0).contains(&coefficient));
    let attack = attack_simulation(&graph, &[NodeId::new(30), NodeId::new(40)], 2)
        .expect("distinct projected removal order");
    assert_eq!(attack.trace[0].largest_component, 3);
    assert_eq!(attack.trace[1].largest_component, 2);
    assert!(!attack.truncated);
    assert_eq!(graph, before);
}

#[cfg(feature = "lang-rust")]
#[test]
fn parsed_call_graph_exposes_the_same_two_edge_connected_blocks() {
    use libcpg::{
        call_graph_projection, CpgBuilder, CpgBuilderConfig, Language, TreeSitterCpgBuilder,
    };

    let source = r#"
        fn a() { b(); }
        fn b() { c(); }
        fn c() { a(); d(); }
        fn d() { e(); }
        fn e() { f(); }
        fn f() { d(); }
    "#;
    let cpg =
        TreeSitterCpgBuilder::with_config(CpgBuilderConfig::default().with_import_resolution(true))
            .build(source, Language::Rust)
            .expect("build and resolve Rust call targets");
    let shape = (cpg.node_count(), cpg.edge_count());
    let graph = call_graph_projection(&cpg);

    let id = |name: &str| {
        cpg.functions()
            .find(|node| node.name() == Some(name))
            .map(|node| node.id)
            .expect("named parsed function")
    };
    let cuts = cut_vertices_and_bridges(&graph).expect("resolved call projection");
    assert_eq!(cuts.articulation_points, vec![id("c"), id("d")]);
    assert_eq!(cuts.bridges, vec![(id("c"), id("d"))]);

    let components = two_edge_connected_components(&graph).expect("resolved call projection");
    let component_of = |function: NodeId| {
        components.component[graph.dense_index(function).expect("projected function") as usize]
    };
    assert_eq!(component_of(id("a")), component_of(id("b")));
    assert_eq!(component_of(id("b")), component_of(id("c")));
    assert_eq!(component_of(id("d")), component_of(id("e")));
    assert_eq!(component_of(id("e")), component_of(id("f")));
    assert_ne!(component_of(id("a")), component_of(id("d")));
    assert_eq!(
        global_min_cut(&graph)
            .expect("unit call weights")
            .expect("six functions")
            .weight,
        1.0
    );
    assert_eq!((cpg.node_count(), cpg.edge_count()), shape);
}
