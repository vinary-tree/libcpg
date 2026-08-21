//! Public-API and parser-pipeline gates for declarative CWE templates.

#[cfg(feature = "lang-rust")]
use std::collections::BTreeSet;

use libcpg::{
    analyze_builtin_cwes, CallGraphResolution, CodePropertyGraph, CpgEdgeKind, CpgNode,
    CpgNodeKind, CweAnalysisConfig, CweEvidence, DfgEdgeKind, Language, MethodSignature, NodeId,
    SourceRange, Visibility,
};

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let id = cpg.add_node(node(kind).with_parent(parent));
    cpg.connect(parent, id, CpgEdgeKind::AstChild)
        .expect("integration AST endpoints exist");
    id
}

fn function(cpg: &mut CodePropertyGraph, name: &str) -> NodeId {
    cpg.add_node(node(CpgNodeKind::Function {
        signature: MethodSignature {
            name: name.into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        },
    }))
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

fn argument(cpg: &mut CodePropertyGraph, site: NodeId, definition: NodeId) {
    let argument = child(
        cpg,
        site,
        CpgNodeKind::Identifier {
            name: "value".into(),
            definition: Some(definition),
        },
    );
    cpg.connect(
        definition,
        argument,
        CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
    )
    .expect("integration DFG endpoints exist");
}

fn sql_fixture() -> (CodePropertyGraph, NodeId, NodeId) {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let vulnerable = function(&mut cpg, "vulnerable");
    let untrusted = call(&mut cpg, vulnerable, "request.body");
    let vulnerable_sink = call(&mut cpg, vulnerable, "sqlx::query");
    argument(&mut cpg, vulnerable_sink, untrusted);

    let safe = function(&mut cpg, "safe");
    let untrusted = call(&mut cpg, safe, "request.body");
    let sanitized = call(&mut cpg, safe, "parameterize");
    argument(&mut cpg, sanitized, untrusted);
    let safe_sink = call(&mut cpg, safe, "sqlx::query");
    argument(&mut cpg, safe_sink, sanitized);
    (cpg, vulnerable_sink, safe_sink)
}

#[cfg(feature = "lang-rust")]
fn containing_function(cpg: &CodePropertyGraph, start: NodeId) -> Option<NodeId> {
    let mut current = Some(start);
    let mut visited = BTreeSet::new();
    while let Some(node) = current {
        if !visited.insert(node) {
            return None;
        }
        if cpg
            .node(node)
            .is_some_and(|node| matches!(node.kind, CpgNodeKind::Function { .. }))
        {
            return Some(node);
        }
        current = cpg.ast_parent(node);
    }
    None
}

#[test]
fn public_builtin_pipeline_reports_only_qualified_unsanitized_evidence() {
    let (cpg, vulnerable_sink, safe_sink) = sql_fixture();
    let graph_size = (cpg.node_count(), cpg.edge_count());
    let report = analyze_builtin_cwes(
        &cpg,
        &CallGraphResolution::default(),
        &CweAnalysisConfig::default(),
    )
    .expect("the public fixture and built-in templates are valid");

    assert!(report.is_complete(), "{report:#?}");
    let sql_candidates: Vec<_> = report
        .candidates
        .iter()
        .filter(|candidate| candidate.cwe == 89)
        .collect();
    assert_eq!(sql_candidates.len(), 1, "{report:#?}");
    assert_eq!(sql_candidates[0].site, vulnerable_sink);
    assert_ne!(sql_candidates[0].site, safe_sink);
    assert!(matches!(
        sql_candidates[0].evidence,
        CweEvidence::Taint { .. }
    ));
    assert!(sql_candidates[0]
        .witness
        .iter()
        .all(|&id| cpg.node(id).is_some()));
    assert_eq!((cpg.node_count(), cpg.edge_count()), graph_size);
}

#[cfg(feature = "lang-rust")]
#[test]
fn parser_built_rust_corpus_distinguishes_vulnerable_and_sanitized_sql_flows() {
    use libcpg::{CpgBuilder, TreeSitterCpgBuilder};

    let source = r#"
fn read_line() -> String { String::new() }
fn parameterize(value: String) -> String { value }
mod sqlx { pub fn query(_value: String) {} }

fn vulnerable() {
    let input = read_line();
    sqlx::query(input);
}

fn safe() {
    let input = read_line();
    let clean = parameterize(input);
    sqlx::query(clean);
}
"#;
    let cpg = TreeSitterCpgBuilder::new()
        .build(source, Language::Rust)
        .expect("parser-built Rust CWE corpus");
    let graph_size = (cpg.node_count(), cpg.edge_count());
    let report = analyze_builtin_cwes(
        &cpg,
        &CallGraphResolution::default(),
        &CweAnalysisConfig::default(),
    )
    .expect("parser-built graph and built-in templates are valid");

    assert!(report.is_complete(), "{report:#?}");
    let sql_candidates: Vec<_> = report
        .candidates
        .iter()
        .filter(|candidate| candidate.cwe == 89)
        .collect();
    assert_eq!(sql_candidates.len(), 1, "{report:#?}");
    let owner = containing_function(&cpg, sql_candidates[0].site)
        .expect("reported SQL sink belongs to a represented function");
    assert_eq!(
        cpg.node(owner).and_then(|node| node.name()),
        Some("vulnerable")
    );
    assert_eq!((cpg.node_count(), cpg.edge_count()), graph_size);
}
