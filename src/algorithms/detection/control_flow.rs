//! Control flow pattern analysis for algorithm detection.

use crate::{CodePropertyGraph, CpgNodeKind, NodeId};
use std::sync::Arc;

/// Analyzes control flow patterns.
#[derive(Debug, Default)]
pub struct ControlFlowAnalyzer;

impl ControlFlowAnalyzer {
    /// Creates a new analyzer.
    pub fn new() -> Self {
        Self
    }

    /// Detects loop structures in a function.
    ///
    /// Traverses AST descendants of the function and identifies loop nodes
    /// (For, While, Loop, Match used as do-while). Computes nesting depth
    /// and determines if the loop is counted (bounded iteration).
    pub fn detect_loops(&self, cpg: &CodePropertyGraph, function: NodeId) -> Vec<LoopPattern> {
        let mut loops = Vec::new();
        let descendants = cpg.ast_descendants(function);

        for node_id in descendants {
            let Some(node) = cpg.node(node_id) else {
                continue;
            };

            let kind = match &node.kind {
                CpgNodeKind::For => LoopKind::For,
                CpgNodeKind::While => LoopKind::While,
                CpgNodeKind::Loop => LoopKind::Infinite,
                // Some languages use DoWhile explicitly
                kind if self.is_do_while_pattern(kind) => LoopKind::DoWhile,
                _ => continue,
            };

            // Calculate nesting depth by counting loop ancestors
            let depth = self.compute_loop_depth(cpg, node_id);

            // Check if this is a counted loop (bounded iteration)
            let is_counted = self.is_counted_loop(cpg, node_id, &kind);

            loops.push(LoopPattern {
                header: node_id,
                kind,
                depth,
                is_counted,
            });
        }

        loops
    }

    /// Detects recursive patterns in a function.
    ///
    /// Looks for call nodes within the function that call the function itself
    /// (direct recursion) or that are part of a mutually recursive cycle
    /// (indirect recursion). Also detects tail recursion patterns.
    pub fn detect_recursion(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
    ) -> Option<RecursionPattern> {
        let func_node = cpg.node(function)?;

        // Extract function name
        let func_name = match &func_node.kind {
            CpgNodeKind::Function { signature } => signature.name.clone(),
            _ => return None,
        };

        let descendants = cpg.ast_descendants(function);
        let mut recursive_calls = Vec::new();
        let mut base_cases = Vec::new();
        let mut is_tail_recursive = false;

        // Find return statements (potential base cases)
        let return_nodes: Vec<NodeId> = descendants
            .iter()
            .filter(|&&id| {
                cpg.node(id)
                    .map(|n| matches!(n.kind, CpgNodeKind::Return))
                    .unwrap_or(false)
            })
            .copied()
            .collect();

        // Find all call sites
        for &node_id in &descendants {
            let Some(node) = cpg.node(node_id) else {
                continue;
            };

            if !matches!(node.kind, CpgNodeKind::Call { .. }) {
                continue;
            }

            // Check if this call is to the same function (direct recursion)
            if self.is_call_to_function(cpg, node_id, &func_name) {
                recursive_calls.push(node_id);

                // Check if this is tail recursion
                if self.is_tail_call(cpg, node_id, &return_nodes) {
                    is_tail_recursive = true;
                }
            }
        }

        if recursive_calls.is_empty() {
            return None;
        }

        // Find base cases: return statements that don't contain recursive calls
        for &return_id in &return_nodes {
            let return_descendants = cpg.ast_descendants(return_id);
            let has_recursive_call = return_descendants
                .iter()
                .any(|id| recursive_calls.contains(id));
            if !has_recursive_call {
                // Check if the return is guarded by a conditional (proper base case)
                let ancestors = cpg.ast_ancestors(return_id);
                let is_guarded = ancestors.iter().any(|&id| {
                    cpg.node(id)
                        .map(|n| {
                            matches!(
                                n.kind,
                                CpgNodeKind::If | CpgNodeKind::Match | CpgNodeKind::MatchArm
                            )
                        })
                        .unwrap_or(false)
                });
                if is_guarded {
                    base_cases.push(return_id);
                }
            }
        }

        let kind = if is_tail_recursive {
            RecursionKind::Tail
        } else {
            RecursionKind::Direct
        };

        Some(RecursionPattern {
            kind,
            base_cases,
            recursive_calls,
        })
    }

