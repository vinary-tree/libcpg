//! Deterministic, bounded Type-1 through Type-4 function clone detection.
//!
//! Type-1 and Type-2 use bottom-up ordered AST Merkle fingerprints. Type-3
//! compares kind-and-name multisets. Type-4 combines a program-dependence WL
//! prefilter with the existing relaxed VF2 matcher. Every search bound is
//! explicit: exceeding one returns [`AnalysisError::BudgetExceeded`] rather
//! than silently omitting clone candidates.

use std::collections::{BTreeMap, BTreeSet};

use rustc_hash::{FxHashMap, FxHashSet};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use super::{NodeKindTag, SubgraphMatcher, Vf2Matcher};
use crate::analysis::wl::stable_wl_mix;
use crate::analysis::{
    function_scope_nodes, pdg_projection, wl_labels, AnalysisError, GraphProjection,
    MAX_WL_ITERATIONS,
};
use crate::{CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, LiteralKind, NodeId};

/// Hard ceiling on Type-3 candidate comparisons in one request.
pub const MAX_CLONE_PAIRS: usize = 100_000;
/// Hard ceiling on vertices in either PDG passed to Type-4 verification.
pub const MAX_CLONE_PDG_NODES: usize = 300;
/// Hard ceiling on relaxed VF2 invocations in one request.
pub const MAX_CLONE_VF2_INVOCATIONS: usize = 100_000;

const TYPE1_SEED: u64 = 0x434c_4f4e_455f_5431;
const TYPE2_SEED: u64 = 0x434c_4f4e_455f_5432;
const CHILD_BOUNDARY: u64 = 0xa57c_91e5_1d3b_704f;
const CYCLE_SENTINEL: u64 = 0xded0_c1e0_0000_0001;

/// Clone taxonomy used by [`detect_clones`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CloneType {
    /// Identical retained syntax, ignoring comments and source locations.
    Type1,
    /// Identical ordered syntax after identifier and literal abstraction.
    Type2,
    /// Similar kind-and-name multisets above the configured Jaccard threshold.
    Type3,
    /// Equivalent bounded program-dependence structure after WL and VF2 checks.
    Type4,
}

/// One connected clone class in the pairwise clone relation.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CloneClass {
    /// Strongest clone relation joining the members.
    pub clone_type: CloneType,
    /// Function-root identifiers, sorted in ascending stable-id order.
    pub members: Vec<NodeId>,
    /// Minimum similarity among the relation edges joining this class.
    pub similarity: f64,
}

/// Thresholds and resource limits for [`detect_clones`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CloneOptions {
    /// Minimum multiset Jaccard similarity for Type-3 clones.
    pub type3_threshold: f64,
    /// Minimum WL-label multiset cosine similarity before Type-4 VF2.
    pub type4_wl_threshold: f64,
    /// Number of WL refinement rounds used by the Type-4 prefilter.
    pub wl_iterations: usize,
    /// Maximum Type-3 size-qualified pair comparisons.
    pub max_pairs: usize,
    /// Maximum number of vertices in each Type-4 PDG.
    pub max_pdg_nodes: usize,
    /// Maximum number of Type-4 VF2 verifier invocations.
    pub max_vf2_invocations: usize,
}

impl Default for CloneOptions {
    fn default() -> Self {
        Self {
            type3_threshold: 0.8,
            type4_wl_threshold: 0.8,
            wl_iterations: 3,
            max_pairs: MAX_CLONE_PAIRS,
            max_pdg_nodes: MAX_CLONE_PDG_NODES,
            max_vf2_invocations: 10_000,
        }
    }
}

#[derive(Debug)]
struct FunctionFeatures {
    root: NodeId,
    type1: u64,
    type2: u64,
    bag: BTreeMap<(u8, String), usize>,
    bag_size: usize,
}

#[derive(Debug, Clone, Copy)]
struct PairRelation {
    left: usize,
    right: usize,
    clone_type: CloneType,
    similarity: f64,
}

