//! Malformed-input hardening: every analysis must **terminate without
//! panicking** on a structurally corrupt graph.
//!
//! `CodePropertyGraph` is an open data structure — `add_node`, `connect`, and
//! `node_mut` are all public, and `connect` deliberately does *not* maintain
//! `node.parent` / `node.children` (the caller does). A consumer that builds a
//! graph by hand, or a language frontend with a bug, can therefore hand the
//! analyses input that no well-formed builder would produce: a parent pointer
//! to a node that does not exist, a cyclic `AstChild` chain, a call whose
//! target was never added, a function with no body.
//!
//! The library's contract for such input is *robustness*, not correctness: an
//! analysis may return a meaningless answer, but it may not panic, loop
//! forever, or overflow the stack (see `docs/security/01-input-and-resource-
//! hardening.md`). These tests assert exactly that contract across the whole
//! public analysis surface, using only the public API — as a downstream
//! consumer would.
//!
//! The cyclic cases are the regression tests for the visited-set guards added
//! to `ast_descendants` / `ast_ancestors` / `ast_depth`; before those guards a
//! cyclic `AstChild` edge sent every ancestor walk into an infinite loop.

use libcpg::{
    BitSetDomain, BlockCfg, BlockId, CodePropertyGraph, Confluence, CpgEdgeKind, CpgNode,
    CpgNodeKind, DataflowAnalysis, Direction, Language, MethodSignature, NodeId, ScopeId,
    SourceRange, Visibility,
};
use proptest::prelude::*;

/// A node id that is guaranteed not to be in any graph these tests build.
fn dangling() -> NodeId {
    NodeId::new(9_999)
}

fn signature(name: &str) -> MethodSignature {
    MethodSignature {
        name: name.into(),
        params: Default::default(),
        return_type: None,
        is_static: false,
        is_async: false,
        visibility: Visibility::Public,
    }
}

fn add(cpg: &mut CodePropertyGraph, kind: CpgNodeKind) -> NodeId {
    cpg.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()))
}

/// Adds `kind` as a *well-formed* AST child of `parent` (edge + parent pointer
/// + child list), mirroring what a real builder does.
fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let id = add(cpg, kind);
    cpg.connect(parent, id, CpgEdgeKind::AstChild);
    if let Some(n) = cpg.node_mut(id) {
        n.parent = Some(parent);
    }
    if let Some(p) = cpg.node_mut(parent) {
        p.children.push(id);
    }
    id
}

struct RobustBlockReach;

impl DataflowAnalysis for RobustBlockReach {
    type Domain = BitSetDomain;

    const NAME: &'static str = "robust-block-reach";
    const DIRECTION: Direction = Direction::Forward;
    const CONFLUENCE: Confluence = Confluence::May;

    fn bottom(&self, cfg: &BlockCfg) -> Self::Domain {
        BitSetDomain::new(cfg.blocks.len())
    }

    fn boundary(&self, cfg: &BlockCfg) -> Self::Domain {
        BitSetDomain::new(cfg.blocks.len())
    }

    fn transfer_block(
        &self,
        _cpg: &CodePropertyGraph,
        _cfg: &BlockCfg,
        block: BlockId,
        state: &mut Self::Domain,
    ) {
        state.insert(block.as_u32());
    }
}

/// The ways a CPG can be structurally corrupt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Corruption {
    /// A node's `parent` points at an id that is not in the graph.
    DanglingParent,
    /// A node's `children` list names an id that is not in the graph.
    DanglingChild,
    /// A `Call` resolves to a callee that is not in the graph.
    DanglingCallTarget,
    /// An `Identifier` names a definition that is not in the graph.
    DanglingDefinition,
    /// A node is its own AST parent (a one-node cycle).
    SelfParent,
    /// Two nodes are each other's AST parent (a two-node cycle).
    ParentCycle,
    /// An `AstChild` edge closes a cycle back to an ancestor.
    AstChildCycle,
    /// A `Function` with no body at all.
    BodylessFunction,
    /// A `Block` that claims a child it is not connected to.
    ChildWithoutEdge,
    /// An `AstChild` edge whose endpoints have no parent/child pointers.
    EdgeWithoutPointers,
}

