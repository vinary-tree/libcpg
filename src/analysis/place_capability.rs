//! Stack-safe ownership and borrowing overlay over typed CPG evidence.
//!
//! The analysis consumes frontend-normalized ownership operations, DFG
//! definition evidence, declaration references, AST scope, and intraprocedural
//! CFG order. All traversal state lives in explicit heap queues or vectors; no
//! source or graph depth is mapped to native recursion.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::Arc,
};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::{
    CodePropertyGraph, CpgEdgeKind, CpgNodeKind, DfgEdgeKind, NodeId, OwnershipOperationKind,
    PlaceCapabilityEdgeKind, TypeInfo,
};

use super::{function_scope_nodes, AnalysisError};

/// Rust copy classification available from represented type evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CopySemantics {
    /// The represented type is known to implement copy semantics.
    Copy,
    /// The represented type is known to move by value.
    Move,
    /// The CPG does not carry enough trait evidence to decide.
    Unknown,
}

/// Role of one existing CPG node in the place-capability report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CapabilityPlaceKind {
    /// Function parameter storage.
    Parameter,
    /// Local variable storage.
    Local,
    /// Declared field storage reached through a typed reference edge.
    Field,
    /// Value version introduced by an assignment node.
    Assignment,
    /// Expression-temporary storage for an uncaptured borrow.
    Temporary,
}

/// One memory place represented by an existing CPG node.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CapabilityPlace {
    /// Stable CPG node representing this place.
    pub node: NodeId,
    /// Storage role.
    pub kind: CapabilityPlaceKind,
    /// Binding or field name when represented.
    pub name: Option<Arc<str>>,
    /// Declared type evidence when represented.
    pub type_info: Option<TypeInfo>,
    /// Whether the declaration permits mutation.
    pub mutable: bool,
    /// Copy-versus-move classification derived from `type_info`.
    pub copy_semantics: CopySemantics,
}

/// Exact CPG witnesses from which a capability relation was reconstructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PlaceCapabilityEvidence {
    /// Function whose AST scope contains the operation and destination.
    pub scope: NodeId,
    /// Expression node that denotes the borrowed or moved-from place.
    pub operand: NodeId,
    /// Definition-to-operand DFG witness, when the source is a local place.
    pub data_flow: Option<(NodeId, NodeId)>,
    /// Operand-to-declaration reference witness, when the source is a field.
    pub reference: Option<(NodeId, NodeId)>,
}

/// One deterministic move, borrow, or reborrow relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PlaceCapabilityEdge {
    /// Place that provides ownership or access capability.
    pub source: NodeId,
    /// Place that receives ownership or access capability.
    pub target: NodeId,
    /// Expression that performs the transfer.
    pub operation: NodeId,
    /// Transfer classification.
    pub kind: PlaceCapabilityEdgeKind,
    /// Typed graph evidence supporting the relation.
    pub evidence: PlaceCapabilityEvidence,
}

/// Qualification attached when ownership evidence is incomplete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum PcgIssue {
    /// A value transfer has no represented source-place DFG/reference witness.
    MissingSource {
        /// Transfer expression.
        operation: NodeId,
    },
    /// A nominal source type has no represented `Copy` trait decision.
    UnknownCopySemantics {
        /// Source place.
        place: NodeId,
        /// Transfer expression.
        operation: NodeId,
    },
}

/// Read-only place-capability graph for one function.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct Pcg {
    /// Function whose ownership operations were analyzed.
    pub function: NodeId,
    /// Represented memory places, sorted by stable node identifier.
    pub places: Vec<CapabilityPlace>,
    /// Move/borrow/reborrow relations in canonical order.
    pub capability_edges: Vec<PlaceCapabilityEdge>,
    /// Evidence limitations that qualify negative conclusions.
    pub issues: Vec<PcgIssue>,
}

impl Pcg {
    /// Returns whether every represented transfer had decisive type evidence.
    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
    }
}

