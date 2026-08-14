//! Channel-event extraction from normalized CPG evidence.

use std::collections::{BTreeMap, BTreeSet};

use rustc_hash::FxHashSet;

use crate::{CodePropertyGraph, CpgEdgeKind, CpgNodeKind, MsgKind, NodeId, SourceRange};

use super::super::function_scope_nodes;

/// Stable identity of a channel.
///
/// A channel is identified by the [`CpgNodeKind::Variable`] created for its
/// binding, rather than by a spelling that can be shadowed or reused.
pub type ChannelId = NodeId;

/// One normalized message-passing event in a process body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ChannelEvent {
    /// Message-passing semantics assigned by the language frontend.
    pub kind: MsgKind,
    /// Resolved channel definition, or `None` when the graph lacks sufficient
    /// def-use/reference evidence or the event has no channel (select/spawn).
    pub channel: Option<ChannelId>,
    /// Statement-precise CPG node representing this event.
    pub site: NodeId,
    /// Source range copied from `site`.
    pub range: SourceRange,
}

/// Extracts ordered channel events for every top-level function/process.
///
/// The outer [`BTreeMap`] is ordered by process [`NodeId`], and every event
/// vector is ordered by event-site [`NodeId`]. Nested functions are independent
/// process definitions and are not folded into their enclosing process.
/// Events whose channel identity is unresolved are retained with `channel =
/// None`; incomplete evidence is never converted into a guessed name match.
pub fn extract_events(cpg: &CodePropertyGraph) -> BTreeMap<NodeId, Vec<ChannelEvent>> {
    let mut processes: Vec<_> = cpg
        .functions()
        .map(|node| node.id)
        .filter(|&function| !has_function_ancestor(cpg, function))
        .collect();
    processes.sort_unstable();
    processes.dedup();

    let mut result = BTreeMap::new();
    for process in processes {
        let mut sites: Vec<_> = function_scope_nodes(cpg, process)
            .into_iter()
            .filter(|&node| {
                cpg.node(node)
                    .is_some_and(|node| node.message_kind.is_some())
            })
            .collect();
        sites.sort_unstable();

        let events = sites
            .into_iter()
            .filter_map(|site| {
                let node = cpg.node(site)?;
                let kind = node.message_kind?;
                let channel = kind
                    .uses_channel()
                    .then(|| resolve_event_variable(cpg, site))
                    .flatten();
                Some(ChannelEvent {
                    kind,
                    channel,
                    site,
                    range: node.range,
                })
            })
            .collect();
        result.insert(process, events);
    }
    result
}

fn has_function_ancestor(cpg: &CodePropertyGraph, node: NodeId) -> bool {
    let mut visited = FxHashSet::default();
    let mut stack: Vec<_> = cpg
        .incoming_edges(node)
        .filter_map(|edge| (edge.kind == CpgEdgeKind::AstChild).then_some(edge.source))
        .collect();
    while let Some(parent) = stack.pop() {
        if !visited.insert(parent) {
            continue;
        }
        if cpg
            .node(parent)
            .is_some_and(|node| matches!(node.kind, CpgNodeKind::Function { .. }))
        {
            return true;
        }
        stack.extend(
            cpg.incoming_edges(parent)
                .filter_map(|edge| (edge.kind == CpgEdgeKind::AstChild).then_some(edge.source)),
        );
    }
    false
}

/// Resolves only identifiers within this event's own syntax subtree. A nested
/// channel event starts a new statement boundary and is deliberately skipped.
pub(crate) fn resolve_event_variable(cpg: &CodePropertyGraph, site: NodeId) -> Option<NodeId> {
    let mut visited = FxHashSet::default();
    let mut stack = cpg.ast_children(site);
    stack.reverse();
    let mut candidates = Vec::new();

    while let Some(candidate) = stack.pop() {
        if !visited.insert(candidate) {
            continue;
        }
        let Some(node) = cpg.node(candidate) else {
            continue;
        };
        if node.message_kind.is_some() || node.lock_kind.is_some() {
            continue;
        }
        if matches!(node.kind, CpgNodeKind::Identifier { .. }) {
            for definition in variable_definitions(cpg, candidate) {
                candidates.push((node.range.start, node.range.end, candidate, definition));
            }
        }
        let mut children = cpg.ast_children(candidate);
        children.reverse();
        stack.extend(children);
    }

    candidates.sort_unstable();
    candidates.first().map(|candidate| candidate.3)
}