/// Detects deterministic Type-1 through Type-4 clone classes among functions.
///
/// Nested functions are analyzed as their own roots and excluded from their
/// enclosing function's subtree. Comments and locations do not contribute to
/// Type-1, matching the standard definition of a Type-1 clone. In the absence
/// of retained lexical tokens, Type-1 is exact with respect to semantic CPG
/// payloads (names, operators, literals, and terminal text).
///
/// Each function pair receives only its strongest applicable classification:
/// Type-1, then Type-2, then Type-4 for equal bags with reordered structure,
/// then Type-3. Pair relations of the same type are grouped by connected
/// components. Results are ordered by clone type and lexicographic members.
///
/// # Errors
///
/// Returns [`AnalysisError::InvalidInput`] for invalid options. Returns
/// [`AnalysisError::BudgetExceeded`] before omitting any size-qualified
/// candidate pair, oversized Type-4 PDG, or required VF2 invocation.
pub fn detect_clones(
    cpg: &CodePropertyGraph,
    options: &CloneOptions,
) -> Result<Vec<CloneClass>, AnalysisError> {
    validate_options(options)?;

    let mut functions: Vec<_> = cpg.functions().map(|node| node.id).collect();
    functions.sort_unstable();
    let features: Vec<_> = functions
        .iter()
        .copied()
        .map(|function| function_features(cpg, function))
        .collect();

    let candidates = candidate_pairs(&features, options)?;
    let mut relations = Vec::new();
    let mut vf2_invocations = 0usize;
    let mut pdg_cache = FxHashMap::<NodeId, GraphProjection>::default();

    for (left, right) in candidates {
        let a = &features[left];
        let b = &features[right];
        if a.type1 == b.type1 {
            relations.push(PairRelation {
                left,
                right,
                clone_type: CloneType::Type1,
                similarity: 1.0,
            });
            continue;
        }
        if a.type2 == b.type2 {
            relations.push(PairRelation {
                left,
                right,
                clone_type: CloneType::Type2,
                similarity: 1.0,
            });
            continue;
        }

        let jaccard = multiset_jaccard(&a.bag, &b.bag);
        if jaccard == 1.0
            && type4_equivalent(
                cpg,
                a.root,
                b.root,
                options,
                &mut pdg_cache,
                &mut vf2_invocations,
            )?
        {
            relations.push(PairRelation {
                left,
                right,
                clone_type: CloneType::Type4,
                similarity: 1.0,
            });
        } else if jaccard >= options.type3_threshold {
            relations.push(PairRelation {
                left,
                right,
                clone_type: CloneType::Type3,
                similarity: jaccard,
            });
        }
    }

    Ok(group_relations(&features, &relations))
}

fn validate_options(options: &CloneOptions) -> Result<(), AnalysisError> {
    validate_probability(options.type3_threshold, "type3_threshold")?;
    validate_probability(options.type4_wl_threshold, "type4_wl_threshold")?;
    if options.wl_iterations > MAX_WL_ITERATIONS {
        return Err(invalid_option(
            "wl_iterations",
            "must not exceed MAX_WL_ITERATIONS",
        ));
    }
    if options.max_pairs > MAX_CLONE_PAIRS {
        return Err(invalid_option(
            "max_pairs",
            "must not exceed MAX_CLONE_PAIRS",
        ));
    }
    if options.max_pdg_nodes > MAX_CLONE_PDG_NODES {
        return Err(invalid_option(
            "max_pdg_nodes",
            "must not exceed MAX_CLONE_PDG_NODES",
        ));
    }
    if options.max_vf2_invocations > MAX_CLONE_VF2_INVOCATIONS {
        return Err(invalid_option(
            "max_vf2_invocations",
            "must not exceed MAX_CLONE_VF2_INVOCATIONS",
        ));
    }
    Ok(())
}

fn validate_probability(value: f64, input: &'static str) -> Result<(), AnalysisError> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) {
        return Err(invalid_option(
            input,
            "must be finite and in the range 0..=1",
        ));
    }
    Ok(())
}

fn invalid_option(input: &'static str, reason: &'static str) -> AnalysisError {
    AnalysisError::InvalidInput {
        analysis: "clone detection",
        input,
        reason,
    }
}

fn enforce_budget(
    analysis: &'static str,
    unit: &'static str,
    needed: usize,
    cap: usize,
) -> Result<(), AnalysisError> {
    if needed > cap {
        return Err(AnalysisError::BudgetExceeded {
            analysis,
            unit,
            needed: needed as u64,
            cap: cap as u64,
        });
    }
    Ok(())
}

fn function_features(cpg: &CodePropertyGraph, root: NodeId) -> FunctionFeatures {
    let scope = function_scope_nodes(cpg, root);
    let (type1, type2) = merkle_fingerprints(cpg, root, &scope);
    let mut bag = BTreeMap::new();
    for node in scope.iter().filter_map(|id| cpg.node(*id)) {
        if matches!(node.kind, CpgNodeKind::Comment { .. }) {
            continue;
        }
        let key = (
            node_tag_code(&node.kind),
            node.name().unwrap_or_default().to_owned(),
        );
        *bag.entry(key).or_default() += 1;
    }
    let bag_size = bag.values().sum();
    FunctionFeatures {
        root,
        type1,
        type2,
        bag,
        bag_size,
    }
}

fn candidate_pairs(
    features: &[FunctionFeatures],
    options: &CloneOptions,
) -> Result<Vec<(usize, usize)>, AnalysisError> {
    let mut buckets = BTreeMap::<usize, Vec<usize>>::new();
    for (index, feature) in features.iter().enumerate() {
        buckets.entry(feature.bag_size).or_default().push(index);
    }

    let mut pairs = Vec::new();
    for (&left_size, left_indices) in &buckets {
        for (&right_size, right_indices) in buckets.range(left_size..) {
            if !size_ratio_qualifies(left_size, right_size, options.type3_threshold) {
                if options.type3_threshold > 0.0 {
                    break;
                }
                continue;
            }
            if left_size == right_size {
                for left in 0..left_indices.len() {
                    for right in left + 1..left_indices.len() {
                        push_candidate(
                            &mut pairs,
                            left_indices[left],
                            left_indices[right],
                            options.max_pairs,
                        )?;
                    }
                }
            } else {
                for &left in left_indices {
                    for &right in right_indices {
                        push_candidate(&mut pairs, left, right, options.max_pairs)?;
                    }
                }
            }
        }
    }
    pairs.sort_unstable();
    Ok(pairs)
}

