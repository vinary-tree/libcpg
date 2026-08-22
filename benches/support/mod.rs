#![allow(dead_code)]

use libcpg::{
    CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, DfgEdgeKind, Language,
    MethodSignature, NodeId, ProjectionKind, SourceRange, Visibility,
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
