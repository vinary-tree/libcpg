//! Deterministic, evidence-bounded call-target resolution.
//!
//! Resolution is an explicit post-pass. It builds a lexical symbol table,
//! records scope relations, and materializes call topology only when the
//! available evidence identifies one target. Ambiguous and unresolved calls
//! remain out-of-graph records so they cannot fabricate call-graph structure.

use std::collections::{BTreeMap, BTreeSet};

use crate::{CodePropertyGraph, CpgEdge, CpgEdgeKind, CpgNodeKind, EdgeId, NodeId};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Strength of the evidence supporting a call resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum Confidence {
    /// A unique declaration was found through the call's lexical scope chain.
    ExactInFile,
    /// A visible import and one in-graph declaration identify the target.
    ExactViaImport,
    /// The input CPG already carried a valid target binding.
    ExactViaBinding,
    /// Variable-type analysis identified the represented runtime targets.
    VariableType,
    /// Rapid-type analysis identified the represented allocated targets.
    RapidType,
    /// Class-hierarchy analysis identified the represented subtype targets.
    ClassHierarchy,
    /// Exactly one in-graph function has the reconstructed bare name.
    BareNameUnique,
    /// Multiple in-graph functions share the reconstructed bare name.
    BareNameAmbiguous,
    /// A visible import names a target that is not represented in this CPG.
    External,
    /// No supported evidence identifies a target.
    Unresolved,
}

impl Confidence {
    /// Returns the stable evidence score used by downstream ranking.
    pub const fn score(self) -> f32 {
        match self {
            Self::ExactInFile => 1.0,
            Self::ExactViaImport => 0.95,
            Self::ExactViaBinding => 0.9,
            Self::VariableType => 0.85,
            Self::RapidType => 0.8,
            Self::ClassHierarchy => 0.75,
            Self::BareNameUnique => 0.7,
            Self::BareNameAmbiguous => 0.3,
            Self::External | Self::Unresolved => 0.0,
        }
    }

    /// Returns whether this evidence is strong enough to create call topology.
    pub const fn creates_topology(self) -> bool {
        matches!(
            self,
            Self::ExactInFile
                | Self::ExactViaImport
                | Self::ExactViaBinding
                | Self::VariableType
                | Self::RapidType
                | Self::ClassHierarchy
                | Self::BareNameUnique
        )
    }

    /// Returns the stable label written to emitted resolution edges.
    pub const fn label(self) -> &'static str {
        match self {
            Self::ExactInFile => "exact_in_file",
            Self::ExactViaImport => "exact_via_import",
            Self::ExactViaBinding => "exact_via_binding",
            Self::VariableType => "variable_type",
            Self::RapidType => "rapid_type",
            Self::ClassHierarchy => "class_hierarchy",
            Self::BareNameUnique => "bare_name_unique",
            Self::BareNameAmbiguous => "bare_name_ambiguous",
            Self::External => "external",
            Self::Unresolved => "unresolved",
        }
    }

    fn from_label(label: &str) -> Option<Self> {
        match label {
            "exact_in_file" => Some(Self::ExactInFile),
            "exact_via_import" => Some(Self::ExactViaImport),
            "exact_via_binding" => Some(Self::ExactViaBinding),
            "variable_type" => Some(Self::VariableType),
            "rapid_type" => Some(Self::RapidType),
            "class_hierarchy" => Some(Self::ClassHierarchy),
            "bare_name_unique" => Some(Self::BareNameUnique),
            "bare_name_ambiguous" => Some(Self::BareNameAmbiguous),
            "external" => Some(Self::External),
            "unresolved" => Some(Self::Unresolved),
            _ => None,
        }
    }

    const fn rank(self) -> u8 {
        match self {
            Self::ExactInFile => 9,
            Self::ExactViaImport => 8,
            Self::ExactViaBinding => 7,
            Self::VariableType => 6,
            Self::RapidType => 5,
            Self::ClassHierarchy => 4,
            Self::BareNameUnique => 3,
            Self::BareNameAmbiguous => 2,
            Self::External => 1,
            Self::Unresolved => 0,
        }
    }
}

/// Resource limits and method-resolution policy for one resolution pass.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ResolveConfig {
    /// Maximum lexical-scope transitions and bounded AST-name search depth.
    pub max_scope_depth: u32,
    /// Maximum candidate identifiers retained for any one name.
    pub max_candidates_per_name: usize,
    /// Maximum number of call nodes processed in ascending `NodeId` order.
    pub max_calls: usize,
    /// Whether unresolved method calls may be matched to in-graph methods.
    pub resolve_methods: bool,
}

impl Default for ResolveConfig {
    fn default() -> Self {
        Self {
            max_scope_depth: 128,
            max_candidates_per_name: 64,
            max_calls: 100_000,
            resolve_methods: true,
        }
    }
}

/// Bounded declaration candidates for one symbol name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SymbolCandidates {
    /// Sorted retained candidate node identifiers.
    pub nodes: Vec<NodeId>,
    /// Total distinct candidates observed before retention was capped.
    pub total: usize,
}

impl SymbolCandidates {
    /// Returns whether some candidate identifiers were omitted by the cap.
    pub fn is_truncated(&self) -> bool {
        self.nodes.len() < self.total
    }

    fn insert(&mut self, node: NodeId, cap: usize) {
        self.total = self.total.saturating_add(1);
        if self.nodes.len() < cap {
            self.nodes.push(node);
            self.nodes.sort_unstable();
            self.nodes.dedup();
        }
    }
}