/// Transactional PlaceCapability-overlay materialization failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PlaceCapabilityOverlayError {
    /// A report relation source is absent from the target CPG.
    #[error("place-capability relation source does not exist: {relation:?}")]
    UnknownSource {
        /// Rejected relation.
        relation: PlaceCapabilityEdge,
    },
    /// A report relation target is absent from the target CPG.
    #[error("place-capability relation target does not exist: {relation:?}")]
    UnknownTarget {
        /// Rejected relation.
        relation: PlaceCapabilityEdge,
    },
}

/// Advisory ownership/borrowing violation class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum BorrowViolationKind {
    /// A represented place is used on a path after its ownership was moved.
    UseAfterMove,
    /// Live shared/exclusive loans of the same place overlap incompatibly.
    ConflictingBorrow,
}

/// One deterministic advisory ownership or borrowing finding.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct BorrowViolation {
    /// Violation class.
    pub kind: BorrowViolationKind,
    /// Function that owns the finding.
    pub function: NodeId,
    /// Borrowed-from or moved-from place.
    pub place: NodeId,
    /// Invalid use or conflicting borrow operation.
    pub operation: NodeId,
    /// Earlier move/borrow operation establishing the conflict.
    pub conflicting_operation: NodeId,
    /// Canonical CFG/DFG witness nodes.
    pub witness: Vec<NodeId>,
    /// Whether missing type evidence qualifies this finding.
    pub qualified: bool,
}

#[derive(Debug, Clone, Copy)]
struct SourceEvidence {
    source: NodeId,
    operand: NodeId,
    data_flow: Option<(NodeId, NodeId)>,
    reference: Option<(NodeId, NodeId)>,
}

