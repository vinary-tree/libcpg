//! Complexity estimation from control flow analysis.
//!
//! Uses loop nesting depth and recursion patterns to estimate
//! time and space complexity of functions.

use crate::{CodePropertyGraph, CpgNodeKind, NodeId};
use super::super::signatures::{ComplexityEstimate, ComplexityClass};
use super::control_flow::{ControlFlowAnalyzer, RecursionKind};

/// Analyzes code complexity.
#[derive(Debug, Default)]
pub struct ComplexityAnalyzer {
    control_flow: ControlFlowAnalyzer,
}

impl ComplexityAnalyzer {
    /// Creates a new analyzer.
    pub fn new() -> Self {
        Self {
            control_flow: ControlFlowAnalyzer::new(),
        }
    }

    /// Estimates time complexity of a function.
    ///
    /// Analyzes loop nesting depth and recursion patterns to estimate
    /// the computational complexity.
    ///
    /// ## Heuristics
    ///
    /// | Pattern | Complexity Class |
    /// |---------|------------------|
    /// | No loops/recursion | O(1) |
    /// | Single counted loop | O(n) |
    /// | Nested loops depth=2 | O(n²) |
    /// | Nested loops depth=3 | O(n³) |
    /// | Divide-and-conquer (1 call) | O(log n) |
    /// | Divide-and-conquer (2 calls) | O(n log n) |
    /// | Binary recursion (no D&C) | O(2ⁿ) |
    /// | Tail recursion | O(n) |
    pub fn estimate_time_complexity(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
    ) -> ComplexityEstimate {
        // Detect loops and recursion
        let loops = self.control_flow.detect_loops(cpg, function);
        let recursion = self.control_flow.detect_recursion(cpg, function);

        // Analyze loop complexity
        let loop_complexity = self.analyze_loop_complexity(&loops);

        // Analyze recursion complexity
        let recursion_complexity = recursion
            .as_ref()
            .map(|r| self.analyze_recursion_complexity(cpg, function, r));

        // Combine: take the dominant (worse) complexity
        match (loop_complexity, recursion_complexity) {
            (None, None) => ComplexityEstimate {
                class: ComplexityClass::Constant,
                confidence: 0.9,
                justification: "No loops or recursion detected".to_string(),
            },
            (Some(loop_est), None) => loop_est,
            (None, Some(rec_est)) => rec_est,
            (Some(loop_est), Some(rec_est)) => {
                // Take the worse complexity
                if rec_est.class.is_better_than(&loop_est.class) {
                    loop_est
                } else {
                    rec_est
                }
            }
        }
    }

    /// Estimates space complexity of a function.
    ///
    /// Analyzes recursion depth and data structure allocations.
    pub fn estimate_space_complexity(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
    ) -> ComplexityEstimate {
        let recursion = self.control_flow.detect_recursion(cpg, function);

        // Check for recursive calls (which use stack space)
        if let Some(rec) = recursion {
            let space_class = match rec.kind {
                RecursionKind::Tail => {
                    // Tail recursion can be optimized to O(1) space
                    ComplexityClass::Constant
                }
                RecursionKind::Direct => {
                    // Direct recursion uses O(n) stack space typically
                    ComplexityClass::Linear
                }
                RecursionKind::Indirect => {
                    // Indirect recursion - assume linear
                    ComplexityClass::Linear
                }
            };

            return ComplexityEstimate {
                class: space_class,
                confidence: 0.6,
                justification: format!(
                    "{:?} recursion detected, stack depth estimated",
                    rec.kind
                ),
            };
        }

        // Check for array/collection allocations
        let has_allocations = self.detect_allocations(cpg, function);

        if has_allocations {
            ComplexityEstimate {
                class: ComplexityClass::Linear,
                confidence: 0.5,
                justification: "Data structure allocations detected".to_string(),
            }
        } else {
            ComplexityEstimate {
                class: ComplexityClass::Constant,
                confidence: 0.7,
                justification: "No significant allocations or recursion detected".to_string(),
            }
        }
    }

