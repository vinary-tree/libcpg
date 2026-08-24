//! Data Flow Graph extraction from CPG.
//!
//! This module provides algorithms to extract DFG edges from a Code Property Graph.
//! It performs reaching definitions analysis and builds def-use chains.

use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;
use std::sync::Arc;

use crate::{CodePropertyGraph, CpgEdgeKind, CpgNodeKind, DfgEdgeKind, NodeId};

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
    /// Compatibility cap for the retired CFG fixed-point implementation.
    ///
    /// The live AST-ordered sweep does not consult this field.
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

/// Immutable-computation result for one function's data-flow graph.
#[derive(Debug, Clone, Default)]
pub(super) struct DfgPlan {
    pub(super) edges: Vec<(NodeId, NodeId, CpgEdgeKind)>,
}

impl DfgPlan {
    fn connect_unique(&mut self, source: NodeId, target: NodeId, kind: CpgEdgeKind) {
        self.edges.push((source, target, kind));
    }

    pub(super) fn emit(self, cpg: &mut CodePropertyGraph) {
        for (source, target, kind) in self.edges {
            cpg.connect_unique(source, target, kind);
        }
    }
}

/// Immutable exact-name index used by member-to-field resolution.
///
/// The full extractor builds it once and shares it across functions, avoiding
/// a graph-wide type scan per access. Candidate lists preserve ambiguity so
/// resolution can fail closed instead of choosing by storage order.
type NamedCandidates = FxHashMap<Arc<str>, SmallVec<[NodeId; 2]>>;

#[derive(Debug, Default)]
pub(super) struct FieldResolutionIndex {
    types: FxHashMap<Option<NodeId>, NamedCandidates>,
    fields: FxHashMap<NodeId, NamedCandidates>,
}

impl FieldResolutionIndex {
    pub(super) fn new(cpg: &CodePropertyGraph) -> Self {
        let mut index = Self::default();
        for id in cpg.node_ids() {
            match cpg.node(id).map(|node| &node.kind) {
                Some(
                    CpgNodeKind::Class { name, .. }
                    | CpgNodeKind::Struct { name }
                    | CpgNodeKind::Enum { name },
                ) if !name.is_empty() => {
                    index
                        .types
                        .entry(declaration_scope(cpg, id))
                        .or_default()
                        .entry(name.clone())
                        .or_default()
                        .push(id);
                }
                Some(CpgNodeKind::Field { name, .. }) if !name.is_empty() => {
                    if let Some(owner) = enclosing_field_owner(cpg, id) {
                        index
                            .fields
                            .entry(owner)
                            .or_default()
                            .entry(name.clone())
                            .or_default()
                            .push(id);
                    }
                }
                _ => {}
            }
        }
        for candidates in index.types.values_mut().flat_map(FxHashMap::values_mut) {
            candidates.sort_unstable();
            candidates.dedup();
        }
        for candidates in index.fields.values_mut().flat_map(FxHashMap::values_mut) {
            candidates.sort_unstable();
            candidates.dedup();
        }
        index
    }

    fn unique_type(&self, scope: Option<NodeId>, name: &str) -> Option<NodeId> {
        let candidates = self.types.get(&scope)?.get(name)?;
        (candidates.len() == 1).then_some(candidates[0])
    }