fn push_candidate(
    pairs: &mut Vec<(usize, usize)>,
    left: usize,
    right: usize,
    cap: usize,
) -> Result<(), AnalysisError> {
    enforce_budget(
        "clone detection",
        "candidate pairs",
        pairs.len().saturating_add(1),
        cap,
    )?;
    pairs.push((left.min(right), left.max(right)));
    Ok(())
}

fn merkle_fingerprints(
    cpg: &CodePropertyGraph,
    root: NodeId,
    scope: &FxHashSet<NodeId>,
) -> (u64, u64) {
    let mut hashes = FxHashMap::<NodeId, (u64, u64)>::default();
    let mut active = FxHashSet::default();
    let mut stack = vec![(root, false)];

    while let Some((node_id, expanded)) = stack.pop() {
        if hashes.contains_key(&node_id) {
            continue;
        }
        if expanded {
            active.remove(&node_id);
            let Some(node) = cpg.node(node_id) else {
                continue;
            };
            let mut exact = exact_node_seed(node);
            let mut abstracted = stable_wl_mix(TYPE2_SEED, node_tag_code(&node.kind) as u64);
            for child in clone_children(cpg, node_id, scope) {
                let (child_exact, child_abstracted) = hashes
                    .get(&child)
                    .copied()
                    .unwrap_or((CYCLE_SENTINEL, CYCLE_SENTINEL));
                exact = stable_wl_mix(stable_wl_mix(exact, CHILD_BOUNDARY), child_exact);
                abstracted =
                    stable_wl_mix(stable_wl_mix(abstracted, CHILD_BOUNDARY), child_abstracted);
            }
            hashes.insert(node_id, (exact, abstracted));
            continue;
        }

        if !active.insert(node_id) {
            continue;
        }
        stack.push((node_id, true));
        let mut children = clone_children(cpg, node_id, scope);
        children.reverse();
        for child in children {
            if !hashes.contains_key(&child) && !active.contains(&child) {
                stack.push((child, false));
            }
        }
    }

    hashes
        .get(&root)
        .copied()
        .unwrap_or((CYCLE_SENTINEL, CYCLE_SENTINEL))
}

fn clone_children(cpg: &CodePropertyGraph, node: NodeId, scope: &FxHashSet<NodeId>) -> Vec<NodeId> {
    cpg.ast_children(node)
        .into_iter()
        .filter(|child| scope.contains(child))
        .filter(|child| {
            cpg.node(*child)
                .is_some_and(|node| !matches!(node.kind, CpgNodeKind::Comment { .. }))
        })
        .collect()
}

fn exact_node_seed(node: &CpgNode) -> u64 {
    let mut hash = stable_wl_mix(TYPE1_SEED, node_tag_code(&node.kind) as u64);
    if let Some(name) = node.name() {
        hash = mix_bytes(hash, name.as_bytes());
    }
    hash = match &node.kind {
        CpgNodeKind::BinaryOp { operator }
        | CpgNodeKind::UnaryOp { operator }
        | CpgNodeKind::Assignment { operator } => mix_bytes(hash, operator.as_bytes()),
        CpgNodeKind::Literal { kind } => mix_literal(hash, kind),
        CpgNodeKind::Call { is_method, .. } => stable_wl_mix(hash, u64::from(*is_method)),
        CpgNodeKind::Class { is_abstract, .. } => stable_wl_mix(hash, u64::from(*is_abstract)),
        CpgNodeKind::Parameter { is_variadic, .. } => stable_wl_mix(hash, u64::from(*is_variadic)),
        CpgNodeKind::Variable { is_mutable, .. } => stable_wl_mix(hash, u64::from(*is_mutable)),
        CpgNodeKind::Comment { .. } => hash,
        _ => hash,
    };
    node.text
        .as_deref()
        .map_or(hash, |text| mix_bytes(hash, text.as_bytes()))
}

fn mix_literal(hash: u64, literal: &LiteralKind) -> u64 {
    match literal {
        LiteralKind::Integer(value) => stable_wl_mix(stable_wl_mix(hash, 0), *value as u64),
        LiteralKind::Float(value) => stable_wl_mix(stable_wl_mix(hash, 1), value.to_bits()),
        LiteralKind::String(value) => mix_bytes(stable_wl_mix(hash, 2), value.as_bytes()),
        LiteralKind::Char(value) => stable_wl_mix(stable_wl_mix(hash, 3), *value as u64),
        LiteralKind::Bool(value) => stable_wl_mix(stable_wl_mix(hash, 4), u64::from(*value)),
        LiteralKind::Null => stable_wl_mix(hash, 5),
        LiteralKind::Array => stable_wl_mix(hash, 6),
        LiteralKind::Object => stable_wl_mix(hash, 7),
        LiteralKind::Regex(value) => mix_bytes(stable_wl_mix(hash, 8), value.as_bytes()),
    }
}

