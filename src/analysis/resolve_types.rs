//! Closed-world hierarchy refinements for dynamic call resolution.
//!
//! [`devirtualize`] refines the out-of-graph candidate records produced by
//! [`crate::resolve_calls`]. Class-hierarchy analysis (CHA) admits every
//! represented concrete subtype of the receiver's declared type. Rapid-type
//! analysis (RTA) retains only receiver types with represented allocation
//! evidence. Variable-type analysis (VTA) additionally retains only allocation
//! types reaching the receiver through its definition or def-use evidence.
//!
//! All graph walks use heap-backed worklists. The analysis neither reparses
//! source text nor mutates the CPG, and a nominal type name is accepted only
//! when it identifies exactly one represented type.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::{
    CallGraphResolution, CallResolution, CodePropertyGraph, Confidence, CpgEdgeKind, CpgNodeKind,
    DfgEdgeKind, NodeId, TypeInfo,
};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Closed-world evidence used to refine virtual-call target sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum DevirtualizationMode {
    /// Class-hierarchy analysis over every represented concrete subtype.
    Cha,
    /// Rapid-type analysis over represented allocated subtypes.
    Rta,
    /// Variable-type analysis over allocated subtypes reaching the receiver.
    Vta,
}

impl DevirtualizationMode {
    const fn confidence(self) -> Confidence {
        match self {
            Self::Cha => Confidence::ClassHierarchy,
            Self::Rta => Confidence::RapidType,
            Self::Vta => Confidence::VariableType,
        }
    }
}

/// Refines method-call candidates with represented hierarchy and type evidence.
///
/// The input and output are out-of-graph resolution records. This function is
/// append-only with respect to analysis knowledge and does not add, remove, or
/// relabel CPG edges. Under its explicit closed-world assumption, a singleton
/// candidate is returned as `target`; multiple candidates remain an ordered
/// candidate set with no fabricated unique target.
///
/// Calls without a typed member receiver, non-method calls, and receiver type
/// names that are absent or ambiguous in the CPG are copied unchanged. CHA,
/// RTA, and VTA therefore refine only calls for which the graph contains the
/// evidence required by the selected mode.
pub fn devirtualize(
    cpg: &CodePropertyGraph,
    resolution: &CallGraphResolution,
    mode: DevirtualizationMode,
) -> CallGraphResolution {
    let index = TypeIndex::build(cpg);
    let mut dispatch_by_name: BTreeMap<String, BTreeMap<NodeId, BTreeSet<NodeId>>> =
        BTreeMap::new();
    let mut output = CallGraphResolution::default();

    for (&call, original) in &resolution.resolutions {
        let mut refined = original.clone();
        if is_method_call(cpg, call) {
            if let (Some(name), Some(receiver)) =
                (original.callee_name.as_deref(), receiver_of_call(cpg, call))
            {
                let static_types = receiver_static_types(cpg, &index, receiver);
                if !static_types.is_empty() {
                    let cha_types = concrete_subtypes(&index, &static_types);
                    let runtime_types = match mode {
                        DevirtualizationMode::Cha => cha_types,
                        DevirtualizationMode::Rta => cha_types
                            .intersection(&index.instantiated)
                            .copied()
                            .collect(),
                        DevirtualizationMode::Vta => {
                            let assigned = receiver_assigned_types(cpg, &index, receiver);
                            cha_types
                                .intersection(&index.instantiated)
                                .filter(|type_id| assigned.contains(type_id))
                                .copied()
                                .collect()
                        }
                    };
                    let dispatch = dispatch_by_name
                        .entry(name.to_owned())
                        .or_insert_with(|| dispatch_for_name(&index, name));
                    let candidates: BTreeSet<_> = runtime_types
                        .iter()
                        .flat_map(|type_id| dispatch.get(type_id).into_iter().flatten().copied())
                        .collect();
                    let candidates: Vec<_> = candidates.into_iter().collect();

                    refined.confidence = mode.confidence();
                    refined.target = (candidates.len() == 1).then_some(candidates[0]);
                    refined.candidate_count = candidates.len();
                    refined.candidates_truncated = false;
                    refined.candidates = candidates;
                }
            }
        }
        count_resolution(&mut output, &refined);
        output.resolutions.insert(call, refined);
    }

    // Devirtualization is deliberately out of graph. The input's historical
    // emission count is not an emission performed by this immutable pass.
    output.topology_edges = 0;
    output
}

