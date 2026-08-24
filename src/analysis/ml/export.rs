//! Canonical CPG tensors and AST path contexts.

use std::{
    cmp::Reverse,
    collections::{BTreeMap, BinaryHeap, VecDeque},
};

use crate::{
    backward_slice, CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, DfgEdgeKind,
    HeapEdgeKind, LiteralKind, NameFlowEdgeKind, NodeId, SourceRange,
};

use super::super::{
    function_scope_nodes, pagerank, reverse_pagerank, AnalysisError, GraphProjection,
    PageRankOptions, ProjectionKind,
};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Number of stable node-kind classes in the feature schema.
const NODE_KIND_CLASS_COUNT: usize = 45;
/// Number of coarse CPG overlay layers with separate in/out degree features.
const EDGE_LAYER_COUNT: usize = 11;
const IN_DEGREE_OFFSET: usize = NODE_KIND_CLASS_COUNT;
const OUT_DEGREE_OFFSET: usize = IN_DEGREE_OFFSET + EDGE_LAYER_COUNT;
const AST_DEPTH_INDEX: usize = OUT_DEGREE_OFFSET + EDGE_LAYER_COUNT;
const AST_DEPTH_KNOWN_INDEX: usize = AST_DEPTH_INDEX + 1;
const LOOP_DEPTH_INDEX: usize = AST_DEPTH_KNOWN_INDEX + 1;
const PAGERANK_INDEX: usize = LOOP_DEPTH_INDEX + 1;
const REVERSE_PAGERANK_INDEX: usize = PAGERANK_INDEX + 1;
const PDG_SLICE_INDEX: usize = REVERSE_PAGERANK_INDEX + 1;

/// Number of columns in every [`PygExport::node_features`] row.
pub const PYG_NODE_FEATURE_COUNT: usize = PDG_SLICE_INDEX + 1;

/// Stable, ordered names for the columns in [`PygExport::node_features`].
pub const PYG_NODE_FEATURE_NAMES: [&str; PYG_NODE_FEATURE_COUNT] = [
    "kind.root",
    "kind.module",
    "kind.class",
    "kind.struct",
    "kind.enum",
    "kind.trait",
    "kind.impl",
    "kind.function",
    "kind.parameter",
    "kind.block",
    "kind.variable",
    "kind.field",
    "kind.return",
    "kind.if",
    "kind.else",
    "kind.while",
    "kind.for",
    "kind.loop",
    "kind.match",
    "kind.match_arm",
    "kind.break",
    "kind.continue",
    "kind.throw",
    "kind.try",
    "kind.catch",
    "kind.finally",
    "kind.binary_op",
    "kind.unary_op",
    "kind.assignment",
    "kind.call",
    "kind.member_access",
    "kind.index_access",
    "kind.identifier",
    "kind.literal",
    "kind.lambda",
    "kind.await",
    "kind.yield",
    "kind.type_annotation",
    "kind.generic_param",
    "kind.comment",
    "kind.import",
    "kind.attribute",
    "kind.macro",
    "kind.error",
    "kind.unknown",
    "degree.in.ast",
    "degree.in.cfg",
    "degree.in.dfg",
    "degree.in.pdg",
    "degree.in.call",
    "degree.in.type",
    "degree.in.reference",
    "degree.in.scope",
    "degree.in.import",
    "degree.in.heap",
    "degree.in.name_flow",
    "degree.out.ast",
    "degree.out.cfg",
    "degree.out.dfg",
    "degree.out.pdg",
    "degree.out.call",
    "degree.out.type",
    "degree.out.reference",
    "degree.out.scope",
    "degree.out.import",
    "degree.out.heap",
    "degree.out.name_flow",
    "ast.depth",
    "ast.depth_known",
    "ast.loop_depth",
    "centrality.pagerank",
    "centrality.reverse_pagerank",
    "localization.backward_pdg_slice",
];

/// Number of stable classes produced by [`edge_kind_class`].
pub const EDGE_KIND_CLASS_COUNT: u16 = 60;

