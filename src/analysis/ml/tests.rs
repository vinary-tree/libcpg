use proptest::prelude::*;

use super::*;
use crate::{
    AnalysisError, CfgEdgeKind, CodePropertyGraph, CpgEdge, CpgEdgeKind, CpgNode, CpgNodeKind,
    DfgEdgeKind, EdgeId, HeapEdgeKind, Language, LiteralKind, MethodSignature, NameFlowEdgeKind,
    NodeId, PlaceCapabilityEdgeKind, RewriteDepEdgeKind, SourceRange, Visibility,
};

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

fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(
        NodeId::new(id),
        kind,
        SourceRange::from_bytes(id, id.saturating_add(1)),
    )
}

fn add_edge(cpg: &mut CodePropertyGraph, id: u32, source: u32, target: u32, kind: CpgEdgeKind) {
    cpg.add_edge_with_id(CpgEdge::new(
        EdgeId::new(id),
        NodeId::new(source),
        NodeId::new(target),
        kind,
    ))
    .expect("fixture endpoints exist");
}

fn exact_fixture() -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    cpg.add_node_with_id(node(10, function("score")));
    cpg.add_node_with_id(node(20, CpgNodeKind::While));
    cpg.add_node_with_id(node(
        30,
        CpgNodeKind::BinaryOp {
            operator: "+".into(),
        },
    ));
    cpg.add_node_with_id(
        node(
            40,
            CpgNodeKind::Identifier {
                name: "left".into(),
                definition: None,
            },
        )
        .with_text("lhs"),
    );
    cpg.add_node_with_id(node(
        50,
        CpgNodeKind::Literal {
            kind: LiteralKind::Integer(7),
        },
    ));

    add_edge(&mut cpg, 1, 10, 20, CpgEdgeKind::AstChild);
    add_edge(&mut cpg, 2, 20, 30, CpgEdgeKind::AstChild);
    add_edge(&mut cpg, 3, 30, 40, CpgEdgeKind::AstChild);
    add_edge(&mut cpg, 4, 30, 50, CpgEdgeKind::AstChild);
    add_edge(
        &mut cpg,
        5,
        10,
        20,
        CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
    );
    add_edge(
        &mut cpg,
        6,
        40,
        50,
        CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
    );
    add_edge(
        &mut cpg,
        7,
        40,
        50,
        CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
    );
    add_edge(&mut cpg, 8, 20, 30, CpgEdgeKind::DataDependence);
    add_edge(&mut cpg, 9, 30, 50, CpgEdgeKind::DataDependence);
    add_edge(&mut cpg, 10, 50, 40, CpgEdgeKind::Reference);
    cpg
}

fn feature(row: &[f32], name: &str) -> f32 {
    row[PygExport::feature_index(name).expect("stable feature name")]
}

