//! Program Dependence Graph (PDG) construction and program slicing.
//!
//! A Program Dependence Graph (Ferrante, Ottenstein & Warren 1987,
//! <https://doi.org/10.1145/24039.24041>) augments the CFG/DFG with two
//! dependence relations that make *program slicing* (Weiser 1981,
//! <https://doi.org/10.5555/800078.802557>) a simple graph reachability query:
//!
//! - **Data dependence** `d -> u`: statement `u` uses a value that statement
//!   `d` defines. libcpg's DFG already materializes these as
//!   [`DfgEdgeKind::DefUse`]/[`DfgEdgeKind::ReachingDef`] edges; this module
//!   re-projects them as first-class [`CpgEdgeKind::DataDependence`] edges so a
//!   slicer can traverse a single, uniform PDG edge set.
//! - **Control dependence** `a -> b`: whether statement `b` executes is decided
//!   by branch node `a`. libcpg has no builder for these; this module computes
//!   them and emits [`CpgEdgeKind::ControlDependence`] edges.
//!
//! ## How control dependence is computed
//!
//! The classic result (Cytron, Ferrante, Rosen, Wegman & Zadeck 1991,
//! <https://doi.org/10.1145/115372.115320>) is that **the control-dependence
//! relation is exactly the dominance frontier of the *reverse* CFG.** Concretely:
//!
//! 1. Take the intraprocedural CFG of `function` (the sub-CFG reachable over
//!    `ControlFlow` edges, excluding `Call` edges into callees).
//! 2. Add a virtual `EXIT` and connect every real exit to it (a function's
//!    recorded exits — e.g. a loop guard's false branch — plus structural sinks
//!    and `Return`/`Throw`).
//! 3. Compute **post-dominators** = dominators of the CFG with edges reversed,
//!    rooted at `EXIT`. petgraph's [`simple_fast`] gives the immediate-dominator
//!    map directly; running it on a reversed view yields immediate
//!    post-dominators for free.
//! 4. Compute the **post-dominance frontier** (a.k.a. reverse dominance
//!    frontier, RDF) via the Cytron et al. walk.
//! 5. `b` is control-dependent on `a` iff `a ∈ RDF(b)`. Emit `a -> b`.
//!
//! `$b$` post-dominates `$a$` when every path from `$a$` to `EXIT` passes
//! through `$b$`; `$b$` is control-dependent on `$a$` when `$b$` post-dominates
//! a successor of `$a$` but not `$a$` itself — precisely the frontier condition.
//!
//! ## Deviations from a naive reading of the CFG
//!
//! - `CodePropertyGraph::function_cfg` is **not** used: it filters to
//!   control-flow/expression nodes and drops `Block` nodes (the CFG's
//!   connective tissue), fragmenting the graph. We instead walk the real
//!   `ControlFlow` edges on the full CPG.
//! - Exit identification uses `cpg.cfg_exits()`, not sink detection alone: a
//!   `while` guard is a recorded exit (its false branch leaves the loop) yet
//!   still has a successor (the body), so it is not a sink.

use std::collections::VecDeque;

use petgraph::algo::dominators::simple_fast;
use petgraph::graphmap::DiGraphMap;
use petgraph::Direction;
use rustc_hash::FxHashSet;

use crate::{CfgEdgeKind, CodePropertyGraph, CpgEdgeKind, CpgNodeKind, DfgEdgeKind, NodeId};

/// Sentinel node id for the virtual CFG exit used during post-dominator
/// computation. Real CPG node ids are small sequential `u32`s, so `u32::MAX`
/// never collides.
const VIRTUAL_EXIT: u32 = u32::MAX;

/// Builds Program Dependence Graph edges (control + data dependence) onto an
/// existing [`CodePropertyGraph`] that already carries CFG and DFG edges.
///
/// Intended to be run once per function on a freshly built CPG; re-running is
/// idempotent (existing PDG edges are not duplicated).
#[derive(Debug, Clone, Default)]
pub struct PdgBuilder {
    _private: (),
}

