//! VF2 subgraph isomorphism algorithm.
//!
//! This module implements the VF2 algorithm for finding subgraph isomorphisms
//! in Code Property Graphs. VF2 is efficient for finding pattern matches
//! in sparse graphs typical of code analysis.

use rayon::prelude::*;
#[cfg(test)]
use rustc_hash::FxHashSet;

use super::{NodeKindTag, PatternMatch, SubgraphMatcher};
use crate::{CodePropertyGraph, CpgEdgeKind, CpgNodeKind, NodeId};

const UNMAPPED: u32 = u32::MAX;

/// VF2 subgraph isomorphism matcher.
///
/// Implements the VF2 algorithm for finding all occurrences of a pattern
/// graph within a target graph.
#[derive(Debug, Clone, Default)]
pub struct Vf2Matcher {
    /// Whether to require exact node kind matches.
    strict_kinds: bool,
    /// Whether to require exact edge kind matches.
    strict_edges: bool,
    /// Maximum number of matches to find (0 = unlimited).
    max_matches: usize,
}

impl Vf2Matcher {
    /// Creates a new VF2 matcher with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets strict node kind matching.
    pub fn with_strict_kinds(mut self, strict: bool) -> Self {
        self.strict_kinds = strict;
        self
    }

    /// Sets strict edge kind matching.
    pub fn with_strict_edges(mut self, strict: bool) -> Self {
        self.strict_edges = strict;
        self
    }

    /// Sets the maximum number of matches to emit.
    ///
    /// Zero means unlimited. A positive value returns a deterministic prefix
    /// and stops after that many embeddings have been found; it does not bound
    /// the work needed to find the next embedding or prove that none exists.
    pub fn with_max_matches(mut self, max: usize) -> Self {
        self.max_matches = max;
        self
    }

    /// Finds matches for an ordered batch of pattern graphs.
    ///
    /// The outer result has one entry per input pattern in the same order. Each
    /// inner result retains [`SubgraphMatcher::find_matches`]'s deterministic
    /// embedding order.
    pub fn find_matches_many(
        &self,
        patterns: &[CodePropertyGraph],
        target: &CodePropertyGraph,
    ) -> Vec<Vec<PatternMatch>> {
        patterns
            .iter()
            .map(|pattern| self.find_matches(pattern, target))
            .collect()
    }

    /// Finds an ordered batch of patterns concurrently on the current Rayon
    /// pool.
    ///
    /// Rayon uses an indexed parallel iterator here: scheduling and worker
    /// count cannot reorder either the pattern results or any pattern's match
    /// list. Install a custom [`rayon::ThreadPool`] around this call to select
    /// the worker count or stack size.
    pub fn find_matches_many_parallel(
        &self,
        patterns: &[CodePropertyGraph],
        target: &CodePropertyGraph,
    ) -> Vec<Vec<PatternMatch>> {
        patterns
            .par_iter()
            .map(|pattern| self.find_matches(pattern, target))
            .collect()
    }

    /// Checks if two nodes are compatible for matching.
    fn nodes_compatible(&self, pattern_node: &CpgNodeKind, target_node: &CpgNodeKind) -> bool {
        node_kinds_compatible(self.strict_kinds, pattern_node, target_node)
    }

    /// Checks if two edges are compatible for matching.
    fn edges_compatible(&self, pattern_edge: &CpgEdgeKind, target_edge: &CpgEdgeKind) -> bool {
        if self.strict_edges {
            pattern_edge == target_edge
        } else {
            // Relaxed matching: sharing a broad category (AST/CFG/DFG/call) is
            // enough. The exact-equal fallback makes relaxed matching a genuine
            // *relaxation* of strict matching (relaxed ⊇ strict) for the edge
            // categories with no coarse bucket here — PDG (control/data
            // dependence), type, reference, scope, and import edges. Without it
            // two identical `ControlDependence` edges would be judged
            // incompatible, making the relaxed matcher paradoxically *stricter*
            // than the strict one for those kinds and breaking the superset
            // guarantee. A cross-category pair (e.g. AST vs CFG) still fails:
            // none of the category clauses hold and the kinds are unequal.
            (pattern_edge.is_ast() && target_edge.is_ast())
                || (pattern_edge.is_cfg() && target_edge.is_cfg())
                || (pattern_edge.is_dfg() && target_edge.is_dfg())
                || (pattern_edge.is_call() && target_edge.is_call())
                || pattern_edge == target_edge
        }
    }
}

fn node_kinds_compatible(
    strict: bool,
    pattern_node: &CpgNodeKind,
    target_node: &CpgNodeKind,
) -> bool {
    if strict {
        return NodeKindTag::from_kind(pattern_node) == NodeKindTag::from_kind(target_node);
    }
    (is_declaration(pattern_node) && is_declaration(target_node))
        || (is_expression(pattern_node) && is_expression(target_node))
        || (is_statement(pattern_node) && is_statement(target_node))
        || NodeKindTag::from_kind(pattern_node) == NodeKindTag::from_kind(target_node)
}

fn is_declaration(kind: &CpgNodeKind) -> bool {
    matches!(
        kind,
        CpgNodeKind::Module { .. }
            | CpgNodeKind::Class { .. }
            | CpgNodeKind::Struct { .. }
            | CpgNodeKind::Enum { .. }
            | CpgNodeKind::Trait { .. }
            | CpgNodeKind::Function { .. }
            | CpgNodeKind::Variable { .. }
            | CpgNodeKind::Field { .. }
    )
}

fn is_expression(kind: &CpgNodeKind) -> bool {
    matches!(
        kind,
        CpgNodeKind::BinaryOp { .. }
            | CpgNodeKind::UnaryOp { .. }
            | CpgNodeKind::Call { .. }
            | CpgNodeKind::MemberAccess { .. }
            | CpgNodeKind::IndexAccess
            | CpgNodeKind::Identifier { .. }
            | CpgNodeKind::Literal { .. }
            | CpgNodeKind::Lambda { .. }
    )
}

fn is_statement(kind: &CpgNodeKind) -> bool {
    matches!(
        kind,
        CpgNodeKind::Return
            | CpgNodeKind::If
            | CpgNodeKind::While
            | CpgNodeKind::For
            | CpgNodeKind::Loop
            | CpgNodeKind::Match
            | CpgNodeKind::Break
            | CpgNodeKind::Continue
    )
}

impl SubgraphMatcher for Vf2Matcher {
    fn find_matches(
        &self,
        pattern: &CodePropertyGraph,
        target: &CodePropertyGraph,
    ) -> Vec<PatternMatch> {
        let mut matches = Vec::new();

        if pattern.node_count() == 0 {
            return matches;
        }
        if target.node_count() < pattern.node_count() {
            return matches;
        }

        // Initialize VF2 state
        let mut state = Vf2State::new(pattern, target);

        // Run the VF2 algorithm
        self.vf2_search(&mut state, &mut matches);

        matches
    }

    fn find_matches_limited(
        &self,
        pattern: &CodePropertyGraph,
        target: &CodePropertyGraph,
        limit: usize,
    ) -> Vec<PatternMatch> {
        if limit == 0 {
            return Vec::new();
        }

        let mut limited = self.clone();
        limited.max_matches = match self.max_matches {
            0 => limit,
            configured => configured.min(limit),
        };
        limited.find_matches(pattern, target)
    }

    fn contains_pattern(&self, pattern: &CodePropertyGraph, target: &CodePropertyGraph) -> bool {
        !self.find_matches_limited(pattern, target, 1).is_empty()
    }

    fn algorithm_name(&self) -> &str {
        "VF2"
    }
}