/// Maximum accepted AST path length in edges.
pub const MAX_AST_PATH_LENGTH: usize = 1_000_000;
/// Maximum accepted terminal count for one function.
pub const MAX_AST_PATH_TERMINALS: usize = 100_000;
/// Maximum accepted terminal-pair examinations.
pub const MAX_AST_PATH_PAIR_CHECKS: usize = 10_000_000;
/// Maximum accepted path-context count.
pub const MAX_AST_PATH_CONTEXTS: usize = 1_000_000;
/// Maximum accepted aggregate nodes retained by emitted paths.
pub const MAX_AST_PATH_TOTAL_NODES: usize = 20_000_000;
/// Maximum accepted aggregate terminal-to-function ancestor steps.
pub const MAX_AST_PATH_ANCESTOR_STEPS: usize = 20_000_000;

/// Canonical tensor-like export consumed by external PyTorch Geometric heads.
///
/// `node_ids[d]` maps dense id `d` back to the CPG. Every feature row and
/// source range is aligned with that same order. `edge_index` stores
/// `(source_dense, target_dense, edge_kind_class)` and preserves parallel edge
/// multiplicity while sorting independently of graph insertion order.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PygExport {
    /// Dense node-feature rows aligned with `node_ids`.
    pub node_features: Vec<Vec<f32>>,
    /// Canonical typed edges over dense node identifiers.
    pub edge_index: Vec<(u32, u32, u16)>,
    /// Stable CPG node identifiers in dense order.
    pub node_ids: Vec<NodeId>,
    /// Source ranges aligned one-for-one with `node_ids`.
    pub source_ranges: Vec<SourceRange>,
    /// Optional criterion whose complete backward PDG slice is encoded.
    pub slice_criterion: Option<NodeId>,
    /// Sorted exact members of that backward PDG slice.
    pub slice_node_ids: Vec<NodeId>,
    /// Whether forward PageRank met its numerical tolerance.
    pub pagerank_converged: bool,
    /// Whether reverse PageRank met its numerical tolerance.
    pub reverse_pagerank_converged: bool,
}

impl PygExport {
    /// Returns the dense index of a stable node identifier.
    pub fn dense_index(&self, node: NodeId) -> Option<u32> {
        self.node_ids
            .binary_search(&node)
            .ok()
            .map(|index| index as u32)
    }

    /// Returns the feature column index with this stable schema name.
    pub fn feature_index(name: &str) -> Option<usize> {
        PYG_NODE_FEATURE_NAMES
            .iter()
            .position(|candidate| *candidate == name)
    }
}

/// Optional backward-PDG localization policy for [`export_pyg_with_options`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PygExportOptions {
    /// Criterion whose complete backward PDG slice should be marked.
    pub slice_criterion: Option<NodeId>,
    /// Maximum complete slice size accepted from that criterion.
    pub max_slice_nodes: usize,
}

impl Default for PygExportOptions {
    fn default() -> Self {
        Self {
            slice_criterion: None,
            max_slice_nodes: 100_000,
        }
    }
}

/// One code2vec/code2seq-style terminal-to-terminal AST path context.
///
/// `path[ancestor_index] == ancestor`; consecutive entries are exact
/// `AstChild` neighbors, with the first segment walking upward and the second
/// walking downward. `node_kinds` is aligned one-for-one with `path`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct PathContext {
    /// Source terminal.
    pub start: NodeId,
    /// Destination terminal.
    pub end: NodeId,
    /// Lowest common AST ancestor.
    pub ancestor: NodeId,
    /// Position of `ancestor` in `path`.
    pub ancestor_index: usize,
    /// Exact terminal-to-ancestor-to-terminal node path.
    pub path: Vec<NodeId>,
    /// Stable node-kind classes aligned with `path`.
    pub node_kinds: Vec<u16>,
    /// Exact source token when available, otherwise a stable semantic token.
    pub start_token: String,
    /// Exact destination token when available, otherwise a stable semantic token.
    pub end_token: String,
}

/// Resource and semantic filters for [`ast_paths`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AstPathOptions {
    /// Maximum number of edges in one retained terminal path.
    pub max_path_length: usize,
    /// Maximum source-ordered child-slot distance at the lowest common ancestor.
    pub max_path_width: usize,
    /// Maximum terminals accepted before pair enumeration.
    pub max_terminals: usize,
    /// Maximum terminal pairs examined, including filtered pairs.
    pub max_pair_checks: usize,
    /// Maximum retained contexts; excess is an error, never truncation.
    pub max_contexts: usize,
    /// Maximum aggregate node ids retained across emitted paths.
    pub max_total_path_nodes: usize,
    /// Maximum aggregate terminal-to-function ancestor steps retained internally.
    pub max_ancestor_steps: usize,
}