fn every_edge_kind() -> Vec<CpgEdgeKind> {
    let cfg = [
        CfgEdgeKind::Sequential,
        CfgEdgeKind::ConditionalTrue,
        CfgEdgeKind::ConditionalFalse,
        CfgEdgeKind::LoopBack,
        CfgEdgeKind::LoopExit,
        CfgEdgeKind::Break,
        CfgEdgeKind::Continue,
        CfgEdgeKind::Return,
        CfgEdgeKind::Throw,
        CfgEdgeKind::Catch,
        CfgEdgeKind::Call,
        CfgEdgeKind::CallReturn,
        CfgEdgeKind::Case,
        CfgEdgeKind::DefaultCase,
    ];
    let dfg = [
        DfgEdgeKind::DefUse,
        DfgEdgeKind::UseDef,
        DfgEdgeKind::ReachingDef,
        DfgEdgeKind::DataDependency,
        DfgEdgeKind::Parameter,
        DfgEdgeKind::ReturnValue,
        DfgEdgeKind::FieldRead,
        DfgEdgeKind::FieldWrite,
        DfgEdgeKind::IndexRead,
        DfgEdgeKind::IndexWrite,
        DfgEdgeKind::Alias,
        DfgEdgeKind::Dereference,
        DfgEdgeKind::AddressOf,
    ];
    let mut kinds = vec![
        CpgEdgeKind::AstChild,
        CpgEdgeKind::AstParent,
        CpgEdgeKind::AstNextSibling,
        CpgEdgeKind::AstPrevSibling,
    ];
    kinds.extend(cfg.into_iter().map(CpgEdgeKind::ControlFlow));
    kinds.extend(dfg.into_iter().map(CpgEdgeKind::DataFlow));
    kinds.extend([
        CpgEdgeKind::ControlDependence,
        CpgEdgeKind::DataDependence,
        CpgEdgeKind::StaticCall,
        CpgEdgeKind::DynamicCall,
        CpgEdgeKind::CallSite,
        CpgEdgeKind::TypeOf,
        CpgEdgeKind::Inherits,
        CpgEdgeKind::Implements,
        CpgEdgeKind::GenericInstance,
        CpgEdgeKind::Reference,
        CpgEdgeKind::Definition,
        CpgEdgeKind::Declaration,
        CpgEdgeKind::EnclosingScope,
        CpgEdgeKind::ContainedIn,
        CpgEdgeKind::Imports,
        CpgEdgeKind::Exports,
        CpgEdgeKind::Heap(HeapEdgeKind::Allocates),
        CpgEdgeKind::Heap(HeapEdgeKind::PointsTo),
        CpgEdgeKind::Heap(HeapEdgeKind::Dereferences),
        CpgEdgeKind::Heap(HeapEdgeKind::Releases),
        CpgEdgeKind::Heap(HeapEdgeKind::RequiresValid),
        CpgEdgeKind::Heap(HeapEdgeKind::EnsuresInvalid),
        CpgEdgeKind::NameFlow(NameFlowEdgeKind::Creates),
        CpgEdgeKind::NameFlow(NameFlowEdgeKind::References),
        CpgEdgeKind::NameFlow(NameFlowEdgeKind::Quotes),
        CpgEdgeKind::NameFlow(NameFlowEdgeKind::Drops),
        CpgEdgeKind::NameFlow(NameFlowEdgeKind::Carries),
        CpgEdgeKind::NameFlow(NameFlowEdgeKind::Receives),
        CpgEdgeKind::NameFlow(NameFlowEdgeKind::Communicates),
        CpgEdgeKind::RewriteDep(RewriteDepEdgeKind::DependencyPair),
        CpgEdgeKind::RewriteDep(RewriteDepEdgeKind::CriticalConflict),
        CpgEdgeKind::PlaceCapability(PlaceCapabilityEdgeKind::Move),
        CpgEdgeKind::PlaceCapability(PlaceCapabilityEdgeKind::SharedBorrow),
        CpgEdgeKind::PlaceCapability(PlaceCapabilityEdgeKind::MutableBorrow),
        CpgEdgeKind::PlaceCapability(PlaceCapabilityEdgeKind::SharedReborrow),
        CpgEdgeKind::PlaceCapability(PlaceCapabilityEdgeKind::MutableReborrow),
    ]);
    kinds
}

