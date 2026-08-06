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
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, Language, MethodSignature, NodeId,
    ScopeId, SourceRange, Visibility,
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
    }

    // --- extractors ---
    let mut work = cpg.clone();
    libcpg::CfgExtractor::new().extract(&mut work);
    libcpg::DfgExtractor::new().extract(&mut work);
    libcpg::PdgBuilder::new().build(&mut work, func);

    // Re-running is idempotent even on corrupt input.
    let edges_after_first = work.edge_count();
    libcpg::CfgExtractor::new().extract(&mut work);
    libcpg::DfgExtractor::new().extract(&mut work);
    libcpg::PdgBuilder::new().build(&mut work, func);
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

    let mut work = cpg.clone();
    libcpg::CfgExtractor::new().extract(&mut work);
    libcpg::DfgExtractor::new().extract(&mut work);
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