impl Default for AstPathOptions {
    fn default() -> Self {
        Self {
            max_path_length: 8,
            max_path_width: 2,
            max_terminals: 10_000,
            max_pair_checks: 1_000_000,
            max_contexts: 200_000,
            max_total_path_nodes: 2_000_000,
            max_ancestor_steps: 2_000_000,
        }
    }
}

/// Exports the complete CPG with no PDG criterion and no ML/runtime dependency.
pub fn export_pyg(cpg: &CodePropertyGraph) -> PygExport {
    build_pyg_export(cpg, None, Vec::new())
}

/// Exports the complete CPG and optionally marks one complete backward PDG slice.
///
/// A slice larger than `max_slice_nodes` returns a budget error instead of a
/// partial localization that a downstream model could mistake for complete.
pub fn export_pyg_with_options(
    cpg: &CodePropertyGraph,
    options: PygExportOptions,
) -> Result<PygExport, AnalysisError> {
    let slice_nodes = match options.slice_criterion {
        None => Vec::new(),
        Some(criterion) => {
            if !cpg.contains_node(criterion) {
                return Err(AnalysisError::UnknownNode(criterion));
            }
            let complete = backward_slice(cpg, criterion, cpg.node_count().max(1));
            let complete = heap_sorted(complete);
            enforce_budget(
                "export_pyg",
                "slice nodes",
                complete.len(),
                options.max_slice_nodes,
            )?;
            complete
        }
    };
    Ok(build_pyg_export(cpg, options.slice_criterion, slice_nodes))
}

fn build_pyg_export(
    cpg: &CodePropertyGraph,
    slice_criterion: Option<NodeId>,
    slice_node_ids: Vec<NodeId>,
) -> PygExport {
    let node_ids = heap_sorted(cpg.node_ids());
    let dense: BTreeMap<_, _> = node_ids
        .iter()
        .copied()
        .enumerate()
        .map(|(index, node)| (node, index as u32))
        .collect();
    let mut in_degree = vec![[0usize; EDGE_LAYER_COUNT]; node_ids.len()];
    let mut out_degree = vec![[0usize; EDGE_LAYER_COUNT]; node_ids.len()];
    let mut edge_heap = BinaryHeap::with_capacity(cpg.edge_count());
    for edge in cpg.edges() {
        let (Some(&source), Some(&target)) = (dense.get(&edge.source), dense.get(&edge.target))
        else {
            continue;
        };
        let layer = edge_layer(&edge.kind);
        out_degree[source as usize][layer] = out_degree[source as usize][layer].saturating_add(1);
        in_degree[target as usize][layer] = in_degree[target as usize][layer].saturating_add(1);
        edge_heap.push(Reverse((source, target, edge_kind_class(&edge.kind))));
    }
    let mut edge_index = Vec::with_capacity(edge_heap.len());
    while let Some(Reverse(edge)) = edge_heap.pop() {
        edge_index.push(edge);
    }

    let (ast_depth, loop_depth, ast_depth_known) = ast_features(cpg, &node_ids, &dense);
    let topology = GraphProjection::from_edges(
        ProjectionKind::External,
        node_ids.iter().copied(),
        cpg.edges().map(|edge| (edge.source, edge.target, 1.0)),
    );
    let forward = pagerank(&topology, PageRankOptions::default());
    let reverse = reverse_pagerank(&topology, PageRankOptions::default());

    let slice_members: BTreeMap<_, _> = slice_node_ids
        .iter()
        .copied()
        .map(|node| (node, ()))
        .collect();
    let mut node_features = Vec::with_capacity(node_ids.len());
    let mut source_ranges = Vec::with_capacity(node_ids.len());
    for (index, &node_id) in node_ids.iter().enumerate() {
        let node = cpg
            .node(node_id)
            .expect("node_ids are enumerated from existing CPG nodes");
        source_ranges.push(node.range);
        let mut row = vec![0.0; PYG_NODE_FEATURE_COUNT];
        row[node_kind_class(&node.kind) as usize] = 1.0;
        for layer in 0..EDGE_LAYER_COUNT {
            row[IN_DEGREE_OFFSET + layer] = in_degree[index][layer] as f32;
            row[OUT_DEGREE_OFFSET + layer] = out_degree[index][layer] as f32;
        }
        row[AST_DEPTH_INDEX] = ast_depth[index] as f32;
        row[AST_DEPTH_KNOWN_INDEX] = f32::from(ast_depth_known[index]);
        row[LOOP_DEPTH_INDEX] = loop_depth[index] as f32;
        row[PAGERANK_INDEX] = forward.scores[index] as f32;
        row[REVERSE_PAGERANK_INDEX] = reverse.scores[index] as f32;
        row[PDG_SLICE_INDEX] = f32::from(slice_members.contains_key(&node_id));
        node_features.push(row);
    }

    PygExport {
        node_features,
        edge_index,
        node_ids,
        source_ranges,
        slice_criterion,
        slice_node_ids,
        pagerank_converged: forward.converged,
        reverse_pagerank_converged: reverse.converged,
    }
}

