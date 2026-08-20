//! Advisory read-modify-write atomicity candidates.

use std::collections::{BTreeMap, BTreeSet};

use crate::{CodePropertyGraph, NodeId, SourceRange};

use super::{
    evidence::{collect_evidence, source_key_from_range, LocationAccess, LockBoundary},
    race::{has_common_excluding_lock, HeldLock},
    ConcurrentAccessKind, MhpRelation,
};

/// Evidence supporting one read-modify-write atomicity candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AtomicityWitness {
    /// Field/member or channel expression underlying `read`.
    pub read_expression: NodeId,
    /// Field/member or channel expression underlying `write`.
    pub write_expression: NodeId,
    /// Parallel access that can enter between the two critical sections.
    pub interfering_access: NodeId,
    /// Expression underlying `interfering_access`.
    pub interfering_expression: NodeId,
    /// Typed behavior of the interfering access.
    pub interfering_kind: ConcurrentAccessKind,
    /// Lock released after `read`.
    pub boundary_lock: NodeId,
    /// Release operation ending the first critical section.
    pub release_site: NodeId,
    /// Source range of `release_site`.
    pub release_range: SourceRange,
    /// Acquisition beginning the second critical section.
    pub reacquire_site: NodeId,
    /// Source range of `reacquire_site`.
    pub reacquire_range: SourceRange,
    /// Locks held at `read`.
    pub read_locks: Vec<HeldLock>,
    /// Locks held at `write`.
    pub write_locks: Vec<HeldLock>,
    /// Locks held at `interfering_access`.
    pub interfering_locks: Vec<HeldLock>,
    /// Whether the supplied MHP relation was truncated at its public cap.
    pub mhp_capped: bool,
}

/// A read and later write split across a lock boundary with possible interference.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct AtomicityCandidate {
    /// Read site beginning the logical update.
    pub read: NodeId,
    /// Write site completing the logical update.
    pub write: NodeId,
    /// Exact referenced field or channel definition.
    pub location: NodeId,
    /// Typed lock-boundary and interfering-access evidence.
    pub witness: AtomicityWitness,
}

/// Reports read-modify-write updates split across a released/reacquired lock.
///
/// For one execution strand and exact location, the detector examines adjacent
/// accesses in source order. A candidate requires a read followed by a write,
/// a resolved lock held at both endpoints but released and reacquired between
/// them, and a conflicting access that is MHP with both endpoints. A different
/// lock continuously excluding the interfering access suppresses the report.
///
/// The result is deterministic, feature-free, out of graph, and advisory.
/// Missing identities or MHP pairs suppress conclusions; no happens-before or
/// pointer-alias fact is fabricated.
pub fn atomicity_candidates(cpg: &CodePropertyGraph, mhp: &MhpRelation) -> Vec<AtomicityCandidate> {
    let evidence = collect_evidence(cpg);
    let mut by_strand_location = BTreeMap::<(NodeId, NodeId), Vec<usize>>::new();
    let mut by_location = BTreeMap::<NodeId, Vec<usize>>::new();
    for (index, access) in evidence.accesses.iter().enumerate() {
        by_location.entry(access.location).or_default().push(index);
        if let Some(strand) = access.strand {
            by_strand_location
                .entry((strand, access.location))
                .or_default()
                .push(index);
        }
    }
    let mhp_pairs: BTreeSet<_> = mhp
        .pairs
        .iter()
        .filter_map(|pair| super::MhpPair::canonical(pair.first, pair.second))
        .collect();

    let mut candidates = Vec::new();
    for ((strand, location), mut indices) in by_strand_location {
        indices.sort_unstable_by_key(|&index| access_key(&evidence.accesses[index]));
        for pair in indices.windows(2) {
            let read = &evidence.accesses[pair[0]];
            let write = &evidence.accesses[pair[1]];
            if read.site == write.site || !read.kind.reads() || !write.kind.writes() {
                continue;
            }
            let Some((release, reacquire)) = split_boundary(
                read,
                write,
                evidence
                    .boundaries
                    .get(&strand)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
            ) else {
                continue;
            };
            for &interference_index in by_location.get(&location).into_iter().flatten() {
                let interference = &evidence.accesses[interference_index];
                if interference.strand == Some(strand)
                    || !contains(&mhp_pairs, read.site, interference.site)
                    || !contains(&mhp_pairs, write.site, interference.site)
                    || continuously_excluded(read, write, interference)
                {
                    continue;
                }
                candidates.push(AtomicityCandidate {
                    read: read.site,
                    write: write.site,
                    location,
                    witness: AtomicityWitness {
                        read_expression: read.expression,
                        write_expression: write.expression,
                        interfering_access: interference.site,
                        interfering_expression: interference.expression,
                        interfering_kind: interference.kind,
                        boundary_lock: release.resource.expect("split boundary is resolved"),
                        release_site: release.site,
                        release_range: release.range,
                        reacquire_site: reacquire.site,
                        reacquire_range: reacquire.range,
                        read_locks: read.locks.clone(),
                        write_locks: write.locks.clone(),
                        interfering_locks: interference.locks.clone(),
                        mhp_capped: mhp.capped,
                    },
                });
            }
        }
    }

    candidates.sort_unstable_by_key(|candidate| {
        (
            candidate.read,
            candidate.write,
            candidate.location,
            candidate.witness.interfering_access,
            candidate.witness.boundary_lock,
            candidate.witness.release_site,
            candidate.witness.reacquire_site,
        )
    });
    candidates.dedup();
    candidates
}

