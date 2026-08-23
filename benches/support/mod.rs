#![allow(dead_code)]

use libcpg::{
    CallGraphResolution, CfgEdgeKind, CodePropertyGraph, Confidence, CpgEdgeKind, CpgNode,
    CpgNodeKind, DfgEdgeKind, Language, MethodSignature, NodeId, ProjectionKind, ResolveConfig,
    ScopeId, SourceRange, Visibility,
};

/// Stable seed used by every synthetic performance fixture.
pub const FIXTURE_SEED: u64 = 0x6c69_6263_7067_0900;

/// One source file in the parser-backed corpus.
#[derive(Debug, Clone, Copy)]
pub struct CorpusFile {
    pub name: &'static str,
    pub language: Language,
    pub source: &'static str,
}

/// Approximately 50 lines of committed, production Rust.
pub const RUST_SMALL: CorpusFile = CorpusFile {
    name: "rust-small-49",
    language: Language::Rust,
    source: include_str!("../../src/algorithms/families/searching.rs"),
};

/// Approximately 500 lines of committed, production Rust.
pub const RUST_MEDIUM: CorpusFile = CorpusFile {
    name: "rust-medium-514",
    language: Language::Rust,
    source: include_str!("../../src/analysis/bugs/deviant.rs"),
};

/// Approximately 5,000 lines of committed, production Rust, split at real
/// module boundaries so every parser invocation remains a valid source file.
pub const RUST_LARGE: [CorpusFile; 2] = [
    CorpusFile {
        name: "rust-large-node-mapper-4285",
        language: Language::Rust,
        source: include_str!("../../src/builder/node_mapper.rs"),
    },
    CorpusFile {
        name: "rust-large-scc-888",
        language: Language::Rust,
        source: include_str!("../../src/analysis/scc.rs"),
    },
];

/// Small committed cross-language corpus used by `analyze_corpus`.
pub const CROSS_LANGUAGE: [CorpusFile; 4] = [
    CorpusFile {
        name: "python-service",
        language: Language::Python,
        source: include_str!("../corpus/service.py"),
    },
    CorpusFile {
        name: "javascript-service",
        language: Language::JavaScript,
        source: include_str!("../corpus/service.js"),
    },
    CorpusFile {
        name: "go-service",
        language: Language::Go,
        source: include_str!("../corpus/service.go"),
    },
    CorpusFile {
        name: "c-service",
        language: Language::C,
        source: include_str!("../corpus/service.c"),
    },
];

/// Synthetic graph shapes that exercise distinct adjacency and call patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntheticShape {
    DeepAst,
    WideAst,
    ManyFunctions,
    DenseCfg,
    DenseCallGraph,
}

impl SyntheticShape {
    pub const ALL: [Self; 5] = [
        Self::DeepAst,
        Self::WideAst,
        Self::ManyFunctions,
        Self::DenseCfg,
        Self::DenseCallGraph,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::DeepAst => "deep_ast",
            Self::WideAst => "wide_ast",
            Self::ManyFunctions => "many_functions",
            Self::DenseCfg => "dense_cfg",
            Self::DenseCallGraph => "dense_call_graph",
        }
    }
}

fn permutation(node_count: usize, mut state: u64) -> Vec<usize> {
    let mut order: Vec<_> = (0..node_count).collect();
    for index in (1..node_count).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let swap = state as usize % (index + 1);
        order.swap(index, swap);
    }
    order
}

fn function_node(name: String) -> CpgNode {
    CpgNode::new(
        0.into(),
        CpgNodeKind::Function {
            signature: Box::new(MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            }),
        },
        SourceRange::default(),
    )
}

