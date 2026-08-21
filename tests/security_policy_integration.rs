//! Public-API and parser-pipeline gates for secret-flow and access-guard analysis.

use libcpg::{
    analyze_builtin_authz, analyze_builtin_secrets, AuthzAnalysisConfig, CallGraphResolution,
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, Language, LiteralKind, MethodSignature,
    NodeId, PeerScope, PopulationThreshold, SecretAnalysisConfig, SecretSinkFamily, SourceRange,
    Visibility,
};

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let id = cpg.add_node(node(kind).with_parent(parent));
    cpg.connect(parent, id, CpgEdgeKind::AstChild)
        .expect("security-policy integration AST endpoints exist");
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

fn call(cpg: &mut CodePropertyGraph, owner: NodeId, path: &str) -> NodeId {
    let site = child(
        cpg,
        owner,
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

fn authz_config(supporting: usize, population: usize) -> AuthzAnalysisConfig {
    AuthzAnalysisConfig {
        minimum_support: PopulationThreshold::new(supporting, population),
        minimum_population: population,
        peer_scope: PeerScope::WholeProgram,
        ..AuthzAnalysisConfig::default()
    }
}

#[test]
fn feature_free_public_apis_report_secret_flow_and_only_the_unguarded_mutation() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    let secret_owner = function(&mut cpg, "login");
    let secret_sink = call(&mut cpg, secret_owner, "client::authenticate");
    let secret = child(
        &mut cpg,
        secret_sink,
        CpgNodeKind::Literal {
            kind: LiteralKind::String("A7z!qP2#mN9$vR4@xK8&".into()),
        },
    );

    let mut mutations = Vec::new();
    for index in 0..5 {
        let owner = function(&mut cpg, &format!("route_{index}"));
        let mutation = call(&mut cpg, owner, "repository::update");
        if index < 4 {
            let guard = call(&mut cpg, owner, "authz::authorize");
            cpg.connect(guard, mutation, CpgEdgeKind::ControlDependence)
                .expect("security-policy integration PDG endpoints exist");
        }
        mutations.push(mutation);
    }
    let shape = (cpg.node_count(), cpg.edge_count());

    let secrets = analyze_builtin_secrets(
        &cpg,
        &CallGraphResolution::default(),
        &SecretAnalysisConfig::default(),
    );
    let authz = analyze_builtin_authz(&cpg, &authz_config(4, 5))
        .expect("valid access-control population policy");

    assert!(secrets.is_complete(), "{secrets:#?}");
    assert!(secrets.findings.iter().any(|finding| {
        finding.source == secret
            && finding.sink == secret_sink
            && finding.family == SecretSinkFamily::Authentication
    }));
    assert!(authz.is_complete(), "{authz:#?}");
    assert_eq!(authz.findings.len(), 1, "{authz:#?}");
    assert_eq!(authz.findings[0].site, mutations[4]);
    assert_eq!((cpg.node_count(), cpg.edge_count()), shape);
}

#[cfg(feature = "lang-rust")]
#[test]
fn parser_built_rust_corpus_composes_literal_taint_and_population_guards() {
    use libcpg::{CpgBuilder, PdgBuilder, TreeSitterCpgBuilder};

    let source = r#"
fn route_one(user: &User) {
    if authz::authorize(user) {
        repository::update();
    }
}

fn route_two(user: &User) {
    if authz::authorize(user) {
        repository::update();
    }
}

fn route_three() {
    repository::update();
}

fn login() {
    client::authenticate("A7z!qP2#mN9$vR4@xK8&");
}
"#;
    let mut cpg = TreeSitterCpgBuilder::new()
        .build(source, Language::Rust)
        .expect("parser-built Rust security-policy corpus");
    let functions: Vec<_> = cpg.functions().map(|function| function.id).collect();
    for function in functions {
        PdgBuilder::new().build(&mut cpg, function);
    }
    let shape = (cpg.node_count(), cpg.edge_count());

    let secrets = analyze_builtin_secrets(
        &cpg,
        &CallGraphResolution::default(),
        &SecretAnalysisConfig::default(),
    );
    let authz = analyze_builtin_authz(&cpg, &authz_config(2, 3))
        .expect("valid parser-built access-control population");

    assert!(secrets.is_complete(), "{secrets:#?}");
    assert_eq!(
        secrets
            .findings
            .iter()
            .filter(|finding| finding.family == SecretSinkFamily::Authentication)
            .count(),
        1,
        "{secrets:#?}"
    );
    assert!(authz.is_complete(), "{authz:#?}");
    assert_eq!(authz.findings.len(), 1, "{authz:#?}");
    assert_eq!(
        authz.findings[0].peer_support[&authz.findings[0].expected_guards[0]],
        libcpg::PopulationFraction::new(2, 3)
    );
    assert_eq!((cpg.node_count(), cpg.edge_count()), shape);
}
