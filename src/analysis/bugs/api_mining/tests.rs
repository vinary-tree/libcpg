use proptest::prelude::*;

use super::*;
use crate::{
    CfgEdgeKind, CpgEdgeKind, CpgNode, DfgEdgeKind, Language, MethodSignature, ScopeId,
    SourceRange, Visibility,
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

fn call(
    cpg: &mut CodePropertyGraph,
    owner: NodeId,
    name: &str,
    argument: Option<CpgNodeKind>,
) -> (NodeId, Option<NodeId>) {
    let site = child(
        cpg,
        owner,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    child(
        cpg,
        site,
        CpgNodeKind::Identifier {
            name: name.into(),
            definition: None,
        },
    );
    let argument = argument.map(|kind| child(cpg, site, kind));
    (site, argument)
}

fn cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId) {
    cpg.connect(
        source,
        target,
        CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
    )
    .expect("fixture CFG endpoints exist");
}

fn support_config(supporting: usize, population: usize) -> ApiUsageMiningConfig {
    ApiUsageMiningConfig {
        minimum_support: PopulationThreshold::new(supporting, population),
        minimum_population: population,
        ..ApiUsageMiningConfig::default()
    }
}

fn order_population(
    has_successor: &[bool],
    reverse_cfg_insertion: bool,
) -> (CodePropertyGraph, Vec<NodeId>) {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let mut anchors = Vec::new();
    let mut cfg_edges = Vec::new();
    for (index, &complete) in has_successor.iter().enumerate() {
        let owner = cpg.add_node(node(function(&format!("peer_{index}"))));
        let (anchor, _) = call(&mut cpg, owner, "lock", None);
        let exit = child(&mut cpg, owner, CpgNodeKind::Return);
        cfg_edges.push((owner, anchor));
        if complete {
            let (successor, _) = call(&mut cpg, owner, "unlock", None);
            cfg_edges.push((anchor, successor));
            cfg_edges.push((successor, exit));
        } else {
            cfg_edges.push((anchor, exit));
        }
        anchors.push(anchor);
    }
    if reverse_cfg_insertion {
        cfg_edges.reverse();
    }
    for (source, target) in cfg_edges {
        cfg(&mut cpg, source, target);
    }
    (cpg, anchors)
}

fn lock_property() -> NamedTemporalProperty {
    NamedTemporalProperty::new(
        "release-after-acquire",
        PropertyFsm::new("error")
            .with_allocation("lock", "acquired")
            .with_event("unlock")
            .with_transition("acquired", "unlock", "released")
            .with_accepting("released"),
    )
}

#[test]
fn nineteen_of_twenty_lock_sites_flag_only_the_missing_unlock() {
    let mut complete = vec![true; 20];
    complete[19] = false;
    let (cpg, anchors) = order_population(&complete, false);
    let before = (cpg.node_count(), cpg.edge_count());

    let report = analyze_api_usage_mining(&cpg, &[], &support_config(19, 20))
        .expect("valid API mining configuration");

    assert!(report.is_complete(), "{report:#?}");
    let pattern = report
        .patterns
        .iter()
        .find(|pattern| {
            pattern.api.name() == "lock"
                && matches!(
                    &pattern.kind,
                    ApiUsagePatternKind::FollowingCall { successor }
                        if successor.name() == "unlock"
                )
        })
        .expect("lock then unlock frequent fragment");
    assert_eq!(pattern.support, PopulationFraction::new(19, 20));
    assert_eq!(pattern.supporting_sites.len(), 19);
    let findings: Vec<_> = report
        .findings
        .iter()
        .filter(|finding| {
            matches!(
                &finding.kind,
                ApiUsageMisuseKind::MissingFollowingCall { expected }
                    if expected.name() == "unlock"
            )
        })
        .collect();
    assert_eq!(findings.len(), 1, "{report:#?}");
    assert_eq!(findings[0].site, anchors[19]);
    assert_eq!(findings[0].witness, vec![anchors[19]]);
    assert_eq!(
        api_usage_misuses(&cpg, &support_config(19, 20)).expect("finding projection is valid"),
        report.findings
    );
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}

#[test]
fn exact_argument_shape_population_reports_the_one_boolean_mismatch() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let mut sites = Vec::new();
    for index in 0..20 {
        let owner = cpg.add_node(node(function(&format!("configure_{index}"))));
        let (site, _) = call(
            &mut cpg,
            owner,
            "configure",
            Some(CpgNodeKind::Literal {
                kind: LiteralKind::Bool(index < 19),
            }),
        );
        let exit = child(&mut cpg, owner, CpgNodeKind::Return);
        cfg(&mut cpg, owner, site);
        cfg(&mut cpg, site, exit);
        sites.push(site);
    }

    let report = analyze_api_usage_mining(&cpg, &[], &support_config(19, 20))
        .expect("valid API mining configuration");

    assert!(report.is_complete(), "{report:#?}");
    assert!(report.patterns.iter().any(|pattern| {
        pattern.api.name() == "configure"
            && pattern.kind
                == ApiUsagePatternKind::Argument {
                    position: 0,
                    shape: ApiDataShape::Literal(ApiLiteralShape::Bool(true)),
                }
            && pattern.support == PopulationFraction::new(19, 20)
    }));
    let mismatches: Vec<_> = report
        .findings
        .iter()
        .filter(|finding| {
            matches!(
                finding.kind,
                ApiUsageMisuseKind::Argument {
                    position: 0,
                    expected: ApiDataShape::Literal(ApiLiteralShape::Bool(true)),
                    observed: Some(ApiDataShape::Literal(ApiLiteralShape::Bool(false))),
                }
            )
        })
        .collect();
    assert_eq!(mismatches.len(), 1, "{report:#?}");
    assert_eq!(mismatches[0].site, sites[19]);
    assert_eq!(mismatches[0].witness.len(), 2);
}

