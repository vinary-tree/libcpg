use proptest::prelude::*;

use super::*;
use crate::{
    CatalogRule, CpgNode, Language, MethodSignature, Pattern, ScopeId, SourceRange, Visibility,
};

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
    let id = cpg.add_node(node(kind).with_parent(parent));
    cpg.connect(parent, id, CpgEdgeKind::AstChild)
        .expect("fixture AST endpoints exist");
    cpg.node_mut(parent)
        .expect("fixture parent exists")
        .children
        .push(id);
    id
}

fn danger_call(cpg: &mut CodePropertyGraph, function: NodeId) -> NodeId {
    let call = child(
        cpg,
        function,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    child(
        cpg,
        call,
        CpgNodeKind::Identifier {
            name: "danger".into(),
            definition: None,
        },
    );
    call
}

fn add_guard(cpg: &mut CodePropertyGraph, function: NodeId, call: NodeId, name: &str) -> NodeId {
    let guard = child(
        cpg,
        function,
        CpgNodeKind::BinaryOp {
            operator: ">".into(),
        },
    );
    child(
        cpg,
        guard,
        CpgNodeKind::Identifier {
            name: name.into(),
            definition: None,
        },
    );
    child(
        cpg,
        guard,
        CpgNodeKind::Literal {
            kind: LiteralKind::Integer(0),
        },
    );
    cpg.connect(guard, call, CpgEdgeKind::ControlDependence)
        .expect("fixture PDG endpoints exist");
    guard
}

fn config() -> MissingCheckConfig {
    let mut catalog = TaintCatalog::empty();
    catalog.sinks.push(CatalogRule {
        pattern: Pattern::Exact("danger".into()),
        class: "critical".into(),
        cwe: Some(862),
    });
    MissingCheckConfig {
        catalog,
        peer_scope: PeerScope::WholeProgram,
        ..MissingCheckConfig::default()
    }
}

fn population(guarded: usize, unguarded: usize) -> (CodePropertyGraph, Vec<NodeId>) {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let mut calls = Vec::new();
    for index in 0..guarded + unguarded {
        let function = cpg.add_node(node(function(&format!("peer_{index}"))));
        let call = danger_call(&mut cpg, function);
        if index < guarded {
            add_guard(&mut cpg, function, call, &format!("value_{index}"));
        }
        calls.push(call);
    }
    (cpg, calls)
}

#[test]
fn four_of_five_guarded_calls_flag_only_the_fifth_with_exact_support() {
    let (cpg, calls) = population(4, 1);
    let before = (cpg.node_count(), cpg.edge_count());

    let report = analyze_missing_checks(&cpg, &config()).expect("valid analysis configuration");

    assert!(report.is_complete(), "{report:#?}");
    assert_eq!(report.findings.len(), 1, "{report:#?}");
    let finding = &report.findings[0];
    assert_eq!(finding.site, calls[4]);
    assert_eq!(finding.peer_sites, calls);
    assert_eq!(finding.expected_checks.len(), 1);
    assert_eq!(
        finding.peer_support[&finding.expected_checks[0]],
        PopulationFraction::new(4, 5)
    );
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}

#[test]
fn identifier_names_are_abstracted_but_literals_and_operators_remain_semantic() {
    let (mut cpg, calls) = population(3, 1);
    let fourth_function = cpg
        .ast_ancestors(calls[3])
        .into_iter()
        .find(|id| {
            cpg.node(*id)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Function { .. }))
        })
        .expect("call belongs to a function");
    let different = child(
        &mut cpg,
        fourth_function,
        CpgNodeKind::BinaryOp {
            operator: "<".into(),
        },
    );
    child(
        &mut cpg,
        different,
        CpgNodeKind::Identifier {
            name: "value_renamed".into(),
            definition: None,
        },
    );
    child(
        &mut cpg,
        different,
        CpgNodeKind::Literal {
            kind: LiteralKind::Integer(1),
        },
    );
    cpg.connect(different, calls[3], CpgEdgeKind::ControlDependence)
        .expect("fixture PDG endpoints exist");

    let mut options = config();
    options.minimum_support = PopulationThreshold::new(3, 4);
    let report = analyze_missing_checks(&cpg, &options).expect("valid analysis configuration");

    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.findings[0].site, calls[3]);
    assert_eq!(report.findings[0].expected_checks.len(), 1);
    assert!(report.findings[0].expected_checks[0]
        .nodes
        .iter()
        .any(|node| matches!(&node.kind, CheckNodeKind::BinaryOperator(op) if op.as_ref() == ">")));
}

#[test]
fn call_graph_radius_two_connects_callers_through_the_shared_api() {
    let (mut cpg, calls) = population(4, 1);
    let target = cpg.add_node(node(function("danger")));
    for call in &calls {
        cpg.node_mut(*call).expect("call exists").kind = CpgNodeKind::Call {
            target: Some(target),
            is_method: false,
        };
        cpg.connect(*call, target, CpgEdgeKind::StaticCall)
            .expect("call graph endpoints exist");
    }
    let mut options = config();
    options.peer_scope = PeerScope::CallGraph { max_distance: 2 };

    let report = analyze_missing_checks(&cpg, &options).expect("valid analysis configuration");

    assert_eq!(report.findings.len(), 1, "{report:#?}");
    assert_eq!(report.findings[0].site, calls[4]);
    assert!(matches!(
        report.findings[0].api,
        ApiIdentity::Resolved { .. }
    ));
}

