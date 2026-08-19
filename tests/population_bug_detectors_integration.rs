//! Public-API, parser-pipeline, and invariance gates for T6.3.

use libcpg::{
    analyze_deviant_behavior, analyze_missing_checks, deviant_behaviors, missing_checks,
    CatalogRule, CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind,
    DeviantBehaviorConfig, DeviantBehaviorKind, DfgEdgeKind, Language, MethodSignature,
    MissingCheckConfig, NodeId, Pattern, PeerScope, PopulationFraction, SourceRange, TaintCatalog,
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
        .expect("integration AST endpoints exist");
    cpg.node_mut(parent)
        .expect("integration parent exists")
        .children
        .push(id);
    id
}

fn call(cpg: &mut CodePropertyGraph, function: NodeId, name: &str) -> NodeId {
    let site = child(
        cpg,
        function,
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

fn missing_config() -> MissingCheckConfig {
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

#[test]
fn public_population_detectors_find_both_oracles_without_mutating_the_cpg() {
    let mut cpg = CodePropertyGraph::new(Language::Unknown);
    let mut danger_calls = Vec::new();
    for index in 0..5 {
        let function = cpg.add_node(node(function(&format!("danger_peer_{index}"))));
        let site = call(&mut cpg, function, "danger");
        let exit = child(&mut cpg, function, CpgNodeKind::Return);
        cfg(&mut cpg, function, site);
        cfg(&mut cpg, site, exit);
        if index < 4 {
            let guard = child(
                &mut cpg,
                function,
                CpgNodeKind::BinaryOp {
                    operator: ">".into(),
                },
            );
            child(
                &mut cpg,
                guard,
                CpgNodeKind::Identifier {
                    name: format!("value_{index}").into(),
                    definition: None,
                },
            );
            cpg.connect(guard, site, CpgEdgeKind::ControlDependence)
                .expect("integration PDG endpoints exist");
        }
        danger_calls.push(site);
    }

    let mut read_calls = Vec::new();
    for index in 0..10 {
        let function = cpg.add_node(node(function(&format!("read_peer_{index}"))));
        let site = call(&mut cpg, function, "read_value");
        let exit = child(&mut cpg, function, CpgNodeKind::Return);
        cfg(&mut cpg, function, site);
        cfg(&mut cpg, site, exit);
        if index < 9 {
            let guard = child(&mut cpg, function, CpgNodeKind::If);
            cpg.connect(site, guard, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
                .expect("integration DFG endpoints exist");
            cpg.connect(guard, exit, CpgEdgeKind::ControlDependence)
                .expect("integration PDG endpoints exist");
        }
        read_calls.push(site);
    }
    let before = (cpg.node_count(), cpg.edge_count());

    let missing = analyze_missing_checks(&cpg, &missing_config()).expect("missing-check report");
    let deviant = analyze_deviant_behavior(&cpg, &DeviantBehaviorConfig::default())
        .expect("deviant-behavior report");

    assert!(missing.is_complete(), "{missing:#?}");
    assert_eq!(missing.findings.len(), 1, "{missing:#?}");
    assert_eq!(missing.findings[0].site, danger_calls[4]);
    assert_eq!(
        missing.findings[0].peer_support[&missing.findings[0].expected_checks[0]],
        PopulationFraction::new(4, 5)
    );
    assert!(deviant.is_complete(), "{deviant:#?}");
    let unchecked: Vec<_> = deviant
        .findings
        .iter()
        .filter(|finding| matches!(finding.kind, DeviantBehaviorKind::UncheckedReturn))
        .collect();
    assert_eq!(unchecked.len(), 1, "{deviant:#?}");
    assert_eq!(unchecked[0].site, read_calls[9]);
    assert_eq!(unchecked[0].peer_support, PopulationFraction::new(9, 10));
    assert_eq!(
        missing_checks(&cpg, &missing_config()).expect("finding projection"),
        missing.findings
    );
    assert_eq!(
        deviant_behaviors(&cpg, &DeviantBehaviorConfig::default()).expect("finding projection"),
        deviant.findings
    );
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}

#[cfg(feature = "lang-rust")]
#[test]
fn parser_built_rust_pdg_exposes_the_four_of_five_missing_check() {
    use libcpg::{CpgBuilder, PdgBuilder, TreeSitterCpgBuilder};

    let source = r#"
        fn danger(_: i32) {}
        fn peer0(value: i32) { if value > 0 { danger(value); } }
        fn peer1(item: i32) { if item > 0 { danger(item); } }
        fn peer2(input: i32) { if input > 0 { danger(input); } }
        fn peer3(candidate: i32) { if candidate > 0 { danger(candidate); } }
        fn peer4(unchecked: i32) { danger(unchecked); }
    "#;
    let mut cpg = TreeSitterCpgBuilder::new()
        .build(source, Language::Rust)
        .expect("parser-built Rust CPG");
    let functions: Vec<_> = cpg.functions().map(|node| node.id).collect();
    for function in functions {
        PdgBuilder::new().build(&mut cpg, function);
    }
    let before = (cpg.node_count(), cpg.edge_count());

    let report = analyze_missing_checks(&cpg, &missing_config()).expect("missing-check report");

    assert!(report.is_complete(), "{report:#?}");
    assert_eq!(report.findings.len(), 1, "{report:#?}");
    assert_eq!(report.findings[0].api.name(), "danger");
    assert_eq!(
        report.findings[0].peer_support[&report.findings[0].expected_checks[0]],
        PopulationFraction::new(4, 5)
    );
    let owner = cpg
        .ast_ancestors(report.findings[0].site)
        .into_iter()
        .find_map(|node| cpg.node(node).and_then(|node| node.name()));
    assert_eq!(owner, Some("peer4"));
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}