    fn unique_field(&self, owner: NodeId, name: &str) -> Option<NodeId> {
        let candidates = self.fields.get(&owner)?.get(name)?;
        (candidates.len() == 1).then_some(candidates[0])
    }
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
        let functions: Vec<NodeId> = cpg.functions().map(|n| n.id).collect();
        let field_index = self
            .config
            .include_field_access
            .then(|| FieldResolutionIndex::new(cpg));
        let plans: Vec<_> = functions
            .into_iter()
            .map(|function| self.plan_function_dfg(cpg, function, field_index.as_ref()))
            .collect();
        for plan in plans {
            plan.emit(cpg);
        }
    }

    /// Extracts all function DFGs concurrently on the current Rayon pool.
    ///
    /// Workers read the same immutable AST, CFG, and call-resolution evidence
    /// and produce owned edge plans. Indexed collection followed by serial
    /// emission preserves the exact serial edge order for every worker count.
    /// Install a custom [`rayon::ThreadPool`] around this call to select the
    /// worker count or stack size.
    pub fn extract_parallel(&self, cpg: &mut CodePropertyGraph) {
        let functions: Vec<NodeId> = cpg.functions().map(|node| node.id).collect();
        let field_index = self
            .config
            .include_field_access
            .then(|| FieldResolutionIndex::new(cpg));
        let plans: Vec<_> = functions
            .par_iter()
            .map(|&function| self.plan_function_dfg(cpg, function, field_index.as_ref()))
            .collect();
        for plan in plans {
            plan.emit(cpg);
        }
    }

    /// Extracts DFG edges for a single function.
    pub fn extract_function_dfg(&self, cpg: &mut CodePropertyGraph, function: NodeId) {
        let field_index = self
            .config
            .include_field_access
            .then(|| FieldResolutionIndex::new(cpg));
        self.plan_function_dfg(cpg, function, field_index.as_ref())
            .emit(cpg);
    }

    pub(super) fn plan_function_dfg(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
        field_index: Option<&FieldResolutionIndex>,
    ) -> DfgPlan {
        let mut plan = DfgPlan::default();
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
        self.build_def_use_edges(cpg, &mut plan, function);

        // Phase 4: Build additional DFG edges
        if self.config.include_parameters {
            self.build_parameter_edges(cpg, &mut plan, function, &collector);
        }

        if self.config.include_return_values {
            self.build_return_edges(cpg, &mut plan, function, &collector);
        }

        if self.config.include_field_access {
            self.build_field_access_edges(
                cpg,
                &mut plan,
                &collector,
                field_index.expect("field index exists when field access is enabled"),
            );
        }
        plan
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
            .filter(|&id| cpg.cfg_predecessors(id).len() > 0 || cpg.cfg_successors(id).len() > 0)
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
            if matches!(
                cpg.node_tag(*def_site),
                Some(crate::CpgNodeKindTag::Parameter)
            ) {
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
                            cpg.connect_unique(
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
    fn build_def_use_edges(&self, cpg: &CodePropertyGraph, plan: &mut DfgPlan, function: NodeId) {
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
        seed_lexical_environment(cpg, function, &mut env);
        let mut on_path: FxHashSet<NodeId> = FxHashSet::default();
        on_path.insert(function);
        for &child in cpg.ast_children(function) {
            visit_reaching(cpg, child, &mut env, false, &mut pairs, &mut on_path);
        }

        for (def, use_site) in pairs {
            if def != use_site && existing.insert((def.0, use_site.0)) {
                plan.connect_unique(def, use_site, CpgEdgeKind::DataFlow(DfgEdgeKind::DefUse));
            }
        }
    }

    /// Builds parameter passing edges.
    fn build_parameter_edges(
        &self,
        cpg: &CodePropertyGraph,
        plan: &mut DfgPlan,
        function: NodeId,
        collector: &DefUseCollector,
    ) {
        // Parameter syntax is commonly nested below a grammar-specific list
        // wrapper. Select every parameter in this function's lexical AST scope
        // (but never a nested function) and restore source order explicitly.
        let scope = crate::analysis::function_scope_nodes(cpg, function);
        let mut params: Vec<NodeId> = scope
            .iter()
            .copied()
            .filter(|&id| {
                cpg.node(id)
                    .map(|n| matches!(n.kind, CpgNodeKind::Parameter { .. }))
                    .unwrap_or(false)
            })
            .collect();
        params.sort_unstable_by_key(|&id| {
            cpg.node(id)
                .map(|node| (node.range.start, node.range.end, id))
                .unwrap_or((u32::MAX, u32::MAX, id))
        });

        // Find call sites that call this function
        let callers = cpg.callers(function);

        for caller in callers {
            let call_args = call_arguments(cpg, caller);

            // Match arguments to parameters
            for (i, &param) in params.iter().enumerate() {
                if let Some(&arg) = call_args.get(i) {
                    plan.connect_unique(arg, param, CpgEdgeKind::DataFlow(DfgEdgeKind::Parameter));
                }
            }
        }

        // Also connect uses to their parameter definition. This is an
        // idempotent supplement to the primary `build_def_use_edges` sweep,
        // which already links parameter uses flow-sensitively. Deduplication
        // avoids double-linking those uses.
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
                            plan.connect_unique(
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
        cpg: &CodePropertyGraph,
        plan: &mut DfgPlan,
        function: NodeId,
        _collector: &DefUseCollector,
    ) {
        // Find return statements
        let scope = crate::analysis::function_scope_nodes(cpg, function);
        let mut returns: Vec<NodeId> = scope
            .iter()
            .copied()
            .filter(|&id| {
                cpg.node(id)
                    .map(|n| matches!(n.kind, CpgNodeKind::Return))
                    .unwrap_or(false)
            })
            .collect();
        returns.sort_unstable_by_key(|&id| {
            cpg.node(id)
                .map(|node| (node.range.start, node.range.end, id))
                .unwrap_or((u32::MAX, u32::MAX, id))
        });

        // Find call sites that call this function
        let callers = cpg.callers(function);

        for &ret in &returns {
            // Get the returned expression
            let ret_children = cpg.ast_children(ret);
            if let Some(&ret_expr) = ret_children.first() {
                for &caller in &callers {
                    plan.connect_unique(
                        ret_expr,
                        caller,
                        CpgEdgeKind::DataFlow(DfgEdgeKind::ReturnValue),
                    );
                }
            }
        }
    }

    /// Builds field/index read and write edges and resolves receiver-local
    /// member accesses to their declaring fields.
    ///
    /// Reads retain the historical direction (`receiver -> access`). For an
    /// assignment whose first AST child is a member or index l-value, the
    /// complementary write edge is `assignment -> access`. A `self.x` or
    /// `this.x` access inside a method is also linked to the uniquely matching
    /// `Field("x")` in its enclosing class/implemented type. Resolution is
    /// deliberately name- and lexical-scope-bounded: missing or ambiguous type
    /// and field evidence produces no `Reference` edge.
    fn build_field_access_edges(
        &self,
        cpg: &CodePropertyGraph,
        plan: &mut DfgPlan,
        collector: &DefUseCollector,
        field_index: &FieldResolutionIndex,
    ) {
        // Process member access nodes
        for &node_id in &collector.field_accesses {
            let children = cpg.ast_children(node_id);

            // First child is typically the object being accessed
            if let Some(&obj) = children.first() {
                // Create field read edge from object to member access
                plan.connect_unique(obj, node_id, CpgEdgeKind::DataFlow(DfgEdgeKind::FieldRead));
            }

            if let Some(field) = resolve_receiver_field(cpg, field_index, node_id) {
                plan.connect_unique(node_id, field, CpgEdgeKind::Reference);
            }
        }

        // Process index access nodes
        for &node_id in &collector.index_accesses {
            let children = cpg.ast_children(node_id);

            if children.len() >= 2 {
                let array = children[0];
                let index = children[1];

                // Create index read edges
                plan.connect_unique(
                    array,
                    node_id,
                    CpgEdgeKind::DataFlow(DfgEdgeKind::IndexRead),
                );
                plan.connect_unique(
                    index,
                    node_id,
                    CpgEdgeKind::DataFlow(DfgEdgeKind::DataDependency),
                );
            }
        }

        // An assignment's first AST child is its l-value. Complex l-values are
        // access sites rather than declaration sites, so retain the assignment
        // as the write origin and point at the same access node used by reads.
        for &assignment in &collector.assignments {
            let Some(target) = cpg.ast_children(assignment).first().copied() else {
                continue;
            };
            let write_kind = match cpg.node(target).map(|node| &node.kind) {
                Some(CpgNodeKind::MemberAccess { .. }) => DfgEdgeKind::FieldWrite,
                Some(CpgNodeKind::IndexAccess) => DfgEdgeKind::IndexWrite,
                _ => continue,
            };
            plan.connect_unique(assignment, target, CpgEdgeKind::DataFlow(write_kind));
        }
    }
}

/// Resolves `self.member` / `this.member` to one declaration in the enclosing
/// nominal type.
///
/// Parser frontends represent the receiver either as an `Identifier` (Python)
/// or as a named keyword retained as `Unknown { kind: "self" | "this" }`
/// (Rust, Java, JavaScript, and C++). Both shapes carry the same semantic
/// evidence and are accepted without consulting [`Language`].
fn resolve_receiver_field(
    cpg: &CodePropertyGraph,
    index: &FieldResolutionIndex,
    access: NodeId,
) -> Option<NodeId> {
    let member = match cpg.node(access).map(|node| &node.kind) {
        Some(CpgNodeKind::MemberAccess { member }) if !member.is_empty() => member.as_ref(),
        _ => return None,
    };

    let receiver = cpg.ast_children(access).first().copied()?;
    let receiver_is_local = match cpg.node(receiver).map(|node| &node.kind) {
        Some(CpgNodeKind::Identifier { name, .. }) => matches!(name.as_ref(), "self" | "this"),
        Some(CpgNodeKind::Unknown { kind }) => matches!(kind.as_ref(), "self" | "this"),
        _ => false,
    };
    if !receiver_is_local {
        return None;
    }

    // The access must first be inside a function; only an owner above that
    // function makes it a method rather than a coincidentally nested access.
    let ancestors = cpg.ast_ancestors(access);
    let method_index = ancestors.iter().position(|&id| {
        matches!(
            cpg.node(id).map(|node| &node.kind),
            Some(CpgNodeKind::Function { .. })
        )
    })?;
    let mut owner = None;
    for &ancestor in &ancestors[method_index + 1..] {
        match cpg.node(ancestor).map(|node| &node.kind) {
            Some(CpgNodeKind::Class { .. } | CpgNodeKind::Impl { .. }) => {
                owner = Some(ancestor);
                break;
            }
            // Reaching a second function means the nearest function is nested
            // inside a method, not itself owned by the nominal type.
            Some(CpgNodeKind::Function { .. }) => return None,
            _ => {}
        }
    }
    let owner = owner?;

    let owner = match cpg.node(owner).map(|node| &node.kind) {
        Some(CpgNodeKind::Class { .. }) => owner,
        Some(CpgNodeKind::Impl {
            for_type: Some(type_name),
            ..
        }) if !type_name.is_empty() => {
            index.unique_type(declaration_scope(cpg, owner), type_name)?
        }
        _ => return None,
    };

    index.unique_field(owner, member)
}

/// Returns the nearest enclosing namespace/type/root declaration. Comparing
/// node ids (not display names) keeps same-named types in sibling modules from
/// being conflated.
fn declaration_scope(cpg: &CodePropertyGraph, node: NodeId) -> Option<NodeId> {
    cpg.ast_ancestors(node).into_iter().find(|&ancestor| {
        matches!(
            cpg.node(ancestor).map(|node| &node.kind),
            Some(
                CpgNodeKind::Root
                    | CpgNodeKind::Module { .. }
                    | CpgNodeKind::Class { .. }
                    | CpgNodeKind::Struct { .. }
                    | CpgNodeKind::Enum { .. }
                    | CpgNodeKind::Trait { .. }
            )
        )
    })
}

/// Finds the nearest nominal owner of a field, but never crosses a function or
/// implementation boundary. Grammar wrapper nodes are transparent.
fn enclosing_field_owner(cpg: &CodePropertyGraph, field: NodeId) -> Option<NodeId> {
    for ancestor in cpg.ast_ancestors(field) {
        match cpg.node(ancestor).map(|node| &node.kind) {
            Some(
                CpgNodeKind::Class { .. } | CpgNodeKind::Struct { .. } | CpgNodeKind::Enum { .. },
            ) => return Some(ancestor),
            Some(CpgNodeKind::Function { .. } | CpgNodeKind::Impl { .. }) => return None,
            _ => {}
        }
    }
    None
}

/// Returns call arguments in source order without including callee syntax.
///
/// Tree-sitter grammars commonly retain one transparent argument-list wrapper
/// below a `Call`; Mode-B process-calculus mappers may instead flatten the
/// arguments directly. Supporting both shapes keeps the interprocedural DFG
/// language-agnostic.
pub(crate) fn call_arguments(cpg: &CodePropertyGraph, call: NodeId) -> Vec<NodeId> {
    let mut children = cpg.ast_children(call).to_vec();
    if children.is_empty() {
        return Vec::new();
    }
    children.sort_unstable_by_key(|&id| {
        cpg.node(id)
            .map(|node| (node.range.start, node.range.end, id))
            .unwrap_or((u32::MAX, u32::MAX, id))
    });
    children.remove(0);

    let mut arguments = Vec::new();
    for child in children {
        let is_container = cpg.node(child).is_some_and(|node| {
            matches!(
                &node.kind,
                CpgNodeKind::Unknown { kind }
                    if matches!(
                        kind.as_ref(),
                        "arguments"
                            | "argument_list"
                            | "arguments_list"
                            | "call_arguments"
                            | "actual_parameters"
                    )
            )
        });
        if is_container {
            arguments.extend(cpg.ast_children(child).iter().copied());
        } else {
            arguments.push(child);
        }
    }
    arguments.sort_unstable_by_key(|&id| {
        cpg.node(id)
            .map(|node| (node.range.start, node.range.end, id))
            .unwrap_or((u32::MAX, u32::MAX, id))
    });
    arguments
}

impl Default for DfgExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// Per-variable set of currently-reaching definition nodes, keyed by bound name.
/// `SmallVec` keeps the common single-definition value inline in its hash-map
/// bucket; multiple entries allocate only after a branch/loop merge.
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
    matches!(
        kind,
        CpgNodeKind::While | CpgNodeKind::For | CpgNodeKind::Loop
    )
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
    cpg.ast_children(node).iter().copied().find(|&c| {
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

/// Seeds a function's reaching environment from lexically preceding outer AST
/// siblings. This represents captured/global definitions (for example a
/// Rholang `new` channel surrounding multiple contracts) without folding a
/// sibling function's local definitions into the target function.
fn seed_lexical_environment(cpg: &CodePropertyGraph, function: NodeId, env: &mut ReachingEnv) {
    let mut path = vec![function];
    let mut cursor = function;
    let mut seen = FxHashSet::default();
    seen.insert(function);
    while let Some(parent) = lexical_parent(cpg, cursor) {
        if !seen.insert(parent) {
            break;
        }
        path.push(parent);
        cursor = parent;
    }
    path.reverse();
    let mut ignored_pairs = Vec::new();

    for pair in path.windows(2) {
        let parent = pair[0];
        let child_on_path = pair[1];
        let mut siblings = cpg.ast_children(parent).to_vec();
        siblings.sort_unstable_by_key(|&node| {
            cpg.node(node)
                .map(|node| (node.range.start, node.range.end, node.id))
                .unwrap_or((u32::MAX, u32::MAX, node))
        });
        for sibling in siblings {
            if sibling == child_on_path {
                break;
            }
            let mut on_path = FxHashSet::default();
            visit_reaching(cpg, sibling, env, false, &mut ignored_pairs, &mut on_path);
            ignored_pairs.clear();
        }
    }
}

fn lexical_parent(cpg: &CodePropertyGraph, node: NodeId) -> Option<NodeId> {
    if let Some(parent) = cpg.node(node).and_then(|node| node.parent) {
        return Some(parent);
    }
    cpg.native_incoming_edges(node)
        .filter_map(|edge| (edge.kind == CpgEdgeKind::AstChild).then_some(edge.source))
        .min()
}

/// One instruction in the stack-safe AST-ordered reaching-definitions machine.
///
/// `Enter` replaces a recursive call, `Bind` is the continuation that runs
/// after a definition's children, and `Exit` removes the node from the active
/// path. Keeping these phases distinct preserves evaluation order without
/// retaining source depth on the native call stack.
#[derive(Debug)]
enum ReachingFrame {
    Enter {
        node: NodeId,
        conditional: bool,
    },
    Bind {
        name: Arc<str>,
        definition: NodeId,
        conditional: bool,
    },
    Exit {
        node: NodeId,
    },
}

/// Core AST-ordered reaching-definitions pushdown machine. See
/// [`DfgExtractor::build_def_use_edges`] for the algorithm and its rationale.
fn visit_reaching(
    cpg: &CodePropertyGraph,
    node: NodeId,
    env: &mut ReachingEnv,
    conditional: bool,
    pairs: &mut Vec<(NodeId, NodeId)>,
    on_path: &mut FxHashSet<NodeId>,
) {
    let mut work = vec![ReachingFrame::Enter { node, conditional }];

    while let Some(frame) = work.pop() {
        match frame {
            ReachingFrame::Enter { node, conditional } => {
                let Some(kind) = cpg.node(node).map(|node| &node.kind) else {
                    continue;
                };

                // A malformed graph can close an `AstChild` cycle. The active
                // path is not a global visited set: a shared subtree is still
                // interpreted once per legitimate occurrence, and the second
                // loop pass can re-enter every node after its matching `Exit`.
                if !on_path.insert(node) {
                    continue;
                }
                work.push(ReachingFrame::Exit { node });

                match kind {
                    // Nested and lexically preceding functions own independent
                    // local environments.
                    CpgNodeKind::Function { .. } => {}

                    // Initializer/default children run before the definition
                    // becomes visible. Binder identifiers are not uses.
                    CpgNodeKind::Variable { name, .. } | CpgNodeKind::Parameter { name, .. } => {
                        let binder = binder_identifier(cpg, node, name);
                        work.push(ReachingFrame::Bind {
                            name: Arc::clone(name),
                            definition: node,
                            conditional,
                        });
                        for &child in cpg.ast_children(node).iter().rev() {
                            if Some(child) != binder {
                                work.push(ReachingFrame::Enter {
                                    node: child,
                                    conditional,
                                });
                            }
                        }
                    }

                    // The l-value rules distinguish pure writes, compound
                    // read-then-writes, and complex access sites exactly as the
                    // recursive equations did.
                    CpgNodeKind::Assignment { operator } => {
                        let compound = operator.as_ref() != "=";
                        let children = cpg.ast_children(node);
                        let target = children.first().copied();
                        let target_is_ident = matches!(
                            target
                                .and_then(|target| cpg.node(target))
                                .map(|node| &node.kind),
                            Some(CpgNodeKind::Identifier { .. })
                        );
                        if target_is_ident {
                            if let Some(name) =
                                target.and_then(|target| identifier_name(cpg, target))
                            {
                                work.push(ReachingFrame::Bind {
                                    name,
                                    definition: node,
                                    conditional,
                                });
                            }
                        }
                        for &child in children.iter().rev() {
                            if Some(child) != target || !target_is_ident || compound {
                                work.push(ReachingFrame::Enter {
                                    node: child,
                                    conditional,
                                });
                            }
                        }
                    }

                    // A use links every definition currently retained for its
                    // bound name. Its `Exit` remains on the work stack.
                    CpgNodeKind::Identifier { name, .. } => {
                        if let Some(definitions) = env.get(name) {
                            pairs.extend(definitions.iter().map(|&definition| (definition, node)));
                        }
                    }

                    // Reverse pushes preserve source order under LIFO. Loop
                    // regions push two identical child sweeps, implementing the
                    // bounded loop-carried-dependence approximation.
                    _ => {
                        let child_conditional = conditional || is_conditional_region(kind);
                        let passes = usize::from(is_loop_region(kind)) + 1;
                        let children = cpg.ast_children(node);
                        for _ in 0..passes {
                            for &child in children.iter().rev() {
                                work.push(ReachingFrame::Enter {
                                    node: child,
                                    conditional: child_conditional,
                                });
                            }
                        }
                    }
                }
            }
            ReachingFrame::Bind {
                name,
                definition,
                conditional,
            } => bind_definition(env, name, definition, conditional),
            ReachingFrame::Exit { node } => {
                on_path.remove(&node);
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
                            self.uses.entry(name.clone()).or_default().push(node_id);
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
        self.def_to_uses.entry(def).or_default().push(use_site);
        self.use_to_defs.entry(use_site).or_default().push(def);
    }

    /// Returns all uses that are reached by the given definition.
    pub fn uses_of(&self, def: NodeId) -> &[NodeId] {
        self.def_to_uses
            .get(&def)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Returns all definitions that reach the given use.
    pub fn definitions_of(&self, use_site: NodeId) -> &[NodeId] {
        self.use_to_defs
            .get(&use_site)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
}

/// Builds def-use chains for all variables in a function.
pub fn build_def_use_chains(
    cpg: &CodePropertyGraph,
    function: NodeId,
) -> FxHashMap<Arc<str>, DefUseChain> {
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
                let chain = chains
                    .entry(name.clone())
                    .or_insert_with(|| DefUseChain::new(name));
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
    use crate::{CpgNode, Language, MethodSignature, ScopeId, SourceRange, Visibility};

    fn create_test_function(cpg: &mut CodePropertyGraph) -> NodeId {
        let func = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Function {
                signature: Box::new(MethodSignature {
                    name: "test".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Public,
                }),
            },
            SourceRange::default(),
        ));

        let body = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
            SourceRange::default(),
        ));

        cpg.connect_unique(func, body, CpgEdgeKind::AstChild);

        func
    }

    #[test]
    fn test_dfg_extractor_basic() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = create_test_function(&mut cpg);
        let body = cpg.ast_children(func)[0];

        // Add variable declaration
        let var = cpg.add_node(
            CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::Variable {
                    name: "x".into(),
                    var_type: None,
                    scope: ScopeId::GLOBAL,
                    is_mutable: true,
                },
                SourceRange::default(),
            )
            .with_parent(body),
        );
        cpg.connect_unique(body, var, CpgEdgeKind::AstChild);

        // Add identifier reference (use)
        let ident = cpg.add_node(
            CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::Identifier {
                    name: "x".into(),
                    definition: Some(var),
                },
                SourceRange::default(),
            )
            .with_parent(body),
        );
        cpg.connect_unique(body, ident, CpgEdgeKind::AstChild);

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
        let var = cpg.add_node(
            CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::Variable {
                    name: "x".into(),
                    var_type: None,
                    scope: ScopeId::GLOBAL,
                    is_mutable: true,
                },
                SourceRange::default(),
            )
            .with_parent(body),
        );
        cpg.connect_unique(body, var, CpgEdgeKind::AstChild);

        // Add identifier reference
        let ident = cpg.add_node(
            CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::Identifier {
                    name: "x".into(),
                    definition: Some(var),
                },
                SourceRange::default(),
            )
            .with_parent(body),
        );
        cpg.connect_unique(body, ident, CpgEdgeKind::AstChild);

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
    fn nodes_where(cpg: &CodePropertyGraph, pred: impl Fn(&CpgNodeKind) -> bool) -> Vec<NodeId> {
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
        let v = nodes_where(
            cpg,
            |k| matches!(k, CpgNodeKind::Variable { name: n, .. } if &**n == name),
        );
        assert_eq!(v.len(), 1, "expected exactly one `let {name}`");
        v[0]
    }

    /// The identifier *use* of `name`: an `Identifier` node that actually
    /// received a reaching definition (i.e. it is not the binder of `name`).
    #[cfg(feature = "lang-rust")]
    fn ident_use(cpg: &CodePropertyGraph, name: &str) -> NodeId {
        nodes_where(
            cpg,
            |k| matches!(k, CpgNodeKind::Identifier { name: n, .. } if &**n == name),
        )
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
        let cpg =
            build_rust("fn foo(fd: i32) { let buf = read(fd); let out = decode(buf); sink(out); }");

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
            cpg.dfg_successors(buf_def)
                .iter()
                .any(|(t, _)| t == buf_use),
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
            cpg.dfg_successors(out_def)
                .iter()
                .any(|(t, _)| t == out_use),
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
            !cpg.dfg_successors(x_def).iter().any(|(t, _)| t == y_use),
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
        let init_use = nodes_where(
            &cpg,
            |k| matches!(k, CpgNodeKind::Identifier { name, .. } if &**name == "n"),
        )
        .into_iter()
        .find(|&id| cpg.reaching_definitions(id).contains(&param_n))
        .expect("initializer use of `n` bound to the parameter");
        assert!(!cpg.reaching_definitions(init_use).contains(&let_n));

        // `consume(n)` sees the shadowing `let n`.
        let consume_use = nodes_where(
            &cpg,
            |k| matches!(k, CpgNodeKind::Identifier { name, .. } if &**name == "n"),
        )
        .into_iter()
        .find(|&id| cpg.reaching_definitions(id).contains(&let_n))
        .expect("consume use of `n` bound to the shadowing let");
        assert!(consume_use != init_use);

        // Idempotency: a second extraction pass must not add any edge.
        let before = cpg.stats().dfg_edges;
        DfgExtractor::new().extract(&mut cpg);
        assert_eq!(before, cpg.stats().dfg_edges, "extract must be idempotent");
    }

    // ================================================================
    // Public-surface coverage: config, DefUseChain, Definition/Use,
    // build_def_use_chains, and aux-builder idempotency.
    // ================================================================

    fn dfg_child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
        let id = cpg.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()));
        cpg.connect(parent, id, CpgEdgeKind::AstChild);
        id
    }

    fn mk_function(cpg: &mut CodePropertyGraph, name: &str) -> NodeId {
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

    fn dfg_ident(name: &str) -> CpgNodeKind {
        CpgNodeKind::Identifier {
            name: name.into(),
            definition: None,
        }
    }

    fn function_kind(name: &str) -> CpgNodeKind {
        CpgNodeKind::Function {
            signature: Box::new(MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Public,
            }),
        }
    }

    #[test]
    fn nested_function_captures_lexically_preceding_outer_definition() {
        let mut cpg = CodePropertyGraph::new(Language::Rholang);
        let outer = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
            SourceRange::default(),
        ));
        let channel = dfg_child(
            &mut cpg,
            outer,
            CpgNodeKind::Variable {
                name: "channel".into(),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: false,
            },
        );
        let function = dfg_child(&mut cpg, outer, function_kind("contract"));
        let body = dfg_child(
            &mut cpg,
            function,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        let use_site = dfg_child(&mut cpg, body, dfg_ident("channel"));

        DfgExtractor::new().extract(&mut cpg);
        assert_eq!(cpg.reaching_definitions(use_site), vec![channel]);
    }

    #[test]
    fn preceding_sibling_function_locals_do_not_leak_into_lexical_seed() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let outer = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
            SourceRange::default(),
        ));
        let first = dfg_child(&mut cpg, outer, function_kind("first"));
        let first_body = dfg_child(
            &mut cpg,
            first,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        dfg_child(
            &mut cpg,
            first_body,
            CpgNodeKind::Variable {
                name: "private_local".into(),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: false,
            },
        );

        let second = dfg_child(&mut cpg, outer, function_kind("second"));
        let second_body = dfg_child(
            &mut cpg,
            second,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        let use_site = dfg_child(&mut cpg, second_body, dfg_ident("private_local"));

        DfgExtractor::new().extract(&mut cpg);
        assert!(cpg.reaching_definitions(use_site).is_empty());
    }

    fn has_typed_edge(
        cpg: &CodePropertyGraph,
        source: NodeId,
        target: NodeId,
        kind: &CpgEdgeKind,
    ) -> bool {
        cpg.edges_between(source, target)
            .into_iter()
            .any(|edge| &edge.kind == kind)
    }

    /// Complex assignment l-values receive write edges, while `self.x` is
    /// resolved only to the same-named field of the implementation's lexical
    /// type. The pass is additive and idempotent.
    #[test]
    fn field_and_index_writes_resolve_scoped_field_idempotently() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let root = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Root,
            SourceRange::default(),
        ));
        let module = dfg_child(&mut cpg, root, CpgNodeKind::Module { name: "m".into() });
        let ty = dfg_child(&mut cpg, module, CpgNodeKind::Struct { name: "S".into() });
        let field_list = dfg_child(
            &mut cpg,
            ty,
            CpgNodeKind::Unknown {
                kind: "field_declaration_list".into(),
            },
        );
        let field = dfg_child(
            &mut cpg,
            field_list,
            CpgNodeKind::Field {
                name: "x".into(),
                field_type: None,
                visibility: Visibility::Private,
            },
        );
        let implementation = dfg_child(
            &mut cpg,
            module,
            CpgNodeKind::Impl {
                for_type: Some("S".into()),
                trait_name: None,
            },
        );
        let method = dfg_child(
            &mut cpg,
            implementation,
            CpgNodeKind::Function {
                signature: Box::new(MethodSignature {
                    name: "set".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Public,
                }),
            },
        );
        let body = dfg_child(
            &mut cpg,
            method,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );

        let field_assignment = dfg_child(
            &mut cpg,
            body,
            CpgNodeKind::Assignment {
                operator: "=".into(),
            },
        );
        let member = dfg_child(
            &mut cpg,
            field_assignment,
            CpgNodeKind::MemberAccess { member: "x".into() },
        );
        let receiver = dfg_child(&mut cpg, member, dfg_ident("self"));
        dfg_child(
            &mut cpg,
            field_assignment,
            CpgNodeKind::Literal {
                kind: crate::LiteralKind::Integer(1),
            },
        );

        let index_assignment = dfg_child(
            &mut cpg,
            body,
            CpgNodeKind::Assignment {
                operator: "=".into(),
            },
        );
        let index_access = dfg_child(&mut cpg, index_assignment, CpgNodeKind::IndexAccess);
        let array = dfg_child(&mut cpg, index_access, dfg_ident("items"));
        let subscript = dfg_child(
            &mut cpg,
            index_access,
            CpgNodeKind::Literal {
                kind: crate::LiteralKind::Integer(0),
            },
        );
        dfg_child(
            &mut cpg,
            index_assignment,
            CpgNodeKind::Literal {
                kind: crate::LiteralKind::Integer(2),
            },
        );

        let mut disabled = cpg.clone();
        DfgExtractor::with_config(DfgExtractorConfig {
            include_field_access: false,
            ..DfgExtractorConfig::default()
        })
        .extract_function_dfg(&mut disabled, method);
        assert!(disabled.edges().all(|edge| !matches!(
            edge.kind,
            CpgEdgeKind::Reference
                | CpgEdgeKind::DataFlow(
                    DfgEdgeKind::FieldRead
                        | DfgEdgeKind::FieldWrite
                        | DfgEdgeKind::IndexRead
                        | DfgEdgeKind::IndexWrite
                        | DfgEdgeKind::DataDependency
                )
        )));

        let original_nodes = cpg.node_count();
        let original_edges = cpg.edge_count();
        DfgExtractor::new().extract_function_dfg(&mut cpg, method);

        assert_eq!(
            cpg.node_count(),
            original_nodes,
            "DFG extraction is append-only"
        );
        assert_eq!(
            cpg.edge_count(),
            original_edges + 6,
            "five DFG edges and one Reference are added"
        );
        assert_eq!(
            cpg.stats().dfg_edges,
            5,
            "Reference is not counted as a DFG edge"
        );
        assert!(has_typed_edge(
            &cpg,
            receiver,
            member,
            &CpgEdgeKind::DataFlow(DfgEdgeKind::FieldRead)
        ));
        assert!(has_typed_edge(
            &cpg,
            field_assignment,
            member,
            &CpgEdgeKind::DataFlow(DfgEdgeKind::FieldWrite)
        ));
        assert!(has_typed_edge(&cpg, member, field, &CpgEdgeKind::Reference));
        assert!(has_typed_edge(
            &cpg,
            array,
            index_access,
            &CpgEdgeKind::DataFlow(DfgEdgeKind::IndexRead)
        ));
        assert!(has_typed_edge(
            &cpg,
            subscript,
            index_access,
            &CpgEdgeKind::DataFlow(DfgEdgeKind::DataDependency)
        ));
        assert!(has_typed_edge(
            &cpg,
            index_assignment,
            index_access,
            &CpgEdgeKind::DataFlow(DfgEdgeKind::IndexWrite)
        ));

        let after_first = cpg.edge_count();
        DfgExtractor::new().extract_function_dfg(&mut cpg, method);
        assert_eq!(
            cpg.edge_count(),
            after_first,
            "the complete pass is idempotent"
        );
    }

    /// A class owns its fields directly (through transparent grammar wrappers),
    /// but unrelated receivers and duplicate declarations are not guessed.
    #[test]
    fn class_field_resolution_is_receiver_local_and_ambiguity_safe() {
        let mut cpg = CodePropertyGraph::new(Language::Java);
        let class = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Class {
                name: "C".into(),
                is_abstract: false,
            },
            SourceRange::default(),
        ));
        let wrapper = dfg_child(
            &mut cpg,
            class,
            CpgNodeKind::Unknown {
                kind: "class_body".into(),
            },
        );
        let field = dfg_child(
            &mut cpg,
            wrapper,
            CpgNodeKind::Field {
                name: "x".into(),
                field_type: None,
                visibility: Visibility::Private,
            },
        );
        let method = dfg_child(
            &mut cpg,
            wrapper,
            CpgNodeKind::Function {
                signature: Box::new(MethodSignature {
                    name: "m".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Public,
                }),
            },
        );
        let body = dfg_child(
            &mut cpg,
            method,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        let resolved = dfg_child(
            &mut cpg,
            body,
            CpgNodeKind::MemberAccess { member: "x".into() },
        );
        dfg_child(
            &mut cpg,
            resolved,
            CpgNodeKind::Unknown {
                kind: "this".into(),
            },
        );
        let unrelated = dfg_child(
            &mut cpg,
            body,
            CpgNodeKind::MemberAccess { member: "x".into() },
        );
        dfg_child(&mut cpg, unrelated, dfg_ident("other"));

        DfgExtractor::new().extract_function_dfg(&mut cpg, method);
        assert!(has_typed_edge(
            &cpg,
            resolved,
            field,
            &CpgEdgeKind::Reference
        ));
        assert!(!cpg
            .outgoing_edges(unrelated)
            .any(|edge| matches!(edge.kind, CpgEdgeKind::Reference)));

        // A duplicate declaration makes the name ambiguous, so a fresh access
        // added after the first pass must remain unresolved.
        dfg_child(
            &mut cpg,
            wrapper,
            CpgNodeKind::Field {
                name: "x".into(),
                field_type: None,
                visibility: Visibility::Private,
            },
        );
        let ambiguous = dfg_child(
            &mut cpg,
            body,
            CpgNodeKind::MemberAccess { member: "x".into() },
        );
        dfg_child(&mut cpg, ambiguous, dfg_ident("this"));
        DfgExtractor::new().extract_function_dfg(&mut cpg, method);
        assert!(!cpg
            .outgoing_edges(ambiguous)
            .any(|edge| matches!(edge.kind, CpgEdgeKind::Reference)));
    }

    /// Implementation owners resolve by exact type name in the implementation's
    /// lexical declaration scope. A same-named type in a sibling module is not
    /// a candidate, while two declarations in the same scope make resolution
    /// ambiguous. A function nested inside a method is not promoted to a method.
    #[test]
    fn impl_field_resolution_is_scope_bounded_and_method_local() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let root = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Root,
            SourceRange::default(),
        ));
        let left = dfg_child(
            &mut cpg,
            root,
            CpgNodeKind::Module {
                name: "left".into(),
            },
        );
        let right = dfg_child(
            &mut cpg,
            root,
            CpgNodeKind::Module {
                name: "right".into(),
            },
        );
        let left_type = dfg_child(&mut cpg, left, CpgNodeKind::Struct { name: "S".into() });
        let left_field = dfg_child(
            &mut cpg,
            left_type,
            CpgNodeKind::Field {
                name: "x".into(),
                field_type: None,
                visibility: Visibility::Private,
            },
        );
        let right_type = dfg_child(&mut cpg, right, CpgNodeKind::Struct { name: "S".into() });
        let right_field = dfg_child(
            &mut cpg,
            right_type,
            CpgNodeKind::Field {
                name: "x".into(),
                field_type: None,
                visibility: Visibility::Private,
            },
        );
        let implementation = dfg_child(
            &mut cpg,
            left,
            CpgNodeKind::Impl {
                for_type: Some("S".into()),
                trait_name: None,
            },
        );
        let method = dfg_child(
            &mut cpg,
            implementation,
            CpgNodeKind::Function {
                signature: Box::new(MethodSignature {
                    name: "method".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Private,
                }),
            },
        );
        let access = dfg_child(
            &mut cpg,
            method,
            CpgNodeKind::MemberAccess { member: "x".into() },
        );
        dfg_child(&mut cpg, access, dfg_ident("self"));

        let nested_function = dfg_child(
            &mut cpg,
            method,
            CpgNodeKind::Function {
                signature: Box::new(MethodSignature {
                    name: "nested".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Private,
                }),
            },
        );
        let nested_access = dfg_child(
            &mut cpg,
            nested_function,
            CpgNodeKind::MemberAccess { member: "x".into() },
        );
        dfg_child(&mut cpg, nested_access, dfg_ident("self"));

        DfgExtractor::new().extract_function_dfg(&mut cpg, method);
        assert!(has_typed_edge(
            &cpg,
            access,
            left_field,
            &CpgEdgeKind::Reference
        ));
        assert!(!has_typed_edge(
            &cpg,
            access,
            right_field,
            &CpgEdgeKind::Reference
        ));
        assert!(!cpg
            .outgoing_edges(nested_access)
            .any(|edge| matches!(edge.kind, CpgEdgeKind::Reference)));

        let duplicate_type = dfg_child(&mut cpg, left, CpgNodeKind::Struct { name: "S".into() });
        dfg_child(
            &mut cpg,
            duplicate_type,
            CpgNodeKind::Field {
                name: "x".into(),
                field_type: None,
                visibility: Visibility::Private,
            },
        );
        let ambiguous_access = dfg_child(
            &mut cpg,
            method,
            CpgNodeKind::MemberAccess { member: "x".into() },
        );
        dfg_child(&mut cpg, ambiguous_access, dfg_ident("self"));

        DfgExtractor::new().extract_function_dfg(&mut cpg, method);
        assert!(!cpg
            .outgoing_edges(ambiguous_access)
            .any(|edge| matches!(edge.kind, CpgEdgeKind::Reference)));
    }

    /// Every missing-evidence shape is a total, fail-closed lookup. This matrix
    /// distinguishes an absent receiver/function/owner/type/field from a valid
    /// resolution and exercises malformed access nodes without manufacturing a
    /// declaration edge.
    #[test]
    fn field_resolution_rejection_matrix_is_total() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let root = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Root,
            SourceRange::default(),
        ));

        // Empty declaration names and fields outside a nominal owner are not
        // indexed. A field below a function is likewise not type-owned.
        dfg_child(&mut cpg, root, CpgNodeKind::Enum { name: "".into() });
        cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Field {
                name: "orphan".into(),
                field_type: None,
                visibility: Visibility::Private,
            },
            SourceRange::default(),
        ));
        cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Field {
                name: "".into(),
                field_type: None,
                visibility: Visibility::Private,
            },
            SourceRange::default(),
        ));

        let free_function = dfg_child(
            &mut cpg,
            root,
            CpgNodeKind::Function {
                signature: Box::new(MethodSignature {
                    name: "free".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Private,
                }),
            },
        );
        dfg_child(
            &mut cpg,
            free_function,
            CpgNodeKind::Field {
                name: "local".into(),
                field_type: None,
                visibility: Visibility::Private,
            },
        );

        let bare_member = dfg_child(
            &mut cpg,
            free_function,
            CpgNodeKind::MemberAccess { member: "x".into() },
        );
        let empty_member = dfg_child(
            &mut cpg,
            free_function,
            CpgNodeKind::MemberAccess { member: "".into() },
        );
        dfg_child(&mut cpg, empty_member, dfg_ident("self"));
        let literal_receiver = dfg_child(
            &mut cpg,
            free_function,
            CpgNodeKind::MemberAccess { member: "x".into() },
        );
        dfg_child(
            &mut cpg,
            literal_receiver,
            CpgNodeKind::Literal {
                kind: crate::LiteralKind::Integer(0),
            },
        );
        let ownerless = dfg_child(
            &mut cpg,
            free_function,
            CpgNodeKind::MemberAccess { member: "x".into() },
        );
        dfg_child(&mut cpg, ownerless, dfg_ident("self"));

        let implementation = dfg_child(
            &mut cpg,
            root,
            CpgNodeKind::Impl {
                for_type: None,
                trait_name: None,
            },
        );
        let method = dfg_child(
            &mut cpg,
            implementation,
            CpgNodeKind::Function {
                signature: Box::new(MethodSignature {
                    name: "method".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Private,
                }),
            },
        );
        let unknown_type = dfg_child(
            &mut cpg,
            method,
            CpgNodeKind::MemberAccess { member: "x".into() },
        );
        dfg_child(&mut cpg, unknown_type, dfg_ident("self"));

        let malformed_index = dfg_child(&mut cpg, method, CpgNodeKind::IndexAccess);
        let childless_assignment = dfg_child(
            &mut cpg,
            method,
            CpgNodeKind::Assignment {
                operator: "=".into(),
            },
        );

        let index = FieldResolutionIndex::new(&cpg);
        for access in [
            bare_member,
            empty_member,
            literal_receiver,
            ownerless,
            unknown_type,
        ] {
            assert_eq!(resolve_receiver_field(&cpg, &index, access), None);
        }
        assert_eq!(resolve_receiver_field(&cpg, &index, root), None);
        assert_eq!(index.unique_type(Some(root), "missing"), None);
        assert_eq!(index.unique_field(root, "missing"), None);

        DfgExtractor::new().extract(&mut cpg);
        assert!(!cpg
            .outgoing_edges(malformed_index)
            .any(|edge| edge.kind.is_dfg()));
        assert!(!cpg
            .outgoing_edges(childless_assignment)
            .any(|edge| matches!(
                edge.kind,
                CpgEdgeKind::DataFlow(DfgEdgeKind::FieldWrite | DfgEdgeKind::IndexWrite)
            )));
        assert!(cpg
            .outgoing_edges(bare_member)
            .all(|edge| !matches!(edge.kind, CpgEdgeKind::Reference)));
    }

    /// Real Rust syntax exercises the complete parser -> mapper -> DFG path:
    /// `self.x = 1` has the write edge and resolves to `struct S::x`.
    #[test]
    #[cfg(feature = "lang-rust")]
    fn parsed_self_field_assignment_emits_write_and_reference() {
        let mut cpg =
            build_rust("struct S { x: i32 } impl S { fn set(&mut self) { self.x = 1; } }");
        let field = nodes_where(
            &cpg,
            |kind| matches!(kind, CpgNodeKind::Field { name, .. } if name.as_ref() == "x"),
        )[0];
        let member = nodes_where(
            &cpg,
            |kind| matches!(kind, CpgNodeKind::MemberAccess { member } if member.as_ref() == "x"),
        )[0];
        let assignment =
            nodes_where(&cpg, |kind| matches!(kind, CpgNodeKind::Assignment { .. }))[0];

        assert!(has_typed_edge(
            &cpg,
            assignment,
            member,
            &CpgEdgeKind::DataFlow(DfgEdgeKind::FieldWrite)
        ));
        assert!(has_typed_edge(&cpg, member, field, &CpgEdgeKind::Reference));

        let edges = cpg.edge_count();
        DfgExtractor::new().extract(&mut cpg);
        assert_eq!(cpg.edge_count(), edges);
    }

    /// `DfgExtractorConfig` field defaults and `DfgExtractor::with_config`.
    #[test]
    fn dfg_extractor_config_fields_and_with_config() {
        let d = DfgExtractorConfig::default();
        assert!(d.include_field_access);
        assert!(d.include_parameters);
        assert!(d.include_return_values);
        assert!(!d.track_aliases);
        assert_eq!(d.max_iterations, 100);

        let custom = DfgExtractorConfig {
            include_field_access: false,
            include_parameters: false,
            include_return_values: false,
            track_aliases: true,
            max_iterations: 7,
        };
        let extractor = DfgExtractor::with_config(custom);
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = create_test_function(&mut cpg);
        // Runs without panic even with every auxiliary pass disabled.
        extractor.extract(&mut cpg);
        let _ = func;
    }

    /// `DefUseChain`: `add_definition`/`add_use` deduplicate, `link` wires both
    /// maps, and `uses_of`/`definitions_of` return an empty slice for unknowns.
    #[test]
    fn def_use_chain_dedup_and_empty_lookups() {
        let mut chain = DefUseChain::new(Arc::from("v"));
        chain.add_definition(NodeId::new(1));
        chain.add_definition(NodeId::new(1)); // duplicate: ignored
        chain.add_definition(NodeId::new(2));
        assert_eq!(chain.definitions.len(), 2);

        chain.add_use(NodeId::new(3));
        chain.add_use(NodeId::new(3)); // duplicate: ignored
        assert_eq!(chain.uses.len(), 1);

        chain.link(NodeId::new(1), NodeId::new(3));
        assert_eq!(chain.uses_of(NodeId::new(1)), &[NodeId::new(3)]);
        assert_eq!(chain.definitions_of(NodeId::new(3)), &[NodeId::new(1)]);

        // Unknown def/use keys resolve to the empty slice, not a panic.
        assert!(chain.uses_of(NodeId::new(99)).is_empty());
        assert!(chain.definitions_of(NodeId::new(99)).is_empty());
        assert_eq!(&*chain.variable, "v");
    }

    /// `Definition` / `Use` construction across every `DefinitionKind`/`UseKind`
    /// variant, plus `Clone`/`PartialEq`.
    #[test]
    fn definition_and_use_construction() {
        let d = Definition {
            variable: Arc::from("x"),
            node: NodeId::new(1),
            kind: DefinitionKind::Declaration,
        };
        assert_eq!(&*d.variable, "x");
        assert_eq!(d.node, NodeId::new(1));
        assert_eq!(d.kind, DefinitionKind::Declaration);
        assert_eq!(d.clone(), d);

        for k in [
            DefinitionKind::Declaration,
            DefinitionKind::Assignment,
            DefinitionKind::Parameter,
            DefinitionKind::FieldWrite,
            DefinitionKind::IndexWrite,
        ] {
            let def = Definition {
                variable: Arc::from("v"),
                node: NodeId::new(0),
                kind: k,
            };
            assert_eq!(def.kind, k);
        }

        let u = Use {
            variable: Arc::from("y"),
            node: NodeId::new(2),
            kind: UseKind::Read,
        };
        assert_eq!(u.kind, UseKind::Read);
        assert_eq!(u.clone(), u);
        for k in [
            UseKind::Read,
            UseKind::FieldRead,
            UseKind::IndexRead,
            UseKind::Argument,
        ] {
            let us = Use {
                variable: Arc::from("v"),
                node: NodeId::new(0),
                kind: k,
            };
            assert_eq!(us.kind, k);
        }
    }

    /// `build_def_use_chains` collects the DFG's DefUse edges of a function into
    /// a per-variable chain linking the definition to its use.
    #[test]
    fn build_def_use_chains_links_def_to_use() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = mk_function(&mut cpg, "f");
        let body = dfg_child(
            &mut cpg,
            func,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        let var = dfg_child(
            &mut cpg,
            body,
            CpgNodeKind::Variable {
                name: "x".into(),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: true,
            },
        );
        let use_x = dfg_child(&mut cpg, body, dfg_ident("x"));

        DfgExtractor::new().extract(&mut cpg);
        let chains = build_def_use_chains(&cpg, func);
        let chain = chains.get("x").expect("a def-use chain for `x`");
        assert!(chain.definitions.contains(&var));
        assert!(chain.uses.contains(&use_x));
        assert_eq!(chain.uses_of(var), &[use_x]);
        assert_eq!(chain.definitions_of(use_x), &[var]);
    }

    /// The FIXED aux-builder idempotency: on a graph exercising parameter,
    /// return-value, and def-use edges (via a caller call-site), a second
    /// `extract` adds no new DFG edges.
    #[test]
    fn dfg_extract_idempotent_with_calls_params_returns() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = mk_function(&mut cpg, "f");
        // Parameter precedes the body in AST child order (like the real builder).
        let param = dfg_child(
            &mut cpg,
            func,
            CpgNodeKind::Parameter {
                name: "x".into(),
                param_type: None,
                is_variadic: false,
            },
        );
        let body = dfg_child(
            &mut cpg,
            func,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        // g(x)
        let call = dfg_child(
            &mut cpg,
            body,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        );
        let _callee = dfg_child(&mut cpg, call, dfg_ident("g"));
        let _call_arg = dfg_child(&mut cpg, call, dfg_ident("x"));
        // return x
        let ret = dfg_child(&mut cpg, body, CpgNodeKind::Return);
        let _ret_val = dfg_child(&mut cpg, ret, dfg_ident("x"));
        // A caller call-site so `callers(func)` is non-empty (drives the
        // parameter and return-value auxiliary edge builders).
        let caller = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
            SourceRange::default(),
        ));
        let _caller_arg = dfg_child(&mut cpg, caller, dfg_ident("arg0"));
        cpg.connect(caller, func, CpgEdgeKind::CallSite);

        DfgExtractor::new().extract(&mut cpg);
        let after_first = cpg.stats().dfg_edges;
        assert!(
            after_first > 0,
            "params/calls/returns must yield DFG edges; got {after_first}"
        );
        DfgExtractor::new().extract(&mut cpg);
        assert_eq!(
            after_first,
            cpg.stats().dfg_edges,
            "auxiliary DFG edge builders must be idempotent"
        );
        let _ = param;
    }

    /// Reaching-defs latest-write (straight-line, flow-sensitive): with two
    /// `let a` bindings, the use of `a` resolves to the LATEST definition and not
    /// the earlier, killed one.
    #[test]
    #[cfg(feature = "lang-rust")]
    fn reaching_defs_latest_write_wins() {
        let cpg = build_rust("fn f() { let a = 1; let a = 2; g(a); }");

        // Both `let a` bindings, sorted by id ⇒ source order (earliest first).
        let a_defs = nodes_where(
            &cpg,
            |k| matches!(k, CpgNodeKind::Variable { name, .. } if &**name == "a"),
        );
        assert_eq!(a_defs.len(), 2, "two `let a` bindings");
        let earliest = a_defs[0]; // let a = 1
        let latest = a_defs[1]; // let a = 2

        // The `a` inside `g(a)` (the only reaching-def use of `a`).
        let a_use = ident_use(&cpg, "a");
        let reaching = cpg.reaching_definitions(a_use);
        assert!(
            reaching.contains(&latest),
            "use must see the latest `let a = 2`"
        );
        assert!(
            !reaching.contains(&earliest),
            "use must NOT see the killed `let a = 1`"
        );
        assert_eq!(
            reaching.len(),
            1,
            "straight-line code ⇒ exactly one reaching def"
        );
    }
}

