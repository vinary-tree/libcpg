//! Rholang fresh-name provenance and obligation/capability deadlock analysis.
//!
//! The frontend supplies normalized message and reflective-name metadata. This
//! module propagates fresh-name origins through typed definition evidence and
//! matched communication payloads with an explicit heap worklist. It then
//! builds channel-level ordering constraints and delegates cycle recognition to
//! the crate's iterative SCC implementation.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque};

use rustc_hash::FxHashSet;

use crate::{
    CodePropertyGraph, CpgEdgeKind, CpgNodeKind, GraphProjection, MsgKind, NameFlowEdgeKind,
    NameOperationKind, NodeId, ProjectionKind, SourceRange,
};

use super::super::projection_sccs;
use super::{extract_events, DeadlockWitness};

/// Default bound on propagated `(fresh origin, reached node)` pairs.
pub const MAX_NAME_FLOW_ORIGIN_ASSIGNMENTS: usize = 2_000_000;
/// Default bound on materializable name-flow relations.
pub const MAX_NAME_FLOW_RELATIONS: usize = 2_000_000;
/// Default bound on matched producer/receiver event pairs.
pub const MAX_NAME_FLOW_COMMUNICATION_MATCHES: usize = 1_000_000;
/// Default bound on channel-level ordering constraints.
pub const MAX_NAME_FLOW_DEPENDENCIES: usize = 1_000_000;
/// Default bound on queue pops and ancestor steps.
pub const MAX_NAME_FLOW_WORK_ITEMS: usize = 4_000_000;

/// Resource limits for name-flow and level inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NameFlowCaps {
    /// Maximum distinct origin assignments retained by the fixed point.
    pub max_origin_assignments: usize,
    /// Maximum distinct typed relations retained for the overlay.
    pub max_relations: usize,
    /// Maximum distinct send/receive matches examined.
    pub max_communication_matches: usize,
    /// Maximum distinct capability-to-obligation constraints.
    pub max_dependencies: usize,
    /// Maximum aggregate queue pops and ancestor visits.
    pub max_work_items: usize,
}

impl Default for NameFlowCaps {
    fn default() -> Self {
        Self {
            max_origin_assignments: MAX_NAME_FLOW_ORIGIN_ASSIGNMENTS,
            max_relations: MAX_NAME_FLOW_RELATIONS,
            max_communication_matches: MAX_NAME_FLOW_COMMUNICATION_MATCHES,
            max_dependencies: MAX_NAME_FLOW_DEPENDENCIES,
            max_work_items: MAX_NAME_FLOW_WORK_ITEMS,
        }
    }
}

/// One fresh-name provenance relation over existing CPG nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NameFlowRelation {
    /// Stable fresh-binder origin.
    pub origin: NodeId,
    /// Existing node reached by the name.
    pub target: NodeId,
    /// Semantic reason the name reaches `target`.
    pub kind: NameFlowEdgeKind,
}

/// Explicit reason a name-flow result is incomplete.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum NameFlowIssue {
    /// Typed definition evidence disagrees for one identifier.
    AmbiguousDefinition {
        /// Identifier with conflicting definitions.
        node: NodeId,
        /// Sorted distinct candidate definitions.
        definitions: Vec<NodeId>,
    },
    /// A fresh binder lacks an enclosing normalized restriction.
    MissingRestriction {
        /// Malformed or hand-built fresh binder.
        binder: NodeId,
    },
    /// A channel-using event has no exact typed channel operand.
    UnresolvedChannel {
        /// Message event lacking exact identity.
        site: NodeId,
    },
    /// A matched communication has different name payload/binder arity.
    PayloadArityMismatch {
        /// Fresh carrier-channel origin.
        channel: NodeId,
        /// Producer event.
        send_site: NodeId,
        /// Receiver event.
        receive_site: NodeId,
        /// Resolved name payload count.
        payloads: usize,
        /// Receive-binder count.
        binders: usize,
    },
    /// The origin-assignment bound was reached.
    OriginAssignmentLimitExceeded {
        /// Configured maximum.
        limit: usize,
    },
    /// The materializable-relation bound was reached.
    RelationLimitExceeded {
        /// Configured maximum.
        limit: usize,
    },
    /// The communication-match bound was reached.
    CommunicationMatchLimitExceeded {
        /// Configured maximum.
        limit: usize,
    },
    /// The aggregate work bound was reached.
    WorkLimitExceeded {
        /// Configured maximum.
        limit: usize,
    },
}