    /// Computes the nesting depth of a loop.
    fn compute_loop_depth(&self, cpg: &CodePropertyGraph, loop_node: NodeId) -> usize {
        let ancestors = cpg.ast_ancestors(loop_node);
        let mut depth = 1; // The loop itself counts as depth 1

        for ancestor in ancestors {
            if let Some(node) = cpg.node(ancestor) {
                if matches!(
                    node.kind,
                    CpgNodeKind::For | CpgNodeKind::While | CpgNodeKind::Loop
                ) {
                    depth += 1;
                }
            }
        }

        depth
    }

    /// Checks if a loop is a counted loop (bounded iteration).
    fn is_counted_loop(&self, cpg: &CodePropertyGraph, loop_node: NodeId, kind: &LoopKind) -> bool {
        match kind {
            LoopKind::For => {
                // For loops are typically counted unless they iterate over unbounded collections
                // Check if it has a range-like pattern
                let children = cpg.ast_children(loop_node);
                for child_id in children {
                    if let Some(child) = cpg.node(child_id) {
                        // Look for range patterns (e.g., 0..n, iter.enumerate())
                        if self.is_range_expression(&child.kind) {
                            return true;
                        }
                        // Look for call to iterator methods that suggest counting
                        if matches!(&child.kind, CpgNodeKind::Call { .. })
                            && self.is_counting_iterator_call(cpg, child_id)
                        {
                            return true;
                        }
                    }
                }
                true // Assume for loops are counted by default
            }
            LoopKind::While => {
                // While loops are counted if they have a counter variable pattern
                // Look for: comparison + counter increment
                let descendants = cpg.ast_descendants(loop_node);
                let has_comparison = descendants.iter().any(|&id| {
                    cpg.node(id)
                        .map(|n| self.is_comparison_op(&n.kind))
                        .unwrap_or(false)
                });
                let has_increment = descendants.iter().any(|&id| {
                    cpg.node(id)
                        .map(|n| self.is_increment_op(&n.kind))
                        .unwrap_or(false)
                });
                has_comparison && has_increment
            }
            LoopKind::DoWhile => {
                // Similar to while loops
                let descendants = cpg.ast_descendants(loop_node);
                let has_comparison = descendants.iter().any(|&id| {
                    cpg.node(id)
                        .map(|n| self.is_comparison_op(&n.kind))
                        .unwrap_or(false)
                });
                let has_increment = descendants.iter().any(|&id| {
                    cpg.node(id)
                        .map(|n| self.is_increment_op(&n.kind))
                        .unwrap_or(false)
                });
                has_comparison && has_increment
            }
            LoopKind::Infinite | LoopKind::Iterator => false,
        }
    }

    /// Checks if a node kind represents a do-while pattern.
    fn is_do_while_pattern(&self, kind: &CpgNodeKind) -> bool {
        // Some ASTs represent do-while explicitly
        // Otherwise, we'd need to detect loop { ... if cond { break; } } patterns
        matches!(kind, CpgNodeKind::Unknown { kind: k } if k.contains("do_while") || k.contains("do_statement"))
    }

    /// Checks if a kind represents a range expression.
    fn is_range_expression(&self, kind: &CpgNodeKind) -> bool {
        matches!(kind, CpgNodeKind::BinaryOp { operator } if operator.contains(".."))
            || matches!(kind, CpgNodeKind::Unknown { kind: k } if k.contains("range"))
    }