impl Vf2Matcher {
    /// Stack-safe depth-first VF2 search.
    ///
    /// Each [`Vf2SearchFrame`] is the defunctionalized continuation of one
    /// recursive search activation: its pattern index and terminal/full-set
    /// selector fix the candidate domain, while `next_target` is the suspended
    /// loop cursor. A non-root frame owns exactly one mapping, which is undone
    /// when that frame is exhausted. Complete mappings are emitted and undone
    /// immediately.
    fn vf2_search(&self, state: &mut Vf2State, matches: &mut Vec<PatternMatch>) {
        if self.max_matches > 0 && matches.len() >= self.max_matches {
            return;
        }
        if state.is_complete() {
            matches.push(state.to_pattern_match());
            return;
        }

        let mut search = Vec::with_capacity(state.pattern_topology.nodes.len());
        search.push(Vf2SearchFrame::root(state));
        while !search.is_empty() {
            if self.max_matches > 0 && matches.len() >= self.max_matches {
                break;
            }

            let candidate = {
                let frame = search.last_mut().expect("search stack is non-empty");
                state.next_candidate(frame)
            };

            let Some((pattern_index, target_index)) = candidate else {
                let frame = search.pop().expect("search stack is non-empty");
                if frame.owns_mapping {
                    state.pop_mapping();
                }
                continue;
            };

            if !self.is_feasible(state, pattern_index, target_index) {
                continue;
            }

            state.push_mapping_indices(pattern_index, target_index);
            if state.is_complete() {
                matches.push(state.to_pattern_match());
                state.pop_mapping();
            } else {
                search.push(Vf2SearchFrame::child(state));
            }
        }

        // A match cap can stop the search with live child frames. Restore the
        // caller-owned state exactly as recursive returns would have done.
        for frame in search.into_iter().rev() {
            if frame.owns_mapping {
                state.pop_mapping();
            }
        }
    }

    /// Checks if adding a mapping is feasible.
    fn is_feasible(&self, state: &Vf2State, pattern_index: usize, target_index: usize) -> bool {
        let (pattern_kind, target_kind) = state.node_kinds(pattern_index, target_index);
        if !self.nodes_compatible(pattern_kind, target_kind) {
            return false;
        }

        // Every distinct predecessor/successor needs a distinct image. This
        // label-agnostic degree rule is a necessary condition for the existing
        // non-induced monomorphism semantics, including parallel edges, and is
        // therefore a sound VF2 look-ahead prune rather than a heuristic veto.
        if state.pattern_topology.out_degree[pattern_index]
            > state.target_topology.out_degree[target_index]
            || state.pattern_topology.in_degree[pattern_index]
                > state.target_topology.in_degree[target_index]
        {
            return false;
        }

        let pattern_node = state.pattern_topology.nodes[pattern_index];
        let target_node = state.target_topology.nodes[target_index];

        // Check edge consistency with existing mappings
        // For each pattern edge involving pattern_node, there must be
        // a compatible edge in target involving target_node

        // Check outgoing edges. A self-loop (`p_edge.target == pattern_node`)
        // references the node being mapped *right now*, which is not yet present
        // in `mapping`; resolve it to `target_node` directly so the loop must be
        // mirrored by a self-loop on `target_node`. Without this, pattern
        // self-loops were never verified and could match a target node lacking
        // one, yielding a structurally invalid embedding.
        for p_edge in state.pattern.outgoing_edges(pattern_node) {
            let mapped_target = if p_edge.target == pattern_node {
                Some(target_node)
            } else {
                state.mapped_target(p_edge.target)
            };
            if let Some(mapped_target) = mapped_target {
                // There must be a compatible edge in the target
                let has_compatible_edge = state
                    .target
                    .edges_between(target_node, mapped_target)
                    .iter()
                    .any(|e| self.edges_compatible(&p_edge.kind, &e.kind));

                if !has_compatible_edge {
                    return false;
                }
            }
        }

        // Check incoming edges. Mirror the self-loop handling above (a self-loop
        // appears in both the incoming and outgoing edge lists, so this is a
        // harmless redundant re-check for that case and the essential check for
        // ordinary incoming edges whose source is already mapped).
        for p_edge in state.pattern.incoming_edges(pattern_node) {
            let mapped_source = if p_edge.source == pattern_node {
                Some(target_node)
            } else {
                state.mapped_target(p_edge.source)
            };
            if let Some(mapped_source) = mapped_source {
                // There must be a compatible edge in the target
                let has_compatible_edge = state
                    .target
                    .edges_between(mapped_source, target_node)
                    .iter()
                    .any(|e| self.edges_compatible(&p_edge.kind, &e.kind));

                if !has_compatible_edge {
                    return false;
                }
            }
        }

        // All feasibility checks passed
        true
    }
}

/// One suspended candidate loop in the iterative VF2 depth-first search.
#[derive(Debug)]
struct Vf2SearchFrame {
    pattern_index: u32,
    next_target: u32,
    terminal_only: bool,
    owns_mapping: bool,
}

impl Vf2SearchFrame {
    fn root(state: &Vf2State<'_>) -> Self {
        Self::new(state, false)
    }

    fn child(state: &Vf2State<'_>) -> Self {
        Self::new(state, true)
    }

    fn new(state: &Vf2State<'_>, owns_mapping: bool) -> Self {
        let terminal_only = !state.pattern_terminal.is_empty() && !state.target_terminal.is_empty();
        let pattern_index = if terminal_only {
            state.pattern_terminal.first()
        } else {
            state.unmapped_pattern.first()
        }
        .expect("an incomplete VF2 state has an unmapped pattern node");

        Self {
            pattern_index: u32::try_from(pattern_index)
                .expect("pattern index fits the dense node domain"),
            next_target: 0,
            terminal_only,
            owns_mapping,
        }
    }
}

/// A dense ordered set backed by a Fenwick tree.
///
/// Membership is one bit per dense node. The Fenwick tree supplies the first
/// member and the next member at or after a cursor in logarithmic time without
/// allocating tree nodes or scanning an ever-longer prefix on deep paths.
#[derive(Debug)]
struct DenseSet {
    present: Vec<bool>,
    /// Empty sets store present bits; full sets store absent bits. The latter
    /// keeps construction O(1) in touched pages instead of filling every
    /// Fenwick cell before the first candidate is read.
    inverted: bool,
    fenwick: Vec<u32>,
    count: usize,
    select_step: usize,
}

impl DenseSet {
    fn empty(length: usize) -> Self {
        assert!(
            length < u32::MAX as usize,
            "VF2 dense node domain must leave one u32 sentinel"
        );
        Self {
            present: vec![false; length],
            inverted: false,
            fenwick: vec![0; length + 1],
            count: 0,
            select_step: if length == 0 {
                0
            } else {
                1usize << length.ilog2()
            },
        }
    }

    fn full(length: usize) -> Self {
        let mut set = Self::empty(length);
        set.present.fill(true);
        set.inverted = true;
        set.count = length;
        set
    }

    fn is_empty(&self) -> bool {
        self.count == 0
    }

    fn len(&self) -> usize {
        self.count
    }

    fn contains(&self, index: usize) -> bool {
        self.present.get(index).is_some_and(|present| *present)
    }

    fn insert(&mut self, index: usize) -> bool {
        assert!(index < self.present.len(), "dense-set index is in bounds");
        if self.present[index] {
            return false;
        }
        self.present[index] = true;
        self.count += 1;
        self.update_stored(index, !self.inverted);
        true
    }

    fn remove(&mut self, index: usize) -> bool {
        assert!(index < self.present.len(), "dense-set index is in bounds");
        if !self.present[index] {
            return false;
        }
        self.present[index] = false;
        self.count -= 1;
        self.update_stored(index, self.inverted);
        true
    }

    fn update_stored(&mut self, index: usize, adding: bool) {
        let mut tree_index = index + 1;
        while tree_index < self.fenwick.len() {
            self.fenwick[tree_index] = if adding {
                self.fenwick[tree_index]
                    .checked_add(1)
                    .expect("Fenwick population fits the u32 dense node domain")
            } else {
                self.fenwick[tree_index]
                    .checked_sub(1)
                    .expect("Fenwick removal mirrors an insertion")
            };
            tree_index += tree_index & tree_index.wrapping_neg();
        }
    }

    fn prefix_count(&self, end: usize) -> usize {
        let mut tree_index = end.min(self.present.len());
        let mut count = 0u32;
        while tree_index > 0 {
            count += self.fenwick[tree_index];
            tree_index &= tree_index - 1;
        }
        if self.inverted {
            end.min(self.present.len()) - count as usize
        } else {
            count as usize
        }
    }