#[derive(Debug, Default)]
struct TypeIndex {
    nominal: BTreeSet<NodeId>,
    concrete: BTreeSet<NodeId>,
    names: BTreeMap<String, Vec<NodeId>>,
    children: BTreeMap<NodeId, Vec<NodeId>>,
    methods: BTreeMap<NodeId, BTreeMap<String, BTreeSet<NodeId>>>,
    instantiated: BTreeSet<NodeId>,
}

impl TypeIndex {
    fn build(cpg: &CodePropertyGraph) -> Self {
        let mut index = Self::default();
        let mut nodes: Vec<_> = cpg.node_ids().collect();
        nodes.sort_unstable();

        for &node_id in &nodes {
            let Some(node) = cpg.node(node_id) else {
                continue;
            };
            if let Some((name, is_concrete)) = nominal_type(&node.kind) {
                index.nominal.insert(node_id);
                if is_concrete {
                    index.concrete.insert(node_id);
                }
                index
                    .names
                    .entry(name.to_owned())
                    .or_default()
                    .push(node_id);
            }
        }
        for named in index.names.values_mut() {
            named.sort_unstable();
            named.dedup();
        }

        let mut hierarchy_edges: Vec<_> = cpg
            .edges()
            .filter(|edge| matches!(edge.kind, CpgEdgeKind::Inherits | CpgEdgeKind::Implements))
            .map(|edge| (edge.source, edge.target))
            .filter(|(subtype, parent)| {
                index.nominal.contains(subtype) && index.nominal.contains(parent)
            })
            .collect();
        hierarchy_edges.sort_unstable();
        hierarchy_edges.dedup();
        for (subtype, parent) in hierarchy_edges {
            index.children.entry(parent).or_default().push(subtype);
        }

        for &node_id in &nodes {
            let Some(node) = cpg.node(node_id) else {
                continue;
            };
            if let Some(type_info) = &node.allocation_type {
                if let Some(type_id) = unique_type(&index.names, type_info) {
                    index.instantiated.insert(type_id);
                }
            }
            let CpgNodeKind::Function { signature } = &node.kind else {
                continue;
            };
            if signature.is_static {
                continue;
            }
            if let Some(owner) = method_owner(cpg, &index.names, node_id) {
                index
                    .methods
                    .entry(owner)
                    .or_default()
                    .entry(signature.name.to_string())
                    .or_default()
                    .insert(node_id);
            }
        }
        index
    }
}

fn nominal_type(kind: &CpgNodeKind) -> Option<(&str, bool)> {
    match kind {
        CpgNodeKind::Class { name, is_abstract } => Some((name, !is_abstract)),
        CpgNodeKind::Struct { name } | CpgNodeKind::Enum { name } => Some((name, true)),
        CpgNodeKind::Trait { name } => Some((name, false)),
        _ => None,
    }
}

fn unique_type(names: &BTreeMap<String, Vec<NodeId>>, type_info: &TypeInfo) -> Option<NodeId> {
    let candidates = names.get(type_info.name.as_ref())?;
    (candidates.len() == 1).then_some(candidates[0])
}

fn method_owner(
    cpg: &CodePropertyGraph,
    names: &BTreeMap<String, Vec<NodeId>>,
    method: NodeId,
) -> Option<NodeId> {
    let mut current = cpg.node(method).and_then(|node| node.parent);
    let mut visited = BTreeSet::new();
    while let Some(candidate) = current {
        if !visited.insert(candidate) {
            return None;
        }
        let node = cpg.node(candidate)?;
        if nominal_type(&node.kind).is_some() {
            return Some(candidate);
        }
        match &node.kind {
            CpgNodeKind::Impl {
                for_type: Some(for_type),
                ..
            } => {
                return unique_type(names, &TypeInfo::new(for_type.clone()));
            }
            // A function nested within another function is not a method of the
            // outer nominal type.
            CpgNodeKind::Function { .. } => return None,
            _ => current = node.parent,
        }
    }
    None
}

fn enclosing_receiver_type(
    cpg: &CodePropertyGraph,
    names: &BTreeMap<String, Vec<NodeId>>,
    receiver: NodeId,
) -> Option<NodeId> {
    let mut current = cpg.node(receiver).and_then(|node| node.parent);
    let mut visited = BTreeSet::new();
    let mut containing_function_seen = false;
    while let Some(candidate) = current {
        if !visited.insert(candidate) {
            return None;
        }
        let node = cpg.node(candidate)?;
        if nominal_type(&node.kind).is_some() {
            return Some(candidate);
        }
        match &node.kind {
            CpgNodeKind::Impl {
                for_type: Some(for_type),
                ..
            } => return unique_type(names, &TypeInfo::new(for_type.clone())),
            CpgNodeKind::Function { .. } if containing_function_seen => return None,
            CpgNodeKind::Function { .. } => containing_function_seen = true,
            _ => {}
        }
        current = node.parent;
    }
    None
}

