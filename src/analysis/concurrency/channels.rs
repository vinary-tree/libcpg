//! Channel topology and linear-consumption classification.

use std::collections::{BTreeMap, BTreeSet};

use crate::{CodePropertyGraph, MsgKind, NodeId};

use super::{extract_events, ChannelEvent, ChannelId};

/// Advisory channel-topology finding category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ChannelFindingKind {
    /// At least one send has no consuming, persistent, or peeking receiver.
    OrphanSend,
    /// At least one linear receive has no producer anywhere in the graph.
    BlockedRecv,
    /// Linear consumption competes among multiple producers or receivers.
    NondeterministicConsumption,
}

impl ChannelFindingKind {
    /// Stable snake-case representation for reports and external protocols.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OrphanSend => "orphan_send",
            Self::BlockedRecv => "blocked_recv",
            Self::NondeterministicConsumption => "nondeterministic_consumption",
        }
    }
}

/// One evidence-backed, advisory channel finding.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ChannelFinding {
    /// Finding category.
    pub kind: ChannelFindingKind,
    /// Resolved channel definition involved in the finding.
    pub channel: ChannelId,
    /// Participating top-level process/function ids, sorted and deduplicated.
    pub processes: Vec<NodeId>,
    /// Event sites supporting the finding, sorted and deduplicated.
    pub sites: Vec<NodeId>,
    /// Human-readable explanation of the structural evidence and limitation.
    pub detail: String,
}

#[derive(Default)]
struct ChannelTally {
    producers: Vec<(NodeId, ChannelEvent)>,
    linear_receivers: Vec<(NodeId, ChannelEvent)>,
    persistent_receivers: Vec<(NodeId, ChannelEvent)>,
    peeks: Vec<(NodeId, ChannelEvent)>,
}

impl ChannelTally {
    fn has_receiver(&self) -> bool {
        !self.linear_receivers.is_empty()
            || !self.persistent_receivers.is_empty()
            || !self.peeks.is_empty()
    }
}

/// Reports channel matching failures and linear-consumption contention.
///
/// The analysis is advisory and read-only: it neither mutates the CPG nor
/// claims reachability. Findings are derived from graph-wide topology only.
/// Persistent receives never produce [`ChannelFindingKind::BlockedRecv`], and
/// peeks satisfy observation matching without counting as consumers.
pub fn channel_report(cpg: &CodePropertyGraph) -> Vec<ChannelFinding> {
    let events = extract_events(cpg);
    let mut tallies: BTreeMap<ChannelId, ChannelTally> = BTreeMap::new();
    for (process, process_events) in events {
        for event in process_events {
            let Some(channel) = event.channel else {
                continue;
            };
            let tally = tallies.entry(channel).or_default();
            match event.kind {
                MsgKind::Send | MsgKind::SendSync => tally.producers.push((process, event)),
                MsgKind::Recv => tally.linear_receivers.push((process, event)),
                MsgKind::RecvPersistent => tally.persistent_receivers.push((process, event)),
                MsgKind::Peek => tally.peeks.push((process, event)),
                MsgKind::Select | MsgKind::Spawn => {}
            }
        }
    }

    let mut findings = Vec::new();
    for (channel, tally) in tallies {
        if !tally.producers.is_empty() && !tally.has_receiver() {
            findings.push(finding(
                ChannelFindingKind::OrphanSend,
                channel,
                &tally.producers,
                format!(
                    "{} send site(s) have no linear, persistent, or peeking receiver",
                    tally.producers.len()
                ),
            ));
        }
        if !tally.linear_receivers.is_empty() && tally.producers.is_empty() {
            findings.push(finding(
                ChannelFindingKind::BlockedRecv,
                channel,
                &tally.linear_receivers,
                format!(
                    "{} linear receive site(s) have no producer; persistent receives and peeks are excluded from this starvation condition",
                    tally.linear_receivers.len()
                ),
            ));
        }
        if !tally.linear_receivers.is_empty()
            && (tally.producers.len() >= 2 || tally.linear_receivers.len() >= 2)
        {
            let evidence: Vec<_> = tally
                .producers
                .iter()
                .chain(&tally.linear_receivers)
                .copied()
                .collect();
            findings.push(finding(
                ChannelFindingKind::NondeterministicConsumption,
                channel,
                &evidence,
                format!(
                    "linear consumption has {} producer site(s) and {} linear receiver site(s); the matching choice is non-deterministic",
                    tally.producers.len(),
                    tally.linear_receivers.len()
                ),
            ));
        }
    }
    findings
}