fn ast_features(
    cpg: &CodePropertyGraph,
    node_ids: &[NodeId],
    dense: &BTreeMap<NodeId, u32>,
) -> (Vec<u32>, Vec<u32>, Vec<bool>) {
    let mut edge_heap = BinaryHeap::new();
    for edge in cpg
        .edges()
        .filter(|edge| matches!(edge.kind, CpgEdgeKind::AstChild))
    {
        if let (Some(&parent), Some(&child)) = (dense.get(&edge.source), dense.get(&edge.target)) {
            edge_heap.push(Reverse((parent, child)));
        }
    }
    let mut children = vec![Vec::new(); node_ids.len()];
    let mut parent_count = vec![0usize; node_ids.len()];
    let mut previous = None;
    while let Some(Reverse((parent, child))) = edge_heap.pop() {
        if previous == Some((parent, child)) {
            continue;
        }
        previous = Some((parent, child));
        children[parent as usize].push(child);
        parent_count[child as usize] = parent_count[child as usize].saturating_add(1);
    }

    let mut depth = vec![0u32; node_ids.len()];
    let mut loop_depth = vec![0u32; node_ids.len()];
    let mut known = vec![false; node_ids.len()];
    let mut queue = VecDeque::new();
    for index in 0..node_ids.len() {
        if parent_count[index] == 0 {
            known[index] = true;
            loop_depth[index] = u32::from(
                cpg.node(node_ids[index])
                    .is_some_and(|node| is_loop_kind(&node.kind)),
            );
            queue.push_back(index as u32);
        }
    }
    while let Some(parent) = queue.pop_front() {
        for &child in &children[parent as usize] {
            let child = child as usize;
            if parent_count[child] != 1 || known[child] {
                continue;
            }
            depth[child] = depth[parent as usize].saturating_add(1);
            loop_depth[child] = loop_depth[parent as usize].saturating_add(u32::from(
                cpg.node(node_ids[child])
                    .is_some_and(|node| is_loop_kind(&node.kind)),
            ));
            known[child] = true;
            queue.push_back(child as u32);
        }
    }
    (depth, loop_depth, known)
}

