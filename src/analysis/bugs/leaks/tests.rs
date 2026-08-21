use std::collections::BTreeSet;

use proptest::prelude::*;

use super::*;
use crate::{
    CfgEdgeKind, CpgEdgeKind, CpgNode, CpgNodeKind, DfgEdgeKind, Language, MethodSignature, NodeId,
    ScopeId, SourceRange, Visibility,
};

fn resource_fsm() -> PropertyFsm {
    PropertyFsm::new("error")
        .with_allocation("open", "open")
        .with_event("close")
        .with_transition("open", "close", "closed")
        .with_accepting("closed")
}

fn add(cpg: &mut CodePropertyGraph, id: u32, kind: CpgNodeKind) -> NodeId {
    let id = NodeId::new(id);
    cpg.add_node_with_id(CpgNode::new(
        id,
        kind,
        SourceRange::from_bytes(id.as_u32() * 4, id.as_u32() * 4 + 1),
    ));
    id
}

fn function_kind() -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: MethodSignature {
            name: "entry".into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        },
    }
}

fn ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("fixture AST endpoints exist");
}

fn call(cpg: &mut CodePropertyGraph, id: u32, event: &str) -> (NodeId, NodeId) {
    let call = add(
        cpg,
        id,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    let callee = add(
        cpg,
        id + 1,
        CpgNodeKind::Identifier {
            name: event.into(),
            definition: None,
        },
    );
    ast(cpg, call, callee);
    (call, callee)
}

#[derive(Debug)]
struct Fixture {
    cpg: CodePropertyGraph,
    open: NodeId,
    close: NodeId,
    exit: NodeId,
}

fn fixture(with_leaking_branch: bool, reverse_relations: bool) -> Fixture {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let function = add(&mut cpg, 1, function_kind());
    let (open, _) = call(&mut cpg, 10, "open");
    let (close, _) = call(&mut cpg, 20, "close");
    let close_argument = add(
        &mut cpg,
        22,
        CpgNodeKind::Identifier {
            name: "resource".into(),
            definition: Some(open),
        },
    );
    ast(&mut cpg, close, close_argument);
    let exit = add(
        &mut cpg,
        30,
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        },
    );
    for child in [open, close, exit] {
        ast(&mut cpg, function, child);
    }

    let mut cfg = vec![
        (function, open, CfgEdgeKind::Sequential),
        (open, close, CfgEdgeKind::ConditionalTrue),
        (close, exit, CfgEdgeKind::Sequential),
    ];
    if with_leaking_branch {
        cfg.push((open, exit, CfgEdgeKind::ConditionalFalse));
    }
    let mut dfg = vec![(open, close_argument)];
    if reverse_relations {
        cfg.reverse();
        dfg.reverse();
    }
    for (source, target, kind) in cfg {
        cpg.connect(source, target, CpgEdgeKind::ControlFlow(kind))
            .expect("fixture CFG endpoints exist");
    }
    for (source, target) in dfg {
        cpg.connect(source, target, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
            .expect("fixture DFG endpoints exist");
    }
    cpg.add_cfg_exit(exit);

    Fixture {
        cpg,
        open,
        close,
        exit,
    }
}

#[test]
fn release_on_every_exit_path_is_clean_and_read_only() {
    let fixture = fixture(false, false);
    let before = (fixture.cpg.node_count(), fixture.cpg.edge_count());

    let report = analyze_resource_leaks(&fixture.cpg, &resource_fsm(), &IfdsCaps::default());

    assert!(report.is_complete(), "{report:#?}");
    assert!(report.leaks.is_empty(), "{report:#?}");
    assert_eq!(
        report
            .typestate
            .object_is_accepting(fixture.open, &resource_fsm()),
        Some(true)
    );
    assert_eq!((fixture.cpg.node_count(), fixture.cpg.edge_count()), before);
}

#[test]
fn recorded_function_exits_exclude_expression_evaluation_side_sinks() {
    let mut fixture = fixture(false, false);
    let function = fixture
        .cpg
        .functions()
        .next()
        .expect("fixture function exists")
        .id;
    let side_sink = add(
        &mut fixture.cpg,
        40,
        CpgNodeKind::Identifier {
            name: "argument".into(),
            definition: None,
        },
    );
    ast(&mut fixture.cpg, function, side_sink);
    fixture
        .cpg
        .connect(
            fixture.open,
            side_sink,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        )
        .expect("side-evaluation CFG endpoints exist");

    let report = analyze_resource_leaks(&fixture.cpg, &resource_fsm(), &IfdsCaps::default());

    assert!(report.is_complete(), "{report:#?}");
    assert!(report.leaks.is_empty(), "{report:#?}");
    assert_eq!(report.typestate.terminals.len(), 1, "{report:#?}");
    assert_eq!(report.typestate.terminals[0].exit, fixture.exit);
}

#[test]
fn missing_release_on_one_branch_reports_its_non_accepting_exit_witness() {
    let fixture = fixture(true, false);

    let report = analyze_resource_leaks(&fixture.cpg, &resource_fsm(), &IfdsCaps::default());

    assert!(report.is_complete(), "{report:#?}");
    assert_eq!(report.leaks.len(), 1, "{report:#?}");
    let leak = &report.leaks[0];
    assert_eq!(leak.object, fixture.open);
    assert_eq!(leak.exit, fixture.exit);
    assert_eq!(leak.state.as_ref(), "open");
    assert_eq!(leak.witness.first().copied(), Some(fixture.open));
    assert_eq!(leak.witness.last().copied(), Some(fixture.exit));
    assert!(!leak.witness.contains(&fixture.close));
    assert!(!leak.capped);
    assert_eq!(
        resource_leaks(&fixture.cpg, &resource_fsm(), &IfdsCaps::default()),
        report.leaks
    );
}

#[test]
fn absent_accepting_vocabulary_is_reported_instead_of_guessing_leak_semantics() {
    let fixture = fixture(true, false);
    let fsm = PropertyFsm::new("error")
        .with_allocation("open", "open")
        .with_event("close")
        .with_transition("open", "close", "closed");

    let report = analyze_resource_leaks(&fixture.cpg, &fsm, &IfdsCaps::default());

    assert!(!report.is_complete());
    assert!(report.leaks.is_empty());
    assert_eq!(
        report.issues,
        BTreeSet::from([ResourceLeakIssue::NoAcceptingState])
    );
}

#[test]
fn typestate_caps_remain_visible_at_the_leak_boundary() {
    let fixture = fixture(true, false);
    let caps = IfdsCaps {
        max_worklist_steps: 1,
        ..IfdsCaps::default()
    };

    let report = analyze_resource_leaks(&fixture.cpg, &resource_fsm(), &caps);

    assert!(!report.is_complete());
    assert!(report.typestate.capped);
    assert!(!report.typestate.cap_events.is_empty());
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn leak_projection_is_deterministic_under_relation_insertion_order(
        with_leaking_branch in any::<bool>(),
    ) {
        let forward = fixture(with_leaking_branch, false);
        let reversed = fixture(with_leaking_branch, true);
        let fsm = resource_fsm();

        prop_assert_eq!(
            analyze_resource_leaks(&forward.cpg, &fsm, &IfdsCaps::default()),
            analyze_resource_leaks(&reversed.cpg, &fsm, &IfdsCaps::default()),
        );
    }
}

#[cfg(feature = "serde")]
#[test]
fn leak_report_round_trips_through_json() {
    let fixture = fixture(true, false);
    let report = analyze_resource_leaks(&fixture.cpg, &resource_fsm(), &IfdsCaps::default());
    let encoded = serde_json::to_string(&report).expect("serialize leak report");
    let decoded: ResourceLeakAnalysis =
        serde_json::from_str(&encoded).expect("deserialize leak report");
    assert_eq!(decoded, report);
}