/// Complete or explicitly partial fresh-name fixed point.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NameFlowAnalysis {
    /// Whether no malformed evidence or resource cap affected the result.
    pub complete: bool,
    /// Reached node to sorted fresh origins.
    pub origins: BTreeMap<NodeId, Vec<NodeId>>,
    /// Sorted, deduplicated overlay relations.
    pub relations: Vec<NameFlowRelation>,
    /// Sorted, deduplicated reasons the fixed point is incomplete.
    pub issues: Vec<NameFlowIssue>,
    /// Number of origin assignments processed from the heap worklist.
    pub processed_assignments: usize,
    /// Number of distinct producer/receiver matches installed.
    pub communication_matches: usize,
}

impl NameFlowAnalysis {
    /// Returns the fresh origins reaching `node`, in stable order.
    pub fn origins_of(&self, node: NodeId) -> &[NodeId] {
        self.origins.get(&node).map_or(&[], Vec::as_slice)
    }
}

/// Transactional NameFlow-overlay materialization failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NameFlowOverlayError {
    /// A relation source is absent from the target CPG.
    #[error("NameFlow relation source does not exist: {origin:?}")]
    UnknownOrigin {
        /// Missing fresh-binder origin.
        origin: NodeId,
    },
    /// A relation target is absent from the target CPG.
    #[error("NameFlow relation target does not exist: {target:?}")]
    UnknownTarget {
        /// Missing reached node.
        target: NodeId,
    },
}

/// Canonical inferred levels for one fresh channel.
///
/// A finite value is present only when the complete channel dependency graph
/// is acyclic. Equal canonical capability and obligation ranks satisfy matching
/// co-action consistency; every dependency edge strictly increases the rank.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ChannelUsageLevel {
    /// Fresh channel binder.
    pub channel: NodeId,
    /// Assumed co-action availability level.
    pub capability: Option<u32>,
    /// Promised action-readiness level.
    pub obligation: Option<u32>,
}

/// Explicit reason type-based deadlock analysis is incomplete.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DeadlockFreedomIssue {
    /// Name-flow inference was partial.
    NameFlow(NameFlowIssue),
    /// The channel dependency bound was reached.
    DependencyLimitExceeded {
        /// Configured maximum.
        limit: usize,
    },
}

/// Compositional result of fresh-name, level, and cyclic-wait inference.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DeadlockFreedomAnalysis {
    /// Whether all name and dependency evidence was processed within bounds.
    pub complete: bool,
    /// Fresh-name analysis used to resolve channel mobility.
    pub name_flow: NameFlowAnalysis,
    /// Canonical per-channel levels; cyclic systems carry `None` levels.
    pub levels: Vec<ChannelUsageLevel>,
    /// Channels participating in cyclic SCCs, sorted and deduplicated.
    pub cyclic_channels: Vec<NodeId>,
    /// Statement-precise internal edges of every cyclic SCC.
    pub witnesses: Vec<DeadlockWitness>,
    /// Sorted, deduplicated incompleteness reasons.
    pub issues: Vec<DeadlockFreedomIssue>,
}