fn finding(
    kind: ChannelFindingKind,
    channel: ChannelId,
    evidence: &[(NodeId, ChannelEvent)],
    detail: String,
) -> ChannelFinding {
    let processes: BTreeSet<_> = evidence.iter().map(|(process, _)| *process).collect();
    let sites: BTreeSet<_> = evidence.iter().map(|(_, event)| event.site).collect();
    ChannelFinding {
        kind,
        channel,
        processes: processes.into_iter().collect(),
        sites: sites.into_iter().collect(),
        detail,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CpgEdgeKind, CpgNode, CpgNodeKind, DfgEdgeKind, Language, MethodSignature, ScopeId,
        SourceRange, Visibility,
    };
    use proptest::prelude::*;

    fn function(id: u32) -> CpgNode {
        CpgNode::new(
            NodeId::new(id),
            CpgNodeKind::Function {
                signature: MethodSignature {
                    name: format!("p{id}").into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Public,
                },
            },
            SourceRange::default(),
        )
    }

    fn variable(id: u32) -> CpgNode {
        CpgNode::new(
            NodeId::new(id),
            CpgNodeKind::Variable {
                name: format!("c{id}").into(),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: false,
            },
            SourceRange::default(),
        )
    }

    fn operation(id: u32, kind: MsgKind) -> CpgNode {
        CpgNode::new(
            NodeId::new(id),
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
            SourceRange::from_bytes(id, id + 1),
        )
        .with_message_kind(kind)
    }

    fn identifier(id: u32, channel: u32) -> CpgNode {
        CpgNode::new(
            NodeId::new(id),
            CpgNodeKind::Identifier {
                name: format!("c{channel}").into(),
                definition: Some(NodeId::new(channel)),
            },
            SourceRange::from_bytes(id, id + 1),
        )
    }

    fn fixture(spec: &[(u32, MsgKind, u32)], reverse_edges: bool) -> CodePropertyGraph {
        let mut cpg = CodePropertyGraph::new(Language::Rholang);
        let channels: BTreeSet<_> = spec.iter().map(|(_, _, channel)| *channel).collect();
        for channel in channels {
            cpg.add_node_with_id(variable(channel));
        }
        let processes: BTreeSet<_> = spec.iter().map(|(process, _, _)| *process).collect();
        for process in processes {
            cpg.add_node_with_id(function(process));
        }
        let mut edges = Vec::new();
        for (index, &(process, kind, channel)) in spec.iter().enumerate() {
            let site = 1_000 + index as u32 * 2;
            let use_site = site + 1;
            cpg.add_node_with_id(operation(site, kind));
            cpg.add_node_with_id(identifier(use_site, channel));
            edges.push((process, site, CpgEdgeKind::AstChild));
            edges.push((site, use_site, CpgEdgeKind::AstChild));
            edges.push((
                channel,
                use_site,
                CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
            ));
        }
        if reverse_edges {
            edges.reverse();
        }
        for (source, target, kind) in edges {
            cpg.connect(NodeId::new(source), NodeId::new(target), kind);
        }
        cpg
    }

    fn kinds(cpg: &CodePropertyGraph) -> Vec<ChannelFindingKind> {
        channel_report(cpg)
            .into_iter()
            .map(|finding| finding.kind)
            .collect()
    }

    #[test]
    fn blocked_linear_receive_without_producer() {
        let cpg = fixture(&[(10, MsgKind::Recv, 100)], false);
        assert_eq!(kinds(&cpg), vec![ChannelFindingKind::BlockedRecv]);
    }

    #[test]
    fn orphan_send_without_receiver() {
        let cpg = fixture(&[(10, MsgKind::Send, 100)], false);
        assert_eq!(kinds(&cpg), vec![ChannelFindingKind::OrphanSend]);
    }

    #[test]
    fn matched_send_and_linear_receive_are_clean() {
        let cpg = fixture(&[(10, MsgKind::Send, 100), (20, MsgKind::Recv, 100)], false);
        assert!(channel_report(&cpg).is_empty());
    }

    #[test]
    fn persistent_receive_never_reports_starvation() {
        let cpg = fixture(&[(10, MsgKind::RecvPersistent, 100)], false);
        assert!(channel_report(&cpg).is_empty());
    }

    #[test]
    fn peek_is_non_consuming_but_observes_a_send() {
        let cpg = fixture(
            &[(10, MsgKind::SendSync, 100), (20, MsgKind::Peek, 100)],
            false,
        );
        assert!(channel_report(&cpg).is_empty());
    }

    #[test]
    fn unresolved_events_are_excluded_instead_of_becoming_one_channel() {
        let mut cpg = CodePropertyGraph::new(Language::Rholang);
        cpg.add_node_with_id(function(10));
        cpg.add_node_with_id(operation(20, MsgKind::Send));
        cpg.add_node_with_id(operation(30, MsgKind::Recv));
        cpg.connect(NodeId::new(10), NodeId::new(20), CpgEdgeKind::AstChild);
        cpg.connect(NodeId::new(10), NodeId::new(30), CpgEdgeKind::AstChild);
        assert!(channel_report(&cpg).is_empty());
    }

    #[test]
    fn two_producers_for_linear_receive_are_flagged() {
        let cpg = fixture(
            &[
                (10, MsgKind::Send, 100),
                (20, MsgKind::SendSync, 100),
                (30, MsgKind::Recv, 100),
            ],
            false,
        );
        let findings = channel_report(&cpg);
        assert_eq!(findings.len(), 1);
        assert_eq!(
            findings[0].kind,
            ChannelFindingKind::NondeterministicConsumption
        );
        assert_eq!(
            findings[0].processes,
            vec![NodeId::new(10), NodeId::new(20), NodeId::new(30)]
        );
    }

    #[test]
    fn two_linear_receivers_are_flagged_even_with_one_producer() {
        let cpg = fixture(
            &[
                (10, MsgKind::Send, 100),
                (20, MsgKind::Recv, 100),
                (20, MsgKind::Recv, 100),
            ],
            false,
        );
        assert_eq!(
            kinds(&cpg),
            vec![ChannelFindingKind::NondeterministicConsumption]
        );
    }

    #[test]
    fn report_is_read_only_and_references_only_graph_nodes() {
        let cpg = fixture(&[(10, MsgKind::Recv, 100)], false);
        let before = (cpg.node_count(), cpg.edge_count());
        let findings = channel_report(&cpg);
        assert_eq!(before, (cpg.node_count(), cpg.edge_count()));
        assert!(findings.iter().all(|finding| {
            cpg.contains_node(finding.channel)
                && finding.processes.iter().all(|id| cpg.contains_node(*id))
                && finding.sites.iter().all(|id| cpg.contains_node(*id))
        }));
    }

    #[test]
    fn findings_are_ordered_by_channel_then_fixed_kind_order() {
        let cpg = fixture(
            &[
                (10, MsgKind::Recv, 300),
                (20, MsgKind::Send, 100),
                (30, MsgKind::Recv, 200),
                (40, MsgKind::Recv, 200),
            ],
            false,
        );
        let findings = channel_report(&cpg);
        assert_eq!(
            findings
                .iter()
                .map(|finding| (finding.channel, finding.kind))
                .collect::<Vec<_>>(),
            vec![
                (NodeId::new(100), ChannelFindingKind::OrphanSend),
                (NodeId::new(200), ChannelFindingKind::BlockedRecv),
                (
                    NodeId::new(200),
                    ChannelFindingKind::NondeterministicConsumption,
                ),
                (NodeId::new(300), ChannelFindingKind::BlockedRecv),
            ]
        );
    }

    #[test]
    fn finding_kind_strings_are_stable() {
        assert_eq!(ChannelFindingKind::OrphanSend.as_str(), "orphan_send");
        assert_eq!(ChannelFindingKind::BlockedRecv.as_str(), "blocked_recv");
        assert_eq!(
            ChannelFindingKind::NondeterministicConsumption.as_str(),
            "nondeterministic_consumption"
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn events_and_findings_round_trip_through_serde() {
        let cpg = fixture(
            &[
                (10, MsgKind::Send, 100),
                (20, MsgKind::SendSync, 100),
                (30, MsgKind::Recv, 100),
            ],
            false,
        );
        let events = extract_events(&cpg);
        let event_json = serde_json::to_string(&events).expect("serialize events");
        let event_round_trip: BTreeMap<NodeId, Vec<ChannelEvent>> =
            serde_json::from_str(&event_json).expect("deserialize events");
        assert_eq!(event_round_trip, events);

        let findings = channel_report(&cpg);
        let finding_json = serde_json::to_string(&findings).expect("serialize findings");
        let finding_round_trip: Vec<ChannelFinding> =
            serde_json::from_str(&finding_json).expect("deserialize findings");
        assert_eq!(finding_round_trip, findings);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn report_order_is_independent_of_edge_insertion_order(reverse in any::<bool>()) {
            let spec = [
                (30, MsgKind::Recv, 300),
                (10, MsgKind::Send, 100),
                (20, MsgKind::Recv, 200),
                (40, MsgKind::SendSync, 100),
                (50, MsgKind::Recv, 100),
            ];
            let expected = channel_report(&fixture(&spec, false));
            prop_assert_eq!(channel_report(&fixture(&spec, reverse)), expected);
        }

        #[test]
        fn classification_follows_structural_counts(
            producer_count in 0usize..5,
            receiver_count in 0usize..5,
        ) {
            let mut spec = Vec::new();
            for index in 0..producer_count {
                spec.push((10 + index as u32, MsgKind::Send, 100));
            }
            for index in 0..receiver_count {
                spec.push((200 + index as u32, MsgKind::Recv, 100));
            }
            let findings = channel_report(&fixture(&spec, false));
            let has = |kind| findings.iter().any(|finding| finding.kind == kind);
            prop_assert_eq!(has(ChannelFindingKind::OrphanSend), producer_count > 0 && receiver_count == 0);
            prop_assert_eq!(has(ChannelFindingKind::BlockedRecv), receiver_count > 0 && producer_count == 0);
            prop_assert_eq!(
                has(ChannelFindingKind::NondeterministicConsumption),
                receiver_count > 0 && (producer_count >= 2 || receiver_count >= 2),
            );
        }
    }
}