#[test]
fn sensitive_class_filter_excludes_other_catalog_classes() {
    let (cpg, _) = population(4, 1);
    let mut options = config();
    options.sensitive_classes.insert("different".into());

    let report = analyze_missing_checks(&cpg, &options).expect("valid analysis configuration");

    assert!(report.findings.is_empty());
    assert!(report.analyzed_sites.is_empty());
}

#[test]
fn unresolved_calls_and_cyclic_guard_asts_are_explicit_issues() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let function = cpg.add_node(node(function("broken")));
    let unidentified = child(
        &mut cpg,
        function,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    let call = danger_call(&mut cpg, function);
    let guard = child(
        &mut cpg,
        function,
        CpgNodeKind::Unknown {
            kind: "guard".into(),
        },
    );
    cpg.connect(guard, guard, CpgEdgeKind::AstChild)
        .expect("self-cycle endpoint exists");
    cpg.connect(guard, call, CpgEdgeKind::ControlDependence)
        .expect("PDG endpoints exist");

    let report = analyze_missing_checks(&cpg, &config()).expect("valid analysis configuration");

    assert!(!report.is_complete());
    assert!(report
        .issues
        .contains(&MissingCheckIssue::MissingCalleeIdentity { site: unidentified }));
    assert!(report
        .issues
        .contains(&MissingCheckIssue::CyclicCheckAst { guard }));
    assert!(!report.analyzed_sites.contains(&call));
}

#[test]
fn fingerprint_node_cap_excludes_incomplete_site_from_population() {
    let (cpg, calls) = population(1, 1);
    let mut options = config();
    options.max_check_nodes = 2;
    options.minimum_support = PopulationThreshold::new(1, 2);

    let report = analyze_missing_checks(&cpg, &options).expect("valid analysis configuration");

    assert!(report.findings.is_empty());
    assert_eq!(report.analyzed_sites, vec![calls[1]]);
    assert!(report
        .issues
        .iter()
        .any(|issue| matches!(issue, MissingCheckIssue::CheckNodeCap { cap: 2, .. })));
}

#[test]
fn invalid_population_and_fingerprint_options_are_rejected() {
    for options in [
        MissingCheckConfig {
            minimum_support: PopulationThreshold::new(1, 0),
            ..config()
        },
        MissingCheckConfig {
            minimum_support: PopulationThreshold::new(2, 1),
            ..config()
        },
        MissingCheckConfig {
            minimum_population: 0,
            ..config()
        },
        MissingCheckConfig {
            max_check_nodes: 0,
            ..config()
        },
    ] {
        assert!(matches!(
            analyze_missing_checks(&CodePropertyGraph::new(Language::Unknown), &options),
            Err(AnalysisError::InvalidInput { .. })
        ));
    }
}

#[test]
fn deep_guard_fingerprint_is_stack_safe_on_a_small_native_stack() {
    const DEPTH: usize = 100_000;
    std::thread::Builder::new()
        .name("missing-check-small-stack".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let (mut cpg, calls) = population(0, 2);
            let function = cpg
                .ast_ancestors(calls[0])
                .into_iter()
                .find(|id| {
                    cpg.node(*id)
                        .is_some_and(|node| matches!(node.kind, CpgNodeKind::Function { .. }))
                })
                .expect("call belongs to a function");
            let guard = child(
                &mut cpg,
                function,
                CpgNodeKind::Unknown {
                    kind: "guard".into(),
                },
            );
            let mut parent = guard;
            for _ in 0..DEPTH {
                parent = child(
                    &mut cpg,
                    parent,
                    CpgNodeKind::Block {
                        scope: ScopeId::GLOBAL,
                    },
                );
            }
            cpg.connect(guard, calls[0], CpgEdgeKind::ControlDependence)
                .expect("PDG endpoints exist");
            let mut options = config();
            options.minimum_support = PopulationThreshold::new(1, 2);
            options.max_check_nodes = DEPTH + 1;

            let report = analyze_missing_checks(&cpg, &options).expect("deep analysis succeeds");

            assert!(report.is_complete(), "{report:#?}");
            assert_eq!(report.findings.len(), 1);
            assert_eq!(report.findings[0].site, calls[1]);
            assert_eq!(report.findings[0].expected_checks[0].nodes.len(), DEPTH + 1);
        })
        .expect("spawn small-stack thread")
        .join()
        .expect("missing-check detector does not overflow");
}

proptest! {
    #[test]
    fn peer_fraction_is_exact_and_repeatable(
        population_size in 2usize..40,
        missing in 1usize..10,
    ) {
        let missing = missing.min(population_size - 1);
        let guarded = population_size - missing;
        let (cpg, calls) = population(guarded, missing);
        let mut options = config();
        options.minimum_support = PopulationThreshold::new(1, population_size);
        let first = analyze_missing_checks(&cpg, &options).expect("valid generated population");
        let second = analyze_missing_checks(&cpg, &options).expect("repeat generated population");

        prop_assert_eq!(&first, &second);
        prop_assert_eq!(first.findings.len(), missing);
        for finding in first.findings {
            prop_assert!(calls[guarded..].contains(&finding.site));
            prop_assert_eq!(
                finding.peer_support[&finding.expected_checks[0]],
                PopulationFraction::new(guarded, population_size),
            );
        }
    }
}

#[cfg(feature = "serde")]
#[test]
fn report_round_trips_with_exact_fraction_and_flat_fingerprint() {
    let (cpg, _) = population(4, 1);
    let report = analyze_missing_checks(&cpg, &config()).expect("valid analysis configuration");
    let encoded = serde_json::to_vec(&report).expect("serialize missing-check report");
    let decoded: MissingCheckAnalysis =
        serde_json::from_slice(&encoded).expect("deserialize missing-check report");
    assert_eq!(decoded, report);
}
