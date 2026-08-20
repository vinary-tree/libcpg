//! Feature-free API-usage and temporal-property mining example.

use libcpg::{
    analyze_api_usage_mining, ApiUsageMiningConfig, ApiUsageMisuseKind, ApiUsagePatternKind,
    CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, DfgEdgeKind, Language,
    MethodSignature, NamedTemporalProperty, NodeId, PopulationFraction, PopulationThreshold,
    PropertyFsm, SourceRange, TemporalPropertyFindingKind, Visibility,
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
        .expect("example AST endpoints exist");
    cpg.node_mut(parent)
        .expect("example parent exists")
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
    .expect("example CFG endpoints exist");
}

fn fixture() -> (CodePropertyGraph, NodeId) {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let mut missing = None;
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
                .expect("example DFG endpoints exist");
            cfg(&mut cpg, lock, unlock);
            cfg(&mut cpg, unlock, exit);
        } else {
            cfg(&mut cpg, lock, exit);
            missing = Some(lock);
        }
    }
    (cpg, missing.expect("one deliberately incomplete peer"))
}

fn main() {
    let property = NamedTemporalProperty::new(
        "release-after-acquire",
        PropertyFsm::new("error")
            .with_allocation("lock", "acquired")
            .with_event("unlock")
            .with_transition("acquired", "unlock", "released")
            .with_accepting("released"),
    );
    let config = ApiUsageMiningConfig {
        minimum_support: PopulationThreshold::new(19, 20),
        minimum_population: 20,
        ..ApiUsageMiningConfig::default()
    };
    let (cpg, missing) = fixture();
    let report = analyze_api_usage_mining(&cpg, &[property], &config)
        .expect("the example graph and configuration are valid");

    assert!(report.is_complete());
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
        .expect("95% release protocol");
    assert_eq!(pattern.support, PopulationFraction::new(19, 20));
    let misuse = report
        .findings
        .iter()
        .find(|finding| {
            matches!(
                &finding.kind,
                ApiUsageMisuseKind::MissingFollowingCall { expected }
                    if expected.name() == "unlock"
            )
        })
        .expect("one missing unlock advisory");
    assert_eq!(misuse.site, missing);
    let temporal = report
        .temporal_findings
        .iter()
        .find(|finding| {
            matches!(
                &finding.kind,
                TemporalPropertyFindingKind::NonAcceptingExit { state }
                    if state.as_ref() == "acquired"
            )
        })
        .expect("one release-after-acquire violation");
    assert_eq!(temporal.object, missing);

    println!(
        "mined {:?} at support {}/{}; misuse {:?} at {:?}; temporal {:?} at {:?}",
        pattern.kind,
        pattern.support.supporting,
        pattern.support.population,
        misuse.kind,
        misuse.site,
        temporal.kind,
        temporal.site,
    );
}