/// Extracts exact terminal-to-ancestor-to-terminal AST path contexts.
///
/// Malformed terminals with a missing, cyclic, or ambiguous parent chain are
/// suppressed. Every cap violation returns an error before a partial result is
/// returned. All source-depth state resides in heap vectors.
pub fn ast_paths(
    cpg: &CodePropertyGraph,
    function: NodeId,
    options: AstPathOptions,
) -> Result<Vec<PathContext>, AnalysisError> {
    validate_function(cpg, function)?;
    validate_path_options(options)?;

    let scope = function_scope_nodes(cpg, function);
    let scope_nodes = heap_sorted(scope);
    let dense: BTreeMap<_, _> = scope_nodes
        .iter()
        .copied()
        .enumerate()
        .map(|(index, node)| (node, index as u32))
        .collect();
    let function_dense = dense[&function];
    let (parents, children, child_positions) = ast_topology(cpg, &scope_nodes, &dense);

    let mut terminal_heap = BinaryHeap::new();
    for (index, &node) in scope_nodes.iter().enumerate() {
        if node == function || !children[index].is_empty() {
            continue;
        }
        let range = cpg
            .node(node)
            .map_or(SourceRange::default(), |node| node.range);
        terminal_heap.push(Reverse((
            range.start,
            range.end,
            range.start_line,
            range.start_col,
            node,
            index as u32,
        )));
    }
    enforce_budget(
        "ast_paths",
        "terminals",
        terminal_heap.len(),
        options.max_terminals,
    )?;

    let mut marks = vec![0u64; scope_nodes.len()];
    let mut generation = 0u64;
    let mut chains = Vec::with_capacity(terminal_heap.len());
    let mut ancestor_steps = 0usize;
    while let Some(Reverse((_, _, _, _, _, terminal))) = terminal_heap.pop() {
        generation = generation.wrapping_add(1);
        if generation == 0 {
            marks.fill(0);
            generation = 1;
        }
        let mut upward = Vec::new();
        let mut current = terminal;
        let complete = loop {
            if marks[current as usize] == generation {
                break false;
            }
            marks[current as usize] = generation;
            upward.push(current);
            ancestor_steps = ancestor_steps.saturating_add(1);
            enforce_budget(
                "ast_paths",
                "ancestor steps",
                ancestor_steps,
                options.max_ancestor_steps,
            )?;
            if current == function_dense {
                break true;
            }
            if parents[current as usize].len() != 1 {
                break false;
            }
            current = parents[current as usize][0];
        };
        if complete {
            chains.push(TerminalChain { terminal, upward });
        }
    }

    let pair_checks = chains
        .len()
        .checked_mul(chains.len().saturating_sub(1))
        .map(|twice| twice / 2)
        .unwrap_or(usize::MAX);
    enforce_budget(
        "ast_paths",
        "terminal pairs",
        pair_checks,
        options.max_pair_checks,
    )?;

    let mut contexts = Vec::new();
    let mut total_path_nodes = 0usize;
    for left in 0..chains.len() {
        for right in left + 1..chains.len() {
            let Some((left_ancestor, right_ancestor)) =
                common_ancestor_offsets(&chains[left].upward, &chains[right].upward)
            else {
                continue;
            };
            let path_length = left_ancestor.saturating_add(right_ancestor);
            if path_length > options.max_path_length || left_ancestor == 0 || right_ancestor == 0 {
                continue;
            }
            let ancestor = chains[left].upward[left_ancestor];
            let left_branch = chains[left].upward[left_ancestor - 1];
            let right_branch = chains[right].upward[right_ancestor - 1];
            let (Some(&left_position), Some(&right_position)) = (
                child_positions.get(&(ancestor, left_branch)),
                child_positions.get(&(ancestor, right_branch)),
            ) else {
                continue;
            };
            if left_position.abs_diff(right_position) > options.max_path_width {
                continue;
            }

            enforce_budget(
                "ast_paths",
                "contexts",
                contexts.len().saturating_add(1),
                options.max_contexts,
            )?;
            let path_nodes = path_length.saturating_add(1);
            total_path_nodes = total_path_nodes.saturating_add(path_nodes);
            enforce_budget(
                "ast_paths",
                "retained path nodes",
                total_path_nodes,
                options.max_total_path_nodes,
            )?;

            let mut path = Vec::with_capacity(path_nodes);
            path.extend(
                chains[left].upward[..=left_ancestor]
                    .iter()
                    .map(|&index| scope_nodes[index as usize]),
            );
            path.extend(
                chains[right].upward[..right_ancestor]
                    .iter()
                    .rev()
                    .map(|&index| scope_nodes[index as usize]),
            );
            let node_kinds = path
                .iter()
                .map(|&node| {
                    cpg.node(node)
                        .map_or(44, |node| node_kind_class(&node.kind))
                })
                .collect();
            let start = scope_nodes[chains[left].terminal as usize];
            let end = scope_nodes[chains[right].terminal as usize];
            contexts.push(PathContext {
                start,
                end,
                ancestor: scope_nodes[ancestor as usize],
                ancestor_index: left_ancestor,
                path,
                node_kinds,
                start_token: terminal_token(
                    cpg.node(start)
                        .expect("terminal belongs to the validated function scope"),
                ),
                end_token: terminal_token(
                    cpg.node(end)
                        .expect("terminal belongs to the validated function scope"),
                ),
            });
        }
    }
    Ok(contexts)
}

type AstTopology = (Vec<Vec<u32>>, Vec<Vec<u32>>, BTreeMap<(u32, u32), usize>);

