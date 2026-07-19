//! Data Flow Graph extraction from CPG.
//!
//! This module provides algorithms to extract DFG edges from a Code Property Graph.
//! It performs reaching definitions analysis and builds def-use chains.

use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;
use std::sync::Arc;

use crate::{
    CodePropertyGraph, CpgEdgeKind, CpgNodeKind, DfgEdgeKind, NodeId,
};

/// Configuration for DFG extraction.
#[derive(Debug, Clone)]
pub struct DfgExtractorConfig {
    /// Whether to include field access edges.
    pub include_field_access: bool,
    /// Whether to include parameter passing edges.
    pub include_parameters: bool,
    /// Whether to include return value edges.
    pub include_return_values: bool,
    /// Whether to track alias relationships.
    pub track_aliases: bool,
    /// Maximum iterations for reaching definitions fixpoint.
    pub max_iterations: usize,
}

impl Default for DfgExtractorConfig {
    fn default() -> Self {
        Self {
            include_field_access: true,
            include_parameters: true,
            include_return_values: true,
            track_aliases: false, // More complex, disabled by default
            max_iterations: 100,
        }
    }
}

/// DFG extractor that adds data flow edges to a CPG.
#[derive(Debug)]
pub struct DfgExtractor {
    config: DfgExtractorConfig,
}

impl DfgExtractor {
    /// Creates a new DFG extractor with default configuration.
    pub fn new() -> Self {
        Self {
            config: DfgExtractorConfig::default(),
        }
    }

    /// Creates a DFG extractor with custom configuration.
    pub fn with_config(config: DfgExtractorConfig) -> Self {
        Self { config }
    }

    /// Extracts DFG edges for all functions in the CPG.
    pub fn extract(&self, cpg: &mut CodePropertyGraph) {
        // Find all function nodes
        let functions: Vec<NodeId> = cpg
            .functions()
            .map(|n| n.id)
            .collect();

        for func_id in functions {
            self.extract_function_dfg(cpg, func_id);
        }
    }

    /// Extracts DFG edges for a single function.
    pub fn extract_function_dfg(&self, cpg: &mut CodePropertyGraph, function: NodeId) {
        // Phase 1: Collect field/index/parameter sites for the auxiliary edge
        // builders below. Def/use *edges* are no longer driven from this
        // collector's name→site tables — see `build_def_use_edges`.
        let collector = {
            let mut c = DefUseCollector::new(function);
            c.collect(cpg);
            c
        };

        // Phase 2+3: Reaching-definition-based def-use edges, threaded all the
        // way down to the AST-expression identifier uses nested inside each
        // statement (e.g. `buf` in `decode(buf)`).
        self.build_def_use_edges(cpg, function);

        // Phase 4: Build additional DFG edges
        if self.config.include_parameters {
            self.build_parameter_edges(cpg, function, &collector);
        }

        if self.config.include_return_values {
            self.build_return_edges(cpg, function, &collector);
        }

        if self.config.include_field_access {
            self.build_field_access_edges(cpg, &collector);
        }
    }

