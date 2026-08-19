//! Uninitialized-use detection from normalized variable and DFG evidence.
//!
//! The detector reuses [`variable_facts`](super::super::variable_facts) for
//! binder-aware use classification. A variable use is a candidate when no
//! represented reaching definition from the same function scope enters it.

use std::collections::BTreeSet;
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::{CodePropertyGraph, CpgEdgeKind, CpgNodeKind, NodeId};

use super::super::{function_scope_nodes, variable_facts};

/// One variable read with no represented in-scope reaching definition.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct UninitializedUse {
    /// Function whose lexical scope contains the use.
    pub function: NodeId,
    /// Identifier node that performs the read.
    pub use_site: NodeId,
    /// Canonical variable spelling assigned by the fact universe.
    pub variable: Arc<str>,
    /// Reaching definitions ignored because they belong to another function.
    pub out_of_scope_definitions: Vec<NodeId>,
}

/// Deterministic uninitialized-use findings and processing evidence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct UninitializedUseAnalysis {
    /// Candidates ordered by function, use site, and variable spelling.
    pub uses: Vec<UninitializedUse>,
    /// Functions examined in stable identifier order.
    pub processed_functions: Vec<NodeId>,
}

impl UninitializedUseAnalysis {
    /// Returns true because this finite read-only projection has no truncation mode.
    pub const fn is_complete(&self) -> bool {
        true
    }
}

/// Computes every represented variable use with an empty in-scope reaching-definition set.
///
/// Unbound function and method names are callee syntax rather than variable
/// reads and are excluded. A bound function-valued variable remains a use and
/// is checked normally. The CPG is never mutated.
pub fn analyze_uninitialized_uses(cpg: &CodePropertyGraph) -> UninitializedUseAnalysis {
    let functions: Vec<_> = cpg
        .functions()
        .map(|node| node.id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut findings = BTreeSet::new();

    for &function in &functions {
        let scope: BTreeSet<_> = function_scope_nodes(cpg, function).into_iter().collect();
        let non_values = non_value_identifiers(cpg, function, &scope);
        let facts = variable_facts(cpg, function);
        for (&use_site, variables) in &facts.uses {
            if non_values.contains(&use_site) {
                continue;
            }
            let mut in_scope = false;
            let mut out_of_scope = BTreeSet::new();
            for definition in cpg.reaching_definitions(use_site) {
                if scope.contains(&definition) {
                    in_scope = true;
                } else {
                    out_of_scope.insert(definition);
                }
            }
            if in_scope {
                continue;
            }
            for &variable in variables {
                if let Some(name) = facts.name(variable) {
                    findings.insert(UninitializedUse {
                        function,
                        use_site,
                        variable: Arc::clone(name),
                        out_of_scope_definitions: out_of_scope.iter().copied().collect(),
                    });
                }
            }
        }
    }

    UninitializedUseAnalysis {
        uses: findings.into_iter().collect(),
        processed_functions: functions,
    }
}

/// Returns the deterministic uninitialized-use finding projection.
pub fn uninitialized_uses(cpg: &CodePropertyGraph) -> Vec<UninitializedUse> {
    analyze_uninitialized_uses(cpg).uses
}

fn non_value_identifiers(
    cpg: &CodePropertyGraph,
    function: NodeId,
    scope: &BTreeSet<NodeId>,
) -> BTreeSet<NodeId> {
    let mut symbols = BTreeSet::new();

    // Tree-sitter retains declaration names and type syntax in the AST. Every
    // unbound identifier before the function body belongs to the signature,
    // not to the executable variable-use relation. Bound identifiers remain
    // eligible so default-value expressions referring to variables are not
    // silently discarded.
    let function_children = cpg.ast_children(function);
    if let Some(body_index) = function_children.iter().position(|&child| {
        cpg.node(child)
            .is_some_and(|node| matches!(node.kind, CpgNodeKind::Block { .. }))
    }) {
        for &child in &function_children[..body_index] {
            for node in std::iter::once(child).chain(cpg.ast_descendants(child)) {
                if cpg
                    .node(node)
                    .is_some_and(|node| matches!(node.kind, CpgNodeKind::Identifier { .. }))
                    && !has_variable_binding(cpg, node)
                {
                    symbols.insert(node);
                }
            }
        }
    }

    for &call in scope {
        if !cpg
            .node(call)
            .is_some_and(|node| matches!(node.kind, CpgNodeKind::Call { .. }))
        {
            continue;
        }
        let Some(callee) = cpg.ast_children(call).first().copied() else {
            continue;
        };
        match cpg.node(callee).map(|node| &node.kind) {
            Some(CpgNodeKind::Identifier { .. }) if !has_variable_binding(cpg, callee) => {
                symbols.insert(callee);
            }
            Some(CpgNodeKind::MemberAccess { member }) => {
                for descendant in std::iter::once(callee).chain(cpg.ast_descendants(callee)) {
                    if cpg.node(descendant).is_some_and(|node| {
                        matches!(
                            &node.kind,
                            CpgNodeKind::Identifier { name, .. }
                                if name == member && !has_variable_binding(cpg, descendant)
                        )
                    }) {
                        symbols.insert(descendant);
                    }
                }
            }
            _ => {}
        }
    }
    symbols
}

fn has_variable_binding(cpg: &CodePropertyGraph, identifier: NodeId) -> bool {
    let binding_kind = |target| {
        cpg.node(target).is_some_and(|node| {
            matches!(
                node.kind,
                CpgNodeKind::Variable { .. }
                    | CpgNodeKind::Parameter { .. }
                    | CpgNodeKind::Field { .. }
                    | CpgNodeKind::Assignment { .. }
            )
        })
    };
    if let Some(CpgNodeKind::Identifier {
        definition: Some(definition),
        ..
    }) = cpg.node(identifier).map(|node| &node.kind)
    {
        if binding_kind(*definition) {
            return true;
        }
    }
    cpg.reaching_definitions(identifier)
        .into_iter()
        .any(binding_kind)
        || cpg
            .outgoing_edges(identifier)
            .any(|edge| matches!(edge.kind, CpgEdgeKind::Reference) && binding_kind(edge.target))
        || cpg
            .incoming_edges(identifier)
            .any(|edge| matches!(edge.kind, CpgEdgeKind::Reference) && binding_kind(edge.source))
}

#[cfg(test)]
#[path = "uninit/tests.rs"]
mod tests;