    fn select(&self, rank: usize) -> Option<usize> {
        if rank >= self.count {
            return None;
        }
        let mut remaining = u32::try_from(rank).expect("rank fits the dense node domain");
        let mut index = 0usize;
        let mut step = self.select_step;
        while step > 0 {
            let next = index + step;
            if next < self.fenwick.len() {
                let stored = self.fenwick[next];
                let population = if self.inverted {
                    u32::try_from(next & next.wrapping_neg())
                        .expect("a Fenwick span fits the u32 dense node domain")
                        - stored
                } else {
                    stored
                };
                if population <= remaining {
                    index = next;
                    remaining -= population;
                }
            }
            step >>= 1;
        }
        debug_assert!(index < self.present.len() && self.present[index]);
        Some(index)
    }

    fn first(&self) -> Option<usize> {
        self.select(0)
    }

    fn next_at_or_after(&self, start: usize) -> Option<usize> {
        if start >= self.present.len() {
            return None;
        }
        self.select(self.prefix_count(start))
    }
}

/// Immutable graph data aligned by ascending stable [`NodeId`].
#[derive(Debug, Clone, Copy)]
struct Vf2Topology<'a> {
    nodes: &'a [NodeId],
    out_degree: &'a [u32],
    in_degree: &'a [u32],
}

impl<'a> Vf2Topology<'a> {
    fn new(graph: &'a CodePropertyGraph) -> Self {
        let dense = graph.dense_node_view();
        Self {
            nodes: dense.ids,
            out_degree: dense.unique_out_degree,
            in_degree: dense.unique_in_degree,
        }
    }

    fn index_of(&self, node: NodeId) -> Option<usize> {
        self.nodes.binary_search(&node).ok()
    }
}

/// Exact inverse information for one dense mapping push.
#[derive(Debug)]
struct Vf2UndoFrame {
    pattern_index: u32,
    target_index: u32,
    pattern_delta_start: u32,
    target_delta_start: u32,
}

/// VF2 algorithm state.
///
/// All hot membership and mapping structures are dense vectors indexed by the
/// immutable ascending-`NodeId` projection. Ordered candidate lookup uses
/// allocation-free Fenwick cursors, while flat delta logs restore terminal
/// counts exactly on backtracking.
#[derive(Debug)]
pub struct Vf2State<'a> {
    pattern: &'a CodePropertyGraph,
    target: &'a CodePropertyGraph,
    pattern_topology: Vf2Topology<'a>,
    target_topology: Vf2Topology<'a>,
    mapping: Vec<u32>,
    reverse_mapping: Vec<u32>,
    unmapped_pattern: DenseSet,
    unused_target: DenseSet,
    pattern_terminal: DenseSet,
    target_terminal: DenseSet,
    pattern_terminal_count: Vec<u32>,
    target_terminal_count: Vec<u32>,
    pattern_seen: Vec<bool>,
    target_seen: Vec<bool>,
    pattern_terminal_delta: Vec<u32>,
    target_terminal_delta: Vec<u32>,
    order: Vec<Vf2UndoFrame>,
}

impl<'a> Vf2State<'a> {
    /// Creates a new, empty partial mapping over deterministic dense views.
    pub fn new(pattern: &'a CodePropertyGraph, target: &'a CodePropertyGraph) -> Self {
        let pattern_topology = Vf2Topology::new(pattern);
        let target_topology = Vf2Topology::new(target);
        let pattern_len = pattern_topology.nodes.len();
        let target_len = target_topology.nodes.len();

        Self {
            pattern,
            target,
            pattern_topology,
            target_topology,
            mapping: vec![UNMAPPED; pattern_len],
            reverse_mapping: vec![UNMAPPED; target_len],
            unmapped_pattern: DenseSet::full(pattern_len),
            unused_target: DenseSet::full(target_len),
            pattern_terminal: DenseSet::empty(pattern_len),
            target_terminal: DenseSet::empty(target_len),
            pattern_terminal_count: vec![0; pattern_len],
            target_terminal_count: vec![0; target_len],
            pattern_seen: vec![false; pattern_len],
            target_seen: vec![false; target_len],
            pattern_terminal_delta: Vec::with_capacity(pattern_len),
            target_terminal_delta: Vec::with_capacity(pattern_len.min(target_len)),
            order: Vec::with_capacity(pattern_len),
        }
    }

    /// Returns true if every pattern node is mapped.
    pub fn is_complete(&self) -> bool {
        self.unmapped_pattern.is_empty()
    }

    /// Returns the canonical candidate pairs for the next mapping.
    pub fn candidate_pairs(&self) -> Vec<(NodeId, NodeId)> {
        if self.is_complete() {
            return Vec::new();
        }
        let mut frame = Vf2SearchFrame::root(self);
        let mut candidates = Vec::with_capacity(if frame.terminal_only {
            self.target_terminal.len()
        } else {
            self.unused_target.len()
        });
        while let Some((pattern_index, target_index)) = self.next_candidate(&mut frame) {
            candidates.push((
                self.pattern_topology.nodes[pattern_index],
                self.target_topology.nodes[target_index],
            ));
        }
        candidates
    }

    fn next_candidate(&self, frame: &mut Vf2SearchFrame) -> Option<(usize, usize)> {
        let targets = if frame.terminal_only {
            &self.target_terminal
        } else {
            &self.unused_target
        };
        let target_index = targets.next_at_or_after(frame.next_target as usize)?;
        debug_assert!(targets.contains(target_index));
        frame.next_target =
            u32::try_from(target_index + 1).expect("target cursor fits the dense node domain");
        Some((frame.pattern_index as usize, target_index))
    }

    fn node_kinds(
        &self,
        pattern_index: usize,
        target_index: usize,
    ) -> (&CpgNodeKind, &CpgNodeKind) {
        let pattern = self
            .pattern
            .node(self.pattern_topology.nodes[pattern_index])
            .expect("a dense pattern node resolves");
        let target = self
            .target
            .node(self.target_topology.nodes[target_index])
            .expect("a dense target node resolves");
        (&pattern.kind, &target.kind)
    }

    fn mapped_target(&self, pattern_node: NodeId) -> Option<NodeId> {
        let pattern_index = self.pattern_topology.index_of(pattern_node)?;
        let target_index = self.mapping[pattern_index];
        (target_index != UNMAPPED).then(|| self.target_topology.nodes[target_index as usize])
    }

    /// Adds a valid unused node pair to the partial mapping.
    ///
    /// Invalid identifiers, a repeated pattern node, or a reused target node
    /// indicate caller corruption and fail immediately instead of silently
    /// producing a non-injective state.
    pub fn push_mapping(&mut self, pattern_node: NodeId, target_node: NodeId) {
        let pattern_index = self
            .pattern_topology
            .index_of(pattern_node)
            .expect("a pushed pattern node belongs to the pattern graph");
        let target_index = self
            .target_topology
            .index_of(target_node)
            .expect("a pushed target node belongs to the target graph");
        self.push_mapping_indices(pattern_index, target_index);
    }

    fn push_mapping_indices(&mut self, pattern_index: usize, target_index: usize) {
        assert!(
            self.mapping[pattern_index] == UNMAPPED,
            "a pattern node may be mapped only once"
        );
        assert!(
            self.reverse_mapping[target_index] == UNMAPPED,
            "a target node may be used only once"
        );

        self.mapping[pattern_index] =
            u32::try_from(target_index).expect("target index fits the dense node domain");
        self.reverse_mapping[target_index] =
            u32::try_from(pattern_index).expect("pattern index fits the dense node domain");
        assert!(self.unmapped_pattern.remove(pattern_index));
        assert!(self.unused_target.remove(target_index));
        self.pattern_terminal.remove(pattern_index);
        self.target_terminal.remove(target_index);

        let pattern_delta_start = u32::try_from(self.pattern_terminal_delta.len())
            .expect("pattern terminal delta fits the edge domain");
        extend_terminal(
            self.pattern,
            self.pattern_topology.nodes,
            pattern_index,
            &self.mapping,
            &mut self.pattern_terminal_count,
            &mut self.pattern_terminal,
            &mut self.pattern_seen,
            &mut self.pattern_terminal_delta,
        );
        let target_delta_start = u32::try_from(self.target_terminal_delta.len())
            .expect("target terminal delta fits the edge domain");
        extend_terminal(
            self.target,
            self.target_topology.nodes,
            target_index,
            &self.reverse_mapping,
            &mut self.target_terminal_count,
            &mut self.target_terminal,
            &mut self.target_seen,
            &mut self.target_terminal_delta,
        );

        self.order.push(Vf2UndoFrame {
            pattern_index: u32::try_from(pattern_index)
                .expect("pattern index fits the dense node domain"),
            target_index: u32::try_from(target_index)
                .expect("target index fits the dense node domain"),
            pattern_delta_start,
            target_delta_start,
        });
    }

