//! Shared, exact evidence extraction for advisory concurrency detectors.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use rustc_hash::FxHashSet;

use crate::{
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, DfgEdgeKind, LockKind, MsgKind, NodeId,
    SourceRange,
};

use super::{
    events::resolve_event_variable, mhp::is_execution_site, ConcurrentAccessKind, HeldLock,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocationAccess {
    pub(crate) site: NodeId,
    pub(crate) expression: NodeId,
    pub(crate) location: NodeId,
    pub(crate) kind: ConcurrentAccessKind,
    pub(crate) range: SourceRange,
    pub(crate) strand: Option<NodeId>,
    pub(crate) locks: Vec<HeldLock>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LockBoundary {
    pub(crate) resource: Option<NodeId>,
    pub(crate) kind: LockKind,
    pub(crate) site: NodeId,
    pub(crate) range: SourceRange,
}

#[derive(Debug, Default)]
pub(crate) struct ConcurrencyEvidence {
    pub(crate) accesses: Vec<LocationAccess>,
    pub(crate) by_site: BTreeMap<NodeId, Vec<usize>>,
    pub(crate) boundaries: BTreeMap<NodeId, Vec<LockBoundary>>,
}

#[derive(Debug, Clone, Copy, Default)]
struct AccessBits {
    reads: bool,
    writes: bool,
}

#[derive(Debug, Clone, Copy)]
struct RawAccess {
    site: NodeId,
    expression: NodeId,
    location: NodeId,
    range: SourceRange,
    strand: Option<NodeId>,
    bits: AccessBits,
}

#[derive(Debug, Clone, Copy)]
enum TimelineItem {
    Access(usize),
    Lock(LockBoundary),
}

pub(crate) fn collect_evidence(cpg: &CodePropertyGraph) -> ConcurrencyEvidence {
    let mut raw = BTreeMap::<(NodeId, NodeId), RawAccess>::new();
    collect_field_accesses(cpg, &mut raw);
    collect_channel_accesses(cpg, &mut raw);

    let mut accesses: Vec<_> = raw
        .into_values()
        .filter_map(|access| {
            let kind = ConcurrentAccessKind::from_bits(access.bits.reads, access.bits.writes)?;
            Some(LocationAccess {
                site: access.site,
                expression: access.expression,
                location: access.location,
                kind,
                range: access.range,
                strand: access.strand,
                locks: Vec::new(),
            })
        })
        .collect();
    accesses.sort_unstable_by_key(|access| (access.site, access.location, access.expression));

    let boundaries = collect_lock_boundaries(cpg);
    attach_locksets(&mut accesses, &boundaries);

    let mut by_site = BTreeMap::<NodeId, Vec<usize>>::new();
    for (index, access) in accesses.iter().enumerate() {
        by_site.entry(access.site).or_default().push(index);
    }

    ConcurrencyEvidence {
        accesses,
        by_site,
        boundaries,
    }
}

fn collect_field_accesses(
    cpg: &CodePropertyGraph,
    raw: &mut BTreeMap<(NodeId, NodeId), RawAccess>,
) {
    for edge in cpg.edges() {
        let (reads, writes) = match edge.kind {
            CpgEdgeKind::DataFlow(DfgEdgeKind::FieldRead | DfgEdgeKind::IndexRead) => (true, false),
            CpgEdgeKind::DataFlow(DfgEdgeKind::FieldWrite | DfgEdgeKind::IndexWrite) => {
                (false, true)
            }
            _ => continue,
        };
        let source_location = exact_referenced_location(cpg, edge.source);
        let target_location = exact_referenced_location(cpg, edge.target);
        let (expression, location) = match (source_location, target_location) {
            (Some(left), Some(right)) if left != right => continue,
            (Some(location), _) => (edge.source, location),
            (_, Some(location)) => (edge.target, location),
            (None, None) => continue,
        };
        let Some(site) = nearest_execution_site(cpg, expression) else {
            continue;
        };
        insert_raw_access(raw, cpg, site, expression, location, reads, writes);
    }
}

fn collect_channel_accesses(
    cpg: &CodePropertyGraph,
    raw: &mut BTreeMap<(NodeId, NodeId), RawAccess>,
) {
    for node in cpg.nodes() {
        let Some(kind) = node.message_kind else {
            continue;
        };
        let (reads, writes) = match kind {
            MsgKind::Send | MsgKind::SendSync => (false, true),
            MsgKind::Recv | MsgKind::RecvPersistent | MsgKind::Peek => (true, false),
            MsgKind::Select | MsgKind::Spawn => continue,
        };
        let Some(location) = resolve_event_variable(cpg, node.id) else {
            continue;
        };
        insert_raw_access(raw, cpg, node.id, node.id, location, reads, writes);
    }
}

fn insert_raw_access(
    raw: &mut BTreeMap<(NodeId, NodeId), RawAccess>,
    cpg: &CodePropertyGraph,
    site: NodeId,
    expression: NodeId,
    location: NodeId,
    reads: bool,
    writes: bool,
) {
    let Some(site_node) = cpg.node(site) else {
        return;
    };
    let strand = execution_strand(cpg, site);
    raw.entry((site, location))
        .and_modify(|existing| {
            existing.bits.reads |= reads;
            existing.bits.writes |= writes;
            existing.expression = existing.expression.min(expression);
        })
        .or_insert(RawAccess {
            site,
            expression,
            location,
            range: site_node.range,
            strand,
            bits: AccessBits { reads, writes },
        });
}

fn exact_referenced_location(cpg: &CodePropertyGraph, expression: NodeId) -> Option<NodeId> {
    let mut locations = BTreeSet::new();
    for edge in cpg
        .outgoing_edges(expression)
        .chain(cpg.incoming_edges(expression))
    {
        if edge.kind != CpgEdgeKind::Reference {
            continue;
        }
        for candidate in [edge.source, edge.target] {
            if candidate != expression
                && cpg.node(candidate).is_some_and(|node| {
                    matches!(
                        node.kind,
                        CpgNodeKind::Variable { .. } | CpgNodeKind::Field { .. }
                    )
                })
            {
                locations.insert(candidate);
            }
        }
    }
    (locations.len() == 1)
        .then(|| locations.iter().next().copied())
        .flatten()
}

fn nearest_execution_site(cpg: &CodePropertyGraph, start: NodeId) -> Option<NodeId> {
    nearest_ancestor(cpg, start, |node, _| is_execution_site(node)).map(|(_, node)| node)
}

fn execution_strand(cpg: &CodePropertyGraph, start: NodeId) -> Option<NodeId> {
    nearest_ancestor(cpg, start, |node, id| {
        matches!(node.kind, CpgNodeKind::Function { .. }) || is_parallel_operand(cpg, id)
    })
    .map(|(_, node)| node)
}

fn nearest_ancestor(
    cpg: &CodePropertyGraph,
    start: NodeId,
    mut predicate: impl FnMut(&CpgNode, NodeId) -> bool,
) -> Option<(usize, NodeId)> {
    let mut queue = VecDeque::from([(start, 0usize)]);
    let mut visited = FxHashSet::default();
    let mut best_depth = None;
    let mut matches = Vec::new();

    while let Some((candidate, depth)) = queue.pop_front() {
        if best_depth.is_some_and(|best| depth > best) {
            break;
        }
        if !visited.insert(candidate) {
            continue;
        }
        let Some(node) = cpg.node(candidate) else {
            continue;
        };
        if predicate(node, candidate) {
            best_depth = Some(depth);
            matches.push(candidate);
            continue;
        }
        let mut parents = ast_parents(cpg, candidate);
        parents.sort_unstable();
        queue.extend(parents.into_iter().map(|parent| (parent, depth + 1)));
    }

    matches.sort_unstable_by_key(|id| source_key(cpg, *id));
    match matches.as_slice() {
        [node] => Some((best_depth?, *node)),
        _ => None,
    }
}

fn ast_parents(cpg: &CodePropertyGraph, node: NodeId) -> Vec<NodeId> {
    let mut parents = BTreeSet::new();
    if let Some(parent) = cpg.node(node).and_then(|node| node.parent) {
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

fn is_parallel_operand(cpg: &CodePropertyGraph, node: NodeId) -> bool {
    ast_parents(cpg, node).into_iter().any(|parent| {
        cpg.node(parent).is_some_and(|node| {
            node.message_kind == Some(MsgKind::Spawn)
                && matches!(node.kind, CpgNodeKind::Block { .. })
        })
    })
}

fn collect_lock_boundaries(cpg: &CodePropertyGraph) -> BTreeMap<NodeId, Vec<LockBoundary>> {
    let mut boundaries = BTreeMap::<NodeId, Vec<LockBoundary>>::new();
    for node in cpg.nodes() {
        let Some(kind) = node.lock_kind else {
            continue;
        };
        let Some(strand) = execution_strand(cpg, node.id) else {
            continue;
        };
        boundaries.entry(strand).or_default().push(LockBoundary {
            resource: resolve_lock_resource(cpg, node.id),
            kind,
            site: node.id,
            range: node.range,
        });
    }
    for events in boundaries.values_mut() {
        events.sort_unstable_by_key(|event| {
            (source_key_from_range(event.range, event.site), event.kind)
        });
        events.dedup();
    }
    boundaries
}

fn resolve_lock_resource(cpg: &CodePropertyGraph, site: NodeId) -> Option<NodeId> {
    let mut resources = BTreeSet::new();
    if let Some(resource) = resolve_event_variable(cpg, site) {
        resources.insert(resource);
    }

    let mut visited = FxHashSet::default();
    let mut stack = cpg.ast_children(site);
    while let Some(candidate) = stack.pop() {
        if !visited.insert(candidate) {
            continue;
        }
        let Some(node) = cpg.node(candidate) else {
            continue;
        };
        if node.lock_kind.is_some() || node.message_kind.is_some() {
            continue;
        }
        if let CpgNodeKind::Identifier {
            definition: Some(definition),
            ..
        } = &node.kind
        {
            if is_location(cpg, *definition) {
                resources.insert(*definition);
            }
        }
        if let Some(resource) = exact_referenced_location(cpg, candidate) {
            resources.insert(resource);
        }
        for definition in cpg.reaching_definitions(candidate) {
            if is_location(cpg, definition) {
                resources.insert(definition);
            }
        }
        stack.extend(cpg.ast_children(candidate));
    }

    (resources.len() == 1)
        .then(|| resources.iter().next().copied())
        .flatten()
}

fn is_location(cpg: &CodePropertyGraph, node: NodeId) -> bool {
    cpg.node(node).is_some_and(|node| {
        matches!(
            node.kind,
            CpgNodeKind::Variable { .. } | CpgNodeKind::Field { .. }
        )
    })
}

fn attach_locksets(
    accesses: &mut [LocationAccess],
    boundaries: &BTreeMap<NodeId, Vec<LockBoundary>>,
) {
    let mut by_strand = BTreeMap::<NodeId, Vec<usize>>::new();
    for (index, access) in accesses.iter().enumerate() {
        if let Some(strand) = access.strand {
            by_strand.entry(strand).or_default().push(index);
        }
    }

    for (strand, access_indices) in by_strand {
        let mut timeline = Vec::new();
        timeline.extend(access_indices.into_iter().map(TimelineItem::Access));
        timeline.extend(
            boundaries
                .get(&strand)
                .into_iter()
                .flatten()
                .copied()
                .map(TimelineItem::Lock),
        );
        timeline.sort_unstable_by_key(|item| match item {
            TimelineItem::Access(index) => (
                source_key_from_range(accesses[*index].range, accesses[*index].site),
                1,
            ),
            TimelineItem::Lock(boundary) => {
                (source_key_from_range(boundary.range, boundary.site), 0)
            }
        });

        let mut held = Vec::<HeldLock>::new();
        for item in timeline {
            match item {
                TimelineItem::Access(index) => {
                    let mut snapshot = held.clone();
                    snapshot.sort_unstable_by_key(|lock| {
                        (lock.resource, lock.mode, lock.acquisition_site)
                    });
                    accesses[index].locks = snapshot;
                }
                TimelineItem::Lock(boundary) => match boundary.kind {
                    LockKind::AcquireRead | LockKind::AcquireWrite => {
                        let (Some(resource), Some(mode)) =
                            (boundary.resource, boundary.kind.acquisition_mode())
                        else {
                            continue;
                        };
                        held.push(HeldLock {
                            resource,
                            mode,
                            acquisition_site: boundary.site,
                            acquisition_range: boundary.range,
                        });
                    }
                    LockKind::Release => match boundary.resource {
                        Some(resource) => {
                            if let Some(position) =
                                held.iter().rposition(|lock| lock.resource == resource)
                            {
                                held.remove(position);
                            }
                        }
                        None => held.clear(),
                    },
                },
            }
        }
    }
}

pub(crate) fn source_key(cpg: &CodePropertyGraph, node: NodeId) -> (u32, u32, u32) {
    cpg.node(node)
        .map(|node| source_key_from_range(node.range, node.id))
        .unwrap_or((u32::MAX, u32::MAX, node.as_u32()))
}

pub(crate) fn source_key_from_range(range: SourceRange, node: NodeId) -> (u32, u32, u32) {
    (range.start, range.end, node.as_u32())
}