fn arb_corruption() -> impl Strategy<Value = Corruption> {
    prop_oneof![
        Just(Corruption::DanglingParent),
        Just(Corruption::DanglingChild),
        Just(Corruption::DanglingCallTarget),
        Just(Corruption::DanglingDefinition),
        Just(Corruption::SelfParent),
        Just(Corruption::ParentCycle),
        Just(Corruption::AstChildCycle),
        Just(Corruption::BodylessFunction),
        Just(Corruption::ChildWithoutEdge),
        Just(Corruption::EdgeWithoutPointers),
    ]
}

/// Builds a small function graph and injects `how` into it.
///
/// The base shape is always a plausible function — `Function → Block →
/// {While → {Block → …}, Call, Return}` — so the analyses get far enough to
/// reach the defensive paths before meeting the corruption.
fn corrupt_graph(how: Corruption) -> (CodePropertyGraph, NodeId) {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let func = add(
        &mut cpg,
        CpgNodeKind::Function {
            signature: signature("f"),
        },
    );

    if how == Corruption::BodylessFunction {
        return (cpg, func);
    }

    let body = child(
        &mut cpg,
        func,
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        },
    );
    let loop_node = child(&mut cpg, body, CpgNodeKind::While);
    child(
        &mut cpg,
        loop_node,
        CpgNodeKind::BinaryOp {
            operator: "<".into(),
        },
    );
    let loop_body = child(
        &mut cpg,
        loop_node,
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        },
    );
    let var = child(
        &mut cpg,
        loop_body,
        CpgNodeKind::Variable {
            name: "x".into(),
            var_type: None,
            scope: ScopeId::GLOBAL,
            is_mutable: true,
        },
    );
    let call = child(
        &mut cpg,
        loop_body,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    let ident = child(
        &mut cpg,
        call,
        CpgNodeKind::Identifier {
            name: "x".into(),
            definition: None,
        },
    );
    child(&mut cpg, body, CpgNodeKind::Return);

    match how {
        Corruption::DanglingParent => {
            cpg.node_mut(var).expect("var").parent = Some(dangling());
        }
        Corruption::DanglingChild => {
            cpg.node_mut(loop_body)
                .expect("loop body")
                .children
                .push(dangling());
        }
        Corruption::DanglingCallTarget => {
            cpg.node_mut(call).expect("call").kind = CpgNodeKind::Call {
                target: Some(dangling()),
                is_method: false,
            };
        }
        Corruption::DanglingDefinition => {
            cpg.node_mut(ident).expect("ident").kind = CpgNodeKind::Identifier {
                name: "x".into(),
                definition: Some(dangling()),
            };
        }
        Corruption::SelfParent => {
            cpg.node_mut(var).expect("var").parent = Some(var);
            cpg.node_mut(var).expect("var").children.push(var);
            cpg.connect(var, var, CpgEdgeKind::AstChild);
        }
        Corruption::ParentCycle => {
            cpg.node_mut(func).expect("func").parent = Some(body);
            cpg.node_mut(body).expect("body").parent = Some(func);
        }
        Corruption::AstChildCycle => {
            // The innermost block claims the function as its child.
            cpg.connect(loop_body, func, CpgEdgeKind::AstChild);
            cpg.node_mut(loop_body)
                .expect("loop body")
                .children
                .push(func);
        }
        Corruption::ChildWithoutEdge => {
            let orphan = add(&mut cpg, CpgNodeKind::Return);
            cpg.node_mut(body).expect("body").children.push(orphan);
        }
        Corruption::EdgeWithoutPointers => {
            let floating = add(&mut cpg, CpgNodeKind::Return);
            cpg.connect(body, floating, CpgEdgeKind::AstChild);
        }
        Corruption::BodylessFunction => unreachable!("handled above"),
    }

    (cpg, func)
}

