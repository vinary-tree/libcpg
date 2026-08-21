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
//!    rooted at `EXIT`. [`crate::post_dominator_tree`] uses the deterministic
//!    Cooper–Harvey–Kennedy algorithm and returns immediate post-dominators.
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

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use petgraph::Direction;
use rustc_hash::FxHashSet;

use crate::{post_dominator_tree, CodePropertyGraph, CpgEdgeKind, DfgEdgeKind, NodeId};

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
    /// [`crate::builder::CpgBuilder::build`] (or
    /// [`crate::builder::TreeSitterCpgBuilder::build_from_tree`]) with
    /// `build_cfg`/`build_dfg` enabled, or the extractors were run manually.
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
    match post_dominator_tree(cpg, function) {
        Ok(tree) => tree.control_dependence_edges(),
        Err(_) => Vec::new(),
    }
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

/// Edge families admitted by one phase of a dependence-graph traversal.
///
/// Program-dependence edges are always admitted. The two interprocedural
/// parameter families are toggled independently so the SDG driver can prevent
/// a traversal from returning through a different call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SliceEdgeMask {
    pub(crate) parameter: bool,
    pub(crate) return_value: bool,
}

impl SliceEdgeMask {
    pub(crate) const PDG_ONLY: Self = Self {
        parameter: false,
        return_value: false,
    };
}

/// Deterministic bounded traversal shared by intraprocedural and SDG slices.
///
/// `seeds` maps each starting node to the number of call boundaries already
/// crossed. `reached` is an accumulator shared across phases. The returned map
/// records the smallest call depth found for each node in this phase, allowing
/// a later shallower discovery to be expanded even if the node was seen first
/// through a deeper path.
pub(crate) fn slice_phase(
    cpg: &CodePropertyGraph,
    seeds: &BTreeMap<NodeId, usize>,
    max_nodes: usize,
    max_call_depth: usize,
    direction: Direction,
    mask: SliceEdgeMask,
    reached: &mut BTreeSet<NodeId>,
) -> BTreeMap<NodeId, usize> {
    let mut best_depth = BTreeMap::new();
    if max_nodes == 0 {
        return best_depth;
    }

    let mut queue = VecDeque::new();
    for (&seed, &depth) in seeds {
        if depth > max_call_depth || cpg.node(seed).is_none() {
            continue;
        }
        if !reached.contains(&seed) && reached.len() >= max_nodes {
            break;
        }
        reached.insert(seed);
        best_depth.insert(seed, depth);
        queue.push_back((seed, depth));
    }

    while let Some((node, depth)) = queue.pop_front() {
        if best_depth.get(&node).is_some_and(|&best| depth > best) {
            continue;
        }

        let mut neighbors: Vec<(NodeId, usize)> = match direction {
            Direction::Incoming => cpg
                .incoming_edges(node)
                .filter_map(|edge| {
                    slice_edge_cost(&edge.kind, mask).map(|cost| (edge.source, cost))
                })
                .collect(),
            Direction::Outgoing => cpg
                .outgoing_edges(node)
                .filter_map(|edge| {
                    slice_edge_cost(&edge.kind, mask).map(|cost| (edge.target, cost))
                })
                .collect(),
        };
        neighbors.sort_unstable();
        neighbors.dedup();

        for (next, edge_cost) in neighbors {
            let Some(next_depth) = depth.checked_add(edge_cost) else {
                continue;
            };
            if next_depth > max_call_depth
                || best_depth
                    .get(&next)
                    .is_some_and(|&known| known <= next_depth)
            {
                continue;
            }
            if !reached.contains(&next) && reached.len() >= max_nodes {
                continue;
            }
            reached.insert(next);
            best_depth.insert(next, next_depth);
            queue.push_back((next, next_depth));
        }
    }

    best_depth
}

fn slice_edge_cost(kind: &CpgEdgeKind, mask: SliceEdgeMask) -> Option<usize> {
    match kind {
        CpgEdgeKind::ControlDependence | CpgEdgeKind::DataDependence => Some(0),
        CpgEdgeKind::DataFlow(DfgEdgeKind::Parameter) if mask.parameter => Some(1),
        CpgEdgeKind::DataFlow(DfgEdgeKind::ReturnValue) if mask.return_value => Some(1),
        _ => None,
    }
}

/// Shared BFS over PDG edges in a single direction.
fn slice(
    cpg: &CodePropertyGraph,
    criterion: NodeId,
    max_nodes: usize,
    direction: Direction,
) -> FxHashSet<NodeId> {
    let seeds = BTreeMap::from([(criterion, 0)]);
    let mut reached = BTreeSet::new();
    slice_phase(
        cpg,
        &seeds,
        max_nodes,
        0,
        direction,
        SliceEdgeMask::PDG_ONLY,
        &mut reached,
    );
    reached.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CfgExtractor, CpgNode, CpgNodeKind, DfgExtractor, Language, MethodSignature, ScopeId,
        SourceRange, Visibility,
    };

    /// Adds a node of `kind` through the invariant-owning AST edge API.
    fn add_child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
        let id = cpg.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()));
        cpg.connect(parent, id, CpgEdgeKind::AstChild);
        id
    }

    fn add_function(cpg: &mut CodePropertyGraph, name: &str) -> NodeId {
        cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Function {
                signature: Box::new(MethodSignature {
                    name: name.into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Public,
                }),
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