impl PdgBuilder {
    /// Creates a new PDG builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds [`CpgEdgeKind::ControlDependence`] and
    /// [`CpgEdgeKind::DataDependence`] edges for `function` to `cpg`.
    ///
    /// Requires that `function`'s CFG (`ControlFlow` edges) and DFG (`DataFlow`
    /// edges) have already been extracted — i.e. `cpg` came from
    /// [`crate::builder::TreeSitterCpgBuilder::build`] (or `build_from_tree`)
    /// with `build_cfg`/`build_dfg` enabled, or the extractors were run
    /// manually.
    pub fn build(&self, cpg: &mut CodePropertyGraph, function: NodeId) {
        // Compute both dependence relations while only borrowing `cpg`
        // immutably, then emit. Snapshot existing PDG edges first so a re-build
        // does not duplicate them (idempotency).
        let existing: FxHashSet<(u32, u32, bool)> = cpg
            .edges()
            .filter_map(|e| match e.kind {
                CpgEdgeKind::ControlDependence => Some((e.source.0, e.target.0, true)),
                CpgEdgeKind::DataDependence => Some((e.source.0, e.target.0, false)),
                _ => None,
            })
            .collect();

        let control_edges = compute_control_dependence(cpg, function);
        let data_edges = compute_data_dependence(cpg, function);

        for (controller, dependent) in control_edges {
            if !existing.contains(&(controller.0, dependent.0, true)) {
                cpg.connect(controller, dependent, CpgEdgeKind::ControlDependence);
            }
        }
        for (def, use_site) in data_edges {
            if !existing.contains(&(def.0, use_site.0, false)) {
                cpg.connect(def, use_site, CpgEdgeKind::DataDependence);
            }
        }
    }
}

/// Computes the control-dependence edges `(controller, dependent)` for
/// `function` without mutating `cpg`.
///
/// Returns a deduplicated list; the caller emits them as
/// [`CpgEdgeKind::ControlDependence`] edges. A loop guard legitimately appears
/// as its own controller (self-dependence via the back edge), matching the
/// Ferrante–Ottenstein–Warren construction.
fn compute_control_dependence(cpg: &CodePropertyGraph, function: NodeId) -> Vec<(NodeId, NodeId)> {
    // 1. Intraprocedural CFG node set: reachable from `function` over
    //    `ControlFlow` edges, excluding `Call` edges (which cross into callees).
    let mut cfg_nodes: FxHashSet<NodeId> = FxHashSet::default();
    cfg_nodes.insert(function);
    let mut stack = vec![function];
    while let Some(node) = stack.pop() {
        for (succ, kind) in cpg.cfg_successors(node) {
            if matches!(kind, CfgEdgeKind::Call) {
                continue;
            }
            if cfg_nodes.insert(succ) {
                stack.push(succ);
            }
        }
    }
    if cfg_nodes.len() < 2 {
        return Vec::new();
    }

    // 2. Reverse CFG as a DiGraphMap keyed by NodeId.0, plus the virtual EXIT.
    //    For each forward edge (a -> b) add the reverse edge (b -> a); post-
    //    dominators are the dominators of this reversed graph rooted at EXIT.
    let mut reverse: DiGraphMap<u32, ()> =
        DiGraphMap::with_capacity(cfg_nodes.len() + 1, cfg_nodes.len() * 2);
    reverse.add_node(VIRTUAL_EXIT);
    for &node in &cfg_nodes {
        reverse.add_node(node.0);
    }
    let mut has_successor: FxHashSet<NodeId> = FxHashSet::default();
    for &node in &cfg_nodes {
        for (succ, kind) in cpg.cfg_successors(node) {
            if matches!(kind, CfgEdgeKind::Call) || !cfg_nodes.contains(&succ) {
                continue;
            }
            reverse.add_edge(succ.0, node.0, ());
            has_successor.insert(node);
        }
    }

    // 3. Exits: the CFG builder's recorded function exits (e.g. a loop guard's
    //    false branch — NOT a sink), plus structural sinks, plus Return/Throw.
    let recorded_exits: FxHashSet<NodeId> = cpg
        .cfg_exits()
        .iter()
        .copied()
        .filter(|n| cfg_nodes.contains(n))
        .collect();
    let mut connected_any_exit = false;
    for &node in &cfg_nodes {
        let is_return_or_throw = matches!(
            cpg.node(node).map(|n| &n.kind),
            Some(CpgNodeKind::Return) | Some(CpgNodeKind::Throw)
        );
        if !has_successor.contains(&node) || is_return_or_throw || recorded_exits.contains(&node) {
            reverse.add_edge(VIRTUAL_EXIT, node.0, ());
            connected_any_exit = true;
        }
    }
    if !connected_any_exit {
        // No exits at all (e.g. a single unbounded loop): post-dominance is
        // undefined. Degrade gracefully — emit no control dependence.
        return Vec::new();
    }

    // 3b. Ensure every node can reach EXIT so post-dominators are total. Any
    //     node still unreachable from EXIT (an infinite loop with no recorded
    //     exit) is connected directly — a standard, sound virtual edge.
    let reachable = reachable_from(&reverse, VIRTUAL_EXIT);
    for &node in &cfg_nodes {
        if !reachable.contains(&node.0) {
            reverse.add_edge(VIRTUAL_EXIT, node.0, ());
        }
    }

    // 4. Post-dominator tree = dominators of the reverse CFG rooted at EXIT.
    let postdom = simple_fast(&reverse, VIRTUAL_EXIT);

    // 5. Reverse dominance frontier (Cytron et al. 1991). For each join `b`
    //    (>= 2 predecessors in the reverse CFG), walk each predecessor up the
    //    post-dominator tree to ipdom(b); every visited node is control-
    //    dependent on `b`. Emit (controller = b, dependent = visited).
    let mut edges: Vec<(NodeId, NodeId)> = Vec::new();
    let mut seen: FxHashSet<(u32, u32)> = FxHashSet::default();
    for b in reverse.nodes() {
        if b == VIRTUAL_EXIT {
            continue;
        }
        let preds: Vec<u32> = reverse.neighbors_directed(b, Direction::Incoming).collect();
        if preds.len() < 2 {
            continue;
        }
        let ipdom_b = match postdom.immediate_dominator(b) {
            Some(dom) => dom,
            None => continue,
        };
        for pred in preds {
            let mut runner = pred;
            while runner != ipdom_b {
                if runner != VIRTUAL_EXIT && seen.insert((b, runner)) {
                    edges.push((NodeId::new(b), NodeId::new(runner)));
                }
                match postdom.immediate_dominator(runner) {
                    Some(dom) => runner = dom,
                    None => break,
                }
            }
        }
    }
    edges
}

