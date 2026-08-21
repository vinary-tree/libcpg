//! Feature-free I6 missing-check and I7 deviant-behavior example.

use libcpg::{
    analyze_deviant_behavior, analyze_missing_checks, CatalogRule, CfgEdgeKind, CodePropertyGraph,
    CpgEdgeKind, CpgNode, CpgNodeKind, DeviantBehaviorConfig, DfgEdgeKind, Language,
    MethodSignature, MissingCheckConfig, NodeId, Pattern, PeerScope, SourceRange, TaintCatalog,
    Visibility,
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

fn sequential(cpg: &mut CodePropertyGraph, source: NodeId, target: NodeId) {
    cpg.connect(
        source,
        target,
        CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
    )
    .expect("example CFG endpoints exist");
}

fn missing_check_policy() -> MissingCheckConfig {
    let mut catalog = TaintCatalog::empty();
    catalog.sinks.push(CatalogRule {
        pattern: Pattern::Exact("authorize".into()),
        class: "access-control".into(),
        cwe: Some(862),
    });
    MissingCheckConfig {
        catalog,
        peer_scope: PeerScope::WholeProgram,
        ..MissingCheckConfig::default()
    }
}

fn main() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);

    // Four of five authorization calls carry the same semantic check. Local
    // identifier spellings differ, but check normalization abstracts them.
    for index in 0..5 {
        let owner = cpg.add_node(node(function(&format!("authorization_peer_{index}"))));
        let site = call(&mut cpg, owner, "authorize");
        let exit = child(&mut cpg, owner, CpgNodeKind::Return);
        sequential(&mut cpg, owner, site);
        sequential(&mut cpg, site, exit);
        if index < 4 {
            let guard = child(
                &mut cpg,
                owner,
                CpgNodeKind::BinaryOp {
                    operator: ">".into(),
                },
            );
            child(
                &mut cpg,
                guard,
                CpgNodeKind::Identifier {
                    name: format!("permission_{index}").into(),
                    definition: None,
                },
            );
            cpg.connect(guard, site, CpgEdgeKind::ControlDependence)
                .expect("example PDG endpoints exist");
        }
    }

    // Nine of ten read results flow to a represented guard.
    for index in 0..10 {
        let owner = cpg.add_node(node(function(&format!("read_peer_{index}"))));
        let site = call(&mut cpg, owner, "read_value");
        let exit = child(&mut cpg, owner, CpgNodeKind::Return);
        sequential(&mut cpg, owner, site);
        sequential(&mut cpg, site, exit);
        if index < 9 {
            let guard = child(&mut cpg, owner, CpgNodeKind::If);
            cpg.connect(site, guard, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
                .expect("example DFG endpoints exist");
            cpg.connect(guard, exit, CpgEdgeKind::ControlDependence)
                .expect("example guard evidence exists");
        }
    }

    let missing =
        analyze_missing_checks(&cpg, &missing_check_policy()).expect("the example policy is valid");
    let deviant = analyze_deviant_behavior(&cpg, &DeviantBehaviorConfig::default())
        .expect("the example policy is valid");

    for finding in &missing.findings {
        println!(
            "missing check at {:?}: {} absent fingerprint(s), peers {:?}",
            finding.site,
            finding.expected_checks.len(),
            finding.peer_sites,
        );
        for support in finding.peer_support.values() {
            println!(
                "  exact support: {}/{}",
                support.supporting, support.population
            );
        }
    }
    for finding in &deviant.findings {
        println!(
            "deviation at {:?}: {:?}, exact support {}/{}",
            finding.site,
            finding.kind,
            finding.peer_support.supporting,
            finding.peer_support.population,
        );
    }

    assert!(missing.is_complete());
    assert_eq!(missing.findings.len(), 1);
    assert!(deviant.is_complete());
    assert_eq!(
        deviant
            .findings
            .iter()
            .filter(|finding| matches!(finding.kind, libcpg::DeviantBehaviorKind::UncheckedReturn))
            .count(),
        1
    );
}
