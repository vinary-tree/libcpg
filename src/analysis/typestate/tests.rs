use std::collections::BTreeSet;
use std::sync::Arc;

use proptest::prelude::*;

use super::*;
use crate::{CpgEdgeKind, CpgNode, Language, MethodSignature, ScopeId, SourceRange, Visibility};

fn open_close_fsm() -> PropertyFsm {
    PropertyFsm::new("error")
        .with_allocation("open", "open")
        .with_event("close")
        .with_event("use")
        .with_transition("open", "use", "open")
        .with_transition("open", "close", "closed")
        .with_accepting("closed")
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

fn add_with_id(cpg: &mut CodePropertyGraph, id: u32, kind: CpgNodeKind) -> NodeId {
    let id = NodeId::new(id);
    cpg.add_node_with_id(CpgNode::new(
        id,
        kind,
        SourceRange::from_bytes(id.as_u32() * 4, id.as_u32() * 4 + 1),
    ));
    id
}

fn connect_ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("fixture endpoints exist");
    cpg.node_mut(parent)
        .expect("fixture parent exists")
        .children
        .push(child);
    cpg.node_mut(child).expect("fixture child exists").parent = Some(parent);
}

#[derive(Debug)]
struct SequenceFixture {
    cpg: CodePropertyGraph,
    function: NodeId,
    calls: Vec<NodeId>,
}

fn sequence_fixture(events_after_open: &[&str], reverse_relations: bool) -> SequenceFixture {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add_with_id(&mut cpg, 1, function_kind("entry"));
    let mut calls = Vec::new();
    let mut argument_nodes = Vec::new();

    for (index, event) in std::iter::once(&"open")
        .chain(events_after_open.iter())
        .enumerate()
    {
        let base = 10 + index as u32 * 4;
        let call = add_with_id(
            &mut cpg,
            base,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        );
        let callee = add_with_id(
            &mut cpg,
            base + 1,
            CpgNodeKind::Identifier {
                name: (*event).into(),
                definition: None,
            },
        );
        connect_ast(&mut cpg, function, call);
        connect_ast(&mut cpg, call, callee);

        if index > 0 {
            let argument = add_with_id(
                &mut cpg,
                base + 2,
                CpgNodeKind::Identifier {
                    name: "resource".into(),
                    definition: Some(calls[0]),
                },
            );
            connect_ast(&mut cpg, call, argument);
            argument_nodes.push(argument);
        }
        calls.push(call);
    }

    let mut cfg = Vec::new();
    let mut previous = function;
    for &call in &calls {
        cfg.push((previous, call));
        previous = call;
    }
    let mut dfg: Vec<_> = argument_nodes
        .iter()
        .copied()
        .map(|argument| (calls[0], argument))
        .collect();
    if reverse_relations {
        cfg.reverse();
        dfg.reverse();
    }
    for (source, target) in cfg {
        cpg.connect(
            source,
            target,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        )
        .expect("fixture CFG endpoints exist");
    }
    for (source, target) in dfg {
        cpg.connect(source, target, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
            .expect("fixture DFG endpoints exist");
    }

    SequenceFixture {
        cpg,
        function,
        calls,
    }
}

#[test]
fn open_use_close_is_clean_and_terminally_accepting() {
    let fixture = sequence_fixture(&["use", "close"], false);
    let fsm = open_close_fsm();
    let report = analyze_typestate(&fixture.cpg, &fsm, &IfdsCaps::default());

    assert!(report.is_complete(), "{report:#?}");
    assert!(report.violations.is_empty());
    assert_eq!(report.processed_functions, vec![fixture.function]);
    assert_eq!(report.processed_objects, BTreeSet::from([fixture.calls[0]]));
    assert_eq!(
        report.terminal_states.get(&fixture.calls[0]),
        Some(&BTreeSet::from([PropertyState::from("closed")]))
    );
    assert_eq!(
        report.object_is_accepting(fixture.calls[0], &fsm),
        Some(true)
    );
    assert!(check_typestate(&fixture.cpg, &fsm, &IfdsCaps::default()).is_empty());
}

#[test]
fn use_after_close_reports_exact_state_event_and_witness() {
    let fixture = sequence_fixture(&["close", "use"], false);
    let report = analyze_typestate(&fixture.cpg, &open_close_fsm(), &IfdsCaps::default());

    assert!(report.is_complete(), "{report:#?}");
    assert_eq!(report.violations.len(), 1);
    let violation = &report.violations[0];
    assert_eq!(violation.object, fixture.calls[0]);
    assert_eq!(violation.site, fixture.calls[2]);
    assert_eq!(violation.from.as_ref(), "closed");
    assert_eq!(violation.event.as_ref(), "use");
    assert_eq!(violation.witness.first().copied(), Some(fixture.calls[0]));
    assert!(violation.witness.contains(&fixture.calls[1]));
    assert!(violation.witness.contains(&fixture.calls[2]));
    assert!(!violation.capped);
}

#[test]
fn error_state_is_absorbing_and_suppresses_follow_on_reports() {
    let fixture = sequence_fixture(&["close", "use", "use", "close"], false);
    let report = analyze_typestate(&fixture.cpg, &open_close_fsm(), &IfdsCaps::default());

    assert!(report.is_complete(), "{report:#?}");
    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations[0].site, fixture.calls[2]);
    assert_eq!(
        report.terminal_states.get(&fixture.calls[0]),
        Some(&BTreeSet::from([PropertyState::from("error")]))
    );
}

