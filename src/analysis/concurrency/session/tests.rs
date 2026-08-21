use super::*;

use crate::{CpgEdgeKind, CpgNode, Language, MethodSignature, ScopeId, Visibility};
use proptest::prelude::*;

#[derive(Clone)]
struct ActionSpec {
    kind: MsgKind,
    payloads: Vec<SessionPayloadType>,
}

fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
    CpgNode::new(
        NodeId::new(id),
        kind,
        SourceRange::from_bytes(id * 2, id * 2 + 1),
    )
}

fn function(id: u32, name: &str) -> CpgNode {
    node(
        id,
        CpgNodeKind::Function {
            signature: Box::new(MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Public,
            }),
        },
    )
}

fn block(id: u32) -> CpgNode {
    node(
        id,
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
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

fn channel_identifier(id: u32) -> CpgNode {
    node(
        id,
        CpgNodeKind::Identifier {
            name: "channel".into(),
            definition: Some(NodeId::new(1)),
        },
    )
}

fn payload_node(id: u32, payload: &SessionPayloadType) -> CpgNode {
    let kind = match payload {
        SessionPayloadType::Integer => CpgNodeKind::Literal {
            kind: LiteralKind::Integer(7),
        },
        SessionPayloadType::Float => CpgNodeKind::Literal {
            kind: LiteralKind::Float(1.5),
        },
        SessionPayloadType::String => CpgNodeKind::Literal {
            kind: LiteralKind::String("payload".into()),
        },
        SessionPayloadType::Character => CpgNodeKind::Literal {
            kind: LiteralKind::Char('p'),
        },
        SessionPayloadType::Boolean => CpgNodeKind::Literal {
            kind: LiteralKind::Bool(true),
        },
        SessionPayloadType::Null => CpgNodeKind::Literal {
            kind: LiteralKind::Null,
        },
        SessionPayloadType::Array => CpgNodeKind::Literal {
            kind: LiteralKind::Array,
        },
        SessionPayloadType::Object => CpgNodeKind::Literal {
            kind: LiteralKind::Object,
        },
        SessionPayloadType::Regex => CpgNodeKind::Literal {
            kind: LiteralKind::Regex(".*".into()),
        },
        SessionPayloadType::Named(name) => CpgNodeKind::Parameter {
            name: "payload".into(),
            param_type: Some(Box::new(TypeInfo::new(name.clone()))),
            is_variadic: false,
        },
        SessionPayloadType::Unknown => CpgNodeKind::Parameter {
            name: "payload".into(),
            param_type: None,
            is_variadic: false,
        },
    };
    node(id, kind)
}

fn add_endpoint(
    nodes: &mut Vec<CpgNode>,
    edges: &mut Vec<(u32, u32)>,
    function_id: u32,
    block_id: u32,
    event_base: u32,
    specs: &[ActionSpec],
) {
    nodes.push(function(function_id, &format!("p{function_id}")));
    nodes.push(block(block_id));
    edges.push((function_id, block_id));

    for (action_index, spec) in specs.iter().enumerate() {
        let event_id = event_base + u32::try_from(action_index).expect("small fixture") * 20;
        nodes.push(event(event_id, spec.kind));
        nodes.push(channel_identifier(event_id + 1));
        edges.push((block_id, event_id));
        edges.push((event_id, event_id + 1));
        for (payload_index, payload) in spec.payloads.iter().enumerate() {
            let payload_id =
                event_id + 2 + u32::try_from(payload_index).expect("small fixture payload count");
            nodes.push(payload_node(payload_id, payload));
            edges.push((event_id, payload_id));
        }
    }
}

fn fixture(
    left: &[ActionSpec],
    right: &[ActionSpec],
    reverse_nodes: bool,
    reverse_edges: bool,
) -> CodePropertyGraph {
    let mut nodes = vec![node(
        1,
        CpgNodeKind::Variable {
            name: "channel".into(),
            var_type: None,
            scope: ScopeId::GLOBAL,
            is_mutable: false,
        },
    )];
    let mut edges = Vec::new();
    add_endpoint(&mut nodes, &mut edges, 10, 11, 100, left);
    add_endpoint(&mut nodes, &mut edges, 20, 21, 1_000, right);
    if reverse_nodes {
        nodes.reverse();
    }
    if reverse_edges {
        edges.reverse();
    }

    let mut cpg = CodePropertyGraph::new(Language::Rholang);
    for node in nodes {
        cpg.add_node_with_id(node);
    }
    for (parent, child) in edges {
        cpg.connect(
            NodeId::new(parent),
            NodeId::new(child),
            CpgEdgeKind::AstChild,
        );
    }
    cpg
}

fn action(kind: MsgKind, payloads: &[SessionPayloadType]) -> ActionSpec {
    ActionSpec {
        kind,
        payloads: payloads.to_vec(),
    }
}

#[test]
fn dual_linear_endpoints_with_matching_types_are_conformant_and_read_only() {
    let left = [
        action(MsgKind::Send, &[SessionPayloadType::Integer]),
        action(MsgKind::Recv, &[SessionPayloadType::String]),
    ];
    let right = [
        action(MsgKind::Recv, &[SessionPayloadType::Integer]),
        action(MsgKind::SendSync, &[SessionPayloadType::String]),
    ];
    let cpg = fixture(&left, &right, false, false);
    let before = (cpg.node_count(), cpg.edge_count());

    let sketches = session_sketches(&cpg);

    assert_eq!(sketches.len(), 1);
    assert_eq!(sketches[0].channel, NodeId::new(1));
    assert_eq!(sketches[0].conformance, SessionConformance::Conformant);
    assert!(sketches[0].issues.is_empty());
    assert_eq!(
        sketches[0]
            .endpoints
            .iter()
            .map(|endpoint| endpoint.process)
            .collect::<Vec<_>>(),
        vec![NodeId::new(10), NodeId::new(20)]
    );
    assert_eq!(before, (cpg.node_count(), cpg.edge_count()));
}

#[test]
fn concrete_order_arity_type_and_length_mismatches_are_all_reported() {
    let left = [
        action(MsgKind::Send, &[SessionPayloadType::Integer]),
        action(
            MsgKind::Send,
            &[SessionPayloadType::Boolean, SessionPayloadType::String],
        ),
        action(MsgKind::Recv, &[SessionPayloadType::Integer]),
    ];
    let right = [
        action(MsgKind::Send, &[SessionPayloadType::String]),
        action(MsgKind::Recv, &[SessionPayloadType::Boolean]),
    ];

    let sketch = session_sketches(&fixture(&left, &right, false, false))
        .pop()
        .expect("channel sketch");
    assert_eq!(sketch.conformance, SessionConformance::Nonconformant);
    let kinds = sketch
        .issues
        .iter()
        .map(|issue| issue.kind)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        kinds,
        BTreeSet::from([
            SessionIssueKind::LengthMismatch,
            SessionIssueKind::DirectionMismatch,
            SessionIssueKind::PayloadArityMismatch,
            SessionIssueKind::PayloadTypeMismatch,
        ])
    );
}

#[test]
fn unknown_types_and_non_linear_receives_are_inconclusive() {
    let left = [action(MsgKind::Send, &[SessionPayloadType::Unknown])];
    let right = [action(
        MsgKind::RecvPersistent,
        &[SessionPayloadType::Unknown],
    )];

    let sketch = session_sketches(&fixture(&left, &right, false, false))
        .pop()
        .expect("channel sketch");
    assert_eq!(sketch.conformance, SessionConformance::Inconclusive);
    assert_eq!(
        sketch
            .issues
            .iter()
            .map(|issue| issue.kind)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            SessionIssueKind::UnknownPayloadType,
            SessionIssueKind::NonLinearReceive,
        ])
    );
}

