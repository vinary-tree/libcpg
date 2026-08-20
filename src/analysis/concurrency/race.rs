//! Advisory static data-race candidates over exact CPG evidence.

use std::collections::BTreeSet;

use crate::{CodePropertyGraph, LockMode, NodeId, SourceRange};

use super::{evidence::collect_evidence, MhpRelation};

/// Read/write behavior of one exact shared-location access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum ConcurrentAccessKind {
    /// Reads the location without typed write evidence.
    Read,
    /// Writes the location without typed read evidence.
    Write,
    /// Carries both typed read and write evidence, as in a compound update.
    ReadWrite,
}

impl ConcurrentAccessKind {
    pub(crate) const fn from_bits(reads: bool, writes: bool) -> Option<Self> {
        match (reads, writes) {
            (true, false) => Some(Self::Read),
            (false, true) => Some(Self::Write),
            (true, true) => Some(Self::ReadWrite),
            (false, false) => None,
        }
    }

    /// Returns true when the access can modify its location.
    pub const fn writes(self) -> bool {
        matches!(self, Self::Write | Self::ReadWrite)
    }

    /// Returns true when the access can observe its location.
    pub const fn reads(self) -> bool {
        matches!(self, Self::Read | Self::ReadWrite)
    }
}

/// One typed lock held at an access site.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct HeldLock {
    /// Exact variable/field identity of the lock resource.
    pub resource: NodeId,
    /// Shared or exclusive acquisition mode.
    pub mode: LockMode,
    /// Typed acquisition call that introduced this held-lock instance.
    pub acquisition_site: NodeId,
    /// Source range of `acquisition_site`.
    pub acquisition_range: SourceRange,
}

/// Evidence supporting one advisory data-race candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RaceWitness {
    /// Field/member or channel expression underlying `access_a`.
    pub expression_a: NodeId,
    /// Field/member or channel expression underlying `access_b`.
    pub expression_b: NodeId,
    /// Typed access behavior at `access_a`.
    pub kind_a: ConcurrentAccessKind,
    /// Typed access behavior at `access_b`.
    pub kind_b: ConcurrentAccessKind,
    /// Source range of `access_a`.
    pub range_a: SourceRange,
    /// Source range of `access_b`.
    pub range_b: SourceRange,
    /// Locks held at `access_a`, sorted by exact identity and acquisition.
    pub locks_a: Vec<HeldLock>,
    /// Locks held at `access_b`, sorted by exact identity and acquisition.
    pub locks_b: Vec<HeldLock>,
    /// Whether the supplied MHP relation was truncated at its public cap.
    pub mhp_capped: bool,
}

/// Two conflicting exact-location accesses that may happen in parallel.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct RaceCandidate {
    /// Canonically lower MHP statement site.
    pub access_a: NodeId,
    /// Canonically higher MHP statement site.
    pub access_b: NodeId,
    /// Exact referenced field or channel definition.
    pub location: NodeId,
    /// Typed access, source-range, lockset, and completeness evidence.
    pub witness: RaceWitness,
}