#[test]
fn missing_release_is_a_non_accepting_temporal_exit() {
    let (cpg, anchors) = order_population(&[false, false], false);

    let report =
        analyze_api_usage_mining(&cpg, &[lock_property()], &ApiUsageMiningConfig::default())
            .expect("valid temporal property");

    assert!(report.is_complete(), "{report:#?}");
    assert_eq!(report.temporal_analyses.len(), 1);
    let leaks: Vec<_> = report
        .temporal_findings
        .iter()
        .filter(|finding| {
            matches!(
                &finding.kind,
                TemporalPropertyFindingKind::NonAcceptingExit { state }
                    if state.as_ref() == "acquired"
            )
        })
        .collect();
    assert_eq!(leaks.len(), 2, "{report:#?}");
    assert_eq!(
        leaks
            .iter()
            .map(|finding| finding.object)
            .collect::<Vec<_>>(),
        anchors
    );
}

#[test]
fn forbidden_temporal_transition_retains_the_typestate_witness() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let owner = cpg.add_node(node(function("double_unlock")));
    let (lock, _) = call(&mut cpg, owner, "lock", None);
    let (first_unlock, first_argument) = call(
        &mut cpg,
        owner,
        "unlock",
        Some(CpgNodeKind::Identifier {
            name: "guard".into(),
            definition: Some(lock),
        }),
    );
    let (second_unlock, second_argument) = call(
        &mut cpg,
        owner,
        "unlock",
        Some(CpgNodeKind::Identifier {
            name: "guard".into(),
            definition: Some(lock),
        }),
    );
    let exit = child(&mut cpg, owner, CpgNodeKind::Return);
    for argument in [first_argument, second_argument].into_iter().flatten() {
        cpg.connect(lock, argument, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
            .expect("fixture DFG endpoints exist");
    }
    for (source, target) in [
        (owner, lock),
        (lock, first_unlock),
        (first_unlock, second_unlock),
        (second_unlock, exit),
    ] {
        cfg(&mut cpg, source, target);
    }

    let report =
        analyze_api_usage_mining(&cpg, &[lock_property()], &ApiUsageMiningConfig::default())
            .expect("valid temporal property");

    let violation = report
        .temporal_findings
        .iter()
        .find(|finding| {
            matches!(
                &finding.kind,
                TemporalPropertyFindingKind::ForbiddenTransition { from, event }
                    if from.as_ref() == "released" && event.as_ref() == "unlock"
            )
        })
        .expect("second unlock is forbidden");
    assert_eq!(violation.site, second_unlock);
    assert_eq!(violation.object, lock);
    assert!(violation.witness.contains(&lock));
    assert!(violation.witness.contains(&first_unlock));
    assert!(violation.witness.contains(&second_unlock));
}

