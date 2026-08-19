//! Public-API, parser-pipeline, invariance, and small-stack gates for T6.2.

use libcpg::{
    analyze_exception_flow, analyze_resource_leaks, analyze_uninitialized_uses, CfgEdgeKind,
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, ExceptionType, IfdsCaps, Language,
    MethodSignature, NodeId, PropertyFsm, ScopeId, SourceRange, TypeInfo, Visibility,
};

#[cfg(feature = "lang-java")]
use libcpg::CfgExtractor;

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn function_kind(name: &str) -> CpgNodeKind {
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

fn block_kind() -> CpgNodeKind {
    CpgNodeKind::Block {
        scope: ScopeId::GLOBAL,
    }
}

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let child = cpg.add_node(node(kind).with_parent(parent));
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("public fixture AST endpoints exist");
    cpg.node_mut(parent)
        .expect("public fixture parent exists")
        .children
        .push(child);
    child
}

fn resource_fsm() -> PropertyFsm {
    PropertyFsm::new("error")
        .with_allocation("open", "open")
        .with_event("close")
        .with_transition("open", "close", "closed")
        .with_accepting("closed")
}

#[test]
fn public_detectors_report_all_three_bug_classes_without_mutating_the_cpg() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let function = cpg.add_node(node(function_kind("entry")));
    let open = child(
        &mut cpg,
        function,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    child(
        &mut cpg,
        open,
        CpgNodeKind::Identifier {
            name: "open".into(),
            definition: None,
        },
    );
    let uninitialized = child(
        &mut cpg,
        function,
        CpgNodeKind::Identifier {
            name: "value".into(),
            definition: None,
        },
    );
    let throw = child(&mut cpg, function, CpgNodeKind::Throw);
    child(
        &mut cpg,
        throw,
        CpgNodeKind::TypeAnnotation {
            type_info: TypeInfo::new("Failure"),
        },
    );
    for (source, target) in [(function, open), (open, throw)] {
        cpg.connect(
            source,
            target,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        )
        .expect("public fixture CFG endpoints exist");
    }
    cpg.add_cfg_exit(throw);
    let before = (cpg.node_count(), cpg.edge_count());

    let leaks = analyze_resource_leaks(&cpg, &resource_fsm(), &IfdsCaps::default());
    let uninit = analyze_uninitialized_uses(&cpg);
    let exceptions = analyze_exception_flow(&cpg, &IfdsCaps::default());

    assert!(leaks.is_complete(), "{leaks:#?}");
    assert_eq!(leaks.leaks.len(), 1, "{leaks:#?}");
    assert_eq!(leaks.leaks[0].object, open);
    assert!(uninit.is_complete());
    assert_eq!(uninit.uses.len(), 1, "{uninit:#?}");
    assert_eq!(uninit.uses[0].use_site, uninitialized);
    assert!(exceptions.is_complete(), "{exceptions:#?}");
    assert_eq!(exceptions.uncaught.len(), 1, "{exceptions:#?}");
    assert_eq!(
        exceptions.uncaught[0].exception,
        ExceptionType::Named("Failure".into())
    );
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}

#[cfg(feature = "lang-rust")]
#[test]
fn parser_built_rust_reports_a_leak_and_a_use_before_definition() {
    use libcpg::{CpgBuilder, TreeSitterCpgBuilder};

    let source = r#"
        struct Resource;
        fn open() -> Resource { Resource }
        fn consume(_: i32) {}

        fn entry() {
            let _resource = open();
            consume(value);
            let value = 1;
        }
    "#;
    let cpg = TreeSitterCpgBuilder::new()
        .build(source, Language::Rust)
        .expect("parser-backed Rust CPG");

    let leaks = analyze_resource_leaks(&cpg, &resource_fsm(), &IfdsCaps::default());
    let uninit = analyze_uninitialized_uses(&cpg);

    assert!(leaks.is_complete(), "{leaks:#?}");
    assert_eq!(leaks.leaks.len(), 1, "{leaks:#?}");
    assert_eq!(leaks.leaks[0].state.as_ref(), "open");
    assert!(uninit
        .uses
        .iter()
        .any(|finding| finding.variable.as_ref() == "value"));
    assert!(!uninit
        .uses
        .iter()
        .any(|finding| finding.variable.as_ref() == "consume"));
}

