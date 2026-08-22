use libcpg::pattern::Vf2Matcher;
use libcpg::{
    CfgExtractor, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, DfgExtractor, Language,
    MethodSignature, NodeId, PdgBuilder, ScopeId, SourceRange, Visibility,
};

const SMALL_WORKER_STACK: usize = 256 * 1024;
const WORKER_COUNTS: [usize; 6] = [1, 2, 4, 8, 16, 32];

fn add_node(cpg: &mut CodePropertyGraph, kind: CpgNodeKind) -> NodeId {
    cpg.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()))
}

fn add_child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let child = cpg
        .add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()).with_parent(parent));
    cpg.connect_unique(parent, child, CpgEdgeKind::AstChild);
    child
}

fn function_kind(index: usize) -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: Box::new(MethodSignature {
            name: format!("function_{index}").into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Public,
        }),
    }
}

fn multi_function_graph(functions: usize, statements: usize) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    for function_index in 0..functions {
        let function = add_node(&mut cpg, function_kind(function_index));
        let body = add_child(
            &mut cpg,
            function,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        for statement_index in 0..statements {
            let name = format!("value_{function_index}_{statement_index}");
            add_child(
                &mut cpg,
                body,
                CpgNodeKind::Variable {
                    name: name.clone().into(),
                    var_type: None,
                    is_mutable: true,
                    scope: ScopeId::GLOBAL,
                },
            );
            add_child(
                &mut cpg,
                body,
                CpgNodeKind::Identifier {
                    name: name.into(),
                    definition: None,
                },
            );
        }
    }
    cpg
}

fn assert_graph_exact(actual: &CodePropertyGraph, expected: &CodePropertyGraph) {
    assert_eq!(
        actual.node_ids().collect::<Vec<_>>(),
        expected.node_ids().collect::<Vec<_>>()
    );
    assert_eq!(
        actual.edges().cloned().collect::<Vec<_>>(),
        expected.edges().cloned().collect::<Vec<_>>()
    );
    assert_eq!(actual.cfg_entries(), expected.cfg_entries());
    assert_eq!(actual.cfg_exits(), expected.cfg_exits());

    #[cfg(feature = "serde")]
    assert_eq!(
        serde_json::to_vec(actual).expect("serialize actual CPG"),
        serde_json::to_vec(expected).expect("serialize expected CPG"),
        "serial and parallel CPG encodings differ"
    );
}

fn match_batch_signature(batch: &[Vec<libcpg::PatternMatch>]) -> Vec<Vec<Vec<(NodeId, NodeId)>>> {
    batch
        .iter()
        .map(|matches| {
            matches
                .iter()
                .map(|matched| {
                    let mut mapping: Vec<_> = matched
                        .node_mapping
                        .iter()
                        .map(|(&pattern, &target)| (pattern, target))
                        .collect();
                    mapping.sort_unstable();
                    mapping
                })
                .collect()
        })
        .collect()
}

#[test]
fn every_worker_count_matches_serial_cfg_dfg_pdg_bytes() {
    let template = multi_function_graph(64, 8);
    let mut serial = template.clone();
    CfgExtractor::new().extract(&mut serial);
    DfgExtractor::new().extract(&mut serial);
    PdgBuilder::new().build_all(&mut serial);

    for workers in WORKER_COUNTS {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .stack_size(SMALL_WORKER_STACK)
            .build()
            .expect("bounded Rayon pool");
        let mut parallel = template.clone();
        pool.install(|| {
            CfgExtractor::new().extract_parallel(&mut parallel);
            DfgExtractor::new().extract_parallel(&mut parallel);
            PdgBuilder::new().build_all_parallel(&mut parallel);
        });
        assert_graph_exact(&parallel, &serial);

        pool.install(|| {
            CfgExtractor::new().extract_parallel(&mut parallel);
            DfgExtractor::new().extract_parallel(&mut parallel);
            PdgBuilder::new().build_all_parallel(&mut parallel);
        });
        assert_graph_exact(&parallel, &serial);
    }
}

#[test]
fn every_worker_count_preserves_pattern_and_embedding_order() {
    let target = multi_function_graph(24, 4);
    let node_ids: Vec<_> = target.node_ids().collect();
    let patterns = vec![
        target.subgraph(&node_ids[0..1]),
        target.subgraph(&node_ids[0..2]),
        target.subgraph(&node_ids[1..4]),
        target.subgraph(&node_ids[4..8]),
    ];
    let matcher = Vf2Matcher::new()
        .with_strict_kinds(true)
        .with_strict_edges(true)
        .with_max_matches(32);
    let serial = matcher.find_matches_many(&patterns, &target);
    let serial_signature = match_batch_signature(&serial);

    for workers in WORKER_COUNTS {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .stack_size(SMALL_WORKER_STACK)
            .build()
            .expect("bounded Rayon pool");
        let parallel = pool.install(|| matcher.find_matches_many_parallel(&patterns, &target));
        assert_eq!(match_batch_signature(&parallel), serial_signature);
    }
}

#[test]
fn parallel_pipeline_crosses_ast_depth_100_000_on_small_worker_stacks() {
    const DEPTH: usize = 100_000;

    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add_node(&mut cpg, function_kind(0));
    let mut parent = add_child(
        &mut cpg,
        function,
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        },
    );
    for depth in 0..DEPTH {
        parent = add_child(
            &mut cpg,
            parent,
            CpgNodeKind::Unknown {
                kind: format!("level_{depth}").into(),
            },
        );
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .stack_size(SMALL_WORKER_STACK)
        .build()
        .expect("bounded Rayon pool");
    pool.install(|| {
        CfgExtractor::new().extract_parallel(&mut cpg);
        DfgExtractor::new().extract_parallel(&mut cpg);
        PdgBuilder::new().build_all_parallel(&mut cpg);
    });

    assert_eq!(cpg.node_count(), DEPTH + 2);
    assert!(cpg.cfg_entries().contains(&function));
    assert_eq!(cpg.stats().cfg_edges, DEPTH + 1);
}

#[test]
#[cfg(feature = "lang-rust")]
fn tree_sitter_parallel_configuration_is_exactly_serial() {
    use libcpg::{CpgBuilder, CpgBuilderConfig, TreeSitterCpgBuilder};

    let mut source = String::new();
    for index in 0..128 {
        source.push_str(&format!(
            "fn f_{index}(x: i32) -> i32 {{ if x > 0 {{ x + {index} }} else {{ x - {index} }} }}\n"
        ));
    }
    let serial = TreeSitterCpgBuilder::new()
        .build(&source, Language::Rust)
        .expect("serial Rust CPG");
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .stack_size(SMALL_WORKER_STACK)
        .build()
        .expect("bounded Rayon pool");
    let parallel = pool.install(|| {
        TreeSitterCpgBuilder::with_config(CpgBuilderConfig::default().with_parallel_analysis(true))
            .build(&source, Language::Rust)
            .expect("parallel Rust CPG")
    });
    assert_graph_exact(&parallel, &serial);
}