#[derive(Debug, Clone)]
struct EventShape {
    process: NodeId,
    site: NodeId,
    range: SourceRange,
    kind: MsgKind,
    channel_operand: Option<NodeId>,
    payloads: Vec<NodeId>,
    binders: Vec<NodeId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct DependencyEdge {
    capability_channel: NodeId,
    obligation_channel: NodeId,
    process: NodeId,
    capability_site: NodeId,
    capability_range: SourceRange,
    obligation_site: NodeId,
    obligation_range: SourceRange,
}

/// Infers fresh-name provenance with default resource limits.
pub fn analyze_name_flow(cpg: &CodePropertyGraph) -> NameFlowAnalysis {
    analyze_name_flow_with_caps(cpg, NameFlowCaps::default())
}

/// Infers fresh-name provenance with caller-selected resource limits.
pub fn analyze_name_flow_with_caps(
    cpg: &CodePropertyGraph,
    caps: NameFlowCaps,
) -> NameFlowAnalysis {
    let mut issues = BTreeSet::new();
    let definitions = definition_evidence(cpg, &mut issues);
    let events = event_shapes(cpg, &definitions, &mut issues);

    let mut direct = BTreeMap::<NodeId, BTreeSet<NodeId>>::new();
    for (&identifier, &definition) in &definitions {
        direct.entry(definition).or_default().insert(identifier);
    }

    let mut channel_watchers = BTreeMap::<NodeId, Vec<usize>>::new();
    let mut payload_watchers = BTreeMap::<NodeId, Vec<usize>>::new();
    for (index, event) in events.iter().enumerate() {
        if let Some(operand) = event.channel_operand {
            channel_watchers.entry(operand).or_default().push(index);
        } else if event.kind.uses_channel() {
            issues.insert(NameFlowIssue::UnresolvedChannel { site: event.site });
        }
        for &payload in &event.payloads {
            payload_watchers.entry(payload).or_default().push(index);
        }
    }

    let mut origins = BTreeMap::<NodeId, BTreeSet<NodeId>>::new();
    let mut relations = BTreeSet::new();
    let mut work = VecDeque::new();
    let mut assignment_count = 0usize;
    let mut processed_assignments = 0usize;
    let mut work_items = 0usize;

    let mut fresh_binders: Vec<_> = cpg
        .nodes()
        .filter(|node| node.name_operation == Some(NameOperationKind::FreshBinder))
        .map(|node| node.id)
        .collect();
    fresh_binders.sort_unstable();

    for binder in fresh_binders {
        assign_origin(
            &mut origins,
            &mut work,
            &mut assignment_count,
            binder,
            binder,
            caps,
            &mut issues,
        );
        match nearest_restriction(cpg, binder, &mut work_items, caps, &mut issues) {
            Some(restriction) => insert_relation(
                &mut relations,
                NameFlowRelation {
                    origin: binder,
                    target: restriction,
                    kind: NameFlowEdgeKind::Creates,
                },
                caps,
                &mut issues,
            ),
            None => {
                issues.insert(NameFlowIssue::MissingRestriction { binder });
            }
        }
    }

    let mut transfers = BTreeMap::<NodeId, BTreeSet<NodeId>>::new();
    let mut registered_sends = BTreeMap::<NodeId, BTreeSet<usize>>::new();
    let mut registered_receives = BTreeMap::<NodeId, BTreeSet<usize>>::new();
    let mut matched = BTreeSet::<(NodeId, usize, usize)>::new();

    while let Some((origin, node)) = work.pop_front() {
        if work_items >= caps.max_work_items {
            issues.insert(NameFlowIssue::WorkLimitExceeded {
                limit: caps.max_work_items,
            });
            break;
        }
        work_items += 1;
        processed_assignments += 1;

        if cpg
            .node(node)
            .is_some_and(|node| matches!(node.kind, CpgNodeKind::Identifier { .. }))
        {
            insert_relation(
                &mut relations,
                NameFlowRelation {
                    origin,
                    target: node,
                    kind: NameFlowEdgeKind::References,
                },
                caps,
                &mut issues,
            );
            for (ancestor, operation) in
                reflective_ancestors(cpg, node, &mut work_items, caps, &mut issues)
            {
                let kind = match operation {
                    NameOperationKind::Quote => NameFlowEdgeKind::Quotes,
                    NameOperationKind::Drop => NameFlowEdgeKind::Drops,
                    NameOperationKind::Restriction | NameOperationKind::FreshBinder => continue,
                };
                insert_relation(
                    &mut relations,
                    NameFlowRelation {
                        origin,
                        target: ancestor,
                        kind,
                    },
                    caps,
                    &mut issues,
                );
            }
        }

        if let Some(targets) = direct.get(&node) {
            for &target in targets {
                assign_origin(
                    &mut origins,
                    &mut work,
                    &mut assignment_count,
                    origin,
                    target,
                    caps,
                    &mut issues,
                );
            }
        }
        if let Some(targets) = transfers.get(&node) {
            for &target in targets {
                insert_relation(
                    &mut relations,
                    NameFlowRelation {
                        origin,
                        target,
                        kind: NameFlowEdgeKind::Receives,
                    },
                    caps,
                    &mut issues,
                );
                assign_origin(
                    &mut origins,
                    &mut work,
                    &mut assignment_count,
                    origin,
                    target,
                    caps,
                    &mut issues,
                );
            }
        }

        if let Some(indices) = payload_watchers.get(&node) {
            for &index in indices {
                insert_relation(
                    &mut relations,
                    NameFlowRelation {
                        origin,
                        target: events[index].site,
                        kind: NameFlowEdgeKind::Carries,
                    },
                    caps,
                    &mut issues,
                );
            }
        }

        let Some(indices) = channel_watchers.get(&node) else {
            continue;
        };
        for &index in indices {
            let event = &events[index];
            insert_relation(
                &mut relations,
                NameFlowRelation {
                    origin,
                    target: event.site,
                    kind: NameFlowEdgeKind::Communicates,
                },
                caps,
                &mut issues,
            );

            let mut pending_matches = Vec::new();
            if event.kind.is_send() {
                if registered_sends.entry(origin).or_default().insert(index) {
                    pending_matches.extend(
                        registered_receives
                            .get(&origin)
                            .into_iter()
                            .flatten()
                            .map(|&receive| (index, receive)),
                    );
                }
            } else if event.kind.is_receive()
                && registered_receives
                    .entry(origin)
                    .or_default()
                    .insert(index)
            {
                pending_matches.extend(
                    registered_sends
                        .get(&origin)
                        .into_iter()
                        .flatten()
                        .map(|&send| (send, index)),
                );
            }

            for (send_index, receive_index) in pending_matches {
                if !matched.insert((origin, send_index, receive_index)) {
                    continue;
                }
                if matched.len() > caps.max_communication_matches {
                    issues.insert(NameFlowIssue::CommunicationMatchLimitExceeded {
                        limit: caps.max_communication_matches,
                    });
                    continue;
                }
                let send = &events[send_index];
                let receive = &events[receive_index];
                if send.payloads.len() != receive.binders.len() {
                    issues.insert(NameFlowIssue::PayloadArityMismatch {
                        channel: origin,
                        send_site: send.site,
                        receive_site: receive.site,
                        payloads: send.payloads.len(),
                        binders: receive.binders.len(),
                    });
                }
                for (&payload, &binder) in send.payloads.iter().zip(&receive.binders) {
                    if !transfers.entry(payload).or_default().insert(binder) {
                        continue;
                    }
                    let payload_origins: Vec<_> = origins
                        .get(&payload)
                        .into_iter()
                        .flatten()
                        .copied()
                        .collect();
                    for payload_origin in payload_origins {
                        insert_relation(
                            &mut relations,
                            NameFlowRelation {
                                origin: payload_origin,
                                target: binder,
                                kind: NameFlowEdgeKind::Receives,
                            },
                            caps,
                            &mut issues,
                        );
                        assign_origin(
                            &mut origins,
                            &mut work,
                            &mut assignment_count,
                            payload_origin,
                            binder,
                            caps,
                            &mut issues,
                        );
                    }
                }
            }
        }
    }

    let origins = origins
        .into_iter()
        .map(|(node, origins)| (node, origins.into_iter().collect()))
        .collect();
    let issues: Vec<_> = issues.into_iter().collect();
    NameFlowAnalysis {
        complete: issues.is_empty(),
        origins,
        relations: relations.into_iter().collect(),
        issues,
        processed_assignments,
        communication_matches: matched.len().min(caps.max_communication_matches),
    }
}

/// Appends a complete or explicitly partial analysis as typed, idempotent edges.
///
/// Endpoint validation is transactional: an invalid relation adds no edge.
pub fn materialize_name_flow_overlay(
    cpg: &mut CodePropertyGraph,
    analysis: &NameFlowAnalysis,
) -> Result<usize, NameFlowOverlayError> {
    for relation in &analysis.relations {
        if !cpg.contains_node(relation.origin) {
            return Err(NameFlowOverlayError::UnknownOrigin {
                origin: relation.origin,
            });
        }
        if !cpg.contains_node(relation.target) {
            return Err(NameFlowOverlayError::UnknownTarget {
                target: relation.target,
            });
        }
    }

    let before = cpg.name_flow_edges().count();
    for relation in &analysis.relations {
        cpg.connect_unique(
            relation.origin,
            relation.target,
            CpgEdgeKind::NameFlow(relation.kind),
        );
    }
    Ok(cpg.name_flow_edges().count().saturating_sub(before))
}

/// Returns cyclic obligation/capability witnesses with default resource limits.
///
/// An empty vector is advisory only. Call [`analyze_deadlock_freedom`] when a
/// consumer must distinguish complete evidence from explicit incompleteness.
pub fn deadlock_freedom(cpg: &CodePropertyGraph) -> Vec<DeadlockWitness> {
    analyze_deadlock_freedom(cpg).witnesses
}

/// Infers name flow, canonical channel levels, and cyclic witnesses.
pub fn analyze_deadlock_freedom(cpg: &CodePropertyGraph) -> DeadlockFreedomAnalysis {
    analyze_deadlock_freedom_with_caps(cpg, NameFlowCaps::default())
}

/// Infers name flow, canonical channel levels, and cyclic witnesses with caps.
pub fn analyze_deadlock_freedom_with_caps(
    cpg: &CodePropertyGraph,
    caps: NameFlowCaps,
) -> DeadlockFreedomAnalysis {
    let name_flow = analyze_name_flow_with_caps(cpg, caps);
    let mut issues: BTreeSet<_> = name_flow
        .issues
        .iter()
        .cloned()
        .map(DeadlockFreedomIssue::NameFlow)
        .collect();
    let definitions = definition_evidence(cpg, &mut BTreeSet::new());
    let events = event_shapes(cpg, &definitions, &mut BTreeSet::new());
    let mut by_process = BTreeMap::<NodeId, Vec<usize>>::new();
    for (index, event) in events.iter().enumerate() {
        by_process.entry(event.process).or_default().push(index);
    }

    let mut dependencies = BTreeSet::new();
    'processes: for indices in by_process.values() {
        for (position, &capability_index) in indices.iter().enumerate() {
            let capability_event = &events[capability_index];
            if capability_event.kind != MsgKind::Recv {
                continue;
            }
            let capability_origins = capability_event
                .channel_operand
                .map(|operand| name_flow.origins_of(operand))
                .unwrap_or(&[]);
            for &obligation_index in &indices[position + 1..] {
                let obligation_event = &events[obligation_index];
                if !obligation_event.kind.is_send() {
                    continue;
                }
                let obligation_origins = obligation_event
                    .channel_operand
                    .map(|operand| name_flow.origins_of(operand))
                    .unwrap_or(&[]);
                for &capability_channel in capability_origins {
                    for &obligation_channel in obligation_origins {
                        dependencies.insert(DependencyEdge {
                            capability_channel,
                            obligation_channel,
                            process: capability_event.process,
                            capability_site: capability_event.site,
                            capability_range: capability_event.range,
                            obligation_site: obligation_event.site,
                            obligation_range: obligation_event.range,
                        });
                        if dependencies.len() > caps.max_dependencies {
                            issues.insert(DeadlockFreedomIssue::DependencyLimitExceeded {
                                limit: caps.max_dependencies,
                            });
                            break 'processes;
                        }
                    }
                }
            }
        }
    }