/// Build a deterministic many-function AST for parallel analysis experiments.
///
/// Each function owns a flat block of alternating variable definitions and
/// identifier uses. The shape provides independent CFG, DFG, and PDG work
/// without making source parsing part of the measured interval.
pub fn parallel_analysis_graph(
    function_count: usize,
    statements_per_function: usize,
) -> CodePropertyGraph {
    let mut graph = CodePropertyGraph::new(Language::Rust);
    for function_index in 0..function_count {
        let function = graph.add_node(function_node(format!("parallel_{function_index}")));
        let body = graph.add_node(
            CpgNode::new(
                0.into(),
                CpgNodeKind::Block {
                    scope: ScopeId::GLOBAL,
                },
                SourceRange::default(),
            )
            .with_parent(function),
        );
        graph.connect_unique(function, body, CpgEdgeKind::AstChild);
        for statement_index in 0..statements_per_function {
            let name = format!("v_{function_index}_{statement_index}");
            let definition = graph.add_node(
                CpgNode::new(
                    0.into(),
                    CpgNodeKind::Variable {
                        name: name.clone().into(),
                        var_type: None,
                        is_mutable: true,
                        scope: ScopeId::GLOBAL,
                    },
                    SourceRange::default(),
                )
                .with_parent(body),
            );
            graph.connect_unique(body, definition, CpgEdgeKind::AstChild);
            let use_site = graph.add_node(
                CpgNode::new(
                    0.into(),
                    CpgNodeKind::Identifier {
                        name: name.into(),
                        definition: None,
                    },
                    SourceRange::default(),
                )
                .with_parent(body),
            );
            graph.connect_unique(body, use_site, CpgEdgeKind::AstChild);
        }
    }
    graph
}

/// Generate a deterministic graph from an explicit seed.
pub fn synthetic_graph(shape: SyntheticShape, node_count: usize, seed: u64) -> CodePropertyGraph {
    assert!(
        node_count > 0,
        "synthetic fixtures require at least one node"
    );

    let mut graph = CodePropertyGraph::new(Language::Rust);
    let function_kinds = matches!(
        shape,
        SyntheticShape::ManyFunctions | SyntheticShape::DenseCallGraph
    );
    let nodes: Vec<_> = (0..node_count)
        .map(|index| {
            if function_kinds {
                graph.add_node(function_node(format!("f_{seed:016x}_{index}")))
            } else {
                graph.add_node(CpgNode::new(
                    0.into(),
                    CpgNodeKind::Return,
                    SourceRange::default(),
                ))
            }
        })
        .collect();
    let order = permutation(node_count, seed);

    match shape {
        SyntheticShape::DeepAst => {
            for endpoints in order.windows(2) {
                graph.connect(
                    nodes[endpoints[0]],
                    nodes[endpoints[1]],
                    CpgEdgeKind::AstChild,
                );
            }
        }
        SyntheticShape::WideAst | SyntheticShape::ManyFunctions => {
            for &child in &order[1..] {
                graph.connect(nodes[order[0]], nodes[child], CpgEdgeKind::AstChild);
            }
        }
        SyntheticShape::DenseCfg => {
            for &source in &order {
                for &target in &order {
                    if source != target {
                        graph.connect(
                            nodes[source],
                            nodes[target],
                            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                        );
                    }
                }
            }
        }
        SyntheticShape::DenseCallGraph => {
            for &source in &order {
                for &target in &order {
                    if source != target {
                        graph.connect(nodes[source], nodes[target], CpgEdgeKind::CallSite);
                    }
                }
            }
        }
    }

    graph
}

/// Assert the exact node/edge cardinalities for a generated shape.
pub fn assert_synthetic_graph(graph: &CodePropertyGraph, shape: SyntheticShape, node_count: usize) {
    let expected_edges = match shape {
        SyntheticShape::DeepAst | SyntheticShape::WideAst | SyntheticShape::ManyFunctions => {
            node_count.saturating_sub(1)
        }
        SyntheticShape::DenseCfg | SyntheticShape::DenseCallGraph => {
            node_count.saturating_mul(node_count.saturating_sub(1))
        }
    };
    assert_eq!(graph.node_count(), node_count, "synthetic node oracle");
    assert_eq!(graph.edge_count(), expected_edges, "synthetic edge oracle");

    let function_count = graph.functions().count();
    if matches!(
        shape,
        SyntheticShape::ManyFunctions | SyntheticShape::DenseCallGraph
    ) {
        assert_eq!(function_count, node_count, "synthetic function oracle");
    } else {
        assert_eq!(function_count, 0, "synthetic non-function oracle");
    }
}

