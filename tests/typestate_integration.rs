//! Public-API integration and stack-safety gates for typestate analysis.

use std::collections::BTreeSet;

use libcpg::{
    analyze_typestate, CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind,
    DfgEdgeKind, IfdsCaps, Language, MethodSignature, NodeId, PropertyFsm, PropertyState, ScopeId,
    SourceRange, Visibility,
};

fn resource_fsm() -> PropertyFsm {
    PropertyFsm::new("error")
        .with_allocation("open", "open")
        .with_event("close")
        .with_transition("open", "close", "closed")
        .with_accepting("closed")
}

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
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

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let child = cpg.add_node(node(kind).with_parent(parent));
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("public fixture AST endpoints exist");
    child
}

fn call(cpg: &mut CodePropertyGraph, parent: NodeId, event: &str) -> NodeId {
    let call = child(
        cpg,
        parent,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    child(
        cpg,
        call,
        CpgNodeKind::Identifier {
            name: event.into(),
            definition: None,
        },
    );
    call
}

#[test]
fn public_api_reports_a_forbidden_transition_without_mutating_the_cpg() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let function = cpg.add_node(node(function("entry")));
    let open = call(&mut cpg, function, "open");
    let close = call(&mut cpg, function, "close");
    let close_argument = child(
        &mut cpg,
        close,
        CpgNodeKind::Identifier {
            name: "resource".into(),
            definition: Some(open),
        },
    );
    let second_close = call(&mut cpg, function, "close");
    let second_argument = child(
        &mut cpg,
        second_close,
        CpgNodeKind::Identifier {
            name: "resource".into(),
            definition: Some(open),
        },
    );
    for (source, target) in [(function, open), (open, close), (close, second_close)] {
        cpg.connect(
            source,
            target,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        )
        .expect("public fixture CFG endpoints exist");
    }
    for argument in [close_argument, second_argument] {
        cpg.connect(open, argument, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
            .expect("public fixture DFG endpoints exist");
    }

    let before = (cpg.node_count(), cpg.edge_count());
    let report = analyze_typestate(&cpg, &resource_fsm(), &IfdsCaps::default());

    assert!(report.is_complete(), "{report:#?}");
    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations[0].object, open);
    assert_eq!(report.violations[0].site, second_close);
    assert_eq!(report.violations[0].from.as_ref(), "closed");
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}

#[cfg(feature = "lang-rust")]
#[test]
fn parser_built_cfg_and_dfg_report_a_rust_double_close() {
    use libcpg::{CpgBuilder, TreeSitterCpgBuilder};

    let source = r#"
        struct Resource;
        fn open() -> Resource { Resource }
        fn close(_resource: Resource) {}

        fn entry() {
            let resource = open();
            close(resource);
            close(resource);
        }
    "#;
    let cpg = TreeSitterCpgBuilder::new()
        .build(source, Language::Rust)
        .expect("parser-backed Rust CPG");
    let report = analyze_typestate(&cpg, &resource_fsm(), &IfdsCaps::default());

    assert!(report.is_complete(), "{report:#?}");
    assert_eq!(report.violations.len(), 1, "{report:#?}");
    assert_eq!(report.violations[0].from.as_ref(), "closed");
    assert_eq!(report.violations[0].event.as_ref(), "close");
}

#[test]
fn public_typestate_machine_crosses_cfg_depth_100_000_on_a_small_stack() {
    const DEPTH: usize = 100_000;

    std::thread::Builder::new()
        .name("typestate-small-stack".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut cpg = CodePropertyGraph::new(Language::Unknown);
            let function = cpg.add_node(node(function("deep")));
            let open = call(&mut cpg, function, "open");
            cpg.connect(
                function,
                open,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
            )
            .expect("deep CFG entry endpoints exist");

            let mut previous = open;
            for _ in 0..DEPTH {
                let block = child(
                    &mut cpg,
                    function,
                    CpgNodeKind::Block {
                        scope: ScopeId::GLOBAL,
                    },
                );
                cpg.connect(
                    previous,
                    block,
                    CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
                )
                .expect("deep CFG endpoints exist");
                previous = block;
            }

            let close = call(&mut cpg, function, "close");
            let argument = child(
                &mut cpg,
                close,
                CpgNodeKind::Identifier {
                    name: "resource".into(),
                    definition: Some(open),
                },
            );
            cpg.connect(
                previous,
                close,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
            )
            .expect("deep CFG exit endpoints exist");
            cpg.connect(open, argument, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
                .expect("deep DFG endpoints exist");

            let report = analyze_typestate(&cpg, &resource_fsm(), &IfdsCaps::default());

            assert!(report.is_complete(), "{report:#?}");
            assert!(report.violations.is_empty());
            assert_eq!(
                report.terminal_states.get(&open),
                Some(&BTreeSet::from([PropertyState::from("closed")]))
            );
        })
        .expect("spawn typestate small-stack test")
        .join()
        .expect("typestate machine completes on a small native stack");
}