    /// Checks if a call is to a counting iterator method.
    fn is_counting_iterator_call(&self, cpg: &CodePropertyGraph, call_node: NodeId) -> bool {
        // Look for iterator methods that suggest counting: enumerate, zip, take, etc.
        let children = cpg.ast_children(call_node);
        for child_id in children {
            if let Some(child) = cpg.node(child_id) {
                if let CpgNodeKind::Identifier { name, .. }
                | CpgNodeKind::MemberAccess { member: name } = &child.kind
                {
                    let name_lower = name.to_lowercase();
                    if name_lower.contains("enumerate")
                        || name_lower.contains("range")
                        || name_lower.contains("take")
                        || name_lower.contains("zip")
                    {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Checks if a kind is a comparison operator.
    fn is_comparison_op(&self, kind: &CpgNodeKind) -> bool {
        match kind {
            CpgNodeKind::BinaryOp { operator } => {
                let op = operator.as_ref();
                op == "<" || op == ">" || op == "<=" || op == ">=" || op == "==" || op == "!="
            }
            _ => false,
        }
    }

    /// Checks if a kind is an increment/decrement operation.
    fn is_increment_op(&self, kind: &CpgNodeKind) -> bool {
        match kind {
            CpgNodeKind::UnaryOp { operator } => {
                let op = operator.as_ref();
                op == "++" || op == "--"
            }
            CpgNodeKind::Assignment { operator } => {
                let op = operator.as_ref();
                op == "+=" || op == "-=" || op == "*=" || op == "/="
            }
            _ => false,
        }
    }

    /// Checks if a call node calls a specific function by name.
    fn is_call_to_function(
        &self,
        cpg: &CodePropertyGraph,
        call_node: NodeId,
        target_name: &Arc<str>,
    ) -> bool {
        let children = cpg.ast_children(call_node);
        for child_id in children {
            if let Some(child) = cpg.node(child_id) {
                match &child.kind {
                    CpgNodeKind::Identifier { name, .. } => {
                        if name == target_name {
                            return true;
                        }
                    }
                    // For method calls like self.func_name()
                    CpgNodeKind::MemberAccess { member } if member == target_name => {
                        return true;
                    }
                    _ => {}
                }
            }
        }
        false
    }

    /// Checks if a call is in tail position (last operation before return).
    fn is_tail_call(
        &self,
        cpg: &CodePropertyGraph,
        call_node: NodeId,
        return_nodes: &[NodeId],
    ) -> bool {
        // A call is in tail position if:
        // 1. It's the expression being returned, OR
        // 2. It's the last statement in a block that's the body of a function

        // Check if any return node has this call as its direct child or value
        for &return_id in return_nodes {
            let return_children = cpg.ast_children(return_id);
            // The return value is typically the first/only child
            if return_children.contains(&call_node) {
                return true;
            }
            // Also check if the call is nested in the return expression
            let return_descendants = cpg.ast_descendants(return_id);
            if return_descendants.contains(&call_node) {
                // Only count as tail if it's the outermost call in the return
                // Check that no other operations wrap the call
                let ancestors_to_return: Vec<NodeId> = cpg
                    .ast_ancestors(call_node)
                    .into_iter()
                    .take_while(|&id| id != return_id)
                    .collect();

                // If only blocks/parens between call and return, it's still tail
                let has_wrapping_op = ancestors_to_return.iter().any(|&id| {
                    cpg.node(id)
                        .map(|n| {
                            matches!(
                                n.kind,
                                CpgNodeKind::BinaryOp { .. }
                                    | CpgNodeKind::UnaryOp { .. }
                                    | CpgNodeKind::Call { .. }
                            )
                        })
                        .unwrap_or(false)
                });

                if !has_wrapping_op {
                    return true;
                }
            }
        }

        false
    }
}

/// Loop structure pattern.
#[derive(Debug, Clone)]
pub struct LoopPattern {
    /// Loop header node.
    pub header: NodeId,
    /// Loop type.
    pub kind: LoopKind,
    /// Nesting depth.
    pub depth: usize,
    /// Whether this is a counted loop.
    pub is_counted: bool,
}

/// Type of loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopKind {
    /// For loop (indexed).
    For,
    /// While loop.
    While,
    /// Do-while loop.
    DoWhile,
    /// Infinite loop with breaks.
    Infinite,
    /// Iterator-based loop.
    Iterator,
}

/// Recursion pattern.
#[derive(Debug, Clone)]
pub struct RecursionPattern {
    /// Type of recursion.
    pub kind: RecursionKind,
    /// Base case nodes.
    pub base_cases: Vec<NodeId>,
    /// Recursive call nodes.
    pub recursive_calls: Vec<NodeId>,
}

/// Type of recursion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecursionKind {
    /// Direct recursion (function calls itself).
    Direct,
    /// Indirect/mutual recursion.
    Indirect,
    /// Tail recursion.
    Tail,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::wf_child;
    use crate::{
        CodePropertyGraph, CpgEdgeKind, CpgNode, Language, MethodSignature, ScopeId, SourceRange,
        Visibility,
    };

    fn create_test_cpg_with_loop() -> (CodePropertyGraph, NodeId) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);

        // Create a function with a for loop inside
        let func = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Function {
                signature: MethodSignature {
                    name: "test_func".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Public,
                },
            },
            SourceRange::default(),
        ));