    let dependencies: Vec<_> = dependencies
        .into_iter()
        .take(caps.max_dependencies)
        .collect();
    let mut channels: BTreeSet<_> = cpg
        .nodes()
        .filter(|node| node.name_operation == Some(NameOperationKind::FreshBinder))
        .map(|node| node.id)
        .collect();
    for edge in &dependencies {
        channels.extend([edge.capability_channel, edge.obligation_channel]);
    }
    let projection = GraphProjection::from_edges(
        ProjectionKind::External,
        channels.iter().copied(),
        dependencies
            .iter()
            .map(|edge| (edge.capability_channel, edge.obligation_channel, 1.0)),
    );
    let decomposition = projection_sccs(&projection);
    let cyclic_channels: BTreeSet<_> = decomposition
        .cyclic_components()
        .flat_map(|component| component.nodes.iter().copied())
        .collect();

    let mut witnesses = Vec::new();
    for component in decomposition.cyclic_components() {
        let members: BTreeSet<_> = component.nodes.iter().copied().collect();
        witnesses.extend(
            dependencies
                .iter()
                .filter(|edge| {
                    members.contains(&edge.capability_channel)
                        && members.contains(&edge.obligation_channel)
                })
                .map(|edge| DeadlockWitness::ObligationCapability {
                    process: edge.process,
                    capability_channel: edge.capability_channel,
                    obligation_channel: edge.obligation_channel,
                    capability_site: edge.capability_site,
                    capability_range: edge.capability_range,
                    obligation_site: edge.obligation_site,
                    obligation_range: edge.obligation_range,
                }),
        );
    }