/// Reconstructs a place-capability overlay from typed CPG evidence.
///
/// Operations are supplied by frontend metadata. Local identity comes from
/// DFG definitions, field identity from `Reference` edges, lexical boundaries
/// from the function AST scope, and ordering from the represented CFG. Missing
/// evidence is reported rather than guessed from source text.
pub fn place_capability_graph(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<Pcg, AnalysisError> {
    validate_function(cpg, function)?;
    let scope = function_scope_nodes(cpg, function);
    let mut operations: Vec<_> = scope
        .iter()
        .copied()
        .filter(|&node| {
            cpg.node(node)
                .is_some_and(|node| node.ownership_operation.is_some())
        })
        .collect();
    operations.sort_unstable_by_key(|&node| source_key(cpg, node));

    let mut relations = BTreeSet::new();
    let mut issues = BTreeSet::new();
    let mut place_nodes = BTreeSet::new();

    for operation in operations {
        let ownership = cpg
            .node(operation)
            .and_then(|node| node.ownership_operation)
            .expect("operation list contains only normalized ownership nodes");
        let sources = operation_sources(cpg, operation, ownership);
        if sources.is_empty() {
            issues.insert(PcgIssue::MissingSource { operation });
            continue;
        }
        let target = destination_place(cpg, function, operation).unwrap_or(operation);
        place_nodes.insert(target);

        for evidence in sources {
            place_nodes.insert(evidence.source);
            let kind = match ownership {
                OwnershipOperationKind::Move => {
                    let semantics = place_copy_semantics(cpg, evidence.source);
                    if semantics == CopySemantics::Copy {
                        continue;
                    }
                    if semantics == CopySemantics::Unknown {
                        issues.insert(PcgIssue::UnknownCopySemantics {
                            place: evidence.source,
                            operation,
                        });
                    }
                    PlaceCapabilityEdgeKind::Move
                }
                OwnershipOperationKind::SharedBorrow => {
                    if is_reference_place(cpg, evidence.source, &relations) {
                        PlaceCapabilityEdgeKind::SharedReborrow
                    } else {
                        PlaceCapabilityEdgeKind::SharedBorrow
                    }
                }
                OwnershipOperationKind::MutableBorrow => {
                    if is_reference_place(cpg, evidence.source, &relations) {
                        PlaceCapabilityEdgeKind::MutableReborrow
                    } else {
                        PlaceCapabilityEdgeKind::MutableBorrow
                    }
                }
            };
            relations.insert(PlaceCapabilityEdge {
                source: evidence.source,
                target,
                operation,
                kind,
                evidence: PlaceCapabilityEvidence {
                    scope: function,
                    operand: evidence.operand,
                    data_flow: evidence.data_flow,
                    reference: evidence.reference,
                },
            });
        }
    }

    let places = place_nodes
        .into_iter()
        .map(|node| capability_place(cpg, node))
        .collect();
    Ok(Pcg {
        function,
        places,
        capability_edges: relations.into_iter().collect(),
        issues: issues.into_iter().collect(),
    })
}

/// Reports move-after-use and incompatible-live-borrow evidence.
///
/// Borrow extent is the CFG region from the borrow operation through a
/// represented DFG use of the destination reference. This is a conservative,
/// source-independent analogue of non-lexical lifetime liveness. Findings are
/// advisory because a tree-sitter CPG does not contain rustc's trait and
/// borrow-checker facts.
pub fn borrow_check(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<Vec<BorrowViolation>, AnalysisError> {
    let pcg = place_capability_graph(cpg, function)?;
    let scope: BTreeSet<_> = function_scope_nodes(cpg, function).into_iter().collect();
    let adjacency = cfg_adjacency(cpg, &scope);
    let qualified_places: BTreeSet<_> = pcg
        .issues
        .iter()
        .filter_map(|issue| match issue {
            PcgIssue::UnknownCopySemantics { place, .. } => Some(*place),
            PcgIssue::MissingSource { .. } => None,
        })
        .collect();
    let mut findings = BTreeSet::new();

    for relation in &pcg.capability_edges {
        if relation.kind != PlaceCapabilityEdgeKind::Move {
            continue;
        }
        for use_site in definition_uses(cpg, relation.source) {
            if use_site == relation.evidence.operand
                || is_ast_descendant_of(cpg, use_site, relation.operation)
                || !reachable(&adjacency, relation.operation, use_site)
            {
                continue;
            }
            findings.insert(BorrowViolation {
                kind: BorrowViolationKind::UseAfterMove,
                function,
                place: relation.source,
                operation: use_site,
                conflicting_operation: relation.operation,
                witness: vec![relation.source, relation.operation, use_site],
                qualified: qualified_places.contains(&relation.source),
            });
        }
    }

    let borrows: Vec<_> = pcg
        .capability_edges
        .iter()
        .filter(|relation| is_borrow(relation.kind))
        .collect();
    for (index, earlier) in borrows.iter().enumerate() {
        for later in borrows.iter().skip(index + 1) {
            if earlier.source != later.source
                || !borrow_kinds_conflict(earlier.kind, later.kind)
                || !reachable(&adjacency, earlier.operation, later.operation)
            {
                continue;
            }
            let Some(live_use) = definition_uses(cpg, earlier.target)
                .into_iter()
                .filter(|&use_site| reachable(&adjacency, later.operation, use_site))
                .min_by_key(|&node| source_key(cpg, node))
            else {
                continue;
            };
            findings.insert(BorrowViolation {
                kind: BorrowViolationKind::ConflictingBorrow,
                function,
                place: later.source,
                operation: later.operation,
                conflicting_operation: earlier.operation,
                witness: vec![earlier.operation, later.operation, live_use],
                qualified: false,
            });
        }
    }

    Ok(findings.into_iter().collect())
}

/// Materializes a valid report as idempotent PlaceCapability edges.
///
/// Every endpoint is validated before the first mutation. No graph node is
/// created or removed.
pub fn materialize_place_capability_overlay(
    cpg: &mut CodePropertyGraph,
    pcg: &Pcg,
) -> Result<usize, PlaceCapabilityOverlayError> {
    for &relation in &pcg.capability_edges {
        if !cpg.contains_node(relation.source) {
            return Err(PlaceCapabilityOverlayError::UnknownSource { relation });
        }
        if !cpg.contains_node(relation.target) {
            return Err(PlaceCapabilityOverlayError::UnknownTarget { relation });
        }
    }
    let before = cpg.place_capability_edges().count();
    for relation in &pcg.capability_edges {
        cpg.connect_unique(
            relation.source,
            relation.target,
            CpgEdgeKind::PlaceCapability(relation.kind),
        );
    }
    Ok(cpg.place_capability_edges().count().saturating_sub(before))
}

fn validate_function(cpg: &CodePropertyGraph, function: NodeId) -> Result<(), AnalysisError> {
    let Some(node) = cpg.node(function) else {
        return Err(AnalysisError::UnknownNode(function));
    };
    if !matches!(node.kind, CpgNodeKind::Function { .. }) {
        return Err(AnalysisError::NotAFunction(function));
    }
    Ok(())
}

fn operation_sources(
    cpg: &CodePropertyGraph,
    operation: NodeId,
    ownership: OwnershipOperationKind,
) -> Vec<SourceEvidence> {
    let mut operands = Vec::new();
    let mut work = vec![operation];
    let mut visited = BTreeSet::new();
    while let Some(node) = work.pop() {
        if !visited.insert(node) {
            continue;
        }
        if node != operation
            && cpg
                .node(node)
                .is_some_and(|node| node.ownership_operation.is_some())
        {
            continue;
        }
        if has_place_evidence(cpg, node) {
            operands.push(node);
            if ownership == OwnershipOperationKind::Move {
                continue;
            }
        }
        let children = cpg.ast_children(node);
        work.extend(children.iter().rev().copied());
    }
    operands.sort_unstable_by_key(|&node| source_key(cpg, node));
    operands.dedup();

    let mut evidence = BTreeSet::new();
    for operand in operands {
        for edge in cpg.incoming_edges(operand) {
            if matches!(
                edge.kind,
                CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse | DfgEdgeKind::ReachingDef)
            ) {
                evidence.insert((edge.source, operand, Some((edge.source, operand)), None));
            }
        }
        for edge in cpg.outgoing_edges(operand) {
            if edge.kind == CpgEdgeKind::Reference
                && cpg
                    .node(edge.target)
                    .is_some_and(|node| matches!(node.kind, CpgNodeKind::Field { .. }))
            {
                evidence.insert((edge.target, operand, None, Some((operand, edge.target))));
            }
        }
    }
    evidence
        .into_iter()
        .map(|(source, operand, data_flow, reference)| SourceEvidence {
            source,
            operand,
            data_flow,
            reference,
        })
        .collect()
}

fn has_place_evidence(cpg: &CodePropertyGraph, node: NodeId) -> bool {
    cpg.incoming_edges(node).any(|edge| {
        matches!(
            edge.kind,
            CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse | DfgEdgeKind::ReachingDef)
        )
    }) || cpg.outgoing_edges(node).any(|edge| {
        edge.kind == CpgEdgeKind::Reference
            && cpg
                .node(edge.target)
                .is_some_and(|node| matches!(node.kind, CpgNodeKind::Field { .. }))
    })
}

