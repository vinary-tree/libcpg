//! Object-oriented and per-function source metrics over semantic CPG evidence.
//!
//! CK and LCOM values consume declared types, resolved calls, and the typed
//! field-access relations emitted by the DFG builder. Halstead, cyclomatic,
//! cognitive, and maintainability values classify existing [`CpgNodeKind`]
//! nodes; this module never reparses retained source text.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use crate::{CodePropertyGraph, CpgEdgeKind, CpgNodeKind, DfgEdgeKind, LiteralKind, NodeId};

use super::{function_scope_nodes, AnalysisError};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Selection policy for [`ck_metrics`].
///
/// Classes are always included. Structs are class-like in languages such as
/// Rust and Go and are included by default; enums are opt-in because many
/// languages use them only as closed value sets. Traits/interfaces are not
/// concrete classes and therefore do not produce rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CkOptions {
    /// Emit CK rows for `Struct` declarations.
    pub include_structs: bool,
    /// Emit CK rows for `Enum` declarations.
    pub include_enums: bool,
    /// Count `Implements` edges as inheritance for DIT and NOC.
    pub include_implements: bool,
}

impl Default for CkOptions {
    fn default() -> Self {
        Self {
            include_structs: true,
            include_enums: false,
            include_implements: false,
        }
    }
}

/// One resolved method-to-field access in a [`MethodFieldGraph`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MethodFieldAccess {
    /// Method containing the access expression.
    pub method: NodeId,
    /// `MemberAccess` or other reference node at the use site.
    pub access: NodeId,
    /// Field declaration reached by a `Reference` edge.
    pub field: NodeId,
    /// Whether a `FieldRead` relation touches `access`.
    pub reads: bool,
    /// Whether a `FieldWrite` relation touches `access`.
    pub writes: bool,
}

/// One resolved invocation between methods of the same nominal type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MethodCall {
    /// Calling method.
    pub caller: NodeId,
    /// Called method.
    pub callee: NodeId,
}

/// Real bipartite method-access-field evidence for one nominal type.
///
/// `methods` and `fields` are the two declaration partitions. Each item in
/// `accesses` identifies the intervening use-site node and its typed read/write
/// role. `method_calls` records resolved internal calls used by LCOM4.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MethodFieldGraph {
    /// Nominal type requested by the caller.
    pub class: NodeId,
    /// Direct methods, including uniquely associated implementation blocks.
    pub methods: Vec<NodeId>,
    /// Direct field declarations.
    pub fields: Vec<NodeId>,
    /// Resolved, typed field-access evidence.
    pub accesses: Vec<MethodFieldAccess>,
    /// Resolved calls whose two endpoints are methods of this type.
    pub method_calls: Vec<MethodCall>,
}

/// The five commonly distinguished LCOM variants.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LcomSuite {
    /// LCOM1: method pairs that share no field.
    pub lcom1: u32,
    /// LCOM2: `max(disjoint_pairs - sharing_pairs, 0)`.
    pub lcom2: u32,
    /// LCOM3: connected components under shared-field adjacency.
    pub lcom3: u32,
    /// LCOM4: connected components under shared fields or internal calls.
    pub lcom4: u32,
    /// Henderson-Sellers LCOM5, defined as zero for fewer than two methods or no fields.
    pub lcom5: f64,
}

/// Chidamber-Kemerer and cohesion metrics for one nominal type.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ClassMetrics {
    /// Class/struct/enum declaration represented by this row.
    pub class: NodeId,
    /// Weighted methods per class: sum of method cyclomatic complexities.
    pub wmc: u32,
    /// Depth of inheritance tree, with root classes at depth zero.
    pub dit: u32,
    /// Number of immediate children.
    pub noc: u32,
    /// Distinct other nominal types coupled by resolved calls or type edges.
    pub cbo: u32,
    /// Response for class: own methods plus distinct directly resolved callees.
    pub rfc: u32,
    /// LCOM1 through LCOM5 over the real method-field graph.
    pub lcom: LcomSuite,
}

/// Stack-safe hierarchy depth and direct-child count for one node.
///
/// A strongly connected set of cyclic parent relations is treated as one
/// hierarchy level. Every member of that component therefore receives the
/// same depth, measured as the longest path from a root component in the SCC
/// condensation DAG. `direct_children` counts distinct input children before
/// condensation, excluding self-relations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct HierarchyMetrics {
    /// Stable node identifier.
    pub node: NodeId,
    /// Longest root-to-node depth after cyclic components are condensed.
    pub depth: u32,
    /// Number of distinct nodes that directly name this node as a parent.
    pub direct_children: u32,
}

/// Computes deterministic metrics for an arbitrary child-to-parent hierarchy.
///
/// `nodes` defines the complete hierarchy domain. Relations whose child or
/// parent is outside that domain are ignored, while duplicate relations are
/// collapsed. Results contain every distinct domain node in ascending
/// [`NodeId`] order. Self-relations and larger cycles are condensed by libcpg's
/// iterative SCC engine, so hierarchy depth never consumes the native call
/// stack and remains finite for malformed cyclic input.
pub fn hierarchy_metrics(
    nodes: &[NodeId],
    child_to_parent: &[(NodeId, NodeId)],
) -> Vec<HierarchyMetrics> {
    let nodes: Vec<_> = nodes
        .iter()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if nodes.is_empty() {
        return Vec::new();
    }

    let domain: BTreeSet<_> = nodes.iter().copied().collect();
    let relations: Vec<_> = child_to_parent
        .iter()
        .copied()
        .filter(|(child, parent)| domain.contains(child) && domain.contains(parent))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let depths = inheritance_depths(&nodes, &relations);
    let mut children = BTreeMap::<NodeId, BTreeSet<NodeId>>::new();
    for &(child, parent) in &relations {
        if child != parent {
            children.entry(parent).or_default().insert(child);
        }
    }

    nodes
        .into_iter()
        .map(|node| HierarchyMetrics {
            node,
            depth: depths.get(&node).copied().unwrap_or(0),
            direct_children: u32_len(children.get(&node).map_or(0, BTreeSet::len)),
        })
        .collect()
}

/// Halstead's distinct and total operator/operand counts and derived values.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct HalsteadMetrics {
    /// Distinct operators, `η₁`.
    pub distinct_operators: u32,
    /// Distinct operands, `η₂`.
    pub distinct_operands: u32,
    /// Total operator occurrences, `N₁`.
    pub total_operators: u32,
    /// Total operand occurrences, `N₂`.
    pub total_operands: u32,
    /// Program vocabulary, `η = η₁ + η₂`.
    pub vocabulary: u32,
    /// Program length, `N = N₁ + N₂`.
    pub length: u64,
    /// Volume, `V = N log₂ η`.
    pub volume: f64,
    /// Difficulty, `D = (η₁ / 2)(N₂ / η₂)`.
    pub difficulty: f64,
    /// Effort, `E = D V`.
    pub effort: f64,
    /// Halstead's estimated delivered bugs, `V / 3000`.
    pub estimated_bugs: f64,
}

/// Language-independent metrics for one function CPG scope.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FunctionMetrics {
    /// Function declaration represented by this row.
    pub function: NodeId,
    /// McCabe cyclomatic complexity, `1 + decision points`.
    pub cyclomatic_complexity: u32,
    /// Campbell/Sonar cognitive complexity.
    pub cognitive_complexity: u32,
    /// Halstead counts and derived values.
    pub halstead: HalsteadMetrics,
    /// Inclusive source-line span of the function, with a minimum of one.
    pub logical_lines: u32,
    /// Distinct comment lines inside the function scope.
    pub comment_lines: u32,
    /// SEI maintainability index clamped to `[0, 100]`.
    pub maintainability_index: f64,
}