    /// Analyzes loop structure to estimate time complexity.
    fn analyze_loop_complexity(
        &self,
        loops: &[super::control_flow::LoopPattern],
    ) -> Option<ComplexityEstimate> {
        if loops.is_empty() {
            return None;
        }

        // Find maximum nesting depth
        let max_depth = loops.iter().map(|l| l.depth).max().unwrap_or(0);

        // Check if loops are counted (bounded)
        let all_counted = loops.iter().all(|l| l.is_counted);

        // Determine complexity based on nesting depth
        let (class, confidence, justification) = match max_depth {
            0 => (ComplexityClass::Constant, 0.9, "No loops".to_string()),
            1 => {
                if all_counted {
                    (
                        ComplexityClass::Linear,
                        0.8,
                        "Single counted loop detected".to_string(),
                    )
                } else {
                    (
                        ComplexityClass::Linear,
                        0.6,
                        "Single loop (possibly unbounded)".to_string(),
                    )
                }
            }
            2 => {
                if all_counted {
                    (
                        ComplexityClass::Quadratic,
                        0.8,
                        "Nested loops (depth 2) detected".to_string(),
                    )
                } else {
                    (
                        ComplexityClass::Quadratic,
                        0.5,
                        "Nested loops (depth 2, possibly unbounded)".to_string(),
                    )
                }
            }
            3 => (
                ComplexityClass::Cubic,
                0.7,
                "Triple nested loops detected".to_string(),
            ),
            d => (
                ComplexityClass::Polynomial(d as u32),
                0.6,
                format!("Nested loops (depth {}) detected", d),
            ),
        };

        Some(ComplexityEstimate {
            class,
            confidence,
            justification,
        })
    }

    /// Analyzes recursion pattern to estimate time complexity.
    fn analyze_recursion_complexity(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
        recursion: &super::control_flow::RecursionPattern,
    ) -> ComplexityEstimate {
        let num_recursive_calls = recursion.recursive_calls.len();
        let has_base_cases = !recursion.base_cases.is_empty();

        // Check for divide-and-conquer pattern
        let is_divide_conquer = self.is_divide_and_conquer(cpg, function, &recursion.recursive_calls);

        let (class, confidence, justification) = match recursion.kind {
            RecursionKind::Tail => (
                ComplexityClass::Linear,
                0.8,
                "Tail recursion detected (equivalent to iteration)".to_string(),
            ),
            RecursionKind::Direct | RecursionKind::Indirect => {
                if is_divide_conquer {
                    match num_recursive_calls {
                        1 => (
                            ComplexityClass::Logarithmic,
                            0.8,
                            "Binary search pattern detected (single recursive call with halving)".to_string(),
                        ),
                        2 => (
                            ComplexityClass::Linearithmic,
                            0.7,
                            "Merge sort/divide-and-conquer pattern detected (2 calls with halving)".to_string(),
                        ),
                        _ => (
                            ComplexityClass::Polynomial(num_recursive_calls as u32),
                            0.5,
                            format!("Divide-and-conquer with {} recursive calls", num_recursive_calls),
                        ),
                    }
                } else {
                    // Non-divide-and-conquer recursion
                    match num_recursive_calls {
                        1 => {
                            if has_base_cases {
                                (
                                    ComplexityClass::Linear,
                                    0.7,
                                    "Linear recursion detected".to_string(),
                                )
                            } else {
                                (
                                    ComplexityClass::Linear,
                                    0.4,
                                    "Recursion without clear base case".to_string(),
                                )
                            }
                        }
                        2 => (
                            ComplexityClass::Exponential,
                            0.7,
                            "Binary recursion (e.g., naive Fibonacci) detected".to_string(),
                        ),
                        _ => (
                            ComplexityClass::Exponential,
                            0.6,
                            format!("Multiple recursive calls ({}) detected", num_recursive_calls),
                        ),
                    }
                }
            }
        };

        ComplexityEstimate {
            class,
            confidence,
            justification,
        }
    }

    /// Detects if the recursion follows a divide-and-conquer pattern.
    ///
    /// Looks for:
    /// - Input being halved/divided before recursive calls
    /// - Binary search patterns (mid calculation)
    fn is_divide_and_conquer(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
        _recursive_calls: &[NodeId],
    ) -> bool {
        let descendants = cpg.ast_descendants(function);

        // Look for division by 2 or right shift (common in D&C)
        for &node_id in &descendants {
            if let Some(node) = cpg.node(node_id) {
                match &node.kind {
                    CpgNodeKind::BinaryOp { operator } => {
                        let op = operator.as_ref();
                        if op == "/" || op == ">>" {
                            // Check if dividing by 2
                            let children = cpg.ast_children(node_id);
                            for child_id in children {
                                if let Some(child) = cpg.node(child_id) {
                                    if let CpgNodeKind::Literal { kind } = &child.kind {
                                        if matches!(kind, crate::LiteralKind::Integer(2)) {
                                            return true;
                                        }
                                    }
                                }
                            }
                        }
                    }
                    CpgNodeKind::Identifier { name, .. } => {
                        // Look for common divide-and-conquer variable names
                        let name_lower = name.to_lowercase();
                        if name_lower == "mid"
                            || name_lower == "middle"
                            || name_lower == "pivot"
                            || name_lower.contains("half")
                        {
                            return true;
                        }
                    }
                    _ => {}
                }
            }
        }

        false
    }