fn destination_place(
    cpg: &CodePropertyGraph,
    function: NodeId,
    operation: NodeId,
) -> Option<NodeId> {
    let mut current = cpg.ast_parent(operation);
    let mut visited = BTreeSet::new();
    while let Some(node) = current {
        if node == function || !visited.insert(node) {
            break;
        }
        if cpg.node(node).is_some_and(|node| {
            matches!(
                node.kind,
                CpgNodeKind::Variable { .. } | CpgNodeKind::Assignment { .. }
            )
        }) {
            return Some(node);
        }
        current = cpg.ast_parent(node);
    }
    None
}

fn capability_place(cpg: &CodePropertyGraph, node: NodeId) -> CapabilityPlace {
    let (kind, name, type_info, mutable) = match cpg.node(node).map(|node| &node.kind) {
        Some(CpgNodeKind::Parameter {
            name, param_type, ..
        }) => (
            CapabilityPlaceKind::Parameter,
            Some(Arc::clone(name)),
            param_type.as_deref().cloned(),
            param_type.as_deref().is_some_and(|kind| kind.is_mutable),
        ),
        Some(CpgNodeKind::Variable {
            name,
            var_type,
            is_mutable,
            ..
        }) => (
            CapabilityPlaceKind::Local,
            Some(Arc::clone(name)),
            var_type.as_deref().cloned(),
            *is_mutable,
        ),
        Some(CpgNodeKind::Field {
            name, field_type, ..
        }) => (
            CapabilityPlaceKind::Field,
            Some(Arc::clone(name)),
            field_type.as_deref().cloned(),
            false,
        ),
        Some(CpgNodeKind::Assignment { .. }) => (CapabilityPlaceKind::Assignment, None, None, true),
        _ => (CapabilityPlaceKind::Temporary, None, None, false),
    };
    let copy_semantics = type_info
        .as_ref()
        .map_or(CopySemantics::Unknown, classify_copy_semantics);
    CapabilityPlace {
        node,
        kind,
        name,
        type_info,
        mutable,
        copy_semantics,
    }
}