    /// Removes the last-pushed mapping and restores the exact preceding state.
    pub fn pop_mapping(&mut self) {
        let Some(frame) = self.order.pop() else {
            return;
        };

        undo_terminal(
            frame.pattern_delta_start as usize,
            &mut self.pattern_terminal_count,
            &mut self.pattern_terminal,
            &mut self.pattern_terminal_delta,
        );
        undo_terminal(
            frame.target_delta_start as usize,
            &mut self.target_terminal_count,
            &mut self.target_terminal,
            &mut self.target_terminal_delta,
        );

        let pattern_index = frame.pattern_index as usize;
        let target_index = frame.target_index as usize;
        self.mapping[pattern_index] = UNMAPPED;
        self.reverse_mapping[target_index] = UNMAPPED;
        assert!(self.unmapped_pattern.insert(pattern_index));
        assert!(self.unused_target.insert(target_index));
        if self.pattern_terminal_count[pattern_index] > 0 {
            self.pattern_terminal.insert(pattern_index);
        }
        if self.target_terminal_count[target_index] > 0 {
            self.target_terminal.insert(target_index);
        }
    }

    /// Converts the current complete state to a public match value.
    pub fn to_pattern_match(&self) -> PatternMatch {
        let root = self
            .mapping
            .iter()
            .find(|&&target| target != UNMAPPED)
            .map_or(NodeId::new(0), |&target| {
                self.target_topology.nodes[target as usize]
            });
        let mut result = PatternMatch::new("VF2Match", root, 1.0);
        for (pattern_index, target_index) in self.mapping.iter().enumerate() {
            if *target_index != UNMAPPED {
                result.node_mapping.insert(
                    self.pattern_topology.nodes[pattern_index],
                    self.target_topology.nodes[*target_index as usize],
                );
            }
        }
        result
    }
}

#[allow(clippy::too_many_arguments)]
fn extend_terminal(
    graph: &CodePropertyGraph,
    nodes: &[NodeId],
    mapped: usize,
    mapping: &[u32],
    terminal_count: &mut [u32],
    terminal: &mut DenseSet,
    seen: &mut [bool],
    delta: &mut Vec<u32>,
) {
    let start = delta.len();
    let node = nodes[mapped];
    let neighbors = graph
        .outgoing_edges(node)
        .map(|edge| edge.target)
        .chain(graph.incoming_edges(node).map(|edge| edge.source));
    for neighbor in neighbors {
        let neighbor_index = nodes
            .binary_search(&neighbor)
            .expect("an edge endpoint belongs to its graph");
        if mapping[neighbor_index] == UNMAPPED && !seen[neighbor_index] {
            seen[neighbor_index] = true;
            delta.push(
                u32::try_from(neighbor_index).expect("neighbor index fits the dense node domain"),
            );
            terminal_count[neighbor_index] = terminal_count[neighbor_index]
                .checked_add(1)
                .expect("a terminal adjacency count fits u32");
            if terminal_count[neighbor_index] == 1 {
                terminal.insert(neighbor_index);
            }
        }
    }
    for &neighbor in &delta[start..] {
        seen[neighbor as usize] = false;
    }
}

