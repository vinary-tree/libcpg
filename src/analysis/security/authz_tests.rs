use proptest::prelude::*;

use super::*;
use crate::{CpgEdgeKind, CpgNode, Language, MethodSignature, SourceRange, Visibility};

fn add(cpg: &mut CodePropertyGraph, kind: crate::CpgNodeKind) -> NodeId {
    let offset = u32::try_from(cpg.node_count()).unwrap_or(u32::MAX / 2);
    cpg.add_node(CpgNode::new(
        NodeId::new(0),
        kind,
        SourceRange::from_bytes(offset.saturating_mul(3), offset.saturating_mul(3) + 1),
    ))
}

fn ast(cpg: &mut CodePropertyGraph, parent: NodeId, child: NodeId) {
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("authz fixture AST endpoints exist");
    cpg.node_mut(parent)
        .expect("authz fixture parent exists")
        .children
        .push(child);
    cpg.node_mut(child)
        .expect("authz fixture child exists")
        .parent = Some(parent);
}

fn function(cpg: &mut CodePropertyGraph, name: &str) -> NodeId {
    add(
        cpg,
        crate::CpgNodeKind::Function {
            signature: MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            },
        },
    )
}

fn call(cpg: &mut CodePropertyGraph, owner: NodeId, path: &str) -> NodeId {
    let site = add(
        cpg,
        crate::CpgNodeKind::Call {
            target: None,
            is_method: path.contains("::") || path.contains('.'),
        },
    );
    ast(cpg, owner, site);
    let callee = add(
        cpg,
        crate::CpgNodeKind::Identifier {
            name: path.into(),
            definition: None,
        },
    );
    ast(cpg, site, callee);
    site
}

fn guard(cpg: &mut CodePropertyGraph, owner: NodeId, site: NodeId, path: &str) -> NodeId {
    let guard = call(cpg, owner, path);
    cpg.connect(guard, site, CpgEdgeKind::ControlDependence)
        .expect("authz fixture PDG endpoints exist");
    guard
}

fn population(
    guarded: usize,
    unguarded: usize,
    guard_path: &str,
) -> (CodePropertyGraph, Vec<NodeId>, Vec<NodeId>) {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let mut mutations = Vec::new();
    let mut guards = Vec::new();
    for index in 0..guarded + unguarded {
        let owner = function(&mut cpg, &format!("route_{index}"));
        let site = call(&mut cpg, owner, "repository::update");
        if index < guarded {
            guards.push(guard(&mut cpg, owner, site, guard_path));
        }
        mutations.push(site);
    }
    (cpg, mutations, guards)
}

fn whole_program_config(guarded: usize, population: usize) -> AuthzAnalysisConfig {
    AuthzAnalysisConfig {
        minimum_support: PopulationThreshold::new(guarded, population),
        minimum_population: population,
        peer_scope: PeerScope::WholeProgram,
        ..AuthzAnalysisConfig::default()
    }
}

#[test]
fn missing_auth_guard_is_reported_while_guarded_routes_remain_clean() {
    let (cpg, mutations, guards) = population(4, 1, "authz::authorize");
    let shape = (cpg.node_count(), cpg.edge_count());

    let report = analyze_builtin_authz(&cpg, &whole_program_config(4, 5))
        .expect("valid access-control policy");

    assert!(report.is_complete(), "{report:#?}");
    assert_eq!(report.findings.len(), 1, "{report:#?}");
    let finding = &report.findings[0];
    assert_eq!(finding.site, mutations[4]);
    assert_eq!(finding.peer_sites, mutations);
    assert_eq!(finding.expected_guards.len(), 1);
    assert_eq!(
        finding.peer_support[&finding.expected_guards[0]],
        PopulationFraction::new(4, 5)
    );
    assert_eq!(finding.guard_witnesses[&finding.expected_guards[0]], guards);
    assert!(!report
        .findings
        .iter()
        .any(|finding| { finding.peer_sites[..4].contains(&finding.site) }));
    assert_eq!((cpg.node_count(), cpg.edge_count()), shape);
}

