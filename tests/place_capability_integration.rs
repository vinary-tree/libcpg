//! Public parser-backed, reconstruction, and deep-stack PCG gates.

use libcpg::{
    borrow_check, materialize_place_capability_overlay, place_capability_graph,
    BorrowViolationKind, CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind,
    DfgEdgeKind, Language, MethodSignature, NodeId, OwnershipOperationKind, ScopeId, SourceRange,
    TypeInfo, Visibility,
};

fn node(kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(NodeId::new(0), kind, SourceRange::default())
}

fn function_kind(name: &str) -> CpgNodeKind {
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

fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
    let child = cpg.add_node(node(kind));
    cpg.connect(parent, child, CpgEdgeKind::AstChild)
        .expect("integration AST endpoints exist");
    child
}

#[test]
#[cfg(feature = "lang-rust")]
fn parser_built_rust_flags_oracles_and_reconstructs_every_relation() {
    use libcpg::{CpgBuilder, PlaceCapabilityEdgeKind, TreeSitterCpgBuilder};

    let source = r#"
        fn consume_owned(_: String) {}
        fn consume_shared(_: &String) {}
        fn consume_mut(_: &mut String) {}

        fn move_after_use(source: String) {
            let moved: String = source;
            consume_owned(source);
            consume_owned(moved);
        }

        fn conflicting_borrows(mut value: String) {
            let shared: &String = &value;
            let exclusive: &mut String = &mut value;
            consume_shared(shared);
            consume_mut(exclusive);
        }

        fn well_borrowed(mut value: String) {
            let shared: &String = &value;
            consume_shared(shared);
            let exclusive: &mut String = &mut value;
            consume_mut(exclusive);
        }

        fn reborrow(target: &mut String) {
            let nested: &mut String = &mut *target;
            consume_mut(nested);
        }
    "#;
    let cpg = TreeSitterCpgBuilder::new()
        .build(source, Language::Rust)
        .expect("build parser-backed Rust ownership corpus");
    let function = |name: &str| {
        cpg.functions()
            .find_map(|node| match &node.kind {
                CpgNodeKind::Function { signature } if signature.name.as_ref() == name => {
                    Some(node.id)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("function {name} is represented"))
    };

    let moved = function("move_after_use");
    let move_report = place_capability_graph(&cpg, moved).expect("move PCG");
    assert!(move_report.is_complete());
    assert!(move_report
        .capability_edges
        .iter()
        .any(|edge| edge.kind == PlaceCapabilityEdgeKind::Move));
    let move_violations = borrow_check(&cpg, moved).expect("move borrow-check report");
    assert!(move_violations
        .iter()
        .any(|finding| finding.kind == BorrowViolationKind::UseAfterMove));

    let conflicting = function("conflicting_borrows");
    let conflict_report = place_capability_graph(&cpg, conflicting).expect("borrow PCG");
    assert!(conflict_report.capability_edges.iter().any(|edge| {
        edge.kind == PlaceCapabilityEdgeKind::SharedBorrow
            || edge.kind == PlaceCapabilityEdgeKind::SharedReborrow
    }));
    assert!(conflict_report.capability_edges.iter().any(|edge| {
        edge.kind == PlaceCapabilityEdgeKind::MutableBorrow
            || edge.kind == PlaceCapabilityEdgeKind::MutableReborrow
    }));
    assert!(borrow_check(&cpg, conflicting)
        .expect("conflicting borrow report")
        .iter()
        .any(|finding| finding.kind == BorrowViolationKind::ConflictingBorrow));

    assert!(borrow_check(&cpg, function("well_borrowed"))
        .expect("well-borrowed report")
        .is_empty());

    let reborrow = function("reborrow");
    let reborrow_report = place_capability_graph(&cpg, reborrow).expect("reborrow PCG");
    assert!(reborrow_report
        .capability_edges
        .iter()
        .any(|edge| edge.kind == PlaceCapabilityEdgeKind::MutableReborrow));

    for (report, scope) in [
        (&move_report, moved),
        (&conflict_report, conflicting),
        (&reborrow_report, reborrow),
    ] {
        for relation in &report.capability_edges {
            assert_eq!(relation.evidence.scope, scope);
            let data_flow = relation
                .evidence
                .data_flow
                .map(|(source, target)| {
                    cpg.edges_between(source, target).into_iter().any(|edge| {
                        matches!(
                            edge.kind,
                            CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse | DfgEdgeKind::ReachingDef)
                        )
                    })
                })
                .unwrap_or(false);
            let reference = relation
                .evidence
                .reference
                .map(|(source, target)| {
                    cpg.edges_between(source, target)
                        .into_iter()
                        .any(|edge| edge.kind == CpgEdgeKind::Reference)
                })
                .unwrap_or(false);
            assert!(
                data_flow || reference,
                "every relation has typed graph evidence"
            );
            assert!(
                cpg.ast_ancestors(relation.evidence.operand)
                    .contains(&relation.operation)
                    || relation.evidence.operand == relation.operation
            );
        }
    }
}

#[test]
fn depth_100k_pcg_analysis_materialization_and_drop_fit_a_64k_stack() {
    std::thread::Builder::new()
        .name("pcg-depth-100k".into())
        .stack_size(64 * 1024)
        .spawn(|| {
            const DEPTH: usize = 100_000;
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            let function = cpg.add_node(node(function_kind("deep_ownership")));
            let source = child(
                &mut cpg,
                function,
                CpgNodeKind::Variable {
                    name: "source".into(),
                    var_type: Some(Box::new(TypeInfo::new("String"))),
                    scope: ScopeId::GLOBAL,
                    is_mutable: false,
                },
            );
            let target = child(
                &mut cpg,
                function,
                CpgNodeKind::Variable {
                    name: "target".into(),
                    var_type: Some(Box::new(TypeInfo::new("String"))),
                    scope: ScopeId::GLOBAL,
                    is_mutable: false,
                },
            );
            let mut parent = target;
            for _ in 0..DEPTH {
                parent = child(
                    &mut cpg,
                    parent,
                    CpgNodeKind::Unknown {
                        kind: "ownership_wrapper".into(),
                    },
                );
            }
            let mut transfer = node(CpgNodeKind::Identifier {
                name: "source".into(),
                definition: None,
            });
            transfer.ownership_operation = Some(OwnershipOperationKind::Move);
            let operation = cpg.add_node(transfer);
            cpg.connect(parent, operation, CpgEdgeKind::AstChild)
                .expect("deep operation AST endpoint exists");
            let later_use = child(
                &mut cpg,
                function,
                CpgNodeKind::Identifier {
                    name: "source".into(),
                    definition: None,
                },
            );
            for use_site in [operation, later_use] {
                cpg.connect(source, use_site, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse))
                    .expect("deep DFG endpoints exist");
            }
            for (from, to) in [
                (function, source),
                (source, operation),
                (operation, later_use),
            ] {
                cpg.connect(from, to, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential))
                    .expect("deep CFG endpoints exist");
            }

            let pcg = place_capability_graph(&cpg, function).expect("deep PCG");
            assert_eq!(pcg.capability_edges.len(), 1);
            assert_eq!(pcg.capability_edges[0].target, target);
            assert_eq!(
                borrow_check(&cpg, function).expect("deep borrow report")[0].kind,
                BorrowViolationKind::UseAfterMove
            );
            assert_eq!(
                materialize_place_capability_overlay(&mut cpg, &pcg)
                    .expect("deep overlay endpoints exist"),
                1
            );
            assert_eq!(
                materialize_place_capability_overlay(&mut cpg, &pcg)
                    .expect("deep overlay remains idempotent"),
                0
            );
            drop(pcg);
            drop(cpg);
        })
        .expect("spawn small-stack PCG integration")
        .join()
        .expect("100k PCG analysis remains stack-safe");
}