fn variable_definitions(cpg: &CodePropertyGraph, identifier: NodeId) -> Vec<NodeId> {
    let mut definitions = BTreeSet::new();
    if let Some(CpgNodeKind::Identifier {
        definition: Some(definition),
        ..
    }) = cpg.node(identifier).map(|node| &node.kind)
    {
        definitions.insert(*definition);
    }
    definitions.extend(cpg.reaching_definitions(identifier));
    for edge in cpg.outgoing_edges(identifier) {
        if edge.kind == CpgEdgeKind::Reference {
            definitions.insert(edge.target);
        }
    }
    for edge in cpg.incoming_edges(identifier) {
        if edge.kind == CpgEdgeKind::Reference {
            definitions.insert(edge.source);
        }
    }
    definitions
        .into_iter()
        .filter(|&definition| {
            cpg.node(definition)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Variable { .. }))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DfgEdgeKind, Language, MethodSignature, ScopeId, Visibility};
    use proptest::prelude::*;

    fn node(id: u32, kind: CpgNodeKind) -> crate::CpgNode {
        crate::CpgNode::new(
            NodeId::new(id),
            kind,
            SourceRange::from_bytes(id * 2, id * 2 + 1),
        )
    }

    fn function(id: u32, name: &str) -> crate::CpgNode {
        node(
            id,
            CpgNodeKind::Function {
                signature: MethodSignature {
                    name: name.into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Public,
                },
            },
        )
    }

    fn variable(id: u32, name: &str) -> crate::CpgNode {
        node(
            id,
            CpgNodeKind::Variable {
                name: name.into(),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: false,
            },
        )
    }

    fn ident(id: u32, name: &str) -> crate::CpgNode {
        node(
            id,
            CpgNodeKind::Identifier {
                name: name.into(),
                definition: None,
            },
        )
    }

    fn event(id: u32, kind: MsgKind) -> crate::CpgNode {
        node(
            id,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        )
        .with_message_kind(kind)
    }

    fn connect_ast(cpg: &mut CodePropertyGraph, parent: u32, child: u32) {
        cpg.connect(
            NodeId::new(parent),
            NodeId::new(child),
            CpgEdgeKind::AstChild,
        );
    }

    fn extraction_fixture(reverse_edges: bool) -> CodePropertyGraph {
        let mut cpg = CodePropertyGraph::new(Language::Rholang);
        for n in [
            function(10, "p"),
            node(
                11,
                CpgNodeKind::Block {
                    scope: ScopeId::GLOBAL,
                },
            ),
            variable(20, "c"),
            event(30, MsgKind::Send),
            ident(31, "c"),
            event(40, MsgKind::Recv),
            node(
                41,
                CpgNodeKind::Parameter {
                    name: "payload".into(),
                    param_type: None,
                    is_variadic: false,
                },
            ),
            ident(42, "c"),
            event(50, MsgKind::Spawn),
        ] {
            cpg.add_node_with_id(n);
        }
        let mut edges = vec![
            (10, 11, CpgEdgeKind::AstChild),
            (11, 20, CpgEdgeKind::AstChild),
            (11, 30, CpgEdgeKind::AstChild),
            (30, 31, CpgEdgeKind::AstChild),
            (11, 40, CpgEdgeKind::AstChild),
            (40, 41, CpgEdgeKind::AstChild),
            (40, 42, CpgEdgeKind::AstChild),
            (11, 50, CpgEdgeKind::AstChild),
            (20, 31, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse)),
            (42, 20, CpgEdgeKind::Reference),
        ];
        if reverse_edges {
            edges.reverse();
        }
        for (source, target, kind) in edges {
            cpg.connect(NodeId::new(source), NodeId::new(target), kind);
        }
        cpg
    }

    #[test]
    fn extracts_statement_precise_events_from_defuse_and_reference_evidence() {
        let cpg = extraction_fixture(false);
        let events = extract_events(&cpg);
        let process_events = &events[&NodeId::new(10)];
        assert_eq!(
            process_events
                .iter()
                .map(|event| (event.kind, event.channel, event.site))
                .collect::<Vec<_>>(),
            vec![
                (MsgKind::Send, Some(NodeId::new(20)), NodeId::new(30)),
                (MsgKind::Recv, Some(NodeId::new(20)), NodeId::new(40)),
                (MsgKind::Spawn, None, NodeId::new(50)),
            ]
        );
        assert_eq!(process_events[0].range, SourceRange::from_bytes(60, 61));
    }

    #[test]
    fn retains_unresolved_events_without_guessing_by_identifier_name() {
        let mut cpg = extraction_fixture(false);
        cpg.add_node_with_id(event(60, MsgKind::Peek));
        cpg.add_node_with_id(ident(61, "c"));
        connect_ast(&mut cpg, 11, 60);
        connect_ast(&mut cpg, 60, 61);
        let events = extract_events(&cpg);
        assert_eq!(events[&NodeId::new(10)].last().expect("peek").channel, None);
    }

    #[test]
    fn earliest_typed_channel_operand_wins_over_later_payload_uses() {
        let mut cpg = extraction_fixture(false);
        cpg.add_node_with_id(variable(60, "payload_channel"));
        cpg.add_node_with_id(ident(32, "payload_channel"));
        connect_ast(&mut cpg, 30, 32);
        cpg.connect(
            NodeId::new(60),
            NodeId::new(32),
            CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
        );

        let events = extract_events(&cpg);
        assert_eq!(
            events[&NodeId::new(10)][0].channel,
            Some(NodeId::new(20)),
            "the earlier send target, not a later channel-valued payload, is the event identity"
        );
    }

    #[test]
    fn reaching_definition_is_valid_channel_identity_evidence() {
        let mut cpg = extraction_fixture(false);
        cpg.add_node_with_id(event(60, MsgKind::SendSync));
        cpg.add_node_with_id(ident(61, "c"));
        connect_ast(&mut cpg, 11, 60);
        connect_ast(&mut cpg, 60, 61);
        cpg.connect(
            NodeId::new(20),
            NodeId::new(61),
            CpgEdgeKind::DataFlow(DfgEdgeKind::ReachingDef),
        );

        let events = extract_events(&cpg);
        assert_eq!(
            events[&NodeId::new(10)].last().expect("sync send").channel,
            Some(NodeId::new(20))
        );
    }

    #[test]
    fn channel_less_kinds_never_capture_descendant_identifiers() {
        let mut cpg = extraction_fixture(false);
        cpg.add_node_with_id(ident(51, "c"));
        connect_ast(&mut cpg, 50, 51);
        cpg.connect(
            NodeId::new(20),
            NodeId::new(51),
            CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
        );
        let events = extract_events(&cpg);
        let spawn = events[&NodeId::new(10)]
            .iter()
            .find(|event| event.kind == MsgKind::Spawn)
            .expect("spawn");
        assert_eq!(spawn.channel, None);
    }

    #[test]
    fn nested_event_is_a_statement_boundary_for_channel_resolution() {
        let mut cpg = extraction_fixture(false);
        cpg.add_node_with_id(event(60, MsgKind::Send));
        cpg.add_node_with_id(event(61, MsgKind::Recv));
        cpg.add_node_with_id(ident(62, "c"));
        connect_ast(&mut cpg, 11, 60);
        connect_ast(&mut cpg, 60, 61);
        connect_ast(&mut cpg, 61, 62);
        cpg.connect(
            NodeId::new(20),
            NodeId::new(62),
            CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
        );

        let events = extract_events(&cpg);
        let events = &events[&NodeId::new(10)];
        assert_eq!(
            events
                .iter()
                .find(|event| event.site == NodeId::new(60))
                .expect("outer")
                .channel,
            None
        );
        assert_eq!(
            events
                .iter()
                .find(|event| event.site == NodeId::new(61))
                .expect("inner")
                .channel,
            Some(NodeId::new(20))
        );
    }

    #[test]
    fn nested_function_events_are_not_folded_into_parent_process() {
        let mut cpg = extraction_fixture(false);
        cpg.add_node_with_id(function(60, "nested"));
        cpg.add_node_with_id(event(61, MsgKind::SendSync));
        cpg.add_node_with_id(ident(62, "c"));
        connect_ast(&mut cpg, 11, 60);
        connect_ast(&mut cpg, 60, 61);
        connect_ast(&mut cpg, 61, 62);
        cpg.connect(
            NodeId::new(20),
            NodeId::new(62),
            CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
        );
        let events = extract_events(&cpg);
        assert_eq!(
            events.len(),
            1,
            "only top-level functions are process roots"
        );
        assert!(!events[&NodeId::new(10)]
            .iter()
            .any(|event| event.site == NodeId::new(61)));
    }

    #[test]
    fn graph_without_top_level_functions_has_no_process_events() {
        let mut cpg = CodePropertyGraph::new(Language::Rholang);
        cpg.add_node_with_id(event(10, MsgKind::Send));
        assert!(extract_events(&cpg).is_empty());
    }

    #[test]
    fn extraction_is_read_only_and_all_emitted_sites_exist() {
        let cpg = extraction_fixture(false);
        let counts = (cpg.node_count(), cpg.edge_count());
        let events = extract_events(&cpg);
        assert_eq!(counts, (cpg.node_count(), cpg.edge_count()));
        assert!(events
            .values()
            .flatten()
            .all(|event| cpg.contains_node(event.site)
                && event
                    .channel
                    .is_none_or(|channel| cpg.contains_node(channel))));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn extraction_is_independent_of_edge_insertion_order(reverse in any::<bool>()) {
            let expected = extract_events(&extraction_fixture(false));
            prop_assert_eq!(extract_events(&extraction_fixture(reverse)), expected);
        }
    }
}
