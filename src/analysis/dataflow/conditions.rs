//! Shared, stack-safe extraction of typed branch-condition expressions.

use std::collections::BTreeSet;

use crate::{CodePropertyGraph, CpgNodeKind, NodeId};

/// Returns the expression that directly controls boolean CFG edges at `source`.
///
/// Canonical CFGs place the expression at the edge source. Wrapper-shaped
/// `if` and `while` nodes are also accepted by searching only their first AST
/// child, which is the frontend contract for the condition. The search uses a
/// heap worklist and cuts cycles. It never examines later `if`/`while`
/// children, which are the branch bodies.
pub(crate) fn guard_expression(cpg: &CodePropertyGraph, source: NodeId) -> Option<NodeId> {
    let source_node = cpg.node(source)?;
    if source_node.is_expression() {
        return Some(source);
    }
    if !matches!(source_node.kind, CpgNodeKind::If | CpgNodeKind::While) {
        return None;
    }
    let root = cpg.ast_children(source).first().copied()?;
    let mut pending = vec![root];
    let mut seen = BTreeSet::new();
    while let Some(node) = pending.pop() {
        if !seen.insert(node) {
            continue;
        }
        let payload = cpg.node(node)?;
        if payload.is_expression() {
            return Some(node);
        }
        if matches!(payload.kind, CpgNodeKind::Function { .. }) {
            continue;
        }
        let mut children = cpg.ast_children(node);
        children.reverse();
        pending.extend(children);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CpgEdgeKind, CpgNode, Language, ScopeId, SourceRange};

    fn add(cpg: &mut CodePropertyGraph, kind: CpgNodeKind) -> NodeId {
        cpg.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default()))
    }

    #[test]
    fn direct_and_wrapper_conditions_are_exact_and_only_first_child_is_searched() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let direct = add(
            &mut cpg,
            CpgNodeKind::BinaryOp {
                operator: "&&".into(),
            },
        );
        assert_eq!(guard_expression(&cpg, direct), Some(direct));

        let wrapper = add(&mut cpg, CpgNodeKind::If);
        let transparent = add(
            &mut cpg,
            CpgNodeKind::Unknown {
                kind: "condition_clause".into(),
            },
        );
        let condition = add(
            &mut cpg,
            CpgNodeKind::UnaryOp {
                operator: "!".into(),
            },
        );
        cpg.connect(wrapper, transparent, CpgEdgeKind::AstChild);
        cpg.connect(transparent, condition, CpgEdgeKind::AstChild);
        assert_eq!(guard_expression(&cpg, wrapper), Some(condition));

        let body_wrapper = add(&mut cpg, CpgNodeKind::While);
        let body = add(
            &mut cpg,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        let body_expression = add(
            &mut cpg,
            CpgNodeKind::BinaryOp {
                operator: "<".into(),
            },
        );
        cpg.connect(body_wrapper, body, CpgEdgeKind::AstChild);
        cpg.connect(body, body_expression, CpgEdgeKind::AstChild);
        assert_eq!(guard_expression(&cpg, body_wrapper), Some(body_expression));
    }

    #[test]
    fn wrapper_cycle_terminates_without_a_condition() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let wrapper = add(&mut cpg, CpgNodeKind::If);
        let transparent = add(
            &mut cpg,
            CpgNodeKind::Unknown {
                kind: "cycle".into(),
            },
        );
        cpg.connect(wrapper, transparent, CpgEdgeKind::AstChild);
        cpg.connect(transparent, transparent, CpgEdgeKind::AstChild);
        assert_eq!(guard_expression(&cpg, wrapper), None);
    }
}