/// Builds the real method-field graph for `class`.
///
/// Only `Class`, `Struct`, `Enum`, and `Trait` nodes are accepted as nominal
/// owners. A missing or non-nominal id returns an empty graph carrying the
/// requested id. An access is included only when a `Reference` edge reaches a
/// field owned by this type and a typed `FieldRead` or `FieldWrite` relation
/// touches the access node. Same-named fields and methods are never guessed.
///
/// Rust-style sibling `Impl` blocks are associated only when their `for_type`
/// name resolves uniquely in the same declaration scope.
pub fn method_field_graph(cpg: &CodePropertyGraph, class: NodeId) -> MethodFieldGraph {
    if !is_nominal(cpg.node(class).map(|node| &node.kind)) {
        return MethodFieldGraph {
            class,
            methods: Vec::new(),
            fields: Vec::new(),
            accesses: Vec::new(),
            method_calls: Vec::new(),
        };
    }

    let nominal_index = nominal_index(cpg);
    let mut methods = BTreeSet::new();
    let mut fields = BTreeSet::new();
    collect_declared_members(cpg, class, true, &mut methods, &mut fields);

    let class_name = cpg
        .node(class)
        .and_then(|node| nominal_name(&node.kind))
        .expect("nominal node checked above");
    let class_scope = declaration_scope(cpg, class);
    for implementation in cpg.nodes().filter(|node| {
        matches!(
            &node.kind,
            CpgNodeKind::Impl {
                for_type: Some(for_type),
                ..
            } if for_type.as_ref() == class_name
        )
    }) {
        let key = (
            declaration_scope(cpg, implementation.id),
            class_name.to_owned(),
        );
        if class_scope == key.0
            && nominal_index
                .get(&key)
                .is_some_and(|candidates| candidates.as_slice() == [class])
        {
            collect_declared_members(cpg, implementation.id, false, &mut methods, &mut fields);
        }
    }

    let methods: Vec<_> = methods.into_iter().collect();
    let fields: Vec<_> = fields.into_iter().collect();
    let field_set: BTreeSet<_> = fields.iter().copied().collect();
    let method_set: BTreeSet<_> = methods.iter().copied().collect();
    let mut accesses = BTreeSet::new();

    for &method in &methods {
        for access in function_scope_nodes(cpg, method) {
            let targets: Vec<_> = cpg
                .outgoing_edges(access)
                .filter_map(|edge| {
                    (matches!(edge.kind, CpgEdgeKind::Reference)
                        && field_set.contains(&edge.target))
                    .then_some(edge.target)
                })
                .collect();
            if targets.is_empty() {
                continue;
            }
            let reads = cpg
                .incoming_edges(access)
                .chain(cpg.outgoing_edges(access))
                .any(|edge| matches!(edge.kind, CpgEdgeKind::DataFlow(DfgEdgeKind::FieldRead)));
            let writes = cpg
                .incoming_edges(access)
                .chain(cpg.outgoing_edges(access))
                .any(|edge| matches!(edge.kind, CpgEdgeKind::DataFlow(DfgEdgeKind::FieldWrite)));
            if !reads && !writes {
                continue;
            }
            for field in targets {
                accesses.insert(MethodFieldAccess {
                    method,
                    access,
                    field,
                    reads,
                    writes,
                });
            }
        }
    }

    let all_functions: Vec<_> = cpg.functions().map(|node| node.id).collect();
    let method_calls = super::scc::call_graph_edges(cpg, &all_functions)
        .into_iter()
        .filter(|(caller, callee)| method_set.contains(caller) && method_set.contains(callee))
        .map(|(caller, callee)| MethodCall { caller, callee })
        .collect();

    MethodFieldGraph {
        class,
        methods,
        fields,
        accesses: accesses.into_iter().collect(),
        method_calls,
    }
}

/// Computes CK and LCOM metrics, sorted by class [`NodeId`].
///
/// WMC and every LCOM variant work without call resolution. RFC, call-derived
/// CBO evidence, and LCOM4 use resolved call targets and call edges, so later
/// resolver improvements require no metrics API change. The computation is
/// read-only and bounded by the finite CPG node and edge sets.
pub fn ck_metrics(cpg: &CodePropertyGraph, options: &CkOptions) -> Vec<ClassMetrics> {
    let all_nominals: Vec<_> = sorted_ids(
        cpg.nodes()
            .filter(|node| is_nominal(Some(&node.kind)))
            .map(|node| node.id),
    );
    let classes: Vec<_> = all_nominals
        .iter()
        .copied()
        .filter(|&id| class_selected(cpg, id, options))
        .collect();
    let graphs: BTreeMap<_, _> = classes
        .iter()
        .copied()
        .map(|class| (class, method_field_graph(cpg, class)))
        .collect();

    let all_functions: Vec<_> = sorted_ids(cpg.functions().map(|node| node.id));
    let calls = super::scc::call_graph_edges(cpg, &all_functions);
    let nominal_lookup = nominal_index(cpg);
    let mut owner_by_method = BTreeMap::new();
    for &nominal in &all_nominals {
        for method in method_field_graph(cpg, nominal).methods {
            owner_by_method.entry(method).or_insert(nominal);
        }
    }

    let inheritance = inheritance_edges(cpg, &all_nominals, options.include_implements);
    let hierarchy: BTreeMap<_, _> = hierarchy_metrics(&all_nominals, &inheritance)
        .into_iter()
        .map(|metrics| (metrics.node, metrics))
        .collect();
    classes
        .into_iter()
        .map(|class| {
            let graph = &graphs[&class];
            let method_set: BTreeSet<_> = graph.methods.iter().copied().collect();
            let wmc = graph.methods.iter().fold(0u32, |sum, &method| {
                sum.saturating_add(function_metrics_unchecked(cpg, method).cyclomatic_complexity)
            });

            let hierarchy = hierarchy
                .get(&class)
                .expect("every selected class belongs to the nominal hierarchy domain");

            let mut response = method_set.clone();
            let mut coupled = BTreeSet::new();
            for &(caller, callee) in &calls {
                if method_set.contains(&caller) {
                    response.insert(callee);
                    if let Some(&owner) = owner_by_method.get(&callee) {
                        if owner != class {
                            coupled.insert(owner);
                        }
                    }
                }
                if method_set.contains(&callee) {
                    if let Some(&owner) = owner_by_method.get(&caller) {
                        if owner != class {
                            coupled.insert(owner);
                        }
                    }
                }
            }
            for edge in cpg.edges().filter(|edge| edge.kind.is_type()) {
                let source_owner = nominal_owner(cpg, edge.source, &nominal_lookup);
                let target_owner = nominal_owner(cpg, edge.target, &nominal_lookup);
                if source_owner == Some(class) {
                    if let Some(target_owner) = target_owner {
                        if target_owner != class {
                            coupled.insert(target_owner);
                        }
                    }
                }
                if target_owner == Some(class) {
                    if let Some(source_owner) = source_owner {
                        if source_owner != class {
                            coupled.insert(source_owner);
                        }
                    }
                }
            }

            ClassMetrics {
                class,
                wmc,
                dit: hierarchy.depth,
                noc: hierarchy.direct_children,
                cbo: u32_len(coupled.len()),
                rfc: u32_len(response.len()),
                lcom: lcom_suite(graph),
            }
        })
        .collect()
}