fn ast_topology(
    cpg: &CodePropertyGraph,
    scope_nodes: &[NodeId],
    dense: &BTreeMap<NodeId, u32>,
) -> AstTopology {
    let mut edge_heap = BinaryHeap::new();
    for edge in cpg
        .edges()
        .filter(|edge| matches!(edge.kind, CpgEdgeKind::AstChild))
    {
        let (Some(&parent), Some(&child)) = (dense.get(&edge.source), dense.get(&edge.target))
        else {
            continue;
        };
        let range = cpg
            .node(edge.target)
            .map_or(SourceRange::default(), |node| node.range);
        edge_heap.push(Reverse((
            parent,
            range.start,
            range.end,
            range.start_line,
            range.start_col,
            edge.target,
            child,
        )));
    }
    let mut parents = vec![Vec::new(); scope_nodes.len()];
    let mut children = vec![Vec::new(); scope_nodes.len()];
    let mut previous = None;
    while let Some(Reverse((parent, _, _, _, _, _, child))) = edge_heap.pop() {
        if previous == Some((parent, child)) {
            continue;
        }
        previous = Some((parent, child));
        parents[child as usize].push(parent);
        children[parent as usize].push(child);
    }
    for candidates in &mut parents {
        *candidates = heap_sorted(std::mem::take(candidates));
        candidates.dedup();
    }
    let mut child_positions = BTreeMap::new();
    for (parent, ordered) in children.iter().enumerate() {
        for (position, &child) in ordered.iter().enumerate() {
            child_positions.insert((parent as u32, child), position);
        }
    }
    (parents, children, child_positions)
}

#[derive(Debug)]
struct TerminalChain {
    terminal: u32,
    upward: Vec<u32>,
}

fn common_ancestor_offsets(left: &[u32], right: &[u32]) -> Option<(usize, usize)> {
    let (mut left_index, mut right_index) = (left.len(), right.len());
    let mut found = false;
    while left_index > 0 && right_index > 0 && left[left_index - 1] == right[right_index - 1] {
        found = true;
        left_index -= 1;
        right_index -= 1;
    }
    found.then_some((left_index, right_index))
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

fn validate_path_options(options: AstPathOptions) -> Result<(), AnalysisError> {
    for (input, requested, hard) in [
        (
            "max_path_length",
            options.max_path_length,
            MAX_AST_PATH_LENGTH,
        ),
        (
            "max_terminals",
            options.max_terminals,
            MAX_AST_PATH_TERMINALS,
        ),
        (
            "max_pair_checks",
            options.max_pair_checks,
            MAX_AST_PATH_PAIR_CHECKS,
        ),
        ("max_contexts", options.max_contexts, MAX_AST_PATH_CONTEXTS),
        (
            "max_total_path_nodes",
            options.max_total_path_nodes,
            MAX_AST_PATH_TOTAL_NODES,
        ),
        (
            "max_ancestor_steps",
            options.max_ancestor_steps,
            MAX_AST_PATH_ANCESTOR_STEPS,
        ),
    ] {
        if requested > hard {
            return Err(AnalysisError::InvalidInput {
                analysis: "ast_paths",
                input,
                reason: "exceeds the compile-time hard ceiling",
            });
        }
    }
    Ok(())
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
            needed: needed.try_into().unwrap_or(u64::MAX),
            cap: cap.try_into().unwrap_or(u64::MAX),
        });
    }
    Ok(())
}

fn heap_sorted<T: Ord>(values: impl IntoIterator<Item = T>) -> Vec<T> {
    let iterator = values.into_iter();
    let mut heap = BinaryHeap::with_capacity(iterator.size_hint().0);
    for value in iterator {
        heap.push(Reverse(value));
    }
    let mut output = Vec::with_capacity(heap.len());
    while let Some(Reverse(value)) = heap.pop() {
        output.push(value);
    }
    output
}

fn is_loop_kind(kind: &CpgNodeKind) -> bool {
    matches!(
        kind,
        CpgNodeKind::While | CpgNodeKind::For | CpgNodeKind::Loop
    )
}