    /// Detects if the function allocates significant data structures.
    fn detect_allocations(&self, cpg: &CodePropertyGraph, function: NodeId) -> bool {
        let descendants = cpg.ast_descendants(function);

        for &node_id in &descendants {
            if let Some(node) = cpg.node(node_id) {
                match &node.kind {
                    // Look for array/vector/collection creation
                    CpgNodeKind::Literal { kind } => {
                        if matches!(kind, crate::LiteralKind::Array) {
                            return true;
                        }
                    }
                    CpgNodeKind::Call { .. } => {
                        // Check for allocation-related function names
                        let children = cpg.ast_children(node_id);
                        for child_id in children {
                            if let Some(child) = cpg.node(child_id) {
                                if let CpgNodeKind::Identifier { name, .. }
                                    | CpgNodeKind::MemberAccess { member: name } = &child.kind
                                {
                                    let name_lower = name.to_lowercase();
                                    if name_lower.contains("vec")
                                        || name_lower.contains("new")
                                        || name_lower.contains("alloc")
                                        || name_lower.contains("create")
                                        || name_lower.contains("clone")
                                        || name_lower.contains("collect")
                                    {
                                        return true;
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CpgNode, Language, SourceRange, MethodSignature, Visibility, CpgEdgeKind, ScopeId};

    fn create_function(cpg: &mut CodePropertyGraph, name: &str) -> NodeId {
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
    fn test_constant_complexity() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = create_function(&mut cpg, "simple");

        let analyzer = ComplexityAnalyzer::new();
        let estimate = analyzer.estimate_time_complexity(&cpg, func);

        assert_eq!(estimate.class, ComplexityClass::Constant);
        assert!(estimate.confidence > 0.5);
    }

    #[test]
    fn test_linear_complexity_single_loop() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = create_function(&mut cpg, "linear");

        let mut block = CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Block { scope: ScopeId::GLOBAL },
            SourceRange::default(),
        );
        block.parent = Some(func);
        let block_id = cpg.add_node(block);
        cpg.connect(func, block_id, CpgEdgeKind::AstChild);

        let mut for_loop = CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::For,
            SourceRange::default(),
        );
        for_loop.parent = Some(block_id);
        let loop_id = cpg.add_node(for_loop);
        cpg.connect(block_id, loop_id, CpgEdgeKind::AstChild);

        let analyzer = ComplexityAnalyzer::new();
        let estimate = analyzer.estimate_time_complexity(&cpg, func);

        assert_eq!(estimate.class, ComplexityClass::Linear);
    }

    #[test]
    fn test_quadratic_complexity_nested_loops() {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = create_function(&mut cpg, "quadratic");

        let mut block = CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Block { scope: ScopeId::GLOBAL },
            SourceRange::default(),
        );
        block.parent = Some(func);
        let block_id = cpg.add_node(block);
        cpg.connect(func, block_id, CpgEdgeKind::AstChild);

        let mut outer = CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::For,
            SourceRange::default(),
        );
        outer.parent = Some(block_id);
        let outer_id = cpg.add_node(outer);
        cpg.connect(block_id, outer_id, CpgEdgeKind::AstChild);

        let mut inner = CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::For,
            SourceRange::default(),
        );
        inner.parent = Some(outer_id);
        let inner_id = cpg.add_node(inner);
        cpg.connect(outer_id, inner_id, CpgEdgeKind::AstChild);

        let analyzer = ComplexityAnalyzer::new();
        let estimate = analyzer.estimate_time_complexity(&cpg, func);

        assert_eq!(estimate.class, ComplexityClass::Quadratic);
    }

    #[test]
    fn test_analyzer_creation() {
        let analyzer = ComplexityAnalyzer::new();
        let default_analyzer = ComplexityAnalyzer::default();
        let _ = format!("{:?}", analyzer);
        let _ = format!("{:?}", default_analyzer);
    }
}
