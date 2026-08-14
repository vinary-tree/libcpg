//! Communication and lock-order deadlock candidates.
//!
//! Both analyses construct deterministic external [`GraphProjection`] values
//! and delegate cycle recognition to the crate's shared SCC implementation.
//! The source CPG is never mutated, and every reported edge retains the CPG
//! nodes and source ranges that justify it.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    CodePropertyGraph, CpgNodeKind, GraphProjection, LockKind, LockMode, MsgKind, NodeId,
    ProjectionKind, SourceRange,
};

use super::super::{function_scope_nodes, projection_sccs};
use super::{events::resolve_event_variable, extract_events, ChannelEvent, ChannelId};

/// Advisory deadlock-candidate category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DeadlockFindingKind {
    /// A strongly connected component in the process wait-for graph.
    CommunicationCycle,
    /// A conflicting strongly connected component in the lock-order graph.
    LockOrderCycle,
}

impl DeadlockFindingKind {
    /// Stable snake-case representation for reports and external protocols.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CommunicationCycle => "communication_cycle",
            Self::LockOrderCycle => "lock_order_cycle",
        }
    }
}

/// One statement-precise edge witnessing a deadlock candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DeadlockWitness {
    /// Process `waiter` cannot reach its send because it initially waits for a
    /// message that `producer` can emit only after its own initial wait.
    Communication {
        /// Initially blocked process.
        waiter: NodeId,
        /// Initially blocked process with a producer for `channel`.
        producer: NodeId,
        /// Definition identity of the awaited channel.
        channel: ChannelId,
        /// Linear receive site establishing the wait.
        wait_site: NodeId,
        /// Source range of `wait_site`.
        wait_range: SourceRange,
        /// Representative send site in `producer`.
        producer_site: NodeId,
        /// Source range of `producer_site`.
        producer_range: SourceRange,
    },
    /// `acquired_lock` can be acquired while `held_lock` is held.
    LockOrder {
        /// Process that holds `held_lock` at the ordering point.
        holder: NodeId,
        /// Process containing the representative acquisition. This differs
        /// from `holder` for an interprocedural summary edge.
        acquirer: NodeId,
        /// Lock held before the second acquisition.
        held_lock: NodeId,
        /// Lock acquired while `held_lock` is held.
        acquired_lock: NodeId,
        /// Sharing mode of the held acquisition.
        held_mode: LockMode,
        /// Sharing mode of the later acquisition.
        acquired_mode: LockMode,
        /// Representative acquisition site for `held_lock`.
        held_site: NodeId,
        /// Source range of `held_site`.
        held_range: SourceRange,
        /// Representative acquisition site for `acquired_lock`.
        acquired_site: NodeId,
        /// Source range of `acquired_site`.
        acquired_range: SourceRange,
        /// Caller-side call site for an interprocedural edge.
        via_call: Option<NodeId>,
    },
}

/// One evidence-backed cyclic-wait candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DeadlockFinding {
    /// Candidate category.
    pub kind: DeadlockFindingKind,
    /// Participating process/function ids, sorted and deduplicated.
    pub processes: Vec<NodeId>,
    /// Participating channel or lock definition ids, sorted and deduplicated.
    pub resources: Vec<NodeId>,
    /// Statement-precise wait-for or lock-order edges inside the cyclic SCC.
    pub witnesses: Vec<DeadlockWitness>,
    /// Human-readable explanation of the structural evidence and limitation.
    pub detail: String,
}