#[test]
fn relation_registry_is_complete_contiguous_and_stable() {
    let cfg = [
        CfgEdgeKind::Sequential,
        CfgEdgeKind::ConditionalTrue,
        CfgEdgeKind::ConditionalFalse,
        CfgEdgeKind::LoopBack,
        CfgEdgeKind::LoopExit,
        CfgEdgeKind::Break,
        CfgEdgeKind::Continue,
        CfgEdgeKind::Return,
        CfgEdgeKind::Throw,
        CfgEdgeKind::Catch,
        CfgEdgeKind::Call,
        CfgEdgeKind::CallReturn,
        CfgEdgeKind::Case,
        CfgEdgeKind::DefaultCase,
    ];
    let dfg = [
        DfgEdgeKind::DefUse,
        DfgEdgeKind::UseDef,
        DfgEdgeKind::ReachingDef,
        DfgEdgeKind::DataDependency,
        DfgEdgeKind::Parameter,
        DfgEdgeKind::ReturnValue,
        DfgEdgeKind::FieldRead,
        DfgEdgeKind::FieldWrite,
        DfgEdgeKind::IndexRead,
        DfgEdgeKind::IndexWrite,
        DfgEdgeKind::Alias,
        DfgEdgeKind::Dereference,
        DfgEdgeKind::AddressOf,
    ];
    let mut kinds = vec![
        CpgEdgeKind::AstChild,
        CpgEdgeKind::AstParent,
        CpgEdgeKind::AstNextSibling,
        CpgEdgeKind::AstPrevSibling,
    ];
    kinds.extend(cfg.into_iter().map(CpgEdgeKind::ControlFlow));
    kinds.extend(dfg.into_iter().map(CpgEdgeKind::DataFlow));
    kinds.extend([
        CpgEdgeKind::ControlDependence,
        CpgEdgeKind::DataDependence,
        CpgEdgeKind::StaticCall,
        CpgEdgeKind::DynamicCall,
        CpgEdgeKind::CallSite,
        CpgEdgeKind::TypeOf,
        CpgEdgeKind::Inherits,
        CpgEdgeKind::Implements,
        CpgEdgeKind::GenericInstance,
        CpgEdgeKind::Reference,
        CpgEdgeKind::Definition,
        CpgEdgeKind::Declaration,
        CpgEdgeKind::EnclosingScope,
        CpgEdgeKind::ContainedIn,
        CpgEdgeKind::Imports,
        CpgEdgeKind::Exports,
        CpgEdgeKind::Heap(HeapEdgeKind::Allocates),
        CpgEdgeKind::Heap(HeapEdgeKind::PointsTo),
        CpgEdgeKind::Heap(HeapEdgeKind::Dereferences),
        CpgEdgeKind::Heap(HeapEdgeKind::Releases),
        CpgEdgeKind::Heap(HeapEdgeKind::RequiresValid),
        CpgEdgeKind::Heap(HeapEdgeKind::EnsuresInvalid),
        CpgEdgeKind::NameFlow(NameFlowEdgeKind::Creates),
        CpgEdgeKind::NameFlow(NameFlowEdgeKind::References),
        CpgEdgeKind::NameFlow(NameFlowEdgeKind::Quotes),
        CpgEdgeKind::NameFlow(NameFlowEdgeKind::Drops),
        CpgEdgeKind::NameFlow(NameFlowEdgeKind::Carries),
        CpgEdgeKind::NameFlow(NameFlowEdgeKind::Receives),
        CpgEdgeKind::NameFlow(NameFlowEdgeKind::Communicates),
        CpgEdgeKind::RewriteDep(RewriteDepEdgeKind::DependencyPair),
        CpgEdgeKind::RewriteDep(RewriteDepEdgeKind::CriticalConflict),
        CpgEdgeKind::PlaceCapability(PlaceCapabilityEdgeKind::Move),
        CpgEdgeKind::PlaceCapability(PlaceCapabilityEdgeKind::SharedBorrow),
        CpgEdgeKind::PlaceCapability(PlaceCapabilityEdgeKind::MutableBorrow),
        CpgEdgeKind::PlaceCapability(PlaceCapabilityEdgeKind::SharedReborrow),
        CpgEdgeKind::PlaceCapability(PlaceCapabilityEdgeKind::MutableReborrow),
    ]);

    assert_eq!(kinds.len(), usize::from(EDGE_KIND_CLASS_COUNT));
    let classes: Vec<_> = kinds.iter().map(edge_kind_class).collect();
    assert_eq!(classes, (0..EDGE_KIND_CLASS_COUNT).collect::<Vec<_>>());
    assert_eq!(PYG_NODE_FEATURE_NAMES.len(), PYG_NODE_FEATURE_COUNT);
    assert_eq!(node_kind_class(&CpgNodeKind::Root), 0);
    assert_eq!(
        node_kind_class(&CpgNodeKind::Unknown {
            kind: "future".into()
        }),
        44
    );
}

#[test]
fn heterogeneous_export_is_exact_for_every_typed_relation() {
    let kinds = every_edge_kind();
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    cpg.add_node_with_id(node(20, CpgNodeKind::Return));
    cpg.add_node_with_id(
        node(
            10,
            CpgNodeKind::Identifier {
                name: "value".into(),
                definition: None,
            },
        )
        .with_text("value"),
    );
    for (index, kind) in kinds.iter().cloned().enumerate() {
        add_edge(&mut cpg, index as u32, 10, 20, kind);
    }

    let export = export_heterogeneous_gnn(&cpg);
    assert_eq!(export.node_ids, [10, 20].map(NodeId::new));
    assert_eq!(export.node_types, [32, 12]);
    assert_eq!(export.tokens, ["value", "return"]);
    assert_eq!(
        export.source_ranges,
        [
            node(10, CpgNodeKind::Return).range,
            node(20, CpgNodeKind::Return).range
        ]
    );
    assert_eq!(export.relations.len(), usize::from(EDGE_KIND_CLASS_COUNT));
    assert_eq!(EDGE_KIND_CLASS_NAMES.len(), export.relations.len());
    for (class, kind) in kinds.iter().enumerate() {
        let relation = export.relation(kind);
        assert_eq!(relation.relation_class, class as u16);
        assert_eq!(relation.relation_name, EDGE_KIND_CLASS_NAMES[class]);
        assert_eq!(relation.edge_index, [(0, 1)]);
    }

    for (row, &node_type) in export.node_features.iter().zip(&export.node_types) {
        assert_eq!(row.len(), HETERO_NODE_FEATURE_COUNT);
        assert_eq!(row[..45].iter().sum::<f32>(), 1.0);
        assert_eq!(row[usize::from(node_type)], 1.0);
        assert!(row[45..].iter().any(|value| *value != 0.0));
    }
}

