//! Public-API and parser-pipeline gates for T6.5 API-usage mining.

use libcpg::{
    analyze_api_usage_mining, api_usage_misuses, ApiUsageMiningConfig, ApiUsageMisuseKind,
    ApiUsagePatternKind, CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind,
    DfgEdgeKind, Language, MethodSignature, NamedTemporalProperty, NodeId, PopulationFraction,
    PopulationThreshold, PropertyFsm, SourceRange, TemporalPropertyFindingKind, Visibility,
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
        .expect("integration AST endpoints exist");
    cpg.node_mut(parent)
        .expect("integration parent exists")
        .children
        .push(id);
    id
}

fn call(cpg: &mut CodePropertyGraph, owner: NodeId, name: &str) -> NodeId {
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
    site
}

fn cfg(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId) {
    cpg.connect(
        source,
        target,
        CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
    )
    .expect("integration CFG endpoints exist");
}

fn release_property() -> NamedTemporalProperty {
    NamedTemporalProperty::new(
        "release-after-acquire",
        PropertyFsm::new("error")
            .with_allocation("lock", "acquired")
            .with_event("unlock")
            .with_transition("acquired", "unlock", "released")
            .with_accepting("released"),
    )
}

fn ninety_five_percent_fixture() -> (CodePropertyGraph, Vec<NodeId>) {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let mut locks = Vec::new();
    for index in 0..20 {
        let owner = cpg.add_node(node(function(&format!("peer_{index}"))));
        let lock = call(&mut cpg, owner, "lock");
        let exit = child(&mut cpg, owner, CpgNodeKind::Return);
        cfg(&mut cpg, owner, lock);
        if index < 19 {
            let unlock = call(&mut cpg, owner, "unlock");
            let argument = child(
                &mut cpg,
                unlock,
                CpgNodeKind::Identifier {
                    name: "guard".into(),
                    definition: Some(lock),
                },
            );
            cpg.connect(lock, argument, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
                .expect("integration DFG endpoints exist");
            cfg(&mut cpg, lock, unlock);
            cfg(&mut cpg, unlock, exit);
        } else {
            cfg(&mut cpg, lock, exit);
        }
        locks.push(lock);
    }
    (cpg, locks)
}

fn config() -> ApiUsageMiningConfig {
    ApiUsageMiningConfig {
        minimum_support: PopulationThreshold::new(19, 20),
        minimum_population: 20,
        ..ApiUsageMiningConfig::default()
    }
}

#[test]
fn public_pipeline_reports_exact_usage_and_temporal_evidence_without_mutation() {
    let (cpg, locks) = ninety_five_percent_fixture();
    let before = (cpg.node_count(), cpg.edge_count());
    let report = analyze_api_usage_mining(&cpg, &[release_property()], &config())
        .expect("valid public API-usage analysis");

    assert!(report.is_complete(), "{report:#?}");
    assert_eq!(report.usage_graphs.len(), 39);
    assert!(report.usage_graphs.iter().all(|graph| graph.is_acyclic()));
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
        .expect("95% lock-then-unlock fragment");
    assert_eq!(pattern.support, PopulationFraction::new(19, 20));
    assert_eq!(pattern.supporting_sites.len(), 19);

    let usage_findings: Vec<_> = report
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
    assert_eq!(usage_findings.len(), 1, "{report:#?}");
    assert_eq!(usage_findings[0].site, locks[19]);
    assert_eq!(
        usage_findings[0].peer_support,
        PopulationFraction::new(19, 20)
    );
    assert_eq!(usage_findings[0].witness, vec![locks[19]]);

    let temporal_findings: Vec<_> = report
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
    assert_eq!(temporal_findings.len(), 1, "{report:#?}");
    assert_eq!(
        temporal_findings[0].property.as_ref(),
        "release-after-acquire"
    );
    assert_eq!(temporal_findings[0].object, locks[19]);
    assert!(temporal_findings[0].witness.contains(&locks[19]));
    assert_eq!(
        api_usage_misuses(&cpg, &config()).expect("public misuse projection"),
        report.findings
    );
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}

#[cfg(feature = "lang-rust")]
#[test]
fn parser_built_rust_corpus_reports_the_missing_release_pattern() {
    use libcpg::{CpgBuilder, TreeSitterCpgBuilder};

    let mut source =
        String::from("struct Guard;\nfn lock() -> Guard { Guard }\nfn unlock(_guard: Guard) {}\n");
    for index in 0..20 {
        if index < 19 {
            source.push_str(&format!(
                "fn peer_{index}() {{ let guard = lock(); unlock(guard); }}\n"
            ));
        } else {
            source.push_str(&format!("fn peer_{index}() {{ let _guard = lock(); }}\n"));
        }
    }
    let cpg = TreeSitterCpgBuilder::new()
        .build(&source, Language::Rust)
        .expect("parser-built Rust corpus");
    let before = (cpg.node_count(), cpg.edge_count());
    let report = analyze_api_usage_mining(&cpg, &[], &config())
        .expect("valid parser-backed API-usage analysis");

    assert!(report.is_complete(), "{report:#?}");
    assert!(report.patterns.iter().any(|pattern| {
        pattern.api.name() == "lock"
            && pattern.support == PopulationFraction::new(19, 20)
            && matches!(
                &pattern.kind,
                ApiUsagePatternKind::FollowingCall { successor }
                    if successor.name() == "unlock"
            )
    }));
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
    assert_eq!(
        cpg.node(findings[0].function).and_then(|node| node.name()),
        Some("peer_19")
    );
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}