fn undo_terminal(
    start: usize,
    terminal_count: &mut [u32],
    terminal: &mut DenseSet,
    delta: &mut Vec<u32>,
) {
    for &neighbor in &delta[start..] {
        let neighbor = neighbor as usize;
        terminal_count[neighbor] = terminal_count[neighbor]
            .checked_sub(1)
            .expect("terminal undo mirrors a mapping push");
        if terminal_count[neighbor] == 0 {
            terminal.remove(neighbor);
        }
    }
    delta.truncate(start);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CpgNode, CpgNodeKind, Language, SourceRange};

    #[test]
    fn test_vf2_matcher_creation() {
        let matcher = Vf2Matcher::new()
            .with_strict_kinds(true)
            .with_strict_edges(false)
            .with_max_matches(10);

        assert!(matcher.strict_kinds);
        assert!(!matcher.strict_edges);
        assert_eq!(matcher.max_matches, 10);
    }

    #[test]
    fn test_empty_pattern() {
        let matcher = Vf2Matcher::new();
        let pattern = CodePropertyGraph::new(Language::Rust);
        let target = CodePropertyGraph::new(Language::Rust);

        let matches = matcher.find_matches(&pattern, &target);
        assert!(matches.is_empty());
    }

    #[test]
    fn test_single_node_match() {
        let matcher = Vf2Matcher::new().with_strict_kinds(true);

        let mut pattern = CodePropertyGraph::new(Language::Rust);
        pattern.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::If,
            SourceRange::default(),
        ));

        let mut target = CodePropertyGraph::new(Language::Rust);
        target.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::If,
            SourceRange::default(),
        ));
        target.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::While,
            SourceRange::default(),
        ));

        let matches = matcher.find_matches(&pattern, &target);
        assert_eq!(matches.len(), 1);
    }

    /// Regression test for the `pop_mapping` backtracking bug.
    ///
    /// A 3-node directed path pattern `p0 -> p1 -> p2` is matched against a
    /// *diamond* target `t0 -> {t1, t2} -> t3`. It has exactly TWO monomorphic
    /// embeddings — `{t0,t1,t3}` and `{t0,t2,t3}` — that diverge at the MIDDLE
    /// node: after mapping `p0 -> t0`, the middle node `p1` has two candidate
    /// targets (`t1` and `t2`). Finding the second embedding therefore requires
    /// the DFS to backtrack *partially* out of the first (pop only `p2` and
    /// `p1`, retain `p0 -> t0`) and retry `p1 -> t2`.
    ///
    /// This is precisely what the old implementation could not do: `pop_mapping`
    /// removed an arbitrary `FxHashMap` entry (via `mapping.iter().last()`)
    /// rather than the last-pushed pair, and never restored the terminal sets —
    /// so the partial backtrack corrupted the state and dropped an embedding.
    /// (A straight path target masks the bug: every node is mapped before any
    /// backtrack, so N arbitrary pops empty the mapping just like N correct
    /// pops. The diamond forces a mid-search retry that exposes it.)
    #[test]
    fn test_multi_embedding_backtracking() {
        // All nodes share a kind so relaxed matching never rejects a pair and
        // the graph *structure* alone determines the matches.
        fn node(g: &mut CodePropertyGraph) -> NodeId {
            g.add_node(CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::If,
                SourceRange::default(),
            ))
        }

        // Pattern: path p0 -> p1 -> p2.
        let mut pattern = CodePropertyGraph::new(Language::Rust);
        let p: Vec<NodeId> = (0..3).map(|_| node(&mut pattern)).collect();
        pattern.connect(p[0], p[1], CpgEdgeKind::AstChild);
        pattern.connect(p[1], p[2], CpgEdgeKind::AstChild);

        // Target: diamond t0 -> {t1, t2} -> t3.
        let mut target = CodePropertyGraph::new(Language::Rust);
        let t: Vec<NodeId> = (0..4).map(|_| node(&mut target)).collect();
        target.connect(t[0], t[1], CpgEdgeKind::AstChild);
        target.connect(t[0], t[2], CpgEdgeKind::AstChild);
        target.connect(t[1], t[3], CpgEdgeKind::AstChild);
        target.connect(t[2], t[3], CpgEdgeKind::AstChild);

        let matches = Vf2Matcher::new().find_matches(&pattern, &target);

        // Exactly the two embeddings, no more, no fewer.
        assert_eq!(
            matches.len(),
            2,
            "expected both diamond embeddings, got {}",
            matches.len()
        );

        // Normalize each match to sorted (pattern_id -> target_id) tuples.
        let embeddings: FxHashSet<Vec<(u32, u32)>> = matches
            .iter()
            .map(|m| {
                let mut pairs: Vec<(u32, u32)> = m
                    .node_mapping
                    .iter()
                    .map(|(pk, tv)| (pk.as_u32(), tv.as_u32()))
                    .collect();
                pairs.sort_unstable();
                pairs
            })
            .collect();

        let via_t1 = vec![
            (p[0].as_u32(), t[0].as_u32()),
            (p[1].as_u32(), t[1].as_u32()),
            (p[2].as_u32(), t[3].as_u32()),
        ];
        let via_t2 = vec![
            (p[0].as_u32(), t[0].as_u32()),
            (p[1].as_u32(), t[2].as_u32()),
            (p[2].as_u32(), t[3].as_u32()),
        ];

        assert!(
            embeddings.contains(&via_t1),
            "missing embedding via t1; found {:?}",
            embeddings
        );
        assert!(
            embeddings.contains(&via_t2),
            "missing embedding via t2; found {:?}",
            embeddings
        );
    }

    // ==================== added coverage: example-based ====================

    #[test]
    fn test_algorithm_name() {
        assert_eq!(Vf2Matcher::new().algorithm_name(), "VF2");
    }

    /// Relaxed matching (same *category*) finds at least as many embeddings as
    /// strict matching (exact kind) on a same-category target.
    #[test]
    fn test_relaxed_finds_at_least_strict() {
        use crate::{MethodSignature, Visibility};
        use std::sync::Arc;

        // pattern: a single `Class` node.
        let mut pattern = CodePropertyGraph::new(Language::Rust);
        pattern.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Class {
                name: Arc::from("C"),
                is_abstract: false,
            },
            SourceRange::default(),
        ));

        // target: three *declaration*-category nodes of different exact kinds.
        let mut target = CodePropertyGraph::new(Language::Rust);
        target.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Class {
                name: Arc::from("A"),
                is_abstract: false,
            },
            SourceRange::default(),
        ));
        target.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Struct {
                name: Arc::from("B"),
            },
            SourceRange::default(),
        ));
        target.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Function {
                signature: Box::new(MethodSignature {
                    name: "f".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Public,
                }),
            },
            SourceRange::default(),
        ));

        let strict = Vf2Matcher::new().with_strict_kinds(true);
        let relaxed = Vf2Matcher::new().with_strict_kinds(false);

        let strict_n = strict.find_matches(&pattern, &target).len();
        let relaxed_n = relaxed.find_matches(&pattern, &target).len();

        assert_eq!(strict_n, 1, "strict matches only the exact Class node");
        assert_eq!(relaxed_n, 3, "relaxed matches every declaration node");
        assert!(relaxed_n >= strict_n);
    }

    #[test]
    fn test_max_matches_caps_results() {
        // pattern: single `If` node.
        let mut pattern = CodePropertyGraph::new(Language::Rust);
        pattern.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::If,
            SourceRange::default(),
        ));

        // target: four `If` nodes -> four unconstrained matches.
        let mut target = CodePropertyGraph::new(Language::Rust);
        for _ in 0..4 {
            target.add_node(CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::If,
                SourceRange::default(),
            ));
        }

        assert_eq!(Vf2Matcher::new().find_matches(&pattern, &target).len(), 4);
        assert_eq!(
            Vf2Matcher::new()
                .with_max_matches(2)
                .find_matches(&pattern, &target)
                .len(),
            2
        );
        assert_eq!(
            Vf2Matcher::new()
                .with_max_matches(1)
                .find_matches(&pattern, &target)
                .len(),
            1
        );
    }

    #[test]
    fn test_returned_mapping_is_injective() {
        // pattern: two `If` nodes joined by an AST edge.
        let mut pattern = CodePropertyGraph::new(Language::Rust);
        let p0 = pattern.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::If,
            SourceRange::default(),
        ));
        let p1 = pattern.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::If,
            SourceRange::default(),
        ));
        pattern.connect(p0, p1, CpgEdgeKind::AstChild);

        // target: a 3-node AST path of `If` nodes.
        let mut target = CodePropertyGraph::new(Language::Rust);
        let t: Vec<NodeId> = (0..3)
            .map(|_| {
                target.add_node(CpgNode::new(
                    NodeId::new(0),
                    CpgNodeKind::If,
                    SourceRange::default(),
                ))
            })
            .collect();
        target.connect(t[0], t[1], CpgEdgeKind::AstChild);
        target.connect(t[1], t[2], CpgEdgeKind::AstChild);

        let matches = Vf2Matcher::new().find_matches(&pattern, &target);
        assert!(!matches.is_empty());
        for m in &matches {
            let images: FxHashSet<NodeId> = m.node_mapping.values().copied().collect();
            assert_eq!(
                images.len(),
                m.node_mapping.len(),
                "mapping is not injective: {:?}",
                m.node_mapping
            );
            assert_eq!(m.node_mapping.len(), 2, "match is complete");
            assert!(m.node_mapping.contains_key(&p0));
            assert!(m.node_mapping.contains_key(&p1));
        }
    }

    /// Regression test for the `is_feasible` self-loop hole: a pattern node with
    /// a self-loop must only match a target node that also carries a compatible
    /// self-loop.
    #[test]
    fn test_self_loop_must_be_mirrored() {
        // pattern: single node with a self-loop.
        let mut pattern = CodePropertyGraph::new(Language::Rust);
        let p = pattern.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::If,
            SourceRange::default(),
        ));
        pattern.connect(p, p, CpgEdgeKind::AstChild);

        // target A: a matching-kind node WITHOUT a self-loop -> no match.
        let mut no_loop = CodePropertyGraph::new(Language::Rust);
        no_loop.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::If,
            SourceRange::default(),
        ));
        assert!(
            Vf2Matcher::new()
                .find_matches(&pattern, &no_loop)
                .is_empty(),
            "self-loop pattern must not match a target node lacking a self-loop"
        );

        // target B: a node WITH a self-loop -> exactly one match onto it.
        let mut with_loop = CodePropertyGraph::new(Language::Rust);
        let t = with_loop.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::If,
            SourceRange::default(),
        ));
        with_loop.connect(t, t, CpgEdgeKind::AstChild);
        let matches = Vf2Matcher::new().find_matches(&pattern, &with_loop);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].node_mapping.get(&p), Some(&t));
    }

    #[test]
    fn parallel_pattern_edges_keep_existential_monomorphism_semantics() {
        fn if_node(graph: &mut CodePropertyGraph) -> NodeId {
            graph.add_node(CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::If,
                SourceRange::default(),
            ))
        }

        let mut pattern = CodePropertyGraph::new(Language::Rust);
        let p0 = if_node(&mut pattern);
        let p1 = if_node(&mut pattern);
        pattern.connect(p0, p1, CpgEdgeKind::AstChild);
        pattern.connect(p0, p1, CpgEdgeKind::AstChild);

        let mut target = CodePropertyGraph::new(Language::Rust);
        let t0 = if_node(&mut target);
        let t1 = if_node(&mut target);
        target.connect(t0, t1, CpgEdgeKind::AstChild);

        let matches = Vf2Matcher::new()
            .with_strict_kinds(true)
            .with_strict_edges(true)
            .find_matches(&pattern, &target);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].node_mapping.get(&p0), Some(&t0));
        assert_eq!(matches[0].node_mapping.get(&p1), Some(&t1));
    }

    #[test]
    fn non_induced_matching_allows_extra_target_edges_but_preserves_direction() {
        fn if_node(graph: &mut CodePropertyGraph) -> NodeId {
            graph.add_node(CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::If,
                SourceRange::default(),
            ))
        }

        let mut pattern = CodePropertyGraph::new(Language::Rust);
        let p0 = if_node(&mut pattern);
        let p1 = if_node(&mut pattern);
        pattern.connect(p0, p1, CpgEdgeKind::AstChild);

        let mut one_way = CodePropertyGraph::new(Language::Rust);
        let a = if_node(&mut one_way);
        let b = if_node(&mut one_way);
        one_way.connect(a, b, CpgEdgeKind::AstChild);
        let directed = Vf2Matcher::new()
            .with_strict_kinds(true)
            .with_strict_edges(true)
            .find_matches(&pattern, &one_way);
        assert_eq!(directed.len(), 1, "the edge direction selects one mapping");

        one_way.connect(b, a, CpgEdgeKind::ControlDependence);
        let extra_incompatible = Vf2Matcher::new()
            .with_strict_kinds(true)
            .with_strict_edges(true)
            .find_matches(&pattern, &one_way);
        assert_eq!(
            embedding_pairs(&extra_incompatible[0]),
            embedding_pairs(&directed[0]),
            "an extra reverse edge of another kind does not veto a non-induced match"
        );

        fn embedding_pairs(found: &PatternMatch) -> Vec<(NodeId, NodeId)> {
            let mut pairs: Vec<_> = found
                .node_mapping
                .iter()
                .map(|(&pattern, &target)| (pattern, target))
                .collect();
            pairs.sort_unstable();
            pairs
        }
    }

    #[test]
    fn disconnected_pattern_enumerates_every_injection() {
        let mut pattern = CodePropertyGraph::new(Language::Rust);
        for _ in 0..2 {
            pattern.add_node(CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::If,
                SourceRange::default(),
            ));
        }
        let mut target = CodePropertyGraph::new(Language::Rust);
        for _ in 0..3 {
            target.add_node(CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::If,
                SourceRange::default(),
            ));
        }

        let matches = Vf2Matcher::new()
            .with_strict_kinds(true)
            .with_strict_edges(true)
            .find_matches(&pattern, &target);
        assert_eq!(matches.len(), 3 * 2);
    }

    #[test]
    fn dense_set_selects_members_in_order_across_boundaries() {
        let mut set = DenseSet::empty(130);
        for index in [129, 0, 64, 63, 65] {
            assert!(set.insert(index));
        }
        assert_eq!(set.len(), 5);
        assert_eq!(set.first(), Some(0));
        assert_eq!(set.next_at_or_after(1), Some(63));
        assert_eq!(set.next_at_or_after(64), Some(64));
        assert_eq!(set.next_at_or_after(66), Some(129));
        assert_eq!(set.next_at_or_after(130), None);
        assert!(set.remove(64));
        assert!(!set.remove(64));
        assert_eq!(set.next_at_or_after(64), Some(65));

        let full = DenseSet::full(130);
        assert_eq!(full.len(), 130);
        for index in 0..130 {
            assert_eq!(full.select(index), Some(index));
        }
    }

    #[test]
    fn search_and_undo_continuations_are_compact_heap_records() {
        use std::mem::size_of;

        assert!(size_of::<Vf2SearchFrame>() <= 16);
        assert!(size_of::<Vf2UndoFrame>() <= 16);
        assert_eq!(size_of::<u32>(), size_of::<NodeId>());
    }

    /// Regression test for the relaxed `edges_compatible` fix. Relaxed matching
    /// must stay a true *relaxation* of strict matching even for edge kinds with
    /// no coarse AST/CFG/DFG/call bucket (here a PDG `ControlDependence` edge).
    /// Before the fix, two identical `ControlDependence` edges were judged
    /// incompatible under relaxed matching, so the relaxed matcher found FEWER
    /// embeddings than the strict one — violating the superset guarantee.
    #[test]
    fn test_relaxed_matches_pdg_edges_like_strict() {
        fn two_block_pdg(kind: CpgEdgeKind) -> CodePropertyGraph {
            let mut g = CodePropertyGraph::new(Language::Rust);
            let a = g.add_node(CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::Block {
                    scope: crate::ScopeId::GLOBAL,
                },
                SourceRange::default(),
            ));
            let b = g.add_node(CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::Block {
                    scope: crate::ScopeId::GLOBAL,
                },
                SourceRange::default(),
            ));
            g.connect(a, b, kind);
            g
        }

        let pattern = two_block_pdg(CpgEdgeKind::ControlDependence);
        let target = two_block_pdg(CpgEdgeKind::ControlDependence);

        let strict = Vf2Matcher::new()
            .with_strict_kinds(true)
            .with_strict_edges(true);
        let relaxed = Vf2Matcher::new()
            .with_strict_kinds(false)
            .with_strict_edges(false);

        let strict_n = strict.find_matches(&pattern, &target).len();
        let relaxed_n = relaxed.find_matches(&pattern, &target).len();

        assert!(strict_n >= 1, "strict should find the identity embedding");
        assert!(
            relaxed_n >= strict_n,
            "relaxed ({relaxed_n}) must be a superset of strict ({strict_n}) for PDG edges"
        );
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::testutil::*;
    use crate::{CpgNode, SourceRange};
    use proptest::prelude::*;

    /// Bounded recursive reference equations retained only for shallow
    /// differential tests. Generated patterns contain at most three nodes, so
    /// this oracle cannot make test-stack usage depend on hostile input.
    fn recursive_search(
        matcher: &Vf2Matcher,
        state: &mut Vf2State<'_>,
        matches: &mut Vec<PatternMatch>,
    ) {
        if matcher.max_matches > 0 && matches.len() >= matcher.max_matches {
            return;
        }
        if state.is_complete() {
            matches.push(state.to_pattern_match());
            return;
        }
        for (pattern_node, target_node) in state.candidate_pairs() {
            let pattern_index = state
                .pattern_topology
                .index_of(pattern_node)
                .expect("candidate pattern node resolves");
            let target_index = state
                .target_topology
                .index_of(target_node)
                .expect("candidate target node resolves");
            if matcher.is_feasible(state, pattern_index, target_index) {
                state.push_mapping_indices(pattern_index, target_index);
                recursive_search(matcher, state, matches);
                state.pop_mapping();
                if matcher.max_matches > 0 && matches.len() >= matcher.max_matches {
                    return;
                }
            }
        }
    }

    fn recursive_matches(
        matcher: &Vf2Matcher,
        pattern: &CodePropertyGraph,
        target: &CodePropertyGraph,
    ) -> Vec<PatternMatch> {
        if pattern.node_count() == 0 || target.node_count() < pattern.node_count() {
            return Vec::new();
        }
        let mut state = Vf2State::new(pattern, target);
        let mut matches = Vec::new();
        recursive_search(matcher, &mut state, &mut matches);
        assert!(state.mapping.iter().all(|&target| target == UNMAPPED));
        assert!(state.order.is_empty());
        matches
    }

    /// The induced subgraph of `g` over its first `k` node ids (1..=k, clamped).
    /// Preserves original ids and every edge whose endpoints are both retained.
    fn small_pattern(g: &CodePropertyGraph, k: usize) -> CodePropertyGraph {
        let ids: Vec<NodeId> = g.node_ids().collect();
        let k = k.min(ids.len()).max(1);
        g.subgraph(&ids[..k])
    }

    /// Normalizes a match set to canonical sorted `(pattern_id, target_id)`
    /// embedding tuples, so two matchers' results can be compared as sets.
    fn embedding_set(matches: &[PatternMatch]) -> FxHashSet<Vec<(u32, u32)>> {
        matches
            .iter()
            .map(|m| {
                let mut v: Vec<(u32, u32)> = m
                    .node_mapping
                    .iter()
                    .map(|(p, t)| (p.as_u32(), t.as_u32()))
                    .collect();
                v.sort_unstable();
                v
            })
            .collect()
    }

    fn embedding_sequence(matches: &[PatternMatch]) -> Vec<Vec<(u32, u32)>> {
        matches
            .iter()
            .map(|m| {
                let mut mapping: Vec<_> = m
                    .node_mapping
                    .iter()
                    .map(|(pattern, target)| (pattern.as_u32(), target.as_u32()))
                    .collect();
                mapping.sort_unstable();
                mapping
            })
            .collect()
    }

    /// Independent exhaustive injection oracle for bounded generated graphs.
    /// It knows nothing about VF2 terminal sets, dense cursors, degree pruning,
    /// or undo logs: it enumerates every injective assignment iteratively and
    /// then applies only the public node/edge compatibility relation.
    fn exhaustive_embedding_set(
        matcher: &Vf2Matcher,
        pattern: &CodePropertyGraph,
        target: &CodePropertyGraph,
    ) -> FxHashSet<Vec<(u32, u32)>> {
        let mut pattern_nodes: Vec<_> = pattern.node_ids().collect();
        let mut target_nodes: Vec<_> = target.node_ids().collect();
        pattern_nodes.sort_unstable();
        target_nodes.sort_unstable();
        if pattern_nodes.is_empty() || target_nodes.len() < pattern_nodes.len() {
            return FxHashSet::default();
        }

        let mut result = FxHashSet::default();
        let mut assignment = vec![0usize; pattern_nodes.len()];
        let mut target_used = vec![false; target_nodes.len()];
        let mut next_target = vec![0usize; pattern_nodes.len()];
        let mut depth = 0usize;

        loop {
            if depth == pattern_nodes.len() {
                let preserves_edges = pattern.edges().all(|edge| {
                    let source = pattern_nodes
                        .binary_search(&edge.source)
                        .expect("oracle edge source exists");
                    let target_index = pattern_nodes
                        .binary_search(&edge.target)
                        .expect("oracle edge target exists");
                    target
                        .edges_between(
                            target_nodes[assignment[source]],
                            target_nodes[assignment[target_index]],
                        )
                        .any(|candidate| matcher.edges_compatible(&edge.kind, &candidate.kind))
                });
                if preserves_edges {
                    result.insert(
                        pattern_nodes
                            .iter()
                            .enumerate()
                            .map(|(index, pattern)| {
                                (pattern.as_u32(), target_nodes[assignment[index]].as_u32())
                            })
                            .collect(),
                    );
                }
                depth -= 1;
                target_used[assignment[depth]] = false;
                continue;
            }

            let mut choice = None;
            while next_target[depth] < target_nodes.len() {
                let candidate = next_target[depth];
                next_target[depth] += 1;
                if target_used[candidate] {
                    continue;
                }
                let pattern_kind = &pattern
                    .node(pattern_nodes[depth])
                    .expect("oracle pattern node exists")
                    .kind;
                let target_kind = &target
                    .node(target_nodes[candidate])
                    .expect("oracle target node exists")
                    .kind;
                if matcher.nodes_compatible(pattern_kind, target_kind) {
                    choice = Some(candidate);
                    break;
                }
            }

            if let Some(candidate) = choice {
                assignment[depth] = candidate;
                target_used[candidate] = true;
                depth += 1;
                if depth < pattern_nodes.len() {
                    next_target[depth] = 0;
                }
            } else {
                next_target[depth] = 0;
                if depth == 0 {
                    break;
                }
                depth -= 1;
                target_used[assignment[depth]] = false;
            }
        }
        result
    }

    fn reverse_insertion(graph: &CodePropertyGraph) -> CodePropertyGraph {
        let mut reversed = CodePropertyGraph::new(graph.language());
        let mut nodes: Vec<_> = graph.nodes().cloned().collect();
        nodes.reverse();
        for node in nodes {
            reversed.add_node_with_id(node);
        }
        let mut edges: Vec<_> = graph.edges().cloned().collect();
        edges.reverse();
        for edge in edges {
            reversed
                .add_edge_with_id(edge)
                .expect("reversed edge endpoints exist");
        }
        reversed
    }

    /// Checks the three structural VF2 invariants for a single returned match
    /// against the matcher that produced it: completeness+injectivity of the
    /// mapping, kind compatibility of every mapped pair, and preservation of
    /// every pattern edge by a compatible target edge.
    fn check_match_invariants(
        matcher: &Vf2Matcher,
        pattern: &CodePropertyGraph,
        target: &CodePropertyGraph,
        m: &PatternMatch,
    ) -> Result<(), TestCaseError> {
        // Completeness: every pattern node is mapped.
        prop_assert_eq!(m.node_mapping.len(), pattern.node_count());

        // Injectivity: the target images are pairwise distinct.
        let images: FxHashSet<NodeId> = m.node_mapping.values().copied().collect();
        prop_assert_eq!(images.len(), m.node_mapping.len());

        // Kind compatibility of every mapped pair (per this matcher's policy).
        for (&p, &t) in &m.node_mapping {
            let pk = &pattern.node(p).expect("mapped pattern node exists").kind;
            let tk = &target.node(t).expect("mapped target node exists").kind;
            prop_assert!(matcher.nodes_compatible(pk, tk));
        }

        // Edge preservation: each pattern edge has a compatible target edge
        // between the images of its endpoints.
        for e in pattern.edges() {
            let ts = *m.node_mapping.get(&e.source).expect("edge source mapped");
            let tt = *m.node_mapping.get(&e.target).expect("edge target mapped");
            let preserved = target
                .edges_between(ts, tt)
                .iter()
                .any(|te| matcher.edges_compatible(&e.kind, &te.kind));
            prop_assert!(
                preserved,
                "pattern edge {:?} not preserved by any target edge",
                e.kind
            );
        }
        Ok(())
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// Every embedding returned by VF2 (strict or relaxed) is a complete,
        /// injective, kind-compatible, edge-preserving map of the pattern into
        /// the target.
        #[test]
        fn prop_vf2_match_invariants(g in arb_cpg_raw(), k in 1usize..=3) {
            let pattern = small_pattern(&g, k);
            let matchers = [
                Vf2Matcher::new().with_max_matches(48),                                  // relaxed
                Vf2Matcher::new().with_strict_kinds(true).with_strict_edges(true).with_max_matches(48),
            ];
            for matcher in matchers {
                for m in &matcher.find_matches(&pattern, &g) {
                    check_match_invariants(&matcher, &pattern, &g, m)?;
                }
            }
        }

        /// The iterative driver is the exact defunctionalization of the former
        /// recursive search equations, including enumeration order and cap
        /// short-circuit position.
        #[test]
        fn prop_iterative_search_matches_bounded_recursive_oracle(
            g in arb_cpg_raw(),
            k in 1usize..=3,
            cap in 0usize..=8,
            strict_kinds in any::<bool>(),
            strict_edges in any::<bool>(),
        ) {
            let pattern = small_pattern(&g, k);
            let matcher = Vf2Matcher::new()
                .with_strict_kinds(strict_kinds)
                .with_strict_edges(strict_edges)
                .with_max_matches(cap);
            let expected = recursive_matches(&matcher, &pattern, &g);
            let actual = matcher.find_matches(&pattern, &g);
            prop_assert_eq!(embedding_sequence(&actual), embedding_sequence(&expected));
        }

        /// Hash-table insertion order cannot change the canonical DFS or which
        /// prefix a public match cap selects.
        #[test]
        fn prop_search_is_deterministic_under_graph_insertion_permutation(
            g in arb_cpg_raw(),
            k in 1usize..=3,
            cap in 1usize..=8,
        ) {
            let pattern = small_pattern(&g, k);
            let reversed_pattern = reverse_insertion(&pattern);
            let reversed = reverse_insertion(&g);
            let matcher = Vf2Matcher::new()
                .with_strict_kinds(true)
                .with_strict_edges(true)
                .with_max_matches(cap);
            let first = matcher.find_matches(&pattern, &g);
            let second = matcher.find_matches(&pattern, &reversed);
            let both_reversed = matcher.find_matches(&reversed_pattern, &reversed);
            let repeat = matcher.find_matches(&pattern, &g);
            prop_assert_eq!(embedding_sequence(&first), embedding_sequence(&second));
            prop_assert_eq!(embedding_sequence(&first), embedding_sequence(&both_reversed));
            prop_assert_eq!(embedding_sequence(&first), embedding_sequence(&repeat));
        }

        /// Dense order-statistics membership is extensionally equal to an
        /// independent ordered-set model after every generated mutation.
        #[test]
        fn prop_dense_set_matches_btree_model(
            length in 0usize..=128,
            operations in prop::collection::vec((any::<bool>(), any::<usize>()), 0..=256),
        ) {
            use std::collections::BTreeSet;

            let mut dense = DenseSet::empty(length);
            let mut model = BTreeSet::new();
            for (adding, raw_index) in operations {
                if length == 0 {
                    prop_assert!(dense.is_empty());
                    continue;
                }
                let index = raw_index % length;
                let dense_changed = if adding {
                    dense.insert(index)
                } else {
                    dense.remove(index)
                };
                let model_changed = if adding {
                    model.insert(index)
                } else {
                    model.remove(&index)
                };
                prop_assert_eq!(dense_changed, model_changed);
                prop_assert_eq!(dense.len(), model.len());
                prop_assert_eq!(dense.first(), model.first().copied());
                for start in 0..=length {
                    prop_assert_eq!(
                        dense.next_at_or_after(start),
                        model.range(start..).next().copied(),
                    );
                }
            }
        }

        /// Increasing a positive cap retains the entire prior canonical prefix.
        #[test]
        fn prop_match_cap_is_prefix_monotone(
            g in arb_cpg_raw(),
            k in 1usize..=2,
            lower in 1usize..=6,
            extra in 0usize..=6,
        ) {
            let pattern = small_pattern(&g, k);
            let lower_matches = Vf2Matcher::new()
                .with_max_matches(lower)
                .find_matches(&pattern, &g);
            let upper_matches = Vf2Matcher::new()
                .with_max_matches(lower + extra)
                .find_matches(&pattern, &g);
            let lower_sequence = embedding_sequence(&lower_matches);
            let upper_sequence = embedding_sequence(&upper_matches);
            prop_assert_eq!(
                lower_sequence.as_slice(),
                &upper_sequence[..lower_sequence.len()]
            );
        }

        /// The public convenience performs the bounded search itself rather
        /// than enumerating all embeddings and truncating afterward.
        #[test]
        fn prop_find_matches_limited_equals_effective_configured_cap(
            g in arb_cpg_raw(),
            k in 1usize..=3,
            configured_cap in 0usize..=8,
            requested_limit in 0usize..=8,
        ) {
            let pattern = small_pattern(&g, k);
            let matcher = Vf2Matcher::new().with_max_matches(configured_cap);
            let limited = matcher.find_matches_limited(&pattern, &g, requested_limit);
            let effective_cap = if requested_limit == 0 {
                0
            } else if configured_cap == 0 {
                requested_limit
            } else {
                configured_cap.min(requested_limit)
            };
            let expected = if effective_cap == 0 {
                Vec::new()
            } else {
                Vf2Matcher::new()
                    .with_max_matches(effective_cap)
                    .find_matches(&pattern, &g)
            };
            prop_assert_eq!(embedding_sequence(&limited), embedding_sequence(&expected));
        }

        /// Indexed parallel batch matching preserves both pattern order and
        /// each pattern's canonical embedding order exactly.
        #[test]
        fn prop_parallel_pattern_batch_is_exactly_serial(
            g in arb_cpg_raw(),
            sizes in prop::collection::vec(1usize..=4, 0..=16),
            strict_kinds in any::<bool>(),
            strict_edges in any::<bool>(),
            cap in 0usize..=8,
        ) {
            let patterns: Vec<_> = sizes
                .into_iter()
                .map(|size| small_pattern(&g, size))
                .collect();
            let matcher = Vf2Matcher::new()
                .with_strict_kinds(strict_kinds)
                .with_strict_edges(strict_edges)
                .with_max_matches(cap);
            let serial = matcher.find_matches_many(&patterns, &g);
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(4)
                .stack_size(256 * 1024)
                .build()
                .expect("bounded Rayon pool");
            let parallel = pool.install(|| matcher.find_matches_many_parallel(&patterns, &g));
            let serial_sequences: Vec<_> = serial.iter().map(|matches| embedding_sequence(matches)).collect();
            let parallel_sequences: Vec<_> = parallel
                .iter()
                .map(|matches| embedding_sequence(matches))
                .collect();
            prop_assert_eq!(parallel_sequences, serial_sequences);
        }

        /// The relaxed matcher's embedding set is a superset of the strict
        /// matcher's: strict compatibility implies relaxed compatibility for
        /// both nodes and edges, and candidate generation is identical.
        #[test]
        fn prop_relaxed_superset_of_strict(g in arb_cpg_raw(), k in 1usize..=2) {
            let pattern = small_pattern(&g, k);
            let strict = Vf2Matcher::new().with_strict_kinds(true).with_strict_edges(true);
            let relaxed = Vf2Matcher::new().with_strict_kinds(false).with_strict_edges(false);
            let strict_set = embedding_set(&strict.find_matches(&pattern, &g));
            let relaxed_set = embedding_set(&relaxed.find_matches(&pattern, &g));
            for e in &strict_set {
                prop_assert!(relaxed_set.contains(e), "relaxed matcher dropped strict embedding {:?}", e);
            }
        }

        /// `with_max_matches(k>0)` bounds the number of returned matches by `k`.
        #[test]
        fn prop_max_matches_caps(g in arb_cpg_raw(), k in 1usize..=2, cap in 1usize..=6) {
            let pattern = small_pattern(&g, k);
            let n = Vf2Matcher::new().with_max_matches(cap).find_matches(&pattern, &g).len();
            prop_assert!(n <= cap, "returned {} matches > cap {}", n, cap);
        }

        /// A non-empty graph always embeds into itself (identity), so strict
        /// self-matching is never empty.
        #[test]
        fn prop_strict_self_match_nonempty(g in arb_cpg_raw()) {
            let matcher = Vf2Matcher::new()
                .with_strict_kinds(true)
                .with_strict_edges(true)
                .with_max_matches(1);
            prop_assert!(!matcher.find_matches(&g, &g).is_empty());
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        /// For bounded random labelled directed multigraphs, dense VF2 returns
        /// exactly the exhaustive set of injections under both strictness axes.
        #[test]
        fn prop_dense_vf2_equals_exhaustive_injection_oracle(
            g in arb_cpg_raw(),
            k in 1usize..=4,
            strict_kinds in any::<bool>(),
            strict_edges in any::<bool>(),
        ) {
            let pattern = small_pattern(&g, k);
            let matcher = Vf2Matcher::new()
                .with_strict_kinds(strict_kinds)
                .with_strict_edges(strict_edges);
            let expected = exhaustive_embedding_set(&matcher, &pattern, &g);
            let actual = embedding_set(&matcher.find_matches(&pattern, &g));
            prop_assert_eq!(actual, expected);
        }

        /// Appending a target node that is strictly incompatible with every
        /// pattern node cannot change either the embedding set or its order.
        #[test]
        fn prop_incompatible_target_node_is_irrelevant(g in arb_cpg_raw(), k in 1usize..=4) {
            let pattern = small_pattern(&g, k);
            let matcher = Vf2Matcher::new()
                .with_strict_kinds(true)
                .with_strict_edges(true);
            let candidates = [
                CpgNodeKind::Root,
                CpgNodeKind::Return,
                CpgNodeKind::If,
                CpgNodeKind::While,
                CpgNodeKind::Break,
                CpgNodeKind::Continue,
            ];
            let kind = candidates
                .into_iter()
                .find(|candidate| {
                    pattern.nodes().all(|node| !matcher.nodes_compatible(&node.kind, candidate))
                })
                .expect("four pattern nodes cannot occupy six distinct strict tags");
            let before = matcher.find_matches(&pattern, &g);
            let mut extended = g.clone();
            extended.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()));
            let after = matcher.find_matches(&pattern, &extended);
            prop_assert_eq!(embedding_sequence(&after), embedding_sequence(&before));
        }
    }
}