fn place_copy_semantics(cpg: &CodePropertyGraph, node: NodeId) -> CopySemantics {
    capability_place(cpg, node).copy_semantics
}

fn classify_copy_semantics(kind: &TypeInfo) -> CopySemantics {
    if kind.is_reference {
        return if kind.is_mutable {
            CopySemantics::Move
        } else {
            CopySemantics::Copy
        };
    }
    let terminal = kind.name.rsplit("::").next().unwrap_or(kind.name.as_ref());
    if matches!(
        terminal,
        "bool"
            | "char"
            | "i8"
            | "i16"
            | "i32"
            | "i64"
            | "i128"
            | "isize"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "u128"
            | "usize"
            | "f32"
            | "f64"
    ) || kind.name.starts_with("*const ")
        || kind.name.starts_with("*mut ")
    {
        CopySemantics::Copy
    } else if matches!(
        terminal,
        "String" | "Vec" | "Box" | "Rc" | "Arc" | "OsString" | "PathBuf" | "CString"
    ) {
        CopySemantics::Move
    } else {
        CopySemantics::Unknown
    }
}

fn is_reference_place(
    cpg: &CodePropertyGraph,
    place: NodeId,
    relations: &BTreeSet<PlaceCapabilityEdge>,
) -> bool {
    capability_place(cpg, place)
        .type_info
        .is_some_and(|kind| kind.is_reference)
        || relations
            .iter()
            .any(|relation| relation.target == place && is_borrow(relation.kind))
}

fn is_borrow(kind: PlaceCapabilityEdgeKind) -> bool {
    !matches!(kind, PlaceCapabilityEdgeKind::Move)
}

fn borrow_kinds_conflict(left: PlaceCapabilityEdgeKind, right: PlaceCapabilityEdgeKind) -> bool {
    is_mutable_borrow(left) || is_mutable_borrow(right)
}

fn is_mutable_borrow(kind: PlaceCapabilityEdgeKind) -> bool {
    matches!(
        kind,
        PlaceCapabilityEdgeKind::MutableBorrow | PlaceCapabilityEdgeKind::MutableReborrow
    )
}

fn definition_uses(cpg: &CodePropertyGraph, definition: NodeId) -> Vec<NodeId> {
    let mut uses: Vec<_> = cpg
        .outgoing_edges(definition)
        .filter_map(|edge| {
            matches!(
                edge.kind,
                CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse | DfgEdgeKind::ReachingDef)
            )
            .then_some(edge.target)
        })
        .collect();
    uses.sort_unstable_by_key(|&node| source_key(cpg, node));
    uses.dedup();
    uses
}