    let ranks = decomposition
        .is_acyclic()
        .then(|| canonical_ranks(&channels, &dependencies));
    let levels = channels
        .into_iter()
        .map(|channel| {
            let rank = ranks
                .as_ref()
                .and_then(|ranks| ranks.get(&channel))
                .copied();
            ChannelUsageLevel {
                channel,
                capability: rank,
                obligation: rank,
            }
        })
        .collect();
    let issues: Vec<_> = issues.into_iter().collect();

    DeadlockFreedomAnalysis {
        complete: name_flow.complete && issues.is_empty(),
        name_flow,
        levels,
        cyclic_channels: cyclic_channels.into_iter().collect(),
        witnesses,
        issues,
    }
}

fn definition_evidence(
    cpg: &CodePropertyGraph,
    issues: &mut BTreeSet<NameFlowIssue>,
) -> BTreeMap<NodeId, NodeId> {
    let mut identifiers: Vec<_> = cpg
        .nodes()
        .filter(|node| matches!(node.kind, CpgNodeKind::Identifier { .. }))
        .map(|node| node.id)
        .collect();
    identifiers.sort_unstable_by_key(|&node| source_key(cpg, node));

    let mut result = BTreeMap::new();
    for identifier in identifiers {
        let mut candidates = BTreeSet::new();
        if let Some(CpgNodeKind::Identifier {
            definition: Some(definition),
            ..
        }) = cpg.node(identifier).map(|node| &node.kind)
        {
            candidates.insert(*definition);
        }
        candidates.extend(cpg.reaching_definitions(identifier));
        for edge in cpg
            .outgoing_edges(identifier)
            .chain(cpg.incoming_edges(identifier))
        {
            if edge.kind != CpgEdgeKind::Reference {
                continue;
            }
            candidates.extend([edge.source, edge.target]);
        }
        candidates.remove(&identifier);
        candidates.retain(|candidate| {
            cpg.node(*candidate).is_some_and(|node| {
                matches!(
                    node.kind,
                    CpgNodeKind::Variable { .. } | CpgNodeKind::Parameter { .. }
                )
            })
        });
        match candidates.len() {
            0 => {}
            1 => {
                result.insert(
                    identifier,
                    *candidates
                        .first()
                        .expect("a singleton definition set has one member"),
                );
            }
            _ => {
                issues.insert(NameFlowIssue::AmbiguousDefinition {
                    node: identifier,
                    definitions: candidates.into_iter().collect(),
                });
            }
        }
    }
    result
}