#[test]
fn centered_aug_contains_typed_parameter_selection_and_order_edges() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let owner = cpg.add_node(node(function("usage")));
    let (anchor, argument) = call(
        &mut cpg,
        owner,
        "configure",
        Some(CpgNodeKind::Literal {
            kind: LiteralKind::Integer(7),
        }),
    );
    let guard = child(
        &mut cpg,
        owner,
        CpgNodeKind::BinaryOp {
            operator: ">".into(),
        },
    );
    let (successor, _) = call(&mut cpg, owner, "commit", None);
    let exit = child(&mut cpg, owner, CpgNodeKind::Return);
    for (source, target) in [(owner, anchor), (anchor, successor), (successor, exit)] {
        cfg(&mut cpg, source, target);
    }
    cpg.connect(guard, anchor, CpgEdgeKind::ControlDependence)
        .expect("fixture PDG endpoints exist");

    let report = analyze_api_usage_mining(&cpg, &[], &ApiUsageMiningConfig::default())
        .expect("valid mining configuration");
    let graph = report
        .usage_graphs
        .iter()
        .find(|graph| graph.anchor == anchor)
        .expect("anchor AUG");

    assert!(graph.is_acyclic());
    assert!(graph.nodes.iter().any(|node| {
        node.source == argument.expect("argument")
            && node.kind
                == ApiUsageNodeKind::Data(ApiDataShape::Literal(ApiLiteralShape::Integer(7)))
    }));
    assert!(graph.nodes.iter().any(|node| {
        node.source == guard && node.kind == ApiUsageNodeKind::Control(ControlContext::Predicate)
    }));
    assert!(graph.nodes.iter().any(|node| {
        node.source == successor
            && matches!(
                &node.kind,
                ApiUsageNodeKind::FollowingCall(api) if api.name() == "commit"
            )
    }));
    assert!(graph
        .edges
        .iter()
        .any(|edge| edge.kind == ApiUsageEdgeKind::Parameter(0)));
    assert!(graph
        .edges
        .iter()
        .any(|edge| edge.kind == ApiUsageEdgeKind::Selection));
    assert!(graph
        .edges
        .iter()
        .any(|edge| edge.kind == ApiUsageEdgeKind::Order));
}

#[test]
fn method_receiver_shape_is_mined_from_typed_member_syntax() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    for index in 0..5 {
        let owner = cpg.add_node(node(function(&format!("method_{index}"))));
        let site = child(
            &mut cpg,
            owner,
            CpgNodeKind::Call {
                target: None,
                is_method: true,
            },
        );
        let callee = child(
            &mut cpg,
            site,
            CpgNodeKind::MemberAccess {
                member: "flush".into(),
            },
        );
        child(
            &mut cpg,
            callee,
            CpgNodeKind::Identifier {
                name: "stream".into(),
                definition: None,
            },
        );
        let exit = child(&mut cpg, owner, CpgNodeKind::Return);
        cfg(&mut cpg, owner, site);
        cfg(&mut cpg, site, exit);
    }
    let config = ApiUsageMiningConfig {
        minimum_support: PopulationThreshold::new(4, 5),
        minimum_population: 5,
        ..ApiUsageMiningConfig::default()
    };

    let report = analyze_api_usage_mining(&cpg, &[], &config).expect("valid mining configuration");

    assert!(report.patterns.iter().any(|pattern| {
        pattern.api.name().ends_with("flush")
            && pattern.kind
                == ApiUsagePatternKind::Receiver {
                    shape: ApiDataShape::Identifier,
                }
    }));
    assert!(report.usage_graphs.iter().all(ApiUsageGraph::is_acyclic));
}

