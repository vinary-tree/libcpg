use proptest::prelude::*;

use super::*;
use crate::{
    CfgEdgeKind, CpgNode, CpgNodeKind, DfgEdgeKind, Language, MethodSignature, SourceRange,
    Visibility,
};

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn function(name: &str) -> CpgNodeKind {
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

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let id = cpg.add_node(node(kind).with_parent(parent));
    cpg.connect(parent, id, CpgEdgeKind::AstChild)
        .expect("fixture AST endpoints exist");
    id
}

fn call(cpg: &mut CodePropertyGraph, function: NodeId, name: &str) -> NodeId {
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
            name: name.into(),
            definition: None,
        },
    );
    call
}

fn cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId) {
    cpg.connect(
        source,
        target,
        CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
    )
    .expect("fixture CFG endpoints exist");
}

fn return_population(checked: usize, unchecked: usize) -> (CodePropertyGraph, Vec<NodeId>) {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let mut calls = Vec::new();
    for index in 0..checked + unchecked {
        let function = cpg.add_node(node(function(&format!("peer_{index}"))));
        let call = call(&mut cpg, function, "read_value");
        let exit = child(&mut cpg, function, CpgNodeKind::Return);
        cfg(&mut cpg, function, call);
        cfg(&mut cpg, call, exit);
        if index < checked {
            let guard = child(&mut cpg, function, CpgNodeKind::If);
            let guarded = child(
                &mut cpg,
                guard,
                CpgNodeKind::Unknown {
                    kind: "then".into(),
                },
            );
            cpg.connect(call, guard, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
                .expect("fixture DFG endpoints exist");
            cpg.connect(guard, guarded, CpgEdgeKind::ControlDependence)
                .expect("fixture PDG endpoints exist");
        }
        calls.push(call);
    }
    (cpg, calls)
}

#[test]
fn nine_of_ten_checked_returns_flag_only_the_tenth() {
    let (cpg, calls) = return_population(9, 1);
    let before = (cpg.node_count(), cpg.edge_count());

    let report = analyze_deviant_behavior(&cpg, &DeviantBehaviorConfig::default())
        .expect("valid analysis configuration");

    assert!(report.is_complete(), "{report:#?}");
    assert_eq!(report.return_check_beliefs.len(), 1, "{report:#?}");
    assert_eq!(
        report.return_check_beliefs[0].support,
        PopulationFraction::new(9, 10)
    );
    let unchecked: Vec<_> = report
        .findings
        .iter()
        .filter(|finding| matches!(finding.kind, DeviantBehaviorKind::UncheckedReturn))
        .collect();
    assert_eq!(unchecked.len(), 1, "{report:#?}");
    assert_eq!(unchecked[0].site, calls[9]);
    assert_eq!(unchecked[0].peer_support, PopulationFraction::new(9, 10));
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}

#[test]
fn four_of_five_predecessors_reaching_a_successor_flag_the_deviation() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let mut predecessors = Vec::new();
    for index in 0..5 {
        let function = cpg.add_node(node(function(&format!("order_{index}"))));
        let predecessor = call(&mut cpg, function, "acquire");
        let exit = child(&mut cpg, function, CpgNodeKind::Return);
        cfg(&mut cpg, function, predecessor);
        if index < 4 {
            let successor = call(&mut cpg, function, "release");
            cfg(&mut cpg, predecessor, successor);
            cfg(&mut cpg, successor, exit);
        } else {
            cfg(&mut cpg, predecessor, exit);
        }
        predecessors.push(predecessor);
    }
    let options = DeviantBehaviorConfig {
        minimum_support: PopulationThreshold::new(4, 5),
        ..DeviantBehaviorConfig::default()
    };

    let report = analyze_deviant_behavior(&cpg, &options).expect("valid analysis configuration");

    let belief = report
        .call_order_beliefs
        .iter()
        .find(|belief| {
            belief.predecessor.name() == "acquire" && belief.successor.name() == "release"
        })
        .expect("acquire-then-release belief");
    assert_eq!(belief.support, PopulationFraction::new(4, 5));
    let deviations: Vec<_> = report
        .findings
        .iter()
        .filter(|finding| {
            matches!(
                &finding.kind,
                DeviantBehaviorKind::MissingFollowingCall { expected }
                    if expected.name() == "release"
            )
        })
        .collect();
    assert_eq!(deviations.len(), 1, "{report:#?}");
    assert_eq!(deviations[0].site, predecessors[4]);
}