    /// **Superseded** by [`Self::build_def_use_edges`]'s AST-ordered sweep, and
    /// retained here (never compiled, via `#[cfg(any())]`) as an executable
    /// record of the original approach rather than silently deleted.
    ///
    /// This computed reaching definitions as an iterative dataflow fixpoint over
    /// the CPG's `ControlFlow` edges, keyed **only by CFG nodes**. On *parsed*
    /// code it produced no usable def-use edges for two compounding reasons:
    ///
    /// 1. Nested-expression identifier uses (the common case, e.g. `buf` in
    ///    `decode(buf)`) are AST `Identifier` nodes that are not CFG nodes, so
    ///    `reaching_defs.get(&use_site)` returned `None` and no edge was added.
    /// 2. Even for the few uses that carry a CFG edge, the coarse parsed CFG for
    ///    straight-line `let`/assignment chains does not carry a definition from
    ///    one statement to the next (statements are chained through deep
    ///    sub-expression "exit" nodes with no back-link to the defining node), so
    ///    the reaching set was empty anyway.
    ///
    /// The replacement abstract-interprets the AST in execution order instead of
    /// relying on the (unreliable, for parsed code) CFG propagation.
    #[cfg(any())]
    fn compute_reaching_definitions(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
        collector: &DefUseCollector,
    ) -> FxHashMap<NodeId, FxHashSet<(Arc<str>, NodeId)>> {
        // Get all CFG nodes in the function
        let descendants = cpg.ast_descendants(function);
        let cfg_nodes: Vec<NodeId> = descendants
            .into_iter()
            .filter(|&id| {
                cpg.cfg_predecessors(id).len() > 0 || cpg.cfg_successors(id).len() > 0
            })
            .collect();

        // Initialize reaching definitions sets
        // IN[n] = set of (variable, definition site) pairs reaching n
        let mut in_sets: FxHashMap<NodeId, FxHashSet<(Arc<str>, NodeId)>> = FxHashMap::default();
        let mut out_sets: FxHashMap<NodeId, FxHashSet<(Arc<str>, NodeId)>> = FxHashMap::default();

        for &node in &cfg_nodes {
            in_sets.insert(node, FxHashSet::default());
            out_sets.insert(node, FxHashSet::default());
        }

        // Also initialize for function entry
        in_sets.insert(function, FxHashSet::default());
        out_sets.insert(function, FxHashSet::default());

        // Add parameter definitions at function entry
        for (var_name, def_site) in &collector.definitions {
            let node_kind = cpg.node(*def_site).map(|n| n.kind.clone());
            if matches!(node_kind.as_ref(), Some(CpgNodeKind::Parameter { .. })) {
                if let Some(out_set) = out_sets.get_mut(&function) {
                    out_set.insert((var_name.clone(), *def_site));
                }
            }
        }

        // Iterative fixpoint computation
        let mut changed = true;
        let mut iterations = 0;

        while changed && iterations < self.config.max_iterations {
            changed = false;
            iterations += 1;

            for &node in &cfg_nodes {
                // IN[n] = union of OUT[p] for all predecessors p
                let mut new_in: FxHashSet<(Arc<str>, NodeId)> = FxHashSet::default();

                for (pred, _) in cpg.cfg_predecessors(node) {
                    if let Some(pred_out) = out_sets.get(&pred) {
                        new_in.extend(pred_out.iter().cloned());
                    }
                }

                // OUT[n] = GEN[n] union (IN[n] - KILL[n])
                let mut new_out = new_in.clone();

                // KILL: Remove definitions of variables that are redefined here
                let node_defs: Vec<_> = collector
                    .definitions
                    .iter()
                    .filter(|(_, &def_site)| def_site == node)
                    .map(|(name, _)| name.clone())
                    .collect();

                for var_name in &node_defs {
                    new_out.retain(|(name, _)| name != var_name);
                }

                // GEN: Add definitions from this node
                for var_name in &node_defs {
                    new_out.insert((var_name.clone(), node));
                }

                // Check for changes
                if new_in != *in_sets.get(&node).unwrap_or(&FxHashSet::default()) {
                    changed = true;
                    in_sets.insert(node, new_in);
                }

                if new_out != *out_sets.get(&node).unwrap_or(&FxHashSet::default()) {
                    changed = true;
                    out_sets.insert(node, new_out);
                }
            }
        }

        in_sets
    }

    /// **Superseded** by the AST-ordered `build_def_use_edges` below; retained
    /// (never compiled, via `#[cfg(any())]`) for reference. It keyed reaching
    /// sets by `use_site`, but nested-expression identifier uses are not CFG
    /// nodes and so were absent from `reaching_defs`, yielding zero edges on
    /// parsed code.
    #[cfg(any())]
    fn build_def_use_edges_cfg(
        &self,
        cpg: &mut CodePropertyGraph,
        collector: &DefUseCollector,
        reaching_defs: &FxHashMap<NodeId, FxHashSet<(Arc<str>, NodeId)>>,
    ) {
        for (var_name, use_sites) in &collector.uses {
            for &use_site in use_sites {
                // Get reaching definitions at this use site
                if let Some(reaching) = reaching_defs.get(&use_site) {
                    for (def_var, def_site) in reaching {
                        if def_var == var_name {
                            // Create def-use edge
                            cpg.connect(
                                *def_site,
                                use_site,
                                CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
                            );
                        }
                    }
                }
            }
        }
    }

    /// Builds def-use edges via an AST-ordered, flow-sensitive reaching-
    /// definitions sweep, threading each statement's reaching definitions down
    /// to the *nested* identifier uses inside it (e.g. `buf` in `decode(buf)` or
    /// `x` in `return f(x)`) — precisely the uses the CFG-propagated predecessor
    /// (`compute_reaching_definitions`, retained above) missed.
    ///
    /// The sweep abstract-interprets the function body in execution order over
    /// the AST, maintaining a per-name set of *currently reaching* definition
    /// nodes ([`ReachingEnv`]):
    ///
    /// * A `let`/`Variable`, `Parameter`, or simple `Assignment` **generates** a
    ///   definition (the defining node) for its bound name. In straight-line
    ///   (unconditional) context it also **kills** prior definitions of that
    ///   name — flow-sensitive: the latest write wins. Inside a conditionally
    ///   executed region (a branch or loop body) a definition is *added* without
    ///   killing, so a write on one path does not erase a write on another — a
    ///   sound over-approximation (extra edges, never a missing one).
    /// * An identifier in *use* position links every currently-reaching
    ///   definition of its name to itself with a [`DfgEdgeKind::DefUse`] edge.
    ///   Binder identifiers (the `x` in `let x = …`, a parameter's own name, the
    ///   target of a plain `x = …`) are definition sites, not uses, and are
    ///   skipped so no spurious self-edge is created.
    /// * The initializer/RHS of a definition is evaluated *before* the name is
    ///   (re)bound, so a use in the initializer sees the pre-existing definition
    ///   (`let x = x + 1`, `x = x + 1`).
    /// * Loop bodies are swept twice (a bounded fixpoint) so a use near the top
    ///   can observe a definition made lower in the same body (loop-carried
    ///   dependence).
    ///
    /// The pass is intraprocedural, bounded, language-agnostic (it dispatches on
    /// [`CpgNodeKind`], never on grammar specifics) and idempotent: an edge is
    /// added only when an identical one does not already exist, so uses that
    /// *are* CFG nodes are not double-linked and re-running `extract` is a no-op.
    fn build_def_use_edges(&self, cpg: &mut CodePropertyGraph, function: NodeId) {
        // Idempotency: never re-add a DefUse edge that already exists.
        let mut existing: FxHashSet<(u32, u32)> = cpg
            .edges()
            .filter_map(|e| match e.kind {
                CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse) => Some((e.source.0, e.target.0)),
                _ => None,
            })
            .collect();