#[test]
fn incomplete_sites_and_atomic_caps_are_explicit() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let owner = cpg.add_node(node(function("incomplete")));
    let unresolved = child(
        &mut cpg,
        owner,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    let (bounded, _) = call(
        &mut cpg,
        owner,
        "bounded",
        Some(CpgNodeKind::Literal {
            kind: LiteralKind::Integer(1),
        }),
    );
    let middle = child(
        &mut cpg,
        owner,
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        },
    );
    let exit = child(&mut cpg, owner, CpgNodeKind::Return);
    for (source, target) in [(owner, bounded), (bounded, middle), (middle, exit)] {
        cfg(&mut cpg, source, target);
    }
    let cfg_capped = ApiUsageMiningConfig {
        max_cfg_nodes_per_site: 1,
        ..ApiUsageMiningConfig::default()
    };
    let report = analyze_api_usage_mining(&cpg, &[], &cfg_capped).expect("valid cap");
    assert!(report
        .issues
        .contains(&ApiUsageMiningIssue::MissingCalleeIdentity { site: unresolved }));
    assert!(report.issues.iter().any(|issue| matches!(
        issue,
        ApiUsageMiningIssue::CfgEvidenceCap { site, .. } if *site == bounded
    )));

    let graph_capped = ApiUsageMiningConfig {
        max_graph_nodes: 1,
        ..ApiUsageMiningConfig::default()
    };
    let report = analyze_api_usage_mining(&cpg, &[], &graph_capped).expect("valid cap");
    assert!(report.issues.iter().any(|issue| matches!(
        issue,
        ApiUsageMiningIssue::GraphNodeCap { site, .. } if *site == bounded
    )));
    assert!(!report.analyzed_sites.contains(&bounded));
}

#[test]
fn invalid_thresholds_budgets_and_property_names_are_rejected() {
    let cpg = CodePropertyGraph::new(Language::Unknown);
    let invalid = vec![
        ApiUsageMiningConfig {
            minimum_support: PopulationThreshold::new(1, 2),
            ..ApiUsageMiningConfig::default()
        },
        ApiUsageMiningConfig {
            minimum_population: 1,
            ..ApiUsageMiningConfig::default()
        },
        ApiUsageMiningConfig {
            max_call_sites: 0,
            ..ApiUsageMiningConfig::default()
        },
        ApiUsageMiningConfig {
            max_cfg_nodes_per_site: MAX_API_USAGE_CFG_NODES_PER_SITE + 1,
            ..ApiUsageMiningConfig::default()
        },
        ApiUsageMiningConfig {
            max_graph_nodes: 0,
            ..ApiUsageMiningConfig::default()
        },
        ApiUsageMiningConfig {
            max_graph_edges: MAX_API_USAGE_GRAPH_EDGES + 1,
            ..ApiUsageMiningConfig::default()
        },
        ApiUsageMiningConfig {
            max_callee_depth: 0,
            ..ApiUsageMiningConfig::default()
        },
    ];
    for config in invalid {
        assert!(analyze_api_usage_mining(&cpg, &[], &config).is_err());
    }

    let empty = NamedTemporalProperty::new("", lock_property().fsm.clone());
    assert!(analyze_api_usage_mining(&cpg, &[empty], &ApiUsageMiningConfig::default()).is_err());
    let duplicate = lock_property();
    assert!(analyze_api_usage_mining(
        &cpg,
        &[duplicate.clone(), duplicate],
        &ApiUsageMiningConfig::default()
    )
    .is_err());
}

#[test]
fn call_site_inventory_cap_fails_closed_before_partial_mining() {
    let (cpg, _) = order_population(&[false, false], false);
    let config = ApiUsageMiningConfig {
        max_call_sites: 1,
        ..ApiUsageMiningConfig::default()
    };

    let report = analyze_api_usage_mining(&cpg, &[], &config).expect("valid cap");

    assert!(report.usage_graphs.is_empty());
    assert!(report.patterns.is_empty());
    assert!(report.findings.is_empty());
    assert_eq!(
        report.issues,
        BTreeSet::from([ApiUsageMiningIssue::CallSiteCap {
            observed: 2,
            cap: 1,
        }])
    );
}