/// The auxiliary edge builders are individually switchable.
///
/// `DfgExtractorConfig` gates three *additional* edge families on top of the
/// reaching-definition core: parameter passing, return values, and field
/// access. Each flag must suppress exactly its own family and nothing else.
#[cfg(test)]
mod config_flags {
    use super::*;
    use crate::testutil::wf_child;
    use crate::{CpgNode, Language, MethodSignature, ScopeId, SourceRange, Visibility};

    /// `fn f(p) { g(p); return p; self.fld }` — a shape that triggers all three
    /// auxiliary families at once.
    fn rich_function() -> (CodePropertyGraph, NodeId) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = cpg.add_node(CpgNode::new(
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
        wf_child(
            &mut cpg,
            func,
            CpgNodeKind::Parameter {
                name: "p".into(),
                param_type: None,
                is_variadic: false,
            },
        );
        let body = wf_child(
            &mut cpg,
            func,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        let call = wf_child(
            &mut cpg,
            body,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        );
        wf_child(
            &mut cpg,
            call,
            CpgNodeKind::Identifier {
                name: "g".into(),
                definition: None,
            },
        );
        wf_child(
            &mut cpg,
            call,
            CpgNodeKind::Identifier {
                name: "p".into(),
                definition: None,
            },
        );
        let field = wf_child(
            &mut cpg,
            body,
            CpgNodeKind::MemberAccess {
                member: "fld".into(),
            },
        );
        wf_child(
            &mut cpg,
            field,
            CpgNodeKind::Identifier {
                name: "p".into(),
                definition: None,
            },
        );
        let ret = wf_child(&mut cpg, body, CpgNodeKind::Return);
        wf_child(
            &mut cpg,
            ret,
            CpgNodeKind::Identifier {
                name: "p".into(),
                definition: None,
            },
        );
        (cpg, func)
    }

    fn extract_with(config: DfgExtractorConfig) -> CodePropertyGraph {
        let (mut cpg, func) = rich_function();
        DfgExtractor::with_config(config).extract_function_dfg(&mut cpg, func);
        cpg
    }

    fn count_of(cpg: &CodePropertyGraph, kind: DfgEdgeKind) -> usize {
        cpg.edges()
            .filter(|e| matches!(&e.kind, CpgEdgeKind::DataFlow(k) if *k == kind))
            .count()
    }

    /// Turning a flag off removes that family and leaves the others intact.
    #[test]
    fn each_auxiliary_family_is_switchable() {
        let all = extract_with(DfgExtractorConfig::default());

        // Disabling parameters removes only `Parameter` edges.
        let no_params = extract_with(DfgExtractorConfig {
            include_parameters: false,
            ..DfgExtractorConfig::default()
        });
        assert_eq!(count_of(&no_params, DfgEdgeKind::Parameter), 0);
        assert_eq!(
            count_of(&no_params, DfgEdgeKind::ReturnValue),
            count_of(&all, DfgEdgeKind::ReturnValue),
            "return edges are unaffected"
        );

        // Disabling return values removes only `ReturnValue` edges.
        let no_returns = extract_with(DfgExtractorConfig {
            include_return_values: false,
            ..DfgExtractorConfig::default()
        });
        assert_eq!(count_of(&no_returns, DfgEdgeKind::ReturnValue), 0);
        assert_eq!(
            count_of(&no_returns, DfgEdgeKind::Parameter),
            count_of(&all, DfgEdgeKind::Parameter),
            "parameter edges are unaffected"
        );

        // Disabling field access removes only the field edges.
        let no_fields = extract_with(DfgExtractorConfig {
            include_field_access: false,
            ..DfgExtractorConfig::default()
        });
        assert_eq!(count_of(&no_fields, DfgEdgeKind::FieldRead), 0);
        assert_eq!(count_of(&no_fields, DfgEdgeKind::FieldWrite), 0);

        // With everything off, only the reaching-definition core remains.
        let core_only = extract_with(DfgExtractorConfig {
            include_parameters: false,
            include_return_values: false,
            include_field_access: false,
            ..DfgExtractorConfig::default()
        });
        assert_eq!(count_of(&core_only, DfgEdgeKind::Parameter), 0);
        assert_eq!(count_of(&core_only, DfgEdgeKind::ReturnValue), 0);
        assert_eq!(count_of(&core_only, DfgEdgeKind::FieldRead), 0);
        assert!(
            core_only.edge_count() < all.edge_count(),
            "the auxiliary families really did contribute edges"
        );
        // The def-use core is still there.
        assert!(count_of(&core_only, DfgEdgeKind::DefUse) > 0);
    }

    /// The config's defaults enable every family.
    #[test]
    fn defaults_enable_every_family() {
        let d = DfgExtractorConfig::default();
        assert!(d.include_parameters);
        assert!(d.include_return_values);
        assert!(d.include_field_access);
        assert_eq!(
            d.max_iterations, 100,
            "the retired fixed-point compatibility field keeps its stable default"
        );
    }
}

/// REGRESSION (malformed input): the former recursive `visit_reaching` could
/// descend a cyclic `AstChild` edge without bound and overflow the stack. The
/// pushdown machine's active-path set cuts the cycle. Found by the
/// `tests/robustness.rs` corruption suite.
#[cfg(test)]
mod cyclic_ast_regression {
    use super::*;
    use crate::{CpgNode, Language, MethodSignature, ScopeId, SourceRange, Visibility};

    #[test]
    fn reaching_definitions_terminate_on_a_cyclic_ast() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = cpg.add_node(CpgNode::new(
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
        let body = crate::testutil::wf_child(
            &mut cpg,
            func,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        let var = crate::testutil::wf_child(
            &mut cpg,
            body,
            CpgNodeKind::Variable {
                name: "x".into(),
                var_type: None,
                scope: ScopeId::GLOBAL,
                is_mutable: true,
            },
        );
        crate::testutil::wf_child(
            &mut cpg,
            body,
            CpgNodeKind::Identifier {
                name: "x".into(),
                definition: None,
            },
        );
        // Close a cycle: the variable claims the body as its own child.
        cpg.connect(var, body, CpgEdgeKind::AstChild);

        // Termination is the property under test.
        DfgExtractor::new().extract_function_dfg(&mut cpg, func);
        let chains = build_def_use_chains(&cpg, func);
        for chain in chains.values() {
            for def in chain.definitions.iter() {
                assert!(cpg.node(*def).is_some(), "definitions are real nodes");
            }
        }
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::testutil::*;
    use crate::{
        CfgExtractor, CpgNode, Language, LiteralKind, MethodSignature, ScopeId, SourceRange,
        Visibility,
    };
    use proptest::prelude::*;

    /// Bounded recursive specification retained only for differential tests.
    /// [`arb_cpg_raw`] supplies at most eight nodes, so this oracle's native
    /// recursion depth is fixed by the test generator rather than caller input.
    fn recursive_visit_reaching_oracle(
        cpg: &CodePropertyGraph,
        node: NodeId,
        env: &mut ReachingEnv,
        conditional: bool,
        pairs: &mut Vec<(NodeId, NodeId)>,
        on_path: &mut FxHashSet<NodeId>,
    ) {
        let Some(kind) = cpg.node(node).map(|node| node.kind.clone()) else {
            return;
        };
        if !on_path.insert(node) {
            return;
        }

        match &kind {
            CpgNodeKind::Function { .. } => {}
            CpgNodeKind::Variable { name, .. } | CpgNodeKind::Parameter { name, .. } => {
                let binder = binder_identifier(cpg, node, name);
                for &child in cpg.ast_children(node) {
                    if Some(child) != binder {
                        recursive_visit_reaching_oracle(
                            cpg,
                            child,
                            env,
                            conditional,
                            pairs,
                            on_path,
                        );
                    }
                }
                bind_definition(env, name.clone(), node, conditional);
            }
            CpgNodeKind::Assignment { operator } => {
                let compound = &**operator != "=";
                let children = cpg.ast_children(node);
                let target = children.first().copied();
                let target_is_ident = matches!(
                    target
                        .and_then(|target| cpg.node(target))
                        .map(|node| &node.kind),
                    Some(CpgNodeKind::Identifier { .. })
                );
                for &child in children {
                    if Some(child) != target || !target_is_ident || compound {
                        recursive_visit_reaching_oracle(
                            cpg,
                            child,
                            env,
                            conditional,
                            pairs,
                            on_path,
                        );
                    }
                }
                if target_is_ident {
                    if let Some(name) = target.and_then(|target| identifier_name(cpg, target)) {
                        bind_definition(env, name, node, conditional);
                    }
                }
            }
            CpgNodeKind::Identifier { name, .. } => {
                if let Some(definitions) = env.get(name) {
                    pairs.extend(definitions.iter().map(|&definition| (definition, node)));
                }
            }
            _ => {
                let child_conditional = conditional || is_conditional_region(&kind);
                let passes = usize::from(is_loop_region(&kind)) + 1;
                for _ in 0..passes {
                    for &child in cpg.ast_children(node) {
                        recursive_visit_reaching_oracle(
                            cpg,
                            child,
                            env,
                            child_conditional,
                            pairs,
                            on_path,
                        );
                    }
                }
            }
        }

        on_path.remove(&node);
    }

    fn seeded_reaching_environment(cpg: &CodePropertyGraph, seed: NodeId) -> ReachingEnv {
        let mut env = ReachingEnv::default();
        for node in cpg.nodes() {
            if let CpgNodeKind::Identifier { name, .. } = &node.kind {
                let definitions = env.entry(name.clone()).or_default();
                if !definitions.contains(&seed) {
                    definitions.push(seed);
                }
            }
        }
        env
    }

    fn fixed_field_graph(
        type_name: &str,
        field_name: &str,
        receiver: &str,
        receiver_as_keyword: bool,
        reverse_insertion: bool,
    ) -> CodePropertyGraph {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        cpg.add_node_with_id(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Root,
            SourceRange::default(),
        ));
        let method = MethodSignature {
            name: "set".into(),
            params: Default::default(),
            return_type: None,
            is_static: false,
            is_async: false,
            visibility: Visibility::Public,
        };
        let receiver_kind = if receiver_as_keyword {
            CpgNodeKind::Unknown {
                kind: receiver.into(),
            }
        } else {
            CpgNodeKind::Identifier {
                name: receiver.into(),
                definition: None,
            }
        };
        let mut nodes = vec![
            CpgNode::new(
                NodeId::new(1),
                CpgNodeKind::Struct {
                    name: type_name.into(),
                },
                SourceRange::default(),
            )
            .with_parent(NodeId::new(0)),
            CpgNode::new(
                NodeId::new(2),
                CpgNodeKind::Unknown {
                    kind: "field_declaration_list".into(),
                },
                SourceRange::default(),
            )
            .with_parent(NodeId::new(1)),
            CpgNode::new(
                NodeId::new(3),
                CpgNodeKind::Field {
                    name: field_name.into(),
                    field_type: None,
                    visibility: Visibility::Private,
                },
                SourceRange::default(),
            )
            .with_parent(NodeId::new(2)),
            CpgNode::new(
                NodeId::new(4),
                CpgNodeKind::Impl {
                    for_type: Some(type_name.into()),
                    trait_name: None,
                },
                SourceRange::default(),
            )
            .with_parent(NodeId::new(0)),
            CpgNode::new(
                NodeId::new(5),
                CpgNodeKind::Function {
                    signature: Box::new(method),
                },
                SourceRange::default(),
            )
            .with_parent(NodeId::new(4)),
            CpgNode::new(
                NodeId::new(6),
                CpgNodeKind::Block {
                    scope: ScopeId::GLOBAL,
                },
                SourceRange::default(),
            )
            .with_parent(NodeId::new(5)),
            CpgNode::new(
                NodeId::new(7),
                CpgNodeKind::Assignment {
                    operator: "=".into(),
                },
                SourceRange::default(),
            )
            .with_parent(NodeId::new(6)),
            CpgNode::new(
                NodeId::new(8),
                CpgNodeKind::MemberAccess {
                    member: field_name.into(),
                },
                SourceRange::default(),
            )
            .with_parent(NodeId::new(7)),
            CpgNode::new(NodeId::new(9), receiver_kind, SourceRange::default())
                .with_parent(NodeId::new(8)),
            CpgNode::new(
                NodeId::new(10),
                CpgNodeKind::Literal {
                    kind: LiteralKind::Integer(1),
                },
                SourceRange::default(),
            )
            .with_parent(NodeId::new(7)),
            CpgNode::new(
                NodeId::new(11),
                CpgNodeKind::Assignment {
                    operator: "=".into(),
                },
                SourceRange::default(),
            )
            .with_parent(NodeId::new(6)),
            CpgNode::new(
                NodeId::new(12),
                CpgNodeKind::IndexAccess,
                SourceRange::default(),
            )
            .with_parent(NodeId::new(11)),
            CpgNode::new(
                NodeId::new(13),
                CpgNodeKind::Identifier {
                    name: "items".into(),
                    definition: None,
                },
                SourceRange::default(),
            )
            .with_parent(NodeId::new(12)),
            CpgNode::new(
                NodeId::new(14),
                CpgNodeKind::Literal {
                    kind: LiteralKind::Integer(0),
                },
                SourceRange::default(),
            )
            .with_parent(NodeId::new(12)),
            CpgNode::new(
                NodeId::new(15),
                CpgNodeKind::Literal {
                    kind: LiteralKind::Integer(2),
                },
                SourceRange::default(),
            )
            .with_parent(NodeId::new(11)),
        ];
        if reverse_insertion {
            nodes.reverse();
        }
        for node in nodes {
            cpg.add_node_with_id(node);
        }
        for (parent, child) in [
            (0, 1),
            (1, 2),
            (2, 3),
            (0, 4),
            (4, 5),
            (5, 6),
            (6, 7),
            (7, 8),
            (8, 9),
            (7, 10),
            (6, 11),
            (11, 12),
            (12, 13),
            (12, 14),
            (11, 15),
        ] {
            cpg.connect(
                NodeId::new(parent),
                NodeId::new(child),
                CpgEdgeKind::AstChild,
            );
        }
        cpg
    }

    fn canonical_field_edges(cpg: &CodePropertyGraph) -> Vec<(u32, u32, u8)> {
        let mut edges: Vec<_> = cpg
            .edges()
            .filter_map(|edge| {
                let tag = match edge.kind {
                    CpgEdgeKind::Reference => 0,
                    CpgEdgeKind::DataFlow(DfgEdgeKind::FieldRead) => 1,
                    CpgEdgeKind::DataFlow(DfgEdgeKind::FieldWrite) => 2,
                    CpgEdgeKind::DataFlow(DfgEdgeKind::IndexRead) => 3,
                    CpgEdgeKind::DataFlow(DfgEdgeKind::IndexWrite) => 4,
                    CpgEdgeKind::DataFlow(DfgEdgeKind::DataDependency) => 5,
                    _ => return None,
                };
                Some((edge.source.0, edge.target.0, tag))
            })
            .collect();
        edges.sort_unstable();
        edges
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// The typed heap machine is the exact defunctionalization of the
        /// previous recursive equations, including pair order, environment
        /// updates, two-pass loops, nested-function cuts, and active-path cycle
        /// behavior.
        #[test]
        fn prop_reaching_machine_matches_bounded_recursive_oracle(
            cpg in arb_cpg_raw(),
            conditional in any::<bool>(),
        ) {
            let root = cpg.node_ids().min().expect("raw CPG is non-empty");
            let initial = seeded_reaching_environment(&cpg, root);
            let mut actual_env = initial.clone();
            let mut expected_env = initial;
            let mut actual_pairs = Vec::new();
            let mut expected_pairs = Vec::new();
            let mut actual_path = FxHashSet::default();
            let mut expected_path = FxHashSet::default();

            visit_reaching(
                &cpg,
                root,
                &mut actual_env,
                conditional,
                &mut actual_pairs,
                &mut actual_path,
            );
            recursive_visit_reaching_oracle(
                &cpg,
                root,
                &mut expected_env,
                conditional,
                &mut expected_pairs,
                &mut expected_path,
            );

            prop_assert_eq!(actual_pairs, expected_pairs);
            prop_assert_eq!(actual_env, expected_env);
            prop_assert_eq!(&actual_path, &expected_path);
            prop_assert!(actual_path.is_empty());
        }

        /// Field/index extraction depends only on semantic shape, not petgraph
        /// node insertion order. Both receiver representations accepted from
        /// Mode-A frontends produce the same six canonical edges.
        #[test]
        fn prop_field_edges_are_deterministic(
            type_name in arb_name(),
            field_name in arb_name(),
            receiver in prop_oneof![Just("self"), Just("this")],
            receiver_as_keyword in any::<bool>(),
        ) {
            let mut forward = fixed_field_graph(
                &type_name,
                &field_name,
                receiver,
                receiver_as_keyword,
                false,
            );
            let mut reversed = fixed_field_graph(
                &type_name,
                &field_name,
                receiver,
                receiver_as_keyword,
                true,
            );
            DfgExtractor::new().extract_function_dfg(&mut forward, NodeId::new(5));
            DfgExtractor::new().extract_function_dfg(&mut reversed, NodeId::new(5));

            let expected = vec![
                (7, 8, 2),
                (8, 3, 0),
                (9, 8, 1),
                (11, 12, 4),
                (13, 12, 3),
                (14, 12, 5),
            ];
            prop_assert_eq!(canonical_field_edges(&forward), expected.clone());
            prop_assert_eq!(canonical_field_edges(&reversed), expected);
        }

        /// On a well-formed CPG (with calls/returns/branches), a second
        /// `DfgExtractor::extract` leaves the DFG edge count unchanged — the
        /// fixed aux-builder idempotency.
        #[test]
        fn prop_dfg_extract_idempotent(cpg in arb_well_formed_cpg()) {
            let mut cpg = cpg;
            CfgExtractor::new().extract(&mut cpg);
            DfgExtractor::new().extract(&mut cpg);
            let after_first = cpg.stats().dfg_edges;
            DfgExtractor::new().extract(&mut cpg);
            prop_assert_eq!(after_first, cpg.stats().dfg_edges);
        }

        /// Grammar argument/parameter wrappers must not shift positional
        /// interprocedural flow. The property covers every arity in the
        /// configured range and also places an excluded nested-function
        /// parameter beside the real parameters.
        #[test]
        fn prop_wrapped_arguments_match_function_scoped_parameters(
            arity in 0usize..16,
        ) {
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            let callee = cpg.add_node(CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::Function {
                    signature: Box::new(MethodSignature {
                        name: "callee".into(),
                        params: Default::default(),
                        return_type: None,
                        is_static: false,
                        is_async: false,
                        visibility: Visibility::Private,
                    }),
                },
                SourceRange::default(),
            ));
            let parameter_list = wf_child(
                &mut cpg,
                callee,
                CpgNodeKind::Unknown {
                    kind: "parameters".into(),
                },
            );
            let mut parameters = Vec::with_capacity(arity);
            for index in 0..arity {
                parameters.push(wf_child(
                    &mut cpg,
                    parameter_list,
                    CpgNodeKind::Parameter {
                        name: format!("p{index}").into(),
                        param_type: None,
                        is_variadic: false,
                    },
                ));
            }

            let nested = wf_child(
                &mut cpg,
                callee,
                CpgNodeKind::Function {
                    signature: Box::new(MethodSignature {
                        name: "nested".into(),
                        params: Default::default(),
                        return_type: None,
                        is_static: false,
                        is_async: false,
                        visibility: Visibility::Private,
                    }),
                },
            );
            let excluded_parameter = wf_child(
                &mut cpg,
                nested,
                CpgNodeKind::Parameter {
                    name: "excluded".into(),
                    param_type: None,
                    is_variadic: false,
                },
            );

            let call = cpg.add_node(CpgNode::new(
                NodeId::new(0),
                CpgNodeKind::Call {
                    target: Some(callee),
                    is_method: false,
                },
                SourceRange::default(),
            ));
            wf_child(
                &mut cpg,
                call,
                CpgNodeKind::Identifier {
                    name: "callee".into(),
                    definition: Some(callee),
                },
            );
            let argument_list = wf_child(
                &mut cpg,
                call,
                CpgNodeKind::Unknown {
                    kind: "arguments".into(),
                },
            );
            let mut arguments = Vec::with_capacity(arity);
            for index in 0..arity {
                arguments.push(wf_child(
                    &mut cpg,
                    argument_list,
                    CpgNodeKind::Literal {
                        kind: LiteralKind::Integer(index as i64),
                    },
                ));
            }
            cpg.connect(call, callee, CpgEdgeKind::StaticCall);
            cpg.connect(call, callee, CpgEdgeKind::CallSite);

            DfgExtractor::new().extract_function_dfg(&mut cpg, callee);

            let actual: FxHashSet<_> = cpg
                .edges()
                .filter_map(|edge| match edge.kind {
                    CpgEdgeKind::DataFlow(DfgEdgeKind::Parameter) => {
                        Some((edge.source, edge.target))
                    }
                    _ => None,
                })
                .collect();
            let expected: FxHashSet<_> = arguments
                .into_iter()
                .zip(parameters.into_iter())
                .collect();
            prop_assert_eq!(actual, expected);
            prop_assert!(cpg
                .incoming_edges(excluded_parameter)
                .all(|edge| !matches!(
                    edge.kind,
                    CpgEdgeKind::DataFlow(DfgEdgeKind::Parameter)
                )));
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        /// Parallel DFG planning produces the exact serial edge stream for
        /// generated calls, returns, branches, fields, and every auxiliary-edge
        /// configuration. Workers run with a 256 KiB stack to preserve the
        /// reaching machine's heap-bounded native stack usage.
        #[test]
        fn prop_parallel_dfg_is_exactly_serial(
            cpg in arb_well_formed_cpg(),
            mask in 0_u8..16,
        ) {
            let config = DfgExtractorConfig {
                include_field_access: mask & 1 != 0,
                include_parameters: mask & 2 != 0,
                include_return_values: mask & 4 != 0,
                track_aliases: mask & 8 != 0,
                max_iterations: 100,
            };
            let mut serial = cpg.clone();
            let mut parallel = cpg;
            DfgExtractor::with_config(config.clone()).extract(&mut serial);
            let pool = rayon::ThreadPoolBuilder::new()
                .num_threads(4)
                .stack_size(256 * 1024)
                .build()
                .expect("bounded Rayon pool");
            pool.install(|| {
                DfgExtractor::with_config(config).extract_parallel(&mut parallel);
            });

            prop_assert_eq!(
                parallel.edges().cloned().collect::<Vec<_>>(),
                serial.edges().cloned().collect::<Vec<_>>(),
            );
        }
    }
}
