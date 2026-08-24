use super::*;

use crate::{CpgNode, DfgEdgeKind, Language, MethodSignature, ScopeId, Visibility};
use proptest::prelude::*;

fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(
        NodeId::new(id),
        kind,
        SourceRange::from_bytes(id, id.saturating_add(1)),
    )
}

fn restriction(id: u32) -> CpgNode {
    node(
        id,
        CpgNodeKind::Block {
            scope: ScopeId::new(id),
        },
    )
    .with_name_operation(NameOperationKind::Restriction)
}

fn fresh(id: u32, name: &str) -> CpgNode {
    node(
        id,
        CpgNodeKind::Variable {
            name: name.into(),
            var_type: None,
            scope: ScopeId::new(id),
            is_mutable: false,
        },
    )
    .with_name_operation(NameOperationKind::FreshBinder)
}

fn function(id: u32) -> CpgNode {
    node(
        id,
        CpgNodeKind::Function {
            signature: Box::new(MethodSignature {
                name: format!("p{id}").into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Public,
            }),
        },
    )
}

fn event(id: u32, kind: MsgKind) -> CpgNode {
    node(
        id,
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        },
    )
    .with_message_kind(kind)
}

fn identifier(id: u32, definition: u32) -> CpgNode {
    node(
        id,
        CpgNodeKind::Identifier {
            name: format!("n{definition}").into(),
            definition: Some(NodeId::new(definition)),
        },
    )
}

fn parameter(id: u32, name: &str) -> CpgNode {
    node(
        id,
        CpgNodeKind::Parameter {
            name: name.into(),
            param_type: None,
            is_variadic: false,
        },
    )
}

fn connect_ast(cpg: &mut CodePropertyGraph, parent: u32, child: u32) {
    cpg.connect(
        NodeId::new(parent),
        NodeId::new(child),
        CpgEdgeKind::AstChild,
    );
}

#[test]
fn direct_quote_drop_and_overlay_are_typed_and_idempotent() {
    let mut cpg = CodePropertyGraph::new(Language::Rholang);
    for value in [
        restriction(1),
        fresh(2, "fresh"),
        node(
            3,
            CpgNodeKind::UnaryOp {
                operator: "@".into(),
            },
        )
        .with_name_operation(NameOperationKind::Quote),
        identifier(4, 2),
        node(
            5,
            CpgNodeKind::UnaryOp {
                operator: "*".into(),
            },
        )
        .with_name_operation(NameOperationKind::Drop),
        identifier(6, 2),
    ] {
        cpg.add_node_with_id(value);
    }
    for (parent, child) in [(1, 2), (1, 3), (3, 4), (1, 5), (5, 6)] {
        connect_ast(&mut cpg, parent, child);
    }

    let analysis = analyze_name_flow(&cpg);
    assert!(analysis.complete, "issues={:#?}", analysis.issues);
    for relation in [
        NameFlowRelation {
            origin: NodeId::new(2),
            target: NodeId::new(1),
            kind: NameFlowEdgeKind::Creates,
        },
        NameFlowRelation {
            origin: NodeId::new(2),
            target: NodeId::new(3),
            kind: NameFlowEdgeKind::Quotes,
        },
        NameFlowRelation {
            origin: NodeId::new(2),
            target: NodeId::new(5),
            kind: NameFlowEdgeKind::Drops,
        },
    ] {
        assert!(
            analysis.relations.contains(&relation),
            "missing {relation:?}"
        );
    }

    let nodes_before = cpg.node_count();
    let added = materialize_name_flow_overlay(&mut cpg, &analysis).expect("valid overlay");
    assert_eq!(added, analysis.relations.len());
    assert_eq!(cpg.stats().name_flow_edges, added);
    assert_eq!(cpg.node_count(), nodes_before);
    assert_eq!(
        materialize_name_flow_overlay(&mut cpg, &analysis).expect("idempotent overlay"),
        0
    );
    assert!(cpg
        .name_flow_successors(NodeId::new(2))
        .all(|edge| edge.kind.is_name_flow()));
}