        // Compute the (def -> use) pairs while borrowing `cpg` immutably, then
        // emit them (mutable borrow). The reaching environment threads through
        // the walk in place; parameters are bound before the body because they
        // precede it in the function's AST child order.
        let mut env: ReachingEnv = FxHashMap::default();
        let mut pairs: Vec<(NodeId, NodeId)> = Vec::new();
        for child in cpg.ast_children(function) {
            visit_reaching(cpg, child, &mut env, false, &mut pairs);
        }

        for (def, use_site) in pairs {
            if def != use_site && existing.insert((def.0, use_site.0)) {
                cpg.connect(def, use_site, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse));
            }
        }
    }

    /// Builds parameter passing edges.
    fn build_parameter_edges(
        &self,
        cpg: &mut CodePropertyGraph,
        function: NodeId,
        collector: &DefUseCollector,
    ) {
        // Find parameter nodes
        let params: Vec<NodeId> = cpg
            .ast_children(function)
            .into_iter()
            .filter(|&id| {
                cpg.node(id)
                    .map(|n| matches!(n.kind, CpgNodeKind::Parameter { .. }))
                    .unwrap_or(false)
            })
            .collect();

        // Find call sites that call this function
        let callers = cpg.callers(function);

        for caller in callers {
            let call_args = cpg.ast_children(caller);

            // Match arguments to parameters
            for (i, &param) in params.iter().enumerate() {
                if let Some(&arg) = call_args.get(i) {
                    cpg.connect(
                        arg,
                        param,
                        CpgEdgeKind::DataFlow(DfgEdgeKind::Parameter),
                    );
                }
            }
        }

        // Also connect uses of parameters *declared as direct children of the
        // function* to their parameter definition. This is an idempotent
        // supplement to the primary `build_def_use_edges` sweep (which already
        // links parameter uses flow-sensitively wherever the parameters are
        // reachable in the AST); it is a no-op for parser-built graphs, where
        // parameters are nested under a `parameters` wrapper rather than direct
        // children. Dedup against existing DefUse edges keeps it idempotent and
        // avoids double-linking the uses the sweep already connected.
        let mut existing_def_use: FxHashSet<(u32, u32)> = cpg
            .edges()
            .filter_map(|e| match e.kind {
                CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse) => Some((e.source.0, e.target.0)),
                _ => None,
            })
            .collect();
        for &param in &params {
            let param_name = cpg.node(param).and_then(|n| n.name().map(Arc::from));
            if let Some(name) = param_name {
                if let Some(uses) = collector.uses.get(&name) {
                    for &use_site in uses {
                        if param != use_site && existing_def_use.insert((param.0, use_site.0)) {
                            cpg.connect(
                                param,
                                use_site,
                                CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse),
                            );
                        }
                    }
                }
            }
        }
    }

    /// Builds return value edges.
    fn build_return_edges(
        &self,
        cpg: &mut CodePropertyGraph,
        function: NodeId,
        _collector: &DefUseCollector,
    ) {
        // Find return statements
        let returns: Vec<NodeId> = cpg
            .ast_descendants(function)
            .into_iter()
            .filter(|&id| {
                cpg.node(id)
                    .map(|n| matches!(n.kind, CpgNodeKind::Return))
                    .unwrap_or(false)
            })
            .collect();

        // Find call sites that call this function
        let callers = cpg.callers(function);

        for &ret in &returns {
            // Get the returned expression
            let ret_children = cpg.ast_children(ret);
            if let Some(&ret_expr) = ret_children.first() {
                for &caller in &callers {
                    cpg.connect(
                        ret_expr,
                        caller,
                        CpgEdgeKind::DataFlow(DfgEdgeKind::ReturnValue),
                    );
                }
            }
        }
    }

    /// Builds field access edges.
    fn build_field_access_edges(
        &self,
        cpg: &mut CodePropertyGraph,
        collector: &DefUseCollector,
    ) {
        // Process member access nodes
        for &node_id in &collector.field_accesses {
            let children = cpg.ast_children(node_id);

            // First child is typically the object being accessed
            if let Some(&obj) = children.first() {
                // Create field read edge from object to member access
                cpg.connect(
                    obj,
                    node_id,
                    CpgEdgeKind::DataFlow(DfgEdgeKind::FieldRead),
                );
            }
        }

        // Process index access nodes
        for &node_id in &collector.index_accesses {
            let children = cpg.ast_children(node_id);

            if children.len() >= 2 {
                let array = children[0];
                let index = children[1];

                // Create index read edges
                cpg.connect(
                    array,
                    node_id,
                    CpgEdgeKind::DataFlow(DfgEdgeKind::IndexRead),
                );
                cpg.connect(
                    index,
                    node_id,
                    CpgEdgeKind::DataFlow(DfgEdgeKind::DataDependency),
                );
            }
        }
    }
}