#[test]
fn non_access_control_population_check_is_not_misreported_as_authz() {
    let (cpg, _, _) = population(4, 1, "quota::within_limit");
    let report = analyze_builtin_authz(&cpg, &whole_program_config(4, 5))
        .expect("valid access-control policy");

    assert!(report.is_complete(), "{report:#?}");
    assert!(report.findings.is_empty(), "{report:#?}");
}

#[test]
fn invalid_policy_and_incomplete_population_evidence_are_explicit() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let outside = add(
        &mut cpg,
        crate::CpgNodeKind::Call {
            target: None,
            is_method: true,
        },
    );
    let callee = add(
        &mut cpg,
        crate::CpgNodeKind::Identifier {
            name: "repository::update".into(),
            definition: None,
        },
    );
    ast(&mut cpg, outside, callee);
    let rules = AuthzRuleSet {
        mutations: vec![
            AuthzMutationRule::new(""),
            AuthzMutationRule::new("invalid").with_call(Pattern::Substring("".into())),
        ],
        guard_patterns: Vec::new(),
    };

    let report = analyze_missing_authz(&cpg, &rules, &AuthzAnalysisConfig::default())
        .expect("numeric population configuration is valid");

    assert!(!report.is_complete());
    assert!(report.issues.contains(&AuthzAnalysisIssue::EmptyRuleName));
    assert!(report
        .issues
        .contains(&AuthzAnalysisIssue::MissingGuardPattern));
    assert!(report.issues.contains(&AuthzAnalysisIssue::EmptyPattern {
        rule: "invalid".into(),
    }));
    assert!(report.issues.contains(&AuthzAnalysisIssue::Population(
        MissingCheckIssue::CallOutsideFunction { site: outside }
    )));
}

#[test]
fn invalid_population_threshold_is_rejected_by_the_shared_detector() {
    let (cpg, _, _) = population(1, 1, "authz::authorize");
    let config = AuthzAnalysisConfig {
        minimum_support: PopulationThreshold::new(1, 0),
        ..AuthzAnalysisConfig::default()
    };
    assert!(matches!(
        analyze_builtin_authz(&cpg, &config),
        Err(AnalysisError::InvalidInput {
            analysis: "missing-check detection",
            input: "minimum_support",
            ..
        })
    ));
}

#[test]
fn flat_guard_qualification_is_stack_safe_at_one_hundred_thousand_nodes() {
    let handle = std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(|| {
            let mut nodes = Vec::with_capacity(100_001);
            nodes.extend((0..100_000).map(|_| crate::CheckFingerprintNode {
                kind: CheckNodeKind::Identifier,
                child_count: 1,
            }));
            nodes.push(crate::CheckFingerprintNode {
                kind: CheckNodeKind::Call(ApiIdentity::Named("authz::authorize".into())),
                child_count: 1,
            });
            is_access_guard(
                &CheckFingerprint { nodes },
                &builtin_authz_rules().guard_patterns,
            )
        })
        .expect("small-stack worker starts");
    assert!(handle.join().expect("small-stack worker completes"));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn population_analysis_is_deterministic(guarded in 1usize..8) {
        let population_size = guarded + 1;
        let (cpg, _, _) = population(guarded, 1, "authz::authorize");
        let config = whole_program_config(guarded, population_size);
        let first = analyze_builtin_authz(&cpg, &config)
            .expect("generated access-control population is valid");
        let second = analyze_builtin_authz(&cpg, &config)
            .expect("repeated access-control population is valid");
        prop_assert_eq!(first, second);
    }
}

#[cfg(feature = "serde")]
#[test]
fn policy_and_report_round_trip_through_json() {
    let rules = builtin_authz_rules();
    let encoded = serde_json::to_string(&rules).expect("policy serializes");
    let decoded: AuthzRuleSet = serde_json::from_str(&encoded).expect("policy deserializes");
    assert_eq!(decoded, rules);

    let (cpg, _, _) = population(4, 1, "authz::authorize");
    let report = analyze_builtin_authz(&cpg, &whole_program_config(4, 5))
        .expect("valid access-control population");
    let encoded = serde_json::to_string(&report).expect("report serializes");
    let decoded: AuthzAnalysis = serde_json::from_str(&encoded).expect("report deserializes");
    assert_eq!(decoded, report);
}