/// Deterministic lexical function and import index for one CPG.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SymbolTable {
    /// Nearest containing lexical scope for each represented non-root node.
    pub enclosing_scope: BTreeMap<NodeId, NodeId>,
    /// Function declarations grouped by containing scope and name.
    pub functions_by_scope: BTreeMap<NodeId, BTreeMap<String, SymbolCandidates>>,
    /// Function declarations grouped globally by bare name.
    pub functions_by_name: BTreeMap<String, SymbolCandidates>,
    /// Import nodes grouped by containing scope and terminal path name.
    pub imports_by_scope: BTreeMap<NodeId, BTreeMap<String, SymbolCandidates>>,
    /// Imported function declarations grouped by containing scope and name.
    ///
    /// These candidates come from declaration-level `Imports` edges emitted by
    /// [`crate::build_import_graph`]. Unlike the global bare-name index, they
    /// retain which represented file an import actually selected.
    pub imported_functions_by_scope: BTreeMap<NodeId, BTreeMap<String, SymbolCandidates>>,
}

/// Authoritative out-of-graph record for one call node.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CallResolution {
    /// Reconstructed callee name, when the AST exposes one.
    pub callee_name: Option<String>,
    /// Nearest lexical scope used to start resolution.
    pub scope: Option<NodeId>,
    /// Evidence tier selected for this call.
    pub confidence: Confidence,
    /// Resolved target when [`Confidence::creates_topology`] is true.
    pub target: Option<NodeId>,
    /// Sorted retained function candidates.
    pub candidates: Vec<NodeId>,
    /// Total candidate count before the configured retention cap.
    pub candidate_count: usize,
    /// Whether `candidates` was truncated by `max_candidates_per_name`.
    pub candidates_truncated: bool,
}

/// Aggregate result of a bounded call-resolution post-pass.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CallGraphResolution {
    /// Resolution record for each processed call, ordered by `NodeId`.
    pub resolutions: BTreeMap<NodeId, CallResolution>,
    /// New `StaticCall`/`DynamicCall`/`CallSite` edges emitted by this pass.
    pub topology_edges: usize,
    /// Calls classified as [`Confidence::BareNameAmbiguous`].
    pub ambiguous: usize,
    /// Calls classified as [`Confidence::Unresolved`].
    pub unresolved: usize,
    /// Calls classified as [`Confidence::External`].
    pub external: usize,
}