fn contains(pairs: &BTreeSet<super::MhpPair>, left: NodeId, right: NodeId) -> bool {
    super::MhpPair::canonical(left, right).is_some_and(|pair| pairs.contains(&pair))
}

fn split_boundary<'a>(
    read: &LocationAccess,
    write: &LocationAccess,
    boundaries: &'a [LockBoundary],
) -> Option<(&'a LockBoundary, &'a LockBoundary)> {
    let read_key = access_key(read);
    let write_key = access_key(write);
    let common_resources: Vec<_> = read
        .locks
        .iter()
        .filter(|held| {
            write
                .locks
                .iter()
                .any(|other| other.resource == held.resource)
        })
        .map(|held| held.resource)
        .collect();

    for resource in common_resources {
        let release = boundaries.iter().find(|event| {
            event.kind == crate::LockKind::Release
                && event.resource == Some(resource)
                && boundary_key(event) > read_key
                && boundary_key(event) < write_key
        });
        let Some(release) = release else {
            continue;
        };
        if let Some(reacquire) = boundaries.iter().find(|event| {
            event.kind.acquisition_mode().is_some()
                && event.resource == Some(resource)
                && boundary_key(event) > boundary_key(release)
                && boundary_key(event) < write_key
        }) {
            return Some((release, reacquire));
        }
    }
    None
}

fn continuously_excluded(
    read: &LocationAccess,
    write: &LocationAccess,
    interference: &LocationAccess,
) -> bool {
    let continuous: Vec<_> = read
        .locks
        .iter()
        .filter(|read_lock| {
            write.locks.iter().any(|write_lock| {
                read_lock.resource == write_lock.resource
                    && read_lock.acquisition_site == write_lock.acquisition_site
            })
        })
        .copied()
        .collect();
    has_common_excluding_lock(&continuous, &interference.locks)
}

fn access_key(access: &LocationAccess) -> (u32, u32, u32) {
    source_key_from_range(access.range, access.site)
}