/// Computes the data-dependence edges `(def, use)` for `function` by projecting
/// the DFG's def-use / reaching-definition edges whose endpoints both lie within
/// the function's AST subtree.
fn compute_data_dependence(cpg: &CodePropertyGraph, function: NodeId) -> Vec<(NodeId, NodeId)> {
    let mut function_nodes: FxHashSet<NodeId> = cpg.ast_descendants(function).into_iter().collect();
    function_nodes.insert(function);

    let mut seen: FxHashSet<(u32, u32)> = FxHashSet::default();
    let mut edges: Vec<(NodeId, NodeId)> = Vec::new();
    for edge in cpg.edges() {
        if matches!(
            edge.kind,
            CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse | DfgEdgeKind::ReachingDef)
        ) && function_nodes.contains(&edge.source)
            && function_nodes.contains(&edge.target)
            && seen.insert((edge.source.0, edge.target.0))
        {
            edges.push((edge.source, edge.target));
        }
    }
    edges
}

/// Reachable-node set from `start` over the outgoing edges of `graph`.
fn reachable_from(graph: &DiGraphMap<u32, ()>, start: u32) -> FxHashSet<u32> {
    let mut seen: FxHashSet<u32> = FxHashSet::default();
    seen.insert(start);
    let mut stack = vec![start];
    while let Some(node) = stack.pop() {
        for next in graph.neighbors_directed(node, Direction::Outgoing) {
            if seen.insert(next) {
                stack.push(next);
            }
        }
    }
    seen
}

/// Backward program slice (Weiser 1981): every node the `criterion`
/// (transitively) depends on, via PDG edges.
///
/// Reverse-BFS following **incoming** [`CpgEdgeKind::ControlDependence`] and
/// [`CpgEdgeKind::DataDependence`] edges — because both point
/// influence-source → influenced, the nodes `criterion` depends on are its PDG
/// predecessors. Includes `criterion` itself. Traversal stops once the slice
/// reaches `max_nodes` nodes.
pub fn backward_slice(
    cpg: &CodePropertyGraph,
    criterion: NodeId,
    max_nodes: usize,
) -> FxHashSet<NodeId> {
    slice(cpg, criterion, max_nodes, Direction::Incoming)
}