#[test]
fn heterogeneous_export_keeps_empty_buckets_and_parallel_multiplicity() {
    let export = export_heterogeneous_gnn(&exact_fixture());
    assert_eq!(export.tokens, ["score", "while", "+", "lhs", "7"]);
    assert_eq!(export.node_types, [7, 15, 26, 32, 33]);
    assert_eq!(
        export
            .relation(&CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
            .edge_index,
        [(3, 4), (3, 4)]
    );
    assert!(export
        .relation(&CpgEdgeKind::Heap(HeapEdgeKind::Allocates))
        .edge_index
        .is_empty());
    assert_eq!(export.dense_index(NodeId::new(30)), Some(2));
    assert_eq!(export.dense_index(NodeId::new(999)), None);
}

#[test]
fn rewrite_dep_has_append_only_degree_columns_and_relation_classes() {
    let mut cpg = CodePropertyGraph::new(Language::MeTTa);
    cpg.add_node_with_id(CpgNode::new(
        NodeId::new(10),
        function("left"),
        SourceRange::default(),
    ));
    cpg.add_node_with_id(CpgNode::new(
        NodeId::new(20),
        function("right"),
        SourceRange::default(),
    ));
    cpg.connect(
        NodeId::new(10),
        NodeId::new(20),
        CpgEdgeKind::RewriteDep(RewriteDepEdgeKind::DependencyPair),
    );

    let export = export_pyg(&cpg);
    let incoming = PYG_NODE_FEATURE_NAMES
        .iter()
        .position(|name| *name == "degree.in.rewrite_dep")
        .expect("incoming RewriteDep column");
    let outgoing = PYG_NODE_FEATURE_NAMES
        .iter()
        .position(|name| *name == "degree.out.rewrite_dep")
        .expect("outgoing RewriteDep column");

    assert_eq!(incoming, 73);
    assert_eq!(outgoing, 74);
    assert_eq!(PygExport::feature_index("ast.depth"), Some(67));
    assert_eq!(export.node_ids, vec![NodeId::new(10), NodeId::new(20)]);
    assert_eq!(export.node_features[0][outgoing], 1.0);
    assert_eq!(export.node_features[0][incoming], 0.0);
    assert_eq!(export.node_features[1][incoming], 1.0);
    assert_eq!(export.node_features[1][outgoing], 0.0);
    assert_eq!(
        export.edge_index,
        vec![(
            0,
            1,
            edge_kind_class(&CpgEdgeKind::RewriteDep(RewriteDepEdgeKind::DependencyPair))
        )]
    );
}

#[test]
fn place_capability_has_append_only_degree_columns_and_relation_classes() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    cpg.add_node_with_id(CpgNode::new(
        NodeId::new(10),
        function("source"),
        SourceRange::default(),
    ));
    cpg.add_node_with_id(CpgNode::new(
        NodeId::new(20),
        function("target"),
        SourceRange::default(),
    ));
    cpg.connect(
        NodeId::new(10),
        NodeId::new(20),
        CpgEdgeKind::PlaceCapability(PlaceCapabilityEdgeKind::MutableReborrow),
    );

    let export = export_pyg(&cpg);
    let incoming = PygExport::feature_index("degree.in.place_capability")
        .expect("incoming PlaceCapability column");
    let outgoing = PygExport::feature_index("degree.out.place_capability")
        .expect("outgoing PlaceCapability column");
    assert_eq!(incoming, 75);
    assert_eq!(outgoing, 76);
    assert_eq!(export.node_features[0][outgoing], 1.0);
    assert_eq!(export.node_features[1][incoming], 1.0);
    assert_eq!(
        export.edge_index,
        vec![(
            0,
            1,
            edge_kind_class(&CpgEdgeKind::PlaceCapability(
                PlaceCapabilityEdgeKind::MutableReborrow
            ))
        )]
    );
}