impl Default for DfgExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-variable set of currently-reaching definition nodes, keyed by bound name.
/// The small on-stack vector keeps the common single-definition (straight-line)
/// case allocation-free; multiple entries arise only after a branch/loop merge.
type ReachingEnv = FxHashMap<Arc<str>, SmallVec<[NodeId; 2]>>;

/// Returns true for node kinds whose children execute *conditionally* (a branch
/// or loop body). A definition inside such a region performs a weak update (add
/// without kill) so it cannot erase a definition live on a sibling path.
fn is_conditional_region(kind: &CpgNodeKind) -> bool {
    matches!(
        kind,
        CpgNodeKind::If
            | CpgNodeKind::Else
            | CpgNodeKind::While
            | CpgNodeKind::For
            | CpgNodeKind::Loop
            | CpgNodeKind::Match
            | CpgNodeKind::MatchArm
            | CpgNodeKind::Try
            | CpgNodeKind::Catch
            | CpgNodeKind::Finally
    )
}

/// Returns true for loop kinds, whose bodies are swept twice so a use near the
/// top of the body can observe a definition made lower in the same body.
fn is_loop_region(kind: &CpgNodeKind) -> bool {
    matches!(kind, CpgNodeKind::While | CpgNodeKind::For | CpgNodeKind::Loop)
}

/// The identifier child of a binding node (`let`/parameter) that *is* the bound
/// name — the binder, which is a definition site, not a use, and is skipped by
/// the walk. Chosen as the first `Identifier` child whose text equals the
/// binder's declared `name`; because a pattern precedes its initializer in
/// source (hence in node/edge order), a shadowing initializer use of the same
/// name (`let x = x + 1`) comes *after* the binder and is still visited as a use.
fn binder_identifier(cpg: &CodePropertyGraph, node: NodeId, name: &str) -> Option<NodeId> {
    if name.is_empty() {
        return None;
    }
    cpg.ast_children(node).into_iter().find(|&c| {
        matches!(
            cpg.node(c).map(|n| &n.kind),
            Some(CpgNodeKind::Identifier { name: n, .. }) if &**n == name
        )
    })
}

/// The bound name of an `Identifier` node, if that is its kind.
fn identifier_name(cpg: &CodePropertyGraph, node: NodeId) -> Option<Arc<str>> {
    match cpg.node(node).map(|n| &n.kind) {
        Some(CpgNodeKind::Identifier { name, .. }) => Some(name.clone()),
        _ => None,
    }
}

/// Records `def` as a reaching definition of `name` in `env`. In straight-line
/// context (`conditional == false`) this is a strong update (kill + gen: the
/// latest write wins); inside a conditional region it is a weak update (add
/// without kill) so a write on one path does not erase another path's.
fn bind_definition(env: &mut ReachingEnv, name: Arc<str>, def: NodeId, conditional: bool) {
    if name.is_empty() {
        return;
    }
    let slot = env.entry(name).or_default();
    if conditional {
        if !slot.contains(&def) {
            slot.push(def);
        }
    } else {
        slot.clear();
        slot.push(def);
    }
}

/// Core AST-ordered reaching-definitions walk. See
/// [`DfgExtractor::build_def_use_edges`] for the algorithm and its rationale.
fn visit_reaching(
    cpg: &CodePropertyGraph,
    node: NodeId,
    env: &mut ReachingEnv,
    conditional: bool,
    pairs: &mut Vec<(NodeId, NodeId)>,
) {
    // Clone the kind so the immutable borrow of `cpg.node(node)` ends before we
    // recurse (each arm re-borrows `cpg` immutably via `ast_children`).
    let kind = match cpg.node(node) {
        Some(n) => n.kind.clone(),
        None => return,
    };

    match &kind {
        // `let` / local declaration: evaluate the initializer (whose uses see the
        // pre-binding environment), then bind the declared name.
        CpgNodeKind::Variable { name, .. } => {
            let binder = binder_identifier(cpg, node, name);
            for child in cpg.ast_children(node) {
                if Some(child) == binder {
                    continue;
                }
                visit_reaching(cpg, child, env, conditional, pairs);
            }
            bind_definition(env, name.clone(), node, conditional);
        }

        // Parameter: its identifier child is the binder (a definition), not a
        // use; any remaining children (e.g. a default-value expression, in
        // languages that have them) are visited as uses.
        CpgNodeKind::Parameter { name, .. } => {
            let binder = binder_identifier(cpg, node, name);
            for child in cpg.ast_children(node) {
                if Some(child) == binder {
                    continue;
                }
                visit_reaching(cpg, child, env, conditional, pairs);
            }
            bind_definition(env, name.clone(), node, conditional);
        }

        // Assignment: the first child is the target l-value. For a plain `=` to a
        // simple variable the target is a pure write (skipped as a use); for a
        // compound assignment (`+=`, …) it is read-then-written; for a complex
        // l-value (`a.f = …`, `a[i] = …`) the target is not a simple identifier,
        // so it is visited normally (its base is a genuine use).
        CpgNodeKind::Assignment { operator } => {
            let compound = &**operator != "=";
            let kids = cpg.ast_children(node);
            let target = kids.first().copied();
            let target_is_ident = matches!(
                target.and_then(|t| cpg.node(t)).map(|n| &n.kind),
                Some(CpgNodeKind::Identifier { .. })
            );

            for &child in &kids {
                if Some(child) == target && target_is_ident && !compound {
                    continue; // simple `x = …`: the target is written, not read
                }
                visit_reaching(cpg, child, env, conditional, pairs);
            }

            if target_is_ident {
                if let Some(name) = target.and_then(|t| identifier_name(cpg, t)) {
                    bind_definition(env, name, node, conditional);
                }
            }
        }

        // Identifier in use position: link every currently-reaching definition
        // of its name. Binder identifiers never reach this arm — their parent
        // construct skips them above.
        CpgNodeKind::Identifier { name, .. } => {
            if let Some(defs) = env.get(name) {
                for &def in defs {
                    pairs.push((def, node));
                }
            }
        }

        // Everything else (blocks, calls, arguments, control flow, …): recurse in
        // source order. Branch/loop bodies switch to conditional (weak-update)
        // context; loop bodies are swept twice for loop-carried dependences.
        _ => {
            let child_conditional = conditional || is_conditional_region(&kind);
            let passes = if is_loop_region(&kind) { 2 } else { 1 };
            for _ in 0..passes {
                for child in cpg.ast_children(node) {
                    visit_reaching(cpg, child, env, child_conditional, pairs);
                }
            }
        }
    }
}

