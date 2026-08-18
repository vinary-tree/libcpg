//! VF2 subgraph isomorphism algorithm.
//!
//! This module implements the VF2 algorithm for finding subgraph isomorphisms
//! in Code Property Graphs. VF2 is efficient for finding pattern matches
//! in sparse graphs typical of code analysis.

use rustc_hash::{FxHashMap, FxHashSet};

use super::{NodeKindTag, PatternMatch, SubgraphMatcher};
use crate::{CodePropertyGraph, CpgEdgeKind, CpgNodeKind, NodeId};

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

    /// Checks if two nodes are compatible for matching.
    fn nodes_compatible(&self, pattern_node: &CpgNodeKind, target_node: &CpgNodeKind) -> bool {
        if self.strict_kinds {
            // Exact kind match required
            NodeKindTag::from_kind(pattern_node) == NodeKindTag::from_kind(target_node)
        } else {
            // Relaxed matching: same category is enough
            match (pattern_node, target_node) {
                // Both are declarations
                (p, t) if is_declaration(p) && is_declaration(t) => true,
                // Both are expressions
                (p, t) if is_expression(p) && is_expression(t) => true,
                // Both are statements
                (p, t) if is_statement(p) && is_statement(t) => true,
                // Same tag
                _ => NodeKindTag::from_kind(pattern_node) == NodeKindTag::from_kind(target_node),
            }
        }
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

        // Get pattern nodes
        let pattern_nodes: Vec<NodeId> = pattern.node_ids().collect();
        if pattern_nodes.is_empty() {
            return matches;
        }

        // Get target nodes
        let target_nodes: Vec<NodeId> = target.node_ids().collect();
        if target_nodes.len() < pattern_nodes.len() {
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
    /// recursive search activation: its candidate vector is the loop-local
    /// value and `next_candidate` is the suspended loop cursor. A non-root
    /// frame owns exactly one mapping, which is undone when that frame is
    /// exhausted. Complete mappings are emitted and undone immediately.
    fn vf2_search(&self, state: &mut Vf2State, matches: &mut Vec<PatternMatch>) {
        if self.max_matches > 0 && matches.len() >= self.max_matches {
            return;
        }
        if state.is_complete() {
            matches.push(state.to_pattern_match());
            return;
        }

        let mut search = vec![Vf2SearchFrame::root(state.candidate_pairs())];
        while !search.is_empty() {
            if self.max_matches > 0 && matches.len() >= self.max_matches {
                break;
            }

            let candidate = {
                let frame = search.last_mut().expect("search stack is non-empty");
                let candidate = frame.candidates.get(frame.next_candidate).copied();
                frame.next_candidate += usize::from(candidate.is_some());
                candidate
            };

            let Some((pattern_node, target_node)) = candidate else {
                let frame = search.pop().expect("search stack is non-empty");
                if frame.owns_mapping {
                    state.pop_mapping();
                }
                continue;
            };

            if !self.is_feasible(state, pattern_node, target_node) {
                continue;
            }

            state.push_mapping(pattern_node, target_node);
            if state.is_complete() {
                matches.push(state.to_pattern_match());
                state.pop_mapping();
            } else {
                search.push(Vf2SearchFrame::child(state.candidate_pairs()));
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
    fn is_feasible(&self, state: &Vf2State, pattern_node: NodeId, target_node: NodeId) -> bool {
        // Get nodes from graphs
        let p_node = match state.pattern.node(pattern_node) {
            Some(n) => n,
            None => return false,
        };
        let t_node = match state.target.node(target_node) {
            Some(n) => n,
            None => return false,
        };

        // Check node compatibility
        if !self.nodes_compatible(&p_node.kind, &t_node.kind) {
            return false;
        }

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
                state.mapping.get(&p_edge.target).copied()
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
                state.mapping.get(&p_edge.source).copied()
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
    candidates: Vec<(NodeId, NodeId)>,
    next_candidate: usize,
    owns_mapping: bool,
}

impl Vf2SearchFrame {
    fn root(candidates: Vec<(NodeId, NodeId)>) -> Self {
        Self {
            candidates,
            next_candidate: 0,
            owns_mapping: false,
        }
    }

    fn child(candidates: Vec<(NodeId, NodeId)>) -> Self {
        Self {
            candidates,
            next_candidate: 0,
            owns_mapping: true,
        }
    }
}

/// A record of exactly what a single [`Vf2State::push_mapping`] changed, so the
/// matching [`Vf2State::pop_mapping`] can undo it precisely.
///
/// The DFS needs to restore the *exact* pre-push state on backtrack. Two bugs
/// previously made that impossible: `pop_mapping` removed an *arbitrary* pair
/// (`mapping.iter().last()`) instead of the last-pushed one, and the terminal-set
/// helper's removal path was a silent no-op (every branch was gated `if adding`).
/// Recording the pushed pair *plus* the precise terminal-set deltas turns
/// backtracking into an exact inverse of the push — the standard VF2 stack
/// discipline (Cordella et al. 2004).
#[derive(Debug)]
struct Vf2Frame {
    /// The pattern node mapped by this push.
    pattern_node: NodeId,
    /// The target node mapped by this push.
    target_node: NodeId,
    /// Whether `pattern_node` was in `pattern_terminal` before the push.
    pattern_was_terminal: bool,
    /// Whether `target_node` was in `target_terminal` before the push.
    target_was_terminal: bool,
    /// Pattern nodes this push newly inserted into `pattern_terminal`.
    pattern_terminal_added: Vec<NodeId>,
    /// Target nodes this push newly inserted into `target_terminal`.
    target_terminal_added: Vec<NodeId>,
}

/// VF2 algorithm state.
#[derive(Debug)]
pub struct Vf2State<'a> {
    /// The pattern graph.
    pattern: &'a CodePropertyGraph,
    /// The target graph.
    target: &'a CodePropertyGraph,
    /// Current mapping from pattern nodes to target nodes.
    mapping: FxHashMap<NodeId, NodeId>,
    /// Reverse mapping from target nodes to pattern nodes.
    reverse_mapping: FxHashMap<NodeId, NodeId>,
    /// Pattern nodes not yet mapped.
    unmapped_pattern: FxHashSet<NodeId>,
    /// Target nodes not yet used.
    unused_target: FxHashSet<NodeId>,
    /// Pattern nodes adjacent to mapped nodes (terminal set).
    pattern_terminal: FxHashSet<NodeId>,
    /// Target nodes adjacent to used nodes (terminal set).
    target_terminal: FxHashSet<NodeId>,
    /// Explicit push-order stack enabling correct DFS backtracking. Each frame
    /// records the mapped pair and the terminal-set changes that push made, so
    /// [`Vf2State::pop_mapping`] restores the exact prior state.
    order: Vec<Vf2Frame>,
}

impl<'a> Vf2State<'a> {
    /// Creates a new VF2 state.
    pub fn new(pattern: &'a CodePropertyGraph, target: &'a CodePropertyGraph) -> Self {
        let unmapped_pattern: FxHashSet<NodeId> = pattern.node_ids().collect();
        let unused_target: FxHashSet<NodeId> = target.node_ids().collect();

        Self {
            pattern,
            target,
            mapping: FxHashMap::default(),
            reverse_mapping: FxHashMap::default(),
            unmapped_pattern,
            unused_target,
            pattern_terminal: FxHashSet::default(),
            target_terminal: FxHashSet::default(),
            order: Vec::new(),
        }
    }

    /// Returns true if the mapping is complete.
    pub fn is_complete(&self) -> bool {
        self.unmapped_pattern.is_empty()
    }

    /// Returns candidate pairs for the next mapping.
    pub fn candidate_pairs(&self) -> Vec<(NodeId, NodeId)> {
        let mut pattern_candidates = Vec::new();
        let mut target_candidates = Vec::new();

        // If there are nodes in terminal sets, prefer those
        if !self.pattern_terminal.is_empty() && !self.target_terminal.is_empty() {
            pattern_candidates.extend(self.pattern_terminal.iter().copied());
            target_candidates.extend(self.target_terminal.iter().copied());
        } else if !self.unmapped_pattern.is_empty() {
            pattern_candidates.extend(self.unmapped_pattern.iter().copied());
            target_candidates.extend(self.unused_target.iter().copied());
        }

        pattern_candidates.sort_unstable();
        target_candidates.sort_unstable();
        let Some(pattern_node) = pattern_candidates.first().copied() else {
            return Vec::new();
        };
        target_candidates
            .into_iter()
            .map(|target_node| (pattern_node, target_node))
            .collect()
    }

    /// Adds a mapping and records an internal undo frame so the change can be
    /// reversed exactly on backtrack.
    pub fn push_mapping(&mut self, pattern_node: NodeId, target_node: NodeId) {
        self.mapping.insert(pattern_node, target_node);
        self.reverse_mapping.insert(target_node, pattern_node);
        self.unmapped_pattern.remove(&pattern_node);
        self.unused_target.remove(&target_node);
        let pattern_was_terminal = self.pattern_terminal.remove(&pattern_node);
        let target_was_terminal = self.target_terminal.remove(&target_node);

        // Extend the terminal sets with still-unmapped neighbors of the newly
        // mapped pair, recording *only the entries actually inserted* so the
        // matching `pop_mapping` removes precisely those (a neighbor already in
        // the set is justified by another mapped node and must survive the pop).
        // `self.pattern`/`self.target` are shared `&` references (Copy), so the
        // edge iterators borrow the graphs, not `self`, leaving the terminal
        // sets free to mutate.
        let mut pattern_terminal_added = Vec::new();
        for edge in self.pattern.outgoing_edges(pattern_node) {
            if self.unmapped_pattern.contains(&edge.target)
                && self.pattern_terminal.insert(edge.target)
            {
                pattern_terminal_added.push(edge.target);
            }
        }
        for edge in self.pattern.incoming_edges(pattern_node) {
            if self.unmapped_pattern.contains(&edge.source)
                && self.pattern_terminal.insert(edge.source)
            {
                pattern_terminal_added.push(edge.source);
            }
        }

        let mut target_terminal_added = Vec::new();
        for edge in self.target.outgoing_edges(target_node) {
            if self.unused_target.contains(&edge.target) && self.target_terminal.insert(edge.target)
            {
                target_terminal_added.push(edge.target);
            }
        }
        for edge in self.target.incoming_edges(target_node) {
            if self.unused_target.contains(&edge.source) && self.target_terminal.insert(edge.source)
            {
                target_terminal_added.push(edge.source);
            }
        }

        self.order.push(Vf2Frame {
            pattern_node,
            target_node,
            pattern_was_terminal,
            target_was_terminal,
            pattern_terminal_added,
            target_terminal_added,
        });
    }

    /// Removes the last-pushed mapping, restoring the exact pre-push state.
    ///
    /// This pops the true top of the push-order stack (not an arbitrary
    /// `FxHashMap` entry) and reverses precisely the terminal-set edits the
    /// corresponding push made, so the DFS explores every embedding.
    pub fn pop_mapping(&mut self) {
        let Some(frame) = self.order.pop() else {
            return;
        };

        // Undo the terminal-set insertions made by the matching push.
        for node in frame.pattern_terminal_added {
            self.pattern_terminal.remove(&node);
        }
        for node in frame.target_terminal_added {
            self.target_terminal.remove(&node);
        }

        // Undo the mapping itself.
        self.mapping.remove(&frame.pattern_node);
        self.reverse_mapping.remove(&frame.target_node);
        self.unmapped_pattern.insert(frame.pattern_node);
        self.unused_target.insert(frame.target_node);

        // Restore the mapped pair's own terminal membership.
        if frame.pattern_was_terminal {
            self.pattern_terminal.insert(frame.pattern_node);
        }
        if frame.target_was_terminal {
            self.target_terminal.insert(frame.target_node);
        }
    }

    /// Converts the current state to a PatternMatch.
    pub fn to_pattern_match(&self) -> PatternMatch {
        let root = self
            .mapping
            .iter()
            .min_by_key(|(pattern, _)| *pattern)
            .map(|(_, target)| *target)
            .unwrap_or(NodeId::new(0));

        let mut pm = PatternMatch::new("VF2Match", root, 1.0);

        for (&p_node, &t_node) in &self.mapping {
            pm.node_mapping.insert(p_node, t_node);
        }

        pm
    }
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
                signature: MethodSignature {
                    name: "f".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Public,
                },
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
            if matcher.is_feasible(state, pattern_node, target_node) {
                state.push_mapping(pattern_node, target_node);
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
        assert!(state.mapping.is_empty());
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
            let reversed = reverse_insertion(&g);
            let matcher = Vf2Matcher::new()
                .with_strict_kinds(true)
                .with_strict_edges(true)
                .with_max_matches(cap);
            let first = matcher.find_matches(&pattern, &g);
            let second = matcher.find_matches(&pattern, &reversed);
            let repeat = matcher.find_matches(&pattern, &g);
            prop_assert_eq!(embedding_sequence(&first), embedding_sequence(&second));
            prop_assert_eq!(embedding_sequence(&first), embedding_sequence(&repeat));
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
}