#[test]
fn use_def_back_edges_do_not_manufacture_a_return_check() {
    let (mut cpg, calls) = return_population(0, 2);
    let guard = cpg.add_node(node(CpgNodeKind::If));
    let controlled = cpg.add_node(node(CpgNodeKind::Return));
    cpg.connect(guard, controlled, CpgEdgeKind::ControlDependence)
        .expect("fixture PDG endpoints exist");
    cpg.connect(calls[0], guard, CpgEdgeKind::DataFlow(DfgEdgeKind::UseDef))
        .expect("fixture reverse DFG endpoints exist");
    let options = DeviantBehaviorConfig {
        minimum_support: PopulationThreshold::new(1, 2),
        ..DeviantBehaviorConfig::default()
    };

    let report = analyze_deviant_behavior(&cpg, &options).expect("valid analysis configuration");

    assert!(report.return_check_beliefs.is_empty(), "{report:#?}");
    assert!(!report
        .findings
        .iter()
        .any(|finding| matches!(finding.kind, DeviantBehaviorKind::UncheckedReturn)));
}

#[test]
fn incomplete_return_and_cfg_traversals_are_excluded_and_reported() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let function = cpg.add_node(node(function("bounded")));
    let first = call(&mut cpg, function, "first");
    let middle = child(
        &mut cpg,
        function,
        CpgNodeKind::Unknown {
            kind: "middle".into(),
        },
    );
    let last = call(&mut cpg, function, "last");
    cfg(&mut cpg, function, first);
    cfg(&mut cpg, first, middle);
    cfg(&mut cpg, middle, last);
    cpg.connect(first, middle, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
        .expect("fixture DFG endpoints exist");
    cpg.connect(middle, last, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
        .expect("fixture DFG endpoints exist");
    let options = DeviantBehaviorConfig {
        max_return_evidence_nodes: 1,
        max_cfg_nodes_per_call: 1,
        ..DeviantBehaviorConfig::default()
    };

    let report = analyze_deviant_behavior(&cpg, &options).expect("valid analysis configuration");

    assert!(!report.is_complete());
    assert!(report.issues.iter().any(|issue| matches!(
        issue,
        DeviantBehaviorIssue::ReturnEvidenceCap { site, .. } if *site == first
    )));
    assert!(report.issues.iter().any(|issue| matches!(
        issue,
        DeviantBehaviorIssue::CfgEvidenceCap { site, .. } if *site == first
    )));
}

#[test]
fn missing_identity_and_missing_cfg_are_explicit_issues() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let function = cpg.add_node(node(function("incomplete")));
    let unknown = child(
        &mut cpg,
        function,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    let isolated = call(&mut cpg, function, "isolated");

    let report = analyze_deviant_behavior(&cpg, &DeviantBehaviorConfig::default())
        .expect("valid analysis configuration");

    assert!(report
        .issues
        .contains(&DeviantBehaviorIssue::MissingCalleeIdentity { site: unknown }));
    assert!(report
        .issues
        .contains(&DeviantBehaviorIssue::CallOutsideCfg {
            site: isolated,
            function,
        }));
}

#[test]
fn invalid_threshold_population_and_budgets_are_rejected() {
    for options in [
        DeviantBehaviorConfig {
            minimum_support: PopulationThreshold::new(1, 0),
            ..DeviantBehaviorConfig::default()
        },
        DeviantBehaviorConfig {
            minimum_support: PopulationThreshold::new(2, 1),
            ..DeviantBehaviorConfig::default()
        },
        DeviantBehaviorConfig {
            minimum_population: 0,
            ..DeviantBehaviorConfig::default()
        },
        DeviantBehaviorConfig {
            max_return_evidence_nodes: 0,
            ..DeviantBehaviorConfig::default()
        },
        DeviantBehaviorConfig {
            max_cfg_nodes_per_call: 0,
            ..DeviantBehaviorConfig::default()
        },
    ] {
        assert!(matches!(
            analyze_deviant_behavior(&CodePropertyGraph::new(Language::Unknown), &options),
            Err(AnalysisError::InvalidInput { .. })
        ));
    }
}

#[test]
fn deep_cfg_order_traversal_is_stack_safe_on_a_small_native_stack() {
    const DEPTH: usize = 100_000;
    std::thread::Builder::new()
        .name("deviant-behavior-small-stack".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut cpg = CodePropertyGraph::new(Language::Unknown);
            let deep_function = cpg.add_node(node(function("deep")));
            let deep_first = call(&mut cpg, deep_function, "first");
            cfg(&mut cpg, deep_function, deep_first);
            let mut previous = deep_first;
            for _ in 0..DEPTH {
                let next = child(
                    &mut cpg,
                    deep_function,
                    CpgNodeKind::Unknown {
                        kind: "step".into(),
                    },
                );
                cfg(&mut cpg, previous, next);
                previous = next;
            }
            let deep_last = call(&mut cpg, deep_function, "last");
            cfg(&mut cpg, previous, deep_last);

            let short_function = cpg.add_node(node(function("short")));
            let short_first = call(&mut cpg, short_function, "first");
            let short_exit = child(&mut cpg, short_function, CpgNodeKind::Return);
            cfg(&mut cpg, short_function, short_first);
            cfg(&mut cpg, short_first, short_exit);
            let options = DeviantBehaviorConfig {
                minimum_support: PopulationThreshold::new(1, 2),
                max_cfg_nodes_per_call: DEPTH + 2,
                ..DeviantBehaviorConfig::default()
            };

            let report = analyze_deviant_behavior(&cpg, &options).expect("deep analysis succeeds");

            assert!(report.is_complete(), "{report:#?}");
            assert!(report.findings.iter().any(|finding| {
                finding.site == short_first
                    && matches!(
                        &finding.kind,
                        DeviantBehaviorKind::MissingFollowingCall { expected }
                            if expected.name() == "last"
                    )
            }));
        })
        .expect("spawn small-stack thread")
        .join()
        .expect("deviant-behavior detector does not overflow");
}

