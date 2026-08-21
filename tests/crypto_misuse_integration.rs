//! Public-API and parser-pipeline gates for cryptographic misuse analysis.

use libcpg::{
    analyze_builtin_crypto, CallGraphResolution, CodePropertyGraph, CpgEdgeKind, CpgNode,
    CpgNodeKind, CryptoAnalysisConfig, CryptoEvidence, CryptoMisuseKind, Language, LiteralKind,
    MethodSignature, NodeId, SourceRange, Visibility,
};

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let id = cpg.add_node(node(kind).with_parent(parent));
    cpg.connect(parent, id, CpgEdgeKind::AstChild)
        .expect("crypto integration AST endpoints exist");
    id
}

fn function(cpg: &mut CodePropertyGraph, name: &str) -> NodeId {
    cpg.add_node(node(CpgNodeKind::Function {
        signature: Box::new(MethodSignature {
            name: name.into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        }),
    }))
}

fn call(cpg: &mut CodePropertyGraph, parent: NodeId, path: &str) -> NodeId {
    let site = child(
        cpg,
        parent,
        CpgNodeKind::Call {
            target: None,
            is_method: path.contains("::") || path.contains('.'),
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

#[test]
fn public_api_retains_literal_taint_evidence_without_mutating_the_cpg() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let owner = function(&mut cpg, "vulnerable");
    let sink = call(&mut cpg, owner, "Cipher::with_key");
    let literal = child(
        &mut cpg,
        sink,
        CpgNodeKind::Literal {
            kind: LiteralKind::String("redacted-integration-key".into()),
        },
    );
    let shape = (cpg.node_count(), cpg.edge_count());

    let analysis = analyze_builtin_crypto(
        &cpg,
        &CallGraphResolution::default(),
        &CryptoAnalysisConfig::default(),
    );

    assert!(analysis.is_complete(), "{analysis:#?}");
    let finding = analysis
        .findings
        .iter()
        .find(|finding| finding.kind == CryptoMisuseKind::HardcodedKey)
        .expect("hardcoded key is reported");
    assert_eq!(finding.site, sink);
    assert!(matches!(
        &finding.evidence,
        CryptoEvidence::Taint { source, sink: evidence_sink, .. }
            if *source == literal && *evidence_sink == sink
    ));
    assert_eq!((cpg.node_count(), cpg.edge_count()), shape);
}

#[cfg(feature = "lang-rust")]
#[test]
fn parser_built_rust_corpus_reports_crypto_misuses_and_keeps_random_nonce_clean() {
    use libcpg::{CpgBuilder, TreeSitterCpgBuilder};

    let source = r#"
fn vulnerable() {
    Cipher::getInstance("AES/ECB/PKCS5Padding");
    MD5::digest("payload");
    Cipher::with_key("redacted-parser-key");
    cipher::set_iv("fixed-parser-iv");
}

fn safe() {
    let nonce = OsRng::generate_nonce();
    cipher::encrypt(nonce, "payload");
}
"#;
    let cpg = TreeSitterCpgBuilder::new()
        .build(source, Language::Rust)
        .expect("parser-built Rust crypto corpus");
    let shape = (cpg.node_count(), cpg.edge_count());
    let analysis = analyze_builtin_crypto(
        &cpg,
        &CallGraphResolution::default(),
        &CryptoAnalysisConfig::default(),
    );

    assert!(analysis.is_complete(), "{analysis:#?}");
    for kind in [
        CryptoMisuseKind::EcbMode,
        CryptoMisuseKind::WeakHash,
        CryptoMisuseKind::HardcodedKey,
        CryptoMisuseKind::PredictableIv,
    ] {
        assert!(
            analysis.findings.iter().any(|finding| finding.kind == kind),
            "missing {kind:?}: {analysis:#?}"
        );
    }
    assert_eq!(
        analysis
            .findings
            .iter()
            .filter(|finding| finding.kind == CryptoMisuseKind::PredictableIv)
            .count(),
        1,
        "the random nonce is not a predictable-IV candidate: {analysis:#?}"
    );
    assert_eq!((cpg.node_count(), cpg.edge_count()), shape);
}