fn mix_bytes(mut hash: u64, bytes: &[u8]) -> u64 {
    hash = stable_wl_mix(hash, bytes.len() as u64);
    for chunk in bytes.chunks(8) {
        let mut value = 0u64;
        for (shift, byte) in chunk.iter().enumerate() {
            value |= u64::from(*byte) << (shift * 8);
        }
        hash = stable_wl_mix(hash, value);
    }
    hash
}

fn node_tag_code(kind: &CpgNodeKind) -> u8 {
    match NodeKindTag::from_kind(kind) {
        NodeKindTag::Root => 0,
        NodeKindTag::Module => 1,
        NodeKindTag::Class => 2,
        NodeKindTag::Struct => 3,
        NodeKindTag::Enum => 4,
        NodeKindTag::Trait => 5,
        NodeKindTag::Impl => 6,
        NodeKindTag::Function => 7,
        NodeKindTag::Parameter => 8,
        NodeKindTag::Block => 9,
        NodeKindTag::Variable => 10,
        NodeKindTag::Field => 11,
        NodeKindTag::Return => 12,
        NodeKindTag::If => 13,
        NodeKindTag::While => 14,
        NodeKindTag::For => 15,
        NodeKindTag::Loop => 16,
        NodeKindTag::Match => 17,
        NodeKindTag::BinaryOp => 18,
        NodeKindTag::UnaryOp => 19,
        NodeKindTag::Assignment => 20,
        NodeKindTag::Call => 21,
        NodeKindTag::MemberAccess => 22,
        NodeKindTag::IndexAccess => 23,
        NodeKindTag::Identifier => 24,
        NodeKindTag::Literal => 25,
        NodeKindTag::Lambda => 26,
        NodeKindTag::Import => 27,
        NodeKindTag::Unknown => 28,
    }
}

fn size_ratio_qualifies(left: usize, right: usize, threshold: f64) -> bool {
    if left == 0 || right == 0 {
        return left == right;
    }
    (left.min(right) as f64 / left.max(right) as f64) >= threshold
}

fn multiset_jaccard(
    left: &BTreeMap<(u8, String), usize>,
    right: &BTreeMap<(u8, String), usize>,
) -> f64 {
    let keys: BTreeSet<_> = left.keys().chain(right.keys()).collect();
    let mut intersection = 0usize;
    let mut union = 0usize;
    for key in keys {
        let a = left.get(key).copied().unwrap_or(0);
        let b = right.get(key).copied().unwrap_or(0);
        intersection += a.min(b);
        union += a.max(b);
    }
    if union == 0 {
        1.0
    } else {
        intersection as f64 / union as f64
    }
}

fn type4_equivalent(
    cpg: &CodePropertyGraph,
    left: NodeId,
    right: NodeId,
    options: &CloneOptions,
    cache: &mut FxHashMap<NodeId, GraphProjection>,
    vf2_invocations: &mut usize,
) -> Result<bool, AnalysisError> {
    for function in [left, right] {
        if let std::collections::hash_map::Entry::Vacant(entry) = cache.entry(function) {
            entry.insert(pdg_projection(cpg, function)?);
        }
    }
    let left_pdg = &cache[&left];
    let right_pdg = &cache[&right];
    let largest = left_pdg.node_count().max(right_pdg.node_count());
    enforce_budget(
        "clone detection",
        "PDG nodes",
        largest,
        options.max_pdg_nodes,
    )?;
    if left_pdg.node_count() != right_pdg.node_count()
        || left_pdg.edge_count() != right_pdg.edge_count()
    {
        return Ok(false);
    }

    let left_labels = wl_labels(left_pdg, options.wl_iterations);
    let right_labels = wl_labels(right_pdg, options.wl_iterations);
    let cosine = multiset_cosine(&left_labels, &right_labels);
    if cosine < options.type4_wl_threshold {
        return Ok(false);
    }

    *vf2_invocations = vf2_invocations.saturating_add(1);
    enforce_budget(
        "clone detection",
        "VF2 invocations",
        *vf2_invocations,
        options.max_vf2_invocations,
    )?;

    let left_graph = pdg_cpg(cpg, left_pdg);
    let right_graph = pdg_cpg(cpg, right_pdg);
    let matcher = Vf2Matcher::new()
        .with_strict_kinds(false)
        .with_strict_edges(false)
        .with_max_matches(1);
    Ok(matcher.contains_pattern(&left_graph, &right_graph))
}