proptest! {
    #[test]
    fn return_check_fraction_is_exact_and_repeatable(
        population_size in 2usize..40,
        unchecked in 1usize..10,
    ) {
        let unchecked = unchecked.min(population_size - 1);
        let checked = population_size - unchecked;
        let (cpg, calls) = return_population(checked, unchecked);
        let options = DeviantBehaviorConfig {
            minimum_support: PopulationThreshold::new(1, population_size),
            ..DeviantBehaviorConfig::default()
        };
        let first = analyze_deviant_behavior(&cpg, &options).expect("valid generated population");
        let second = analyze_deviant_behavior(&cpg, &options).expect("repeat generated population");

        prop_assert_eq!(&first, &second);
        let belief = first.return_check_beliefs.iter()
            .find(|belief| belief.api.name() == "read_value")
            .expect("generated checked-return belief");
        prop_assert_eq!(belief.support, PopulationFraction::new(checked, population_size));
        let findings: Vec<_> = first.findings.iter()
            .filter(|finding| matches!(finding.kind, DeviantBehaviorKind::UncheckedReturn))
            .collect();
        prop_assert_eq!(findings.len(), unchecked);
        prop_assert!(findings.iter().all(|finding| calls[checked..].contains(&finding.site)));
    }
}

#[cfg(feature = "serde")]
#[test]
fn report_round_trips_with_exact_beliefs() {
    let (cpg, _) = return_population(9, 1);
    let report = analyze_deviant_behavior(&cpg, &DeviantBehaviorConfig::default())
        .expect("valid analysis configuration");
    let encoded = serde_json::to_vec(&report).expect("serialize deviant report");
    let decoded: DeviantBehaviorAnalysis =
        serde_json::from_slice(&encoded).expect("deserialize deviant report");
    assert_eq!(decoded, report);
}
