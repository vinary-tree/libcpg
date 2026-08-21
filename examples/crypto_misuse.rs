//! Runs the feature-free cryptographic API-misuse checker on a tiny CPG.

use libcpg::{
    analyze_builtin_crypto, CallGraphResolution, CodePropertyGraph, CpgEdgeKind, CpgNode,
    CpgNodeKind, CryptoAnalysisConfig, Language, LiteralKind, MethodSignature, NodeId, SourceRange,
    Visibility,
};

fn main() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let function = cpg.add_node(CpgNode::new(
        NodeId::new(0),
        CpgNodeKind::Function {
            signature: MethodSignature {
                name: "example".into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            },
        },
        SourceRange::default(),
    ));
    let call = cpg.add_node(
        CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Call {
                target: None,
                is_method: true,
            },
            SourceRange::default(),
        )
        .with_parent(function),
    );
    let callee = cpg.add_node(
        CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Identifier {
                name: "Cipher::with_key".into(),
                definition: None,
            },
            SourceRange::default(),
        )
        .with_parent(call),
    );
    let key = cpg.add_node(
        CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Literal {
                kind: LiteralKind::String("redacted-example-key".into()),
            },
            SourceRange::default(),
        )
        .with_parent(call),
    );
    for (parent, child) in [(function, call), (call, callee), (call, key)] {
        cpg.connect(parent, child, CpgEdgeKind::AstChild)
            .expect("example AST endpoints exist");
    }

    let analysis = analyze_builtin_crypto(
        &cpg,
        &CallGraphResolution::default(),
        &CryptoAnalysisConfig::default(),
    );
    println!("complete: {}", analysis.is_complete());
    for finding in analysis.findings {
        println!("{:?} at {:?}: {}", finding.kind, finding.site, finding.rule);
    }
}