/// Graph carrying parallel AST, CFG, and DFG edges for traversal microbenchmarks.
pub fn traversal_graph(node_count: usize) -> (CodePropertyGraph, Vec<NodeId>) {
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
        graph.connect(
            endpoints[0],
            endpoints[1],
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        );
        graph.connect(
            endpoints[0],
            endpoints[1],
            CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
        );
    }
    (graph, nodes)
}

/// Assert the complete mixed-overlay fixture before timing traversal APIs.
pub fn assert_traversal_graph(graph: &CodePropertyGraph, nodes: &[NodeId]) {
    assert_eq!(graph.node_count(), nodes.len(), "traversal node oracle");
    assert_eq!(
        graph.edge_count(),
        nodes.len().saturating_sub(1) * 3,
        "traversal edge oracle"
    );
    for endpoints in nodes.windows(2) {
        assert!(graph.node(endpoints[0]).is_some(), "node lookup oracle");
        assert_eq!(graph.ast_children(endpoints[0]), vec![endpoints[1]]);
        assert_eq!(
            graph.cfg_successors(endpoints[0]),
            vec![(endpoints[1], CfgEdgeKind::Sequential)]
        );
        assert_eq!(
            graph.dfg_successors(endpoints[0]),
            vec![(endpoints[1], DfgEdgeKind::DefUse)]
        );
        assert_eq!(graph.edges_between(endpoints[0], endpoints[1]).len(), 3);
    }
}

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

/// Construct a chain of CFG diamonds as a feature-free dominator workload.
///
/// Every stage adds two branch arms and one join. The stable node order is
/// `entry, left, right, join, ...`; the final join is the sole `Return` node
/// in the CPG fixture. This shape exercises immediate dominators, dominance
/// frontiers, post-dominators, and control dependence without parser costs.
pub fn dominator_diamond_fixture(
    stages: usize,
) -> (libcpg::GraphProjection, CodePropertyGraph, NodeId) {
    assert!(stages > 0, "dominator fixtures require at least one stage");
    let node_count = stages
        .checked_mul(3)
        .and_then(|count| count.checked_add(1))
        .expect("dominator fixture node count overflow");
    let dense_node_count = u32::try_from(node_count)
        .expect("dominator fixture node count exceeds the dense-index domain");
    let edge_count = stages
        .checked_mul(4)
        .expect("dominator fixture edge count overflow");
    let stable_nodes: Vec<_> = (0..dense_node_count).map(NodeId::new).collect();
    let mut edges = Vec::with_capacity(edge_count);
    let mut current = 0usize;
    for stage in 0..stages {
        let left = stage * 3 + 1;
        let right = left + 1;
        let join = left + 2;
        edges.extend([
            (current, left),
            (current, right),
            (left, join),
            (right, join),
        ]);
        current = join;
    }
    let projection = libcpg::GraphProjection::from_edges(
        ProjectionKind::External,
        stable_nodes.iter().copied(),
        edges
            .iter()
            .map(|&(source, target)| (stable_nodes[source], stable_nodes[target], 1.0)),
    );

    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = cpg.add_node(function_node("dominator_fixture".into()));
    let mut cpg_nodes = Vec::with_capacity(node_count);
    cpg_nodes.push(function);
    for index in 1..node_count {
        let kind = if index + 1 == node_count {
            CpgNodeKind::Return
        } else {
            CpgNodeKind::Block {
                scope: libcpg::ScopeId::GLOBAL,
            }
        };
        cpg_nodes.push(cpg.add_node(CpgNode::new(0.into(), kind, SourceRange::default())));
    }
    for &(source, target) in &edges {
        cpg.connect(
            cpg_nodes[source],
            cpg_nodes[target],
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        );
    }

    (projection, cpg, function)
}