fn is_method_call(cpg: &CodePropertyGraph, call: NodeId) -> bool {
    matches!(
        cpg.node(call).map(|node| &node.kind),
        Some(CpgNodeKind::Call {
            is_method: true,
            ..
        })
    )
}

fn receiver_of_call(cpg: &CodePropertyGraph, call: NodeId) -> Option<NodeId> {
    let callee = cpg.ast_children(call).into_iter().next()?;
    if !matches!(
        cpg.node(callee).map(|node| &node.kind),
        Some(CpgNodeKind::MemberAccess { .. })
    ) {
        return None;
    }
    cpg.ast_children(callee).into_iter().next()
}

fn receiver_static_types(
    cpg: &CodePropertyGraph,
    index: &TypeIndex,
    receiver: NodeId,
) -> BTreeSet<NodeId> {
    let mut types = BTreeSet::new();
    add_static_type_evidence(cpg, index, receiver, &mut types);

    if let Some(CpgNodeKind::Identifier {
        name,
        definition: Some(definition),
    }) = cpg.node(receiver).map(|node| &node.kind)
    {
        add_static_type_evidence(cpg, index, *definition, &mut types);
        if matches!(name.as_ref(), "self" | "this") {
            if let Some(owner) = enclosing_receiver_type(cpg, &index.names, receiver) {
                types.insert(owner);
            }
        }
    } else if let Some(CpgNodeKind::Identifier { name, .. }) =
        cpg.node(receiver).map(|node| &node.kind)
    {
        if matches!(name.as_ref(), "self" | "this") {
            if let Some(owner) = enclosing_receiver_type(cpg, &index.names, receiver) {
                types.insert(owner);
            }
        }
    }
    types
}

fn add_static_type_evidence(
    cpg: &CodePropertyGraph,
    index: &TypeIndex,
    node_id: NodeId,
    output: &mut BTreeSet<NodeId>,
) {
    let Some(node) = cpg.node(node_id) else {
        return;
    };
    let declared = match &node.kind {
        CpgNodeKind::Variable { var_type, .. } => var_type.as_ref(),
        CpgNodeKind::Parameter { param_type, .. } => param_type.as_ref(),
        CpgNodeKind::Field { field_type, .. } => field_type.as_ref(),
        CpgNodeKind::TypeAnnotation { type_info } => Some(type_info),
        CpgNodeKind::Function { signature } => signature.return_type.as_ref(),
        _ => None,
    };
    if let Some(type_id) = declared.and_then(|type_info| unique_type(&index.names, type_info)) {
        output.insert(type_id);
    }
    if let Some(type_id) = node
        .allocation_type
        .as_ref()
        .and_then(|type_info| unique_type(&index.names, type_info))
    {
        output.insert(type_id);
    }
    for edge in cpg
        .outgoing_edges(node_id)
        .filter(|edge| edge.kind == CpgEdgeKind::TypeOf)
    {
        if index.nominal.contains(&edge.target) {
            output.insert(edge.target);
        } else if let Some(CpgNodeKind::TypeAnnotation { type_info }) =
            cpg.node(edge.target).map(|node| &node.kind)
        {
            if let Some(type_id) = unique_type(&index.names, type_info) {
                output.insert(type_id);
            }
        }
    }
    if let CpgNodeKind::Call {
        target: Some(target),
        ..
    } = node.kind
    {
        if let Some(CpgNodeKind::Function { signature }) =
            cpg.node(target).map(|target| &target.kind)
        {
            if let Some(type_id) = signature
                .return_type
                .as_ref()
                .and_then(|type_info| unique_type(&index.names, type_info))
            {
                output.insert(type_id);
            }
        }
    }
}

fn concrete_subtypes(index: &TypeIndex, roots: &BTreeSet<NodeId>) -> BTreeSet<NodeId> {
    let mut visited = BTreeSet::new();
    let mut work: VecDeque<_> = roots.iter().copied().collect();
    while let Some(current) = work.pop_front() {
        if !visited.insert(current) {
            continue;
        }
        if let Some(children) = index.children.get(&current) {
            work.extend(children.iter().copied());
        }
    }
    visited.intersection(&index.concrete).copied().collect()
}