/// Runs the full public analysis surface over `cpg`, asserting the invariants
/// that must survive corruption. Returns nothing — the point is that it
/// returns *at all*.
fn exercise_every_analysis(cpg: &CodePropertyGraph, func: NodeId) {
    // --- graph-level queries ---
    let stats = cpg.stats();
    assert_eq!(stats.node_count, cpg.node_count());
    assert_eq!(stats.edge_count, cpg.edge_count());
    assert!(cpg.cyclomatic_complexity() >= 1, "complexity is at least 1");
    assert!(
        cpg.ast_depth() <= cpg.node_count(),
        "the AST depth walk is bounded by the graph size (visited-set guard)"
    );
    let descendants = cpg.ast_descendants(func);
    assert!(
        descendants.len() <= cpg.node_count(),
        "a visited set bounds the descendant walk by the graph size"
    );
    for id in cpg.node_ids() {
        // Ancestor walks terminate and stay inside the graph.
        for anc in cpg.ast_ancestors(id) {
            assert!(cpg.node(anc).is_some(), "ancestors are real nodes");
        }
        let _ = cpg.ast_children(id);
        let _ = cpg.cfg_successors(id);
        let _ = cpg.cfg_predecessors(id);
    }
    let _ = cpg.subgraph(&descendants);

    // --- exact SCC decomposition ---
    let cfg_sccs = libcpg::control_flow_sccs(cpg, func).expect("func remains a function");
    assert!(cfg_sccs.node_count() <= cpg.node_count());
    let call_sccs = libcpg::call_graph_sccs(cpg);
    assert_eq!(call_sccs.node_count(), cpg.functions().count());
    for decomposition in [&cfg_sccs, &call_sccs] {
        assert_eq!(
            decomposition
                .components
                .iter()
                .map(|component| component.nodes.len())
                .sum::<usize>(),
            decomposition.node_count(),
            "every projected node belongs to exactly one SCC"
        );
        assert!(
            decomposition
                .components
                .iter()
                .flat_map(|component| &component.nodes)
                .all(|node| cpg.node(*node).is_some()),
            "SCCs contain only real nodes"
        );
        assert!(
            decomposition
                .condensation_edges
                .iter()
                .all(|edge| edge.source != edge.target),
            "condensation edges never contain self-loops"
        );
        let metrics = libcpg::condensation_metrics(
            decomposition,
            &libcpg::CondensationOptions {
                coffman_graham_width: Some(2),
                ..libcpg::CondensationOptions::default()
            },
        );
        assert!(metrics.complete, "SCC output is a valid condensation");
        assert_eq!(metrics.levels.len(), decomposition.components.len());
        let layering = libcpg::coffman_graham(decomposition, 2)
            .expect("SCC output has a valid bounded layering");
        assert_eq!(layering.layer_of.len(), decomposition.components.len());
    }
    let call_projection = libcpg::call_graph_projection(cpg);
    let projected_sccs = libcpg::projection_sccs(&call_projection);
    assert_eq!(projected_sccs, call_sccs);
    let dsm = libcpg::analyze_dsm(&call_projection);
    assert!(dsm.complete);
    assert_eq!(dsm.visibility_fan_in.len(), call_projection.node_count());
    assert_eq!(dsm.visibility_fan_out.len(), call_projection.node_count());
    let core_periphery = libcpg::classify_core_periphery(&call_projection, &dsm);
    assert!(core_periphery.complete);
    assert_eq!(core_periphery.classes.len(), call_projection.node_count());
    assert!(core_periphery
        .cyclic_core
        .iter()
        .all(|&dense| dense < call_projection.node_count()));
    let wl_labels = libcpg::wl_labels(&call_projection, 3);
    assert_eq!(wl_labels.len(), call_projection.node_count());
    let clone_classes = libcpg::structural_clone_classes(&call_projection, 3, 2);
    assert!(clone_classes.iter().all(|class| {
        class.len() >= 2
            && class.windows(2).all(|pair| pair[0] < pair[1])
            && class
                .iter()
                .all(|&node| call_projection.dense_index(node).is_some())
    }));
    let page_options = libcpg::PageRankOptions::default();
    let iteration_options = libcpg::IterationOptions::default();
    let budget = libcpg::CentralityBudget::default();
    let mut centralities = vec![
        libcpg::pagerank(&call_projection, page_options),
        libcpg::reverse_pagerank(&call_projection, page_options),
        libcpg::betweenness_centrality(&call_projection, libcpg::BetweennessOptions::default())
            .expect("call projection fits the default betweenness budget"),
        libcpg::eigenvector_centrality(&call_projection, iteration_options),
        libcpg::katz_centrality(&call_projection, libcpg::KatzOptions::default()),
        libcpg::closeness_centrality(&call_projection, budget)
            .expect("call projection fits the default closeness budget"),
        libcpg::harmonic_centrality(&call_projection, budget)
            .expect("call projection fits the default harmonic budget"),
        libcpg::burt_constraint(&call_projection, budget)
            .expect("call projection fits the default constraint budget"),
    ];
    if let Some(&seed) = call_projection.nodes.first() {
        centralities.push(
            libcpg::personalized_pagerank(&call_projection, &[(seed, 1.0)], page_options)
                .expect("the seed comes from the projection"),
        );
    }
    let hits = libcpg::hits(&call_projection, iteration_options);
    centralities.extend([hits.hubs, hits.authorities]);
    for centrality in centralities {
        assert_eq!(centrality.nodes, call_projection.nodes);
        assert_eq!(centrality.scores.len(), call_projection.node_count());
        assert!(centrality.scores.iter().all(|score| score.is_finite()));
    }
    let edge_centrality = libcpg::edge_betweenness(&call_projection, budget)
        .expect("call projection fits the default edge-betweenness budget");
    assert_eq!(edge_centrality.edges.len(), edge_centrality.scores.len());
    let communities = libcpg::louvain(&call_projection, libcpg::LouvainOptions::default())
        .expect("call projection has finite nonnegative weights");
    assert_eq!(communities.nodes, call_projection.nodes);
    assert_eq!(communities.community.len(), call_projection.node_count());
    assert!(communities.modularity.is_finite());
    let refined = libcpg::refine_connected(&call_projection, &communities);
    assert_eq!(refined.nodes, call_projection.nodes);
    assert_eq!(refined.community.len(), call_projection.node_count());
    assert!(refined.modularity.is_finite());
    let core = libcpg::k_core_decomposition(&call_projection)
        .expect("call projection is structurally valid");
    assert_eq!(core.nodes, call_projection.nodes);
    assert_eq!(core.coreness.len(), call_projection.node_count());
    let truss = libcpg::k_truss_decomposition(&call_projection)
        .expect("corrupt fixture call graph stays below the edge cap");
    assert!(truss.edges.iter().all(|edge| {
        call_projection.dense_index(edge.source).is_some()
            && call_projection.dense_index(edge.target).is_some()
            && edge.source < edge.target
    }));
    let cuts = libcpg::cut_vertices_and_bridges(&call_projection)
        .expect("call projection is structurally valid");
    assert!(cuts
        .articulation_points
        .iter()
        .all(|&node| call_projection.dense_index(node).is_some()));
    assert!(cuts.bridges.iter().all(|&(left, right)| {
        left < right
            && call_projection.dense_index(left).is_some()
            && call_projection.dense_index(right).is_some()
    }));
    let edge_components = libcpg::two_edge_connected_components(&call_projection)
        .expect("call projection is structurally valid");
    assert_eq!(edge_components.nodes, call_projection.nodes);
    assert_eq!(
        edge_components.component.len(),
        call_projection.node_count()
    );
    assert_eq!(
        edge_components
            .components
            .iter()
            .map(Vec::len)
            .sum::<usize>(),
        call_projection.node_count()
    );
    let minimum_cut =
        libcpg::global_min_cut(&call_projection).expect("call projection has finite unit weights");
    assert!(minimum_cut
        .as_ref()
        .is_none_or(|cut| cut.weight.is_finite() && cut.weight >= 0.0));
    let assortativity = libcpg::degree_assortativity(&call_projection)
        .expect("call projection is structurally valid");
    assert!((-1.0..=1.0).contains(&assortativity));
    let attack = libcpg::attack_simulation(
        &call_projection,
        &call_projection.nodes,
        call_projection.node_count(),
    )
    .expect("call nodes form a unique projected removal order");
    assert_eq!(attack.trace.len(), call_projection.node_count());
    assert!(attack.auc.is_finite() && (0.0..=1.0).contains(&attack.auc));
    let spectral =
        libcpg::try_algebraic_connectivity(&call_projection, &libcpg::SpectralOptions::default())
            .expect("call projection is structurally valid");
    if let Some(spectral) = spectral {
        assert_eq!(spectral.nodes, call_projection.nodes);
        assert_eq!(spectral.fiedler.len(), call_projection.node_count());
        assert_eq!(spectral.partition.len(), call_projection.node_count());
        assert!(spectral.algebraic_connectivity.is_finite());
        assert!(spectral.algebraic_connectivity >= 0.0);
        assert!(spectral.residual_norm.is_finite());
        assert!(spectral.iterations <= libcpg::MAX_SPECTRAL_ITERATIONS);
        assert!(spectral.starts <= libcpg::MAX_SPECTRAL_STARTS);
    }

    // --- extractors ---
    let mut work = cpg.clone();
    libcpg::CfgExtractor::new().extract(&mut work);
    let reachability = libcpg::unreachable_code(&work, func).expect("func remains a function");
    assert!(reachability
        .reachable
        .iter()
        .chain(&reachability.unreachable)
        .all(|node| work.node(*node).is_some()));
    assert!(reachability
        .reachable
        .iter()
        .all(|node| reachability.unreachable.binary_search(node).is_err()));
    let blocks = libcpg::block_cfg(&work, func).expect("func remains a function");
    assert_eq!(
        blocks.block_of_node.len(),
        blocks.blocks.iter().map(|b| b.nodes.len()).sum::<usize>()
    );
    assert!(blocks
        .blocks
        .iter()
        .all(|block| block.nodes.first() == Some(&block.leader)));
    assert_eq!(blocks.reverse_post_order().len(), blocks.blocks.len());
    let loops = libcpg::loop_forest(&work, func).expect("func remains a function");
    assert!(loops.max_depth() <= loops.loops.len());
    for natural_loop in &loops.loops {
        assert!(
            natural_loop
                .body
                .iter()
                .all(|node| work.node(*node).is_some()),
            "loop bodies contain only real nodes"
        );
        if let Some(parent) = natural_loop.parent {
            assert!(natural_loop.body.len() < loops.loops[parent].body.len());
        }
    }
    let variables = libcpg::variable_facts(&work, func);
    let expressions = libcpg::expression_facts(&work, func);
    assert!(variables.universe.len() <= work.node_count());
    assert!(expressions.universe.len() <= work.node_count());
    let solution = libcpg::solve(
        &work,
        &blocks,
        &RobustBlockReach,
        &libcpg::SolverConfig::default(),
        Some(&loops),
    )
    .expect("finite bit-set dataflow terminates on corrupt input");
    assert_eq!(solution.before.len(), blocks.blocks.len());
    assert_eq!(solution.after.len(), blocks.blocks.len());
    assert_eq!(solution.visits.len(), blocks.blocks.len());
    let liveness = libcpg::liveness(&work, func).expect("liveness remains total");
    let dead_stores = libcpg::dead_stores(&work, func).expect("dead stores remain total");
    let available = libcpg::available_expressions(&work, func).expect("availability remains total");
    let very_busy =
        libcpg::very_busy_expressions(&work, func).expect("anticipability remains total");
    assert!(liveness.live_in.len() <= work.node_count());
    assert!(liveness.live_out.len() <= work.node_count());
    assert!(available.available_in.len() <= work.node_count());
    assert!(very_busy.busy_out.len() <= work.node_count());
    assert!(dead_stores
        .dead
        .iter()
        .all(|store| work.node(store.node).is_some()));
    let first_resolution = libcpg::resolve_calls(&mut work, &libcpg::ResolveConfig::default());
    libcpg::DfgExtractor::new().extract(&mut work);
    let sparse_constants = libcpg::sccp(&work, func).expect("SCCP remains total");
    assert!(sparse_constants
        .constants
        .keys()
        .chain(&sparse_constants.unreachable_nodes)
        .all(|node| work.node(*node).is_some()));
    assert!(sparse_constants
        .executable_edges
        .iter()
        .chain(&sparse_constants.dead_edges)
        .all(|(source, target, _)| {
            work.node(*source).is_some() && work.node(*target).is_some()
        }));
    let ranges = libcpg::value_ranges(&work, func).expect("interval analysis remains total");
    assert!(ranges.at_block_entry.iter().all(|(leader, entries)| {
        work.node(*leader).is_some()
            && entries
                .windows(2)
                .all(|pair| pair[0].0.as_ref() < pair[1].0.as_ref())
    }));
    let induction = libcpg::loop_induction(&work, func, &loops)
        .expect("loop induction remains total on the validated forest");
    assert_eq!(induction.invariants.len(), loops.loops.len());
    assert_eq!(induction.bounds.len(), loops.loops.len());
    assert!(induction
        .invariants
        .iter()
        .flat_map(|report| report.invariant_nodes.iter().chain(&report.hoistable))
        .chain(induction.ivs.iter().map(|iv| &iv.update_node))
        .all(|node| work.node(*node).is_some()));
    assert!(induction.invariants.iter().all(|report| report
        .hoistable
        .iter()
        .all(|node| report.invariant_nodes.binary_search(node).is_ok())));
    libcpg::PdgBuilder::new().build(&mut work, func);

    // Re-running is idempotent even on corrupt input.
    let edges_after_first = work.edge_count();
    libcpg::CfgExtractor::new().extract(&mut work);
    let second_resolution = libcpg::resolve_calls(&mut work, &libcpg::ResolveConfig::default());
    libcpg::DfgExtractor::new().extract(&mut work);
    libcpg::PdgBuilder::new().build(&mut work, func);
    assert_eq!(
        second_resolution.resolutions, first_resolution.resolutions,
        "call-resolution evidence stays deterministic on malformed input"
    );
    assert_eq!(
        edges_after_first,
        work.edge_count(),
        "extraction stays idempotent on malformed input"
    );

    // --- slicing ---
    for max in [0usize, 1, 8, 64] {
        let back = libcpg::backward_slice(&work, func, max);
        let fwd = libcpg::forward_slice(&work, func, max);
        assert!(
            back.len() <= max && fwd.len() <= max,
            "slices honor their budget"
        );
        for id in back.iter().chain(fwd.iter()) {
            assert!(work.node(*id).is_some(), "slices contain only real nodes");
        }
    }

    // --- matching and similarity ---
    #[cfg(feature = "lang-rust")]
    {
        use libcpg::pattern::{GraphSimilarity, SimilarityMetric, SubgraphMatcher, Vf2Matcher};
        let matches = Vf2Matcher::new().find_matches_limited(cpg, cpg, 4);
        assert!(matches.len() <= 4);
        for metric in [
            SimilarityMetric::Jaccard,
            SimilarityMetric::Cosine,
            SimilarityMetric::GraphEdit,
            SimilarityMetric::WeisfeilerLehman,
        ] {
            let s = GraphSimilarity::new()
                .with_metric(metric)
                .similarity(cpg, cpg);
            assert!(s.is_finite(), "{metric:?} similarity is finite");
            assert!(
                (0.0..=1.0).contains(&s),
                "{metric:?} similarity in [0,1], got {s}"
            );
        }
    }

    // --- gated analyses ---
    #[cfg(feature = "algorithm-detection")]
    {
        use libcpg::algorithms::AlgorithmDetector;
        let found =
            libcpg::algorithms::detection::DefaultAlgorithmDetector::new().detect(cpg, func);
        for d in &found {
            assert!(d.confidence.is_finite() && (0.0..=1.0).contains(&d.confidence));
        }
    }
    #[cfg(feature = "design-patterns")]
    {
        use libcpg::patterns::design::{GofPatternDetector, PatternDetector};
        for m in GofPatternDetector::new().detect(cpg) {
            assert!(m.confidence.is_finite() && (0.0..=1.0).contains(&m.confidence));
        }
    }
    #[cfg(feature = "gnn")]
    {
        use libcpg::gnn::{CpgGnn, GraphNeuralNetwork};
        let mut gnn = CpgGnn::new(cpg.clone())
            .with_embedding_dim(4)
            .with_num_layers(1);
        gnn.propagate(2);
        for id in cpg.node_ids() {
            if let Some(e) = gnn.node_embedding(id) {
                assert!(e.iter().all(|v| v.is_finite()), "embeddings stay finite");
            }
        }
    }
    #[cfg(feature = "serde")]
    {
        let json = serde_json::to_string(cpg).expect("a corrupt graph still serializes");
        let back: CodePropertyGraph = serde_json::from_str(&json).expect("and deserializes");
        assert_eq!(back.node_count(), cpg.node_count());
        assert_eq!(back.edge_count(), cpg.edge_count());
    }
}