/// Computes Halstead, cyclomatic, cognitive, and maintainability metrics.
///
/// The selected node must be a function. Nested functions are excluded from
/// the analyzed scope. Operator and operand vocabularies are semantic CPG
/// categories and payloads rather than source tokens, making the classifier
/// language-independent and usable when source retention is disabled.
pub fn function_metrics(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> Result<FunctionMetrics, AnalysisError> {
    let Some(node) = cpg.node(function) else {
        return Err(AnalysisError::UnknownNode(function));
    };
    if !matches!(node.kind, CpgNodeKind::Function { .. }) {
        return Err(AnalysisError::NotAFunction(function));
    }
    Ok(function_metrics_unchecked(cpg, function))
}

fn function_metrics_unchecked(cpg: &CodePropertyGraph, function: NodeId) -> FunctionMetrics {
    let scope = function_scope_nodes(cpg, function);
    let cyclomatic_complexity = cyclomatic_complexity(cpg, function, &scope);
    let cognitive_complexity = cognitive_complexity(cpg, function);
    let halstead = halstead_metrics(cpg, function, &scope);
    let function_node = cpg
        .node(function)
        .expect("unchecked metrics are called only for known functions");
    let logical_lines = line_span(function_node.range.start_line, function_node.range.end_line);
    let comment_lines = comment_line_count(
        cpg,
        function,
        function_node.range.start_line,
        function_node
            .range
            .end_line
            .max(function_node.range.start_line),
    );
    let maintainability_index = maintainability_index(
        halstead.volume,
        cyclomatic_complexity,
        logical_lines,
        comment_lines,
    );
    FunctionMetrics {
        function,
        cyclomatic_complexity,
        cognitive_complexity,
        halstead,
        logical_lines,
        comment_lines,
        maintainability_index,
    }
}

fn collect_declared_members(
    cpg: &CodePropertyGraph,
    owner: NodeId,
    collect_fields: bool,
    methods: &mut BTreeSet<NodeId>,
    fields: &mut BTreeSet<NodeId>,
) {
    let mut visited = BTreeSet::from([owner]);
    let mut stack = cpg.ast_children(owner);
    stack.reverse();
    while let Some(node_id) = stack.pop() {
        if !visited.insert(node_id) {
            continue;
        }
        let Some(node) = cpg.node(node_id) else {
            continue;
        };
        match node.kind {
            CpgNodeKind::Function { .. } => {
                methods.insert(node_id);
            }
            CpgNodeKind::Field { .. } if collect_fields => {
                fields.insert(node_id);
            }
            CpgNodeKind::Class { .. }
            | CpgNodeKind::Struct { .. }
            | CpgNodeKind::Enum { .. }
            | CpgNodeKind::Trait { .. }
            | CpgNodeKind::Impl { .. } => {}
            _ => {
                let mut children = cpg.ast_children(node_id);
                children.reverse();
                stack.extend(children);
            }
        }
    }
}

fn nominal_index(cpg: &CodePropertyGraph) -> BTreeMap<(Option<NodeId>, String), Vec<NodeId>> {
    let mut index: BTreeMap<(Option<NodeId>, String), Vec<NodeId>> = BTreeMap::new();
    for node in cpg.nodes() {
        if let Some(name) = nominal_name(&node.kind) {
            index
                .entry((declaration_scope(cpg, node.id), name.to_owned()))
                .or_default()
                .push(node.id);
        }
    }
    for candidates in index.values_mut() {
        candidates.sort_unstable();
        candidates.dedup();
    }
    index
}

fn declaration_scope(cpg: &CodePropertyGraph, node: NodeId) -> Option<NodeId> {
    cpg.ast_ancestors(node).into_iter().find(|&ancestor| {
        matches!(
            cpg.node(ancestor).map(|node| &node.kind),
            Some(CpgNodeKind::Root | CpgNodeKind::Module { .. })
        )
    })
}

fn nominal_owner(
    cpg: &CodePropertyGraph,
    node: NodeId,
    index: &BTreeMap<(Option<NodeId>, String), Vec<NodeId>>,
) -> Option<NodeId> {
    for candidate in std::iter::once(node).chain(cpg.ast_ancestors(node)) {
        let kind = &cpg.node(candidate)?.kind;
        if nominal_name(kind).is_some() {
            return Some(candidate);
        }
        if let CpgNodeKind::Impl {
            for_type: Some(for_type),
            ..
        } = kind
        {
            let key = (declaration_scope(cpg, candidate), for_type.to_string());
            if let Some(owners) = index.get(&key) {
                if owners.len() == 1 {
                    return owners.first().copied();
                }
            }
            return None;
        }
    }
    None
}

fn nominal_name(kind: &CpgNodeKind) -> Option<&str> {
    match kind {
        CpgNodeKind::Class { name, .. }
        | CpgNodeKind::Struct { name }
        | CpgNodeKind::Enum { name }
        | CpgNodeKind::Trait { name } => Some(name),
        _ => None,
    }
}

fn is_nominal(kind: Option<&CpgNodeKind>) -> bool {
    kind.and_then(nominal_name).is_some()
}

fn class_selected(cpg: &CodePropertyGraph, class: NodeId, options: &CkOptions) -> bool {
    matches!(
        cpg.node(class).map(|node| &node.kind),
        Some(CpgNodeKind::Class { .. })
    ) || (options.include_structs
        && matches!(
            cpg.node(class).map(|node| &node.kind),
            Some(CpgNodeKind::Struct { .. })
        ))
        || (options.include_enums
            && matches!(
                cpg.node(class).map(|node| &node.kind),
                Some(CpgNodeKind::Enum { .. })
            ))
}

fn inheritance_edges(
    cpg: &CodePropertyGraph,
    nominals: &[NodeId],
    include_implements: bool,
) -> Vec<(NodeId, NodeId)> {
    let nominal_set: BTreeSet<_> = nominals.iter().copied().collect();
    let mut edges: Vec<_> = cpg
        .edges()
        .filter_map(|edge| {
            let selected = matches!(edge.kind, CpgEdgeKind::Inherits)
                || (include_implements && matches!(edge.kind, CpgEdgeKind::Implements));
            (selected && nominal_set.contains(&edge.source) && nominal_set.contains(&edge.target))
                .then_some((edge.source, edge.target))
        })
        .collect();
    edges.sort_unstable();
    edges.dedup();
    edges
}

fn inheritance_depths(
    nominals: &[NodeId],
    inheritance: &[(NodeId, NodeId)],
) -> BTreeMap<NodeId, u32> {
    if nominals.is_empty() {
        return BTreeMap::new();
    }
    let adjacency = super::scc::adjacency_from_edges(nominals, inheritance.iter().copied());
    let decomposition = super::scc::decompose(
        super::SccProjection::Projection(super::ProjectionKind::External),
        nominals.to_vec(),
        adjacency,
    );
    let component_count = decomposition.components.len();
    let mut parents = vec![BTreeSet::new(); component_count];
    let mut children = vec![BTreeSet::new(); component_count];
    for &(child, parent) in inheritance {
        let Some(&child_component) = decomposition.component_by_node.get(&child) else {
            continue;
        };
        let Some(&parent_component) = decomposition.component_by_node.get(&parent) else {
            continue;
        };
        if child_component != parent_component {
            parents[child_component].insert(parent_component);
            children[parent_component].insert(child_component);
        }
    }
    let mut remaining_parents: Vec<_> = parents.iter().map(BTreeSet::len).collect();
    let mut depth = vec![0u32; component_count];
    let mut queue: VecDeque<_> = (0..component_count)
        .filter(|&component| remaining_parents[component] == 0)
        .collect();
    while let Some(parent) = queue.pop_front() {
        for &child in &children[parent] {
            depth[child] = depth[child].max(depth[parent].saturating_add(1));
            remaining_parents[child] -= 1;
            if remaining_parents[child] == 0 {
                queue.push_back(child);
            }
        }
    }
    decomposition
        .component_by_node
        .into_iter()
        .map(|(node, component)| (node, depth[component]))
        .collect()
}

fn lcom_suite(graph: &MethodFieldGraph) -> LcomSuite {
    let method_count = graph.methods.len();
    if method_count == 0 {
        return LcomSuite {
            lcom1: 0,
            lcom2: 0,
            lcom3: 0,
            lcom4: 0,
            lcom5: 0.0,
        };
    }
    let index: BTreeMap<_, _> = graph
        .methods
        .iter()
        .copied()
        .enumerate()
        .map(|(index, method)| (method, index))
        .collect();
    let mut method_fields = vec![BTreeSet::new(); method_count];
    for access in &graph.accesses {
        if let Some(&method) = index.get(&access.method) {
            method_fields[method].insert(access.field);
        }
    }

    let mut disjoint_pairs = 0usize;
    let mut sharing_pairs = 0usize;
    let mut field_union = UnionFind::new(method_count);
    for left in 0..method_count {
        for right in left + 1..method_count {
            if method_fields[left].is_disjoint(&method_fields[right]) {
                disjoint_pairs = disjoint_pairs.saturating_add(1);
            } else {
                sharing_pairs = sharing_pairs.saturating_add(1);
                field_union.union(left, right);
            }
        }
    }
    let mut call_union = field_union.clone();
    for call in &graph.method_calls {
        if let (Some(&caller), Some(&callee)) = (index.get(&call.caller), index.get(&call.callee)) {
            call_union.union(caller, callee);
        }
    }

    let lcom5 = if method_count < 2 || graph.fields.is_empty() {
        0.0
    } else {
        let total_usage: usize = graph
            .fields
            .iter()
            .map(|field| {
                method_fields
                    .iter()
                    .filter(|fields| fields.contains(field))
                    .count()
            })
            .sum();
        let average_usage = total_usage as f64 / graph.fields.len() as f64;
        ((method_count as f64 - average_usage) / (method_count - 1) as f64).clamp(0.0, 1.0)
    };

    LcomSuite {
        lcom1: u32_len(disjoint_pairs),
        lcom2: u32_len(disjoint_pairs.saturating_sub(sharing_pairs)),
        lcom3: u32_len(field_union.components()),
        lcom4: u32_len(call_union.components()),
        lcom5,
    }
}

#[derive(Debug, Clone)]
struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl UnionFind {
    fn new(size: usize) -> Self {
        Self {
            parent: (0..size).collect(),
            rank: vec![0; size],
        }
    }

    fn find(&mut self, mut node: usize) -> usize {
        let mut root = node;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        while self.parent[node] != node {
            let next = self.parent[node];
            self.parent[node] = root;
            node = next;
        }
        root
    }

    fn union(&mut self, left: usize, right: usize) {
        let mut left_root = self.find(left);
        let mut right_root = self.find(right);
        if left_root == right_root {
            return;
        }
        if self.rank[left_root] < self.rank[right_root] {
            std::mem::swap(&mut left_root, &mut right_root);
        }
        self.parent[right_root] = left_root;
        if self.rank[left_root] == self.rank[right_root] {
            self.rank[left_root] = self.rank[left_root].saturating_add(1);
        }
    }

    fn components(&mut self) -> usize {
        (0..self.parent.len())
            .map(|node| self.find(node))
            .collect::<BTreeSet<_>>()
            .len()
    }
}

fn cyclomatic_complexity(
    cpg: &CodePropertyGraph,
    _function: NodeId,
    scope: &rustc_hash::FxHashSet<NodeId>,
) -> u32 {
    let mut decisions = 0u32;
    for &node_id in scope {
        let Some(node) = cpg.node(node_id) else {
            continue;
        };
        match &node.kind {
            CpgNodeKind::If
            | CpgNodeKind::While
            | CpgNodeKind::For
            | CpgNodeKind::Loop
            | CpgNodeKind::Catch => decisions = decisions.saturating_add(1),
            CpgNodeKind::BinaryOp { operator }
                if matches!(operator.as_ref(), "&&" | "||" | "??" | "and" | "or") =>
            {
                decisions = decisions.saturating_add(1);
            }
            CpgNodeKind::Match => {
                let arms = cpg
                    .ast_children(node_id)
                    .into_iter()
                    .filter(|&child| {
                        matches!(
                            cpg.node(child).map(|node| &node.kind),
                            Some(CpgNodeKind::MatchArm)
                        )
                    })
                    .count();
                decisions = decisions.saturating_add(u32_len(arms.saturating_sub(1)));
            }
            _ => {}
        }
    }
    decisions.saturating_add(1)
}

fn cognitive_complexity(cpg: &CodePropertyGraph, function: NodeId) -> u32 {
    let mut total = 0u32;
    let mut visited = BTreeSet::from([function]);
    let mut stack: Vec<_> = cpg
        .ast_children(function)
        .into_iter()
        .rev()
        .map(|child| (child, 0u32))
        .collect();
    while let Some((node_id, depth)) = stack.pop() {
        if !visited.insert(node_id) {
            continue;
        }
        let Some(node) = cpg.node(node_id) else {
            continue;
        };
        if matches!(node.kind, CpgNodeKind::Function { .. }) {
            continue;
        }

        let nesting_event = matches!(
            node.kind,
            CpgNodeKind::If
                | CpgNodeKind::While
                | CpgNodeKind::For
                | CpgNodeKind::Loop
                | CpgNodeKind::Match
                | CpgNodeKind::Catch
        );
        if nesting_event {
            total = total.saturating_add(depth.saturating_add(1));
        }
        match &node.kind {
            CpgNodeKind::Break | CpgNodeKind::Continue => total = total.saturating_add(1),
            CpgNodeKind::BinaryOp { operator }
                if matches!(operator.as_ref(), "&&" | "||" | "??" | "and" | "or") =>
            {
                total = total.saturating_add(1);
            }
            CpgNodeKind::Call {
                target: Some(target),
                ..
            } if *target == function => total = total.saturating_add(1),
            _ => {}
        }

        let increases_depth = matches!(
            node.kind,
            CpgNodeKind::If
                | CpgNodeKind::While
                | CpgNodeKind::For
                | CpgNodeKind::Loop
                | CpgNodeKind::Match
                | CpgNodeKind::Lambda { .. }
        );
        let child_depth = if increases_depth {
            depth.saturating_add(1)
        } else {
            depth
        };
        let mut children = cpg.ast_children(node_id);
        children.reverse();
        for child in children {
            stack.push((child, child_depth));
        }
    }
    total
}

fn halstead_metrics(
    cpg: &CodePropertyGraph,
    function: NodeId,
    scope: &rustc_hash::FxHashSet<NodeId>,
) -> HalsteadMetrics {
    let mut operators = BTreeMap::<String, u32>::new();
    let mut operands = BTreeMap::<String, u32>::new();
    for &node_id in scope {
        if node_id == function {
            continue;
        }
        let Some(node) = cpg.node(node_id) else {
            continue;
        };
        if let Some(operator) = operator_key(&node.kind) {
            increment(&mut operators, operator);
        }
        if let Some(operand) = operand_key(&node.kind) {
            increment(&mut operands, operand);
        }
    }
    let distinct_operators = u32_len(operators.len());
    let distinct_operands = u32_len(operands.len());
    let total_operators = operators.values().copied().fold(0u32, u32::saturating_add);
    let total_operands = operands.values().copied().fold(0u32, u32::saturating_add);
    let vocabulary = distinct_operators.saturating_add(distinct_operands);
    let length = u64::from(total_operators) + u64::from(total_operands);
    let volume = if vocabulary == 0 {
        0.0
    } else {
        length as f64 * f64::from(vocabulary).log2()
    };
    let difficulty = if distinct_operators == 0 || distinct_operands == 0 {
        0.0
    } else {
        (f64::from(distinct_operators) / 2.0)
            * (f64::from(total_operands) / f64::from(distinct_operands))
    };
    HalsteadMetrics {
        distinct_operators,
        distinct_operands,
        total_operators,
        total_operands,
        vocabulary,
        length,
        volume,
        difficulty,
        effort: difficulty * volume,
        estimated_bugs: volume / 3000.0,
    }
}

fn increment(counts: &mut BTreeMap<String, u32>, key: String) {
    let count = counts.entry(key).or_default();
    *count = count.saturating_add(1);
}

fn operator_key(kind: &CpgNodeKind) -> Option<String> {
    let key = match kind {
        CpgNodeKind::Return => "return",
        CpgNodeKind::If => "if",
        CpgNodeKind::Else => "else",
        CpgNodeKind::While => "while",
        CpgNodeKind::For => "for",
        CpgNodeKind::Loop => "loop",
        CpgNodeKind::Match => "match",
        CpgNodeKind::MatchArm => "match-arm",
        CpgNodeKind::Break => "break",
        CpgNodeKind::Continue => "continue",
        CpgNodeKind::Throw => "throw",
        CpgNodeKind::Try => "try",
        CpgNodeKind::Catch => "catch",
        CpgNodeKind::Finally => "finally",
        CpgNodeKind::BinaryOp { operator }
        | CpgNodeKind::UnaryOp { operator }
        | CpgNodeKind::Assignment { operator } => return Some(operator.to_string()),
        CpgNodeKind::Call { .. } => "call",
        CpgNodeKind::MemberAccess { .. } => "member-access",
        CpgNodeKind::IndexAccess => "index-access",
        CpgNodeKind::Lambda { .. } => "lambda",
        CpgNodeKind::Await => "await",
        CpgNodeKind::Yield => "yield",
        CpgNodeKind::Macro { name } => return Some(format!("macro:{name}")),
        _ => return None,
    };
    Some(key.to_owned())
}

fn operand_key(kind: &CpgNodeKind) -> Option<String> {
    match kind {
        CpgNodeKind::Parameter { name, .. }
        | CpgNodeKind::Variable { name, .. }
        | CpgNodeKind::Field { name, .. }
        | CpgNodeKind::Identifier { name, .. } => Some(format!("symbol:{name}")),
        CpgNodeKind::MemberAccess { member } => Some(format!("symbol:{member}")),
        CpgNodeKind::Literal { kind } => Some(literal_key(kind)),
        CpgNodeKind::GenericParam { name } => Some(format!("generic:{name}")),
        CpgNodeKind::TypeAnnotation { type_info } => Some(format!("type:{}", type_info.name)),
        _ => None,
    }
}

fn literal_key(literal: &LiteralKind) -> String {
    match literal {
        LiteralKind::Integer(value) => format!("integer:{value}"),
        LiteralKind::Float(value) => format!("float:{:016x}", value.to_bits()),
        LiteralKind::String(value) => format!("string:{value}"),
        LiteralKind::Char(value) => format!("char:{value}"),
        LiteralKind::Bool(value) => format!("bool:{value}"),
        LiteralKind::Null => "null".to_owned(),
        LiteralKind::Array => "array".to_owned(),
        LiteralKind::Object => "object".to_owned(),
        LiteralKind::Regex(value) => format!("regex:{value}"),
    }
}

fn line_span(start: u32, end: u32) -> u32 {
    end.saturating_sub(start).saturating_add(1)
}

fn comment_line_count(
    cpg: &CodePropertyGraph,
    function: NodeId,
    function_start: u32,
    function_end: u32,
) -> u32 {
    let mut ranges: Vec<_> = function_scope_nodes(cpg, function)
        .into_iter()
        .filter_map(|node_id| {
            let node = cpg.node(node_id)?;
            if node.range.end_line < function_start || node.range.start_line > function_end {
                return None;
            }
            matches!(node.kind, CpgNodeKind::Comment { .. }).then_some((
                node.range.start_line.max(function_start).min(function_end),
                node.range
                    .end_line
                    .max(node.range.start_line)
                    .min(function_end),
            ))
        })
        .collect();
    ranges.sort_unstable();
    let mut total = 0u32;
    let mut current: Option<(u32, u32)> = None;
    for (start, end) in ranges {
        match current {
            Some((current_start, current_end)) if start <= current_end.saturating_add(1) => {
                current = Some((current_start, current_end.max(end)));
            }
            Some((current_start, current_end)) => {
                total = total.saturating_add(line_span(current_start, current_end));
                current = Some((start, end));
            }
            None => current = Some((start, end)),
        }
    }
    if let Some((start, end)) = current {
        total = total.saturating_add(line_span(start, end));
    }
    total
}

fn maintainability_index(volume: f64, complexity: u32, lines: u32, comments: u32) -> f64 {
    let volume = volume.max(1.0);
    let lines_f64 = f64::from(lines.max(1));
    let comment_ratio = if lines == 0 {
        0.0
    } else {
        f64::from(comments) / f64::from(lines)
    };
    let value = 171.0 - 5.2 * volume.ln() - 0.23 * f64::from(complexity) - 16.2 * lines_f64.ln()
        + 50.0 * (2.4 * comment_ratio).max(0.0).sqrt().sin();
    value.clamp(0.0, 100.0)
}

fn sorted_ids(ids: impl IntoIterator<Item = NodeId>) -> Vec<NodeId> {
    let mut ids: Vec<_> = ids.into_iter().collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

fn u32_len(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::{CpgNode, Language, MethodSignature, ScopeId, SourceRange, TypeInfo, Visibility};

    fn node(id: u32, kind: CpgNodeKind, parent: Option<u32>) -> CpgNode {
        let node = CpgNode::new(NodeId::new(id), kind, SourceRange::default());
        match parent {
            Some(parent) => node.with_parent(NodeId::new(parent)),
            None => node,
        }
    }

    fn ranged_node(
        id: u32,
        kind: CpgNodeKind,
        parent: Option<u32>,
        start_line: u32,
        end_line: u32,
    ) -> CpgNode {
        let range = SourceRange {
            start_line,
            end_line,
            ..SourceRange::default()
        };
        let node = CpgNode::new(NodeId::new(id), kind, range);
        match parent {
            Some(parent) => node.with_parent(NodeId::new(parent)),
            None => node,
        }
    }

    fn function(name: &str) -> CpgNodeKind {
        CpgNodeKind::Function {
            signature: MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Public,
            },
        }
    }

    fn field(name: &str) -> CpgNodeKind {
        CpgNodeKind::Field {
            name: name.into(),
            field_type: None,
            visibility: Visibility::Private,
        }
    }

    fn add_ast(cpg: &mut CodePropertyGraph, parent: u32, child: u32) {
        cpg.connect(
            NodeId::new(parent),
            NodeId::new(child),
            CpgEdgeKind::AstChild,
        )
        .expect("AST endpoints exist");
    }

    fn class_fixture() -> CodePropertyGraph {
        let mut cpg = CodePropertyGraph::new(Language::Java);
        for declaration in [
            node(
                1,
                CpgNodeKind::Class {
                    name: "Base".into(),
                    is_abstract: false,
                },
                None,
            ),
            node(
                2,
                CpgNodeKind::Class {
                    name: "C".into(),
                    is_abstract: false,
                },
                None,
            ),
            node(
                3,
                CpgNodeKind::Class {
                    name: "Child".into(),
                    is_abstract: false,
                },
                None,
            ),
            node(
                4,
                CpgNodeKind::Class {
                    name: "D".into(),
                    is_abstract: false,
                },
                None,
            ),
            node(10, field("x"), Some(2)),
            node(11, field("y"), Some(2)),
            node(20, function("first"), Some(2)),
            node(21, CpgNodeKind::If, Some(20)),
            node(
                22,
                CpgNodeKind::MemberAccess { member: "x".into() },
                Some(20),
            ),
            node(
                23,
                CpgNodeKind::Call {
                    target: Some(NodeId::new(50)),
                    is_method: true,
                },
                Some(20),
            ),
            node(30, function("second"), Some(2)),
            node(31, CpgNodeKind::While, Some(30)),
            node(
                32,
                CpgNodeKind::MemberAccess { member: "x".into() },
                Some(30),
            ),
            node(
                33,
                CpgNodeKind::Assignment {
                    operator: "=".into(),
                },
                Some(30),
            ),
            node(
                34,
                CpgNodeKind::Call {
                    target: Some(NodeId::new(40)),
                    is_method: true,
                },
                Some(30),
            ),
            node(40, function("third"), Some(2)),
            node(
                42,
                CpgNodeKind::MemberAccess { member: "y".into() },
                Some(40),
            ),
            node(50, function("external"), Some(4)),
            node(
                51,
                CpgNodeKind::Call {
                    target: Some(NodeId::new(40)),
                    is_method: true,
                },
                Some(50),
            ),
        ] {
            cpg.add_node_with_id(declaration);
        }
        for (parent, child) in [
            (2, 10),
            (2, 11),
            (2, 20),
            (20, 21),
            (20, 22),
            (20, 23),
            (2, 30),
            (30, 31),
            (30, 32),
            (30, 33),
            (30, 34),
            (2, 40),
            (40, 42),
            (4, 50),
            (50, 51),
        ] {
            add_ast(&mut cpg, parent, child);
        }
        cpg.connect(NodeId::new(22), NodeId::new(10), CpgEdgeKind::Reference);
        cpg.connect(
            NodeId::new(20),
            NodeId::new(22),
            CpgEdgeKind::DataFlow(DfgEdgeKind::FieldRead),
        );
        cpg.connect(NodeId::new(32), NodeId::new(10), CpgEdgeKind::Reference);
        cpg.connect(
            NodeId::new(33),
            NodeId::new(32),
            CpgEdgeKind::DataFlow(DfgEdgeKind::FieldWrite),
        );
        cpg.connect(NodeId::new(42), NodeId::new(11), CpgEdgeKind::Reference);
        cpg.connect(
            NodeId::new(40),
            NodeId::new(42),
            CpgEdgeKind::DataFlow(DfgEdgeKind::FieldRead),
        );
        cpg.connect(NodeId::new(2), NodeId::new(1), CpgEdgeKind::Inherits);
        cpg.connect(NodeId::new(3), NodeId::new(2), CpgEdgeKind::Inherits);
        cpg
    }

    #[test]
    fn hand_class_has_exact_ck_and_lcom_suite() {
        let cpg = class_fixture();
        let graph = method_field_graph(&cpg, NodeId::new(2));
        assert_eq!(
            graph.methods,
            vec![NodeId::new(20), NodeId::new(30), NodeId::new(40)]
        );
        assert_eq!(graph.fields, vec![NodeId::new(10), NodeId::new(11)]);
        assert_eq!(graph.accesses.len(), 3);
        assert_eq!(
            graph.method_calls,
            vec![MethodCall {
                caller: NodeId::new(30),
                callee: NodeId::new(40),
            }]
        );

        let metrics = ck_metrics(&cpg, &CkOptions::default());
        assert_eq!(
            metrics.iter().map(|row| row.class).collect::<Vec<_>>(),
            vec![
                NodeId::new(1),
                NodeId::new(2),
                NodeId::new(3),
                NodeId::new(4)
            ]
        );
        let row = metrics
            .iter()
            .find(|row| row.class == NodeId::new(2))
            .expect("C metrics");
        assert_eq!(
            (row.wmc, row.dit, row.noc, row.cbo, row.rfc),
            (5, 1, 1, 3, 4)
        );
        assert_eq!((row.lcom.lcom1, row.lcom.lcom2), (2, 1));
        assert_eq!((row.lcom.lcom3, row.lcom.lcom4), (2, 1));
        assert!((row.lcom.lcom5 - 0.75).abs() < 1e-12);

        let base = metrics
            .iter()
            .find(|row| row.class == NodeId::new(1))
            .expect("base metrics");
        let child = metrics
            .iter()
            .find(|row| row.class == NodeId::new(3))
            .expect("child metrics");
        assert_eq!((base.dit, base.noc), (0, 1));
        assert_eq!((child.dit, child.noc), (2, 0));
    }

    #[test]
    fn method_field_graph_requires_reference_and_typed_access_evidence() {
        let mut cpg = class_fixture();
        cpg.add_node_with_id(node(
            60,
            CpgNodeKind::MemberAccess { member: "y".into() },
            Some(20),
        ));
        add_ast(&mut cpg, 20, 60);
        cpg.connect(NodeId::new(60), NodeId::new(11), CpgEdgeKind::Reference);
        let graph = method_field_graph(&cpg, NodeId::new(2));
        assert!(!graph
            .accesses
            .iter()
            .any(|access| access.access == NodeId::new(60)));

        cpg.connect(
            NodeId::new(20),
            NodeId::new(60),
            CpgEdgeKind::DataFlow(DfgEdgeKind::FieldRead),
        );
        let graph = method_field_graph(&cpg, NodeId::new(2));
        assert!(graph.accesses.iter().any(|access| {
            access.access == NodeId::new(60)
                && access.field == NodeId::new(11)
                && access.reads
                && !access.writes
        }));
    }

    #[test]
    fn uniquely_scoped_impl_methods_attach_to_struct_but_ambiguous_names_do_not() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        for declaration in [
            node(0, CpgNodeKind::Root, None),
            node(1, CpgNodeKind::Module { name: "m".into() }, Some(0)),
            node(10, CpgNodeKind::Struct { name: "S".into() }, Some(1)),
            node(11, field("x"), Some(10)),
            node(
                20,
                CpgNodeKind::Impl {
                    for_type: Some("S".into()),
                    trait_name: None,
                },
                Some(1),
            ),
            node(21, function("get"), Some(20)),
            node(
                22,
                CpgNodeKind::MemberAccess { member: "x".into() },
                Some(21),
            ),
        ] {
            cpg.add_node_with_id(declaration);
        }
        for (parent, child) in [(0, 1), (1, 10), (10, 11), (1, 20), (20, 21), (21, 22)] {
            add_ast(&mut cpg, parent, child);
        }
        cpg.connect(NodeId::new(22), NodeId::new(11), CpgEdgeKind::Reference);
        cpg.connect(
            NodeId::new(21),
            NodeId::new(22),
            CpgEdgeKind::DataFlow(DfgEdgeKind::FieldRead),
        );

        let graph = method_field_graph(&cpg, NodeId::new(10));
        assert_eq!(graph.methods, vec![NodeId::new(21)]);
        assert_eq!(graph.accesses.len(), 1);

        cpg.add_node_with_id(node(12, CpgNodeKind::Struct { name: "S".into() }, Some(1)));
        add_ast(&mut cpg, 1, 12);
        assert!(method_field_graph(&cpg, NodeId::new(10)).methods.is_empty());
        assert!(method_field_graph(&cpg, NodeId::new(12)).methods.is_empty());
    }

    #[test]
    fn function_metrics_match_halstead_cognitive_and_mi_oracles() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let declarations = [
            ranged_node(1, function("f"), None, 10, 19),
            node(
                2,
                CpgNodeKind::Variable {
                    name: "x".into(),
                    var_type: None,
                    scope: ScopeId::GLOBAL,
                    is_mutable: true,
                },
                Some(1),
            ),
            node(
                3,
                CpgNodeKind::Identifier {
                    name: "x".into(),
                    definition: Some(NodeId::new(2)),
                },
                Some(1),
            ),
            node(
                4,
                CpgNodeKind::Identifier {
                    name: "x".into(),
                    definition: Some(NodeId::new(2)),
                },
                Some(1),
            ),
            node(
                5,
                CpgNodeKind::Literal {
                    kind: LiteralKind::Integer(1),
                },
                Some(1),
            ),
            node(
                6,
                CpgNodeKind::Literal {
                    kind: LiteralKind::Integer(2),
                },
                Some(1),
            ),
            node(
                7,
                CpgNodeKind::BinaryOp {
                    operator: "+".into(),
                },
                Some(1),
            ),
            node(
                8,
                CpgNodeKind::BinaryOp {
                    operator: "+".into(),
                },
                Some(1),
            ),
            node(9, CpgNodeKind::Return, Some(1)),
            node(
                10,
                CpgNodeKind::Assignment {
                    operator: "=".into(),
                },
                Some(1),
            ),
            ranged_node(11, CpgNodeKind::Comment { is_doc: false }, Some(1), 11, 11),
            ranged_node(12, CpgNodeKind::Comment { is_doc: true }, Some(1), 12, 13),
            ranged_node(13, CpgNodeKind::Comment { is_doc: false }, Some(1), 13, 13),
            ranged_node(14, CpgNodeKind::Comment { is_doc: false }, Some(1), 30, 31),
            ranged_node(15, CpgNodeKind::Comment { is_doc: false }, Some(1), 16, 16),
        ];
        for declaration in declarations {
            cpg.add_node_with_id(declaration);
        }
        for child in 2..=15 {
            add_ast(&mut cpg, 1, child);
        }

        let metrics = function_metrics(&cpg, NodeId::new(1)).expect("function metrics");
        assert_eq!(metrics.cyclomatic_complexity, 1);
        assert_eq!(metrics.cognitive_complexity, 0);
        assert_eq!(
            (
                metrics.halstead.distinct_operators,
                metrics.halstead.distinct_operands,
                metrics.halstead.total_operators,
                metrics.halstead.total_operands,
                metrics.halstead.vocabulary,
                metrics.halstead.length,
            ),
            (3, 3, 4, 5, 6, 9)
        );
        let expected_volume = 9.0 * 6.0_f64.log2();
        assert!((metrics.halstead.volume - expected_volume).abs() < 1e-12);
        assert!((metrics.halstead.difficulty - 2.5).abs() < 1e-12);
        assert!((metrics.halstead.effort - 2.5 * expected_volume).abs() < 1e-12);
        assert!((metrics.halstead.estimated_bugs - expected_volume / 3000.0).abs() < 1e-12);
        assert_eq!((metrics.logical_lines, metrics.comment_lines), (10, 4));

        // Numeric cross-check against pgmcp/src/parsing/complexity.rs.
        let expected_mi =
            (171.0 - 5.2 * expected_volume.max(1.0).ln() - 0.23 - 16.2 * 10.0_f64.ln()
                + 50.0 * (2.4_f64 * 4.0 / 10.0).sqrt().sin())
            .clamp(0.0, 100.0);
        assert!((metrics.maintainability_index - expected_mi).abs() < 1e-12);
    }

    #[test]
    fn nested_control_flow_logical_sequences_and_recursion_score_cognitively() {
        let mut cpg = CodePropertyGraph::new(Language::JavaScript);
        for declaration in [
            node(1, function("f"), None),
            node(2, CpgNodeKind::If, Some(1)),
            node(3, CpgNodeKind::While, Some(2)),
            node(
                4,
                CpgNodeKind::BinaryOp {
                    operator: "&&".into(),
                },
                Some(3),
            ),
            node(5, CpgNodeKind::Break, Some(3)),
            node(
                6,
                CpgNodeKind::Call {
                    target: Some(NodeId::new(1)),
                    is_method: false,
                },
                Some(3),
            ),
            node(7, function("nested"), Some(3)),
            node(8, CpgNodeKind::If, Some(7)),
        ] {
            cpg.add_node_with_id(declaration);
        }
        for (parent, child) in [(1, 2), (2, 3), (3, 4), (3, 5), (3, 6), (3, 7), (7, 8)] {
            add_ast(&mut cpg, parent, child);
        }
        let metrics = function_metrics(&cpg, NodeId::new(1)).expect("function metrics");
        assert_eq!(metrics.cyclomatic_complexity, 4);
        assert_eq!(metrics.cognitive_complexity, 6);
    }

    #[test]
    fn semantic_operator_and_operand_taxonomy_is_complete() {
        let mut cpg = CodePropertyGraph::new(Language::Unknown);
        let kinds = vec![
            function("taxonomy"),
            CpgNodeKind::Else,
            CpgNodeKind::For,
            CpgNodeKind::Loop,
            CpgNodeKind::Match,
            CpgNodeKind::MatchArm,
            CpgNodeKind::MatchArm,
            CpgNodeKind::Continue,
            CpgNodeKind::Throw,
            CpgNodeKind::Try,
            CpgNodeKind::Catch,
            CpgNodeKind::Finally,
            CpgNodeKind::UnaryOp {
                operator: "-".into(),
            },
            CpgNodeKind::Assignment {
                operator: "+=".into(),
            },
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
            CpgNodeKind::MemberAccess { member: "m".into() },
            CpgNodeKind::IndexAccess,
            CpgNodeKind::Lambda {
                captures: Default::default(),
            },
            CpgNodeKind::Await,
            CpgNodeKind::Yield,
            CpgNodeKind::Macro {
                name: "expand".into(),
            },
            CpgNodeKind::Parameter {
                name: "p".into(),
                param_type: None,
                is_variadic: false,
            },
            field("f"),
            CpgNodeKind::GenericParam { name: "G".into() },
            CpgNodeKind::TypeAnnotation {
                type_info: TypeInfo::new("T"),
            },
            CpgNodeKind::Literal {
                kind: LiteralKind::Integer(-1),
            },
            CpgNodeKind::Literal {
                kind: LiteralKind::Float(1.5),
            },
            CpgNodeKind::Literal {
                kind: LiteralKind::String("text".into()),
            },
            CpgNodeKind::Literal {
                kind: LiteralKind::Char('x'),
            },
            CpgNodeKind::Literal {
                kind: LiteralKind::Bool(true),
            },
            CpgNodeKind::Literal {
                kind: LiteralKind::Null,
            },
            CpgNodeKind::Literal {
                kind: LiteralKind::Array,
            },
            CpgNodeKind::Literal {
                kind: LiteralKind::Object,
            },
            CpgNodeKind::Literal {
                kind: LiteralKind::Regex("a+".into()),
            },
        ];
        for (index, kind) in kinds.into_iter().enumerate() {
            let id = u32::try_from(index + 1).expect("small fixture");
            let parent = match id {
                1 => None,
                6 | 7 => Some(5),
                _ => Some(1),
            };
            cpg.add_node_with_id(node(id, kind, parent));
            if id != 1 {
                add_ast(&mut cpg, parent.expect("non-root parent"), id);
            }
        }

        let metrics = function_metrics(&cpg, NodeId::new(1)).expect("function metrics");
        assert_eq!(metrics.cyclomatic_complexity, 5);
        assert_eq!(metrics.cognitive_complexity, 5);
        assert_eq!(metrics.halstead.distinct_operators, 19);
        assert_eq!(metrics.halstead.total_operators, 20);
        assert_eq!(metrics.halstead.distinct_operands, 14);
        assert_eq!(metrics.halstead.total_operands, 14);
        assert_eq!(maintainability_index(0.0, 0, 0, 0), 100.0);
    }

    #[test]
    fn invalid_selectors_are_total_and_report_shared_errors() {
        let mut cpg = CodePropertyGraph::new(Language::Unknown);
        cpg.add_node_with_id(node(1, CpgNodeKind::Root, None));
        assert_eq!(
            function_metrics(&cpg, NodeId::new(99)),
            Err(AnalysisError::UnknownNode(NodeId::new(99)))
        );
        assert_eq!(
            function_metrics(&cpg, NodeId::new(1)),
            Err(AnalysisError::NotAFunction(NodeId::new(1)))
        );
        assert_eq!(
            method_field_graph(&cpg, NodeId::new(99)),
            MethodFieldGraph {
                class: NodeId::new(99),
                methods: Vec::new(),
                fields: Vec::new(),
                accesses: Vec::new(),
                method_calls: Vec::new(),
            }
        );
        assert!(ck_metrics(&cpg, &CkOptions::default()).is_empty());
    }

    #[test]
    fn inheritance_cycles_collapse_without_inventing_depth() {
        let mut cpg = CodePropertyGraph::new(Language::Java);
        for (id, name) in [(1, "A"), (2, "B")] {
            cpg.add_node_with_id(node(
                id,
                CpgNodeKind::Class {
                    name: name.into(),
                    is_abstract: false,
                },
                None,
            ));
        }
        cpg.connect(NodeId::new(1), NodeId::new(2), CpgEdgeKind::Inherits);
        cpg.connect(NodeId::new(2), NodeId::new(1), CpgEdgeKind::Inherits);
        let metrics = ck_metrics(&cpg, &CkOptions::default());
        assert_eq!(
            metrics.iter().map(|row| row.dit).collect::<Vec<_>>(),
            vec![0, 0]
        );
        assert_eq!(
            metrics.iter().map(|row| row.noc).collect::<Vec<_>>(),
            vec![1, 1]
        );
    }

    #[test]
    fn hierarchy_metrics_cover_branching_depth_children_and_external_edges() {
        let nodes = (0..=4).map(NodeId::new).collect::<Vec<_>>();
        let metrics = hierarchy_metrics(
            &nodes,
            &[
                (NodeId::new(1), NodeId::new(0)),
                (NodeId::new(2), NodeId::new(0)),
                (NodeId::new(3), NodeId::new(1)),
                (NodeId::new(3), NodeId::new(2)),
                (NodeId::new(4), NodeId::new(3)),
                (NodeId::new(4), NodeId::new(99)),
            ],
        );

        assert_eq!(
            metrics,
            vec![
                HierarchyMetrics {
                    node: NodeId::new(0),
                    depth: 0,
                    direct_children: 2,
                },
                HierarchyMetrics {
                    node: NodeId::new(1),
                    depth: 1,
                    direct_children: 1,
                },
                HierarchyMetrics {
                    node: NodeId::new(2),
                    depth: 1,
                    direct_children: 1,
                },
                HierarchyMetrics {
                    node: NodeId::new(3),
                    depth: 2,
                    direct_children: 1,
                },
                HierarchyMetrics {
                    node: NodeId::new(4),
                    depth: 3,
                    direct_children: 0,
                },
            ]
        );
    }

    #[test]
    fn hierarchy_metrics_are_stack_safe_on_a_deep_chain() {
        const NODE_COUNT: u32 = 50_000;
        std::thread::Builder::new()
            .name("hierarchy-small-stack".into())
            .stack_size(64 * 1024)
            .spawn(|| {
                let nodes = (0..NODE_COUNT).map(NodeId::new).collect::<Vec<_>>();
                let relations = (1..NODE_COUNT)
                    .map(|child| (NodeId::new(child), NodeId::new(child - 1)))
                    .collect::<Vec<_>>();
                let metrics = hierarchy_metrics(&nodes, &relations);
                assert_eq!(metrics.len(), NODE_COUNT as usize);
                assert_eq!(metrics[0].depth, 0);
                assert_eq!(metrics.last().expect("last node").depth, NODE_COUNT - 1);
            })
            .expect("spawn constrained-stack hierarchy test")
            .join()
            .expect("hierarchy analysis must not overflow the native stack");
    }

    #[test]
    fn class_selection_options_cover_structs_enums_and_implements() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        cpg.add_node_with_id(node(
            1,
            CpgNodeKind::Class {
                name: "C".into(),
                is_abstract: false,
            },
            None,
        ));
        cpg.add_node_with_id(node(2, CpgNodeKind::Struct { name: "S".into() }, None));
        cpg.add_node_with_id(node(3, CpgNodeKind::Enum { name: "E".into() }, None));
        cpg.add_node_with_id(node(4, CpgNodeKind::Trait { name: "T".into() }, None));
        cpg.connect(NodeId::new(1), NodeId::new(4), CpgEdgeKind::Implements);

        let defaults = ck_metrics(&cpg, &CkOptions::default());
        assert_eq!(
            defaults.iter().map(|row| row.class).collect::<Vec<_>>(),
            vec![NodeId::new(1), NodeId::new(2)]
        );
        assert_eq!(defaults[0].dit, 0);

        let all = ck_metrics(
            &cpg,
            &CkOptions {
                include_structs: true,
                include_enums: true,
                include_implements: true,
            },
        );
        assert_eq!(
            all.iter().map(|row| row.class).collect::<Vec<_>>(),
            vec![NodeId::new(1), NodeId::new(2), NodeId::new(3)]
        );
        assert_eq!(all[0].dit, 1);
    }

    fn generated_class(
        method_count: usize,
        field_count: usize,
        mask: &[bool],
        reverse: bool,
    ) -> CodePropertyGraph {
        let mut declarations = vec![node(
            1,
            CpgNodeKind::Class {
                name: "Generated".into(),
                is_abstract: false,
            },
            None,
        )];
        for field_index in 0..field_count {
            declarations.push(node(
                10 + field_index as u32,
                field(&format!("f{field_index}")),
                Some(1),
            ));
        }
        for method_index in 0..method_count {
            let method = 100 + method_index as u32;
            declarations.push(node(method, function(&format!("m{method_index}")), Some(1)));
            for field_index in 0..field_count {
                if mask[(method_index * field_count + field_index) % mask.len()] {
                    let access = 1_000 + method_index as u32 * 10 + field_index as u32;
                    declarations.push(node(
                        access,
                        CpgNodeKind::MemberAccess {
                            member: format!("f{field_index}").into(),
                        },
                        Some(method),
                    ));
                }
            }
        }
        if reverse {
            declarations.reverse();
        }
        let mut cpg = CodePropertyGraph::new(Language::Java);
        for declaration in declarations {
            cpg.add_node_with_id(declaration);
        }
        let mut edges = Vec::new();
        for field_index in 0..field_count {
            edges.push((
                NodeId::new(1),
                NodeId::new(10 + field_index as u32),
                CpgEdgeKind::AstChild,
            ));
        }
        for method_index in 0..method_count {
            let method = NodeId::new(100 + method_index as u32);
            edges.push((NodeId::new(1), method, CpgEdgeKind::AstChild));
            for field_index in 0..field_count {
                if mask[(method_index * field_count + field_index) % mask.len()] {
                    let access = NodeId::new(1_000 + method_index as u32 * 10 + field_index as u32);
                    edges.push((method, access, CpgEdgeKind::AstChild));
                    edges.push((
                        access,
                        NodeId::new(10 + field_index as u32),
                        CpgEdgeKind::Reference,
                    ));
                    edges.push((
                        method,
                        access,
                        CpgEdgeKind::DataFlow(DfgEdgeKind::FieldRead),
                    ));
                }
            }
        }
        if reverse {
            edges.reverse();
        }
        for (source, target, kind) in edges {
            cpg.connect(source, target, kind)
                .expect("generated endpoints exist");
        }
        cpg
    }

    proptest! {
        #[test]
        fn hierarchy_metrics_are_permutation_invariant_and_bounded(
            node_count in 1u32..40,
            raw_relations in prop::collection::vec((0u32..80, 0u32..80), 0..160),
        ) {
            let nodes = (0..node_count).map(NodeId::new).collect::<Vec<_>>();
            let relations = raw_relations
                .into_iter()
                .map(|(child, parent)| {
                    (NodeId::new(child % node_count), NodeId::new(parent % node_count))
                })
                .collect::<Vec<_>>();
            let mut permuted = relations.clone();
            permuted.reverse();
            permuted.extend(relations.iter().copied());

            let first = hierarchy_metrics(&nodes, &relations);
            let second = hierarchy_metrics(&nodes, &permuted);
            prop_assert_eq!(&first, &second);
            prop_assert_eq!(first.len(), node_count as usize);
            for (position, metrics) in first.iter().enumerate() {
                prop_assert_eq!(metrics.node, NodeId::new(position as u32));
                prop_assert!(metrics.depth < node_count);
                prop_assert!(metrics.direct_children < node_count);
            }
        }

        #[test]
        fn ck_and_lcom_are_deterministic_under_insertion_order(
            method_count in 1usize..6,
            field_count in 1usize..5,
            mask in prop::collection::vec(any::<bool>(), 1..31),
        ) {
            let forward = generated_class(method_count, field_count, &mask, false);
            let reverse = generated_class(method_count, field_count, &mask, true);
            let first = ck_metrics(&forward, &CkOptions::default());
            let second = ck_metrics(&reverse, &CkOptions::default());
            prop_assert_eq!(&first, &second);
            let lcom = first[0].lcom;
            prop_assert!(lcom.lcom2 <= lcom.lcom1);
            prop_assert!(lcom.lcom4 <= lcom.lcom3);
            prop_assert!((0.0..=1.0).contains(&lcom.lcom5));
            prop_assert!(lcom.lcom5.is_finite());
        }

        #[test]
        fn pgmcp_mi_formula_is_total_and_bounded(
            volume in 0.0f64..1.0e12,
            complexity in any::<u32>(),
            lines in any::<u32>(),
            comments in any::<u32>(),
        ) {
            let value = maintainability_index(volume, complexity, lines, comments);
            prop_assert!(value.is_finite());
            prop_assert!((0.0..=100.0).contains(&value));
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn public_metrics_round_trip_through_serde() {
        let cpg = class_fixture();
        let options = CkOptions::default();
        let graph = method_field_graph(&cpg, NodeId::new(2));
        let metrics = ck_metrics(&cpg, &options);
        let function = function_metrics(&cpg, NodeId::new(20)).expect("function metrics");

        let options_back: CkOptions =
            serde_json::from_str(&serde_json::to_string(&options).expect("serialize CK options"))
                .expect("deserialize CK options");
        let graph_back: MethodFieldGraph = serde_json::from_str(
            &serde_json::to_string(&graph).expect("serialize method-field graph"),
        )
        .expect("deserialize method-field graph");
        let metrics_back: Vec<ClassMetrics> = serde_json::from_str(
            &serde_json::to_string(&metrics).expect("serialize class metrics"),
        )
        .expect("deserialize class metrics");
        let function_back: FunctionMetrics = serde_json::from_str(
            &serde_json::to_string(&function).expect("serialize function metrics"),
        )
        .expect("deserialize function metrics");
        assert_eq!(options_back, options);
        assert_eq!(graph_back, graph);
        assert_eq!(metrics_back, metrics);
        assert_eq!(function_back, function);
    }
}