/// Reports exact-identity data-race candidates from a precomputed MHP relation.
///
/// A candidate requires two accesses to the same resolved field or channel,
/// at least one typed write, MHP membership, and no common lock whose modes
/// mutually exclude the two sites. Read/read pairs are clean. Two shared/read
/// acquisitions do not suppress a write because they can coexist.
///
/// The function is deterministic, feature-free, read-only, and advisory. It
/// does not guess locations from names, infer pointer aliases, or reinterpret
/// a capped MHP relation as complete.
pub fn race_candidates(cpg: &CodePropertyGraph, mhp: &MhpRelation) -> Vec<RaceCandidate> {
    let evidence = collect_evidence(cpg);
    let mut candidates = Vec::new();
    let pairs: BTreeSet<_> = mhp
        .pairs
        .iter()
        .filter_map(|pair| super::MhpPair::canonical(pair.first, pair.second))
        .collect();

    for pair in pairs {
        let (Some(left), Some(right)) = (
            evidence.by_site.get(&pair.first),
            evidence.by_site.get(&pair.second),
        ) else {
            continue;
        };
        let mut left_cursor = 0;
        let mut right_cursor = 0;
        while left_cursor < left.len() && right_cursor < right.len() {
            let left_index = left[left_cursor];
            let right_index = right[right_cursor];
            let access_a = &evidence.accesses[left_index];
            let access_b = &evidence.accesses[right_index];
            match access_a.location.cmp(&access_b.location) {
                std::cmp::Ordering::Less => {
                    left_cursor += 1;
                    continue;
                }
                std::cmp::Ordering::Greater => {
                    right_cursor += 1;
                    continue;
                }
                std::cmp::Ordering::Equal => {}
            }
            if (access_a.kind.writes() || access_b.kind.writes())
                && !has_common_excluding_lock(&access_a.locks, &access_b.locks)
            {
                candidates.push(RaceCandidate {
                    access_a: pair.first,
                    access_b: pair.second,
                    location: access_a.location,
                    witness: RaceWitness {
                        expression_a: access_a.expression,
                        expression_b: access_b.expression,
                        kind_a: access_a.kind,
                        kind_b: access_b.kind,
                        range_a: access_a.range,
                        range_b: access_b.range,
                        locks_a: access_a.locks.clone(),
                        locks_b: access_b.locks.clone(),
                        mhp_capped: mhp.capped,
                    },
                });
            }
            left_cursor += 1;
            right_cursor += 1;
        }
    }

    candidates.sort_unstable_by_key(|candidate| {
        (
            candidate.access_a,
            candidate.access_b,
            candidate.location,
            candidate.witness.expression_a,
            candidate.witness.expression_b,
        )
    });
    candidates.dedup();
    candidates
}

