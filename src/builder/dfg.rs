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
        // Phase 1: Collect all definitions and uses (takes ownership of collected data)
        let collector = {
            let mut c = DefUseCollector::new(function);
            c.collect(cpg);
            c
        };

        // Phase 2: Compute reaching definitions
        let reaching_defs = self.compute_reaching_definitions(cpg, function, &collector);

        // Phase 3: Build def-use edges
        self.build_def_use_edges(cpg, &collector, &reaching_defs);

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

    /// Computes reaching definitions using iterative dataflow analysis.
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

    /// Builds def-use edges based on reaching definitions.
    fn build_def_use_edges(
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

        // Also connect uses of parameters within the function
        for &param in &params {
            let param_name = cpg.node(param).and_then(|n| n.name().map(Arc::from));
            if let Some(name) = param_name {
                if let Some(uses) = collector.uses.get(&name) {
                    for &use_site in uses {
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
                                .or_insert_with(SmallVec::new)
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
            .or_insert_with(Vec::new)
            .push(use_site);
        self.use_to_defs
            .entry(use_site)
            .or_insert_with(Vec::new)
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
}