#[test]
fn payload_scope_extrusion_reaches_receive_binder_and_later_use() {
    let mut cpg = CodePropertyGraph::new(Language::Rholang);
    for value in [
        restriction(1),
        fresh(10, "carrier"),
        fresh(20, "payload"),
        function(30),
        function(40),
        event(100, MsgKind::Send),
        identifier(101, 10),
        identifier(102, 20),
        event(200, MsgKind::Recv),
        identifier(201, 10),
        parameter(202, "received"),
        identifier(203, 202),
    ] {
        cpg.add_node_with_id(value);
    }
    for (parent, child) in [
        (1, 10),
        (1, 20),
        (1, 30),
        (1, 40),
        (30, 100),
        (100, 101),
        (100, 102),
        (40, 200),
        (200, 201),
        (200, 202),
        (200, 203),
    ] {
        connect_ast(&mut cpg, parent, child);
    }

    let analysis = analyze_name_flow(&cpg);
    assert!(analysis.complete, "issues={:#?}", analysis.issues);
    assert_eq!(analysis.communication_matches, 1);
    assert_eq!(analysis.origins_of(NodeId::new(202)), &[NodeId::new(20)]);
    assert_eq!(analysis.origins_of(NodeId::new(203)), &[NodeId::new(20)]);
    assert!(analysis.relations.contains(&NameFlowRelation {
        origin: NodeId::new(20),
        target: NodeId::new(202),
        kind: NameFlowEdgeKind::Receives,
    }));
}

fn wait_cycle(reverse_edges: bool) -> CodePropertyGraph {
    let mut cpg = CodePropertyGraph::new(Language::Rholang);
    for value in [
        restriction(1),
        fresh(10, "left"),
        fresh(20, "right"),
        function(30),
        function(40),
        event(100, MsgKind::Recv),
        identifier(101, 10),
        event(110, MsgKind::Send),
        identifier(111, 20),
        event(200, MsgKind::Recv),
        identifier(201, 20),
        event(210, MsgKind::Send),
        identifier(211, 10),
    ] {
        cpg.add_node_with_id(value);
    }
    let mut edges = vec![
        (1, 10),
        (1, 20),
        (1, 30),
        (1, 40),
        (30, 100),
        (100, 101),
        (30, 110),
        (110, 111),
        (40, 200),
        (200, 201),
        (40, 210),
        (210, 211),
    ];
    if reverse_edges {
        edges.reverse();
    }
    for (parent, child) in edges {
        connect_ast(&mut cpg, parent, child);
    }
    cpg
}

#[test]
fn mutual_wait_is_an_unsatisfiable_level_cycle_via_shared_scc() {
    let analysis = analyze_deadlock_freedom(&wait_cycle(false));
    assert!(analysis.complete, "issues={:#?}", analysis.issues);
    assert_eq!(
        analysis.cyclic_channels,
        vec![NodeId::new(10), NodeId::new(20)]
    );
    assert_eq!(analysis.witnesses.len(), 2);
    assert!(analysis
        .levels
        .iter()
        .all(|level| level.capability.is_none() && level.obligation.is_none()));
    assert!(analysis
        .witnesses
        .iter()
        .all(|witness| matches!(witness, DeadlockWitness::ObligationCapability { .. })));
}

#[test]
fn send_first_protocol_has_finite_levels_and_no_witness() {
    let mut cpg = wait_cycle(false);
    cpg.node_mut(NodeId::new(100))
        .expect("left first event")
        .message_kind = Some(MsgKind::Send);
    cpg.node_mut(NodeId::new(110))
        .expect("left second event")
        .message_kind = Some(MsgKind::Recv);
    cpg.node_mut(NodeId::new(200))
        .expect("right first event")
        .message_kind = Some(MsgKind::Send);
    cpg.node_mut(NodeId::new(210))
        .expect("right second event")
        .message_kind = Some(MsgKind::Recv);

    let analysis = analyze_deadlock_freedom(&cpg);
    assert!(analysis.complete, "issues={:#?}", analysis.issues);
    assert!(analysis.witnesses.is_empty());
    assert!(analysis.cyclic_channels.is_empty());
    assert!(analysis
        .levels
        .iter()
        .all(|level| level.capability == Some(0) && level.obligation == Some(0)));
}

#[test]
fn caps_and_malformed_definition_evidence_are_explicit() {
    let cpg = wait_cycle(false);
    let capped = analyze_name_flow_with_caps(
        &cpg,
        NameFlowCaps {
            max_origin_assignments: 0,
            ..NameFlowCaps::default()
        },
    );
    assert!(!capped.complete);
    assert!(capped
        .issues
        .contains(&NameFlowIssue::OriginAssignmentLimitExceeded { limit: 0 }));

    let mut ambiguous = cpg;
    ambiguous.connect(NodeId::new(101), NodeId::new(20), CpgEdgeKind::Reference);
    let analysis = analyze_name_flow(&ambiguous);
    assert!(!analysis.complete);
    assert!(analysis.issues.iter().any(|issue| matches!(
        issue,
        NameFlowIssue::AmbiguousDefinition { node, .. } if *node == NodeId::new(101)
    )));
}

