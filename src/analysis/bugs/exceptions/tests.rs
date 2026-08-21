use proptest::prelude::*;

use super::*;
use crate::{
    CpgEdgeKind, CpgNode, Language, MethodSignature, ScopeId, SourceRange, TypeInfo, Visibility,
};

fn add(cpg: &mut CodePropertyGraph, id: u32, kind: CpgNodeKind) -> NodeId {
    let id = NodeId::new(id);
    cpg.add_node_with_id(CpgNode::new(
        id,
        kind,
        SourceRange::from_bytes(id.as_u32() * 5, id.as_u32() * 5 + 1),
    ));
    id
}

fn function_kind(name: &str) -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: Box::new(MethodSignature {
            name: name.into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        }),
    }
}

fn block_kind() -> CpgNodeKind {
    CpgNodeKind::Block {
        scope: ScopeId::GLOBAL,
    }
}

fn ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("fixture AST endpoints exist");
}

fn function_shell(cpg: &mut CodePropertyGraph, id: u32, name: &str) -> (NodeId, NodeId) {
    let function = add(cpg, id, function_kind(name));
    let body = add(cpg, id + 1, block_kind());
    ast(cpg, function, body);
    (function, body)
}

fn typed_throw(
    cpg: &mut CodePropertyGraph,
    parent: NodeId,
    id: u32,
    exception: Option<&str>,
) -> NodeId {
    let throw = add(cpg, id, CpgNodeKind::Throw);
    ast(cpg, parent, throw);
    if let Some(exception) = exception {
        let value = add(
            cpg,
            id + 1,
            CpgNodeKind::TypeAnnotation {
                type_info: Box::new(TypeInfo::new(exception)),
            },
        );
        ast(cpg, throw, value);
    }
    throw
}

fn typed_catch(
    cpg: &mut CodePropertyGraph,
    try_node: NodeId,
    id: u32,
    exception: Option<&str>,
) -> (NodeId, NodeId) {
    let catch = add(cpg, id, CpgNodeKind::Catch);
    ast(cpg, try_node, catch);
    if let Some(exception) = exception {
        let parameter = add(
            cpg,
            id + 1,
            CpgNodeKind::Parameter {
                name: "error".into(),
                param_type: Some(Box::new(TypeInfo::new(exception))),
                is_variadic: false,
            },
        );
        ast(cpg, catch, parameter);
    }
    let body = add(cpg, id + 2, block_kind());
    ast(cpg, catch, body);
    cpg.connect(catch, body, CpgEdgeKind::ControlFlow(CfgEdgeKind::Catch))
        .expect("fixture catch CFG endpoints exist");
    (catch, body)
}

fn try_shell(cpg: &mut CodePropertyGraph, parent: NodeId, id: u32) -> (NodeId, NodeId) {
    let try_node = add(cpg, id, CpgNodeKind::Try);
    let body = add(cpg, id + 1, block_kind());
    ast(cpg, parent, try_node);
    ast(cpg, try_node, body);
    (try_node, body)
}

#[test]
fn direct_uncaught_throw_reaches_the_root_entry_with_a_typed_witness() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let (function, body) = function_shell(&mut cpg, 1, "entry");
    let throw = typed_throw(&mut cpg, body, 10, Some("Failure"));
    let before = (cpg.node_count(), cpg.edge_count());

    let report = analyze_exception_flow(&cpg, &IfdsCaps::default());

    assert!(report.is_complete(), "{report:#?}");
    assert_eq!(
        report.summaries[&function].escaping,
        BTreeSet::from([ExceptionType::Named("Failure".into())])
    );
    assert_eq!(report.uncaught.len(), 1, "{report:#?}");
    assert_eq!(report.uncaught[0].entry, function);
    assert_eq!(report.uncaught[0].origin, throw);
    assert_eq!(report.uncaught[0].witness, vec![function, throw]);
    assert!(report.dead_catches.is_empty());
    assert_eq!(
        uncaught_exceptions(&cpg, &IfdsCaps::default()),
        report.uncaught
    );
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}