pub(crate) fn has_common_excluding_lock(left: &[HeldLock], right: &[HeldLock]) -> bool {
    left.iter().any(|left_lock| {
        right.iter().any(|right_lock| {
            left_lock.resource == right_lock.resource
                && (left_lock.mode == LockMode::Write || right_lock.mode == LockMode::Write)
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CpgEdgeKind, CpgNode, CpgNodeKind, DfgEdgeKind, Language, LockKind, MsgKind, ScopeId,
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

    fn field(id: u32) -> CpgNode {
        node(
            id,
            CpgNodeKind::Field {
                name: format!("field_{id}").into(),
                field_type: None,
                visibility: crate::Visibility::Private,
            },
        )
    }

    fn access_site(id: u32) -> CpgNode {
        node(
            id,
            CpgNodeKind::Assignment {
                operator: "=".into(),
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

    fn lock_call(id: u32, kind: LockKind) -> CpgNode {
        node(
            id,
            CpgNodeKind::Call {
                target: None,
                is_method: true,
            },
        )
        .with_lock_kind(kind)
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
        let operand = site + 10_000;
        cpg.add_node_with_id(lock_call(site, kind));
        cpg.add_node_with_id(identifier(operand, lock));
        add_ast(cpg, strand, site);
        add_ast(cpg, site, operand);
    }

    fn add_access(
        cpg: &mut CodePropertyGraph,
        strand: u32,
        site: u32,
        expression_id: u32,
        location: u32,
        kind: DfgEdgeKind,
    ) {
        cpg.add_node_with_id(access_site(site));
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

    fn two_access_fixture(
        left_kind: DfgEdgeKind,
        right_kind: DfgEdgeKind,
        shared_lock: Option<LockKind>,
    ) -> CodePropertyGraph {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        for item in [
            block(1, true),
            block(10, false),
            block(20, false),
            field(100),
        ] {
            cpg.add_node_with_id(item);
        }
        add_ast(&mut cpg, 1, 10);
        add_ast(&mut cpg, 1, 20);
        if let Some(kind) = shared_lock {
            cpg.add_node_with_id(variable(200));
            add_lock(&mut cpg, 10, 11, 200, kind);
            add_lock(&mut cpg, 20, 21, 200, kind);
        }
        add_access(&mut cpg, 10, 12, 1_012, 100, left_kind);
        add_access(&mut cpg, 20, 22, 1_022, 100, right_kind);
        cpg
    }

    fn generated_fixture(location_count: u32, reverse: bool) -> CodePropertyGraph {
        let mut nodes = vec![
            block(1, true),
            block(10, false),
            block(20, false),
            access_site(12),
            access_site(22),
        ];
        let mut edges = vec![
            (1, 10, CpgEdgeKind::AstChild),
            (1, 20, CpgEdgeKind::AstChild),
            (10, 12, CpgEdgeKind::AstChild),
            (20, 22, CpgEdgeKind::AstChild),
        ];
        for offset in 0..location_count {
            let location = 100 + offset;
            let left_expression = 1_000 + offset;
            let right_expression = 2_000 + offset;
            nodes.extend([
                field(location),
                expression(left_expression),
                expression(right_expression),
            ]);
            edges.extend([
                (12, left_expression, CpgEdgeKind::AstChild),
                (22, right_expression, CpgEdgeKind::AstChild),
                (
                    12,
                    left_expression,
                    CpgEdgeKind::DataFlow(DfgEdgeKind::FieldRead),
                ),
                (
                    22,
                    right_expression,
                    CpgEdgeKind::DataFlow(DfgEdgeKind::FieldWrite),
                ),
                (left_expression, location, CpgEdgeKind::Reference),
                (right_expression, location, CpgEdgeKind::Reference),
            ]);
        }
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
    fn two_parallel_writes_without_a_common_lock_are_reported() {
        let cpg = two_access_fixture(DfgEdgeKind::FieldWrite, DfgEdgeKind::FieldWrite, None);
        let before = (cpg.node_count(), cpg.edge_count());
        let mhp = super::super::may_happen_in_parallel(&cpg);
        let candidates = race_candidates(&cpg, &mhp);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].access_a, NodeId::new(12));
        assert_eq!(candidates[0].access_b, NodeId::new(22));
        assert_eq!(candidates[0].location, NodeId::new(100));
        assert_eq!(candidates[0].witness.kind_a, ConcurrentAccessKind::Write);
        assert_eq!(candidates[0].witness.kind_b, ConcurrentAccessKind::Write);
        assert_eq!(before, (cpg.node_count(), cpg.edge_count()));
    }

    #[test]
    fn common_exclusive_lock_suppresses_the_candidate() {
        let cpg = two_access_fixture(
            DfgEdgeKind::FieldWrite,
            DfgEdgeKind::FieldWrite,
            Some(LockKind::AcquireWrite),
        );
        let mhp = super::super::may_happen_in_parallel(&cpg);
        assert!(race_candidates(&cpg, &mhp).is_empty());
    }

    #[test]
    fn exact_field_lock_identity_also_suppresses_the_candidate() {
        let mut cpg = two_access_fixture(DfgEdgeKind::FieldWrite, DfgEdgeKind::FieldWrite, None);
        cpg.add_node_with_id(field(200));
        add_lock(&mut cpg, 10, 11, 200, LockKind::AcquireWrite);
        add_lock(&mut cpg, 20, 21, 200, LockKind::AcquireWrite);
        let mhp = super::super::may_happen_in_parallel(&cpg);
        assert!(race_candidates(&cpg, &mhp).is_empty());
    }

    #[test]
    fn read_read_is_clean_but_shared_read_locks_do_not_hide_a_write() {
        let reads = two_access_fixture(DfgEdgeKind::FieldRead, DfgEdgeKind::FieldRead, None);
        let reads_mhp = super::super::may_happen_in_parallel(&reads);
        assert!(race_candidates(&reads, &reads_mhp).is_empty());

        let bad_guard = two_access_fixture(
            DfgEdgeKind::FieldRead,
            DfgEdgeKind::FieldWrite,
            Some(LockKind::AcquireRead),
        );
        let bad_guard_mhp = super::super::may_happen_in_parallel(&bad_guard);
        assert_eq!(race_candidates(&bad_guard, &bad_guard_mhp).len(), 1);
    }

    #[test]
    fn ambiguous_reference_identity_is_suppressed_instead_of_name_matched() {
        let mut cpg = two_access_fixture(DfgEdgeKind::FieldWrite, DfgEdgeKind::FieldWrite, None);
        cpg.add_node_with_id(field(101));
        cpg.connect(NodeId::new(1_012), NodeId::new(101), CpgEdgeKind::Reference);
        let mhp = super::super::may_happen_in_parallel(&cpg);
        assert!(race_candidates(&cpg, &mhp).is_empty());
    }

    #[test]
    fn exact_channel_identity_uses_frontend_metadata() {
        let mut cpg = CodePropertyGraph::new(Language::Rholang);
        for item in [
            block(1, true),
            block(10, false),
            block(20, false),
            variable(100),
        ] {
            cpg.add_node_with_id(item);
        }
        add_ast(&mut cpg, 1, 10);
        add_ast(&mut cpg, 1, 20);
        for (strand, site) in [(10, 12), (20, 22)] {
            cpg.add_node_with_id(
                node(
                    site,
                    CpgNodeKind::Call {
                        target: None,
                        is_method: false,
                    },
                )
                .with_message_kind(MsgKind::Send),
            );
            cpg.add_node_with_id(identifier(site + 1_000, 100));
            add_ast(&mut cpg, strand, site);
            add_ast(&mut cpg, site, site + 1_000);
        }
        let mhp = super::super::may_happen_in_parallel(&cpg);
        let candidates = race_candidates(&cpg, &mhp);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].location, NodeId::new(100));
    }

    #[test]
    fn malformed_public_mhp_pairs_are_canonicalized_and_deduplicated() {
        let cpg = two_access_fixture(DfgEdgeKind::FieldWrite, DfgEdgeKind::FieldWrite, None);
        let mhp = MhpRelation {
            pairs: vec![
                super::super::MhpPair {
                    first: NodeId::new(22),
                    second: NodeId::new(12),
                },
                super::super::MhpPair {
                    first: NodeId::new(12),
                    second: NodeId::new(22),
                },
                super::super::MhpPair {
                    first: NodeId::new(12),
                    second: NodeId::new(12),
                },
            ],
            capped: false,
        };
        let candidates = race_candidates(&cpg, &mhp);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].access_a, NodeId::new(12));
        assert_eq!(candidates[0].access_b, NodeId::new(22));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn candidate_round_trips_through_serde() {
        let cpg = two_access_fixture(DfgEdgeKind::FieldWrite, DfgEdgeKind::FieldWrite, None);
        let mhp = super::super::may_happen_in_parallel(&cpg);
        let candidates = race_candidates(&cpg, &mhp);
        let json = serde_json::to_string(&candidates).expect("serialize candidates");
        let round_trip: Vec<RaceCandidate> =
            serde_json::from_str(&json).expect("deserialize candidates");
        assert_eq!(round_trip, candidates);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn candidates_are_deterministic_for_graph_insertion_order(
            location_count in 1u32..9,
            capped in any::<bool>(),
        ) {
            let forward = generated_fixture(location_count, false);
            let reversed = generated_fixture(location_count, true);
            let mut forward_mhp = super::super::may_happen_in_parallel(&forward);
            let mut reversed_mhp = super::super::may_happen_in_parallel(&reversed);
            forward_mhp.capped = capped;
            reversed_mhp.capped = capped;
            let forward_candidates = race_candidates(&forward, &forward_mhp);
            let reversed_candidates = race_candidates(&reversed, &reversed_mhp);
            prop_assert_eq!(&reversed_candidates, &forward_candidates);
            prop_assert_eq!(forward_candidates.len(), location_count as usize);
            let all_witnesses_are_canonical = forward_candidates.iter().all(|candidate| {
                candidate.access_a == NodeId::new(12)
                    && candidate.access_b == NodeId::new(22)
                    && candidate.witness.mhp_capped == capped
            });
            prop_assert!(all_witnesses_are_canonical);
        }
    }
}