#[test]
fn invalid_overlay_is_transactional() {
    let cpg = wait_cycle(false);
    let mut analysis = analyze_name_flow(&cpg);
    analysis.relations.push(NameFlowRelation {
        origin: NodeId::new(10),
        target: NodeId::new(u32::MAX),
        kind: NameFlowEdgeKind::References,
    });
    let mut target = cpg;
    let before = target.edge_count();
    assert_eq!(
        materialize_name_flow_overlay(&mut target, &analysis),
        Err(NameFlowOverlayError::UnknownTarget {
            target: NodeId::new(u32::MAX)
        })
    );
    assert_eq!(target.edge_count(), before);
}

#[test]
fn typed_dfg_definition_evidence_is_accepted_without_spelling() {
    let mut cpg = wait_cycle(false);
    let identifier = cpg.node_mut(NodeId::new(101)).expect("channel identifier");
    if let CpgNodeKind::Identifier { definition, name } = &mut identifier.kind {
        *definition = None;
        *name = "unrelated_alias".into();
    }
    cpg.connect(
        NodeId::new(10),
        NodeId::new(101),
        CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
    );
    let analysis = analyze_name_flow(&cpg);
    assert!(analysis.complete, "issues={:#?}", analysis.issues);
    assert_eq!(analysis.origins_of(NodeId::new(101)), &[NodeId::new(10)]);
}

#[cfg(feature = "serde")]
#[test]
fn serde_round_trip_is_canonical() {
    let analysis = analyze_deadlock_freedom(&wait_cycle(false));
    let encoded = serde_json::to_vec(&analysis).expect("serialize deadlock analysis");
    let decoded: DeadlockFreedomAnalysis =
        serde_json::from_slice(&encoded).expect("deserialize deadlock analysis");
    assert_eq!(decoded, analysis);
    assert_eq!(
        serde_json::to_vec(&decoded).expect("reserialize deadlock analysis"),
        encoded
    );
}

proptest! {
    #[test]
    fn edge_insertion_permutation_is_deterministic(reverse in any::<bool>()) {
        let forward = wait_cycle(false);
        let permuted = wait_cycle(reverse);
        prop_assert_eq!(analyze_name_flow(&forward), analyze_name_flow(&permuted));
        prop_assert_eq!(
            analyze_deadlock_freedom(&forward),
            analyze_deadlock_freedom(&permuted),
        );
    }

    #[test]
    fn acyclic_dependency_chain_has_strictly_increasing_levels(length in 2u32..12) {
        let mut cpg = CodePropertyGraph::new(Language::Rholang);
        cpg.add_node_with_id(restriction(1));
        for index in 0..length {
            let channel = 10 + index;
            cpg.add_node_with_id(fresh(channel, &format!("c{index}")));
            connect_ast(&mut cpg, 1, channel);
        }
        for index in 0..(length - 1) {
            let process = 1_000 + index * 100;
            let receive = process + 10;
            let send = process + 20;
            cpg.add_node_with_id(function(process));
            cpg.add_node_with_id(event(receive, MsgKind::Recv));
            cpg.add_node_with_id(identifier(receive + 1, 10 + index));
            cpg.add_node_with_id(event(send, MsgKind::Send));
            cpg.add_node_with_id(identifier(send + 1, 11 + index));
            connect_ast(&mut cpg, 1, process);
            connect_ast(&mut cpg, process, receive);
            connect_ast(&mut cpg, receive, receive + 1);
            connect_ast(&mut cpg, process, send);
            connect_ast(&mut cpg, send, send + 1);
        }

        let analysis = analyze_deadlock_freedom(&cpg);
        prop_assert!(analysis.complete, "issues={:#?}", analysis.issues);
        prop_assert!(analysis.witnesses.is_empty());
        for pair in analysis.levels.windows(2) {
            prop_assert!(pair[0].capability < pair[1].capability);
            prop_assert_eq!(pair[0].capability, pair[0].obligation);
        }
    }
}