fn event_shapes(
    cpg: &CodePropertyGraph,
    definitions: &BTreeMap<NodeId, NodeId>,
    issues: &mut BTreeSet<NameFlowIssue>,
) -> Vec<EventShape> {
    let mut result = Vec::new();
    for (process, events) in extract_events(cpg) {
        for event in events {
            let mut stack = cpg.ast_children(event.site).to_vec();
            stack.reverse();
            let mut visited = FxHashSet::default();
            let mut identifiers = Vec::new();
            let mut binders = Vec::new();
            while let Some(node) = stack.pop() {
                if !visited.insert(node) {
                    continue;
                }
                let Some(payload) = cpg.node(node) else {
                    continue;
                };
                if payload.message_kind.is_some() || payload.lock_kind.is_some() {
                    continue;
                }
                match payload.kind {
                    CpgNodeKind::Identifier { .. } if definitions.contains_key(&node) => {
                        identifiers.push(node);
                    }
                    CpgNodeKind::Parameter { .. } => binders.push(node),
                    _ => {}
                }
                let mut children = cpg.ast_children(node).to_vec();
                children.reverse();
                stack.extend(children);
            }
            identifiers.sort_unstable_by_key(|&node| source_key(cpg, node));
            identifiers.dedup();
            binders.sort_unstable_by_key(|&node| source_key(cpg, node));
            binders.dedup();
            let channel_operand = event
                .kind
                .uses_channel()
                .then(|| {
                    identifiers.first().copied().or_else(|| {
                        issues.insert(NameFlowIssue::UnresolvedChannel { site: event.site });
                        None
                    })
                })
                .flatten();
            let payloads = if event.kind.is_send() {
                identifiers
                    .into_iter()
                    .filter(|identifier| Some(*identifier) != channel_operand)
                    .collect()
            } else {
                Vec::new()
            };
            result.push(EventShape {
                process,
                site: event.site,
                range: event.range,
                kind: event.kind,
                channel_operand,
                payloads,
                binders: if event.kind.is_receive() {
                    binders
                } else {
                    Vec::new()
                },
            });
        }
    }
    result.sort_unstable_by_key(|event| (event.process, source_key(cpg, event.site)));
    result
}

