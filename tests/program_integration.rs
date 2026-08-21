use libcpg::{
    build_import_graph, merge_programs, resolve_calls, CodePropertyGraph, Confidence, CpgEdgeKind,
    CpgNode, CpgNodeKind, ImportResolutionKind, Language, MethodSignature, NodeId, ResolveConfig,
    SourceRange, Visibility,
};

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn function(name: &str) -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: Box::new(MethodSignature {
            name: name.into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Public,
        }),
    }
}

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let child = cpg.add_node(node(kind));
    cpg.connect(parent, child, CpgEdgeKind::AstChild);
    child
}

fn library(path: &str) -> (CodePropertyGraph, NodeId) {
    let mut cpg = CodePropertyGraph::new(Language::Rust).with_source_path(path);
    let root = cpg.add_node(node(CpgNodeKind::Root));
    let add = child(&mut cpg, root, function("add"));
    (cpg, add)
}

fn application() -> (CodePropertyGraph, NodeId, NodeId) {
    let mut cpg = CodePropertyGraph::new(Language::Rust).with_source_path("src/app.rs");
    let root = cpg.add_node(node(CpgNodeKind::Root));
    let import = child(
        &mut cpg,
        root,
        CpgNodeKind::Import {
            path: "use crate::math::add;".into(),
        },
    );
    let caller = child(&mut cpg, root, function("caller"));
    let call = child(
        &mut cpg,
        caller,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    child(
        &mut cpg,
        call,
        CpgNodeKind::Identifier {
            name: "add".into(),
            definition: None,
        },
    );
    (cpg, import, call)
}

#[test]
fn public_program_api_links_and_resolves_the_selected_cross_file_target() {
    let (app, app_import, app_call) = application();
    let (math, math_add) = library("src/math.rs");
    let (rival, _) = library("src/rival.rs");
    let expected_nodes = app.node_count() + math.node_count() + rival.node_count();
    let expected_edges = app.edge_count() + math.edge_count() + rival.edge_count();
    let mut program = merge_programs(vec![app, math, rival]);
    let import = program.files[0]
        .translated(app_import)
        .expect("translated import");
    let call = program.files[0]
        .translated(app_call)
        .expect("translated call");
    let add = program.files[1]
        .translated(math_add)
        .expect("translated declaration");

    assert_eq!(program.cpg.node_count(), expected_nodes);
    assert_eq!(program.cpg.edge_count(), expected_edges);
    assert_eq!(program.files.len(), 3);
    let imports = build_import_graph(&mut program);
    assert_eq!(
        imports.resolutions[&import].kind,
        ImportResolutionKind::Exact
    );
    assert_eq!(imports.resolutions[&import].target_files, vec![1]);
    assert_eq!(imports.resolutions[&import].imported_nodes, vec![add]);

    let calls = resolve_calls(&mut program.cpg, &ResolveConfig::default());
    assert_eq!(
        calls.resolutions[&call].confidence,
        Confidence::ExactViaImport
    );
    assert_eq!(calls.resolutions[&call].target, Some(add));
    assert_eq!(calls.resolutions[&call].candidate_count, 1);
    assert!(program
        .cpg
        .edges_between(call, add)
        .iter()
        .any(|edge| edge.kind == CpgEdgeKind::CallSite));
}

#[test]
fn public_program_api_keeps_ambiguous_file_aliases_out_of_dependency_topology() {
    let (app, app_import, _) = application();
    let (left, _) = library("src/left/math.rs");
    let (right, _) = library("src/right/math.rs");
    let mut program = merge_programs(vec![app, left, right]);
    let import = program.files[0]
        .translated(app_import)
        .expect("translated import");

    let imports = build_import_graph(&mut program);
    assert_eq!(
        imports.resolutions[&import].kind,
        ImportResolutionKind::Ambiguous
    );
    assert_eq!(imports.resolutions[&import].target_files, vec![1, 2]);
    assert!(imports.dependencies.is_empty());
    assert!(program
        .cpg
        .outgoing_edges(import)
        .all(|edge| edge.kind != CpgEdgeKind::Imports));
}

#[test]
fn public_program_api_resolves_a_renamed_import_by_its_local_binding() {
    let mut app = CodePropertyGraph::new(Language::Python).with_source_path("pkg/app.py");
    let root = app.add_node(node(CpgNodeKind::Root));
    let import = child(
        &mut app,
        root,
        CpgNodeKind::Import {
            path: "from math import add as sum".into(),
        },
    );
    let caller = child(&mut app, root, function("caller"));
    let call = child(
        &mut app,
        caller,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    );
    child(
        &mut app,
        call,
        CpgNodeKind::Identifier {
            name: "sum".into(),
            definition: None,
        },
    );
    let (math, source_add) = library("pkg/math.py");
    let (rival, _) = library("pkg/rival.py");
    let mut program = merge_programs(vec![app, math, rival]);
    let import = program.files[0]
        .translated(import)
        .expect("translated import");
    let call = program.files[0].translated(call).expect("translated call");
    let add = program.files[1]
        .translated(source_add)
        .expect("translated declaration");

    build_import_graph(&mut program);
    assert!(program
        .cpg
        .edges_between(import, add)
        .iter()
        .any(|edge| edge.kind == CpgEdgeKind::Imports && edge.label.as_deref() == Some("sum")));
    let calls = resolve_calls(&mut program.cpg, &ResolveConfig::default());
    assert_eq!(
        calls.resolutions[&call].confidence,
        Confidence::ExactViaImport
    );
    assert_eq!(calls.resolutions[&call].target, Some(add));
}