#[test]
fn pyg_export_has_exact_alignment_features_and_parallel_edges() {
    let cpg = exact_fixture();
    let before = (cpg.node_count(), cpg.edge_count());
    let export = export_pyg(&cpg);

    assert_eq!(export.node_ids, [10, 20, 30, 40, 50].map(NodeId::new));
    assert_eq!(
        export.source_ranges,
        [10, 20, 30, 40, 50].map(|id| SourceRange::from_bytes(id, id + 1))
    );
    assert_eq!(
        export.edge_index,
        vec![
            (0, 1, 0),
            (0, 1, 4),
            (1, 2, 0),
            (1, 2, 32),
            (2, 3, 0),
            (2, 4, 0),
            (2, 4, 32),
            (3, 4, 18),
            (3, 4, 18),
            (4, 3, 40),
        ]
    );
    assert_eq!(export.dense_index(NodeId::new(30)), Some(2));
    assert_eq!(export.dense_index(NodeId::new(99)), None);

    for (row, expected_kind) in export.node_features.iter().zip([7, 15, 26, 32, 33]) {
        assert_eq!(row.len(), PYG_NODE_FEATURE_COUNT);
        assert_eq!(row[..45].iter().sum::<f32>(), 1.0);
        assert_eq!(row[expected_kind], 1.0);
        assert!(row.iter().all(|value| value.is_finite()));
    }
    assert_eq!(
        export
            .node_features
            .iter()
            .map(|row| feature(row, "ast.depth"))
            .collect::<Vec<_>>(),
        vec![0.0, 1.0, 2.0, 3.0, 3.0]
    );
    assert!(export
        .node_features
        .iter()
        .all(|row| feature(row, "ast.depth_known") == 1.0));
    assert_eq!(
        export
            .node_features
            .iter()
            .map(|row| feature(row, "ast.loop_depth"))
            .collect::<Vec<_>>(),
        vec![0.0, 1.0, 1.0, 1.0, 1.0]
    );
    assert_eq!(feature(&export.node_features[2], "degree.out.ast"), 2.0);
    assert_eq!(feature(&export.node_features[3], "degree.out.dfg"), 2.0);
    assert_eq!(
        feature(&export.node_features[4], "degree.out.reference"),
        1.0
    );
    assert!(export.slice_node_ids.is_empty());
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);
}

#[test]
fn pdg_localization_is_complete_or_returns_an_error() {
    let cpg = exact_fixture();
    let criterion = NodeId::new(50);
    let localized = export_pyg_with_options(
        &cpg,
        PygExportOptions {
            slice_criterion: Some(criterion),
            max_slice_nodes: 3,
        },
    )
    .expect("complete bounded slice");
    assert_eq!(localized.slice_node_ids, [20, 30, 50].map(NodeId::new));
    assert_eq!(localized.slice_criterion, Some(criterion));
    assert_eq!(
        localized
            .node_features
            .iter()
            .map(|row| feature(row, "localization.backward_pdg_slice"))
            .collect::<Vec<_>>(),
        vec![0.0, 1.0, 1.0, 0.0, 1.0]
    );

    assert!(matches!(
        export_pyg_with_options(
            &cpg,
            PygExportOptions {
                slice_criterion: Some(criterion),
                max_slice_nodes: 2,
            }
        ),
        Err(AnalysisError::BudgetExceeded {
            analysis: "export_pyg",
            unit: "slice nodes",
            needed: 3,
            cap: 2,
        })
    ));
    assert_eq!(
        export_pyg_with_options(
            &cpg,
            PygExportOptions {
                slice_criterion: Some(NodeId::new(999)),
                max_slice_nodes: 8,
            }
        ),
        Err(AnalysisError::UnknownNode(NodeId::new(999)))
    );
}