/// Collects definitions and uses from a function.
#[derive(Debug)]
struct DefUseCollector {
    function: NodeId,
    /// Map from variable name to definition sites.
    definitions: FxHashMap<Arc<str>, NodeId>,
    /// Map from variable name to use sites.
    uses: FxHashMap<Arc<str>, SmallVec<[NodeId; 8]>>,
    /// Field access nodes.
    field_accesses: Vec<NodeId>,
    /// Index access nodes.
    index_accesses: Vec<NodeId>,
    /// Assignment nodes.
    assignments: Vec<NodeId>,
}

impl DefUseCollector {
    fn new(function: NodeId) -> Self {
        Self {
            function,
            definitions: FxHashMap::default(),
            uses: FxHashMap::default(),
            field_accesses: Vec::new(),
            index_accesses: Vec::new(),
            assignments: Vec::new(),
        }
    }

    /// Collects all definitions and uses in the function.
    fn collect(&mut self, cpg: &CodePropertyGraph) {
        let descendants = cpg.ast_descendants(self.function);

        // First pass: collect definitions
        for &node_id in &descendants {
            if let Some(node) = cpg.node(node_id) {
                match &node.kind {
                    // Variable declaration is a definition
                    CpgNodeKind::Variable { name, .. } => {
                        self.definitions.insert(name.clone(), node_id);
                    }
                    // Parameter is a definition
                    CpgNodeKind::Parameter { name, .. } => {
                        self.definitions.insert(name.clone(), node_id);
                    }
                    // Assignment: LHS is a definition
                    CpgNodeKind::Assignment { .. } => {
                        self.assignments.push(node_id);
                        // Find the target of the assignment
                        let children = cpg.ast_children(node_id);
                        if let Some(&lhs) = children.first() {
                            if let Some(lhs_node) = cpg.node(lhs) {
                                if let CpgNodeKind::Identifier { name, .. } = &lhs_node.kind {
                                    self.definitions.insert(name.clone(), node_id);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        // Second pass: collect uses
        for &node_id in &descendants {
            if let Some(node) = cpg.node(node_id) {
                match &node.kind {
                    // Identifier reference is a use (unless it's the LHS of an assignment)
                    CpgNodeKind::Identifier { name, .. } => {
                        // Check if this is the LHS of an assignment
                        let parent = node.parent;
                        let is_assignment_target = parent
                            .and_then(|p| cpg.node(p))
                            .map(|p| {
                                matches!(p.kind, CpgNodeKind::Assignment { .. })
                                    && cpg.ast_children(p.id).first() == Some(&node_id)
                            })
                            .unwrap_or(false);

                        if !is_assignment_target {
                            self.uses
                                .entry(name.clone())
                                .or_default()
                                .push(node_id);
                        }
                    }
                    // Member access
                    CpgNodeKind::MemberAccess { .. } => {
                        self.field_accesses.push(node_id);
                    }
                    // Index access
                    CpgNodeKind::IndexAccess => {
                        self.index_accesses.push(node_id);
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Represents a definition point in the data flow.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Definition {
    /// The variable being defined.
    pub variable: Arc<str>,
    /// The node where the definition occurs.
    pub node: NodeId,
    /// The kind of definition.
    pub kind: DefinitionKind,
}

/// Kind of definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DefinitionKind {
    /// Variable declaration.
    Declaration,
    /// Assignment.
    Assignment,
    /// Parameter.
    Parameter,
    /// Field write.
    FieldWrite,
    /// Index write.
    IndexWrite,
}

/// Represents a use point in the data flow.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Use {
    /// The variable being used.
    pub variable: Arc<str>,
    /// The node where the use occurs.
    pub node: NodeId,
    /// The kind of use.
    pub kind: UseKind,
}

/// Kind of use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum UseKind {
    /// Simple variable read.
    Read,
    /// Field read.
    FieldRead,
    /// Index read.
    IndexRead,
    /// Function call argument.
    Argument,
}

/// Def-use chain for a variable.
#[derive(Debug, Clone)]
pub struct DefUseChain {
    /// The variable name.
    pub variable: Arc<str>,
    /// All definitions of this variable.
    pub definitions: Vec<NodeId>,
    /// All uses of this variable.
    pub uses: Vec<NodeId>,
    /// Map from definition to its uses.
    pub def_to_uses: FxHashMap<NodeId, Vec<NodeId>>,
    /// Map from use to its reaching definitions.
    pub use_to_defs: FxHashMap<NodeId, Vec<NodeId>>,
}

impl DefUseChain {
    /// Creates a new def-use chain for a variable.
    pub fn new(variable: Arc<str>) -> Self {
        Self {
            variable,
            definitions: Vec::new(),
            uses: Vec::new(),
            def_to_uses: FxHashMap::default(),
            use_to_defs: FxHashMap::default(),
        }
    }

    /// Adds a definition.
    pub fn add_definition(&mut self, def: NodeId) {
        if !self.definitions.contains(&def) {
            self.definitions.push(def);
        }
    }

    /// Adds a use.
    pub fn add_use(&mut self, use_site: NodeId) {
        if !self.uses.contains(&use_site) {
            self.uses.push(use_site);
        }
    }

    /// Links a definition to a use.
    pub fn link(&mut self, def: NodeId, use_site: NodeId) {
        self.def_to_uses
            .entry(def)
            .or_default()
            .push(use_site);
        self.use_to_defs
            .entry(use_site)
            .or_default()
            .push(def);
    }

    /// Returns all uses that are reached by the given definition.
    pub fn uses_of(&self, def: NodeId) -> &[NodeId] {
        self.def_to_uses.get(&def).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Returns all definitions that reach the given use.
    pub fn definitions_of(&self, use_site: NodeId) -> &[NodeId] {
        self.use_to_defs.get(&use_site).map(|v| v.as_slice()).unwrap_or(&[])
    }
}

/// Builds def-use chains for all variables in a function.
pub fn build_def_use_chains(cpg: &CodePropertyGraph, function: NodeId) -> FxHashMap<Arc<str>, DefUseChain> {
    let mut chains: FxHashMap<Arc<str>, DefUseChain> = FxHashMap::default();

    // Collect all def-use edges
    for edge in cpg.edges() {
        if let CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse) = &edge.kind {
            let def_id = edge.source;
            let use_id = edge.target;

            // Check if they belong to this function
            let def_in_func = cpg.ast_ancestors(def_id).contains(&function) || def_id == function;
            let use_in_func = cpg.ast_ancestors(use_id).contains(&function) || use_id == function;

            if !def_in_func || !use_in_func {
                continue;
            }

            // Get variable name from definition
            let var_name = cpg.node(def_id).and_then(|n| match &n.kind {
                CpgNodeKind::Variable { name, .. } => Some(name.clone()),
                CpgNodeKind::Parameter { name, .. } => Some(name.clone()),
                CpgNodeKind::Assignment { .. } => {
                    // Get the target identifier
                    let children = cpg.ast_children(def_id);
                    children.first().and_then(|&lhs| {
                        cpg.node(lhs).and_then(|lhs_node| {
                            if let CpgNodeKind::Identifier { name, .. } = &lhs_node.kind {
                                Some(name.clone())
                            } else {
                                None
                            }
                        })
                    })
                }
                _ => None,
            });

            if let Some(name) = var_name {
                let chain = chains.entry(name.clone()).or_insert_with(|| DefUseChain::new(name));
                chain.add_definition(def_id);
                chain.add_use(use_id);
                chain.link(def_id, use_id);
            }
        }
    }

    chains
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CpgNode, SourceRange, Language, ScopeId, MethodSignature, Visibility};

    fn create_test_function(cpg: &mut CodePropertyGraph) -> NodeId {
        let func = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Function {
                signature: MethodSignature {
                    name: "test".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Public,
                },
            },
            SourceRange::default(),
        ));

        let body = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Block { scope: ScopeId::GLOBAL },
            SourceRange::default(),
        ));

        cpg.connect(func, body, CpgEdgeKind::AstChild);
        cpg.node_mut(body).unwrap().parent = Some(func);

        func
    }

    #[test]
    fn test_dfg_extractor_basic() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = create_test_function(&mut cpg);
        let body = cpg.ast_children(func)[0];

        // Add variable declaration
        let var = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Variable {
                name: "x".into(),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: true,
            },
            SourceRange::default(),
        ).with_parent(body));
        cpg.connect(body, var, CpgEdgeKind::AstChild);

        // Add identifier reference (use)
        let ident = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Identifier {
                name: "x".into(),
                definition: Some(var),
            },
            SourceRange::default(),
        ).with_parent(body));
        cpg.connect(body, ident, CpgEdgeKind::AstChild);

        let extractor = DfgExtractor::new();
        extractor.extract(&mut cpg);

        // The extractor should work without panicking
        // (Full def-use chains require CFG to be built first)
    }