#[test]
fn known_non_alphabet_calls_preserve_state() {
    let fixture = sequence_fixture(&["inspect", "close"], false);
    let report = analyze_typestate(&fixture.cpg, &open_close_fsm(), &IfdsCaps::default());

    assert!(report.is_complete(), "{report:#?}");
    assert!(report.violations.is_empty());
    assert_eq!(
        report.terminal_states.get(&fixture.calls[0]),
        Some(&BTreeSet::from([PropertyState::from("closed")]))
    );
}

#[test]
fn branch_join_retains_may_states_and_flags_only_forbidden_path() {
    let mut fixture = sequence_fixture(&["close", "use"], false);
    fixture
        .cpg
        .connect(
            fixture.calls[0],
            fixture.calls[2],
            CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalFalse),
        )
        .expect("branch endpoints exist");

    let report = analyze_typestate(&fixture.cpg, &open_close_fsm(), &IfdsCaps::default());

    assert!(report.is_complete(), "{report:#?}");
    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations[0].site, fixture.calls[2]);
    assert_eq!(
        report.terminal_states.get(&fixture.calls[0]),
        Some(&BTreeSet::from([
            PropertyState::from("open"),
            PropertyState::from("error"),
        ]))
    );
}

#[test]
fn represented_call_target_classifies_event_without_callee_syntax() {
    let mut fixture = sequence_fixture(&[], false);
    let close_function = add_with_id(&mut fixture.cpg, 100, function_kind("close"));
    let close_call = add_with_id(
        &mut fixture.cpg,
        104,
        CpgNodeKind::Call {
            target: Some(close_function),
            is_method: false,
        },
    );
    connect_ast(&mut fixture.cpg, fixture.function, close_call);
    fixture
        .cpg
        .connect(
            fixture.calls[0],
            close_call,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        )
        .expect("CFG endpoints exist");
    fixture
        .cpg
        .connect(
            fixture.calls[0],
            close_call,
            CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
        )
        .expect("DFG endpoints exist");

    let report = analyze_typestate(&fixture.cpg, &open_close_fsm(), &IfdsCaps::default());

    assert!(report.is_complete(), "{report:#?}");
    assert!(report.violations.is_empty());
    assert_eq!(
        report.terminal_states.get(&fixture.calls[0]),
        Some(&BTreeSet::from([PropertyState::from("closed")]))
    );
}