#[test]
fn matching_handler_consumes_the_throw_and_a_nonmatching_handler_is_dead() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let (function, body) = function_shell(&mut cpg, 1, "entry");
    let (try_node, try_body) = try_shell(&mut cpg, body, 10);
    typed_throw(&mut cpg, try_body, 20, Some("Failure"));
    let (matching, _) = typed_catch(&mut cpg, try_node, 30, Some("Failure"));
    let (dead, _) = typed_catch(&mut cpg, try_node, 40, Some("Other"));

    let report = analyze_exception_flow(&cpg, &IfdsCaps::default());

    assert!(report.is_complete(), "{report:#?}");
    assert!(report.summaries[&function].escaping.is_empty());
    assert!(report.uncaught.is_empty());
    assert_eq!(
        report.reached_catches[&matching],
        BTreeSet::from([ExceptionType::Named("Failure".into())])
    );
    assert_eq!(report.dead_catches.len(), 1, "{report:#?}");
    assert_eq!(report.dead_catches[0].catch, dead);
    assert_eq!(
        dead_catches(&cpg, &IfdsCaps::default()),
        report.dead_catches
    );
}

#[test]
fn catch_all_consumes_unknown_types_but_unknown_throw_evidence_remains_explicit() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let (function, body) = function_shell(&mut cpg, 1, "entry");
    let (try_node, try_body) = try_shell(&mut cpg, body, 10);
    let throw = typed_throw(&mut cpg, try_body, 20, None);
    let (catch, _) = typed_catch(&mut cpg, try_node, 30, None);

    let report = analyze_exception_flow(&cpg, &IfdsCaps::default());

    assert!(!report.is_complete());
    assert!(report.summaries[&function].escaping.is_empty());
    assert!(report.uncaught.is_empty());
    assert_eq!(
        report.reached_catches[&catch],
        BTreeSet::from([ExceptionType::Unknown])
    );
    assert!(report
        .issues
        .contains(&ExceptionFlowIssue::UnknownThrowType { site: throw }));
}

#[test]
fn subtype_dispatch_follows_iterative_inheritance_closure() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let parent = add(
        &mut cpg,
        100,
        CpgNodeKind::Class {
            name: "Failure".into(),
            is_abstract: true,
        },
    );
    let middle = add(
        &mut cpg,
        101,
        CpgNodeKind::Class {
            name: "IoFailure".into(),
            is_abstract: true,
        },
    );
    let child = add(
        &mut cpg,
        102,
        CpgNodeKind::Class {
            name: "DiskFailure".into(),
            is_abstract: false,
        },
    );
    cpg.connect(child, middle, CpgEdgeKind::Inherits)
        .expect("child inheritance endpoints exist");
    cpg.connect(middle, parent, CpgEdgeKind::Inherits)
        .expect("parent inheritance endpoints exist");
    let (function, body) = function_shell(&mut cpg, 1, "entry");
    let (try_node, try_body) = try_shell(&mut cpg, body, 10);
    typed_throw(&mut cpg, try_body, 20, Some("DiskFailure"));
    let (catch, _) = typed_catch(&mut cpg, try_node, 30, Some("Failure"));

    let report = analyze_exception_flow(&cpg, &IfdsCaps::default());

    assert!(report.is_complete(), "{report:#?}");
    assert!(report.summaries[&function].escaping.is_empty());
    assert_eq!(
        report.reached_catches[&catch],
        BTreeSet::from([ExceptionType::Named("DiskFailure".into())])
    );
}