fn assign_origin(
    origins: &mut BTreeMap<NodeId, BTreeSet<NodeId>>,
    work: &mut VecDeque<(NodeId, NodeId)>,
    assignment_count: &mut usize,
    origin: NodeId,
    node: NodeId,
    caps: NameFlowCaps,
    issues: &mut BTreeSet<NameFlowIssue>,
) {
    if origins
        .get(&node)
        .is_some_and(|existing| existing.contains(&origin))
    {
        return;
    }
    if *assignment_count >= caps.max_origin_assignments {
        issues.insert(NameFlowIssue::OriginAssignmentLimitExceeded {
            limit: caps.max_origin_assignments,
        });
        return;
    }
    origins.entry(node).or_default().insert(origin);
    *assignment_count += 1;
    work.push_back((origin, node));
}

fn insert_relation(
    relations: &mut BTreeSet<NameFlowRelation>,
    relation: NameFlowRelation,
    caps: NameFlowCaps,
    issues: &mut BTreeSet<NameFlowIssue>,
) {
    if relations.contains(&relation) {
        return;
    }
    if relations.len() >= caps.max_relations {
        issues.insert(NameFlowIssue::RelationLimitExceeded {
            limit: caps.max_relations,
        });
        return;
    }
    relations.insert(relation);
}

fn nearest_restriction(
    cpg: &CodePropertyGraph,
    start: NodeId,
    work_items: &mut usize,
    caps: NameFlowCaps,
    issues: &mut BTreeSet<NameFlowIssue>,
) -> Option<NodeId> {
    let mut queue: VecDeque<_> = ast_parents(cpg, start)
        .into_iter()
        .map(|parent| (parent, 1usize))
        .collect();
    let mut visited = FxHashSet::default();
    let mut best_depth = None;
    let mut matches = Vec::new();
    while let Some((node, depth)) = queue.pop_front() {
        if best_depth.is_some_and(|best| depth > best) {
            break;
        }
        if !consume_work(work_items, caps, issues) || !visited.insert(node) {
            continue;
        }
        if cpg
            .node(node)
            .is_some_and(|node| node.name_operation == Some(NameOperationKind::Restriction))
        {
            best_depth = Some(depth);
            matches.push(node);
            continue;
        }
        queue.extend(
            ast_parents(cpg, node)
                .into_iter()
                .map(|parent| (parent, depth + 1)),
        );
    }
    matches.sort_unstable_by_key(|&node| source_key(cpg, node));
    matches.first().copied()
}