fn dispatch_for_name(index: &TypeIndex, name: &str) -> BTreeMap<NodeId, BTreeSet<NodeId>> {
    let mut dispatch: BTreeMap<NodeId, BTreeSet<NodeId>> = index
        .nominal
        .iter()
        .copied()
        .map(|type_id| {
            let methods = index
                .methods
                .get(&type_id)
                .and_then(|methods| methods.get(name))
                .cloned()
                .unwrap_or_default();
            (type_id, methods)
        })
        .collect();
    let own: BTreeSet<_> = dispatch
        .iter()
        .filter_map(|(&type_id, methods)| (!methods.is_empty()).then_some(type_id))
        .collect();
    let mut work: VecDeque<_> = own.iter().copied().collect();
    let mut queued = own.clone();

    while let Some(parent) = work.pop_front() {
        queued.remove(&parent);
        let inherited = dispatch.get(&parent).cloned().unwrap_or_default();
        for child in index.children.get(&parent).into_iter().flatten() {
            if own.contains(child) {
                continue;
            }
            let child_dispatch = dispatch.entry(*child).or_default();
            let before = child_dispatch.len();
            child_dispatch.extend(inherited.iter().copied());
            if child_dispatch.len() != before && queued.insert(*child) {
                work.push_back(*child);
            }
        }
    }
    dispatch
}

fn receiver_assigned_types(
    cpg: &CodePropertyGraph,
    index: &TypeIndex,
    receiver: NodeId,
) -> BTreeSet<NodeId> {
    let mut roots = BTreeSet::from([receiver]);
    if let Some(CpgNodeKind::Identifier {
        definition: Some(definition),
        ..
    }) = cpg.node(receiver).map(|node| &node.kind)
    {
        roots.insert(*definition);
    }
    for edge in cpg.incoming_edges(receiver).filter(|edge| {
        matches!(
            edge.kind,
            CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse | DfgEdgeKind::ReachingDef)
        )
    }) {
        roots.insert(edge.source);
    }
    for edge in cpg
        .outgoing_edges(receiver)
        .filter(|edge| matches!(edge.kind, CpgEdgeKind::DataFlow(DfgEdgeKind::UseDef)))
    {
        roots.insert(edge.target);
    }

    let mut assigned = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut work: Vec<_> = roots.into_iter().collect();
    while let Some(current) = work.pop() {
        if !visited.insert(current) {
            continue;
        }
        let Some(node) = cpg.node(current) else {
            continue;
        };
        if let Some(type_id) = node
            .allocation_type
            .as_ref()
            .and_then(|type_info| unique_type(&index.names, type_info))
        {
            assigned.insert(type_id);
        }
        if current != receiver && matches!(node.kind, CpgNodeKind::Function { .. }) {
            continue;
        }
        let mut children = cpg.ast_children(current);
        children.reverse();
        work.extend(children);
    }
    assigned
}