fn boundary_key(boundary: &LockBoundary) -> (u32, u32, u32) {
    source_key_from_range(boundary.range, boundary.site)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CpgEdgeKind, CpgNode, CpgNodeKind, DfgEdgeKind, Language, LockKind, MsgKind, ScopeId,
        Visibility,
    };
    use proptest::prelude::*;

    fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(id), kind, SourceRange::from_bytes(id, id + 1))
    }

    fn block(id: u32, parallel: bool) -> CpgNode {
        let node = node(
            id,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        if parallel {
            node.with_message_kind(MsgKind::Spawn)
        } else {
            node
        }
    }

    fn variable(id: u32) -> CpgNode {
        node(
            id,
            CpgNodeKind::Variable {
                name: format!("v{id}").into(),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: true,
            },
        )
    }

    fn field(id: u32) -> CpgNode {
        node(
            id,
            CpgNodeKind::Field {
                name: format!("f{id}").into(),
                field_type: None,
                visibility: Visibility::Private,
            },
        )
    }

    fn call(id: u32) -> CpgNode {
        node(
            id,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        )
    }

    fn expression(id: u32) -> CpgNode {
        node(
            id,
            CpgNodeKind::MemberAccess {
                member: "value".into(),
            },
        )
    }

    fn identifier(id: u32, definition: u32) -> CpgNode {
        node(
            id,
            CpgNodeKind::Identifier {
                name: format!("v{definition}").into(),
                definition: Some(NodeId::new(definition)),
            },
        )
    }

    fn add_ast(cpg: &mut CodePropertyGraph, parent: u32, child: u32) {
        cpg.connect(
            NodeId::new(parent),
            NodeId::new(child),
            CpgEdgeKind::AstChild,
        );
        cpg.node_mut(NodeId::new(child)).expect("child").parent = Some(NodeId::new(parent));
    }

    fn add_lock(cpg: &mut CodePropertyGraph, strand: u32, site: u32, lock: u32, kind: LockKind) {
        cpg.add_node_with_id(call(site).with_lock_kind(kind));
        cpg.add_node_with_id(identifier(site + 10_000, lock));
        add_ast(cpg, strand, site);
        add_ast(cpg, site, site + 10_000);
    }

    fn add_access(
        cpg: &mut CodePropertyGraph,
        strand: u32,
        site: u32,
        expression_id: u32,
        location: u32,
        kind: DfgEdgeKind,
    ) {
        cpg.add_node_with_id(call(site));
        cpg.add_node_with_id(expression(expression_id));
        add_ast(cpg, strand, site);
        add_ast(cpg, site, expression_id);
        cpg.connect(
            NodeId::new(site),
            NodeId::new(expression_id),
            CpgEdgeKind::DataFlow(kind),
        );
        cpg.connect(
            NodeId::new(expression_id),
            NodeId::new(location),
            CpgEdgeKind::Reference,
        );
    }

    fn split_fixture(include_boundary: bool, include_interference: bool) -> CodePropertyGraph {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        for item in [
            block(1, true),
            block(10, false),
            block(20, false),
            field(100),
            variable(200),
        ] {
            cpg.add_node_with_id(item);
        }
        add_ast(&mut cpg, 1, 10);
        add_ast(&mut cpg, 1, 20);
        add_lock(&mut cpg, 10, 11, 200, LockKind::AcquireWrite);
        add_access(&mut cpg, 10, 12, 1_012, 100, DfgEdgeKind::FieldRead);
        if include_boundary {
            add_lock(&mut cpg, 10, 13, 200, LockKind::Release);
            add_lock(&mut cpg, 10, 14, 200, LockKind::AcquireWrite);
        }
        add_access(&mut cpg, 10, 15, 1_015, 100, DfgEdgeKind::FieldWrite);
        if include_interference {
            add_access(&mut cpg, 20, 22, 1_022, 100, DfgEdgeKind::FieldWrite);
        } else {
            cpg.add_node_with_id(call(22));
            add_ast(&mut cpg, 20, 22);
        }
        cpg
    }

    fn insertion_order_fixture(reverse: bool) -> CodePropertyGraph {
        let mut nodes = vec![
            block(1, true),
            block(10, false),
            block(20, false),
            field(100),
            variable(200),
            call(11).with_lock_kind(LockKind::AcquireWrite),
            identifier(10_011, 200),
            call(12),
            expression(1_012),
            call(13).with_lock_kind(LockKind::Release),
            identifier(10_013, 200),
            call(14).with_lock_kind(LockKind::AcquireWrite),
            identifier(10_014, 200),
            call(15),
            expression(1_015),
            call(22),
            expression(1_022),
        ];
        let mut edges = vec![
            (1, 10, CpgEdgeKind::AstChild),
            (1, 20, CpgEdgeKind::AstChild),
            (10, 11, CpgEdgeKind::AstChild),
            (11, 10_011, CpgEdgeKind::AstChild),
            (10, 12, CpgEdgeKind::AstChild),
            (12, 1_012, CpgEdgeKind::AstChild),
            (12, 1_012, CpgEdgeKind::DataFlow(DfgEdgeKind::FieldRead)),
            (1_012, 100, CpgEdgeKind::Reference),
            (10, 13, CpgEdgeKind::AstChild),
            (13, 10_013, CpgEdgeKind::AstChild),
            (10, 14, CpgEdgeKind::AstChild),
            (14, 10_014, CpgEdgeKind::AstChild),
            (10, 15, CpgEdgeKind::AstChild),
            (15, 1_015, CpgEdgeKind::AstChild),
            (15, 1_015, CpgEdgeKind::DataFlow(DfgEdgeKind::FieldWrite)),
            (1_015, 100, CpgEdgeKind::Reference),
            (20, 22, CpgEdgeKind::AstChild),
            (22, 1_022, CpgEdgeKind::AstChild),
            (22, 1_022, CpgEdgeKind::DataFlow(DfgEdgeKind::FieldWrite)),
            (1_022, 100, CpgEdgeKind::Reference),
        ];
        if reverse {
            nodes.reverse();
            edges.reverse();
        }

        let mut cpg = CodePropertyGraph::new(Language::Rust);
        for node in nodes {
            cpg.add_node_with_id(node);
        }
        for (source, target, kind) in edges {
            let ast = kind == CpgEdgeKind::AstChild;
            cpg.connect(NodeId::new(source), NodeId::new(target), kind);
            if ast {
                cpg.node_mut(NodeId::new(target)).expect("AST child").parent =
                    Some(NodeId::new(source));
            }
        }
        cpg
    }

    #[test]
    fn rmw_split_across_unlock_relock_with_parallel_interference_is_reported() {
        let cpg = split_fixture(true, true);
        let before = (cpg.node_count(), cpg.edge_count());
        let mhp = super::super::may_happen_in_parallel(&cpg);
        let candidates = atomicity_candidates(&cpg, &mhp);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].read, NodeId::new(12));
        assert_eq!(candidates[0].write, NodeId::new(15));
        assert_eq!(candidates[0].location, NodeId::new(100));
        assert_eq!(candidates[0].witness.boundary_lock, NodeId::new(200));
        assert_eq!(candidates[0].witness.release_site, NodeId::new(13));
        assert_eq!(candidates[0].witness.reacquire_site, NodeId::new(14));
        assert_eq!(candidates[0].witness.interfering_access, NodeId::new(22));
        assert_eq!(before, (cpg.node_count(), cpg.edge_count()));
    }

    #[test]
    fn continuous_critical_section_and_absent_interference_are_clean() {
        let continuous = split_fixture(false, true);
        let continuous_mhp = super::super::may_happen_in_parallel(&continuous);
        assert!(atomicity_candidates(&continuous, &continuous_mhp).is_empty());

        let isolated = split_fixture(true, false);
        let isolated_mhp = super::super::may_happen_in_parallel(&isolated);
        assert!(atomicity_candidates(&isolated, &isolated_mhp).is_empty());
    }

    #[test]
    fn incomplete_mhp_evidence_suppresses_the_candidate() {
        let cpg = split_fixture(true, true);
        assert!(atomicity_candidates(&cpg, &MhpRelation::default()).is_empty());
    }

    #[test]
    fn malformed_public_mhp_order_is_normalized_before_membership_queries() {
        let cpg = split_fixture(true, true);
        let relation = super::super::may_happen_in_parallel(&cpg);
        let mhp = MhpRelation {
            pairs: relation
                .pairs
                .iter()
                .rev()
                .map(|pair| super::super::MhpPair {
                    first: pair.second,
                    second: pair.first,
                })
                .collect(),
            capped: relation.capped,
        };
        assert_eq!(atomicity_candidates(&cpg, &mhp).len(), 1);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn candidate_round_trips_through_serde() {
        let cpg = split_fixture(true, true);
        let mhp = super::super::may_happen_in_parallel(&cpg);
        let candidates = atomicity_candidates(&cpg, &mhp);
        let json = serde_json::to_string(&candidates).expect("serialize candidates");
        let round_trip: Vec<AtomicityCandidate> =
            serde_json::from_str(&json).expect("deserialize candidates");
        assert_eq!(round_trip, candidates);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn candidates_are_deterministic_for_graph_insertion_order(
            capped in any::<bool>(),
        ) {
            let forward = insertion_order_fixture(false);
            let reversed = insertion_order_fixture(true);
            let mut forward_mhp = super::super::may_happen_in_parallel(&forward);
            let mut reversed_mhp = super::super::may_happen_in_parallel(&reversed);
            forward_mhp.capped = capped;
            reversed_mhp.capped = capped;
            let forward_candidates = atomicity_candidates(&forward, &forward_mhp);
            let reversed_candidates = atomicity_candidates(&reversed, &reversed_mhp);
            prop_assert_eq!(&reversed_candidates, &forward_candidates);
            prop_assert_eq!(forward_candidates.len(), 1);
            prop_assert_eq!(forward_candidates[0].read, NodeId::new(12));
            prop_assert_eq!(forward_candidates[0].write, NodeId::new(15));
            prop_assert_eq!(forward_candidates[0].witness.interfering_access, NodeId::new(22));
            prop_assert_eq!(forward_candidates[0].witness.mhp_capped, capped);
        }
    }
}