fn edge_layer(kind: &CpgEdgeKind) -> usize {
    match kind {
        CpgEdgeKind::AstChild
        | CpgEdgeKind::AstParent
        | CpgEdgeKind::AstNextSibling
        | CpgEdgeKind::AstPrevSibling => 0,
        CpgEdgeKind::ControlFlow(_) => 1,
        CpgEdgeKind::DataFlow(_) => 2,
        CpgEdgeKind::ControlDependence | CpgEdgeKind::DataDependence => 3,
        CpgEdgeKind::StaticCall | CpgEdgeKind::DynamicCall | CpgEdgeKind::CallSite => 4,
        CpgEdgeKind::TypeOf
        | CpgEdgeKind::Inherits
        | CpgEdgeKind::Implements
        | CpgEdgeKind::GenericInstance => 5,
        CpgEdgeKind::Reference | CpgEdgeKind::Definition | CpgEdgeKind::Declaration => 6,
        CpgEdgeKind::EnclosingScope | CpgEdgeKind::ContainedIn => 7,
        CpgEdgeKind::Imports | CpgEdgeKind::Exports => 8,
        CpgEdgeKind::Heap(_) => 9,
        CpgEdgeKind::NameFlow(_) => 10,
    }
}

/// Maps every CPG edge kind to a stable relation class in `0..60`.
pub const fn edge_kind_class(kind: &CpgEdgeKind) -> u16 {
    match kind {
        CpgEdgeKind::AstChild => 0,
        CpgEdgeKind::AstParent => 1,
        CpgEdgeKind::AstNextSibling => 2,
        CpgEdgeKind::AstPrevSibling => 3,
        CpgEdgeKind::ControlFlow(kind) => 4 + cfg_kind_class(*kind),
        CpgEdgeKind::DataFlow(kind) => 18 + dfg_kind_class(*kind),
        CpgEdgeKind::ControlDependence => 31,
        CpgEdgeKind::DataDependence => 32,
        CpgEdgeKind::StaticCall => 33,
        CpgEdgeKind::DynamicCall => 34,
        CpgEdgeKind::CallSite => 35,
        CpgEdgeKind::TypeOf => 36,
        CpgEdgeKind::Inherits => 37,
        CpgEdgeKind::Implements => 38,
        CpgEdgeKind::GenericInstance => 39,
        CpgEdgeKind::Reference => 40,
        CpgEdgeKind::Definition => 41,
        CpgEdgeKind::Declaration => 42,
        CpgEdgeKind::EnclosingScope => 43,
        CpgEdgeKind::ContainedIn => 44,
        CpgEdgeKind::Imports => 45,
        CpgEdgeKind::Exports => 46,
        CpgEdgeKind::Heap(kind) => 47 + heap_kind_class(*kind),
        CpgEdgeKind::NameFlow(kind) => 53 + name_flow_kind_class(*kind),
    }
}

const fn name_flow_kind_class(kind: NameFlowEdgeKind) -> u16 {
    match kind {
        NameFlowEdgeKind::Creates => 0,
        NameFlowEdgeKind::References => 1,
        NameFlowEdgeKind::Quotes => 2,
        NameFlowEdgeKind::Drops => 3,
        NameFlowEdgeKind::Carries => 4,
        NameFlowEdgeKind::Receives => 5,
        NameFlowEdgeKind::Communicates => 6,
    }
}

const fn heap_kind_class(kind: HeapEdgeKind) -> u16 {
    match kind {
        HeapEdgeKind::Allocates => 0,
        HeapEdgeKind::PointsTo => 1,
        HeapEdgeKind::Dereferences => 2,
        HeapEdgeKind::Releases => 3,
        HeapEdgeKind::RequiresValid => 4,
        HeapEdgeKind::EnsuresInvalid => 5,
    }
}

const fn cfg_kind_class(kind: CfgEdgeKind) -> u16 {
    match kind {
        CfgEdgeKind::Sequential => 0,
        CfgEdgeKind::ConditionalTrue => 1,
        CfgEdgeKind::ConditionalFalse => 2,
        CfgEdgeKind::LoopBack => 3,
        CfgEdgeKind::LoopExit => 4,
        CfgEdgeKind::Break => 5,
        CfgEdgeKind::Continue => 6,
        CfgEdgeKind::Return => 7,
        CfgEdgeKind::Throw => 8,
        CfgEdgeKind::Catch => 9,
        CfgEdgeKind::Call => 10,
        CfgEdgeKind::CallReturn => 11,
        CfgEdgeKind::Case => 12,
        CfgEdgeKind::DefaultCase => 13,
    }
}