fn count_resolution(output: &mut CallGraphResolution, resolution: &CallResolution) {
    match resolution.confidence {
        Confidence::BareNameAmbiguous => output.ambiguous = output.ambiguous.saturating_add(1),
        Confidence::Unresolved => output.unresolved = output.unresolved.saturating_add(1),
        Confidence::External => output.external = output.external.saturating_add(1),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::{
        CodePropertyGraph, CpgNode, Language, MethodSignature, ScopeId, SourceRange, Visibility,
    };

    fn node(kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(0), kind, SourceRange::default())
    }

    fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
        let child = cpg.add_node(node(kind));
        cpg.connect(parent, child, CpgEdgeKind::AstChild);
        cpg.node_mut(parent).expect("parent").children.push(child);
        cpg.node_mut(child).expect("child").parent = Some(parent);
        child
    }

    fn method(cpg: &mut CodePropertyGraph, owner: NodeId, name: &str) -> NodeId {
        child(
            cpg,
            owner,
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

    struct Fixture {
        cpg: CodePropertyGraph,
        resolution: CallGraphResolution,
        call: NodeId,
        base_method: NodeId,
        mid_method: NodeId,
        leaf_method: NodeId,
    }

    fn hierarchy_fixture_with_edge_order(reverse_edges: bool) -> Fixture {
        let mut cpg = CodePropertyGraph::new(Language::Java);
        let root = cpg.add_node(node(CpgNodeKind::Root));
        let base = child(
            &mut cpg,
            root,
            CpgNodeKind::Class {
                name: "Base".into(),
                is_abstract: false,
            },
        );
        let mid = child(
            &mut cpg,
            root,
            CpgNodeKind::Class {
                name: "Mid".into(),
                is_abstract: false,
            },
        );
        let leaf = child(
            &mut cpg,
            root,
            CpgNodeKind::Class {
                name: "Leaf".into(),
                is_abstract: false,
            },
        );
        let hierarchy = [(mid, base), (leaf, mid)];
        if reverse_edges {
            for &(subtype, parent) in hierarchy.iter().rev() {
                cpg.connect(subtype, parent, CpgEdgeKind::Inherits);
            }
        } else {
            for (subtype, parent) in hierarchy {
                cpg.connect(subtype, parent, CpgEdgeKind::Inherits);
            }
        }
        let base_method = method(&mut cpg, base, "run");
        let mid_method = method(&mut cpg, mid, "run");
        let leaf_method = method(&mut cpg, leaf, "run");

        let receiver_definition = child(
            &mut cpg,
            root,
            CpgNodeKind::Variable {
                name: "service".into(),
                var_type: Some(TypeInfo::new("Base")),
                scope: ScopeId::GLOBAL,
                is_mutable: true,
            },
        );
        child(
            &mut cpg,
            receiver_definition,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        );
        let initializer = cpg
            .ast_children(receiver_definition)
            .into_iter()
            .next()
            .expect("initializer");
        cpg.node_mut(initializer)
            .expect("initializer")
            .allocation_type = Some(TypeInfo::new("Leaf"));

        let mid_allocation = child(
            &mut cpg,
            root,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        );
        cpg.node_mut(mid_allocation)
            .expect("mid allocation")
            .allocation_type = Some(TypeInfo::new("Mid"));

        let call = child(
            &mut cpg,
            root,
            CpgNodeKind::Call {
                target: None,
                is_method: true,
            },
        );
        let member = child(
            &mut cpg,
            call,
            CpgNodeKind::MemberAccess {
                member: "run".into(),
            },
        );
        child(
            &mut cpg,
            member,
            CpgNodeKind::Identifier {
                name: "service".into(),
                definition: Some(receiver_definition),
            },
        );

        let mut resolution = CallGraphResolution::default();
        resolution.resolutions.insert(
            call,
            CallResolution {
                callee_name: Some("run".into()),
                scope: Some(root),
                confidence: Confidence::BareNameAmbiguous,
                target: None,
                candidates: vec![base_method, mid_method, leaf_method],
                candidate_count: 3,
                candidates_truncated: false,
            },
        );
        resolution.ambiguous = 1;
        Fixture {
            cpg,
            resolution,
            call,
            base_method,
            mid_method,
            leaf_method,
        }
    }

    fn hierarchy_fixture() -> Fixture {
        hierarchy_fixture_with_edge_order(false)
    }

    #[test]
    fn cha_rta_and_vta_form_the_required_monotone_refinement() {
        let fixture = hierarchy_fixture();
        let cha = devirtualize(&fixture.cpg, &fixture.resolution, DevirtualizationMode::Cha);
        let rta = devirtualize(&fixture.cpg, &fixture.resolution, DevirtualizationMode::Rta);
        let vta = devirtualize(&fixture.cpg, &fixture.resolution, DevirtualizationMode::Vta);

        assert_eq!(
            cha.resolutions[&fixture.call].candidates,
            vec![fixture.base_method, fixture.mid_method, fixture.leaf_method]
        );
        assert_eq!(
            rta.resolutions[&fixture.call].candidates,
            vec![fixture.mid_method, fixture.leaf_method]
        );
        assert_eq!(
            vta.resolutions[&fixture.call].candidates,
            vec![fixture.leaf_method]
        );
        assert_eq!(
            vta.resolutions[&fixture.call].target,
            Some(fixture.leaf_method)
        );
        assert_eq!(
            vta.resolutions[&fixture.call].confidence,
            Confidence::VariableType
        );
        assert_eq!(cha.topology_edges, 0);
        assert_eq!(rta.topology_edges, 0);
        assert_eq!(vta.topology_edges, 0);
    }

    #[test]
    fn inherited_dispatch_survives_rta_pruning() {
        let mut fixture = hierarchy_fixture();
        fixture
            .cpg
            .node_mut(fixture.leaf_method)
            .expect("leaf method")
            .kind = CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        };
        fixture
            .cpg
            .node_mut(fixture.mid_method)
            .expect("mid method")
            .kind = CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        };

        let rta = devirtualize(&fixture.cpg, &fixture.resolution, DevirtualizationMode::Rta);
        assert_eq!(
            rta.resolutions[&fixture.call].candidates,
            vec![fixture.base_method]
        );
    }

    #[test]
    fn vta_reads_assignment_allocations_from_reaching_definitions() {
        let mut fixture = hierarchy_fixture();
        let node_ids: Vec<_> = fixture.cpg.node_ids().collect();
        for node_id in node_ids {
            if fixture
                .cpg
                .node(node_id)
                .expect("represented node")
                .allocation_type
                .as_ref()
                .is_some_and(|type_info| type_info.name.as_ref() == "Leaf")
            {
                fixture
                    .cpg
                    .node_mut(node_id)
                    .expect("represented node")
                    .allocation_type = None;
            }
        }
        let receiver = receiver_of_call(&fixture.cpg, fixture.call).expect("receiver");
        let root = fixture.cpg.root().expect("root");
        let assignment = child(
            &mut fixture.cpg,
            root,
            CpgNodeKind::Assignment {
                operator: "=".into(),
            },
        );
        child(
            &mut fixture.cpg,
            assignment,
            CpgNodeKind::Identifier {
                name: "service".into(),
                definition: None,
            },
        );
        let allocation = child(
            &mut fixture.cpg,
            assignment,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        );
        fixture
            .cpg
            .node_mut(allocation)
            .expect("allocation")
            .allocation_type = Some(TypeInfo::new("Mid"));
        fixture.cpg.connect(
            assignment,
            receiver,
            CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
        );

        let vta = devirtualize(&fixture.cpg, &fixture.resolution, DevirtualizationMode::Vta);
        assert_eq!(
            vta.resolutions[&fixture.call].candidates,
            vec![fixture.mid_method]
        );
    }

    #[test]
    fn ambiguous_nominal_names_fail_closed_and_non_methods_are_unchanged() {
        let mut fixture = hierarchy_fixture();
        let root = fixture.cpg.root().expect("root");
        child(
            &mut fixture.cpg,
            root,
            CpgNodeKind::Class {
                name: "Base".into(),
                is_abstract: false,
            },
        );
        let unchanged = devirtualize(&fixture.cpg, &fixture.resolution, DevirtualizationMode::Cha);
        assert_eq!(
            unchanged.resolutions[&fixture.call],
            fixture.resolution.resolutions[&fixture.call]
        );

        fixture.cpg.node_mut(fixture.call).expect("call").kind = CpgNodeKind::Call {
            target: None,
            is_method: false,
        };
        let unchanged = devirtualize(&fixture.cpg, &fixture.resolution, DevirtualizationMode::Vta);
        assert_eq!(
            unchanged.resolutions[&fixture.call],
            fixture.resolution.resolutions[&fixture.call]
        );
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        #[test]
        fn hierarchy_edge_insertion_order_cannot_change_refinement(mode in 0_u8..3) {
            let mode = match mode {
                0 => DevirtualizationMode::Cha,
                1 => DevirtualizationMode::Rta,
                _ => DevirtualizationMode::Vta,
            };
            let forward = hierarchy_fixture_with_edge_order(false);
            let reversed = hierarchy_fixture_with_edge_order(true);
            let forward_result = devirtualize(&forward.cpg, &forward.resolution, mode);
            let reversed_result = devirtualize(&reversed.cpg, &reversed.resolution, mode);

            prop_assert_eq!(
                &forward_result.resolutions[&forward.call],
                &reversed_result.resolutions[&reversed.call]
            );

            let cha = devirtualize(
                &forward.cpg,
                &forward.resolution,
                DevirtualizationMode::Cha,
            );
            let rta = devirtualize(
                &forward.cpg,
                &forward.resolution,
                DevirtualizationMode::Rta,
            );
            let vta = devirtualize(
                &forward.cpg,
                &forward.resolution,
                DevirtualizationMode::Vta,
            );
            let cha: BTreeSet<_> = cha.resolutions[&forward.call]
                .candidates
                .iter()
                .copied()
                .collect();
            let rta: BTreeSet<_> = rta.resolutions[&forward.call]
                .candidates
                .iter()
                .copied()
                .collect();
            let vta: BTreeSet<_> = vta.resolutions[&forward.call]
                .candidates
                .iter()
                .copied()
                .collect();
            prop_assert!(vta.is_subset(&rta));
            prop_assert!(rta.is_subset(&cha));
        }
    }
}