fn multiset_cosine(left: &[u64], right: &[u64]) -> f64 {
    let mut left_counts = BTreeMap::<u64, usize>::new();
    let mut right_counts = BTreeMap::<u64, usize>::new();
    for &label in left {
        *left_counts.entry(label).or_default() += 1;
    }
    for &label in right {
        *right_counts.entry(label).or_default() += 1;
    }
    let dot: f64 = left_counts
        .iter()
        .map(|(label, &count)| count as f64 * right_counts.get(label).copied().unwrap_or(0) as f64)
        .sum();
    let left_norm: f64 = left_counts
        .values()
        .map(|&count| (count * count) as f64)
        .sum();
    let right_norm: f64 = right_counts
        .values()
        .map(|&count| (count * count) as f64)
        .sum();
    if left_norm == 0.0 || right_norm == 0.0 {
        f64::from(left.is_empty() && right.is_empty())
    } else {
        dot / (left_norm.sqrt() * right_norm.sqrt())
    }
}

fn pdg_cpg(source: &CodePropertyGraph, projection: &GraphProjection) -> CodePropertyGraph {
    let mut graph = CodePropertyGraph::new(source.language());
    for &node in &projection.nodes {
        if let Some(node) = source.node(node) {
            graph.add_node_with_id(node.clone());
        }
    }
    for (from, to, _) in projection.edges() {
        graph.connect(
            projection.node_id(from),
            projection.node_id(to),
            CpgEdgeKind::ControlDependence,
        );
    }
    graph
}

fn group_relations(features: &[FunctionFeatures], relations: &[PairRelation]) -> Vec<CloneClass> {
    let mut result = Vec::new();
    for clone_type in [
        CloneType::Type1,
        CloneType::Type2,
        CloneType::Type3,
        CloneType::Type4,
    ] {
        let selected: Vec<_> = relations
            .iter()
            .filter(|relation| relation.clone_type == clone_type)
            .collect();
        let mut parent: Vec<usize> = (0..features.len()).collect();
        for relation in &selected {
            union(&mut parent, relation.left, relation.right);
        }

        let mut members = BTreeMap::<usize, Vec<NodeId>>::new();
        for relation in &selected {
            for index in [relation.left, relation.right] {
                let root = find(&mut parent, index);
                members.entry(root).or_default().push(features[index].root);
            }
        }
        for values in members.values_mut() {
            values.sort_unstable();
            values.dedup();
        }
        for (component, values) in members {
            let similarity = selected
                .iter()
                .filter(|relation| {
                    find(&mut parent, relation.left) == component
                        && find(&mut parent, relation.right) == component
                })
                .map(|relation| relation.similarity)
                .min_by(f64::total_cmp)
                .unwrap_or(1.0);
            result.push(CloneClass {
                clone_type,
                members: values,
                similarity,
            });
        }
    }
    result.sort_by(|left, right| {
        left.clone_type
            .cmp(&right.clone_type)
            .then_with(|| left.members.cmp(&right.members))
            .then_with(|| left.similarity.total_cmp(&right.similarity))
    });
    result
}

fn find(parent: &mut [usize], mut node: usize) -> usize {
    let mut root = node;
    while parent[root] != root {
        root = parent[root];
    }
    while parent[node] != node {
        let next = parent[node];
        parent[node] = root;
        node = next;
    }
    root
}