#[test]
fn a_single_process_is_not_misrepresented_as_a_binary_session() {
    let left = [action(MsgKind::Send, &[SessionPayloadType::Integer])];
    let cpg = fixture(&left, &[], false, false);

    let sketch = session_sketches(&cpg).pop().expect("channel sketch");
    assert_eq!(sketch.endpoints.len(), 1);
    assert_eq!(sketch.conformance, SessionConformance::Inconclusive);
    assert_eq!(sketch.issues[0].kind, SessionIssueKind::EndpointCardinality);
    assert_eq!(sketch.issues[0].processes, vec![NodeId::new(10)]);
    assert!(sketch.issues[0]
        .sites
        .iter()
        .all(|&site| cpg.contains_node(site)));
}

#[test]
fn a_shared_three_process_channel_is_explicitly_inconclusive() {
    let send = [action(MsgKind::Send, &[SessionPayloadType::Integer])];
    let recv = [action(MsgKind::Recv, &[SessionPayloadType::Integer])];
    let mut cpg = fixture(&send, &recv, false, false);
    for item in [
        function(30, "p30"),
        block(31),
        event(3_000, MsgKind::Recv),
        channel_identifier(3_001),
        payload_node(3_002, &SessionPayloadType::Integer),
    ] {
        cpg.add_node_with_id(item);
    }
    for (parent, child) in [(30, 31), (31, 3_000), (3_000, 3_001), (3_000, 3_002)] {
        cpg.connect(
            NodeId::new(parent),
            NodeId::new(child),
            CpgEdgeKind::AstChild,
        );
    }

    let sketch = session_sketches(&cpg).pop().expect("channel sketch");
    assert_eq!(sketch.endpoints.len(), 3);
    assert_eq!(sketch.conformance, SessionConformance::Inconclusive);
    assert_eq!(sketch.issues[0].kind, SessionIssueKind::EndpointCardinality);
}

#[test]
fn every_literal_category_and_custom_named_type_is_preserved() {
    let payloads = vec![
        SessionPayloadType::Integer,
        SessionPayloadType::Float,
        SessionPayloadType::String,
        SessionPayloadType::Character,
        SessionPayloadType::Boolean,
        SessionPayloadType::Null,
        SessionPayloadType::Array,
        SessionPayloadType::Object,
        SessionPayloadType::Regex,
        SessionPayloadType::Named("Account".into()),
    ];
    let left = [action(MsgKind::Send, &payloads)];
    let cpg = fixture(&left, &[], false, false);

    let sketch = session_sketches(&cpg).pop().expect("channel sketch");
    assert_eq!(sketch.endpoints[0].actions[0].payload_types, payloads);
}