/// Builds a deterministic lexical symbol table and materializes scope edges.
///
/// Scope nodes point to their nearest parent scope with `EnclosingScope`.
/// Every other contained node points to its nearest scope with `ContainedIn`.
/// Both relations use `connect_unique`, so repeated builds add no edges.
pub fn build_symbol_table(cpg: &mut CodePropertyGraph, cfg: &ResolveConfig) -> SymbolTable {
    let mut node_ids: Vec<_> = cpg.node_ids().collect();
    node_ids.sort_unstable();
    let scope_nodes: BTreeSet<_> = node_ids
        .iter()
        .copied()
        .filter(|&node| cpg.node(node).is_some_and(|node| is_scope(&node.kind)))
        .collect();

    let mut table = SymbolTable::default();
    let mut scope_edges = Vec::new();
    let mut imported_functions: BTreeMap<NodeId, BTreeMap<String, BTreeSet<NodeId>>> =
        BTreeMap::new();
    for &node in &node_ids {
        let enclosing = nearest_scope(cpg, node, &scope_nodes, cfg.max_scope_depth);
        if let Some(scope) = enclosing {
            table.enclosing_scope.insert(node, scope);
            let kind = if scope_nodes.contains(&node) {
                CpgEdgeKind::EnclosingScope
            } else {
                CpgEdgeKind::ContainedIn
            };
            scope_edges.push((node, scope, kind));
        }

        let Some(cpg_node) = cpg.node(node) else {
            continue;
        };
        match &cpg_node.kind {
            CpgNodeKind::Function { signature } => {
                insert_named_candidate(
                    &mut table.functions_by_name,
                    signature.name.as_ref(),
                    node,
                    cfg.max_candidates_per_name,
                );
                if let Some(scope) = enclosing {
                    insert_scoped_candidate(
                        &mut table.functions_by_scope,
                        scope,
                        signature.name.as_ref(),
                        node,
                        cfg.max_candidates_per_name,
                    );
                }
            }
            CpgNodeKind::Import { path } => {
                if let Some(scope) = enclosing {
                    let mut represented_names = BTreeSet::new();
                    let mut targets: Vec<_> = cpg
                        .native_outgoing_edges(node)
                        .filter(|edge| matches!(edge.kind, CpgEdgeKind::Imports))
                        .filter_map(|edge| {
                            let target = cpg.node(edge.target)?;
                            match &target.kind {
                                CpgNodeKind::Function { signature } => Some((
                                    edge.label
                                        .as_deref()
                                        .filter(|name| !name.is_empty())
                                        .unwrap_or(signature.name.as_ref())
                                        .to_owned(),
                                    edge.target,
                                )),
                                _ => None,
                            }
                        })
                        .collect();
                    targets.sort();
                    targets.dedup();
                    for (name, target) in targets {
                        represented_names.insert(name.clone());
                        imported_functions
                            .entry(scope)
                            .or_default()
                            .entry(name.clone())
                            .or_default()
                            .insert(target);
                        insert_scoped_candidate(
                            &mut table.imports_by_scope,
                            scope,
                            &name,
                            node,
                            cfg.max_candidates_per_name,
                        );
                    }
                    if let Some(name) = terminal_name(path) {
                        if represented_names.insert(name.clone()) {
                            insert_scoped_candidate(
                                &mut table.imports_by_scope,
                                scope,
                                &name,
                                node,
                                cfg.max_candidates_per_name,
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }

    for (scope, symbols) in imported_functions {
        for (name, targets) in symbols {
            for target in targets {
                insert_scoped_candidate(
                    &mut table.imported_functions_by_scope,
                    scope,
                    &name,
                    target,
                    cfg.max_candidates_per_name,
                );
            }
        }
    }

    scope_edges.sort_by_key(|(source, target, kind)| (*source, *target, edge_kind_rank(kind)));
    scope_edges.dedup();
    for (source, target, kind) in scope_edges {
        cpg.connect_unique(source, target, kind);
    }
    table
}

/// Resolves calls and emits only topology supported by unique evidence.
///
/// Calls are processed in ascending `NodeId` order and capped by
/// [`ResolveConfig::max_calls`]. Ambiguous, external, and unresolved records do
/// not set `Call::target` and do not emit `Reference` or call-graph edges.
pub fn resolve_calls(cpg: &mut CodePropertyGraph, cfg: &ResolveConfig) -> CallGraphResolution {
    let table = build_symbol_table(cpg, cfg);
    let mut calls: Vec<_> = cpg.calls().map(|node| node.id).collect();
    calls.sort_unstable();
    calls.truncate(cfg.max_calls);

    let mut result = CallGraphResolution::default();
    for call in calls {
        let (is_method, original_target) = match cpg.node(call).map(|node| &node.kind) {
            Some(CpgNodeKind::Call { target, is_method }) => (*is_method, *target),
            _ => continue,
        };
        let scope = table.enclosing_scope.get(&call).copied();
        let callee_name = callee_name(cpg, call, cfg.max_scope_depth);
        let evidence = CallEvidence {
            call,
            scope,
            name: callee_name.as_deref(),
            is_method,
            original_target,
        };
        let decision = decide_call(cpg, &table, cfg, &evidence);

        match decision.confidence {
            Confidence::BareNameAmbiguous => result.ambiguous = result.ambiguous.saturating_add(1),
            Confidence::External => result.external = result.external.saturating_add(1),
            Confidence::Unresolved => result.unresolved = result.unresolved.saturating_add(1),
            _ => {}
        }

        if decision.confidence.creates_topology() {
            if let Some(target) = decision.target {
                if original_target != Some(target) {
                    if let Some(CpgNodeKind::Call { target: slot, .. }) =
                        cpg.node_mut(call).map(|node| &mut node.kind)
                    {
                        *slot = Some(target);
                    }
                }
                let label = decision.confidence.label();
                connect_labeled_unique(cpg, call, target, CpgEdgeKind::Reference, label);
                let call_kind = if is_method {
                    CpgEdgeKind::DynamicCall
                } else {
                    CpgEdgeKind::StaticCall
                };
                result.topology_edges =
                    result
                        .topology_edges
                        .saturating_add(usize::from(connect_labeled_unique(
                            cpg, call, target, call_kind, label,
                        )));
                result.topology_edges =
                    result
                        .topology_edges
                        .saturating_add(usize::from(connect_labeled_unique(
                            cpg,
                            call,
                            target,
                            CpgEdgeKind::CallSite,
                            label,
                        )));
            }
        } else if original_target.is_some()
            && !original_target.is_some_and(|target| is_function(cpg, target))
        {
            if let Some(CpgNodeKind::Call { target, .. }) =
                cpg.node_mut(call).map(|node| &mut node.kind)
            {
                *target = None;
            }
        }

        result.resolutions.insert(
            call,
            CallResolution {
                callee_name,
                scope,
                confidence: decision.confidence,
                target: decision.target,
                candidates: decision.candidates.nodes.clone(),
                candidate_count: decision.candidates.total,
                candidates_truncated: decision.candidates.is_truncated(),
            },
        );
    }
    result
}

#[derive(Debug)]
struct Decision {
    confidence: Confidence,
    target: Option<NodeId>,
    candidates: SymbolCandidates,
}

struct CallEvidence<'a> {
    call: NodeId,
    scope: Option<NodeId>,
    name: Option<&'a str>,
    is_method: bool,
    original_target: Option<NodeId>,
}

fn decide_call(
    cpg: &CodePropertyGraph,
    table: &SymbolTable,
    cfg: &ResolveConfig,
    evidence: &CallEvidence<'_>,
) -> Decision {
    if let Some(target) = evidence
        .original_target
        .filter(|&target| is_function(cpg, target))
    {
        let confidence = existing_resolution_confidence(cpg, evidence.call, target)
            .unwrap_or(Confidence::ExactViaBinding);
        return Decision {
            confidence,
            target: Some(target),
            candidates: singleton_candidates(target),
        };
    }

    let Some(name) = evidence.name.filter(|name| !name.is_empty()) else {
        return unresolved_decision();
    };
    if evidence.is_method && !cfg.resolve_methods {
        return unresolved_decision();
    }

    let mut current_scope = evidence.scope;
    let mut visited = BTreeSet::new();
    for _ in 0..=cfg.max_scope_depth {
        let Some(current) = current_scope else {
            break;
        };
        if !visited.insert(current) {
            break;
        }
        if let Some(candidates) = table
            .functions_by_scope
            .get(&current)
            .and_then(|symbols| symbols.get(name))
        {
            return decision_from_candidates(candidates, Confidence::ExactInFile);
        }
        current_scope = table.enclosing_scope.get(&current).copied();
    }

    let mut current_scope = evidence.scope;
    let mut visited = BTreeSet::new();
    for _ in 0..=cfg.max_scope_depth {
        let Some(current) = current_scope else {
            break;
        };
        if !visited.insert(current) {
            break;
        }
        if let Some(candidates) = table
            .imported_functions_by_scope
            .get(&current)
            .and_then(|symbols| symbols.get(name))
        {
            return decision_from_candidates(candidates, Confidence::ExactViaImport);
        }
        current_scope = table.enclosing_scope.get(&current).copied();
    }

    let mut current_scope = evidence.scope;
    let mut visited = BTreeSet::new();
    let mut visible_import = false;
    for _ in 0..=cfg.max_scope_depth {
        let Some(current) = current_scope else {
            break;
        };
        if !visited.insert(current) {
            break;
        }
        if table
            .imports_by_scope
            .get(&current)
            .is_some_and(|imports| imports.contains_key(name))
        {
            visible_import = true;
            break;
        }
        current_scope = table.enclosing_scope.get(&current).copied();
    }

    if let Some(candidates) = table.functions_by_name.get(name) {
        let confidence = if visible_import {
            Confidence::ExactViaImport
        } else {
            Confidence::BareNameUnique
        };
        return decision_from_candidates(candidates, confidence);
    }
    if visible_import {
        return Decision {
            confidence: Confidence::External,
            target: None,
            candidates: SymbolCandidates::default(),
        };
    }
    unresolved_decision()
}

fn decision_from_candidates(candidates: &SymbolCandidates, unique: Confidence) -> Decision {
    if candidates.total > 1 {
        return Decision {
            confidence: Confidence::BareNameAmbiguous,
            target: None,
            candidates: candidates.clone(),
        };
    }
    if candidates.total == 1 {
        if let Some(&target) = candidates.nodes.first() {
            return Decision {
                confidence: unique,
                target: Some(target),
                candidates: candidates.clone(),
            };
        }
    }
    unresolved_decision()
}

fn unresolved_decision() -> Decision {
    Decision {
        confidence: Confidence::Unresolved,
        target: None,
        candidates: SymbolCandidates::default(),
    }
}

fn singleton_candidates(node: NodeId) -> SymbolCandidates {
    SymbolCandidates {
        nodes: vec![node],
        total: 1,
    }
}

fn existing_resolution_confidence(
    cpg: &CodePropertyGraph,
    call: NodeId,
    target: NodeId,
) -> Option<Confidence> {
    cpg.native_edges_between(call, target)
        .filter(|edge| edge.kind.is_call())
        .filter_map(|edge| edge.label.as_deref().and_then(Confidence::from_label))
        .max_by_key(|confidence| confidence.rank())
}

fn connect_labeled_unique(
    cpg: &mut CodePropertyGraph,
    source: NodeId,
    target: NodeId,
    kind: CpgEdgeKind,
    label: &str,
) -> bool {
    if cpg
        .native_edges_between(source, target)
        .any(|edge| edge.kind == kind)
    {
        return false;
    }
    cpg.add_edge(CpgEdge::new(EdgeId::new(0), source, target, kind).with_label(label))
        .is_some()
}

fn callee_name(cpg: &CodePropertyGraph, call: NodeId, max_depth: u32) -> Option<String> {
    // A call grammar places the callee before its argument list. Restrict the
    // search to that first subtree: falling through into later argument
    // subtrees could fabricate a target from an argument identifier.
    let first = sorted_ast_children(cpg, call).into_iter().next()?;
    let mut stack = vec![(first, 0u32)];
    let mut visited = BTreeSet::new();
    while let Some((node, depth)) = stack.pop() {
        if !visited.insert(node) {
            continue;
        }
        let Some(cpg_node) = cpg.node(node) else {
            continue;
        };
        match &cpg_node.kind {
            CpgNodeKind::MemberAccess { member } => return Some(member.to_string()),
            CpgNodeKind::Identifier { name, .. } => return Some(name.to_string()),
            CpgNodeKind::Call { .. } => continue,
            _ => {}
        }
        if depth >= max_depth {
            continue;
        }
        let mut descendants = sorted_ast_children(cpg, node);
        descendants.reverse();
        stack.extend(
            descendants
                .into_iter()
                .map(|descendant| (descendant, depth.saturating_add(1))),
        );
    }
    None
}

/// Result of bounded typed callee-path reconstruction.
pub(crate) struct CalleePathEvidence {
    /// Reconstructed path when typed evidence was sufficient.
    pub(crate) path: Option<String>,
    /// Whether depth or cycle protection stopped reconstruction.
    pub(crate) capped: bool,
}

/// Reconstructs a call's typed callee path without reading or parsing source text.
pub(crate) fn call_callee_path_evidence(
    cpg: &CodePropertyGraph,
    call: NodeId,
    max_depth: u32,
) -> CalleePathEvidence {
    // A call grammar places the callee before its argument list. Restrict the
    // search to that first subtree: falling through into later argument
    // subtrees could fabricate a target from an argument identifier.
    let Some(mut current) = sorted_ast_children(cpg, call).into_iter().next() else {
        return CalleePathEvidence {
            path: None,
            capped: false,
        };
    };
    let mut visited = BTreeSet::new();
    let mut suffixes = Vec::new();
    for depth in 0..=max_depth {
        if !visited.insert(current) {
            return CalleePathEvidence {
                path: None,
                capped: true,
            };
        }
        let Some(cpg_node) = cpg.node(current) else {
            return CalleePathEvidence {
                path: None,
                capped: false,
            };
        };
        match &cpg_node.kind {
            CpgNodeKind::MemberAccess { member } => {
                suffixes.push(member.to_string());
            }
            CpgNodeKind::Identifier { name, .. } => {
                let mut path = name.to_string();
                for suffix in suffixes.iter().rev() {
                    if !path.is_empty() {
                        path.push_str("::");
                    }
                    path.push_str(suffix);
                }
                return CalleePathEvidence {
                    path: (!path.is_empty()).then_some(path),
                    capped: false,
                };
            }
            CpgNodeKind::Call { .. } => {
                return CalleePathEvidence {
                    path: None,
                    capped: false,
                };
            }
            _ => {}
        };
        let Some(next) = sorted_ast_children(cpg, current).into_iter().next() else {
            return CalleePathEvidence {
                path: None,
                capped: false,
            };
        };
        if depth == max_depth {
            return CalleePathEvidence {
                path: None,
                capped: true,
            };
        }
        current = next;
    }
    CalleePathEvidence {
        path: None,
        capped: true,
    }
}

pub(crate) fn call_callee_path(
    cpg: &CodePropertyGraph,
    call: NodeId,
    max_depth: u32,
) -> Option<String> {
    call_callee_path_evidence(cpg, call, max_depth).path
}

fn sorted_ast_children(cpg: &CodePropertyGraph, node: NodeId) -> Vec<NodeId> {
    let mut children = cpg.ast_children(node).to_vec();
    children.sort_by_key(|child| {
        cpg.node(*child)
            .map(|node| (node.range.start, node.range.end, *child))
            .unwrap_or((u32::MAX, u32::MAX, *child))
    });
    children
}

fn nearest_scope(
    cpg: &CodePropertyGraph,
    node: NodeId,
    scopes: &BTreeSet<NodeId>,
    max_depth: u32,
) -> Option<NodeId> {
    let mut current = cpg.node(node).and_then(|node| node.parent);
    let mut visited = BTreeSet::new();
    for _ in 0..=max_depth {
        let parent = current?;
        if !visited.insert(parent) {
            return None;
        }
        if scopes.contains(&parent) {
            return Some(parent);
        }
        current = cpg.node(parent).and_then(|node| node.parent);
    }
    None
}

fn is_scope(kind: &CpgNodeKind) -> bool {
    matches!(
        kind,
        CpgNodeKind::Root
            | CpgNodeKind::Module { .. }
            | CpgNodeKind::Class { .. }
            | CpgNodeKind::Struct { .. }
            | CpgNodeKind::Enum { .. }
            | CpgNodeKind::Trait { .. }
            | CpgNodeKind::Impl { .. }
            | CpgNodeKind::Function { .. }
            | CpgNodeKind::Block { .. }
            | CpgNodeKind::Lambda { .. }
    )
}

fn is_function(cpg: &CodePropertyGraph, node: NodeId) -> bool {
    cpg.node(node)
        .is_some_and(|node| matches!(node.kind, CpgNodeKind::Function { .. }))
}

fn terminal_name(path: &str) -> Option<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for character in path.chars() {
        if character == '_' || character == '$' || character.is_alphanumeric() {
            current.push(character);
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens.into_iter().rev().find(|token| {
        !matches!(
            token.as_str(),
            "as" | "from" | "import" | "use" | "include" | "require" | "export"
        )
    })
}

fn insert_named_candidate(
    symbols: &mut BTreeMap<String, SymbolCandidates>,
    name: &str,
    node: NodeId,
    cap: usize,
) {
    symbols
        .entry(name.to_owned())
        .or_default()
        .insert(node, cap);
}

fn insert_scoped_candidate(
    scopes: &mut BTreeMap<NodeId, BTreeMap<String, SymbolCandidates>>,
    scope: NodeId,
    name: &str,
    node: NodeId,
    cap: usize,
) {
    insert_named_candidate(scopes.entry(scope).or_default(), name, node, cap);
}

fn edge_kind_rank(kind: &CpgEdgeKind) -> u8 {
    u8::from(matches!(kind, CpgEdgeKind::ContainedIn))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use proptest::prelude::*;

    use super::*;
    use crate::{
        call_graph_sccs, CpgNode, Language, MethodSignature, ScopeId, SourceRange, Visibility,
    };

    fn node(kind: CpgNodeKind) -> CpgNode {
        CpgNode::new(NodeId::new(0), kind, SourceRange::default())
    }

    fn function(name: &str) -> CpgNodeKind {
        CpgNodeKind::Function {
            signature: Box::new(MethodSignature {
                name: name.into(),
                params: Default::default(),
                return_type: None,
                is_static: false,
                is_async: false,
                visibility: Visibility::Private,
            }),
        }
    }

    fn child(cpg: &mut CodePropertyGraph, parent: NodeId, kind: CpgNodeKind) -> NodeId {
        let child = cpg.add_node(node(kind));
        cpg.connect(parent, child, CpgEdgeKind::AstChild);
        child
    }

    fn exact_fixture(is_method: bool) -> (CodePropertyGraph, NodeId, NodeId) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let root = cpg.add_node(node(CpgNodeKind::Root));
        let callee = child(&mut cpg, root, function("callee"));
        let caller = child(&mut cpg, root, function("caller"));
        let block = child(
            &mut cpg,
            caller,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        let call = child(
            &mut cpg,
            block,
            CpgNodeKind::Call {
                target: None,
                is_method,
            },
        );
        let callee_kind = if is_method {
            CpgNodeKind::MemberAccess {
                member: "callee".into(),
            }
        } else {
            CpgNodeKind::Identifier {
                name: "callee".into(),
                definition: None,
            }
        };
        child(&mut cpg, call, callee_kind);
        (cpg, call, callee)
    }

    fn is_call_edges(cpg: &CodePropertyGraph) -> BTreeSet<(NodeId, NodeId, String)> {
        cpg.edges()
            .filter(|edge| edge.kind.is_call())
            .map(|edge| {
                (
                    edge.source,
                    edge.target,
                    format!("{:?}:{}", edge.kind, edge.label.as_deref().unwrap_or("")),
                )
            })
            .collect()
    }

    #[test]
    fn confidence_scores_and_topology_boundary_are_exact() {
        let expected = [
            (Confidence::ExactInFile, 1.0, true, "exact_in_file", 9),
            (
                Confidence::ExactViaImport,
                0.95,
                true,
                "exact_via_import",
                8,
            ),
            (
                Confidence::ExactViaBinding,
                0.9,
                true,
                "exact_via_binding",
                7,
            ),
            (Confidence::VariableType, 0.85, true, "variable_type", 6),
            (Confidence::RapidType, 0.8, true, "rapid_type", 5),
            (Confidence::ClassHierarchy, 0.75, true, "class_hierarchy", 4),
            (Confidence::BareNameUnique, 0.7, true, "bare_name_unique", 3),
            (
                Confidence::BareNameAmbiguous,
                0.3,
                false,
                "bare_name_ambiguous",
                2,
            ),
            (Confidence::External, 0.0, false, "external", 1),
            (Confidence::Unresolved, 0.0, false, "unresolved", 0),
        ];
        for (confidence, score, topology, label, rank) in expected {
            assert_eq!(confidence.score(), score);
            assert_eq!(confidence.creates_topology(), topology);
            assert_eq!(confidence.label(), label);
            assert_eq!(confidence.rank(), rank);
            assert_eq!(Confidence::from_label(label), Some(confidence));
        }
        assert_eq!(Confidence::from_label("invented"), None);
    }

    #[test]
    fn exact_in_file_resolution_matches_an_explicit_call_graph_and_is_idempotent() {
        let (mut cpg, call, callee) = exact_fixture(false);
        let mut explicit = cpg.clone();
        if let Some(CpgNodeKind::Call { target, .. }) =
            explicit.node_mut(call).map(|node| &mut node.kind)
        {
            *target = Some(callee);
        }

        let before_nodes = cpg.node_count();
        let first = resolve_calls(&mut cpg, &ResolveConfig::default());
        let first_edges = cpg.edge_count();
        let record = &first.resolutions[&call];
        assert_eq!(record.confidence, Confidence::ExactInFile);
        assert_eq!(record.target, Some(callee));
        assert_eq!(record.candidates, vec![callee]);
        assert_eq!(first.topology_edges, 2);
        assert_eq!(cpg.node_count(), before_nodes);
        assert_eq!(call_graph_sccs(&cpg), call_graph_sccs(&explicit));

        let call_edges: Vec<_> = cpg
            .edges_between(call, callee)
            .into_iter()
            .filter(|edge| edge.kind.is_call())
            .collect();
        assert_eq!(call_edges.len(), 2);
        assert!(call_edges
            .iter()
            .all(|edge| edge.label.as_deref() == Some("exact_in_file")));
        assert!(cpg
            .edges_between(call, callee)
            .into_iter()
            .any(|edge| edge.kind == CpgEdgeKind::Reference
                && edge.label.as_deref() == Some("exact_in_file")));

        let second = resolve_calls(&mut cpg, &ResolveConfig::default());
        assert_eq!(second.resolutions, first.resolutions);
        assert_eq!(second.topology_edges, 0);
        assert_eq!(cpg.edge_count(), first_edges);
    }

    #[test]
    fn symbol_table_emits_deterministic_scope_relations_once() {
        let (mut cpg, call, callee) = exact_fixture(false);
        let table = build_symbol_table(&mut cpg, &ResolveConfig::default());
        let after_first = cpg.edge_count();
        assert_eq!(table.functions_by_name["callee"].nodes, vec![callee]);
        assert!(table.enclosing_scope.contains_key(&call));
        assert!(cpg
            .edges()
            .any(|edge| edge.source == call && edge.kind == CpgEdgeKind::ContainedIn));
        assert!(cpg.edges().any(|edge| {
            edge.source == table.enclosing_scope[&call] && edge.kind == CpgEdgeKind::EnclosingScope
        }));

        let repeated = build_symbol_table(&mut cpg, &ResolveConfig::default());
        assert_eq!(repeated, table);
        assert_eq!(cpg.edge_count(), after_first);
    }

    #[test]
    fn valid_existing_binding_is_preserved_and_invalid_binding_is_cleared() {
        let (mut cpg, call, callee) = exact_fixture(false);
        if let Some(CpgNodeKind::Call { target, .. }) =
            cpg.node_mut(call).map(|node| &mut node.kind)
        {
            *target = Some(callee);
        }
        let result = resolve_calls(&mut cpg, &ResolveConfig::default());
        assert_eq!(
            result.resolutions[&call].confidence,
            Confidence::ExactViaBinding
        );

        let (mut malformed, call, _) = exact_fixture(false);
        if let Some(CpgNodeKind::Call { target, .. }) =
            malformed.node_mut(call).map(|node| &mut node.kind)
        {
            *target = Some(NodeId::new(999));
        }
        let result = resolve_calls(
            &mut malformed,
            &ResolveConfig {
                max_candidates_per_name: 0,
                ..ResolveConfig::default()
            },
        );
        assert_eq!(result.resolutions[&call].confidence, Confidence::Unresolved);
        assert!(matches!(
            malformed.node(call).map(|node| &node.kind),
            Some(CpgNodeKind::Call { target: None, .. })
        ));
    }

    #[test]
    fn methods_use_dynamic_edges_and_can_be_disabled() {
        let (mut cpg, call, callee) = exact_fixture(true);
        let result = resolve_calls(&mut cpg, &ResolveConfig::default());
        assert_eq!(result.resolutions[&call].target, Some(callee));
        assert!(cpg
            .edges_between(call, callee)
            .into_iter()
            .any(|edge| edge.kind == CpgEdgeKind::DynamicCall));
        assert!(!cpg
            .edges_between(call, callee)
            .into_iter()
            .any(|edge| edge.kind == CpgEdgeKind::StaticCall));

        let (mut disabled, call, _) = exact_fixture(true);
        let result = resolve_calls(
            &mut disabled,
            &ResolveConfig {
                resolve_methods: false,
                ..ResolveConfig::default()
            },
        );
        assert_eq!(result.resolutions[&call].confidence, Confidence::Unresolved);
        assert!(is_call_edges(&disabled).is_empty());
    }

    #[test]
    fn callee_reconstruction_never_falls_through_into_argument_subtrees() {
        let (mut cpg, call, _) = exact_fixture(false);
        let first_child = sorted_ast_children(&cpg, call)[0];
        cpg.node_mut(first_child).expect("callee child").kind = CpgNodeKind::Literal {
            kind: crate::LiteralKind::Integer(1),
        };
        child(
            &mut cpg,
            call,
            CpgNodeKind::Identifier {
                name: "callee".into(),
                definition: None,
            },
        );

        let result = resolve_calls(&mut cpg, &ResolveConfig::default());
        assert_eq!(result.resolutions[&call].confidence, Confidence::Unresolved);
        assert!(is_call_edges(&cpg).is_empty());
    }

    #[test]
    fn bounded_callee_reconstruction_handles_nested_calls_depth_limits_and_cycles() {
        let mut nested = CodePropertyGraph::new(Language::Rust);
        let outer = nested.add_node(node(CpgNodeKind::Call {
            target: None,
            is_method: false,
        }));
        let inner = child(
            &mut nested,
            outer,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        );
        child(
            &mut nested,
            inner,
            CpgNodeKind::Identifier {
                name: "not_the_outer_callee".into(),
                definition: None,
            },
        );
        assert_eq!(callee_name(&nested, outer, 128), None);

        let mut bounded = CodePropertyGraph::new(Language::Rust);
        let call = bounded.add_node(node(CpgNodeKind::Call {
            target: None,
            is_method: false,
        }));
        let group = child(
            &mut bounded,
            call,
            CpgNodeKind::Unknown {
                kind: "callee_group".into(),
            },
        );
        child(
            &mut bounded,
            group,
            CpgNodeKind::Identifier {
                name: "too_deep".into(),
                definition: None,
            },
        );
        assert_eq!(callee_name(&bounded, call, 0), None);

        let mut cyclic = CodePropertyGraph::new(Language::Rust);
        let call = cyclic.add_node(node(CpgNodeKind::Call {
            target: None,
            is_method: false,
        }));
        let group = child(
            &mut cyclic,
            call,
            CpgNodeKind::Unknown {
                kind: "cyclic_callee_group".into(),
            },
        );
        cyclic.connect(group, group, CpgEdgeKind::AstChild);
        assert_eq!(callee_name(&cyclic, call, 128), None);
    }

    #[test]
    fn terminal_import_names_support_all_separators_and_ignore_empty_paths() {
        for path in [
            "prefix:name",
            "prefix.name",
            "prefix/name",
            "prefix\\name",
            "prefix name",
            "prefix{name",
            "prefix}name",
            "prefix,name",
            "prefix::name;",
            "prefix::name*",
        ] {
            assert_eq!(terminal_name(path).as_deref(), Some("name"), "path: {path}");
        }
        assert_eq!(terminal_name(""), None);
        assert_eq!(terminal_name(";**"), None);

        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let root = cpg.add_node(node(CpgNodeKind::Root));
        child(&mut cpg, root, CpgNodeKind::Import { path: "".into() });
        let table = build_symbol_table(&mut cpg, &ResolveConfig::default());
        assert!(table.imports_by_scope.is_empty());
    }

    #[test]
    fn imports_distinguish_in_graph_exact_targets_from_external_calls() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let root = cpg.add_node(node(CpgNodeKind::Root));
        let module = child(
            &mut cpg,
            root,
            CpgNodeKind::Module {
                name: "dependency".into(),
            },
        );
        let target = child(&mut cpg, module, function("imported"));
        child(
            &mut cpg,
            root,
            CpgNodeKind::Import {
                path: "dependency::imported".into(),
            },
        );
        child(
            &mut cpg,
            root,
            CpgNodeKind::Import {
                path: "dependency::external".into(),
            },
        );
        let caller = child(&mut cpg, root, function("caller"));
        let imported_call = child(
            &mut cpg,
            caller,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        );
        child(
            &mut cpg,
            imported_call,
            CpgNodeKind::Identifier {
                name: "imported".into(),
                definition: None,
            },
        );
        let external_call = child(
            &mut cpg,
            caller,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        );
        child(
            &mut cpg,
            external_call,
            CpgNodeKind::Identifier {
                name: "external".into(),
                definition: None,
            },
        );

        let result = resolve_calls(&mut cpg, &ResolveConfig::default());
        assert_eq!(
            result.resolutions[&imported_call].confidence,
            Confidence::ExactViaImport
        );
        assert_eq!(result.resolutions[&imported_call].target, Some(target));
        assert_eq!(
            result.resolutions[&external_call].confidence,
            Confidence::External
        );
        assert_eq!(result.external, 1);
        assert!(cpg.callees(external_call).is_empty());
    }

    #[test]
    fn unique_global_fallback_is_distinct_from_lexical_resolution() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let root = cpg.add_node(node(CpgNodeKind::Root));
        let target = cpg.add_node(node(function("detached")));
        let caller = child(&mut cpg, root, function("caller"));
        let call = child(
            &mut cpg,
            caller,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        );
        child(
            &mut cpg,
            call,
            CpgNodeKind::Identifier {
                name: "detached".into(),
                definition: None,
            },
        );

        let result = resolve_calls(&mut cpg, &ResolveConfig::default());
        let record = &result.resolutions[&call];
        assert_eq!(record.confidence, Confidence::BareNameUnique);
        assert_eq!(record.target, Some(target));
        assert_eq!(record.candidates, vec![target]);
    }

    #[test]
    fn nearest_lexical_scope_wins_over_same_named_outer_declarations() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let root = cpg.add_node(node(CpgNodeKind::Root));
        child(&mut cpg, root, function("select"));
        let caller = child(&mut cpg, root, function("caller"));
        let local = child(&mut cpg, caller, function("select"));
        let block = child(
            &mut cpg,
            caller,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        let call = child(
            &mut cpg,
            block,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        );
        child(
            &mut cpg,
            call,
            CpgNodeKind::Identifier {
                name: "select".into(),
                definition: None,
            },
        );

        let result = resolve_calls(&mut cpg, &ResolveConfig::default());
        let record = &result.resolutions[&call];
        assert_eq!(record.confidence, Confidence::ExactInFile);
        assert_eq!(record.target, Some(local));
        assert_eq!(record.candidates, vec![local]);
    }

    #[test]
    fn cyclic_callee_subtree_is_visited_once_and_remains_deterministic() {
        let (mut cpg, call, _) = exact_fixture(false);
        let callee_syntax = sorted_ast_children(&cpg, call)[0];
        cpg.node_mut(callee_syntax).expect("callee syntax").kind = CpgNodeKind::Unknown {
            kind: "callee_group".into(),
        };
        cpg.connect(callee_syntax, callee_syntax, CpgEdgeKind::AstChild);

        let first = resolve_calls(&mut cpg, &ResolveConfig::default());
        let edge_count = cpg.edge_count();
        let second = resolve_calls(&mut cpg, &ResolveConfig::default());

        assert_eq!(first.resolutions[&call].confidence, Confidence::Unresolved);
        assert_eq!(second.resolutions, first.resolutions);
        assert_eq!(cpg.edge_count(), edge_count);
    }

    #[test]
    fn candidate_and_call_caps_are_enforced_without_changing_ambiguity() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let root = cpg.add_node(node(CpgNodeKind::Root));
        for _ in 0..3 {
            child(&mut cpg, root, function("duplicate"));
        }
        let caller = child(&mut cpg, root, function("caller"));
        let mut calls = Vec::new();
        for _ in 0..2 {
            let call = child(
                &mut cpg,
                caller,
                CpgNodeKind::Call {
                    target: None,
                    is_method: false,
                },
            );
            child(
                &mut cpg,
                call,
                CpgNodeKind::Identifier {
                    name: "duplicate".into(),
                    definition: None,
                },
            );
            calls.push(call);
        }
        let result = resolve_calls(
            &mut cpg,
            &ResolveConfig {
                max_candidates_per_name: 1,
                max_calls: 1,
                ..ResolveConfig::default()
            },
        );
        assert_eq!(result.resolutions.len(), 1);
        let record = &result.resolutions[&calls[0]];
        assert_eq!(record.confidence, Confidence::BareNameAmbiguous);
        assert_eq!(record.candidate_count, 3);
        assert_eq!(record.candidates.len(), 1);
        assert!(record.candidates_truncated);
        assert!(is_call_edges(&cpg).is_empty());
    }

    fn permutation_fixture(reverse_declarations: bool, target_count: usize) -> CodePropertyGraph {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        cpg.add_node_with_id(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Root,
            SourceRange::default(),
        ));
        let mut declarations: Vec<_> = (0..target_count)
            .map(|index| {
                CpgNode::new(
                    NodeId::new(index as u32 + 1),
                    function("target"),
                    SourceRange::default(),
                )
                .with_parent(NodeId::new(0))
            })
            .collect();
        if reverse_declarations {
            declarations.reverse();
        }
        for declaration in declarations {
            cpg.add_node_with_id(declaration);
        }
        cpg.add_node_with_id(
            CpgNode::new(NodeId::new(10), function("caller"), SourceRange::default())
                .with_parent(NodeId::new(0)),
        );
        cpg.add_node_with_id(
            CpgNode::new(
                NodeId::new(11),
                CpgNodeKind::Call {
                    target: None,
                    is_method: false,
                },
                SourceRange::default(),
            )
            .with_parent(NodeId::new(10)),
        );
        cpg.add_node_with_id(
            CpgNode::new(
                NodeId::new(12),
                CpgNodeKind::Identifier {
                    name: "target".into(),
                    definition: None,
                },
                SourceRange::default(),
            )
            .with_parent(NodeId::new(11)),
        );

        let mut ast_edges: Vec<_> = (0..target_count)
            .map(|index| (NodeId::new(0), NodeId::new(index as u32 + 1)))
            .chain([
                (NodeId::new(0), NodeId::new(10)),
                (NodeId::new(10), NodeId::new(11)),
                (NodeId::new(11), NodeId::new(12)),
            ])
            .collect();
        if reverse_declarations {
            ast_edges.reverse();
        }
        for (parent, child) in ast_edges {
            cpg.connect(parent, child, CpgEdgeKind::AstChild);
        }
        cpg
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn resolution_is_deterministic_under_node_and_edge_permutations(
            target_count in 1usize..7,
        ) {
            let mut forward = permutation_fixture(false, target_count);
            let mut reverse = permutation_fixture(true, target_count);
            let forward_result = resolve_calls(&mut forward, &ResolveConfig::default());
            let reverse_result = resolve_calls(&mut reverse, &ResolveConfig::default());
            prop_assert_eq!(forward_result, reverse_result);
            prop_assert_eq!(is_call_edges(&forward), is_call_edges(&reverse));
        }

        #[test]
        fn ambiguous_generated_names_never_change_call_topology_or_sccs(
            mut cpg in crate::testutil::arb_well_formed_cpg(),
        ) {
            let mut names: BTreeSet<String> = cpg
                .calls()
                .filter_map(|call| callee_name(&cpg, call.id, 128))
                .collect();
            names.insert("duplicate".to_owned());
            for name in names {
                cpg.add_node(node(function(&name)));
                cpg.add_node(node(function(&name)));
            }
            let root = cpg.root().expect("generated CPG root");
            let call = crate::testutil::wf_child(
                &mut cpg,
                root,
                CpgNodeKind::Call { target: None, is_method: false },
            );
            crate::testutil::wf_child(
                &mut cpg,
                call,
                CpgNodeKind::Identifier { name: "duplicate".into(), definition: None },
            );

            let call_edges_before = is_call_edges(&cpg);
            let sccs_before = call_graph_sccs(&cpg);
            let result = resolve_calls(&mut cpg, &ResolveConfig::default());
            prop_assert_eq!(is_call_edges(&cpg), call_edges_before);
            prop_assert_eq!(call_graph_sccs(&cpg), sccs_before);
            prop_assert_eq!(result.resolutions[&call].confidence, Confidence::BareNameAmbiguous);
            prop_assert!(result.resolutions.values().all(|record| !record.confidence.creates_topology()));
            let all_unresolved = cpg
                .calls()
                .all(|node| matches!(node.kind, CpgNodeKind::Call { target: None, .. }));
            prop_assert!(all_unresolved);
        }
    }

    #[cfg(feature = "serde")]
    #[test]
    fn resolution_results_round_trip_through_json() {
        let (mut cpg, _, _) = exact_fixture(false);
        let result = resolve_calls(&mut cpg, &ResolveConfig::default());
        let encoded = serde_json::to_string(&result).expect("serialize resolution");
        let decoded: CallGraphResolution =
            serde_json::from_str(&encoded).expect("deserialize resolution");
        assert_eq!(decoded, result);
    }
}