#[test]
fn ast_context_reconstructs_the_exact_source_ordered_path() {
    let cpg = exact_fixture();
    let before = (cpg.node_count(), cpg.edge_count());
    let contexts = ast_paths(&cpg, NodeId::new(10), AstPathOptions::default())
        .expect("well-formed exact AST paths");
    assert_eq!(contexts.len(), 1);
    let context = &contexts[0];
    assert_eq!(context.start, NodeId::new(40));
    assert_eq!(context.end, NodeId::new(50));
    assert_eq!(context.ancestor, NodeId::new(30));
    assert_eq!(context.ancestor_index, 1);
    assert_eq!(context.path, [40, 30, 50].map(NodeId::new));
    assert_eq!(context.node_kinds, vec![32, 26, 33]);
    assert_eq!(context.start_token, "lhs");
    assert_eq!(context.end_token, "7");
    for pair in context.path.windows(2) {
        assert!(
            cpg.edges_between(pair[0], pair[1])
                .iter()
                .any(|edge| { edge.kind == CpgEdgeKind::AstChild })
                || cpg
                    .edges_between(pair[1], pair[0])
                    .iter()
                    .any(|edge| { edge.kind == CpgEdgeKind::AstChild })
        );
    }
    assert_eq!((cpg.node_count(), cpg.edge_count()), before);

    let filtered = ast_paths(
        &cpg,
        NodeId::new(10),
        AstPathOptions {
            max_path_length: 1,
            ..AstPathOptions::default()
        },
    )
    .expect("semantic filters are not failures");
    assert!(filtered.is_empty());
}

#[test]
fn malformed_parent_chains_are_suppressed_without_inventing_paths() {
    let mut cpg = CodePropertyGraph::new(Language::Rust);
    for (id, kind) in [
        (1, function("ambiguous")),
        (2, CpgNodeKind::If),
        (3, CpgNodeKind::Else),
        (
            4,
            CpgNodeKind::Identifier {
                name: "shared".into(),
                definition: None,
            },
        ),
    ] {
        cpg.add_node_with_id(node(id, kind));
    }
    add_edge(&mut cpg, 1, 1, 2, CpgEdgeKind::AstChild);
    add_edge(&mut cpg, 2, 1, 3, CpgEdgeKind::AstChild);
    add_edge(&mut cpg, 3, 2, 4, CpgEdgeKind::AstChild);
    add_edge(&mut cpg, 4, 3, 4, CpgEdgeKind::AstChild);
    add_edge(&mut cpg, 5, 2, 3, CpgEdgeKind::AstChild);
    add_edge(&mut cpg, 6, 3, 2, CpgEdgeKind::AstChild);

    assert!(ast_paths(&cpg, NodeId::new(1), AstPathOptions::default())
        .expect("malformed paths are safely suppressed")
        .is_empty());
    let exported = export_pyg(&cpg);
    for id in [2, 3, 4] {
        let row = &exported.node_features[exported.dense_index(NodeId::new(id)).unwrap() as usize];
        assert_eq!(feature(row, "ast.depth_known"), 0.0);
    }
}

#[test]
fn path_validation_reports_selectors_and_every_resource_boundary() {
    let cpg = exact_fixture();
    assert_eq!(
        ast_paths(&cpg, NodeId::new(999), AstPathOptions::default()),
        Err(AnalysisError::UnknownNode(NodeId::new(999)))
    );
    assert_eq!(
        ast_paths(&cpg, NodeId::new(20), AstPathOptions::default()),
        Err(AnalysisError::NotAFunction(NodeId::new(20)))
    );

    let cases = [
        AstPathOptions {
            max_terminals: 1,
            ..AstPathOptions::default()
        },
        AstPathOptions {
            max_pair_checks: 0,
            ..AstPathOptions::default()
        },
        AstPathOptions {
            max_contexts: 0,
            ..AstPathOptions::default()
        },
        AstPathOptions {
            max_total_path_nodes: 2,
            ..AstPathOptions::default()
        },
        AstPathOptions {
            max_ancestor_steps: 0,
            ..AstPathOptions::default()
        },
    ];
    for options in cases {
        assert!(matches!(
            ast_paths(&cpg, NodeId::new(10), options),
            Err(AnalysisError::BudgetExceeded { .. })
        ));
    }
    assert!(matches!(
        ast_paths(
            &cpg,
            NodeId::new(10),
            AstPathOptions {
                max_path_length: MAX_AST_PATH_LENGTH + 1,
                ..AstPathOptions::default()
            }
        ),
        Err(AnalysisError::InvalidInput {
            analysis: "ast_paths",
            input: "max_path_length",
            ..
        })
    ));
}