        let block = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
            SourceRange::default(),
        ));
        cpg.connect(func, block, CpgEdgeKind::AstChild);

        let for_loop = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::For,
            SourceRange::default(),
        ));
        cpg.connect(block, for_loop, CpgEdgeKind::AstChild);

        (cpg, func)
    }

    fn create_test_cpg_with_nested_loops() -> (CodePropertyGraph, NodeId) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);

        let func = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Function {
                signature: MethodSignature {
                    name: "nested_loops".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Public,
                },
            },
            SourceRange::default(),
        ));

        let mut block_node = CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
            SourceRange::default(),
        );
        block_node.parent = Some(func);
        let block = cpg.add_node(block_node);
        cpg.connect(func, block, CpgEdgeKind::AstChild);

        let mut outer_loop_node =
            CpgNode::new(NodeId::new(0), CpgNodeKind::For, SourceRange::default());
        outer_loop_node.parent = Some(block);
        let outer_loop = cpg.add_node(outer_loop_node);
        cpg.connect(block, outer_loop, CpgEdgeKind::AstChild);

        let mut inner_loop_node =
            CpgNode::new(NodeId::new(0), CpgNodeKind::While, SourceRange::default());
        inner_loop_node.parent = Some(outer_loop);
        let inner_loop = cpg.add_node(inner_loop_node);
        cpg.connect(outer_loop, inner_loop, CpgEdgeKind::AstChild);

        (cpg, func)
    }

    #[test]
    fn test_detect_single_loop() {
        let (cpg, func) = create_test_cpg_with_loop();
        let analyzer = ControlFlowAnalyzer::new();
        let loops = analyzer.detect_loops(&cpg, func);

        assert_eq!(loops.len(), 1);
        assert_eq!(loops[0].kind, LoopKind::For);
        assert_eq!(loops[0].depth, 1);
    }

    #[test]
    fn test_detect_nested_loops() {
        let (cpg, func) = create_test_cpg_with_nested_loops();
        let analyzer = ControlFlowAnalyzer::new();
        let loops = analyzer.detect_loops(&cpg, func);

        assert_eq!(loops.len(), 2);

        // Find outer and inner loops
        let outer = loops
            .iter()
            .find(|l| l.kind == LoopKind::For)
            .expect("outer loop");
        let inner = loops
            .iter()
            .find(|l| l.kind == LoopKind::While)
            .expect("inner loop");

        assert_eq!(outer.depth, 1);
        assert_eq!(inner.depth, 2);
    }

    #[test]
    fn test_no_loops() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Function {
                signature: MethodSignature {
                    name: "no_loops".into(),
                    params: Default::default(),
                    return_type: None,
                    is_static: false,
                    is_async: false,
                    visibility: Visibility::Public,
                },
            },
            SourceRange::default(),
        ));

        let analyzer = ControlFlowAnalyzer::new();
        let loops = analyzer.detect_loops(&cpg, func);

        assert!(loops.is_empty());
    }

    #[test]
    fn test_no_recursion_in_non_recursive_function() {
        let (cpg, func) = create_test_cpg_with_loop();
        let analyzer = ControlFlowAnalyzer::new();
        let recursion = analyzer.detect_recursion(&cpg, func);

        assert!(recursion.is_none());
    }

    #[test]
    fn test_analyzer_creation() {
        let analyzer = ControlFlowAnalyzer::new();
        // `::default()` rather than the unit literal on purpose: the `Default`
        // impl is what this assertion exercises.
        #[allow(clippy::default_constructed_unit_structs)]
        let default_analyzer = ControlFlowAnalyzer::default();
        // Just verify they can be created
        let _ = format!("{:?}", analyzer);
        let _ = format!("{:?}", default_analyzer);
    }

    // --- positive recursion detection ---

    fn make_function(cpg: &mut CodePropertyGraph, name: &str) -> NodeId {
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

    #[test]
    fn test_detect_recursion_direct_with_base_case() {
        // fn fact() { if .. { return 1; }  fact(); }
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = make_function(&mut cpg, "fact");

        // Guarded base case: If -> Return(1).
        let if_node = wf_child(&mut cpg, func, CpgNodeKind::If);
        let base_ret = wf_child(&mut cpg, if_node, CpgNodeKind::Return);
        wf_child(
            &mut cpg,
            base_ret,
            CpgNodeKind::Literal {
                kind: crate::LiteralKind::Integer(1),
            },
        );

        // Direct (non-tail) self-call: Call -> Identifier("fact").
        let call = wf_child(
            &mut cpg,
            func,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        );
        wf_child(
            &mut cpg,
            call,
            CpgNodeKind::Identifier {
                name: "fact".into(),
                definition: None,
            },
        );

        let rec = ControlFlowAnalyzer::new()
            .detect_recursion(&cpg, func)
            .expect("direct recursion should be detected");
        assert_eq!(rec.kind, RecursionKind::Direct);
        assert_eq!(rec.recursive_calls.len(), 1);
        assert_eq!(rec.recursive_calls[0], call);
        // The guarded, non-recursive return is a base case.
        assert_eq!(rec.base_cases.len(), 1);
        assert_eq!(rec.base_cases[0], base_ret);
    }

    #[test]
    fn test_detect_recursion_tail() {
        // fn go() { return go(); } — recursive call in tail position.
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = make_function(&mut cpg, "go");

        let ret = wf_child(&mut cpg, func, CpgNodeKind::Return);
        let call = wf_child(
            &mut cpg,
            ret,
            CpgNodeKind::Call {
                target: None,
                is_method: false,
            },
        );
        wf_child(
            &mut cpg,
            call,
            CpgNodeKind::Identifier {
                name: "go".into(),
                definition: None,
            },
        );

        let rec = ControlFlowAnalyzer::new()
            .detect_recursion(&cpg, func)
            .expect("tail recursion should be detected");
        assert_eq!(rec.kind, RecursionKind::Tail);
        assert_eq!(rec.recursive_calls.len(), 1);
        // The tail return wraps the recursive call, so it is NOT a base case.
        assert!(rec.base_cases.is_empty());
    }

    #[test]
    fn test_detect_recursion_via_method_call() {
        // fn walk() { self.walk(); } — member-access self call is direct recursion.
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = make_function(&mut cpg, "walk");
        let call = wf_child(
            &mut cpg,
            func,
            CpgNodeKind::Call {
                target: None,
                is_method: true,
            },
        );
        wf_child(
            &mut cpg,
            call,
            CpgNodeKind::MemberAccess {
                member: "walk".into(),
            },
        );

        let rec = ControlFlowAnalyzer::new()
            .detect_recursion(&cpg, func)
            .expect("method self-call should be detected as recursion");
        assert_eq!(rec.kind, RecursionKind::Direct);
        assert_eq!(rec.recursive_calls.len(), 1);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use crate::testutil::*;
    use proptest::prelude::*;

    fn function_root(g: &CodePropertyGraph) -> NodeId {
        node_ids(g)
            .into_iter()
            .find(|&id| {
                matches!(
                    g.node(id).map(|n| &n.kind),
                    Some(CpgNodeKind::Function { .. })
                )
            })
            .expect("well-formed cpg has a Function root")
    }

    fn is_loop(g: &CodePropertyGraph, id: NodeId) -> bool {
        matches!(
            g.node(id).map(|n| &n.kind),
            Some(CpgNodeKind::For | CpgNodeKind::While | CpgNodeKind::Loop)
        )
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(128))]

        /// `detect_loops` yields exactly one pattern per loop node reachable from
        /// the function, and each pattern's `depth` equals `1 + (number of loop
        /// ancestors)` of its header.
        #[test]
        fn prop_loop_depth_equals_one_plus_loop_ancestors(g in arb_well_formed_cpg()) {
            let root = function_root(&g);
            let loops = ControlFlowAnalyzer::new().detect_loops(&g, root);

            // Oracle: every loop node among the function's descendants.
            let loop_nodes: Vec<NodeId> = g
                .ast_descendants(root)
                .into_iter()
                .filter(|&id| is_loop(&g, id))
                .collect();

            // One detected pattern per loop node (bijection on headers).
            prop_assert_eq!(loops.len(), loop_nodes.len());
            let headers: std::collections::HashSet<NodeId> =
                loops.iter().map(|l| l.header).collect();
            let oracle: std::collections::HashSet<NodeId> =
                loop_nodes.iter().copied().collect();
            prop_assert_eq!(headers, oracle);

            // depth correctness for each detected loop.
            for lp in &loops {
                let loop_ancestors = g
                    .ast_ancestors(lp.header)
                    .into_iter()
                    .filter(|&a| is_loop(&g, a))
                    .count();
                prop_assert_eq!(lp.depth, 1 + loop_ancestors);
            }
        }
    }
}