    #[test]
    fn test_def_use_collector() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = create_test_function(&mut cpg);
        let body = cpg.ast_children(func)[0];

        // Add variable declaration
        let var = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Variable {
                name: "x".into(),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: true,
            },
            SourceRange::default(),
        ).with_parent(body));
        cpg.connect(body, var, CpgEdgeKind::AstChild);

        // Add identifier reference
        let ident = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Identifier {
                name: "x".into(),
                definition: Some(var),
            },
            SourceRange::default(),
        ).with_parent(body));
        cpg.connect(body, ident, CpgEdgeKind::AstChild);

        let mut collector = DefUseCollector::new(func);
        collector.collect(&cpg);

        // Should find the definition
        assert!(collector.definitions.contains_key(&Arc::from("x")));

        // Should find the use
        assert!(collector.uses.contains_key(&Arc::from("x")));
    }

    #[test]
    fn test_def_use_chain() {
        let mut chain = DefUseChain::new("x".into());

        chain.add_definition(NodeId::new(1));
        chain.add_definition(NodeId::new(2));
        chain.add_use(NodeId::new(3));
        chain.add_use(NodeId::new(4));

        chain.link(NodeId::new(1), NodeId::new(3));
        chain.link(NodeId::new(2), NodeId::new(4));

        assert_eq!(chain.uses_of(NodeId::new(1)), &[NodeId::new(3)]);
        assert_eq!(chain.definitions_of(NodeId::new(4)), &[NodeId::new(2)]);
    }

    // ===================================================================
    // Parsed-code def-use (the P7c prerequisite): the DFG must be non-empty
    // and correct on a normally-parsed Rust function, with reaching
    // definitions threaded down to nested-expression identifier uses.
    // ===================================================================

    #[cfg(feature = "lang-rust")]
    fn build_rust(src: &str) -> CodePropertyGraph {
        use crate::{CpgBuilder, TreeSitterCpgBuilder};
        TreeSitterCpgBuilder::new()
            .build(src, Language::Rust)
            .expect("parse + build should succeed")
    }

    /// All node ids whose kind satisfies `pred`, sorted by id for determinism.
    #[cfg(feature = "lang-rust")]
    fn nodes_where(
        cpg: &CodePropertyGraph,
        pred: impl Fn(&CpgNodeKind) -> bool,
    ) -> Vec<NodeId> {
        let mut ids: Vec<NodeId> = cpg
            .node_ids()
            .filter(|&id| cpg.node(id).map(|n| pred(&n.kind)).unwrap_or(false))
            .collect();
        ids.sort_by_key(|n| n.0);
        ids
    }

    /// The (unique) `Variable` definition node named `name`.
    #[cfg(feature = "lang-rust")]
    fn var_named(cpg: &CodePropertyGraph, name: &str) -> NodeId {
        let v = nodes_where(cpg, |k| {
            matches!(k, CpgNodeKind::Variable { name: n, .. } if &**n == name)
        });
        assert_eq!(v.len(), 1, "expected exactly one `let {name}`");
        v[0]
    }

    /// The identifier *use* of `name`: an `Identifier` node that actually
    /// received a reaching definition (i.e. it is not the binder of `name`).
    #[cfg(feature = "lang-rust")]
    fn ident_use(cpg: &CodePropertyGraph, name: &str) -> NodeId {
        nodes_where(cpg, |k| {
            matches!(k, CpgNodeKind::Identifier { name: n, .. } if &**n == name)
        })
        .into_iter()
        .find(|&id| !cpg.reaching_definitions(id).is_empty())
        .unwrap_or_else(|| panic!("no reaching-def use of `{name}` (bug: DFG empty?)"))
    }

    /// Scenario 1: `let buf = read(fd); let out = decode(buf); sink(out);`.
    /// The nested `buf` inside `decode(buf)` — an AST `Identifier`, not a CFG
    /// node — must resolve to the `let buf` definition, and the whole
    /// buf -> out -> sink-arg data path must exist.
    #[test]
    #[cfg(feature = "lang-rust")]
    fn parsed_nested_use_resolves_and_chains() {
        let cpg = build_rust(
            "fn foo(fd: i32) { let buf = read(fd); let out = decode(buf); sink(out); }",
        );

        // The whole point: a normally-parsed function now has DFG edges.
        // (fd -> use in read, buf -> use in decode, out -> use in sink.)
        assert!(
            cpg.stats().dfg_edges >= 3,
            "parsed code must have def-use edges; got {}",
            cpg.stats().dfg_edges
        );

        let buf_def = var_named(&cpg, "buf");
        let out_def = var_named(&cpg, "out");
        let buf_use = ident_use(&cpg, "buf"); // the `buf` inside decode(buf)
        let out_use = ident_use(&cpg, "out"); // the `out` inside sink(out)

        // reaching_definitions of the nested `buf` use returns the `let buf` def.
        assert!(
            cpg.reaching_definitions(buf_use).contains(&buf_def),
            "reaching defs of the nested `buf` use must include `let buf`"
        );
        // dfg_successors(let buf) reaches the `buf` use.
        assert!(
            cpg.dfg_successors(buf_def).iter().any(|(t, _)| *t == buf_use),
            "dfg_successors(let buf) must reach the `buf` use"
        );

        // The `buf` use is the initializer of `let out`, and the `out` use's
        // reaching def is that same `let out` — chaining buf -> out -> sink-arg.
        assert!(
            cpg.ast_descendants(out_def).contains(&buf_use),
            "the `buf` use lives inside the `let out` statement"
        );
        assert!(
            cpg.reaching_definitions(out_use).contains(&out_def),
            "reaching defs of the sink argument `out` must include `let out`"
        );
        assert!(
            cpg.dfg_successors(out_def).iter().any(|(t, _)| *t == out_use),
            "dfg_successors(let out) must reach the `out` use"
        );
    }

    /// Scenario 2: a reassignment. `let mut x = a; x = b; use_it(x);` — the use
    /// must resolve to the LATEST definition (`x = b`), not the killed first one
    /// (`let mut x = a`). Flow-sensitivity in straight-line code.
    #[test]
    #[cfg(feature = "lang-rust")]
    fn parsed_reassignment_uses_latest_definition() {
        let cpg = build_rust("fn g(a: i32, b: i32) { let mut x = a; x = b; use_it(x); }");

        let let_x = var_named(&cpg, "x");
        let assign_x = nodes_where(&cpg, |k| matches!(k, CpgNodeKind::Assignment { .. }));
        assert_eq!(assign_x.len(), 1, "one assignment `x = b`");
        let assign_x = assign_x[0];
        let x_use = ident_use(&cpg, "x");

        let reaching = cpg.reaching_definitions(x_use);
        assert!(
            reaching.contains(&assign_x),
            "the use must see the latest def `x = b`"
        );
        assert!(
            !reaching.contains(&let_x),
            "the use must NOT see the killed def `let mut x = a`"
        );
        assert_eq!(
            reaching.len(),
            1,
            "straight-line code: exactly one reaching def"
        );
    }

    /// Scenario 3: unrelated variables do not flow into each other's uses.
    #[test]
    #[cfg(feature = "lang-rust")]
    fn parsed_unrelated_variable_does_not_flow() {
        let cpg = build_rust("fn f() { let x = mk(); let y = mk(); use_x(x); use_y(y); }");

        let x_def = var_named(&cpg, "x");
        let y_def = var_named(&cpg, "y");
        let x_use = ident_use(&cpg, "x");
        let y_use = ident_use(&cpg, "y");

        assert!(cpg.reaching_definitions(x_use).contains(&x_def));
        assert!(cpg.reaching_definitions(y_use).contains(&y_def));
        assert!(
            !cpg.reaching_definitions(x_use).contains(&y_def),
            "`y` must not flow to `x`'s use"
        );
        assert!(
            !cpg.reaching_definitions(y_use).contains(&x_def),
            "`x` must not flow to `y`'s use"
        );
        assert!(
            !cpg.dfg_successors(x_def).iter().any(|(t, _)| *t == y_use),
            "`x`'s def must not reach `y`'s use"
        );
    }

    /// A shadowing initializer use sees the *outer* definition, and the walk is
    /// idempotent: re-running `extract` adds no duplicate edges.
    #[test]
    #[cfg(feature = "lang-rust")]
    fn parsed_shadowing_and_idempotent() {
        let mut cpg = build_rust("fn f(n: i32) { let n = n + 1; consume(n); }");

        // Two distinct definitions of `n`: the parameter and the shadowing let.
        let param_n = nodes_where(&cpg, |k| matches!(k, CpgNodeKind::Parameter { .. }))[0];
        let let_n = var_named(&cpg, "n");

        // The `n` in `n + 1` is the shadowing initializer use: it must resolve
        // to the parameter (the pre-binding definition), not to the `let n`.
        let init_use = nodes_where(&cpg, |k| {
            matches!(k, CpgNodeKind::Identifier { name, .. } if &**name == "n")
        })
        .into_iter()
        .find(|&id| cpg.reaching_definitions(id).contains(&param_n))
        .expect("initializer use of `n` bound to the parameter");
        assert!(!cpg.reaching_definitions(init_use).contains(&let_n));

        // `consume(n)` sees the shadowing `let n`.
        let consume_use = nodes_where(&cpg, |k| {
            matches!(k, CpgNodeKind::Identifier { name, .. } if &**name == "n")
        })
        .into_iter()
        .find(|&id| cpg.reaching_definitions(id).contains(&let_n))
        .expect("consume use of `n` bound to the shadowing let");
        assert!(consume_use != init_use);

        // Idempotency: a second extraction pass must not add any edge.
        let before = cpg.stats().dfg_edges;
        DfgExtractor::new().extract(&mut cpg);
        assert_eq!(before, cpg.stats().dfg_edges, "extract must be idempotent");
    }
}