#[test]
fn propagated_callee_throw_is_caught_in_the_caller() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let (caller, caller_body) = function_shell(&mut cpg, 1, "caller");
    let (callee, callee_body) = function_shell(&mut cpg, 100, "callee");
    typed_throw(&mut cpg, callee_body, 110, Some("Failure"));
    let (try_node, try_body) = try_shell(&mut cpg, caller_body, 10);
    let call = add(
        &mut cpg,
        20,
        CpgNodeKind::Call {
            target: Some(callee),
            is_method: false,
        },
    );
    ast(&mut cpg, try_body, call);
    let (catch, _) = typed_catch(&mut cpg, try_node, 30, Some("Failure"));

    let report = analyze_exception_flow(&cpg, &IfdsCaps::default());

    assert!(report.is_complete(), "{report:#?}");
    assert_eq!(
        report.summaries[&callee].escaping,
        BTreeSet::from([ExceptionType::Named("Failure".into())])
    );
    assert!(report.summaries[&caller].escaping.is_empty());
    assert!(report.uncaught.is_empty());
    assert_eq!(
        report.reached_catches[&catch],
        BTreeSet::from([ExceptionType::Named("Failure".into())])
    );
}

#[test]
fn uncaught_callee_throw_retains_a_complete_interprocedural_witness() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let (caller, caller_body) = function_shell(&mut cpg, 1, "caller");
    let (callee, callee_body) = function_shell(&mut cpg, 100, "callee");
    let throw = typed_throw(&mut cpg, callee_body, 110, Some("Failure"));
    let call = add(
        &mut cpg,
        20,
        CpgNodeKind::Call {
            target: Some(callee),
            is_method: false,
        },
    );
    ast(&mut cpg, caller_body, call);

    let report = analyze_exception_flow(&cpg, &IfdsCaps::default());

    assert!(report.is_complete(), "{report:#?}");
    assert_eq!(report.uncaught.len(), 1, "{report:#?}");
    assert_eq!(report.uncaught[0].entry, caller);
    assert_eq!(report.uncaught[0].origin, call);
    assert_eq!(
        report.uncaught[0].witness,
        vec![caller, call, callee, throw]
    );
}

#[test]
fn nested_dispatch_skips_inner_mismatch_and_reaches_outer_handler() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let (function, body) = function_shell(&mut cpg, 1, "entry");
    let (outer_try, outer_body) = try_shell(&mut cpg, body, 10);
    let (inner_try, inner_body) = try_shell(&mut cpg, outer_body, 20);
    typed_throw(&mut cpg, inner_body, 30, Some("Failure"));
    let (inner_catch, _) = typed_catch(&mut cpg, inner_try, 40, Some("Other"));
    let (outer_catch, _) = typed_catch(&mut cpg, outer_try, 50, Some("Failure"));

    let report = analyze_exception_flow(&cpg, &IfdsCaps::default());

    assert!(report.is_complete(), "{report:#?}");
    assert!(report.summaries[&function].escaping.is_empty());
    assert!(report.reached_catches.contains_key(&outer_catch));
    assert!(!report.reached_catches.contains_key(&inner_catch));
    assert_eq!(report.dead_catches[0].catch, inner_catch);
}

#[test]
fn every_applicable_shared_solver_cap_is_preserved() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let (function, body) = function_shell(&mut cpg, 1, "recursive");
    typed_throw(&mut cpg, body, 10, Some("Failure"));
    let call = add(
        &mut cpg,
        20,
        CpgNodeKind::Call {
            target: Some(function),
            is_method: false,
        },
    );
    ast(&mut cpg, body, call);

    let cases = [
        IfdsCaps {
            max_functions: 0,
            ..IfdsCaps::default()
        },
        IfdsCaps {
            max_worklist_steps: 0,
            ..IfdsCaps::default()
        },
        IfdsCaps {
            max_facts_per_node: 0,
            ..IfdsCaps::default()
        },
        IfdsCaps {
            max_scc_iters: 0,
            ..IfdsCaps::default()
        },
    ];
    for caps in cases {
        let report = analyze_exception_flow(&cpg, &caps);
        assert!(!report.is_complete(), "{caps:#?}: {report:#?}");
        assert!(report.capped, "{caps:#?}: {report:#?}");
        assert!(!report.cap_events.is_empty(), "{caps:#?}: {report:#?}");
    }
}