/// Reports structural communication and lock-order deadlock candidates.
///
/// Communication edges have the form `P -> Q` when `P`'s first relevant
/// message operation is a linear receive on channel `C`, every known producer
/// of `C` is itself initially blocked, and `Q` is one such producer. A send as
/// the first relevant operation therefore breaks the wait. Persistent receives,
/// peeks, selection, and spawning do not establish an initial linear wait.
///
/// Lock-order edges have the form `A -> B` when a typed acquisition of `B`
/// occurs while `A` is held. Resolved calls contribute finite transitive
/// acquisition summaries in the RacerD style. All-read SCCs are excluded
/// because shared acquisitions do not mutually exclude one another.
///
/// The result is deterministic, advisory, and out of graph. Missing channel,
/// lock, or call-target evidence suppresses conclusions rather than inventing
/// identity or control flow.
pub fn communication_deadlocks(cpg: &CodePropertyGraph) -> Vec<DeadlockFinding> {
    let channel_events = extract_events(cpg);
    let processes: Vec<_> = channel_events.keys().copied().collect();
    let mut findings = communication_findings(&channel_events);
    findings.extend(lock_order_findings(cpg, &processes));
    findings
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WaitEdge {
    waiter: NodeId,
    producer: NodeId,
    channel: ChannelId,
    wait: ChannelEvent,
    send: ChannelEvent,
}

fn communication_findings(
    events_by_process: &BTreeMap<NodeId, Vec<ChannelEvent>>,
) -> Vec<DeadlockFinding> {
    let mut initial_waits = BTreeMap::<NodeId, (ChannelId, ChannelEvent)>::new();
    let mut producers = BTreeMap::<ChannelId, BTreeMap<NodeId, ChannelEvent>>::new();

    for (&process, events) in events_by_process {
        for &event in events {
            if event.kind.is_send() {
                if let Some(channel) = event.channel {
                    producers
                        .entry(channel)
                        .or_default()
                        .entry(process)
                        .or_insert(event);
                }
            }
        }

        let first_relevant = events
            .iter()
            .find(|event| event.kind.is_send() || event.kind.is_linear_receive());
        if let Some(&event) = first_relevant {
            if event.kind == MsgKind::Recv {
                if let Some(channel) = event.channel {
                    initial_waits.insert(process, (channel, event));
                }
            }
        }
    }

    let mut edges = Vec::new();
    for (&waiter, &(channel, wait)) in &initial_waits {
        let Some(channel_producers) = producers.get(&channel) else {
            continue;
        };
        if channel_producers.is_empty()
            || channel_producers
                .keys()
                .any(|producer| !initial_waits.contains_key(producer))
        {
            continue;
        }
        for (&producer, &send) in channel_producers {
            edges.push(WaitEdge {
                waiter,
                producer,
                channel,
                wait,
                send,
            });
        }
    }
    edges.sort_unstable_by_key(|edge| {
        (
            edge.waiter,
            edge.producer,
            edge.channel,
            edge.wait.site,
            edge.send.site,
        )
    });

    let nodes: BTreeSet<_> = edges
        .iter()
        .flat_map(|edge| [edge.waiter, edge.producer])
        .collect();
    let projection = GraphProjection::from_edges(
        ProjectionKind::External,
        nodes,
        edges.iter().map(|edge| (edge.waiter, edge.producer, 1.0)),
    );
    let decomposition = projection_sccs(&projection);

    let mut findings = Vec::new();
    for component in decomposition.cyclic_components() {
        let members: BTreeSet<_> = component.nodes.iter().copied().collect();
        let internal: Vec<_> = edges
            .iter()
            .filter(|edge| members.contains(&edge.waiter) && members.contains(&edge.producer))
            .collect();
        let resources: BTreeSet<_> = internal.iter().map(|edge| edge.channel).collect();
        let witnesses = internal
            .iter()
            .map(|edge| DeadlockWitness::Communication {
                waiter: edge.waiter,
                producer: edge.producer,
                channel: edge.channel,
                wait_site: edge.wait.site,
                wait_range: edge.wait.range,
                producer_site: edge.send.site,
                producer_range: edge.send.range,
            })
            .collect();
        findings.push(DeadlockFinding {
            kind: DeadlockFindingKind::CommunicationCycle,
            processes: component.nodes.clone(),
            resources: resources.into_iter().collect(),
            witnesses,
            detail: format!(
                "{} initially blocked process(es) form a wait-for SCC over {} channel(s); this is a structural candidate, not a reachability proof",
                component.nodes.len(),
                internal
                    .iter()
                    .map(|edge| edge.channel)
                    .collect::<BTreeSet<_>>()
                    .len()
            ),
        });
    }
    findings
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LockEvent {
    process: NodeId,
    kind: LockKind,
    resource: Option<NodeId>,
    site: NodeId,
    range: SourceRange,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LockSkeletonEvent {
    Lock(LockEvent),
    Call { site: NodeId, callee: NodeId },
}

impl LockSkeletonEvent {
    fn site(self) -> NodeId {
        match self {
            Self::Lock(event) => event.site,
            Self::Call { site, .. } => site,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Acquisition {
    process: NodeId,
    resource: NodeId,
    mode: LockMode,
    site: NodeId,
    range: SourceRange,
}

impl Acquisition {
    fn from_event(event: LockEvent) -> Option<Self> {
        Some(Self {
            process: event.process,
            resource: event.resource?,
            mode: event.kind.acquisition_mode()?,
            site: event.site,
            range: event.range,
        })
    }

    fn stable_key(self) -> (NodeId, NodeId, LockMode) {
        (self.site, self.process, self.mode)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LockEdge {
    holder: NodeId,
    acquirer: NodeId,
    held_lock: NodeId,
    acquired_lock: NodeId,
    held_mode: LockMode,
    acquired_mode: LockMode,
    held_site: NodeId,
    held_range: SourceRange,
    acquired_site: NodeId,
    acquired_range: SourceRange,
    via_call: Option<NodeId>,
}

fn lock_order_findings(cpg: &CodePropertyGraph, processes: &[NodeId]) -> Vec<DeadlockFinding> {
    let skeletons = lock_skeletons(cpg, processes);
    let summaries = reachable_acquisitions(&skeletons);
    let mut edges = build_lock_edges(&skeletons, &summaries);
    edges.sort_unstable_by_key(|edge| {
        (
            edge.held_lock,
            edge.acquired_lock,
            edge.holder,
            edge.acquirer,
            edge.held_site,
            edge.acquired_site,
            edge.via_call,
        )
    });
    edges.dedup();

    let nodes: BTreeSet<_> = edges
        .iter()
        .flat_map(|edge| [edge.held_lock, edge.acquired_lock])
        .collect();
    let projection = GraphProjection::from_edges(
        ProjectionKind::External,
        nodes,
        edges
            .iter()
            .map(|edge| (edge.held_lock, edge.acquired_lock, 1.0)),
    );
    let decomposition = projection_sccs(&projection);

    let mut findings = Vec::new();
    for component in decomposition.cyclic_components() {
        let members: BTreeSet<_> = component.nodes.iter().copied().collect();
        let internal: Vec<_> = edges
            .iter()
            .filter(|edge| {
                members.contains(&edge.held_lock) && members.contains(&edge.acquired_lock)
            })
            .collect();
        if internal
            .iter()
            .all(|edge| edge.held_mode == LockMode::Read && edge.acquired_mode == LockMode::Read)
        {
            continue;
        }
        let processes: BTreeSet<_> = internal
            .iter()
            .flat_map(|edge| [edge.holder, edge.acquirer])
            .collect();
        let witnesses = internal
            .iter()
            .map(|edge| DeadlockWitness::LockOrder {
                holder: edge.holder,
                acquirer: edge.acquirer,
                held_lock: edge.held_lock,
                acquired_lock: edge.acquired_lock,
                held_mode: edge.held_mode,
                acquired_mode: edge.acquired_mode,
                held_site: edge.held_site,
                held_range: edge.held_range,
                acquired_site: edge.acquired_site,
                acquired_range: edge.acquired_range,
                via_call: edge.via_call,
            })
            .collect();
        findings.push(DeadlockFinding {
            kind: DeadlockFindingKind::LockOrderCycle,
            processes: processes.into_iter().collect(),
            resources: component.nodes.clone(),
            witnesses,
            detail: format!(
                "{} lock resource(s) form a conflicting lock-order SCC; this is a structural candidate, not a schedule proof",
                component.nodes.len()
            ),
        });
    }
    findings
}

fn lock_skeletons(
    cpg: &CodePropertyGraph,
    processes: &[NodeId],
) -> BTreeMap<NodeId, Vec<LockSkeletonEvent>> {
    let process_set: BTreeSet<_> = processes.iter().copied().collect();
    let mut result = BTreeMap::new();
    for &process in processes {
        let mut sites: Vec<_> = function_scope_nodes(cpg, process)
            .into_iter()
            .filter(|&site| {
                cpg.node(site)
                    .is_some_and(|node| matches!(node.kind, CpgNodeKind::Call { .. }))
            })
            .collect();
        sites.sort_unstable();

        let mut events = Vec::new();
        for site in sites {
            let Some(node) = cpg.node(site) else {
                continue;
            };
            if let Some(kind) = node.lock_kind {
                events.push(LockSkeletonEvent::Lock(LockEvent {
                    process,
                    kind,
                    resource: resolve_event_variable(cpg, site),
                    site,
                    range: node.range,
                }));
            } else if let CpgNodeKind::Call {
                target: Some(callee),
                ..
            } = &node.kind
            {
                if process_set.contains(callee) {
                    events.push(LockSkeletonEvent::Call {
                        site,
                        callee: *callee,
                    });
                }
            }
        }
        events.sort_unstable_by_key(|event| event.site());
        result.insert(process, events);
    }
    result
}

fn reachable_acquisitions(
    skeletons: &BTreeMap<NodeId, Vec<LockSkeletonEvent>>,
) -> BTreeMap<NodeId, BTreeMap<NodeId, Acquisition>> {
    let mut summaries = BTreeMap::<NodeId, BTreeMap<NodeId, Acquisition>>::new();
    for (&process, events) in skeletons {
        let summary = summaries.entry(process).or_default();
        for &event in events {
            if let LockSkeletonEvent::Lock(lock) = event {
                if let Some(acquisition) = Acquisition::from_event(lock) {
                    insert_representative(summary, acquisition);
                }
            }
        }
    }

    loop {
        let snapshot = summaries.clone();
        let mut changed = false;
        for (&process, events) in skeletons {
            let summary = summaries.entry(process).or_default();
            for &event in events {
                let LockSkeletonEvent::Call { callee, .. } = event else {
                    continue;
                };
                let Some(callee_summary) = snapshot.get(&callee) else {
                    continue;
                };
                for &acquisition in callee_summary.values() {
                    changed |= insert_representative(summary, acquisition);
                }
            }
        }
        if !changed {
            return summaries;
        }
    }
}

fn insert_representative(
    summary: &mut BTreeMap<NodeId, Acquisition>,
    acquisition: Acquisition,
) -> bool {
    match summary.get(&acquisition.resource) {
        Some(previous) if previous.stable_key() <= acquisition.stable_key() => false,
        _ => {
            summary.insert(acquisition.resource, acquisition);
            true
        }
    }
}

fn build_lock_edges(
    skeletons: &BTreeMap<NodeId, Vec<LockSkeletonEvent>>,
    summaries: &BTreeMap<NodeId, BTreeMap<NodeId, Acquisition>>,
) -> Vec<LockEdge> {
    let mut edges = Vec::new();
    for (&process, events) in skeletons {
        let mut held = Vec::<Acquisition>::new();
        for &event in events {
            match event {
                LockSkeletonEvent::Lock(lock) => {
                    if let Some(acquisition) = Acquisition::from_event(lock) {
                        for &prior in &held {
                            push_lock_edge(&mut edges, prior, acquisition, process, None);
                        }
                        held.push(acquisition);
                    } else if lock.kind == LockKind::Release {
                        match lock.resource {
                            Some(resource) => {
                                if let Some(position) =
                                    held.iter().rposition(|held| held.resource == resource)
                                {
                                    held.remove(position);
                                }
                            }
                            None => held.clear(),
                        }
                    }
                }
                LockSkeletonEvent::Call { site, callee, .. } => {
                    let Some(reachable) = summaries.get(&callee) else {
                        continue;
                    };
                    for &acquisition in reachable.values() {
                        for &prior in &held {
                            push_lock_edge(&mut edges, prior, acquisition, process, Some(site));
                        }
                    }
                }
            }
        }
    }
    edges
}

fn push_lock_edge(
    edges: &mut Vec<LockEdge>,
    held: Acquisition,
    acquired: Acquisition,
    holder: NodeId,
    via_call: Option<NodeId>,
) {
    if held.resource == acquired.resource {
        return;
    }
    edges.push(LockEdge {
        holder,
        acquirer: acquired.process,
        held_lock: held.resource,
        acquired_lock: acquired.resource,
        held_mode: held.mode,
        acquired_mode: acquired.mode,
        held_site: held.site,
        held_range: held.range,
        acquired_site: acquired.site,
        acquired_range: acquired.range,
        via_call,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CpgEdgeKind, CpgNode, DfgEdgeKind, Language, MethodSignature, ScopeId, Visibility,
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
            SourceRange::from_bytes(id, id + 1),
        )
    }

    fn variable(id: u32) -> CpgNode {
        CpgNode::new(
            NodeId::new(id),
            CpgNodeKind::Variable {
                name: format!("r{id}").into(),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: false,
            },
            SourceRange::from_bytes(id, id + 1),
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

    fn lock_call(id: u32, kind: LockKind) -> CpgNode {
        CpgNode::new(
            NodeId::new(id),
            CpgNodeKind::Call {
                target: None,
                is_method: true,
            },
            SourceRange::from_bytes(id, id + 1),
        )
        .with_lock_kind(kind)
    }

    fn ordinary_call(id: u32, target: u32) -> CpgNode {
        CpgNode::new(
            NodeId::new(id),
            CpgNodeKind::Call {
                target: Some(NodeId::new(target)),
                is_method: false,
            },
            SourceRange::from_bytes(id, id + 1),
        )
    }

    fn identifier(id: u32, resource: u32) -> CpgNode {
        CpgNode::new(
            NodeId::new(id),
            CpgNodeKind::Identifier {
                name: format!("r{resource}").into(),
                definition: Some(NodeId::new(resource)),
            },
            SourceRange::from_bytes(id, id + 1),
        )
    }

    fn channel_fixture(
        spec: &[(u32, MsgKind, u32, u32)],
        reverse_edges: bool,
    ) -> CodePropertyGraph {
        let mut cpg = CodePropertyGraph::new(Language::Rholang);
        for process in spec.iter().map(|entry| entry.0).collect::<BTreeSet<_>>() {
            cpg.add_node_with_id(function(process));
        }
        for channel in spec.iter().map(|entry| entry.2).collect::<BTreeSet<_>>() {
            cpg.add_node_with_id(variable(channel));
        }
        let mut edges = Vec::new();
        for &(process, kind, channel, site) in spec {
            cpg.add_node_with_id(operation(site, kind));
            cpg.add_node_with_id(identifier(site + 1, channel));
            edges.push((process, site, CpgEdgeKind::AstChild));
            edges.push((site, site + 1, CpgEdgeKind::AstChild));
            edges.push((
                channel,
                site + 1,
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

    fn add_lock_event(
        cpg: &mut CodePropertyGraph,
        process: u32,
        site: u32,
        resource: u32,
        kind: LockKind,
    ) {
        cpg.add_node_with_id(lock_call(site, kind));
        cpg.add_node_with_id(identifier(site + 1, resource));
        cpg.connect(
            NodeId::new(process),
            NodeId::new(site),
            CpgEdgeKind::AstChild,
        );
        cpg.connect(
            NodeId::new(site),
            NodeId::new(site + 1),
            CpgEdgeKind::AstChild,
        );
    }

    fn lock_fixture() -> CodePropertyGraph {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        for node in [function(10), function(20), variable(100), variable(200)] {
            cpg.add_node_with_id(node);
        }
        add_lock_event(&mut cpg, 10, 1_000, 100, LockKind::AcquireWrite);
        add_lock_event(&mut cpg, 10, 1_010, 200, LockKind::AcquireWrite);
        add_lock_event(&mut cpg, 20, 2_000, 200, LockKind::AcquireWrite);
        add_lock_event(&mut cpg, 20, 2_010, 100, LockKind::AcquireWrite);
        cpg
    }

    fn lock_fixture_with_reversed_edges(reverse: bool) -> CodePropertyGraph {
        let original = lock_fixture();
        if !reverse {
            return original;
        }
        let mut nodes: Vec<_> = original.nodes().cloned().collect();
        let mut edges: Vec<_> = original
            .edges()
            .map(|edge| (edge.source, edge.target, edge.kind.clone()))
            .collect();
        nodes.reverse();
        edges.reverse();
        let mut rebuilt = CodePropertyGraph::new(Language::Rust);
        for node in nodes {
            rebuilt.add_node_with_id(node);
        }
        for (source, target, kind) in edges {
            rebuilt.connect(source, target, kind);
        }
        rebuilt
    }

    #[test]
    fn mutual_initial_receive_cycle_has_precise_witnesses() {
        let cpg = channel_fixture(
            &[
                (10, MsgKind::Recv, 100, 1_000),
                (10, MsgKind::Send, 200, 1_010),
                (20, MsgKind::Recv, 200, 2_000),
                (20, MsgKind::SendSync, 100, 2_010),
            ],
            false,
        );
        let findings = communication_deadlocks(&cpg);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, DeadlockFindingKind::CommunicationCycle);
        assert_eq!(
            findings[0].processes,
            vec![NodeId::new(10), NodeId::new(20)]
        );
        assert_eq!(
            findings[0].resources,
            vec![NodeId::new(100), NodeId::new(200)]
        );
        assert_eq!(findings[0].witnesses.len(), 2);
    }

    #[test]
    fn send_first_breaks_communication_cycle() {
        let cpg = channel_fixture(
            &[
                (10, MsgKind::Send, 100, 1_000),
                (10, MsgKind::Recv, 200, 1_010),
                (20, MsgKind::Recv, 100, 2_000),
                (20, MsgKind::Send, 200, 2_010),
            ],
            false,
        );
        assert!(communication_deadlocks(&cpg).is_empty());
    }

    #[test]
    fn unblocked_alternative_producer_breaks_wait_edge() {
        let cpg = channel_fixture(
            &[
                (10, MsgKind::Recv, 100, 1_000),
                (10, MsgKind::Send, 200, 1_010),
                (20, MsgKind::Recv, 200, 2_000),
                (20, MsgKind::Send, 100, 2_010),
                (30, MsgKind::Send, 100, 3_000),
            ],
            false,
        );
        assert!(communication_deadlocks(&cpg).is_empty());
    }

    #[test]
    fn self_wait_before_own_send_is_a_cycle() {
        let cpg = channel_fixture(
            &[
                (10, MsgKind::Recv, 100, 1_000),
                (10, MsgKind::Send, 100, 1_010),
            ],
            false,
        );
        let findings = communication_deadlocks(&cpg);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].processes, vec![NodeId::new(10)]);
    }

    #[test]
    fn persistent_and_peek_events_do_not_create_linear_waits() {
        let cpg = channel_fixture(
            &[
                (10, MsgKind::RecvPersistent, 100, 1_000),
                (10, MsgKind::Peek, 200, 1_010),
                (20, MsgKind::Send, 100, 2_000),
                (20, MsgKind::Send, 200, 2_010),
            ],
            false,
        );
        assert!(communication_deadlocks(&cpg).is_empty());
    }

    #[test]
    fn ab_ba_lock_order_cycle_is_detected() {
        let findings = communication_deadlocks(&lock_fixture());
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, DeadlockFindingKind::LockOrderCycle);
        assert_eq!(
            findings[0].resources,
            vec![NodeId::new(100), NodeId::new(200)]
        );
        assert_eq!(findings[0].witnesses.len(), 2);
    }

    #[test]
    fn consistent_lock_order_is_acyclic() {
        let mut cpg = lock_fixture();
        cpg.node_mut(NodeId::new(2_000))
            .expect("first lock")
            .lock_kind = Some(LockKind::AcquireWrite);
        cpg.node_mut(NodeId::new(2_001))
            .expect("first lock arg")
            .kind = CpgNodeKind::Identifier {
            name: "r100".into(),
            definition: Some(NodeId::new(100)),
        };
        cpg.node_mut(NodeId::new(2_010))
            .expect("second lock")
            .lock_kind = Some(LockKind::AcquireWrite);
        cpg.node_mut(NodeId::new(2_011))
            .expect("second lock arg")
            .kind = CpgNodeKind::Identifier {
            name: "r200".into(),
            definition: Some(NodeId::new(200)),
        };
        assert!(communication_deadlocks(&cpg).is_empty());
    }

    #[test]
    fn release_removes_lock_from_held_stack() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        for node in [function(10), function(20), variable(100), variable(200)] {
            cpg.add_node_with_id(node);
        }
        add_lock_event(&mut cpg, 10, 1_000, 100, LockKind::AcquireWrite);
        add_lock_event(&mut cpg, 10, 1_010, 100, LockKind::Release);
        add_lock_event(&mut cpg, 10, 1_020, 200, LockKind::AcquireWrite);
        add_lock_event(&mut cpg, 20, 2_000, 200, LockKind::AcquireWrite);
        add_lock_event(&mut cpg, 20, 2_010, 100, LockKind::AcquireWrite);
        assert!(communication_deadlocks(&cpg).is_empty());
    }

    #[test]
    fn unresolved_release_clears_held_state_conservatively() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        for node in [function(10), function(20), variable(100), variable(200)] {
            cpg.add_node_with_id(node);
        }
        add_lock_event(&mut cpg, 10, 1_000, 100, LockKind::AcquireWrite);
        cpg.add_node_with_id(lock_call(1_010, LockKind::Release));
        cpg.connect(NodeId::new(10), NodeId::new(1_010), CpgEdgeKind::AstChild);
        add_lock_event(&mut cpg, 10, 1_020, 200, LockKind::AcquireWrite);
        add_lock_event(&mut cpg, 20, 2_000, 200, LockKind::AcquireWrite);
        add_lock_event(&mut cpg, 20, 2_010, 100, LockKind::AcquireWrite);
        assert!(communication_deadlocks(&cpg).is_empty());
    }

    #[test]
    fn all_read_lock_cycle_is_not_a_deadlock_candidate() {
        let mut cpg = lock_fixture();
        for site in [1_000, 1_010, 2_000, 2_010] {
            cpg.node_mut(NodeId::new(site)).expect("lock").lock_kind = Some(LockKind::AcquireRead);
        }
        assert!(communication_deadlocks(&cpg).is_empty());
    }

    #[test]
    fn mixed_read_write_lock_cycle_is_a_candidate() {
        let mut cpg = lock_fixture();
        cpg.node_mut(NodeId::new(1_000)).expect("lock").lock_kind = Some(LockKind::AcquireRead);
        cpg.node_mut(NodeId::new(1_010)).expect("lock").lock_kind = Some(LockKind::AcquireRead);
        let findings = communication_deadlocks(&cpg);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, DeadlockFindingKind::LockOrderCycle);
    }

    #[test]
    fn resolved_calls_contribute_transitive_acquisition_summaries() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        for node in [
            function(10),
            function(20),
            function(30),
            function(40),
            variable(100),
            variable(200),
        ] {
            cpg.add_node_with_id(node);
        }
        add_lock_event(&mut cpg, 10, 1_000, 100, LockKind::AcquireWrite);
        cpg.add_node_with_id(ordinary_call(1_010, 30));
        cpg.connect(NodeId::new(10), NodeId::new(1_010), CpgEdgeKind::AstChild);
        add_lock_event(&mut cpg, 30, 3_000, 200, LockKind::AcquireWrite);

        add_lock_event(&mut cpg, 20, 2_000, 200, LockKind::AcquireWrite);
        cpg.add_node_with_id(ordinary_call(2_010, 40));
        cpg.connect(NodeId::new(20), NodeId::new(2_010), CpgEdgeKind::AstChild);
        add_lock_event(&mut cpg, 40, 4_000, 100, LockKind::AcquireWrite);

        let findings = communication_deadlocks(&cpg);
        assert_eq!(findings.len(), 1);
        assert!(findings[0].witnesses.iter().all(|witness| matches!(
            witness,
            DeadlockWitness::LockOrder {
                via_call: Some(_),
                ..
            }
        )));
    }

    #[test]
    fn transitive_recursive_call_summaries_reach_fixpoint() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        for node in [function(10), function(20), variable(100), variable(200)] {
            cpg.add_node_with_id(node);
        }
        cpg.add_node_with_id(ordinary_call(1_000, 20));
        cpg.connect(NodeId::new(10), NodeId::new(1_000), CpgEdgeKind::AstChild);
        cpg.add_node_with_id(ordinary_call(2_000, 10));
        cpg.connect(NodeId::new(20), NodeId::new(2_000), CpgEdgeKind::AstChild);
        add_lock_event(&mut cpg, 20, 2_010, 100, LockKind::AcquireWrite);
        let events = extract_events(&cpg);
        let processes: Vec<_> = events.keys().copied().collect();
        let summaries = reachable_acquisitions(&lock_skeletons(&cpg, &processes));
        assert!(summaries[&NodeId::new(10)].contains_key(&NodeId::new(100)));
        assert!(summaries[&NodeId::new(20)].contains_key(&NodeId::new(100)));
    }

    #[test]
    fn findings_are_read_only_and_all_witness_nodes_exist() {
        let cpg = lock_fixture();
        let before = (cpg.node_count(), cpg.edge_count());
        let findings = communication_deadlocks(&cpg);
        assert_eq!(before, (cpg.node_count(), cpg.edge_count()));
        assert!(findings.iter().all(|finding| {
            finding.processes.iter().all(|id| cpg.contains_node(*id))
                && finding.resources.iter().all(|id| cpg.contains_node(*id))
                && finding.witnesses.iter().all(|witness| match witness {
                    DeadlockWitness::Communication {
                        wait_site,
                        producer_site,
                        ..
                    } => cpg.contains_node(*wait_site) && cpg.contains_node(*producer_site),
                    DeadlockWitness::LockOrder {
                        held_site,
                        acquired_site,
                        via_call,
                        ..
                    } => {
                        cpg.contains_node(*held_site)
                            && cpg.contains_node(*acquired_site)
                            && via_call.is_none_or(|site| cpg.contains_node(site))
                    }
                })
        }));
    }

    #[test]
    fn finding_kind_strings_are_stable() {
        assert_eq!(
            DeadlockFindingKind::CommunicationCycle.as_str(),
            "communication_cycle"
        );
        assert_eq!(
            DeadlockFindingKind::LockOrderCycle.as_str(),
            "lock_order_cycle"
        );
    }

    #[cfg(feature = "serde")]
    #[test]
    fn findings_round_trip_through_serde() {
        let findings = communication_deadlocks(&lock_fixture());
        let json = serde_json::to_string(&findings).expect("serialize deadlocks");
        let round_trip: Vec<DeadlockFinding> =
            serde_json::from_str(&json).expect("deserialize deadlocks");
        assert_eq!(round_trip, findings);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn communication_findings_ignore_node_and_edge_insertion_order(reverse in any::<bool>()) {
            let mut spec = vec![
                (10, MsgKind::Recv, 100, 1_000),
                (10, MsgKind::Send, 200, 1_010),
                (20, MsgKind::Recv, 200, 2_000),
                (20, MsgKind::Send, 100, 2_010),
            ];
            let expected = communication_deadlocks(&channel_fixture(&spec, false));
            if reverse {
                spec.reverse();
            }
            prop_assert_eq!(communication_deadlocks(&channel_fixture(&spec, reverse)), expected);
        }

        #[test]
        fn lock_findings_ignore_node_and_edge_insertion_order(reverse in any::<bool>()) {
            let expected = communication_deadlocks(&lock_fixture());
            prop_assert_eq!(
                communication_deadlocks(&lock_fixture_with_reversed_edges(reverse)),
                expected,
            );
        }

        #[test]
        fn generated_ring_is_cyclic_exactly_when_every_process_receives_first(
            process_count in 2u32..8,
            breaker in proptest::option::of(0u32..8),
        ) {
            let breaker = breaker.filter(|index| *index < process_count);
            let mut spec = Vec::new();
            for index in 0..process_count {
                let process = 10 + index;
                let waits_on = 100 + index;
                let produces = 100 + (index + 1) % process_count;
                let base = 1_000 + index * 100;
                if breaker == Some(index) {
                    spec.push((process, MsgKind::Send, produces, base));
                    spec.push((process, MsgKind::Recv, waits_on, base + 10));
                } else {
                    spec.push((process, MsgKind::Recv, waits_on, base));
                    spec.push((process, MsgKind::Send, produces, base + 10));
                }
            }
            let findings = communication_deadlocks(&channel_fixture(&spec, false));
            prop_assert_eq!(
                findings.iter().any(|finding| finding.kind == DeadlockFindingKind::CommunicationCycle),
                breaker.is_none(),
            );
        }
    }
}