fn reflective_ancestors(
    cpg: &CodePropertyGraph,
    start: NodeId,
    work_items: &mut usize,
    caps: NameFlowCaps,
    issues: &mut BTreeSet<NameFlowIssue>,
) -> Vec<(NodeId, NameOperationKind)> {
    let mut stack = ast_parents(cpg, start);
    let mut visited = FxHashSet::default();
    let mut result = Vec::new();
    while let Some(node) = stack.pop() {
        if !consume_work(work_items, caps, issues) || !visited.insert(node) {
            continue;
        }
        if let Some(operation @ (NameOperationKind::Quote | NameOperationKind::Drop)) =
            cpg.node(node).and_then(|node| node.name_operation)
        {
            result.push((node, operation));
        }
        stack.extend(ast_parents(cpg, node));
    }
    result.sort_unstable_by_key(|(node, operation)| (source_key(cpg, *node), *operation));
    result.dedup();
    result
}

fn consume_work(
    work_items: &mut usize,
    caps: NameFlowCaps,
    issues: &mut BTreeSet<NameFlowIssue>,
) -> bool {
    if *work_items >= caps.max_work_items {
        issues.insert(NameFlowIssue::WorkLimitExceeded {
            limit: caps.max_work_items,
        });
        return false;
    }
    *work_items += 1;
    true
}

fn ast_parents(cpg: &CodePropertyGraph, node: NodeId) -> Vec<NodeId> {
    let mut parents = BTreeSet::new();
    if let Some(parent) = cpg.ast_parent(node) {
        parents.insert(parent);
    }
    parents.extend(
        cpg.incoming_edges(node)
            .filter_map(|edge| (edge.kind == CpgEdgeKind::AstChild).then_some(edge.source)),
    );
    parents.extend(
        cpg.outgoing_edges(node)
            .filter_map(|edge| (edge.kind == CpgEdgeKind::AstParent).then_some(edge.target)),
    );
    parents.into_iter().collect()
}

fn canonical_ranks(
    channels: &BTreeSet<NodeId>,
    dependencies: &[DependencyEdge],
) -> BTreeMap<NodeId, u32> {
    let mut successors = BTreeMap::<NodeId, BTreeSet<NodeId>>::new();
    let mut indegree: BTreeMap<_, usize> = channels.iter().map(|&node| (node, 0)).collect();
    for edge in dependencies {
        if successors
            .entry(edge.capability_channel)
            .or_default()
            .insert(edge.obligation_channel)
        {
            *indegree.entry(edge.obligation_channel).or_default() += 1;
        }
    }
    let mut ready = BinaryHeap::new();
    for (&node, &degree) in &indegree {
        if degree == 0 {
            ready.push(Reverse(node));
        }
    }
    let mut ranks: BTreeMap<_, u32> = channels.iter().map(|&node| (node, 0)).collect();
    while let Some(Reverse(node)) = ready.pop() {
        let rank = ranks.get(&node).copied().unwrap_or(0);
        for &successor in successors.get(&node).into_iter().flatten() {
            ranks
                .entry(successor)
                .and_modify(|value| *value = (*value).max(rank.saturating_add(1)))
                .or_insert(rank.saturating_add(1));
            let degree = indegree
                .get_mut(&successor)
                .expect("a dependency target has an indegree entry");
            *degree -= 1;
            if *degree == 0 {
                ready.push(Reverse(successor));
            }
        }
    }
    ranks
}

fn source_key(cpg: &CodePropertyGraph, node: NodeId) -> (u32, u32, NodeId) {
    cpg.node(node)
        .map(|node| (node.range.start, node.range.end, node.id))
        .unwrap_or((u32::MAX, u32::MAX, node))
}

#[cfg(test)]
mod tests;