/// Validate all structural facts relied on by dominator benchmarks.
pub fn assert_dominator_diamond_fixture(
    projection: &libcpg::GraphProjection,
    cpg: &CodePropertyGraph,
    function: NodeId,
    stages: usize,
) {
    let node_count = stages * 3 + 1;
    assert_eq!(
        projection.node_count(),
        node_count,
        "projection node oracle"
    );
    assert_eq!(
        projection.edge_count(),
        stages * 4,
        "projection edge oracle"
    );
    assert!(projection.is_well_formed(), "projection CSR oracle");
    assert_eq!(cpg.node_count(), node_count, "CPG node oracle");
    assert_eq!(cpg.edge_count(), stages * 4, "CPG edge oracle");

    let dominators =
        libcpg::dominator_tree(projection, NodeId::new(0)).expect("dominator fixture analysis");
    assert_eq!(dominators.idom.len(), node_count - 1, "idom oracle");
    assert!(dominators.dominates(NodeId::new(0), NodeId::new((node_count - 1) as u32)));

    let post_dominators =
        libcpg::post_dominator_tree(cpg, function).expect("post-dominator fixture analysis");
    assert_eq!(post_dominators.exits.len(), 1, "exit oracle");
    assert_eq!(post_dominators.ipdom.len(), node_count - 1, "ipdom oracle");
    assert_eq!(
        post_dominators.control_dependence_edges().len(),
        stages * 2,
        "control-dependence oracle"
    );
}

/// Resolver behavior isolated by the H14B benchmark and profile fixtures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolverCase {
    /// Every call has one exact declaration at the root scope.
    Exact,
    /// Every call names a symbol absent from the complete graph.
    Unresolved,
    /// Every call name has two exact declarations in the same scope.
    Ambiguous,
    /// Exact declarations are reached through a caller-selected scope depth.
    DeepExact { scope_depth: usize },
}

impl ResolverCase {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Unresolved => "unresolved",
            Self::Ambiguous => "ambiguous",
            Self::DeepExact { .. } => "deep_exact",
        }
    }

    const fn declarations_per_name(self) -> usize {
        match self {
            Self::Ambiguous => 2,
            Self::Exact | Self::Unresolved | Self::DeepExact { .. } => 1,
        }
    }

    const fn scope_depth(self) -> usize {
        match self {
            Self::DeepExact { scope_depth } => scope_depth,
            Self::Exact | Self::Unresolved | Self::Ambiguous => 1,
        }
    }

    const fn creates_topology(self) -> bool {
        matches!(self, Self::Exact | Self::DeepExact { .. })
    }
}

/// Deterministic exact-name resolver fixture with an analytical oracle.
pub struct ResolverFixture {
    pub graph: CodePropertyGraph,
    pub calls: Vec<NodeId>,
    pub expected_targets: Vec<Option<NodeId>>,
    pub case: ResolverCase,
    pub symbol_count: usize,
    pub config: ResolveConfig,
}

fn add_ast_node(graph: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let node = graph
        .add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()).with_parent(parent));
    graph.connect(parent, node, CpgEdgeKind::AstChild);
    node
}

