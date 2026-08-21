//! Small-stack regression for deep ML feature and AST-path extraction.

use libcpg::{
    ast_paths, export_pyg, AstPathOptions, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind,
    Language, MethodSignature, NodeId, PygExport, ScopeId, SourceRange, Visibility,
};

const DEPTH: u32 = 100_000;
const FUNCTION: u32 = 0;
const BINARY: u32 = DEPTH + 1;
const LEFT: u32 = DEPTH + 2;
const RIGHT: u32 = DEPTH + 3;

fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(id), kind, SourceRange::from_bytes(id, id + 1))
}

fn function() -> CpgNodeKind {
    CpgNodeKind::Function {
        signature: Box::new(MethodSignature {
            name: "deep_export".into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Private,
        }),
    }
}

fn add_ast(cpg: &mut CodePropertyGraph, parent: u32, child: u32) {
    cpg.connect(
        NodeId::new(parent),
        NodeId::new(child),
        CpgEdgeKind::AstChild,
    )
    .expect("deep fixture endpoints exist");
}

#[test]
fn hundred_thousand_node_ast_exports_on_a_small_native_stack() {
    std::thread::Builder::new()
        .name("ml-export-small-stack".into())
        .stack_size(256 * 1024)
        .spawn(|| {
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            cpg.add_node_with_id(node(FUNCTION, function()));
            for id in 1..=DEPTH {
                cpg.add_node_with_id(node(
                    id,
                    CpgNodeKind::Block {
                        scope: ScopeId::GLOBAL,
                    },
                ));
            }
            cpg.add_node_with_id(node(
                BINARY,
                CpgNodeKind::BinaryOp {
                    operator: "+".into(),
                },
            ));
            cpg.add_node_with_id(node(
                LEFT,
                CpgNodeKind::Identifier {
                    name: "left".into(),
                    definition: None,
                },
            ));
            cpg.add_node_with_id(node(
                RIGHT,
                CpgNodeKind::Identifier {
                    name: "right".into(),
                    definition: None,
                },
            ));
            add_ast(&mut cpg, FUNCTION, 1);
            for id in 1..DEPTH {
                add_ast(&mut cpg, id, id + 1);
            }
            add_ast(&mut cpg, DEPTH, BINARY);
            add_ast(&mut cpg, BINARY, LEFT);
            add_ast(&mut cpg, BINARY, RIGHT);

            let before = (cpg.node_count(), cpg.edge_count());
            let paths = ast_paths(
                &cpg,
                NodeId::new(FUNCTION),
                AstPathOptions {
                    max_ancestor_steps: 300_000,
                    ..AstPathOptions::default()
                },
            )
            .expect("deep path extraction remains stack safe");
            assert_eq!(paths.len(), 1);
            assert_eq!(paths[0].path, [LEFT, BINARY, RIGHT].map(NodeId::new));

            let export = export_pyg(&cpg);
            assert_eq!(export.node_ids.len(), DEPTH as usize + 4);
            let deepest = export.dense_index(NodeId::new(RIGHT)).expect("right leaf") as usize;
            let depth = PygExport::feature_index("ast.depth").expect("stable AST-depth column");
            assert_eq!(export.node_features[deepest][depth], (DEPTH + 2) as f32);
            assert_eq!((cpg.node_count(), cpg.edge_count()), before);
        })
        .expect("spawn small-stack ML export test")
        .join()
        .expect("small-stack ML export thread completed");
}
