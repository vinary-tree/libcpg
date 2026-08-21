//! Public-API integration coverage for deterministic external ML exports.

use libcpg::{
    ast_paths, export_pyg_with_options, AstPathOptions, CodePropertyGraph, CpgEdgeKind, CpgNode,
    CpgNodeKind, DfgEdgeKind, Language, LiteralKind, MethodSignature, NodeId, PygExport,
    PygExportOptions, SourceRange, Visibility,
};

fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(
        NodeId::new(id),
        kind,
        SourceRange::from_bytes(id * 10, id * 10 + 1),
    )
}

fn function(name: &str) -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: Box::new(MethodSignature {
            name: name.into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        }),
    }
}

fn connect(cpg: &mut CodePropertyGraph, source: u32, target: u32, kind: CpgEdgeKind) {
    cpg.connect(NodeId::new(source), NodeId::new(target), kind)
        .expect("public fixture endpoints exist");
}

#[test]
fn public_pipeline_exports_exact_paths_typed_edges_and_localization() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    cpg.add_node_with_id(node(100, function("classify")));
    cpg.add_node_with_id(node(
        200,
        CpgNodeKind::BinaryOp {
            operator: "<".into(),
        },
    ));
    cpg.add_node_with_id(
        node(
            300,
            CpgNodeKind::Identifier {
                name: "input".into(),
                definition: None,
            },
        )
        .with_text("input"),
    );
    cpg.add_node_with_id(
        node(
            400,
            CpgNodeKind::Literal {
                kind: LiteralKind::Integer(10),
            },
        )
        .with_text("10"),
    );
    connect(&mut cpg, 100, 200, CpgEdgeKind::AstChild);
    connect(&mut cpg, 200, 300, CpgEdgeKind::AstChild);
    connect(&mut cpg, 200, 400, CpgEdgeKind::AstChild);
    connect(
        &mut cpg,
        300,
        400,
        CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
    );
    connect(&mut cpg, 300, 400, CpgEdgeKind::DataDependence);

    let before = (cpg.node_count(), cpg.edge_count());
    let export = export_pyg_with_options(
        &cpg,
        PygExportOptions {
            slice_criterion: Some(NodeId::new(400)),
            max_slice_nodes: 2,
        },
    )
    .expect("public localized export");
    assert_eq!(export.node_ids, [100, 200, 300, 400].map(NodeId::new));
    assert_eq!(export.slice_node_ids, [300, 400].map(NodeId::new));
    assert_eq!(export.edge_index.len(), cpg.edge_count());
    assert_eq!(export.node_features.len(), cpg.node_count());
    assert_eq!(export.source_ranges.len(), cpg.node_count());
    assert_eq!(PygExport::feature_index("ast.depth"), Some(63));

    let paths = ast_paths(&cpg, NodeId::new(100), AstPathOptions::default())
        .expect("public exact AST paths");
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0].path, [300, 200, 400].map(NodeId::new));
    assert_eq!(paths[0].start_token, "input");
    assert_eq!(paths[0].end_token, "10");
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}
