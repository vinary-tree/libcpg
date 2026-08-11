//! Public-API integration for CFG/DFG-backed complexity evidence.

#![cfg(feature = "algorithm-detection")]

use libcpg::algorithms::detection::ComplexityAnalyzer;
use libcpg::algorithms::ComplexityClass;
use libcpg::{
    CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, Language, LiteralKind,
    MethodSignature, NodeId, ScopeId, SourceRange, Visibility,
};

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn add(cpg: &mut CodePropertyGraph, kind: CpgNodeKind) -> NodeId {
    cpg.add_node(node(kind))
}

fn function(name: &str) -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: MethodSignature {
            name: name.into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        },
    }
}

fn block() -> CpgNodeKind {
    CpgNodeKind::Block {
        scope: ScopeId::GLOBAL,
    }
}

fn ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("integration AST endpoints");
    cpg.node_mut(child).expect("integration child").parent = Some(parent);
    cpg.node_mut(parent)
        .expect("integration parent")
        .children
        .push(child);
}

fn cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId, kind: CfgEdgeKind) {
    cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind))
        .expect("integration CFG endpoints");
}

fn range_loop(
    cpg: &mut CodePropertyGraph,
    parent: NodeId,
    variable: &str,
    bound: i64,
) -> (NodeId, NodeId, NodeId) {
    let header = add(cpg, CpgNodeKind::For);
    let binder = add(
        cpg,
        CpgNodeKind::Variable {
            name: variable.into(),
            var_type: None,
            scope: ScopeId::GLOBAL,
            is_mutable: false,
        },
    );
    let range = add(
        cpg,
        CpgNodeKind::BinaryOp {
            operator: "..".into(),
        },
    );
    let start = add(
        cpg,
        CpgNodeKind::Literal {
            kind: LiteralKind::Integer(0),
        },
    );
    let bound = add(
        cpg,
        CpgNodeKind::Literal {
            kind: LiteralKind::Integer(bound),
        },
    );
    let body = add(cpg, block());
    ast(cpg, parent, header);
    for child in [binder, range, body] {
        ast(cpg, header, child);
    }
    for child in [start, bound] {
        ast(cpg, range, child);
    }
    (header, body, bound)
}

#[test]
fn public_analyzer_uses_counted_nested_cfg_evidence_without_mutation() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add(&mut cpg, function("nested"));
    let (outer, outer_body, outer_bound) = range_loop(&mut cpg, function, "i", 100);
    let (inner, inner_body, inner_bound) = range_loop(&mut cpg, outer_body, "j", 50);
    let inner_latch = add(
        &mut cpg,
        CpgNodeKind::Unknown {
            kind: "inner_latch".into(),
        },
    );
    let outer_latch = add(
        &mut cpg,
        CpgNodeKind::Unknown {
            kind: "outer_latch".into(),
        },
    );
    let exit = add(&mut cpg, CpgNodeKind::Return);
    ast(&mut cpg, inner_body, inner_latch);
    ast(&mut cpg, outer_body, outer_latch);
    ast(&mut cpg, function, exit);

    for (source, target, kind) in [
        (function, outer, CfgEdgeKind::Sequential),
        (outer, inner, CfgEdgeKind::ConditionalTrue),
        (inner, inner_latch, CfgEdgeKind::ConditionalTrue),
        (inner_latch, inner, CfgEdgeKind::LoopBack),
        (inner, outer_latch, CfgEdgeKind::LoopExit),
        (outer_latch, outer, CfgEdgeKind::LoopBack),
        (outer, exit, CfgEdgeKind::LoopExit),
    ] {
        cfg(&mut cpg, source, target, kind);
    }
    cpg.add_cfg_entry(function);
    cpg.add_cfg_exit(exit);

    let before = (cpg.node_count(), cpg.edge_count());
    let estimate = ComplexityAnalyzer::new().estimate_time_complexity_with_evidence(&cpg, function);
    assert_eq!(estimate.class, ComplexityClass::Quadratic);
    assert_eq!(estimate.confidence, 1.0);
    for node in [outer, inner, outer_bound, inner_bound] {
        assert!(estimate.justification.contains(&format!("{node:?}")));
    }
    assert!(estimate.justification.contains("counted IV i"));
    assert!(estimate.justification.contains("counted IV j"));
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}

#[cfg(feature = "lang-rust")]
#[test]
fn parsed_loop_free_corpus_matches_the_legacy_class_and_confidence_exactly() {
    use libcpg::{CpgBuilder, TreeSitterCpgBuilder};

    let source = r#"
        fn identity(x: i32) -> i32 { x }
        fn choose(x: i32) -> i32 { if x > 0 { x } else { -x } }
        fn add(x: i32, y: i32) -> i32 { x + y }
    "#;
    let cpg = TreeSitterCpgBuilder::new()
        .build(source, Language::Rust)
        .expect("loop-free Rust corpus");
    let analyzer = ComplexityAnalyzer::new();
    for function in cpg.functions().map(|node| node.id) {
        let heuristic = analyzer.estimate_time_complexity(&cpg, function);
        let evidence = analyzer.estimate_time_complexity_with_evidence(&cpg, function);
        assert_eq!(evidence.class, heuristic.class);
        assert_eq!(
            evidence.confidence.to_bits(),
            heuristic.confidence.to_bits()
        );
        assert_eq!(evidence.justification, heuristic.justification);
    }
}

#[cfg(feature = "lang-rust")]
#[test]
fn default_detector_attaches_counted_evidence_on_real_nested_rust_ranges() {
    use libcpg::algorithms::detection::DefaultAlgorithmDetector;
    use libcpg::algorithms::{AlgorithmDetector, AlgorithmFamily};
    use libcpg::{CpgBuilder, TreeSitterCpgBuilder};

    let source = r#"
        fn sort(a: &mut [i32]) {
            for i in 0..10 {
                for j in 0..10 {
                    if a[i] < a[j] { a.swap(i, j); }
                }
            }
        }
    "#;
    let cpg = TreeSitterCpgBuilder::new()
        .build(source, Language::Rust)
        .expect("nested Rust range loops");
    let function = cpg.functions().next().expect("sort function").id;
    let found = DefaultAlgorithmDetector::new().detect(&cpg, function);
    let sorting = found
        .iter()
        .find(|detection| detection.family == AlgorithmFamily::Sorting)
        .expect("sorting detection");
    let estimate = sorting
        .signature
        .time_complexity
        .as_ref()
        .expect("detector complexity signature");
    assert_eq!(estimate.class, ComplexityClass::Quadratic);
    assert_eq!(estimate.confidence, 1.0);
    assert!(estimate.justification.contains("CFG/DFG loop evidence"));
    assert!(estimate.justification.contains("counted IV i"));
    assert!(estimate.justification.contains("counted IV j"));
}