fn cfg_adjacency(
    cpg: &CodePropertyGraph,
    scope: &BTreeSet<NodeId>,
) -> BTreeMap<NodeId, Vec<NodeId>> {
    let mut adjacency: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
    for edge in cpg.edges() {
        if edge.kind.is_cfg() && scope.contains(&edge.source) && scope.contains(&edge.target) {
            adjacency.entry(edge.source).or_default().push(edge.target);
        }
    }
    for targets in adjacency.values_mut() {
        targets.sort_unstable();
        targets.dedup();
    }
    adjacency
}

fn reachable(adjacency: &BTreeMap<NodeId, Vec<NodeId>>, source: NodeId, target: NodeId) -> bool {
    if source == target {
        return true;
    }
    let mut visited = BTreeSet::new();
    let mut work = VecDeque::from([source]);
    while let Some(node) = work.pop_front() {
        if !visited.insert(node) {
            continue;
        }
        for &successor in adjacency.get(&node).map_or(&[][..], Vec::as_slice) {
            if successor == target {
                return true;
            }
            if !visited.contains(&successor) {
                work.push_back(successor);
            }
        }
    }
    false
}

fn is_ast_descendant_of(cpg: &CodePropertyGraph, node: NodeId, ancestor: NodeId) -> bool {
    let mut current = Some(node);
    let mut visited = BTreeSet::new();
    while let Some(candidate) = current {
        if candidate == ancestor {
            return true;
        }
        if !visited.insert(candidate) {
            break;
        }
        current = cpg.ast_parent(candidate);
    }
    false
}

