//! Runs the feature-free secret-flow and missing access-guard analyses.

use libcpg::{
    analyze_builtin_authz, analyze_builtin_secrets, AuthzAnalysisConfig, CallGraphResolution,
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, Language, LiteralKind, MethodSignature,
    NodeId, PeerScope, PopulationThreshold, SecretAnalysisConfig, SourceRange, Visibility,
};

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
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

fn call(cpg: &mut CodePropertyGraph, owner: NodeId, path: &str) -> NodeId {
    let site = child(
        cpg,
        owner,
        CpgNodeKind::Call {
            target: None,
            is_method: path.contains("::"),
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
    let login = function(&mut cpg, "login");
    let authentication = call(&mut cpg, login, "client::authenticate");
    child(
        &mut cpg,
        authentication,
        CpgNodeKind::Literal {
            // Findings retain only length/entropy and node IDs, never this value.
            kind: LiteralKind::String("A7z!qP2#mN9$vR4@xK8&".into()),
        },
    );

    for index in 0..5 {
        let route = function(&mut cpg, &format!("route_{index}"));
        let mutation = call(&mut cpg, route, "repository::update");
        if index < 4 {
            let guard = call(&mut cpg, route, "authz::authorize");
            cpg.connect(guard, mutation, CpgEdgeKind::ControlDependence)
                .expect("example PDG endpoints exist");
        }
    }

    let secrets = analyze_builtin_secrets(
        &cpg,
        &CallGraphResolution::default(),
        &SecretAnalysisConfig::default(),
    );
    let authz = analyze_builtin_authz(
        &cpg,
        &AuthzAnalysisConfig {
            minimum_support: PopulationThreshold::new(4, 5),
            minimum_population: 5,
            peer_scope: PeerScope::WholeProgram,
            ..AuthzAnalysisConfig::default()
        },
    )
    .expect("example population configuration is valid");

    println!(
        "secret analysis complete: {}; findings: {}",
        secrets.is_complete(),
        secrets.findings.len()
    );
    for finding in secrets.findings {
        println!(
            "  {:?} flow: {:?} -> {:?}, argument {}",
            finding.family, finding.source, finding.sink, finding.argument_index
        );
    }
    println!(
        "access-control analysis complete: {}; findings: {}",
        authz.is_complete(),
        authz.findings.len()
    );
    for finding in authz.findings {
        println!(
            "  missing access guard at {:?}; {} peer(s)",
            finding.site,
            finding.peer_sites.len()
        );
    }
}