/// Construct one feature-free resolver workload.
pub fn resolver_fixture(
    symbol_count: usize,
    call_count: usize,
    case: ResolverCase,
) -> ResolverFixture {
    assert!(symbol_count > 0, "resolver fixtures require a symbol");
    let mut graph = CodePropertyGraph::new(Language::Rust);
    let root = graph.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Root,
        SourceRange::default(),
    ));

    let mut first_targets = Vec::with_capacity(symbol_count);
    for symbol in 0..symbol_count {
        let name = format!("resolver_symbol_{symbol:06}");
        let mut first = None;
        for _ in 0..case.declarations_per_name() {
            let declaration = add_ast_node(
                &mut graph,
                root,
                CpgNodeKind::Function {
                    signature: Box::new(MethodSignature {
                        name: name.clone().into(),
                        params: Default::default(),
                        return_type: None,
                        is_static: false,
                        is_async: false,
                        visibility: Visibility::Private,
                    }),
                },
            );
            first.get_or_insert(declaration);
        }
        first_targets.push(first.expect("each resolver name has a declaration"));
    }

    let caller = add_ast_node(
        &mut graph,
        root,
        CpgNodeKind::Function {
            signature: Box::new(MethodSignature {
                name: "resolver_caller".into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            }),
        },
    );
    let mut call_scope = caller;
    for _ in 0..case.scope_depth() {
        call_scope = add_ast_node(
            &mut graph,
            call_scope,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
    }

    let mut calls = Vec::with_capacity(call_count);
    let mut expected_targets = Vec::with_capacity(call_count);
    for call_index in 0..call_count {
        let symbol = call_index % symbol_count;
        let call = add_ast_node(
            &mut graph,
            call_scope,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        );
        let name = if case == ResolverCase::Unresolved {
            format!("missing_symbol_{symbol:06}")
        } else {
            format!("resolver_symbol_{symbol:06}")
        };
        add_ast_node(
            &mut graph,
            call,
            CpgNodeKind::Identifier {
                name: name.into(),
                definition: None,
            },
        );
        calls.push(call);
        expected_targets.push(case.creates_topology().then_some(first_targets[symbol]));
    }

    let max_scope_depth = u32::try_from(case.scope_depth().saturating_add(4))
        .expect("resolver benchmark scope depth exceeds u32");
    ResolverFixture {
        graph,
        calls,
        expected_targets,
        case,
        symbol_count,
        config: ResolveConfig {
            max_scope_depth,
            max_candidates_per_name: 64,
            max_calls: call_count,
            resolve_methods: true,
        },
    }
}

/// Check every result dimension relied on by the resolver performance harness.
pub fn assert_resolver_fixture(
    fixture: &ResolverFixture,
    graph: &CodePropertyGraph,
    result: &CallGraphResolution,
) {
    assert_eq!(
        graph.node_count(),
        fixture.graph.node_count(),
        "node oracle"
    );
    assert_eq!(result.resolutions.len(), fixture.calls.len(), "call oracle");
    let expected_confidence = match fixture.case {
        ResolverCase::Exact | ResolverCase::DeepExact { .. } => Confidence::ExactInFile,
        ResolverCase::Unresolved => Confidence::Unresolved,
        ResolverCase::Ambiguous => Confidence::BareNameAmbiguous,
    };
    let expected_candidate_count = usize::from(fixture.case != ResolverCase::Unresolved)
        * fixture.case.declarations_per_name();
    for (((&call, resolution), expected_target), expected_call) in result
        .resolutions
        .iter()
        .zip(&fixture.expected_targets)
        .zip(&fixture.calls)
    {
        assert_eq!(call, *expected_call, "call ordering oracle");
        assert_eq!(resolution.confidence, expected_confidence);
        assert_eq!(resolution.target, *expected_target);
        assert_eq!(resolution.candidate_count, expected_candidate_count);
        assert_eq!(
            resolution.candidates.len(),
            expected_candidate_count.min(fixture.config.max_candidates_per_name),
        );
        assert!(!resolution.candidates_truncated);
        assert!(matches!(
            graph.node(call).map(|node| &node.kind),
            Some(CpgNodeKind::Call { target, .. }) if target == expected_target
        ));
    }
    let expected_topology = if fixture.case.creates_topology() {
        fixture.calls.len() * 2
    } else {
        0
    };
    assert_eq!(result.topology_edges, expected_topology);
    assert_eq!(
        result.ambiguous,
        usize::from(fixture.case == ResolverCase::Ambiguous) * fixture.calls.len()
    );
    assert_eq!(
        result.unresolved,
        usize::from(fixture.case == ResolverCase::Unresolved) * fixture.calls.len()
    );
    assert_eq!(result.external, 0);
}
