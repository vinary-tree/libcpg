//! Public-API and parser-pipeline gates for PDG information-flow analysis.

use std::collections::BTreeSet;

use libcpg::{
    constant_time, noninterference, CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNode,
    CpgNodeKind, FlowViolationKind, Language, LeakSiteKind, NodeId, SecurityLabels, SourceRange,
};

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn add(cpg: &mut CodePropertyGraph, kind: CpgNodeKind) -> NodeId {
    cpg.add_node(node(kind))
}

fn opaque(name: &str) -> CpgNodeKind {
    CpgNodeKind::Unknown { kind: name.into() }
}

#[test]
fn feature_free_public_apis_report_complete_replayable_advisory_evidence() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let secret = add(&mut cpg, opaque("secret"));
    let explicit_sink = add(&mut cpg, opaque("explicit sink"));
    let guard = add(&mut cpg, opaque("guard"));
    let implicit_sink = add(&mut cpg, opaque("implicit sink"));
    let declassifier = add(&mut cpg, opaque("declassifier"));
    let released_sink = add(&mut cpg, opaque("released sink"));
    let index = add(&mut cpg, CpgNodeKind::IndexAccess);
    let true_target = add(&mut cpg, opaque("true target"));
    let false_target = add(&mut cpg, opaque("false target"));

    cpg.connect(secret, explicit_sink, CpgEdgeKind::DataDependence)
        .expect("explicit dependence endpoints exist");
    cpg.connect(secret, guard, CpgEdgeKind::DataDependence)
        .expect("guard dependence endpoints exist");
    cpg.connect(guard, implicit_sink, CpgEdgeKind::ControlDependence)
        .expect("implicit dependence endpoints exist");
    cpg.connect(secret, declassifier, CpgEdgeKind::DataDependence)
        .expect("declassification input endpoints exist");
    cpg.connect(declassifier, released_sink, CpgEdgeKind::DataDependence)
        .expect("declassification output endpoints exist");
    cpg.connect(secret, index, CpgEdgeKind::DataDependence)
        .expect("index dependence endpoints exist");
    cpg.connect(
        guard,
        true_target,
        CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalTrue),
    )
    .expect("true branch endpoints exist");
    cpg.connect(
        guard,
        false_target,
        CpgEdgeKind::ControlFlow(CfgEdgeKind::ConditionalFalse),
    )
    .expect("false branch endpoints exist");
    let shape = (cpg.node_count(), cpg.edge_count());
    let labels = SecurityLabels::new(
        [secret, NodeId::new(u32::MAX)],
        [explicit_sink, implicit_sink, released_sink],
    )
    .with_declassifier(declassifier);

    let violations = noninterference(&cpg, &labels);
    assert_eq!(
        violations
            .iter()
            .map(|finding| (finding.sink, finding.kind))
            .collect::<Vec<_>>(),
        vec![
            (explicit_sink, FlowViolationKind::Explicit),
            (implicit_sink, FlowViolationKind::Implicit),
        ]
    );
    assert!(violations.iter().all(|finding| {
        finding.sink != released_sink
            && finding
                .explicit_witness
                .iter()
                .chain(finding.implicit_witness.iter())
                .flat_map(|witness| &witness.steps)
                .all(|step| step.target != declassifier)
    }));

    let findings = constant_time(&cpg, &BTreeSet::from([secret, NodeId::new(u32::MAX)]));
    assert_eq!(
        findings
            .iter()
            .map(|finding| (finding.site, finding.kind))
            .collect::<Vec<_>>(),
        vec![
            (
                guard,
                LeakSiteKind::ConditionalTrue {
                    target: true_target,
                },
            ),
            (
                guard,
                LeakSiteKind::ConditionalFalse {
                    target: false_target,
                },
            ),
            (index, LeakSiteKind::IndexAccess),
        ]
    );
    assert_eq!(noninterference(&cpg, &labels), violations);
    assert_eq!(constant_time(&cpg, &BTreeSet::from([secret])), findings);
    assert_eq!((cpg.node_count(), cpg.edge_count()), shape);
}

#[cfg(feature = "lang-rust")]
#[test]
fn parser_built_rust_cfg_dfg_and_pdg_feed_the_public_ifc_apis() {
    use libcpg::{CpgBuilder, PdgBuilder, TreeSitterCpgBuilder};

    let source = r#"
        fn expose(secret: usize, data: &[usize]) -> usize {
            let copied = secret;
            let indexed = data[secret];
            if secret > 0 { copied } else { indexed }
        }
    "#;
    let mut cpg = TreeSitterCpgBuilder::new()
        .build(source, Language::Rust)
        .expect("parser-built Rust IFC fixture");
    PdgBuilder::new().build_all(&mut cpg);

    let secret_parameter = cpg
        .nodes()
        .find(
            |node| matches!(&node.kind, CpgNodeKind::Parameter { name, .. } if &**name == "secret"),
        )
        .expect("parsed secret parameter")
        .id;
    let explicit_sink = cpg
        .edges()
        .find_map(|edge| {
            (edge.source == secret_parameter && edge.kind == CpgEdgeKind::DataDependence)
                .then_some(edge.target)
        })
        .expect("parsed secret parameter has a PDG use");
    let (branch_site, branch_target) = cpg
        .edges()
        .find_map(|edge| {
            matches!(
                edge.kind,
                CpgEdgeKind::ControlFlow(
                    CfgEdgeKind::ConditionalTrue | CfgEdgeKind::ConditionalFalse
                )
            )
            .then_some((edge.source, edge.target))
        })
        .expect("parsed conditional CFG edge");
    let implicit_sink = cpg
        .edges()
        .find_map(|edge| {
            (edge.source == branch_site
                && edge.target != branch_site
                && edge.kind == CpgEdgeKind::ControlDependence)
                .then_some(edge.target)
        })
        .expect("parsed branch has a distinct control-dependent node");
    let index = cpg
        .nodes()
        .find(|node| matches!(node.kind, CpgNodeKind::IndexAccess))
        .expect("parsed index access")
        .id;
    let shape = (cpg.node_count(), cpg.edge_count());

    let explicit = noninterference(
        &cpg,
        &SecurityLabels::new([secret_parameter], [explicit_sink]),
    );
    assert_eq!(explicit.len(), 1, "{explicit:#?}");
    assert_eq!(explicit[0].kind, FlowViolationKind::Explicit);

    let implicit = noninterference(&cpg, &SecurityLabels::new([branch_site], [implicit_sink]));
    assert_eq!(implicit.len(), 1, "{implicit:#?}");
    assert_eq!(implicit[0].kind, FlowViolationKind::Implicit);
    assert!(noninterference(
        &cpg,
        &SecurityLabels::new([secret_parameter], [explicit_sink]).with_declassifier(explicit_sink),
    )
    .is_empty());

    let timing = constant_time(&cpg, &BTreeSet::from([branch_site, index]));
    assert!(timing.iter().any(|finding| {
        finding.secret == branch_site
            && finding.site == branch_site
            && matches!(
                finding.kind,
                LeakSiteKind::ConditionalTrue { target }
                    | LeakSiteKind::ConditionalFalse { target }
                    if target == branch_target
            )
    }));
    assert!(timing.iter().any(|finding| {
        finding.secret == index
            && finding.site == index
            && finding.kind == LeakSiteKind::IndexAccess
            && finding.witness.steps.is_empty()
    }));
    assert_eq!((cpg.node_count(), cpg.edge_count()), shape);
}