fn union(parent: &mut [usize], left: usize, right: usize) {
    let left = find(parent, left);
    let right = find(parent, right);
    if left != right {
        let (root, child) = (left.min(right), left.max(right));
        parent[child] = root;
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use proptest::prelude::*;
    use smallvec::smallvec;

    use super::*;
    use crate::{CpgNode, Language, MethodSignature, SourceRange, TypeInfo, Visibility};

    fn function_kind(name: &str) -> CpgNodeKind {
        CpgNodeKind::Function {
            signature: MethodSignature {
                name: Arc::from(name),
                params: smallvec![],
                return_type: Some(TypeInfo::new("i64")),
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            },
        }
    }

    fn identifier(name: &str) -> CpgNodeKind {
        CpgNodeKind::Identifier {
            name: Arc::from(name),
            definition: None,
        }
    }

    fn integer(value: i64) -> CpgNodeKind {
        CpgNodeKind::Literal {
            kind: LiteralKind::Integer(value),
        }
    }

    fn add_function(
        cpg: &mut CodePropertyGraph,
        name: &str,
        body: Vec<CpgNodeKind>,
    ) -> (NodeId, Vec<NodeId>) {
        let root = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            function_kind(name),
            SourceRange::default(),
        ));
        let mut children = Vec::new();
        for kind in body {
            let child = cpg.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()));
            cpg.connect(root, child, CpgEdgeKind::AstChild);
            children.push(child);
        }
        (root, children)
    }

    fn only_class(cpg: &CodePropertyGraph) -> CloneClass {
        let classes = detect_clones(cpg, &CloneOptions::default()).unwrap();
        assert_eq!(classes.len(), 1, "classes: {classes:?}");
        classes.into_iter().next().unwrap()
    }

    #[test]
    fn verbatim_subtrees_are_type1_and_comments_do_not_change_them() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (left, _) = add_function(&mut cpg, "same", vec![identifier("value"), integer(7)]);
        let (right, children) = add_function(
            &mut cpg,
            "same",
            vec![
                CpgNodeKind::Comment { is_doc: true },
                identifier("value"),
                integer(7),
            ],
        );
        assert!(!children.is_empty());

        assert_eq!(
            only_class(&cpg),
            CloneClass {
                clone_type: CloneType::Type1,
                members: vec![left, right],
                similarity: 1.0,
            }
        );
    }

    #[test]
    fn renamed_identifiers_and_literals_are_type2() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (left, _) = add_function(&mut cpg, "same", vec![identifier("left"), integer(1)]);
        let (right, _) = add_function(&mut cpg, "same", vec![identifier("right"), integer(99)]);

        assert_eq!(
            only_class(&cpg),
            CloneClass {
                clone_type: CloneType::Type2,
                members: vec![left, right],
                similarity: 1.0,
            }
        );
    }

    #[test]
    fn one_inserted_statement_is_type3_at_default_threshold() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let common = || {
            vec![
                identifier("a"),
                identifier("b"),
                identifier("c"),
                identifier("d"),
            ]
        };
        let (left, _) = add_function(&mut cpg, "same", common());
        let mut inserted = common();
        inserted.push(CpgNodeKind::Return);
        let (right, _) = add_function(&mut cpg, "same", inserted);

        let class = only_class(&cpg);
        assert_eq!(class.clone_type, CloneType::Type3);
        assert_eq!(class.members, vec![left, right]);
        assert!((class.similarity - 5.0 / 6.0).abs() < 1.0e-12);
    }

    #[test]
    fn reordered_independent_statements_are_type4() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (left, left_body) = add_function(
            &mut cpg,
            "same",
            vec![identifier("value"), CpgNodeKind::Return],
        );
        let (right, right_body) = add_function(
            &mut cpg,
            "same",
            vec![CpgNodeKind::Return, identifier("value")],
        );
        for child in left_body {
            cpg.connect(left, child, CpgEdgeKind::ControlDependence);
        }
        for child in right_body {
            cpg.connect(right, child, CpgEdgeKind::ControlDependence);
        }

        assert_eq!(
            only_class(&cpg),
            CloneClass {
                clone_type: CloneType::Type4,
                members: vec![left, right],
                similarity: 1.0,
            }
        );
    }

    #[test]
    fn pair_pdg_and_vf2_caps_return_typed_errors() {
        let mut type3 = CodePropertyGraph::new(Language::Rust);
        add_function(&mut type3, "same", vec![identifier("a")]);
        add_function(&mut type3, "same", vec![CpgNodeKind::Return]);
        let error = detect_clones(
            &type3,
            &CloneOptions {
                type3_threshold: 0.0,
                max_pairs: 0,
                ..CloneOptions::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            error,
            AnalysisError::BudgetExceeded {
                unit: "candidate pairs",
                needed: 1,
                cap: 0,
                ..
            }
        ));

        let mut type4 = CodePropertyGraph::new(Language::Rust);
        let (left, left_body) = add_function(
            &mut type4,
            "same",
            vec![identifier("value"), CpgNodeKind::Return],
        );
        let (right, right_body) = add_function(
            &mut type4,
            "same",
            vec![CpgNodeKind::Return, identifier("value")],
        );
        for child in left_body {
            type4.connect(left, child, CpgEdgeKind::ControlDependence);
        }
        for child in right_body {
            type4.connect(right, child, CpgEdgeKind::ControlDependence);
        }

        let pdg_error = detect_clones(
            &type4,
            &CloneOptions {
                max_pdg_nodes: 2,
                ..CloneOptions::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            pdg_error,
            AnalysisError::BudgetExceeded {
                unit: "PDG nodes",
                needed: 3,
                cap: 2,
                ..
            }
        ));

        let vf2_error = detect_clones(
            &type4,
            &CloneOptions {
                max_vf2_invocations: 0,
                ..CloneOptions::default()
            },
        )
        .unwrap_err();
        assert!(matches!(
            vf2_error,
            AnalysisError::BudgetExceeded {
                unit: "VF2 invocations",
                needed: 1,
                cap: 0,
                ..
            }
        ));
    }

    #[test]
    fn invalid_options_empty_graphs_and_cyclic_ast_are_total() {
        let empty = CodePropertyGraph::new(Language::Unknown);
        assert!(detect_clones(&empty, &CloneOptions::default())
            .unwrap()
            .is_empty());
        for options in [
            CloneOptions {
                type3_threshold: f64::NAN,
                ..CloneOptions::default()
            },
            CloneOptions {
                type4_wl_threshold: 1.1,
                ..CloneOptions::default()
            },
            CloneOptions {
                wl_iterations: MAX_WL_ITERATIONS + 1,
                ..CloneOptions::default()
            },
            CloneOptions {
                max_pairs: MAX_CLONE_PAIRS + 1,
                ..CloneOptions::default()
            },
            CloneOptions {
                max_pdg_nodes: MAX_CLONE_PDG_NODES + 1,
                ..CloneOptions::default()
            },
            CloneOptions {
                max_vf2_invocations: MAX_CLONE_VF2_INVOCATIONS + 1,
                ..CloneOptions::default()
            },
        ] {
            assert!(matches!(
                detect_clones(&empty, &options),
                Err(AnalysisError::InvalidInput { .. })
            ));
        }

        let mut cyclic = CodePropertyGraph::new(Language::Rust);
        let (left, left_body) = add_function(&mut cyclic, "same", vec![identifier("x")]);
        let (right, right_body) = add_function(&mut cyclic, "same", vec![identifier("x")]);
        cyclic.connect(left_body[0], left, CpgEdgeKind::AstChild);
        cyclic.connect(right_body[0], right, CpgEdgeKind::AstChild);
        let classes = detect_clones(&cyclic, &CloneOptions::default()).unwrap();
        assert_eq!(classes[0].members, vec![left, right]);
    }

    #[test]
    fn transitive_pair_relations_form_sorted_clone_classes() {
        let features = [10, 20, 30]
            .into_iter()
            .map(|id| FunctionFeatures {
                root: NodeId::new(id),
                type1: 0,
                type2: 0,
                bag: BTreeMap::new(),
                bag_size: 0,
            })
            .collect::<Vec<_>>();
        let relations = vec![
            PairRelation {
                left: 1,
                right: 2,
                clone_type: CloneType::Type3,
                similarity: 0.8,
            },
            PairRelation {
                left: 0,
                right: 1,
                clone_type: CloneType::Type3,
                similarity: 0.9,
            },
        ];
        assert_eq!(
            group_relations(&features, &relations),
            vec![CloneClass {
                clone_type: CloneType::Type3,
                members: vec![NodeId::new(10), NodeId::new(20), NodeId::new(30)],
                similarity: 0.8,
            }]
        );
    }

    #[test]
    fn empty_prefilters_and_dissimilar_equal_size_pairs_are_total() {
        assert!(size_ratio_qualifies(0, 0, 0.8));
        assert!(!size_ratio_qualifies(0, 1, 0.0));
        assert_eq!(multiset_jaccard(&BTreeMap::new(), &BTreeMap::new()), 1.0);
        assert_eq!(multiset_cosine(&[], &[]), 1.0);
        assert_eq!(multiset_cosine(&[], &[1]), 0.0);
        assert_eq!(multiset_cosine(&[1], &[]), 0.0);

        let zero_sized = vec![
            FunctionFeatures {
                root: NodeId::new(0),
                type1: 0,
                type2: 0,
                bag: BTreeMap::new(),
                bag_size: 0,
            },
            FunctionFeatures {
                root: NodeId::new(1),
                type1: 1,
                type2: 1,
                bag: BTreeMap::new(),
                bag_size: 1,
            },
        ];
        assert!(candidate_pairs(
            &zero_sized,
            &CloneOptions {
                type3_threshold: 0.0,
                ..CloneOptions::default()
            }
        )
        .unwrap()
        .is_empty());

        let mut cpg = CodePropertyGraph::new(Language::Rust);
        add_function(&mut cpg, "left", vec![identifier("alpha")]);
        add_function(&mut cpg, "right", vec![CpgNodeKind::Return]);
        assert!(detect_clones(&cpg, &CloneOptions::default())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn type4_prefilter_rejects_vertex_edge_and_wl_mismatches() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (left, left_body) = add_function(
            &mut cpg,
            "same",
            vec![identifier("a"), identifier("b"), identifier("c")],
        );
        let (right, right_body) = add_function(
            &mut cpg,
            "same",
            vec![identifier("c"), identifier("b"), identifier("a")],
        );
        let projection = |function: NodeId, nodes: &[NodeId], edges: &[(NodeId, NodeId)]| {
            GraphProjection::from_edges(
                crate::ProjectionKind::ProgramDependence { function },
                std::iter::once(function).chain(nodes.iter().copied()),
                edges.iter().map(|&(source, target)| (source, target, 1.0)),
            )
        };
        let left_star = projection(
            left,
            &left_body,
            &left_body
                .iter()
                .map(|&child| (left, child))
                .collect::<Vec<_>>(),
        );
        let right_path = projection(
            right,
            &right_body,
            &[
                (right, right_body[0]),
                (right_body[0], right_body[1]),
                (right_body[1], right_body[2]),
            ],
        );
        let options = CloneOptions {
            type4_wl_threshold: 0.8,
            ..CloneOptions::default()
        };
        let mut invocations = 0;
        let mut cache = FxHashMap::default();
        cache.insert(left, left_star.clone());
        cache.insert(right, right_path);
        assert!(
            !type4_equivalent(&cpg, left, right, &options, &mut cache, &mut invocations).unwrap()
        );
        assert_eq!(invocations, 0);

        cache.insert(
            right,
            projection(right, &right_body[..2], &[(right, right_body[0])]),
        );
        assert!(
            !type4_equivalent(&cpg, left, right, &options, &mut cache, &mut invocations).unwrap()
        );

        cache.insert(
            right,
            projection(
                right,
                &right_body,
                &[(right, right_body[0]), (right_body[0], right_body[1])],
            ),
        );
        assert!(
            !type4_equivalent(&cpg, left, right, &options, &mut cache, &mut invocations).unwrap()
        );
    }

    #[test]
    fn shared_ast_and_disconnected_relations_remain_canonical() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (root, body) = add_function(
            &mut cpg,
            "dag",
            vec![identifier("parent"), identifier("shared")],
        );
        cpg.connect(body[0], body[1], CpgEdgeKind::AstChild);
        let scope = function_scope_nodes(&cpg, root);
        assert_eq!(
            merkle_fingerprints(&cpg, root, &scope),
            merkle_fingerprints(&cpg, root, &scope)
        );

        let projection = GraphProjection::from_edges(
            crate::ProjectionKind::External,
            [root, NodeId::new(10_000)],
            [],
        );
        assert_eq!(pdg_cpg(&cpg, &projection).node_count(), 1);

        let features = [10, 20, 30, 40]
            .into_iter()
            .map(|id| FunctionFeatures {
                root: NodeId::new(id),
                type1: 0,
                type2: 0,
                bag: BTreeMap::new(),
                bag_size: 0,
            })
            .collect::<Vec<_>>();
        let relations = [
            PairRelation {
                left: 0,
                right: 1,
                clone_type: CloneType::Type3,
                similarity: 0.9,
            },
            PairRelation {
                left: 1,
                right: 0,
                clone_type: CloneType::Type3,
                similarity: 0.8,
            },
            PairRelation {
                left: 2,
                right: 3,
                clone_type: CloneType::Type3,
                similarity: 0.85,
            },
        ];
        assert_eq!(
            group_relations(&features, &relations),
            vec![
                CloneClass {
                    clone_type: CloneType::Type3,
                    members: vec![NodeId::new(10), NodeId::new(20)],
                    similarity: 0.8,
                },
                CloneClass {
                    clone_type: CloneType::Type3,
                    members: vec![NodeId::new(30), NodeId::new(40)],
                    similarity: 0.85,
                },
            ]
        );
    }

    fn relabeled_pair(ids: [u32; 6]) -> (CodePropertyGraph, BTreeMap<NodeId, usize>) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let kinds = [
            function_kind("same"),
            identifier("alpha"),
            integer(3),
            function_kind("same"),
            identifier("beta"),
            integer(9),
        ];
        let mut inverse = BTreeMap::new();
        for (canonical, (id, kind)) in ids.into_iter().zip(kinds).enumerate() {
            let id = NodeId::new(id);
            inverse.insert(id, canonical);
            cpg.add_node_with_id(CpgNode::new(id, kind, SourceRange::default()));
        }
        for (source, target) in [(0, 1), (0, 2), (3, 4), (3, 5)] {
            cpg.connect(
                NodeId::new(ids[source]),
                NodeId::new(ids[target]),
                CpgEdgeKind::AstChild,
            );
        }
        (cpg, inverse)
    }

    proptest! {
        #[test]
        fn detection_is_byte_deterministic_and_id_permutation_invariant(
            ids in prop::collection::vec(0u32..1_000, 6)
                .prop_filter("identifiers must be unique", |ids| {
                    ids.iter().copied().collect::<BTreeSet<_>>().len() == 6
                })
        ) {
            let canonical = [0, 1, 2, 3, 4, 5];
            let (baseline, baseline_inverse) = relabeled_pair(canonical);
            let ids: [u32; 6] = ids.try_into().unwrap();
            let (permuted, permuted_inverse) = relabeled_pair(ids);

            let first = detect_clones(&baseline, &CloneOptions::default()).unwrap();
            let second = detect_clones(&baseline, &CloneOptions::default()).unwrap();
            prop_assert_eq!(format!("{first:?}").into_bytes(), format!("{second:?}").into_bytes());
            #[cfg(feature = "serde")]
            prop_assert_eq!(
                serde_json::to_vec(&first).unwrap(),
                serde_json::to_vec(&second).unwrap()
            );

            let normalize = |classes: Vec<CloneClass>, inverse: &BTreeMap<NodeId, usize>| {
                classes.into_iter().map(|class| {
                    let mut members = class.members.into_iter().map(|id| inverse[&id]).collect::<Vec<_>>();
                    members.sort_unstable();
                    (class.clone_type, members, class.similarity.to_bits())
                }).collect::<Vec<_>>()
            };
            prop_assert_eq!(
                normalize(first, &baseline_inverse),
                normalize(detect_clones(&permuted, &CloneOptions::default()).unwrap(), &permuted_inverse)
            );
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn public_types_round_trip_through_json() {
        let options = CloneOptions::default();
        let encoded = serde_json::to_string(&options).unwrap();
        assert_eq!(
            serde_json::from_str::<CloneOptions>(&encoded).unwrap(),
            options
        );

        let class = CloneClass {
            clone_type: CloneType::Type4,
            members: vec![NodeId::new(2), NodeId::new(7)],
            similarity: 0.875,
        };
        let encoded = serde_json::to_string(&class).unwrap();
        assert_eq!(serde_json::from_str::<CloneClass>(&encoded).unwrap(), class);
    }
}
