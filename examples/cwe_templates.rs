//! Feature-free declarative CWE-template analysis example.

use libcpg::{
    analyze_builtin_cwes, CallGraphResolution, CodePropertyGraph, CpgEdgeKind, CpgNode,
    CpgNodeKind, CweAnalysisConfig, DfgEdgeKind, Language, MethodSignature, NodeId, SourceRange,
    Visibility,
};

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let id = cpg.add_node(node(kind).with_parent(parent));
    cpg.connect(parent, id, CpgEdgeKind::AstChild)
        .expect("example AST endpoints exist");
    id
}

fn call(cpg: &mut CodePropertyGraph, parent: NodeId, path: &str) -> NodeId {
    let site = child(
        cpg,
        parent,
        CpgNodeKind::Call {
            target: None,
            is_method: path.contains('.') || path.contains("::"),
        },
    );
    child(
        cpg,
        site,
        CpgNodeKind::Identifier {
            name: path.into(),
            definition: None,
        },
    );
    site
}

fn main() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let owner = cpg.add_node(node(CpgNodeKind::Function {
        signature: Box::new(MethodSignature {
            name: "endpoint".into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        }),
    }));
    let source = call(&mut cpg, owner, "request.body");
    let sink = call(&mut cpg, owner, "sqlx::query");
    let argument = child(
        &mut cpg,
        sink,
        CpgNodeKind::Identifier {
            name: "body".into(),
            definition: Some(source),
        },
    );
    cpg.connect(source, argument, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
        .expect("example DFG endpoints exist");

    let report = analyze_builtin_cwes(
        &cpg,
        &CallGraphResolution::default(),
        &CweAnalysisConfig::default(),
    )
    .expect("the example graph and built-in templates are valid");
    assert!(report.is_complete(), "{report:#?}");
    assert_eq!(report.candidates.len(), 1, "{report:#?}");

    let candidate = &report.candidates[0];
    assert_eq!(candidate.cwe, 89);
    assert_eq!(candidate.site, sink);
    println!(
        "advisory CWE-{} candidate at {:?}, confidence {:.3}, witness {:?}",
        candidate.cwe,
        candidate.site,
        f32::from(candidate.confidence_milli) / 1_000.0,
        candidate.witness,
    );
}