#[test]
fn ambiguous_calls_and_missing_typed_handler_entries_are_issues() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let (_entry, body) = function_shell(&mut cpg, 1, "entry");
    let (left, _) = function_shell(&mut cpg, 100, "left");
    let (right, _) = function_shell(&mut cpg, 200, "right");
    let call = add(
        &mut cpg,
        10,
        CpgNodeKind::Call {
            target: Some(left),
            is_method: false,
        },
    );
    ast(&mut cpg, body, call);
    cpg.connect(call, right, CpgEdgeKind::StaticCall)
        .expect("ambiguous call endpoints exist");
    let (try_node, _) = try_shell(&mut cpg, body, 20);
    let catch = add(&mut cpg, 30, CpgNodeKind::Catch);
    let catch_body = add(&mut cpg, 31, block_kind());
    ast(&mut cpg, try_node, catch);
    ast(&mut cpg, catch, catch_body);

    let report = analyze_exception_flow(&cpg, &IfdsCaps::default());

    assert!(!report.is_complete());
    assert!(report
        .issues
        .contains(&ExceptionFlowIssue::AmbiguousCall { site: call }));
    assert!(report
        .issues
        .contains(&ExceptionFlowIssue::MissingCatchEdge { catch }));
}

fn generated_chain(
    length: usize,
    catcher: Option<usize>,
    reverse_relations: bool,
) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let mut functions = Vec::new();
    let mut bodies = Vec::new();
    for index in 0..length {
        let base = 1 + index as u32 * 100;
        let (function, body) = function_shell(&mut cpg, base, &format!("f{index}"));
        functions.push(function);
        bodies.push(body);
    }
    let leaf_base = 1 + (length - 1) as u32 * 100;
    typed_throw(
        &mut cpg,
        bodies[length - 1],
        leaf_base + 10,
        Some("Failure"),
    );

    let mut call_edges = Vec::new();
    for index in 0..length.saturating_sub(1) {
        let base = 1 + index as u32 * 100;
        let parent = if catcher == Some(index) {
            let (try_node, try_body) = try_shell(&mut cpg, bodies[index], base + 20);
            typed_catch(&mut cpg, try_node, base + 40, Some("Failure"));
            try_body
        } else {
            bodies[index]
        };
        let call = add(
            &mut cpg,
            base + 10,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        );
        ast(&mut cpg, parent, call);
        call_edges.push((call, functions[index + 1]));
    }
    if reverse_relations {
        call_edges.reverse();
    }
    for (call, callee) in call_edges {
        cpg.connect(call, callee, CpgEdgeKind::StaticCall)
            .expect("generated call endpoints exist");
    }
    cpg
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn generated_call_chains_match_the_independent_handler_oracle(
        length in 1usize..20,
        catch_seed in any::<Option<usize>>(),
    ) {
        let catcher = catch_seed.map(|seed| seed % length.saturating_sub(1).max(1));
        let catcher = (length > 1).then_some(catcher).flatten();
        let forward = generated_chain(length, catcher, false);
        let reversed = generated_chain(length, catcher, true);
        let forward_report = analyze_exception_flow(&forward, &IfdsCaps::default());
        let reversed_report = analyze_exception_flow(&reversed, &IfdsCaps::default());

        prop_assert!(forward_report.is_complete(), "{forward_report:#?}");
        prop_assert_eq!(forward_report.clone(), reversed_report);
        prop_assert_eq!(forward_report.uncaught.is_empty(), catcher.is_some());
        prop_assert_eq!(forward_report.reached_catches.len(), usize::from(catcher.is_some()));
    }
}

#[cfg(feature = "serde")]
#[test]
fn report_round_trips_through_json() {
    let cpg = generated_chain(4, Some(1), false);
    let report = analyze_exception_flow(&cpg, &IfdsCaps::default());
    let encoded = serde_json::to_string(&report).expect("serialize exception-flow report");
    let decoded: ExceptionFlowAnalysis =
        serde_json::from_str(&encoded).expect("deserialize exception-flow report");
    assert_eq!(decoded, report);
}
