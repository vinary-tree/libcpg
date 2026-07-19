//! VF2 subgraph isomorphism algorithm.
//!
//! This module implements the VF2 algorithm for finding subgraph isomorphisms
//! in Code Property Graphs. VF2 is efficient for finding pattern matches
//! in sparse graphs typical of code analysis.

use rustc_hash::{FxHashMap, FxHashSet};

use crate::{CodePropertyGraph, NodeId, CpgNodeKind, CpgEdgeKind};
use super::{PatternMatch, SubgraphMatcher, NodeKindTag};

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

    /// Sets maximum number of matches to find.
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
            // Relaxed matching: same category is enough
            match (pattern_edge.is_ast(), target_edge.is_ast()) {
                (true, true) => true,
                (false, false) => {
                    (pattern_edge.is_cfg() && target_edge.is_cfg())
                        || (pattern_edge.is_dfg() && target_edge.is_dfg())
                        || (pattern_edge.is_call() && target_edge.is_call())
                }
                _ => false,
            }
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

    fn algorithm_name(&self) -> &str {
        "VF2"
    }
}

impl Vf2Matcher {
    /// Recursive VF2 search.
    fn vf2_search(&self, state: &mut Vf2State, matches: &mut Vec<PatternMatch>) {
        // Check if we've reached the match limit
        if self.max_matches > 0 && matches.len() >= self.max_matches {
            return;
        }

        // Check if we have a complete match
        if state.is_complete() {
            let pm = state.to_pattern_match();
            matches.push(pm);
            return;
        }

        // Get candidate pairs
        let candidates = state.candidate_pairs();

        for (pattern_node, target_node) in candidates {
            // Check feasibility
            if self.is_feasible(state, pattern_node, target_node) {
                // Add mapping
                state.push_mapping(pattern_node, target_node);

                // Recurse
                self.vf2_search(state, matches);

                // Backtrack
                state.pop_mapping();

                // Check match limit again
                if self.max_matches > 0 && matches.len() >= self.max_matches {
                    return;
                }
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

        // Check outgoing edges
        for p_edge in state.pattern.outgoing_edges(pattern_node) {
            // If the target of this pattern edge is already mapped
            if let Some(&mapped_target) = state.mapping.get(&p_edge.target) {
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

        // Check incoming edges
        for p_edge in state.pattern.incoming_edges(pattern_node) {
            // If the source of this pattern edge is already mapped
            if let Some(&mapped_source) = state.mapping.get(&p_edge.source) {
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
        let mut pairs = Vec::new();

        // If there are nodes in terminal sets, prefer those
        if !self.pattern_terminal.is_empty() && !self.target_terminal.is_empty() {
            // Pick the first terminal pattern node
            let p_node = *self.pattern_terminal.iter().next().expect("terminal not empty");

            // Pair with all terminal target nodes
            for &t_node in &self.target_terminal {
                pairs.push((p_node, t_node));
            }
        } else if !self.unmapped_pattern.is_empty() {
            // Fall back to any unmapped pattern node
            let p_node = *self.unmapped_pattern.iter().next().expect("unmapped not empty");

            // Pair with all unused target nodes
            for &t_node in &self.unused_target {
                pairs.push((p_node, t_node));
            }
        }

        pairs
    }

    /// Adds a mapping, recording a [`Vf2Frame`] so the change can be undone
    /// exactly on backtrack.
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
            if self.unused_target.contains(&edge.target)
                && self.target_terminal.insert(edge.target)
            {
                target_terminal_added.push(edge.target);
            }
        }
        for edge in self.target.incoming_edges(target_node) {
            if self.unused_target.contains(&edge.source)
                && self.target_terminal.insert(edge.source)
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
            .values()
            .next()
            .copied()
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
    use crate::{CpgNode, CpgNodeKind, SourceRange, Language};

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
}