#[test]
fn cfg_edge_insertion_order_does_not_change_the_report() {
    let population = [true, true, true, true, false];
    let (forward, _) = order_population(&population, false);
    let (reverse, _) = order_population(&population, true);
    let config = ApiUsageMiningConfig {
        minimum_support: PopulationThreshold::new(4, 5),
        minimum_population: 5,
        ..ApiUsageMiningConfig::default()
    };

    let left = analyze_api_usage_mining(&forward, &[], &config).expect("forward report");
    let right = analyze_api_usage_mining(&reverse, &[], &config).expect("reverse report");

    assert_eq!(left, right);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn mined_following_fragment_matches_exact_population_oracle(
        population in prop::collection::vec(any::<bool>(), 3..24)
            .prop_filter("true is a strict majority", |values| {
                values.iter().filter(|&&value| value).count() * 2 > values.len()
                    && values.iter().any(|&value| !value)
            })
    ) {
        let supporting = population.iter().filter(|&&value| value).count();
        let (cpg, anchors) = order_population(&population, false);
        let config = support_config(supporting, population.len());

        let report = analyze_api_usage_mining(&cpg, &[], &config).unwrap();
        let actual: BTreeSet<_> = report.findings.iter().filter_map(|finding| {
            matches!(
                &finding.kind,
                ApiUsageMisuseKind::MissingFollowingCall { expected }
                    if expected.name() == "unlock"
            ).then_some(finding.site)
        }).collect();
        let expected: BTreeSet<_> = population.iter().enumerate().filter_map(|(index, &value)| {
            (!value).then_some(anchors[index])
        }).collect();
        prop_assert_eq!(actual, expected);
    }

    #[test]
    fn repeated_runs_and_cfg_edge_permutations_are_deterministic(
        population in prop::collection::vec(any::<bool>(), 3..20)
            .prop_filter("true is a strict majority", |values| {
                values.iter().filter(|&&value| value).count() * 2 > values.len()
            })
    ) {
        let supporting = population.iter().filter(|&&value| value).count();
        let config = support_config(supporting, population.len());
        let (forward, _) = order_population(&population, false);
        let (reverse, _) = order_population(&population, true);
        let first = analyze_api_usage_mining(&forward, &[], &config).unwrap();
        let repeated = analyze_api_usage_mining(&forward, &[], &config).unwrap();
        let permuted = analyze_api_usage_mining(&reverse, &[], &config).unwrap();
        prop_assert_eq!(&first, &repeated);
        prop_assert_eq!(&first, &permuted);
        prop_assert!(first.usage_graphs.iter().all(ApiUsageGraph::is_acyclic));
    }
}

#[test]
fn one_hundred_thousand_cfg_nodes_use_a_small_native_stack() {
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(|| {
            let mut cpg = CodePropertyGraph::new(Language::Unknown);
            let owner = cpg.add_node(node(function("deep")));
            let (anchor, _) = call(&mut cpg, owner, "anchor", None);
            cfg(&mut cpg, owner, anchor);
            let mut previous = anchor;
            for _ in 0..100_000 {
                let next = child(
                    &mut cpg,
                    owner,
                    CpgNodeKind::Block {
                        scope: ScopeId::GLOBAL,
                    },
                );
                cfg(&mut cpg, previous, next);
                previous = next;
            }
            let exit = child(&mut cpg, owner, CpgNodeKind::Return);
            cfg(&mut cpg, previous, exit);
            let config = ApiUsageMiningConfig {
                max_cfg_nodes_per_site: 100_010,
                ..ApiUsageMiningConfig::default()
            };

            let report = analyze_api_usage_mining(&cpg, &[], &config)
                .expect("deep iterative API-usage traversal");
            assert!(report.is_complete(), "{report:#?}");
            assert_eq!(report.usage_graphs.len(), 1);
            assert!(report.usage_graphs[0].is_acyclic());
        })
        .expect("spawn small-stack API-mining gate")
        .join()
        .expect("small-stack API-mining gate completes");
}

#[cfg(feature = "serde")]
#[test]
fn configuration_and_full_report_round_trip_through_serde() {
    let mut complete = vec![true; 10];
    complete[9] = false;
    let (cpg, _) = order_population(&complete, false);
    let config = support_config(9, 10);
    let report = analyze_api_usage_mining(&cpg, &[lock_property()], &config)
        .expect("serializable API mining report");

    let config_bytes = serde_json::to_vec(&config).expect("serialize configuration");
    let report_bytes = serde_json::to_vec(&report).expect("serialize report");
    assert_eq!(
        serde_json::from_slice::<ApiUsageMiningConfig>(&config_bytes)
            .expect("deserialize configuration"),
        config
    );
    assert_eq!(
        serde_json::from_slice::<ApiUsageMiningAnalysis>(&report_bytes)
            .expect("deserialize report"),
        report
    );
}