/// Forward program slice: every node (transitively) affected by the
/// `criterion`, via PDG edges.
///
/// Forward-BFS following **outgoing** [`CpgEdgeKind::ControlDependence`] and
/// [`CpgEdgeKind::DataDependence`] edges. Includes `criterion` itself. Traversal
/// stops once the slice reaches `max_nodes` nodes.
pub fn forward_slice(
    cpg: &CodePropertyGraph,
    criterion: NodeId,
    max_nodes: usize,
) -> FxHashSet<NodeId> {
    slice(cpg, criterion, max_nodes, Direction::Outgoing)
}

/// Shared BFS over PDG edges in a single direction.
fn slice(
    cpg: &CodePropertyGraph,
    criterion: NodeId,
    max_nodes: usize,
    direction: Direction,
) -> FxHashSet<NodeId> {
    let mut slice: FxHashSet<NodeId> = FxHashSet::default();
    if max_nodes == 0 || cpg.node(criterion).is_none() {
        return slice;
    }
    slice.insert(criterion);

    let mut queue: VecDeque<NodeId> = VecDeque::new();
    queue.push_back(criterion);
    while let Some(node) = queue.pop_front() {
        // Collect PDG neighbors in `direction` (owned, so the immutable borrow
        // of `cpg` ends before we mutate `slice`).
        let neighbors: Vec<NodeId> = match direction {
            Direction::Incoming => cpg
                .incoming_edges(node)
                .filter(|e| e.kind.is_pdg())
                .map(|e| e.source)
                .collect(),
            Direction::Outgoing => cpg
                .outgoing_edges(node)
                .filter(|e| e.kind.is_pdg())
                .map(|e| e.target)
                .collect(),
        };
        for next in neighbors {
            if slice.len() >= max_nodes {
                return slice;
            }
            if slice.insert(next) {
                queue.push_back(next);
            }
        }
    }
    slice
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CfgExtractor, CpgNode, DfgExtractor, Language, MethodSignature, ScopeId, SourceRange,
        Visibility,
    };

    /// Adds a node of `kind` as an AST child of `parent`, wiring the AstChild
    /// edge, the parent pointer, and the parent's child list — the shape the
    /// CFG/DFG extractors read.
    fn add_child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
        let id = cpg.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()));
        cpg.connect(parent, id, CpgEdgeKind::AstChild);
        if let Some(node) = cpg.node_mut(id) {
            node.parent = Some(parent);
        }
        if let Some(parent_node) = cpg.node_mut(parent) {
            parent_node.children.push(id);
        }
        id
    }

    fn add_function(cpg: &mut CodePropertyGraph, name: &str) -> NodeId {
        cpg.add_node(CpgNode::new(
            NodeId::new(0),
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
            SourceRange::default(),
        ))
    }

    fn ident(name: &str) -> CpgNodeKind {
        CpgNodeKind::Identifier {
            name: name.into(),
            definition: None,
        }
    }

    fn variable(name: &str) -> CpgNodeKind {
        CpgNodeKind::Variable {
            name: name.into(),
            var_type: None,
            scope: ScopeId::GLOBAL,
            is_mutable: false,
        }
    }

    fn block() -> CpgNodeKind {
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        }
    }

    fn has_control_dependence(cpg: &CodePropertyGraph, from: NodeId, to: NodeId) -> bool {
        cpg.edges_between(from, to)
            .iter()
            .any(|e| e.kind == CpgEdgeKind::ControlDependence)
    }

    fn control_dependence_count(cpg: &CodePropertyGraph) -> usize {
        cpg.edges()
            .filter(|e| e.kind == CpgEdgeKind::ControlDependence)
            .count()
    }

    /// An if-diamond: both branch bodies are control-dependent on the condition;
    /// straight-line code above/around it is not.
    #[test]
    fn if_diamond_control_dependence() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = add_function(&mut cpg, "f");
        let body = add_child(&mut cpg, func, block());
        let if_node = add_child(&mut cpg, body, CpgNodeKind::If);
        // if children are [condition, then, else].
        let cond = add_child(&mut cpg, if_node, ident("p"));
        let then_block = add_child(&mut cpg, if_node, block());
        let then_stmt = add_child(&mut cpg, then_block, ident("a"));
        let else_block = add_child(&mut cpg, if_node, block());
        let else_stmt = add_child(&mut cpg, else_block, ident("b"));

        CfgExtractor::new().extract(&mut cpg);
        PdgBuilder::new().build(&mut cpg, func);

        // Both arms (and their statements) depend on the condition node.
        assert!(has_control_dependence(&cpg, cond, then_block));
        assert!(has_control_dependence(&cpg, cond, else_block));
        assert!(has_control_dependence(&cpg, cond, then_stmt));
        assert!(has_control_dependence(&cpg, cond, else_stmt));

        // The condition itself is unconditionally reached -> not control-
        // dependent on anything (no incoming ControlDependence edge).
        assert!(
            !cpg.incoming_edges(cond)
                .any(|e| e.kind == CpgEdgeKind::ControlDependence),
            "the top-level condition must not be control-dependent"
        );
    }

    /// A while loop: the body is control-dependent on the guard; the guard is
    /// self-dependent via the loop back edge (Ferrante–Ottenstein–Warren).
    #[test]
    fn while_body_control_dependent_on_guard() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = add_function(&mut cpg, "f");
        let body = add_child(&mut cpg, func, block());
        let while_node = add_child(&mut cpg, body, CpgNodeKind::While);
        // while children are [condition, body].
        let guard = add_child(&mut cpg, while_node, ident("c"));
        let loop_body = add_child(&mut cpg, while_node, block());
        let loop_stmt = add_child(&mut cpg, loop_body, ident("work"));

        CfgExtractor::new().extract(&mut cpg);
        PdgBuilder::new().build(&mut cpg, func);

        assert!(has_control_dependence(&cpg, guard, loop_body));
        assert!(has_control_dependence(&cpg, guard, loop_stmt));
        // Guard self-dependence via the back edge.
        assert!(has_control_dependence(&cpg, guard, guard));
    }

    /// A straight-line block has no branches -> no control dependence at all.
    #[test]
    fn straight_line_has_no_control_dependence() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = add_function(&mut cpg, "f");
        let body = add_child(&mut cpg, func, block());
        let _s1 = add_child(&mut cpg, body, ident("s1"));
        let _s2 = add_child(&mut cpg, body, ident("s2"));
        let _s3 = add_child(&mut cpg, body, ident("s3"));

        CfgExtractor::new().extract(&mut cpg);
        PdgBuilder::new().build(&mut cpg, func);

        assert_eq!(
            control_dependence_count(&cpg),
            0,
            "straight-line code must have no control dependence"
        );
    }

    /// Backward slice over a def-use chain returns the use and its reaching def.
    #[test]
    fn def_use_backward_slice() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = add_function(&mut cpg, "f");
        let body = add_child(&mut cpg, func, block());
        let def_x = add_child(&mut cpg, body, variable("x"));
        let use_x = add_child(&mut cpg, body, ident("x"));

        CfgExtractor::new().extract(&mut cpg);
        DfgExtractor::new().extract(&mut cpg);
        PdgBuilder::new().build(&mut cpg, func);

        // The DFG produced a def-use edge that the PDG re-projected as a data
        // dependence edge; the backward slice of the use reaches the def.
        let slice = backward_slice(&cpg, use_x, 100);
        assert!(slice.contains(&use_x), "slice must contain the criterion");
        assert!(
            slice.contains(&def_x),
            "backward slice of a use must contain its reaching definition"
        );

        // Forward slice from the def reaches the use.
        let forward = forward_slice(&cpg, def_x, 100);
        assert!(forward.contains(&def_x));
        assert!(forward.contains(&use_x));

        // The traversal bound is respected.
        let capped = backward_slice(&cpg, use_x, 1);
        assert_eq!(capped.len(), 1);
    }

    /// End-to-end through the real parser: an `if`/`else` over a parsed Rust
    /// snippet yields at least one control-dependence edge, and the slicer runs
    /// on the resulting PDG.
    #[test]
    #[cfg(feature = "lang-rust")]
    fn parsed_rust_if_has_control_dependence() {
        use crate::{CpgBuilder, TreeSitterCpgBuilder};

        let source = "fn f(x: bool) { if x { a(); } else { b(); } }";
        let mut cpg = TreeSitterCpgBuilder::new()
            .build(source, Language::Rust)
            .expect("build should succeed");
        let func = cpg.functions().map(|n| n.id).next().expect("function node");

        PdgBuilder::new().build(&mut cpg, func);
        assert!(
            control_dependence_count(&cpg) >= 1,
            "a parsed if/else must yield control dependence"
        );

        // The slicer must run and at least return the criterion itself.
        let some_node = cpg.functions().map(|n| n.id).next().expect("node");
        assert!(backward_slice(&cpg, some_node, 64).contains(&some_node));
    }

    /// End-to-end: a parsed function with an empty body has no branch, hence no
    /// control dependence.
    #[test]
    #[cfg(feature = "lang-rust")]
    fn parsed_rust_straight_line_no_control_dependence() {
        use crate::{CpgBuilder, TreeSitterCpgBuilder};

        let source = "fn f() {}";
        let mut cpg = TreeSitterCpgBuilder::new()
            .build(source, Language::Rust)
            .expect("build should succeed");
        let func = cpg.functions().map(|n| n.id).next().expect("function node");

        PdgBuilder::new().build(&mut cpg, func);
        assert_eq!(control_dependence_count(&cpg), 0);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::testutil::*;
    use crate::{CfgExtractor, DfgExtractor};
    use proptest::prelude::*;

    /// Naive oracle: every node reachable from `criterion` over *incoming*
    /// `ControlDependence`/`DataDependence` edges (criterion included). The
    /// bounded `backward_slice` must always be a subset of this set.
    fn reachable_incoming_pdg(cpg: &CodePropertyGraph, criterion: NodeId) -> FxHashSet<NodeId> {
        let mut seen = FxHashSet::default();
        seen.insert(criterion);
        let mut stack = vec![criterion];
        while let Some(n) = stack.pop() {
            for e in cpg.incoming_edges(n) {
                if e.kind.is_pdg() && seen.insert(e.source) {
                    stack.push(e.source);
                }
            }
        }
        seen
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// Slice bounds + model-based oracle: `|slice| ≤ max_nodes`;
        /// `max_nodes == 0 ⇒ empty`; `max_nodes ≥ 1 ⇒ criterion ∈ slice`; and
        /// `backward_slice ⊆` the brute-force incoming-PDG reachable set.
        #[test]
        fn prop_slice_bounds_and_oracle(
            cpg in arb_pdg_graph(),
            idx in any::<usize>(),
            max_nodes in 0usize..=12,
        ) {
            let ids = node_ids(&cpg);
            prop_assume!(!ids.is_empty());
            let criterion = ids[idx % ids.len()];

            let bwd = backward_slice(&cpg, criterion, max_nodes);
            prop_assert!(bwd.len() <= max_nodes);
            if max_nodes == 0 {
                prop_assert!(bwd.is_empty());
            } else {
                prop_assert!(bwd.contains(&criterion));
            }
            let oracle = reachable_incoming_pdg(&cpg, criterion);
            prop_assert!(bwd.is_subset(&oracle));

            // Forward slice shares the bound / empty / membership properties.
            let fwd = forward_slice(&cpg, criterion, max_nodes);
            prop_assert!(fwd.len() <= max_nodes);
            if max_nodes == 0 {
                prop_assert!(fwd.is_empty());
            } else {
                prop_assert!(fwd.contains(&criterion));
            }
        }

        /// The slice is monotone in `max_nodes`: a smaller bound yields a subset
        /// of a larger bound's slice (both truncate the same BFS discovery order).
        #[test]
        fn prop_slice_monotone_in_max_nodes(
            cpg in arb_pdg_graph(),
            idx in any::<usize>(),
            a in 0usize..=12,
            b in 0usize..=12,
        ) {
            let ids = node_ids(&cpg);
            prop_assume!(!ids.is_empty());
            let criterion = ids[idx % ids.len()];
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            let s_lo = backward_slice(&cpg, criterion, lo);
            let s_hi = backward_slice(&cpg, criterion, hi);
            prop_assert!(s_lo.is_subset(&s_hi));
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        /// `PdgBuilder::build` is idempotent: after CFG + DFG extraction, a second
        /// build on the function leaves `edge_count` unchanged (existing PDG edges
        /// are not duplicated).
        #[test]
        fn prop_pdg_build_idempotent(cpg in arb_well_formed_cpg()) {
            let mut cpg = cpg;
            CfgExtractor::new().extract(&mut cpg);
            DfgExtractor::new().extract(&mut cpg);
            let func = cpg.functions().map(|n| n.id).next();
            prop_assume!(func.is_some());
            let func = func.expect("function id");

            PdgBuilder::new().build(&mut cpg, func);
            let after_first = cpg.edge_count();
            PdgBuilder::new().build(&mut cpg, func);
            prop_assert_eq!(after_first, cpg.edge_count());
        }
    }
}