#[test]
fn every_corruption_is_survived_by_every_analysis() {
    for how in [
        Corruption::DanglingParent,
        Corruption::DanglingChild,
        Corruption::DanglingCallTarget,
        Corruption::DanglingDefinition,
        Corruption::SelfParent,
        Corruption::ParentCycle,
        Corruption::AstChildCycle,
        Corruption::BodylessFunction,
        Corruption::ChildWithoutEdge,
        Corruption::EdgeWithoutPointers,
    ] {
        let (cpg, func) = corrupt_graph(how);
        exercise_every_analysis(&cpg, func);
    }
}

/// Analyses over a node id that is not in the graph at all are inert.
#[test]
fn an_absent_node_id_is_inert_everywhere() {
    let (cpg, _) = corrupt_graph(Corruption::DanglingParent);

    assert!(cpg.node(dangling()).is_none());
    assert!(cpg.ast_children(dangling()).is_empty());
    assert!(cpg.ast_descendants(dangling()).is_empty());
    assert!(cpg.ast_ancestors(dangling()).is_empty());
    assert!(cpg.cfg_successors(dangling()).is_empty());
    assert!(cpg.cfg_predecessors(dangling()).is_empty());
    assert_eq!(
        libcpg::control_flow_sccs(&cpg, dangling()),
        Err(libcpg::SccAnalysisError::UnknownNode(dangling()))
    );
    assert_eq!(
        libcpg::loop_forest(&cpg, dangling()),
        Err(libcpg::AnalysisError::UnknownNode(dangling()))
    );
    assert_eq!(
        libcpg::unreachable_code(&cpg, dangling()),
        Err(libcpg::AnalysisError::UnknownNode(dangling()))
    );
    assert_eq!(
        libcpg::block_cfg(&cpg, dangling()),
        Err(libcpg::AnalysisError::UnknownNode(dangling()))
    );
    assert_eq!(
        libcpg::available_expressions(&cpg, dangling()),
        Err(libcpg::AnalysisError::UnknownNode(dangling()))
    );
    assert_eq!(
        libcpg::very_busy_expressions(&cpg, dangling()),
        Err(libcpg::AnalysisError::UnknownNode(dangling()))
    );
    assert_eq!(
        libcpg::sccp(&cpg, dangling()),
        Err(libcpg::AnalysisError::UnknownNode(dangling()))
    );
    assert_eq!(
        libcpg::value_ranges(&cpg, dangling()),
        Err(libcpg::AnalysisError::UnknownNode(dangling()))
    );
    assert_eq!(
        libcpg::loop_induction(&cpg, dangling(), &libcpg::LoopForest::default()),
        Err(libcpg::AnalysisError::UnknownNode(dangling()))
    );

    let mut work = cpg.clone();
    libcpg::CfgExtractor::new().extract_function_cfg(&mut work, dangling());
    libcpg::PdgBuilder::new().build(&mut work, dangling());
    assert!(libcpg::backward_slice(&work, dangling(), 8).len() <= 8);
    assert!(libcpg::forward_slice(&work, dangling(), 8).len() <= 8);
}