#[test]
fn unresolved_event_and_missing_cfg_are_explicit_incompleteness() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = add_with_id(&mut cpg, 1, function_kind("entry"));
    let open = add_with_id(
        &mut cpg,
        10,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    let open_name = add_with_id(
        &mut cpg,
        11,
        CpgNodeKind::Identifier {
            name: "open".into(),
            definition: None,
        },
    );
    let unresolved = add_with_id(
        &mut cpg,
        20,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    connect_ast(&mut cpg, function, open);
    connect_ast(&mut cpg, open, open_name);
    connect_ast(&mut cpg, function, unresolved);
    cpg.connect(open, unresolved, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
        .expect("DFG endpoints exist");

    let report = analyze_typestate(&cpg, &open_close_fsm(), &IfdsCaps::default());

    assert!(!report.is_complete());
    assert!(report
        .issues
        .contains(&TypestateIssue::UnresolvedEvent { site: unresolved }));
    assert!(report
        .issues
        .contains(&TypestateIssue::MissingControlFlow { function }));
    assert!(!report.capped);
}

#[test]
fn invalid_fsm_and_caps_stop_before_unbudgeted_work() {
    let fixture = sequence_fixture(&["close"], false);
    let invalid = PropertyFsm::new("error");
    let invalid_report = analyze_typestate(&fixture.cpg, &invalid, &IfdsCaps::default());
    assert_eq!(
        invalid_report.issues,
        BTreeSet::from([TypestateIssue::Fsm(PropertyFsmIssue::NoAllocationEvent)])
    );
    assert_eq!(invalid_report.worklist_steps, 0);

    let caps = IfdsCaps {
        max_functions: 0,
        ..IfdsCaps::default()
    };
    let capped = analyze_typestate(&fixture.cpg, &open_close_fsm(), &caps);
    assert!(capped.capped);
    assert_eq!(capped.worklist_steps, 0);
    assert!(capped.cap_events.contains(&TypestateCapEvent::Functions {
        observed: 1,
        cap: 0,
    }));

    let caps = IfdsCaps {
        max_worklist_steps: 0,
        ..IfdsCaps::default()
    };
    let capped = analyze_typestate(&fixture.cpg, &open_close_fsm(), &caps);
    assert!(capped.capped);
    assert!(capped
        .cap_events
        .contains(&TypestateCapEvent::WorklistSteps { needed: 1, cap: 0 }));
}

#[test]
fn fsm_validation_reports_closed_vocabulary_and_error_invariants() {
    let mut fsm = open_close_fsm();
    fsm.states.remove(&fsm.error);
    fsm.accepting.insert(Arc::clone(&fsm.error));
    fsm.alphabet.insert(PropertyEvent::from(""));
    fsm.states.insert(PropertyState::from(""));
    fsm.allocations
        .insert(PropertyEvent::from("ghost"), Arc::clone(&fsm.error));
    fsm.transitions.insert(
        (Arc::clone(&fsm.error), PropertyEvent::from("use")),
        PropertyState::from("open"),
    );
    fsm.transitions.insert(
        (
            PropertyState::from("missing"),
            PropertyEvent::from("unknown"),
        ),
        PropertyState::from("absent"),
    );
    fsm.transitions.insert(
        (PropertyState::from("open"), PropertyEvent::from("open")),
        PropertyState::from("open"),
    );

    let issues = fsm.validate();

    assert_eq!(issues, fsm.validate(), "validation order must be stable");
    for expected in [
        PropertyFsmIssue::EmptyState,
        PropertyFsmIssue::EmptyEvent,
        PropertyFsmIssue::UnknownErrorState {
            state: PropertyState::from("error"),
        },
        PropertyFsmIssue::AcceptingErrorState,
        PropertyFsmIssue::UnknownAllocationEvent {
            event: PropertyEvent::from("ghost"),
        },
        PropertyFsmIssue::AllocationEntersError {
            event: PropertyEvent::from("ghost"),
        },
        PropertyFsmIssue::NonAbsorbingErrorTransition {
            event: PropertyEvent::from("use"),
            to: PropertyState::from("open"),
        },
        PropertyFsmIssue::AllocationHasOrdinaryTransition {
            event: PropertyEvent::from("open"),
        },
    ] {
        assert!(
            issues.contains(&expected),
            "missing {expected:?} in {issues:#?}"
        );
    }
}

#[test]
fn fact_and_call_depth_caps_are_reported_with_exact_sites() {
    let fixture = sequence_fixture(&["close"], false);
    let facts_caps = IfdsCaps {
        max_facts_per_node: 0,
        ..IfdsCaps::default()
    };
    let facts_report = analyze_typestate(&fixture.cpg, &open_close_fsm(), &facts_caps);
    assert!(facts_report.capped);
    assert!(facts_report
        .cap_events
        .contains(&TypestateCapEvent::FactsPerNode {
            function: fixture.function,
            node: fixture.calls[0],
            observed: 1,
            cap: 0,
        }));

    let mut fixture = sequence_fixture(&[], false);
    let nested_call = add_with_id(
        &mut fixture.cpg,
        200,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    let wrapper = add_with_id(
        &mut fixture.cpg,
        201,
        CpgNodeKind::Unknown {
            kind: "callee_wrapper".into(),
        },
    );
    let close_name = add_with_id(
        &mut fixture.cpg,
        202,
        CpgNodeKind::Identifier {
            name: "close".into(),
            definition: None,
        },
    );
    connect_ast(&mut fixture.cpg, fixture.function, nested_call);
    connect_ast(&mut fixture.cpg, nested_call, wrapper);
    connect_ast(&mut fixture.cpg, wrapper, close_name);
    fixture
        .cpg
        .connect(
            fixture.calls[0],
            nested_call,
            CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
        )
        .expect("cap fixture CFG endpoints exist");
    let depth_caps = IfdsCaps {
        max_call_depth: 0,
        ..IfdsCaps::default()
    };
    let depth_report = analyze_typestate(&fixture.cpg, &open_close_fsm(), &depth_caps);
    assert!(depth_report.capped);
    assert!(depth_report
        .cap_events
        .contains(&TypestateCapEvent::CallDepth {
            site: nested_call,
            cap: 0,
        }));
}

fn oracle_first_violation(events: &[bool]) -> Option<usize> {
    let mut open = true;
    for (index, &is_close) in events.iter().enumerate() {
        if is_close {
            if !open {
                return Some(index);
            }
            open = false;
        } else if !open {
            return Some(index);
        }
    }
    None
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn generated_linear_programs_match_independent_oracle(events in prop::collection::vec(any::<bool>(), 0..48)) {
        let names: Vec<_> = events
            .iter()
            .map(|is_close| if *is_close { "close" } else { "use" })
            .collect();
        let fixture = sequence_fixture(&names, false);
        let report = analyze_typestate(&fixture.cpg, &open_close_fsm(), &IfdsCaps::default());
        let expected = oracle_first_violation(&events);

        prop_assert!(report.is_complete(), "{report:#?}");
        prop_assert_eq!(report.violations.len(), usize::from(expected.is_some()));
        if let Some(index) = expected {
            prop_assert_eq!(report.violations[0].site, fixture.calls[index + 1]);
        }
    }

    #[test]
    fn cfg_and_dfg_insertion_permutations_are_deterministic(events in prop::collection::vec(any::<bool>(), 0..48)) {
        let names: Vec<_> = events
            .iter()
            .map(|is_close| if *is_close { "close" } else { "use" })
            .collect();
        let forward = sequence_fixture(&names, false);
        let reversed = sequence_fixture(&names, true);

        prop_assert_eq!(
            analyze_typestate(&forward.cpg, &open_close_fsm(), &IfdsCaps::default()),
            analyze_typestate(&reversed.cpg, &open_close_fsm(), &IfdsCaps::default()),
        );
    }

    #[test]
    fn completeness_is_monotone_in_work_budget(
        events in prop::collection::vec(any::<bool>(), 0..32),
        low in 0usize..128,
        increment in 0usize..128,
    ) {
        let names: Vec<_> = events
            .iter()
            .map(|is_close| if *is_close { "close" } else { "use" })
            .collect();
        let fixture = sequence_fixture(&names, false);
        let low_caps = IfdsCaps {
            max_worklist_steps: low,
            ..IfdsCaps::default()
        };
        let high_caps = IfdsCaps {
            max_worklist_steps: low.saturating_add(increment),
            ..IfdsCaps::default()
        };
        let low_report = analyze_typestate(&fixture.cpg, &open_close_fsm(), &low_caps);
        let high_report = analyze_typestate(&fixture.cpg, &open_close_fsm(), &high_caps);

        if low_report.is_complete() {
            prop_assert!(high_report.is_complete());
            prop_assert_eq!(low_report.violations, high_report.violations);
            prop_assert_eq!(low_report.terminal_states, high_report.terminal_states);
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn serde_round_trips_fsm_and_report_exactly() {
    let fixture = sequence_fixture(&["close", "use"], false);
    let fsm = open_close_fsm();
    let report = analyze_typestate(&fixture.cpg, &fsm, &IfdsCaps::default());

    let fsm_json = serde_json::to_string(&fsm).expect("FSM serializes");
    let report_json = serde_json::to_string(&report).expect("report serializes");
    assert_eq!(
        serde_json::from_str::<PropertyFsm>(&fsm_json).expect("FSM deserializes"),
        fsm
    );
    assert_eq!(
        serde_json::from_str::<TypestateAnalysis>(&report_json).expect("report deserializes"),
        report
    );
}

#[test]
fn allocation_binds_through_variable_and_alias_evidence() {
    let mut fixture = sequence_fixture(&["close"], false);
    let alias = add_with_id(
        &mut fixture.cpg,
        200,
        CpgNodeKind::Variable {
            name: "alias".into(),
            var_type: None,
            scope: ScopeId::GLOBAL,
            is_mutable: false,
        },
    );
    connect_ast(&mut fixture.cpg, fixture.function, alias);
    let argument = fixture
        .cpg
        .ast_children(fixture.calls[1])
        .into_iter()
        .nth(1)
        .expect("close argument");
    fixture
        .cpg
        .connect(
            fixture.calls[0],
            alias,
            CpgEdgeKind::DataFlow(DfgEdgeKind::Alias),
        )
        .expect("alias endpoints exist");
    fixture
        .cpg
        .connect(alias, argument, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
        .expect("def-use endpoints exist");

    let report = analyze_typestate(&fixture.cpg, &open_close_fsm(), &IfdsCaps::default());
    assert!(report.is_complete(), "{report:#?}");
    assert!(report.violations.is_empty());
}