#[test]
fn typed_identifier_payload_uses_definition_evidence() {
    let left = [action(MsgKind::Send, &[SessionPayloadType::Unknown])];
    let mut cpg = fixture(&left, &[], false, false);
    let payload = NodeId::new(102);
    cpg.node_mut(payload).expect("payload").kind = CpgNodeKind::Identifier {
        name: "account".into(),
        definition: Some(NodeId::new(50)),
    };
    cpg.add_node_with_id(node(
        50,
        CpgNodeKind::Variable {
            name: "account".into(),
            var_type: Some(Box::new(TypeInfo::new("Account"))),
            scope: ScopeId::GLOBAL,
            is_mutable: false,
        },
    ));

    let sketch = session_sketches(&cpg).pop().expect("channel sketch");
    assert_eq!(
        sketch.endpoints[0].actions[0].payload_types,
        vec![SessionPayloadType::Named("Account".into())]
    );
}

#[test]
fn generic_and_reference_qualifiers_remain_part_of_named_types() {
    assert_eq!(
        type_info_payload_type(&TypeInfo::new("List").with_generic("Int")),
        SessionPayloadType::Named("List<Int>".into())
    );
    assert_eq!(
        type_info_payload_type(
            &TypeInfo::new("Account")
                .with_reference(true)
                .with_mutable(true)
        ),
        SessionPayloadType::Named("&mut Account".into())
    );
}

#[test]
fn nested_events_and_ast_cycles_do_not_leak_payloads_or_loop() {
    let left = [action(MsgKind::Send, &[])];
    let mut cpg = fixture(&left, &[], false, false);
    cpg.add_node_with_id(event(102, MsgKind::Recv));
    cpg.add_node_with_id(node(
        103,
        CpgNodeKind::Literal {
            kind: LiteralKind::Integer(9),
        },
    ));
    cpg.connect(NodeId::new(100), NodeId::new(102), CpgEdgeKind::AstChild);
    cpg.connect(NodeId::new(102), NodeId::new(103), CpgEdgeKind::AstChild);
    cpg.connect(NodeId::new(103), NodeId::new(100), CpgEdgeKind::AstChild);

    let sketch = session_sketches(&cpg).pop().expect("channel sketch");
    assert!(sketch.endpoints[0].actions[0].payload_types.is_empty());
}

#[cfg(feature = "serde")]
#[test]
fn session_sketch_json_round_trip_preserves_public_evidence() {
    let left = [action(MsgKind::Send, &[SessionPayloadType::Integer])];
    let right = [action(MsgKind::Recv, &[SessionPayloadType::Integer])];
    let sketches = session_sketches(&fixture(&left, &right, false, false));

    let json = serde_json::to_string(&sketches).expect("serialize sketches");
    let decoded: Vec<SessionSketch> = serde_json::from_str(&json).expect("deserialize sketches");
    assert_eq!(decoded, sketches);
}

fn payload_from_tag(tag: u8) -> SessionPayloadType {
    match tag % 4 {
        0 => SessionPayloadType::Integer,
        1 => SessionPayloadType::String,
        2 => SessionPayloadType::Boolean,
        _ => SessionPayloadType::Named("Account".into()),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(128))]

    #[test]
    fn exact_duals_are_deterministic_across_node_and_edge_insertion_order(
        raw in prop::collection::vec((any::<bool>(), 0_u8..16), 1..8)
    ) {
        let left = raw
            .iter()
            .map(|(send, tag)| action(
                if *send { MsgKind::Send } else { MsgKind::Recv },
                &[payload_from_tag(*tag)],
            ))
            .collect::<Vec<_>>();
        let right = raw
            .iter()
            .map(|(send, tag)| action(
                if *send { MsgKind::Recv } else { MsgKind::SendSync },
                &[payload_from_tag(*tag)],
            ))
            .collect::<Vec<_>>();

        let forward_cpg = fixture(&left, &right, false, false);
        let reversed_cpg = fixture(&left, &right, true, true);
        let forward = session_sketches(&forward_cpg);
        let reversed = session_sketches(&reversed_cpg);
        prop_assert_eq!(&forward, &reversed);
        prop_assert_eq!(forward.len(), 1);
        prop_assert_eq!(forward[0].conformance, SessionConformance::Conformant);
        prop_assert!(forward[0].issues.is_empty());
        prop_assert!(forward[0]
            .endpoints
            .iter()
            .all(|endpoint| cpg_sites_exist(&forward_cpg, endpoint)));
    }
}

fn cpg_sites_exist(cpg: &CodePropertyGraph, endpoint: &SessionEndpoint) -> bool {
    cpg.contains_node(endpoint.process)
        && endpoint
            .actions
            .iter()
            .all(|action| cpg.contains_node(action.site))
}