const fn dfg_kind_class(kind: DfgEdgeKind) -> u16 {
    match kind {
        DfgEdgeKind::DefUse => 0,
        DfgEdgeKind::UseDef => 1,
        DfgEdgeKind::ReachingDef => 2,
        DfgEdgeKind::DataDependency => 3,
        DfgEdgeKind::Parameter => 4,
        DfgEdgeKind::ReturnValue => 5,
        DfgEdgeKind::FieldRead => 6,
        DfgEdgeKind::FieldWrite => 7,
        DfgEdgeKind::IndexRead => 8,
        DfgEdgeKind::IndexWrite => 9,
        DfgEdgeKind::Alias => 10,
        DfgEdgeKind::Dereference => 11,
        DfgEdgeKind::AddressOf => 12,
    }
}

/// Maps every CPG node kind to a stable one-hot class in `0..45`.
pub const fn node_kind_class(kind: &CpgNodeKind) -> u16 {
    match kind {
        CpgNodeKind::Root => 0,
        CpgNodeKind::Module { .. } => 1,
        CpgNodeKind::Class { .. } => 2,
        CpgNodeKind::Struct { .. } => 3,
        CpgNodeKind::Enum { .. } => 4,
        CpgNodeKind::Trait { .. } => 5,
        CpgNodeKind::Impl { .. } => 6,
        CpgNodeKind::Function { .. } => 7,
        CpgNodeKind::Parameter { .. } => 8,
        CpgNodeKind::Block { .. } => 9,
        CpgNodeKind::Variable { .. } => 10,
        CpgNodeKind::Field { .. } => 11,
        CpgNodeKind::Return => 12,
        CpgNodeKind::If => 13,
        CpgNodeKind::Else => 14,
        CpgNodeKind::While => 15,
        CpgNodeKind::For => 16,
        CpgNodeKind::Loop => 17,
        CpgNodeKind::Match => 18,
        CpgNodeKind::MatchArm => 19,
        CpgNodeKind::Break => 20,
        CpgNodeKind::Continue => 21,
        CpgNodeKind::Throw => 22,
        CpgNodeKind::Try => 23,
        CpgNodeKind::Catch => 24,
        CpgNodeKind::Finally => 25,
        CpgNodeKind::BinaryOp { .. } => 26,
        CpgNodeKind::UnaryOp { .. } => 27,
        CpgNodeKind::Assignment { .. } => 28,
        CpgNodeKind::Call { .. } => 29,
        CpgNodeKind::MemberAccess { .. } => 30,
        CpgNodeKind::IndexAccess => 31,
        CpgNodeKind::Identifier { .. } => 32,
        CpgNodeKind::Literal { .. } => 33,
        CpgNodeKind::Lambda { .. } => 34,
        CpgNodeKind::Await => 35,
        CpgNodeKind::Yield => 36,
        CpgNodeKind::TypeAnnotation { .. } => 37,
        CpgNodeKind::GenericParam { .. } => 38,
        CpgNodeKind::Comment { .. } => 39,
        CpgNodeKind::Import { .. } => 40,
        CpgNodeKind::Attribute { .. } => 41,
        CpgNodeKind::Macro { .. } => 42,
        CpgNodeKind::Error { .. } => 43,
        CpgNodeKind::Unknown { .. } => 44,
    }
}

fn terminal_token(node: &CpgNode) -> String {
    if let Some(text) = node.text.as_deref().filter(|text| !text.is_empty()) {
        return text.to_owned();
    }
    if let Some(name) = node.name() {
        return name.to_owned();
    }
    match &node.kind {
        CpgNodeKind::BinaryOp { operator }
        | CpgNodeKind::UnaryOp { operator }
        | CpgNodeKind::Assignment { operator } => operator.to_string(),
        CpgNodeKind::Literal { kind } => match kind {
            LiteralKind::Integer(value) => value.to_string(),
            LiteralKind::Float(value) => value.to_string(),
            LiteralKind::String(value) | LiteralKind::Regex(value) => value.to_string(),
            LiteralKind::Char(value) => value.to_string(),
            LiteralKind::Bool(value) => value.to_string(),
            LiteralKind::Null => "null".to_owned(),
            LiteralKind::Array => "array".to_owned(),
            LiteralKind::Object => "object".to_owned(),
        },
        _ => PYG_NODE_FEATURE_NAMES[node_kind_class(&node.kind) as usize]
            .trim_start_matches("kind.")
            .to_owned(),
    }
}