fn source_key(cpg: &CodePropertyGraph, node: NodeId) -> (u32, u32, NodeId) {
    cpg.node(node)
        .map(|node| (node.range.start, node.range.end, node.id))
        .unwrap_or((u32::MAX, u32::MAX, node))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CfgEdgeKind, CpgNode, Language, MethodSignature, ScopeId, SourceRange, Visibility,
    };
    use proptest::prelude::*;

    fn node(id: u32, kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(id), kind, SourceRange::from_bytes(id, id + 1))
    }

    fn function() -> CpgNodeKind {
        CpgNodeKind::Function {
            signature: Box::new(MethodSignature {
                name: "ownership".into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            }),
        }
    }

    fn variable(name: &str, kind: TypeInfo, mutable: bool) -> CpgNodeKind {
        CpgNodeKind::Variable {
            name: name.into(),
            var_type: Some(Box::new(kind)),
            scope: ScopeId::GLOBAL,
            is_mutable: mutable,
        }
    }

    fn identifier(name: &str) -> CpgNodeKind {
        CpgNodeKind::Identifier {
            name: name.into(),
            definition: None,
        }
    }

    fn add(cpg: &mut CodePropertyGraph, id: u32, kind: CpgNodeKind) {
        cpg.add_node_with_id(node(id, kind));
    }

    fn edge(cpg: &mut CodePropertyGraph, source: u32, target: u32, kind: CpgEdgeKind) {
        cpg.connect(NodeId::new(source), NodeId::new(target), kind)
            .expect("fixture endpoints exist");
    }

    fn move_fixture(edge_order: &[usize]) -> (CodePropertyGraph, NodeId) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        add(&mut cpg, 0, function());
        add(
            &mut cpg,
            1,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        add(
            &mut cpg,
            2,
            variable("source", TypeInfo::new("String"), false),
        );
        add(
            &mut cpg,
            3,
            variable("moved", TypeInfo::new("String"), false),
        );
        let mut transfer = node(4, identifier("source"));
        transfer.ownership_operation = Some(OwnershipOperationKind::Move);
        cpg.add_node_with_id(transfer);
        add(&mut cpg, 5, identifier("source"));
        let edges = [
            (0, 1, CpgEdgeKind::AstChild),
            (1, 2, CpgEdgeKind::AstChild),
            (1, 3, CpgEdgeKind::AstChild),
            (3, 4, CpgEdgeKind::AstChild),
            (1, 5, CpgEdgeKind::AstChild),
            (2, 4, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse)),
            (2, 5, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse)),
            (0, 1, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential)),
            (1, 2, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential)),
            (2, 3, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential)),
            (3, 4, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential)),
            (4, 5, CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential)),
        ];
        for &index in edge_order {
            let (source, target, kind) = edges[index].clone();
            edge(&mut cpg, source, target, kind);
        }
        (cpg, NodeId::new(0))
    }

    fn borrow_fixture(conflicting: bool) -> (CodePropertyGraph, NodeId) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        add(&mut cpg, 0, function());
        add(
            &mut cpg,
            1,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        add(
            &mut cpg,
            2,
            variable("value", TypeInfo::new("String"), true),
        );
        add(
            &mut cpg,
            3,
            variable(
                "shared",
                TypeInfo::new("String").with_reference(true),
                false,
            ),
        );
        let mut shared = node(
            4,
            CpgNodeKind::UnaryOp {
                operator: "&".into(),
            },
        );
        shared.ownership_operation = Some(OwnershipOperationKind::SharedBorrow);
        cpg.add_node_with_id(shared);
        add(&mut cpg, 5, identifier("value"));
        add(
            &mut cpg,
            6,
            variable(
                "exclusive",
                TypeInfo::new("String")
                    .with_reference(true)
                    .with_mutable(true),
                false,
            ),
        );
        let mut second = node(
            7,
            CpgNodeKind::UnaryOp {
                operator: "&".into(),
            },
        );
        second.ownership_operation = Some(if conflicting {
            OwnershipOperationKind::MutableBorrow
        } else {
            OwnershipOperationKind::SharedBorrow
        });
        cpg.add_node_with_id(second);
        add(&mut cpg, 8, identifier("value"));
        add(&mut cpg, 9, identifier("shared"));
        for (source, target) in [
            (0, 1),
            (1, 2),
            (1, 3),
            (3, 4),
            (4, 5),
            (1, 6),
            (6, 7),
            (7, 8),
            (1, 9),
        ] {
            edge(&mut cpg, source, target, CpgEdgeKind::AstChild);
        }
        for (source, target) in [(2, 5), (2, 8), (3, 9)] {
            edge(
                &mut cpg,
                source,
                target,
                CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
            );
        }
        for (source, target) in [(0, 1), (1, 2), (2, 4), (4, 5), (5, 7), (7, 8), (8, 9)] {
            edge(
                &mut cpg,
                source,
                target,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
            );
        }
        (cpg, NodeId::new(0))
    }

    #[test]
    fn move_after_use_has_exact_dfg_witness() {
        let (cpg, function) = move_fixture(&(0..12).collect::<Vec<_>>());
        let pcg = place_capability_graph(&cpg, function).expect("valid function");
        assert!(pcg.is_complete());
        assert_eq!(pcg.capability_edges.len(), 1);
        assert_eq!(pcg.capability_edges[0].kind, PlaceCapabilityEdgeKind::Move);
        assert_eq!(
            pcg.capability_edges[0].evidence.data_flow,
            Some((NodeId::new(2), NodeId::new(4)))
        );
        let violations = borrow_check(&cpg, function).expect("valid function");
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].kind, BorrowViolationKind::UseAfterMove);
        assert!(!violations[0].qualified);
    }

    #[test]
    fn mutable_and_live_shared_borrows_conflict_but_shared_pair_is_clean() {
        let (conflicting, function) = borrow_fixture(true);
        let violations = borrow_check(&conflicting, function).expect("valid function");
        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].kind, BorrowViolationKind::ConflictingBorrow);
        assert_eq!(violations[0].witness.last(), Some(&NodeId::new(9)));

        let (well_borrowed, function) = borrow_fixture(false);
        assert!(borrow_check(&well_borrowed, function)
            .expect("valid function")
            .is_empty());
    }

    #[test]
    fn field_borrow_is_reconstructed_from_reference_evidence() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        add(&mut cpg, 0, function());
        add(
            &mut cpg,
            1,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        add(
            &mut cpg,
            2,
            variable(
                "borrowed",
                TypeInfo::new("String").with_reference(true),
                false,
            ),
        );
        let mut borrow = node(
            3,
            CpgNodeKind::UnaryOp {
                operator: "&".into(),
            },
        );
        borrow.ownership_operation = Some(OwnershipOperationKind::SharedBorrow);
        cpg.add_node_with_id(borrow);
        add(
            &mut cpg,
            4,
            CpgNodeKind::MemberAccess {
                member: "value".into(),
            },
        );
        add(
            &mut cpg,
            10,
            CpgNodeKind::Field {
                name: "value".into(),
                field_type: Some(Box::new(TypeInfo::new("String"))),
                visibility: Visibility::Private,
            },
        );
        for (source, target) in [(0, 1), (1, 2), (2, 3), (3, 4)] {
            edge(&mut cpg, source, target, CpgEdgeKind::AstChild);
        }
        edge(&mut cpg, 4, 10, CpgEdgeKind::Reference);
        for (source, target) in [(0, 1), (1, 3), (3, 4)] {
            edge(
                &mut cpg,
                source,
                target,
                CpgEdgeKind::ControlFlow(CfgEdgeKind::Sequential),
            );
        }

        let pcg = place_capability_graph(&cpg, NodeId::new(0)).expect("field borrow PCG");
        assert_eq!(pcg.capability_edges.len(), 1);
        assert_eq!(pcg.capability_edges[0].source, NodeId::new(10));
        assert_eq!(pcg.capability_edges[0].target, NodeId::new(2));
        assert_eq!(
            pcg.capability_edges[0].evidence.reference,
            Some((NodeId::new(4), NodeId::new(10)))
        );
        assert_eq!(pcg.capability_edges[0].evidence.data_flow, None);
    }

    proptest! {
        #[test]
        fn report_is_independent_of_edge_insertion_order(keys in prop::collection::vec(any::<u16>(), 0..96)) {
            let canonical: Vec<_> = (0..12).collect();
            let mut permutation = canonical.clone();
            permutation.sort_by_key(|&index| (keys.get(index).copied().unwrap_or_default(), index));
            let (expected, function) = move_fixture(&canonical);
            let (actual, _) = move_fixture(&permutation);
            prop_assert_eq!(
                place_capability_graph(&actual, function),
                place_capability_graph(&expected, function)
            );
            prop_assert_eq!(borrow_check(&actual, function), borrow_check(&expected, function));
        }
    }

    #[test]
    fn materialization_is_transactional_and_idempotent() {
        let (mut cpg, function) = borrow_fixture(true);
        let pcg = place_capability_graph(&cpg, function).expect("valid function");
        let node_count = cpg.node_count();
        assert_eq!(materialize_place_capability_overlay(&mut cpg, &pcg), Ok(2));
        assert_eq!(materialize_place_capability_overlay(&mut cpg, &pcg), Ok(0));
        assert_eq!(cpg.node_count(), node_count);
        assert_eq!(cpg.place_capability_edges().count(), 2);

        let mut malformed = pcg;
        malformed.capability_edges[0].source = NodeId::new(10_000);
        let edge_count = cpg.edge_count();
        assert!(matches!(
            materialize_place_capability_overlay(&mut cpg, &malformed),
            Err(PlaceCapabilityOverlayError::UnknownSource { .. })
        ));
        assert_eq!(cpg.edge_count(), edge_count);
    }
}