fn permutation_fixture(
    leaf_count: usize,
    extra_edges: &[bool],
    reverse: bool,
) -> CodePropertyGraph {
    let mut nodes = vec![node(0, function("permuted"))];
    nodes.push(node(
        1,
        CpgNodeKind::BinaryOp {
            operator: "+".into(),
        },
    ));
    for leaf in 0..leaf_count {
        let id = leaf as u32 + 2;
        nodes.push(node(
            id,
            CpgNodeKind::Identifier {
                name: format!("v{leaf}").into(),
                definition: None,
            },
        ));
    }
    if reverse {
        nodes.reverse();
    }

    let mut edges = vec![(0, 0, 1, CpgEdgeKind::AstChild)];
    for leaf in 0..leaf_count {
        edges.push((leaf as u32 + 1, 1, leaf as u32 + 2, CpgEdgeKind::AstChild));
    }
    for (index, enabled) in extra_edges.iter().copied().enumerate() {
        if enabled && index + 1 < leaf_count {
            edges.push((
                10_000 + index as u32,
                index as u32 + 2,
                index as u32 + 3,
                if index % 2 == 0 {
                    CpgEdgeKind::DataDependence
                } else {
                    CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse)
                },
            ));
        }
    }
    if reverse {
        edges.reverse();
    }

    let mut cpg = CodePropertyGraph::new(Language::Rust);
    for item in nodes {
        cpg.add_node_with_id(item);
    }
    for (id, source, target, kind) in edges {
        add_edge(&mut cpg, id, source, target, kind);
    }
    cpg
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn exports_and_paths_are_insertion_order_invariant(
        leaf_count in 2usize..14,
        extra_edges in prop::collection::vec(any::<bool>(), 0..20),
    ) {
        let forward = permutation_fixture(leaf_count, &extra_edges, false);
        let reverse = permutation_fixture(leaf_count, &extra_edges, true);
        prop_assert_eq!(export_pyg(&forward), export_pyg(&reverse));
        let forward_heterogeneous = export_heterogeneous_gnn(&forward);
        let reverse_heterogeneous = export_heterogeneous_gnn(&reverse);
        prop_assert_eq!(&forward_heterogeneous, &reverse_heterogeneous);
        #[cfg(feature = "serde")]
        prop_assert_eq!(
            serde_json::to_vec(&forward_heterogeneous).unwrap(),
            serde_json::to_vec(&reverse_heterogeneous).unwrap(),
        );

        let options = AstPathOptions {
            max_path_width: leaf_count,
            max_terminals: leaf_count,
            max_pair_checks: leaf_count * leaf_count,
            max_contexts: leaf_count * leaf_count,
            max_total_path_nodes: leaf_count * leaf_count * 3,
            max_ancestor_steps: leaf_count * 3,
            ..AstPathOptions::default()
        };
        prop_assert_eq!(
            ast_paths(&forward, NodeId::new(0), options),
            ast_paths(&reverse, NodeId::new(0), options)
        );
    }
}

#[cfg(feature = "serde")]
#[test]
fn public_export_layers_round_trip_and_encode_deterministically() {
    let cpg = exact_fixture();
    let export = export_pyg(&cpg);
    let heterogeneous = export_heterogeneous_gnn(&cpg);
    let contexts = ast_paths(&cpg, NodeId::new(10), AstPathOptions::default()).unwrap();
    let export_bytes = serde_json::to_vec(&export).expect("serialize PyG export");
    let path_bytes = serde_json::to_vec(&contexts).expect("serialize path contexts");
    let heterogeneous_bytes =
        serde_json::to_vec(&heterogeneous).expect("serialize heterogeneous export");
    assert_eq!(
        serde_json::from_slice::<PygExport>(&export_bytes).unwrap(),
        export
    );
    assert_eq!(
        serde_json::from_slice::<Vec<PathContext>>(&path_bytes).unwrap(),
        contexts
    );
    assert_eq!(serde_json::to_vec(&export).unwrap(), export_bytes);
    assert_eq!(serde_json::to_vec(&contexts).unwrap(), path_bytes);
    assert_eq!(
        serde_json::from_slice::<HeteroGnnExport>(&heterogeneous_bytes).unwrap(),
        heterogeneous
    );
    assert_eq!(
        serde_json::to_vec(&heterogeneous).unwrap(),
        heterogeneous_bytes
    );
}