/// Loop classification: kind, nesting depth, and boundedness.
///
/// [`ControlFlowAnalyzer::detect_loops`] answers three questions per loop —
/// *what kind*, *how deeply nested*, and *is the trip count bounded* — and the
/// last one drives the complexity ladder (a counted loop contributes a factor
/// of `n`; an unbounded one is only a lower bound). These tests pin each
/// classification rule against the minimal AST that exhibits it.
#[cfg(test)]
mod loop_classification {
    use super::*;
    use crate::testutil::wf_child;
    use crate::{
        CodePropertyGraph, CpgNode, Language, LiteralKind, MethodSignature, NodeId, ScopeId,
        SourceRange, Visibility,
    };

    fn func_with_body(cpg: &mut CodePropertyGraph) -> (NodeId, NodeId) {
        let func = cpg.add_node(CpgNode::new(
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
        let body = wf_child(
            cpg,
            func,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        (func, body)
    }

    fn ident(name: &str) -> CpgNodeKind {
        CpgNodeKind::Identifier {
            name: name.into(),
            definition: None,
        }
    }

    /// The single loop `detect_loops` finds in `cpg` below `function`.
    fn only_loop(cpg: &CodePropertyGraph, function: NodeId) -> LoopPattern {
        let mut loops = ControlFlowAnalyzer::new().detect_loops(cpg, function);
        assert_eq!(loops.len(), 1, "expected exactly one loop, got {loops:?}");
        loops.remove(0)
    }

    /// Each loop syntax maps to its own [`LoopKind`], and an infinite loop is
    /// never counted (it has no trip bound to read).
    #[test]
    fn loop_syntax_determines_the_kind() {
        for (kind, expected) in [
            (CpgNodeKind::For, LoopKind::For),
            (CpgNodeKind::While, LoopKind::While),
            (CpgNodeKind::Loop, LoopKind::Infinite),
            // Frontends that model `do … while` as an opaque node are honored
            // by name, since the CPG vocabulary has no dedicated kind for it.
            (
                CpgNodeKind::Unknown {
                    kind: "do_statement".into(),
                },
                LoopKind::DoWhile,
            ),
        ] {
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            let (func, body) = func_with_body(&mut cpg);
            wf_child(&mut cpg, body, kind.clone());

            let found = only_loop(&cpg, func);
            assert_eq!(found.kind, expected, "for node kind {kind:?}");
            assert_eq!(found.depth, 1, "a top-level loop has depth 1");
        }

        // A non-loop opaque node is not a loop at all.
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = func_with_body(&mut cpg);
        wf_child(
            &mut cpg,
            body,
            CpgNodeKind::Unknown {
                kind: "match_arm".into(),
            },
        );
        assert!(ControlFlowAnalyzer::new()
            .detect_loops(&cpg, func)
            .is_empty());
    }

    /// Depth counts enclosing loops, so a triply-nested loop reports depth 3.
    #[test]
    fn depth_counts_enclosing_loops() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = func_with_body(&mut cpg);

        let mut parent = body;
        let mut headers = Vec::with_capacity(3);
        for _ in 0..3 {
            let head = wf_child(&mut cpg, parent, CpgNodeKind::For);
            headers.push(head);
            parent = wf_child(
                &mut cpg,
                head,
                CpgNodeKind::Block {
                    scope: ScopeId::GLOBAL,
                },
            );
        }

        let loops = ControlFlowAnalyzer::new().detect_loops(&cpg, func);
        assert_eq!(loops.len(), 3);
        for (i, header) in headers.iter().enumerate() {
            let pattern = loops
                .iter()
                .find(|l| l.header == *header)
                .expect("each header is reported");
            assert_eq!(pattern.depth, i + 1, "loop {i} nests {} deep", i + 1);
        }
    }

    /// A `for` is counted by default, and explicitly so when its subject is a
    /// range expression or a counting iterator adaptor.
    #[test]
    fn for_loops_are_counted_by_range_or_iterator_witness() {
        // Range expression spelled as an operator (`0..n`).
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = func_with_body(&mut cpg);
        let head = wf_child(&mut cpg, body, CpgNodeKind::For);
        wf_child(
            &mut cpg,
            head,
            CpgNodeKind::BinaryOp {
                operator: "..".into(),
            },
        );
        assert!(only_loop(&cpg, func).is_counted);

        // Range expression spelled as an opaque node kind.
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = func_with_body(&mut cpg);
        let head = wf_child(&mut cpg, body, CpgNodeKind::For);
        wf_child(
            &mut cpg,
            head,
            CpgNodeKind::Unknown {
                kind: "range_expr".into(),
            },
        );
        assert!(only_loop(&cpg, func).is_counted);

        // Counting iterator adaptor (`.enumerate()`).
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = func_with_body(&mut cpg);
        let head = wf_child(&mut cpg, body, CpgNodeKind::For);
        let call = wf_child(
            &mut cpg,
            head,
            CpgNodeKind::Call {
                target: None,
                is_method: true,
            },
        );
        wf_child(
            &mut cpg,
            call,
            CpgNodeKind::MemberAccess {
                member: "enumerate".into(),
            },
        );
        assert!(only_loop(&cpg, func).is_counted);

        // A call that is not a counting adaptor still leaves the default in
        // place: a `for` is presumed to iterate a bounded subject.
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = func_with_body(&mut cpg);
        let head = wf_child(&mut cpg, body, CpgNodeKind::For);
        let call = wf_child(
            &mut cpg,
            head,
            CpgNodeKind::Call {
                target: None,
                is_method: true,
            },
        );
        wf_child(&mut cpg, call, ident("lines"));
        assert!(
            only_loop(&cpg, func).is_counted,
            "`for` defaults to counted"
        );
    }

    /// A `while`/`do…while` is counted only when it carries *both* a comparison
    /// and an induction step — the syntactic trace of a counter.
    #[test]
    fn conditional_loops_need_a_comparison_and_an_induction_step() {
        /// Builds `<loop_kind> { <extras> }` and reports whether it is counted.
        fn counted(loop_kind: CpgNodeKind, extras: &[CpgNodeKind]) -> bool {
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            let (func, body) = func_with_body(&mut cpg);
            let head = wf_child(&mut cpg, body, loop_kind);
            let inner = wf_child(
                &mut cpg,
                head,
                CpgNodeKind::Block {
                    scope: ScopeId::GLOBAL,
                },
            );
            for e in extras {
                wf_child(&mut cpg, inner, e.clone());
            }
            only_loop(&cpg, func).is_counted
        }

        let cmp = CpgNodeKind::BinaryOp {
            operator: "<".into(),
        };
        let inc = CpgNodeKind::UnaryOp {
            operator: "++".into(),
        };

        for loop_kind in [
            CpgNodeKind::While,
            CpgNodeKind::Unknown {
                kind: "do_statement".into(),
            },
        ] {
            assert!(
                counted(loop_kind.clone(), &[cmp.clone(), inc.clone()]),
                "comparison + increment ⇒ counted"
            );
            assert!(
                !counted(loop_kind.clone(), std::slice::from_ref(&cmp)),
                "a comparison alone does not bound the loop"
            );
            assert!(
                !counted(loop_kind.clone(), std::slice::from_ref(&inc)),
                "an increment alone does not bound the loop"
            );
            assert!(!counted(loop_kind, &[]), "no witnesses ⇒ unbounded");
        }

        // Every comparison operator is recognized …
        for op in ["<", ">", "<=", ">=", "==", "!="] {
            assert!(
                counted(
                    CpgNodeKind::While,
                    &[
                        CpgNodeKind::BinaryOp {
                            operator: op.into()
                        },
                        inc.clone()
                    ],
                ),
                "`{op}` is a comparison"
            );
        }
        // … and a non-comparison operator is not.
        assert!(!counted(
            CpgNodeKind::While,
            &[
                CpgNodeKind::BinaryOp {
                    operator: "+".into()
                },
                inc.clone()
            ],
        ));

        // Every induction step spelling is recognized …
        for op in ["++", "--"] {
            assert!(
                counted(
                    CpgNodeKind::While,
                    &[
                        cmp.clone(),
                        CpgNodeKind::UnaryOp {
                            operator: op.into()
                        }
                    ],
                ),
                "`{op}` is an induction step"
            );
        }
        for op in ["+=", "-=", "*=", "/="] {
            assert!(
                counted(
                    CpgNodeKind::While,
                    &[
                        cmp.clone(),
                        CpgNodeKind::Assignment {
                            operator: op.into()
                        }
                    ],
                ),
                "`{op}` is an induction step"
            );
        }
        // … and a plain assignment is not an induction step.
        assert!(!counted(
            CpgNodeKind::While,
            &[
                cmp.clone(),
                CpgNodeKind::Assignment {
                    operator: "=".into()
                }
            ],
        ));
        // Nor is a literal, which exercises the non-operator fall-through.
        assert!(!counted(
            CpgNodeKind::While,
            &[
                cmp,
                CpgNodeKind::Literal {
                    kind: LiteralKind::Integer(1)
                },
            ],
        ));
    }

    /// Every counting-iterator adaptor marks a `for` as counted, and the
    /// opaque do-while spellings are both recognized.
    #[test]
    fn iterator_adaptor_and_do_while_vocabularies() {
        // The adaptor is named either by a bare identifier or by a method.
        for word in [
            "enumerate",
            "range",
            "take",
            "zip",
            "take_while",
            "char_range",
        ] {
            for as_member in [false, true] {
                let mut cpg = CodePropertyGraph::new(Language::Rust);
                let (func, body) = func_with_body(&mut cpg);
                let head = wf_child(&mut cpg, body, CpgNodeKind::For);
                let call = wf_child(
                    &mut cpg,
                    head,
                    CpgNodeKind::Call {
                        target: None,
                        is_method: as_member,
                    },
                );
                let named = if as_member {
                    CpgNodeKind::MemberAccess {
                        member: word.into(),
                    }
                } else {
                    ident(word)
                };
                wf_child(&mut cpg, call, named);
                assert!(
                    only_loop(&cpg, func).is_counted,
                    "`{word}` (member: {as_member}) is a counting adaptor"
                );
            }
        }

        // Both opaque do-while spellings are recognized as loops.
        for spelling in ["do_while", "do_statement", "do_while_statement"] {
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            let (func, body) = func_with_body(&mut cpg);
            wf_child(
                &mut cpg,
                body,
                CpgNodeKind::Unknown {
                    kind: spelling.into(),
                },
            );
            assert_eq!(
                only_loop(&cpg, func).kind,
                LoopKind::DoWhile,
                "`{spelling}` is a do-while"
            );
        }

        // A range expression written as an opaque node is also a bound.
        for spelling in ["range_expression", "int_range"] {
            let mut cpg = CodePropertyGraph::new(Language::Rust);
            let (func, body) = func_with_body(&mut cpg);
            let head = wf_child(&mut cpg, body, CpgNodeKind::For);
            wf_child(
                &mut cpg,
                head,
                CpgNodeKind::Unknown {
                    kind: spelling.into(),
                },
            );
            assert!(
                only_loop(&cpg, func).is_counted,
                "`{spelling}` bounds the loop"
            );
        }
    }

    /// An infinite loop is never counted, whatever it contains.
    #[test]
    fn infinite_loops_are_never_counted() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let (func, body) = func_with_body(&mut cpg);
        let head = wf_child(&mut cpg, body, CpgNodeKind::Loop);
        let inner = wf_child(
            &mut cpg,
            head,
            CpgNodeKind::Block {
                scope: ScopeId::GLOBAL,
            },
        );
        wf_child(
            &mut cpg,
            inner,
            CpgNodeKind::BinaryOp {
                operator: "<".into(),
            },
        );
        wf_child(
            &mut cpg,
            inner,
            CpgNodeKind::UnaryOp {
                operator: "++".into(),
            },
        );

        let found = only_loop(&cpg, func);
        assert_eq!(found.kind, LoopKind::Infinite);
        assert!(!found.is_counted, "`loop` has no trip bound to read");
    }
}