/// An empty graph is a degenerate but legal input.
#[test]
fn the_empty_graph_is_analyzable() {
    let cpg = CodePropertyGraph::new(Language::Unknown);
    assert_eq!(cpg.node_count(), 0);
    assert_eq!(cpg.cyclomatic_complexity(), 1, "E − N + 2 with no nodes");
    assert_eq!(cpg.functions().count(), 0);
    let stats = cpg.stats();
    assert_eq!(stats.node_count, 0);
    assert_eq!(stats.edge_count, 0);
    let sccs = libcpg::call_graph_sccs(&cpg);
    assert_eq!(sccs.node_count(), 0);
    assert!(sccs.is_acyclic());
    let metrics = libcpg::condensation_metrics(&sccs, &libcpg::CondensationOptions::default());
    assert!(metrics.complete);
    assert_eq!(metrics.height, 0);
    assert!(libcpg::coffman_graham(&sccs, 1)
        .expect("empty condensation is layerable")
        .layers
        .is_empty());

    let mut work = cpg.clone();
    libcpg::CfgExtractor::new().extract(&mut work);
    let resolution = libcpg::resolve_calls(&mut work, &libcpg::ResolveConfig::default());
    libcpg::DfgExtractor::new().extract(&mut work);
    assert!(resolution.resolutions.is_empty());
    assert_eq!(work.node_count(), 0, "extraction adds no nodes");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Whatever corruption is injected, the whole analysis surface terminates
    /// and every invariant above holds.
    #[test]
    fn prop_analyses_survive_arbitrary_corruption(how in arb_corruption()) {
        let (cpg, func) = corrupt_graph(how);
        exercise_every_analysis(&cpg, func);
    }

    /// Depth is bounded by the node count even when parent pointers cycle —
    /// the visited-set guard, stated as a property.
    #[test]
    fn prop_ast_depth_is_bounded_by_node_count(how in arb_corruption()) {
        let (cpg, _) = corrupt_graph(how);
        prop_assert!(
            cpg.ast_depth() <= cpg.node_count(),
            "depth {} exceeded the node count {}", cpg.ast_depth(), cpg.node_count()
        );
    }
}