#[cfg(feature = "lang-java")]
#[test]
fn java_build_modes_emit_identical_typed_exception_edges_and_findings() {
    use libcpg::builder::ParserRegistry;
    use libcpg::{CpgBuilder, TreeSitterCpgBuilder};

    let source = r#"
        class Demo {
            static void uncaught(Exception ex) { throw ex; }
            static void caught(Exception ex) {
                try { throw ex; } catch (Exception handled) { }
            }
            static void dead(Exception ex) {
                try { throw ex; } catch (RuntimeException handled) { }
            }
        }
    "#;
    let language = ParserRegistry::global()
        .get(Language::Java)
        .expect("Java grammar is enabled");
    let mut parser = tree_sitter::Parser::new();
    parser.set_language(&language).expect("set Java grammar");
    let tree = parser.parse(source, None).expect("parse Java source");
    let builder = TreeSitterCpgBuilder::new();
    let from_tree = builder
        .build_from_tree(&tree, source, Language::Java)
        .expect("build from caller-supplied Java tree");
    let mut from_source = builder
        .build(source, Language::Java)
        .expect("build from Java source");

    let edge_set = |cpg: &CodePropertyGraph| {
        cpg.edges()
            .map(|edge| (edge.source, edge.target, format!("{:?}", edge.kind)))
            .collect::<std::collections::BTreeSet<_>>()
    };
    assert_eq!(from_tree.node_count(), from_source.node_count());
    assert_eq!(from_tree.edge_count(), from_source.edge_count());
    assert_eq!(edge_set(&from_tree), edge_set(&from_source));
    let catch_entries = from_source
        .edges()
        .filter(|edge| matches!(edge.kind, CpgEdgeKind::ControlFlow(CfgEdgeKind::Catch)))
        .count();
    assert_eq!(catch_entries, 2);

    let before_rerun = (from_source.node_count(), from_source.edge_count());
    CfgExtractor::new().extract(&mut from_source);
    assert_eq!(
        (from_source.node_count(), from_source.edge_count()),
        before_rerun,
        "typed exception extraction is append-only and idempotent"
    );

    let tree_report = analyze_exception_flow(&from_tree, &IfdsCaps::default());
    let source_report = analyze_exception_flow(&from_source, &IfdsCaps::default());
    assert!(tree_report.is_complete(), "{tree_report:#?}");
    assert_eq!(tree_report, source_report);
    assert_eq!(tree_report.uncaught.len(), 2, "{tree_report:#?}");
    assert_eq!(tree_report.dead_catches.len(), 1, "{tree_report:#?}");
    assert_eq!(tree_report.reached_catches.len(), 1, "{tree_report:#?}");
}

#[test]
fn public_detectors_cross_ast_depth_100_000_on_a_small_stack() {
    const DEPTH: usize = 100_000;

    std::thread::Builder::new()
        .name("bug-detectors-small-stack".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut cpg = CodePropertyGraph::new(Language::Unknown);
            let function = cpg.add_node(node(function_kind("deep")));
            let body = child(&mut cpg, function, block_kind());
            let variable = child(
                &mut cpg,
                body,
                CpgNodeKind::Variable {
                    name: "value".into(),
                    var_type: None,
                    scope: ScopeId::GLOBAL,
                    is_mutable: true,
                },
            );
            let try_node = child(&mut cpg, body, CpgNodeKind::Try);
            let protected = child(&mut cpg, try_node, block_kind());
            let mut parent = protected;
            for _ in 0..DEPTH {
                parent = child(&mut cpg, parent, block_kind());
            }
            let use_site = child(
                &mut cpg,
                parent,
                CpgNodeKind::Identifier {
                    name: "value".into(),
                    definition: Some(variable),
                },
            );
            let throw = child(&mut cpg, parent, CpgNodeKind::Throw);
            child(
                &mut cpg,
                throw,
                CpgNodeKind::TypeAnnotation {
                    type_info: TypeInfo::new("Failure"),
                },
            );
            let catch = child(&mut cpg, try_node, CpgNodeKind::Catch);
            child(
                &mut cpg,
                catch,
                CpgNodeKind::Parameter {
                    name: "error".into(),
                    param_type: Some(TypeInfo::new("Failure")),
                    is_variadic: false,
                },
            );
            let catch_body = child(&mut cpg, catch, block_kind());
            cpg.connect(
                catch,
                catch_body,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Catch),
            )
            .expect("deep Catch edge endpoints exist");

            let uninit = analyze_uninitialized_uses(&cpg);
            let exceptions = analyze_exception_flow(&cpg, &IfdsCaps::default());

            assert_eq!(uninit.uses.len(), 1, "{uninit:#?}");
            assert_eq!(uninit.uses[0].use_site, use_site);
            assert!(exceptions.is_complete(), "{exceptions:#?}");
            assert!(exceptions.uncaught.is_empty(), "{exceptions:#?}");
            assert_eq!(
                exceptions.reached_catches[&catch],
                std::collections::BTreeSet::from([ExceptionType::Named("Failure".into())])
            );
        })
        .expect("spawn small-stack bug-detector test")
        .join()
        .expect("bug detectors complete without native-stack growth");
}
