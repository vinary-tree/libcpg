//! Algorithm detection from CPGs.
//!
//! Analyzes control flow patterns and data access patterns
//! to identify common algorithm families.

mod complexity;
mod control_flow;

pub use complexity::ComplexityAnalyzer;
pub use control_flow::{
    ControlFlowAnalyzer, LoopKind, LoopPattern, RecursionKind, RecursionPattern,
};

use super::families::AlgorithmFamily;
use super::signatures::{
    AlgorithmSignature, ComplexityClass, ComplexityEstimate, LoopBounds, LoopStructure, LoopType,
    RecursionPattern as SigRecursionPattern, ReductionPattern, SigLoopKind, SigRecursionKind,
};
use crate::{CodePropertyGraph, CpgNodeKind, NodeId};

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Trait for algorithm detection.
pub trait AlgorithmDetector: Send + Sync {
    /// Detects algorithms in a function.
    fn detect(&self, cpg: &CodePropertyGraph, function: NodeId) -> Vec<DetectedAlgorithm>;

    /// Returns the algorithm families this detector can identify.
    fn supported_families(&self) -> &[AlgorithmFamily];
}

/// A detected algorithm instance.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DetectedAlgorithm {
    /// The algorithm family.
    pub family: AlgorithmFamily,
    /// Specific algorithm name (if identifiable).
    pub name: Option<String>,
    /// The function containing this algorithm.
    pub function: NodeId,
    /// Key nodes in the implementation.
    pub key_nodes: Vec<NodeId>,
    /// Extracted signature.
    pub signature: AlgorithmSignature,
    /// Confidence score (0.0 to 1.0).
    pub confidence: f64,
}

impl DetectedAlgorithm {
    /// Creates a new detected algorithm.
    pub fn new(family: AlgorithmFamily, function: NodeId, confidence: f64) -> Self {
        Self {
            family,
            name: None,
            function,
            key_nodes: Vec::new(),
            signature: AlgorithmSignature::default(),
            confidence,
        }
    }

    /// Sets the algorithm name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Adds a key node.
    pub fn with_key_node(mut self, node: NodeId) -> Self {
        self.key_nodes.push(node);
        self
    }

    /// Sets the signature.
    pub fn with_signature(mut self, signature: AlgorithmSignature) -> Self {
        self.signature = signature;
        self
    }
}

/// Default algorithm detector using CFG analysis.
#[derive(Debug, Default)]
pub struct DefaultAlgorithmDetector {
    /// Minimum confidence threshold.
    min_confidence: f64,
    /// Control flow analyzer.
    control_flow: ControlFlowAnalyzer,
    /// Complexity analyzer.
    complexity: ComplexityAnalyzer,
}

impl DefaultAlgorithmDetector {
    /// Creates a new detector.
    pub fn new() -> Self {
        Self {
            min_confidence: 0.5,
            control_flow: ControlFlowAnalyzer::new(),
            complexity: ComplexityAnalyzer::new(),
        }
    }

    /// Sets the minimum confidence threshold.
    pub fn with_min_confidence(mut self, confidence: f64) -> Self {
        self.min_confidence = confidence;
        self
    }

    /// Detects sorting algorithm patterns.
    fn detect_sorting(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
        loops: &[LoopPattern],
        time_complexity: &ComplexityEstimate,
    ) -> Option<DetectedAlgorithm> {
        // Sorting algorithms typically have:
        // - Nested loops (O(n²)) OR recursion with divide (O(n log n))
        // - Comparison operations
        // - Swap operations or array assignments

        if loops.is_empty() {
            return None;
        }

        // Check for comparison and swap patterns
        let has_comparisons = self.has_comparison_pattern(cpg, function);
        let has_swaps = self.has_swap_pattern(cpg, function);
        let has_array_access = self.has_array_access(cpg, function);

        if !has_comparisons {
            return None;
        }

        // Different sorting heuristics based on complexity
        let (confidence, name) = match time_complexity.class {
            ComplexityClass::Quadratic => {
                // O(n²) sorting: bubble, insertion, selection
                if has_swaps {
                    if self.has_adjacent_swap_pattern(cpg, function) {
                        (0.7, Some("Bubble Sort".to_string()))
                    } else {
                        (0.6, Some("Selection Sort".to_string()))
                    }
                } else if has_array_access {
                    (0.6, Some("Insertion Sort".to_string()))
                } else {
                    (0.5, None)
                }
            }
            ComplexityClass::Linearithmic => {
                // O(n log n) sorting: merge, quick, heap
                (0.6, None)
            }
            ComplexityClass::Linear => {
                // Could be radix/counting sort
                (0.4, None)
            }
            _ => return None,
        };

        if confidence < self.min_confidence {
            return None;
        }

        let mut algo = DetectedAlgorithm::new(AlgorithmFamily::Sorting, function, confidence);
        if let Some(n) = name {
            algo = algo.with_name(n);
        }
        algo = algo.with_signature(self.build_signature(loops, None, time_complexity.clone()));

        Some(algo)
    }

    /// Detects searching algorithm patterns.
    fn detect_searching(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
        loops: &[LoopPattern],
        recursion: Option<&RecursionPattern>,
        time_complexity: &ComplexityEstimate,
    ) -> Option<DetectedAlgorithm> {
        // Searching algorithms typically have:
        // - A single loop OR divide-and-conquer recursion
        // - Comparison operations
        // - Early return (found/not found)

        let has_comparisons = self.has_comparison_pattern(cpg, function);
        let has_early_return = self.has_early_return(cpg, function);

        if !has_comparisons {
            return None;
        }

        let (confidence, name) = match time_complexity.class {
            ComplexityClass::Logarithmic => {
                // Binary search pattern
                if recursion.is_some() || self.has_midpoint_calculation(cpg, function) {
                    (0.8, Some("Binary Search".to_string()))
                } else {
                    (0.6, None)
                }
            }
            ComplexityClass::Linear => {
                // Linear search
                if loops.len() == 1 && has_early_return {
                    (0.7, Some("Linear Search".to_string()))
                } else {
                    (0.5, None)
                }
            }
            _ => return None,
        };

        if confidence < self.min_confidence {
            return None;
        }

        let mut algo = DetectedAlgorithm::new(AlgorithmFamily::Searching, function, confidence);
        if let Some(n) = name {
            algo = algo.with_name(n);
        }

        Some(algo)
    }

    /// Detects graph algorithm patterns.
    fn detect_graph(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
        loops: &[LoopPattern],
    ) -> Option<DetectedAlgorithm> {
        // Graph algorithms typically have:
        // - Queue or stack operations (BFS/DFS)
        // - Visited set/array
        // - Adjacency list traversal

        let has_queue = self.has_queue_operations(cpg, function);
        let has_stack = self.has_stack_operations(cpg, function);
        let has_visited = self.has_visited_tracking(cpg, function);

        if !has_visited {
            return None;
        }

        let (confidence, name) = if has_queue {
            (0.7, Some("BFS".to_string()))
        } else if has_stack || loops.iter().any(|l| matches!(l.kind, LoopKind::While)) {
            (0.6, Some("DFS".to_string()))
        } else {
            return None;
        };

        if confidence < self.min_confidence {
            return None;
        }

        let mut algo =
            DetectedAlgorithm::new(AlgorithmFamily::GraphTraversal, function, confidence);
        if let Some(n) = name {
            algo = algo.with_name(n);
        }

        Some(algo)
    }

    /// Detects dynamic programming patterns.
    fn detect_dp(
        &self,
        cpg: &CodePropertyGraph,
        function: NodeId,
        loops: &[LoopPattern],
    ) -> Option<DetectedAlgorithm> {
        // DP algorithms typically have:
        // - Memoization table (array/hashmap)
        // - Nested loops OR recursion with memoization
        // - State transitions (assignments to table)

        let has_table = self.has_memoization_table(cpg, function);
        let has_nested_loops = loops.iter().any(|l| l.depth >= 2);

        if !has_table {
            return None;
        }

        let confidence = if has_nested_loops { 0.7 } else { 0.5 };

        if confidence < self.min_confidence {
            return None;
        }

        Some(DetectedAlgorithm::new(
            AlgorithmFamily::DynamicProgramming,
            function,
            confidence,
        ))
    }

    /// Detects divide-and-conquer patterns.
    fn detect_divide_conquer(
        &self,
        _cpg: &CodePropertyGraph,
        function: NodeId,
        recursion: Option<&RecursionPattern>,
        time_complexity: &ComplexityEstimate,
    ) -> Option<DetectedAlgorithm> {
        let rec = recursion?;

        // D&C typically has:
        // - Recursion with input halving
        // - Multiple recursive calls (2 for merge sort, 1-2 for quicksort)
        // - O(n log n) or O(log n) complexity

        let is_dc_complexity = matches!(
            time_complexity.class,
            ComplexityClass::Logarithmic | ComplexityClass::Linearithmic
        );

        if !is_dc_complexity {
            return None;
        }

        // Check number of recursive calls
        let call_count = rec.recursive_calls.len();
        let confidence = match call_count {
            1 => 0.6, // Binary search pattern
            2 => 0.7, // Merge sort / typical D&C
            _ => 0.5,
        };

        if confidence < self.min_confidence {
            return None;
        }

        Some(DetectedAlgorithm::new(
            AlgorithmFamily::DivideAndConquer,
            function,
            confidence,
        ))
    }

    /// Builds an algorithm signature from detected patterns.
    fn build_signature(
        &self,
        loops: &[LoopPattern],
        recursion: Option<&RecursionPattern>,
        time_complexity: ComplexityEstimate,
    ) -> AlgorithmSignature {
        let mut sig = AlgorithmSignature::new().with_time_complexity(time_complexity);

        if !loops.is_empty() {
            let max_depth = loops.iter().map(|l| l.depth).max().unwrap_or(0);
            let loop_types: Vec<LoopType> = loops
                .iter()
                .map(|l| {
                    let sig_kind = match l.kind {
                        LoopKind::For => SigLoopKind::CountedFor,
                        LoopKind::While => SigLoopKind::While,
                        LoopKind::DoWhile => SigLoopKind::DoWhile,
                        LoopKind::Infinite => SigLoopKind::Infinite,
                        LoopKind::Iterator => SigLoopKind::ForEach,
                    };
                    LoopType {
                        header: l.header,
                        kind: sig_kind,
                        depth: l.depth,
                        bounds: if l.is_counted {
                            LoopBounds::LinearN
                        } else {
                            LoopBounds::Unknown
                        },
                        has_early_exit: false,
                    }
                })
                .collect();

            let loop_structure = LoopStructure {
                max_depth,
                loop_count: loops.len(),
                loop_types,
                loops_independent: false, // Conservative default
            };
            sig = sig.with_loop_structure(loop_structure);
        }

        if let Some(r) = recursion {
            let sig_kind = match r.kind {
                RecursionKind::Direct => SigRecursionKind::Direct,
                RecursionKind::Indirect => SigRecursionKind::Indirect,
                RecursionKind::Tail => SigRecursionKind::Tail,
            };

            let recursion_pattern = SigRecursionPattern {
                kind: sig_kind,
                base_cases: r.base_cases.clone(),
                recursive_calls: r.recursive_calls.clone(),
                reduction: ReductionPattern::Unknown,
                tail_optimizable: matches!(r.kind, RecursionKind::Tail),
            };
            sig = sig.with_recursion(recursion_pattern);
        }

        sig
    }

    // --- Pattern detection helpers ---

    /// Checks for comparison operations (==, <, >, etc.).
    fn has_comparison_pattern(&self, cpg: &CodePropertyGraph, function: NodeId) -> bool {
        let descendants = cpg.ast_descendants(function);
        descendants.iter().any(|&id| {
            cpg.node(id)
                .map(|n| {
                    matches!(&n.kind, CpgNodeKind::BinaryOp { operator }
                    if matches!(operator.as_ref(), "<" | ">" | "<=" | ">=" | "==" | "!="))
                })
                .unwrap_or(false)
        })
    }

    /// Checks for swap patterns (temp = a; a = b; b = temp).
    fn has_swap_pattern(&self, cpg: &CodePropertyGraph, function: NodeId) -> bool {
        let descendants = cpg.ast_descendants(function);

        // Look for multiple assignments that suggest swapping
        let assignment_count = descendants
            .iter()
            .filter(|&&id| {
                cpg.node(id)
                    .map(|n| matches!(n.kind, CpgNodeKind::Assignment { .. }))
                    .unwrap_or(false)
            })
            .count();

        // Also check for std::mem::swap or similar calls
        let has_swap_call = descendants.iter().any(|&id| {
            cpg.node(id)
                .map(|n| {
                    if let CpgNodeKind::Identifier { name, .. }
                    | CpgNodeKind::MemberAccess { member: name } = &n.kind
                    {
                        name.to_lowercase().contains("swap")
                    } else {
                        false
                    }
                })
                .unwrap_or(false)
        });

        assignment_count >= 3 || has_swap_call
    }

    /// Checks for adjacent element swap pattern (arr[i] <-> arr[i+1]).
    fn has_adjacent_swap_pattern(&self, cpg: &CodePropertyGraph, function: NodeId) -> bool {
        // Look for index expressions like arr[i] and arr[i+1] near swaps
        let descendants = cpg.ast_descendants(function);

        descendants.iter().any(|&id| {
            cpg.node(id)
                .map(|n| {
                    if let CpgNodeKind::BinaryOp { operator } = &n.kind {
                        // Look for i+1 pattern
                        let op = operator.as_ref();
                        if op == "+" {
                            let children = cpg.ast_children(id);
                            children.iter().any(|&child_id| {
                                cpg.node(child_id)
                                    .map(|c| {
                                        matches!(
                                            &c.kind,
                                            CpgNodeKind::Literal {
                                                kind: crate::LiteralKind::Integer(1)
                                            }
                                        )
                                    })
                                    .unwrap_or(false)
                            })
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                })
                .unwrap_or(false)
        })
    }

    /// Checks for array/slice access patterns.
    fn has_array_access(&self, cpg: &CodePropertyGraph, function: NodeId) -> bool {
        let descendants = cpg.ast_descendants(function);
        descendants.iter().any(|&id| {
            cpg.node(id)
                .map(|n| matches!(n.kind, CpgNodeKind::IndexAccess))
                .unwrap_or(false)
        })
    }

    /// Checks for early return statements (used in searching).
    fn has_early_return(&self, cpg: &CodePropertyGraph, function: NodeId) -> bool {
        let descendants = cpg.ast_descendants(function);

        // Look for return inside a loop or conditional
        descendants.iter().any(|&id| {
            if cpg
                .node(id)
                .map(|n| matches!(n.kind, CpgNodeKind::Return))
                .unwrap_or(false)
            {
                // Check if inside a loop
                let ancestors = cpg.ast_ancestors(id);
                ancestors.iter().any(|&anc| {
                    cpg.node(anc)
                        .map(|n| {
                            matches!(
                                n.kind,
                                CpgNodeKind::For | CpgNodeKind::While | CpgNodeKind::Loop
                            )
                        })
                        .unwrap_or(false)
                })
            } else {
                false
            }
        })
    }

    /// Checks for midpoint calculation (mid = (low + high) / 2).
    fn has_midpoint_calculation(&self, cpg: &CodePropertyGraph, function: NodeId) -> bool {
        let descendants = cpg.ast_descendants(function);

        // Look for division by 2 or right shift by 1
        descendants.iter().any(|&id| {
            cpg.node(id)
                .map(|n| {
                    if let CpgNodeKind::BinaryOp { operator } = &n.kind {
                        let op = operator.as_ref();
                        if op == "/" || op == ">>" {
                            let children = cpg.ast_children(id);
                            children.iter().any(|&child_id| {
                                cpg.node(child_id)
                                    .map(|c| {
                                        matches!(
                                            &c.kind,
                                            CpgNodeKind::Literal {
                                                kind: crate::LiteralKind::Integer(2)
                                            }
                                        ) || matches!(
                                            &c.kind,
                                            CpgNodeKind::Literal {
                                                kind: crate::LiteralKind::Integer(1)
                                            }
                                        )
                                    })
                                    .unwrap_or(false)
                            })
                        } else {
                            false
                        }
                    } else if let CpgNodeKind::Identifier { name, .. } = &n.kind {
                        let name_lower = name.to_lowercase();
                        name_lower == "mid" || name_lower == "middle"
                    } else {
                        false
                    }
                })
                .unwrap_or(false)
        })
    }

    /// Checks for queue operations (push_back, pop_front, etc.).
    fn has_queue_operations(&self, cpg: &CodePropertyGraph, function: NodeId) -> bool {
        let descendants = cpg.ast_descendants(function);

        descendants.iter().any(|&id| {
            cpg.node(id)
                .map(|n| {
                    if let CpgNodeKind::Identifier { name, .. }
                    | CpgNodeKind::MemberAccess { member: name } = &n.kind
                    {
                        let name_lower = name.to_lowercase();
                        name_lower.contains("queue")
                            || name_lower.contains("push_back")
                            || name_lower.contains("pop_front")
                            || name_lower.contains("deque")
                            || name_lower.contains("enqueue")
                            || name_lower.contains("dequeue")
                    } else {
                        false
                    }
                })
                .unwrap_or(false)
        })
    }

    /// Checks for stack operations (push, pop on stack-like structure).
    fn has_stack_operations(&self, cpg: &CodePropertyGraph, function: NodeId) -> bool {
        let descendants = cpg.ast_descendants(function);

        descendants.iter().any(|&id| {
            cpg.node(id)
                .map(|n| {
                    if let CpgNodeKind::Identifier { name, .. }
                    | CpgNodeKind::MemberAccess { member: name } = &n.kind
                    {
                        let name_lower = name.to_lowercase();
                        name_lower.contains("stack")
                            || (name_lower == "push" || name_lower == "pop")
                    } else {
                        false
                    }
                })
                .unwrap_or(false)
        })
    }

    /// Checks for visited/seen tracking patterns.
    fn has_visited_tracking(&self, cpg: &CodePropertyGraph, function: NodeId) -> bool {
        let descendants = cpg.ast_descendants(function);

        descendants.iter().any(|&id| {
            cpg.node(id)
                .map(|n| {
                    if let CpgNodeKind::Identifier { name, .. } = &n.kind {
                        let name_lower = name.to_lowercase();
                        name_lower.contains("visited")
                            || name_lower.contains("seen")
                            || name_lower.contains("marked")
                    } else {
                        false
                    }
                })
                .unwrap_or(false)
        })
    }

    /// Checks for memoization table patterns.
    fn has_memoization_table(&self, cpg: &CodePropertyGraph, function: NodeId) -> bool {
        let descendants = cpg.ast_descendants(function);

        descendants.iter().any(|&id| {
            cpg.node(id)
                .map(|n| {
                    if let CpgNodeKind::Identifier { name, .. } = &n.kind {
                        let name_lower = name.to_lowercase();
                        name_lower.contains("memo")
                            || name_lower.contains("dp")
                            || name_lower.contains("cache")
                            || name_lower.contains("table")
                            || name_lower == "f"
                            || name_lower == "dp_table"
                    } else {
                        false
                    }
                })
                .unwrap_or(false)
        })
    }
}

impl AlgorithmDetector for DefaultAlgorithmDetector {
    fn detect(&self, cpg: &CodePropertyGraph, function: NodeId) -> Vec<DetectedAlgorithm> {
        let mut detections = Vec::new();

        // Phase 1: Control flow analysis
        let loops = self.control_flow.detect_loops(cpg, function);
        let recursion = self.control_flow.detect_recursion(cpg, function);

        // Phase 2: Complexity estimation
        let time_complexity = self.complexity.estimate_time_complexity(cpg, function);

        // Phase 3: Algorithm family detection
        // Try each detector and collect matches above threshold

        // Sorting detection
        if let Some(algo) = self.detect_sorting(cpg, function, &loops, &time_complexity) {
            detections.push(algo);
        }

        // Searching detection
        if let Some(algo) =
            self.detect_searching(cpg, function, &loops, recursion.as_ref(), &time_complexity)
        {
            detections.push(algo);
        }

        // Graph algorithm detection
        if let Some(algo) = self.detect_graph(cpg, function, &loops) {
            detections.push(algo);
        }

        // Dynamic programming detection
        if let Some(algo) = self.detect_dp(cpg, function, &loops) {
            detections.push(algo);
        }

        // Divide and conquer detection
        if let Some(algo) =
            self.detect_divide_conquer(cpg, function, recursion.as_ref(), &time_complexity)
        {
            detections.push(algo);
        }

        // Sort by confidence (highest first)
        detections.sort_by(|a, b| {
            b.confidence
                .partial_cmp(&a.confidence)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        detections
    }

    fn supported_families(&self) -> &[AlgorithmFamily] {
        &[
            AlgorithmFamily::Sorting,
            AlgorithmFamily::Searching,
            AlgorithmFamily::GraphTraversal,
            AlgorithmFamily::DynamicProgramming,
            AlgorithmFamily::DivideAndConquer,
            AlgorithmFamily::Greedy,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::wf_child;
    use crate::{CpgNode, Language, MethodSignature, ScopeId, SourceRange, Visibility};

    #[test]
    fn test_detector_creation() {
        let detector = DefaultAlgorithmDetector::new().with_min_confidence(0.7);

        assert_eq!(detector.min_confidence, 0.7);
        assert!(!detector.supported_families().is_empty());
    }

    #[test]
    fn test_detected_algorithm() {
        let algo = DetectedAlgorithm::new(AlgorithmFamily::Sorting, NodeId::new(1), 0.9)
            .with_name("quicksort")
            .with_key_node(NodeId::new(2));

        assert_eq!(algo.family, AlgorithmFamily::Sorting);
        assert_eq!(algo.name, Some("quicksort".to_string()));
        assert_eq!(algo.key_nodes.len(), 1);
    }

    #[test]
    fn test_detected_algorithm_with_signature() {
        let sig = AlgorithmSignature::new();
        let algo = DetectedAlgorithm::new(AlgorithmFamily::Searching, NodeId::new(4), 0.6)
            .with_signature(sig);
        assert!(algo.signature.time_complexity.is_none());
        assert_eq!(algo.function, NodeId::new(4));
    }

    // --- hand-built CPGs seeded with heuristic tokens ---

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

    fn ident(name: &str) -> CpgNodeKind {
        CpgNodeKind::Identifier {
            name: name.into(),
            definition: None,
        }
    }

    fn block() -> CpgNodeKind {
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        }
    }

    /// `fn bubble() { for { for { arr[j] > arr[j+1]; t=..; a=..; b=..; } } }`.
    /// Nested counted loops (⇒ O(n²)) + comparison + 3 assignments (swap) +
    /// an `i+1` adjacent index ⇒ classified as Bubble Sort.
    fn build_bubble_sort_cpg() -> (CodePropertyGraph, NodeId) {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = make_function(&mut cpg, "bubble");
        let body = wf_child(&mut cpg, func, block());
        let outer = wf_child(&mut cpg, body, CpgNodeKind::For);
        let outer_body = wf_child(&mut cpg, outer, block());
        let inner = wf_child(&mut cpg, outer_body, CpgNodeKind::For);
        let inner_body = wf_child(&mut cpg, inner, block());

        wf_child(
            &mut cpg,
            inner_body,
            CpgNodeKind::BinaryOp {
                operator: ">".into(),
            },
        );
        wf_child(
            &mut cpg,
            inner_body,
            CpgNodeKind::Assignment {
                operator: "=".into(),
            },
        );
        wf_child(
            &mut cpg,
            inner_body,
            CpgNodeKind::Assignment {
                operator: "=".into(),
            },
        );
        wf_child(
            &mut cpg,
            inner_body,
            CpgNodeKind::Assignment {
                operator: "=".into(),
            },
        );
        let plus = wf_child(
            &mut cpg,
            inner_body,
            CpgNodeKind::BinaryOp {
                operator: "+".into(),
            },
        );
        wf_child(
            &mut cpg,
            plus,
            CpgNodeKind::Literal {
                kind: crate::LiteralKind::Integer(1),
            },
        );
        wf_child(&mut cpg, inner_body, CpgNodeKind::IndexAccess);
        (cpg, func)
    }

    #[test]
    fn test_detect_bubble_sort() {
        let (cpg, func) = build_bubble_sort_cpg();
        let results = DefaultAlgorithmDetector::new().detect(&cpg, func);

        assert!(!results.is_empty(), "expected a detection");
        let sort = results
            .iter()
            .find(|r| r.family == AlgorithmFamily::Sorting)
            .expect("sorting family should be detected");
        assert_eq!(sort.name.as_deref(), Some("Bubble Sort"));
        assert!(sort.confidence >= 0.5);
        // Signature carries the quadratic time estimate + loop structure.
        let sig = &sort.signature;
        assert_eq!(
            sig.time_complexity.as_ref().expect("time complexity").class,
            ComplexityClass::Quadratic
        );
        assert_eq!(
            sig.loop_structure
                .as_ref()
                .expect("loop structure")
                .max_depth,
            2
        );
    }

    #[test]
    fn test_detect_binary_search() {
        // fn binary_search() { mid; a < b; binary_search(); }
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = make_function(&mut cpg, "binary_search");
        let body = wf_child(&mut cpg, func, block());
        wf_child(&mut cpg, body, ident("mid")); // midpoint token ⇒ divide & conquer
        wf_child(
            &mut cpg,
            body,
            CpgNodeKind::BinaryOp {
                operator: "<".into(),
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
        wf_child(&mut cpg, call, ident("binary_search")); // direct self-call

        let results = DefaultAlgorithmDetector::new().detect(&cpg, func);
        let search = results
            .iter()
            .find(|r| r.family == AlgorithmFamily::Searching)
            .expect("searching family should be detected");
        assert_eq!(search.name.as_deref(), Some("Binary Search"));
        // Divide-and-conquer is also recognised (at lower confidence).
        assert!(results
            .iter()
            .any(|r| r.family == AlgorithmFamily::DivideAndConquer));
        // Results are sorted by descending confidence.
        for w in results.windows(2) {
            assert!(w[0].confidence >= w[1].confidence);
        }
    }

    #[test]
    fn test_detect_bfs_graph_traversal() {
        // fn bfs() { visited; queue; }
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = make_function(&mut cpg, "bfs");
        let body = wf_child(&mut cpg, func, block());
        wf_child(&mut cpg, body, ident("visited"));
        wf_child(&mut cpg, body, ident("queue"));

        let results = DefaultAlgorithmDetector::new().detect(&cpg, func);
        let g = results
            .iter()
            .find(|r| r.family == AlgorithmFamily::GraphTraversal)
            .expect("graph traversal should be detected");
        assert_eq!(g.name.as_deref(), Some("BFS"));
        assert!(g.confidence >= 0.5);
    }

    #[test]
    fn test_detect_dynamic_programming() {
        // fn knapsack() { memo; for { for { } } }
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = make_function(&mut cpg, "knapsack");
        let body = wf_child(&mut cpg, func, block());
        wf_child(&mut cpg, body, ident("memo")); // memoization table token
        let outer = wf_child(&mut cpg, body, CpgNodeKind::For);
        let outer_body = wf_child(&mut cpg, outer, block());
        let inner = wf_child(&mut cpg, outer_body, CpgNodeKind::For);
        wf_child(&mut cpg, inner, block());

        let results = DefaultAlgorithmDetector::new().detect(&cpg, func);
        let dp = results
            .iter()
            .find(|r| r.family == AlgorithmFamily::DynamicProgramming)
            .expect("dynamic programming should be detected");
        // has_table + nested loops ⇒ 0.7 confidence.
        assert!(dp.confidence >= 0.7 - 1e-9);
    }

    #[test]
    fn test_detect_respects_min_confidence() {
        let (cpg, func) = build_bubble_sort_cpg();

        // Bubble sort scores 0.7; a 0.75 floor filters it out entirely.
        let strict = DefaultAlgorithmDetector::new().with_min_confidence(0.75);
        assert!(strict.detect(&cpg, func).is_empty());

        // The default 0.5 floor keeps it.
        let lenient = DefaultAlgorithmDetector::new();
        assert!(!lenient.detect(&cpg, func).is_empty());
    }

    #[test]
    fn test_detect_empty_function_yields_nothing() {
        // A function with no loops, recursion, or heuristic tokens.
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = make_function(&mut cpg, "noop");
        wf_child(&mut cpg, func, block());

        let results = DefaultAlgorithmDetector::new().detect(&cpg, func);
        assert!(results.is_empty());
    }

    #[test]
    fn test_supported_families_membership() {
        let det = DefaultAlgorithmDetector::new();
        let fams = det.supported_families();
        assert!(fams.contains(&AlgorithmFamily::Sorting));
        assert!(fams.contains(&AlgorithmFamily::Searching));
        assert!(fams.contains(&AlgorithmFamily::GraphTraversal));
        assert!(fams.contains(&AlgorithmFamily::DynamicProgramming));
        assert!(fams.contains(&AlgorithmFamily::DivideAndConquer));
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

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(64))]

        /// `detect` never panics on a well-formed function and every result (a)
        /// meets the confidence floor, (b) names a supported family, (c) points
        /// back at the analysed function; and results are in descending
        /// confidence order.
        #[test]
        fn prop_detect_results_bounded_and_sorted(g in arb_well_formed_cpg()) {
            let det = DefaultAlgorithmDetector::new();
            let root = function_root(&g);
            let results = det.detect(&g, root);

            for r in &results {
                prop_assert!(r.confidence >= det.min_confidence);
                prop_assert!(det.supported_families().contains(&r.family));
                prop_assert_eq!(r.function, root);
            }
            for w in results.windows(2) {
                prop_assert!(w[0].confidence >= w[1].confidence);
            }
        }
    }
}

/// End-to-end detection scenarios.
///
/// [`DefaultAlgorithmDetector::detect`] is a three-phase pipeline — control-flow
/// analysis, complexity estimation, then per-family recognition — and each
/// family detector is a *conjunction of syntactic witnesses* (a comparison, a
/// swap, a visited set, a memo table, …). These tests build the minimal CPG
/// that carries each witness set and assert the family, name, and confidence
/// the pipeline is specified to report, including the negative cases where a
/// missing witness must suppress a detection. Together they exercise every
/// family detector, the complexity ladder, and the confidence threshold.
#[cfg(test)]
mod scenarios {
    use super::*;
    use crate::testutil::wf_child;
    use crate::{
        CpgNode, Language, LiteralKind, MethodSignature, ScopeId, SourceRange, Visibility,
    };

    /// A function scaffold: `Function <name> → Block`.
    struct Scaffold {
        cpg: CodePropertyGraph,
        func: NodeId,
        body: NodeId,
    }

    fn scaffold(name: &str) -> Scaffold {
        let mut cpg = CodePropertyGraph::new(Language::Rust);
        let func = cpg.add_node(CpgNode::new(
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
        ));
        let body = wf_child(&mut cpg, func, block());
        Scaffold { cpg, func, body }
    }

    // ---- node-kind shorthands ----
    fn block() -> CpgNodeKind {
        CpgNodeKind::Block {
            scope: ScopeId::GLOBAL,
        }
    }
    fn ident(name: &str) -> CpgNodeKind {
        CpgNodeKind::Identifier {
            name: name.into(),
            definition: None,
        }
    }
    fn member(name: &str) -> CpgNodeKind {
        CpgNodeKind::MemberAccess {
            member: name.into(),
        }
    }
    fn binop(op: &str) -> CpgNodeKind {
        CpgNodeKind::BinaryOp {
            operator: op.into(),
        }
    }
    fn assign(op: &str) -> CpgNodeKind {
        CpgNodeKind::Assignment {
            operator: op.into(),
        }
    }
    fn int(v: i64) -> CpgNodeKind {
        CpgNodeKind::Literal {
            kind: LiteralKind::Integer(v),
        }
    }
    fn call() -> CpgNodeKind {
        CpgNodeKind::Call {
            target: None,
            is_method: false,
        }
    }

    /// `for … { <body built by `f`> }` under `parent`, returning the loop node.
    fn for_loop(
        cpg: &mut CodePropertyGraph,
        parent: NodeId,
        f: impl FnOnce(&mut CodePropertyGraph, NodeId),
    ) -> NodeId {
        let head = wf_child(cpg, parent, CpgNodeKind::For);
        wf_child(cpg, head, ident("i"));
        let body = wf_child(cpg, head, block());
        f(cpg, body);
        head
    }

    /// A comparison `a < b` under `parent`.
    fn comparison(cpg: &mut CodePropertyGraph, parent: NodeId) -> NodeId {
        let cmp = wf_child(cpg, parent, binop("<"));
        wf_child(cpg, cmp, ident("a"));
        wf_child(cpg, cmp, ident("b"));
        cmp
    }

    /// The detected families, most-confident first.
    fn families(found: &[DetectedAlgorithm]) -> Vec<AlgorithmFamily> {
        found.iter().map(|d| d.family).collect()
    }

    fn find(found: &[DetectedAlgorithm], family: AlgorithmFamily) -> Option<&DetectedAlgorithm> {
        found.iter().find(|d| d.family == family)
    }

    // ============================== sorting ==============================

    /// Nested counted loops + a comparison + three assignments (a swap) + an
    /// `i + 1` index ⇒ O(n²) sorting with adjacent exchanges: Bubble Sort.
    #[test]
    fn bubble_sort_is_recognized_from_adjacent_swaps() {
        let mut s = scaffold("sort");
        let cpg = &mut s.cpg;
        for_loop(cpg, s.body, |cpg, outer_body| {
            for_loop(cpg, outer_body, |cpg, inner| {
                comparison(cpg, inner);
                wf_child(cpg, inner, CpgNodeKind::IndexAccess);
                // temp = a; a = b; b = temp  (three assignments)
                for _ in 0..3 {
                    wf_child(cpg, inner, assign("="));
                }
                // arr[j + 1]
                let plus = wf_child(cpg, inner, binop("+"));
                wf_child(cpg, plus, ident("j"));
                wf_child(cpg, plus, int(1));
            });
        });

        let found = DefaultAlgorithmDetector::new().detect(&s.cpg, s.func);
        let sorting = find(&found, AlgorithmFamily::Sorting).expect("a sorting detection");
        assert_eq!(sorting.name.as_deref(), Some("Bubble Sort"));
        assert!((sorting.confidence - 0.7).abs() < 1e-9);
        // The signature records the loop structure the detector saw.
        let sig = &sorting.signature;
        let loops = sig.loop_structure.as_ref().expect("loop structure");
        assert_eq!(loops.loop_count, 2);
        assert_eq!(loops.max_depth, 2);
        assert!(matches!(
            sig.time_complexity.as_ref().map(|c| &c.class),
            Some(ComplexityClass::Quadratic)
        ));
    }

    /// The same shape without the `+ 1` index is a non-adjacent exchange sort.
    #[test]
    fn selection_sort_is_recognized_from_non_adjacent_swaps() {
        let mut s = scaffold("sort");
        let cpg = &mut s.cpg;
        for_loop(cpg, s.body, |cpg, outer_body| {
            for_loop(cpg, outer_body, |cpg, inner| {
                comparison(cpg, inner);
                // A named swap helper is a witness on its own.
                let c = wf_child(cpg, inner, call());
                wf_child(cpg, c, ident("swap"));
            });
        });

        let found = DefaultAlgorithmDetector::new().detect(&s.cpg, s.func);
        let sorting = find(&found, AlgorithmFamily::Sorting).expect("a sorting detection");
        assert_eq!(sorting.name.as_deref(), Some("Selection Sort"));
        assert!((sorting.confidence - 0.6).abs() < 1e-9);
    }

    /// Nested loops with indexing but no exchange ⇒ Insertion Sort.
    #[test]
    fn insertion_sort_is_recognized_from_indexing_without_swaps() {
        let mut s = scaffold("sort");
        let cpg = &mut s.cpg;
        for_loop(cpg, s.body, |cpg, outer_body| {
            for_loop(cpg, outer_body, |cpg, inner| {
                comparison(cpg, inner);
                wf_child(cpg, inner, CpgNodeKind::IndexAccess);
            });
        });

        let found = DefaultAlgorithmDetector::new().detect(&s.cpg, s.func);
        let sorting = find(&found, AlgorithmFamily::Sorting).expect("a sorting detection");
        assert_eq!(sorting.name.as_deref(), Some("Insertion Sort"));
    }

    /// Quadratic with a comparison but neither swaps nor indexing is reported
    /// as an unnamed sorting candidate at the floor confidence.
    #[test]
    fn quadratic_without_witnesses_is_an_unnamed_candidate() {
        let mut s = scaffold("sort");
        let cpg = &mut s.cpg;
        for_loop(cpg, s.body, |cpg, outer_body| {
            for_loop(cpg, outer_body, |cpg, inner| {
                comparison(cpg, inner);
            });
        });

        let found = DefaultAlgorithmDetector::new().detect(&s.cpg, s.func);
        let sorting = find(&found, AlgorithmFamily::Sorting).expect("a sorting detection");
        assert_eq!(sorting.name, None, "no sub-family can be named");
        assert!((sorting.confidence - 0.5).abs() < 1e-9);
    }

    /// Without any comparison there is no sorting evidence at all.
    #[test]
    fn no_comparison_means_no_sorting_detection() {
        let mut s = scaffold("sort");
        let cpg = &mut s.cpg;
        for_loop(cpg, s.body, |cpg, outer_body| {
            for_loop(cpg, outer_body, |cpg, inner| {
                wf_child(cpg, inner, CpgNodeKind::IndexAccess);
            });
        });

        let found = DefaultAlgorithmDetector::new().detect(&s.cpg, s.func);
        assert!(
            find(&found, AlgorithmFamily::Sorting).is_none(),
            "a comparison is a necessary witness"
        );
    }

    // ============================= searching =============================

    /// One counted loop + a comparison + a `return` *inside* the loop ⇒ the
    /// early-exit scan of a linear search.
    #[test]
    fn linear_search_is_recognized_from_an_early_return_in_a_loop() {
        let mut s = scaffold("find");
        let cpg = &mut s.cpg;
        for_loop(cpg, s.body, |cpg, body| {
            let guard = wf_child(cpg, body, CpgNodeKind::If);
            comparison(cpg, guard);
            let then = wf_child(cpg, guard, block());
            wf_child(cpg, then, CpgNodeKind::Return);
        });

        let found = DefaultAlgorithmDetector::new().detect(&s.cpg, s.func);
        let search = find(&found, AlgorithmFamily::Searching).expect("a searching detection");
        assert_eq!(search.name.as_deref(), Some("Linear Search"));
        assert!((search.confidence - 0.7).abs() < 1e-9);
    }

    /// A linear scan whose `return` is outside the loop is only a weak,
    /// unnamed candidate.
    #[test]
    fn linear_scan_without_early_exit_is_unnamed() {
        let mut s = scaffold("find");
        let cpg = &mut s.cpg;
        for_loop(cpg, s.body, |cpg, body| {
            comparison(cpg, body);
        });
        wf_child(cpg, s.body, CpgNodeKind::Return);

        let found = DefaultAlgorithmDetector::new().detect(&s.cpg, s.func);
        let search = find(&found, AlgorithmFamily::Searching).expect("a searching detection");
        assert_eq!(search.name, None);
        assert!((search.confidence - 0.5).abs() < 1e-9);
    }

    /// A single self-call plus a halving witness (`mid`) is the divide-and-
    /// conquer shape: O(log n), reported both as Binary Search and as a
    /// divide-and-conquer algorithm.
    #[test]
    fn binary_search_is_recognized_from_halving_recursion() {
        let mut s = scaffold("bsearch");
        let cpg = &mut s.cpg;
        // Guarded base case.
        let guard = wf_child(cpg, s.body, CpgNodeKind::If);
        comparison(cpg, guard);
        let then = wf_child(cpg, guard, block());
        wf_child(cpg, then, CpgNodeKind::Return);
        // mid = (lo + hi) / 2
        let div = wf_child(cpg, s.body, binop("/"));
        wf_child(cpg, div, ident("mid"));
        wf_child(cpg, div, int(2));
        // Self-call.
        let c = wf_child(cpg, s.body, call());
        wf_child(cpg, c, ident("bsearch"));

        let found = DefaultAlgorithmDetector::new().detect(&s.cpg, s.func);
        let search = find(&found, AlgorithmFamily::Searching).expect("a searching detection");
        assert_eq!(search.name.as_deref(), Some("Binary Search"));
        assert!((search.confidence - 0.8).abs() < 1e-9);

        let dc = find(&found, AlgorithmFamily::DivideAndConquer).expect("a D&C detection");
        assert!(
            (dc.confidence - 0.6).abs() < 1e-9,
            "one recursive call ⇒ 0.6"
        );

        // Results are ordered by descending confidence.
        for w in found.windows(2) {
            assert!(w[0].confidence >= w[1].confidence);
        }
        assert_eq!(families(&found)[0], AlgorithmFamily::Searching);
    }

    // ======================== recursion complexity ========================

    /// Two halving self-calls ⇒ O(n log n), the merge-sort shape.
    #[test]
    fn two_halving_calls_are_linearithmic() {
        let mut s = scaffold("msort");
        let cpg = &mut s.cpg;
        let div = wf_child(cpg, s.body, binop("/"));
        wf_child(cpg, div, ident("lo"));
        wf_child(cpg, div, int(2));
        for _ in 0..2 {
            let c = wf_child(cpg, s.body, call());
            wf_child(cpg, c, ident("msort"));
        }

        let est = ComplexityAnalyzer::new().estimate_time_complexity(&s.cpg, s.func);
        assert!(
            matches!(est.class, ComplexityClass::Linearithmic),
            "got {est:?}"
        );

        let found = DefaultAlgorithmDetector::new().detect(&s.cpg, s.func);
        let dc = find(&found, AlgorithmFamily::DivideAndConquer).expect("a D&C detection");
        assert!(
            (dc.confidence - 0.7).abs() < 1e-9,
            "two recursive calls ⇒ 0.7"
        );
    }

    /// Three or more halving self-calls fall back to a polynomial estimate,
    /// and the D&C detector reports its floor confidence.
    #[test]
    fn many_halving_calls_are_polynomial() {
        let mut s = scaffold("multi");
        let cpg = &mut s.cpg;
        wf_child(cpg, s.body, ident("pivot")); // halving witness
        for _ in 0..3 {
            let c = wf_child(cpg, s.body, call());
            wf_child(cpg, c, ident("multi"));
        }

        let est = ComplexityAnalyzer::new().estimate_time_complexity(&s.cpg, s.func);
        assert!(
            matches!(est.class, ComplexityClass::Polynomial(3)),
            "got {est:?}"
        );
    }

    /// Binary recursion *without* halving is the naive-Fibonacci shape: O(2ⁿ).
    #[test]
    fn binary_recursion_without_halving_is_exponential() {
        let mut s = scaffold("fib");
        let cpg = &mut s.cpg;
        for _ in 0..2 {
            let c = wf_child(cpg, s.body, call());
            wf_child(cpg, c, ident("fib"));
        }

        let est = ComplexityAnalyzer::new().estimate_time_complexity(&s.cpg, s.func);
        assert!(
            matches!(est.class, ComplexityClass::Exponential),
            "got {est:?}"
        );
        assert!((est.confidence - 0.7).abs() < 1e-9);
    }

    /// More than two non-halving self-calls stay exponential, with lower
    /// confidence.
    #[test]
    fn many_non_halving_calls_are_exponential_with_less_confidence() {
        let mut s = scaffold("branchy");
        let cpg = &mut s.cpg;
        for _ in 0..3 {
            let c = wf_child(cpg, s.body, call());
            wf_child(cpg, c, ident("branchy"));
        }

        let est = ComplexityAnalyzer::new().estimate_time_complexity(&s.cpg, s.func);
        assert!(matches!(est.class, ComplexityClass::Exponential));
        assert!((est.confidence - 0.6).abs() < 1e-9);
    }

    /// A single self-call with a guarded base case is linear recursion; the
    /// same call *without* a base case is reported far less confidently.
    #[test]
    fn linear_recursion_confidence_depends_on_the_base_case() {
        let with_base = {
            let mut s = scaffold("walk");
            let cpg = &mut s.cpg;
            let guard = wf_child(cpg, s.body, CpgNodeKind::If);
            let then = wf_child(cpg, guard, block());
            wf_child(cpg, then, CpgNodeKind::Return);
            let c = wf_child(cpg, s.body, call());
            wf_child(cpg, c, ident("walk"));
            ComplexityAnalyzer::new().estimate_time_complexity(&s.cpg, s.func)
        };
        assert!(matches!(with_base.class, ComplexityClass::Linear));
        assert!((with_base.confidence - 0.7).abs() < 1e-9);

        let without_base = {
            let mut s = scaffold("walk");
            let cpg = &mut s.cpg;
            let c = wf_child(cpg, s.body, call());
            wf_child(cpg, c, ident("walk"));
            ComplexityAnalyzer::new().estimate_time_complexity(&s.cpg, s.func)
        };
        assert!(matches!(without_base.class, ComplexityClass::Linear));
        assert!(
            (without_base.confidence - 0.4).abs() < 1e-9,
            "recursion with no base case is a weak signal"
        );
    }

    /// A self-call in tail position is as cheap as iteration in both time and
    /// space, and is reached through a method call (`self.walk()`).
    #[test]
    fn tail_recursion_is_linear_time_and_constant_space() {
        let mut s = scaffold("walk");
        let cpg = &mut s.cpg;
        let ret = wf_child(cpg, s.body, CpgNodeKind::Return);
        let c = wf_child(cpg, ret, call());
        wf_child(cpg, c, member("walk")); // method-call spelling

        let analyzer = ComplexityAnalyzer::new();
        let time = analyzer.estimate_time_complexity(&s.cpg, s.func);
        assert!(matches!(time.class, ComplexityClass::Linear));
        assert!((time.confidence - 0.8).abs() < 1e-9);

        let space = analyzer.estimate_space_complexity(&s.cpg, s.func);
        assert!(
            matches!(space.class, ComplexityClass::Constant),
            "tail calls need no stack growth"
        );
    }

    /// A self-call wrapped in arithmetic is *not* in tail position.
    #[test]
    fn a_call_wrapped_in_arithmetic_is_not_a_tail_call() {
        let mut s = scaffold("sum");
        let cpg = &mut s.cpg;
        let ret = wf_child(cpg, s.body, CpgNodeKind::Return);
        let plus = wf_child(cpg, ret, binop("+"));
        wf_child(cpg, plus, ident("n"));
        let c = wf_child(cpg, plus, call());
        wf_child(cpg, c, ident("sum"));

        let space = ComplexityAnalyzer::new().estimate_space_complexity(&s.cpg, s.func);
        assert!(
            matches!(space.class, ComplexityClass::Linear),
            "non-tail recursion grows the stack"
        );
    }

    /// Space complexity from allocation witnesses, with no recursion.
    #[test]
    fn allocation_witnesses_drive_space_complexity() {
        // An allocating call (`Vec::new`).
        let mut s = scaffold("build");
        let cpg = &mut s.cpg;
        let c = wf_child(cpg, s.body, call());
        wf_child(cpg, c, member("alloc_buffer"));
        let est = ComplexityAnalyzer::new().estimate_space_complexity(&s.cpg, s.func);
        assert!(matches!(est.class, ComplexityClass::Linear), "got {est:?}");

        // No witnesses at all ⇒ constant space.
        let plain = scaffold("plain");
        let est = ComplexityAnalyzer::new().estimate_space_complexity(&plain.cpg, plain.func);
        assert!(matches!(est.class, ComplexityClass::Constant));
    }

    // ========================= graph and DP =========================

    /// A visited set plus a queue is breadth-first search; a visited set plus a
    /// stack (or a bare `while`) is depth-first.
    #[test]
    fn graph_traversals_are_told_apart_by_their_frontier() {
        let bfs = {
            let mut s = scaffold("bfs");
            let cpg = &mut s.cpg;
            wf_child(cpg, s.body, ident("visited"));
            wf_child(cpg, s.body, ident("queue"));
            DefaultAlgorithmDetector::new().detect(&s.cpg, s.func)
        };
        let g = find(&bfs, AlgorithmFamily::GraphTraversal).expect("a graph detection");
        assert_eq!(g.name.as_deref(), Some("BFS"));
        assert!((g.confidence - 0.7).abs() < 1e-9);

        let dfs = {
            let mut s = scaffold("dfs");
            let cpg = &mut s.cpg;
            wf_child(cpg, s.body, ident("visited"));
            wf_child(cpg, s.body, member("stack"));
            DefaultAlgorithmDetector::new().detect(&s.cpg, s.func)
        };
        let g = find(&dfs, AlgorithmFamily::GraphTraversal).expect("a graph detection");
        assert_eq!(g.name.as_deref(), Some("DFS"));
        assert!((g.confidence - 0.6).abs() < 1e-9);

        // A `while`-driven frontier is also DFS-shaped.
        let while_dfs = {
            let mut s = scaffold("dfs2");
            let cpg = &mut s.cpg;
            wf_child(cpg, s.body, ident("visited_set"));
            let w = wf_child(cpg, s.body, CpgNodeKind::While);
            wf_child(cpg, w, ident("more"));
            wf_child(cpg, w, block());
            DefaultAlgorithmDetector::new().detect(&s.cpg, s.func)
        };
        assert_eq!(
            find(&while_dfs, AlgorithmFamily::GraphTraversal)
                .expect("a graph detection")
                .name
                .as_deref(),
            Some("DFS")
        );

        // Without a visited set there is no traversal evidence …
        let no_visited = {
            let mut s = scaffold("plain");
            let cpg = &mut s.cpg;
            wf_child(cpg, s.body, ident("queue"));
            DefaultAlgorithmDetector::new().detect(&s.cpg, s.func)
        };
        assert!(find(&no_visited, AlgorithmFamily::GraphTraversal).is_none());

        // … and a visited set with no frontier at all is not enough either.
        let no_frontier = {
            let mut s = scaffold("plain");
            let cpg = &mut s.cpg;
            wf_child(cpg, s.body, ident("visited"));
            DefaultAlgorithmDetector::new().detect(&s.cpg, s.func)
        };
        assert!(find(&no_frontier, AlgorithmFamily::GraphTraversal).is_none());
    }

    /// A memo table with nested loops is a tabulating DP; the same table
    /// without nesting is a weaker signal.
    #[test]
    fn dynamic_programming_confidence_tracks_the_loop_nesting() {
        let tabulated = {
            let mut s = scaffold("dp");
            let cpg = &mut s.cpg;
            wf_child(cpg, s.body, ident("memo"));
            for_loop(cpg, s.body, |cpg, outer| {
                for_loop(cpg, outer, |_, _| {});
            });
            DefaultAlgorithmDetector::new().detect(&s.cpg, s.func)
        };
        let dp = find(&tabulated, AlgorithmFamily::DynamicProgramming).expect("a DP detection");
        assert!((dp.confidence - 0.7).abs() < 1e-9);

        let flat = {
            let mut s = scaffold("dp");
            let cpg = &mut s.cpg;
            wf_child(cpg, s.body, ident("memo"));
            DefaultAlgorithmDetector::new().detect(&s.cpg, s.func)
        };
        let dp = find(&flat, AlgorithmFamily::DynamicProgramming).expect("a DP detection");
        assert!((dp.confidence - 0.5).abs() < 1e-9);

        // No table ⇒ no DP.
        let plain = scaffold("plain");
        let found = DefaultAlgorithmDetector::new().detect(&plain.cpg, plain.func);
        assert!(find(&found, AlgorithmFamily::DynamicProgramming).is_none());
    }

    // ========================== threshold & guards ==========================

    /// The confidence threshold suppresses every detection below it, and every
    /// reported family is one the detector advertises.
    #[test]
    fn min_confidence_filters_and_families_are_advertised() {
        let mut s = scaffold("dp");
        let cpg = &mut s.cpg;
        wf_child(cpg, s.body, ident("memo"));

        let permissive = DefaultAlgorithmDetector::new();
        let found = permissive.detect(&s.cpg, s.func);
        assert!(!found.is_empty());
        for d in &found {
            assert!(
                permissive.supported_families().contains(&d.family),
                "{:?} must be an advertised family",
                d.family
            );
            assert!(d.confidence >= 0.5);
            assert!(d.confidence <= 1.0);
            assert_eq!(d.function, s.func);
        }

        let strict = DefaultAlgorithmDetector::new().with_min_confidence(0.99);
        assert!(
            strict.detect(&s.cpg, s.func).is_empty(),
            "nothing clears a 0.99 threshold"
        );
    }

    // ===================== heuristic vocabulary sweeps =====================
    //
    // Several detectors recognize a family by a *vocabulary*: a queue-like
    // frontier is any identifier containing `queue`, `push_back`, `deque`, …
    // Each alternative is a separate rule, so each is pinned separately — a
    // typo'd or dropped keyword silently disables recognition of every program
    // that spells the concept that way.

    /// Builds a function whose body contains a single identifier `name`.
    fn function_naming(name: &str) -> Scaffold {
        let mut s = scaffold("f");
        let cpg = &mut s.cpg;
        wf_child(cpg, s.body, ident(name));
        s
    }

    /// Every queue vocabulary word marks a breadth-first frontier.
    #[test]
    fn every_queue_word_is_recognized() {
        for word in ["queue", "push_back", "pop_front", "deque", "enqueue"] {
            let mut s = scaffold("bfs");
            let cpg = &mut s.cpg;
            wf_child(cpg, s.body, ident("visited"));
            wf_child(cpg, s.body, ident(word));
            let found = DefaultAlgorithmDetector::new().detect(&s.cpg, s.func);
            assert_eq!(
                find(&found, AlgorithmFamily::GraphTraversal)
                    .unwrap_or_else(|| panic!("`{word}` must mark a queue"))
                    .name
                    .as_deref(),
                Some("BFS"),
                "`{word}` is a queue word"
            );
        }
    }

    /// Every stack vocabulary word marks a depth-first frontier.
    #[test]
    fn every_stack_word_is_recognized() {
        for word in ["stack", "push", "pop", "my_stack"] {
            let mut s = scaffold("dfs");
            let cpg = &mut s.cpg;
            wf_child(cpg, s.body, ident("visited"));
            wf_child(cpg, s.body, member(word));
            let found = DefaultAlgorithmDetector::new().detect(&s.cpg, s.func);
            assert_eq!(
                find(&found, AlgorithmFamily::GraphTraversal)
                    .unwrap_or_else(|| panic!("`{word}` must mark a stack"))
                    .name
                    .as_deref(),
                Some("DFS"),
                "`{word}` is a stack word"
            );
        }
    }

    /// Every visited-set vocabulary word marks traversal bookkeeping.
    #[test]
    fn every_visited_word_is_recognized() {
        for word in ["visited", "seen", "was_visited", "already_seen"] {
            let mut s = scaffold("bfs");
            let cpg = &mut s.cpg;
            wf_child(cpg, s.body, ident(word));
            wf_child(cpg, s.body, ident("queue"));
            let found = DefaultAlgorithmDetector::new().detect(&s.cpg, s.func);
            assert!(
                find(&found, AlgorithmFamily::GraphTraversal).is_some(),
                "`{word}` is a visited-set word"
            );
        }
        // A word from neither vocabulary is not a witness.
        let s = function_naming("frobnicate");
        let found = DefaultAlgorithmDetector::new().detect(&s.cpg, s.func);
        assert!(find(&found, AlgorithmFamily::GraphTraversal).is_none());
    }

    /// Every memoization vocabulary word marks a DP table.
    #[test]
    fn every_memo_table_word_is_recognized() {
        for word in ["memo", "dp", "cache", "table", "f", "dp_table", "memoized"] {
            let s = function_naming(word);
            let found = DefaultAlgorithmDetector::new().detect(&s.cpg, s.func);
            assert!(
                find(&found, AlgorithmFamily::DynamicProgramming).is_some(),
                "`{word}` is a memo-table word"
            );
        }
        let s = function_naming("frobnicate");
        let found = DefaultAlgorithmDetector::new().detect(&s.cpg, s.func);
        assert!(find(&found, AlgorithmFamily::DynamicProgramming).is_none());
    }

    /// A swap is witnessed either by three assignments *or* by a named helper —
    /// two independent rules, each sufficient on its own.
    #[test]
    fn a_swap_is_witnessed_by_either_rule() {
        // Rule 1: three assignments (temp = a; a = b; b = temp).
        let by_assignments = {
            let mut s = scaffold("sort");
            let cpg = &mut s.cpg;
            for_loop(cpg, s.body, |cpg, outer| {
                for_loop(cpg, outer, |cpg, inner| {
                    comparison(cpg, inner);
                    for _ in 0..3 {
                        wf_child(cpg, inner, assign("="));
                    }
                });
            });
            DefaultAlgorithmDetector::new().detect(&s.cpg, s.func)
        };
        assert_eq!(
            find(&by_assignments, AlgorithmFamily::Sorting)
                .expect("a sorting detection")
                .name
                .as_deref(),
            Some("Selection Sort"),
            "three assignments are a swap"
        );

        // Two assignments are not enough, and with indexing the shape reads as
        // an insertion sort instead.
        let too_few = {
            let mut s = scaffold("sort");
            let cpg = &mut s.cpg;
            for_loop(cpg, s.body, |cpg, outer| {
                for_loop(cpg, outer, |cpg, inner| {
                    comparison(cpg, inner);
                    wf_child(cpg, inner, CpgNodeKind::IndexAccess);
                    for _ in 0..2 {
                        wf_child(cpg, inner, assign("="));
                    }
                });
            });
            DefaultAlgorithmDetector::new().detect(&s.cpg, s.func)
        };
        assert_eq!(
            find(&too_few, AlgorithmFamily::Sorting)
                .expect("a sorting detection")
                .name
                .as_deref(),
            Some("Insertion Sort"),
            "two assignments are not a swap"
        );

        // Rule 2: a named helper, in either spelling.
        for word in ["swap", "swap_unchecked", "SWAP"] {
            let mut s = scaffold("sort");
            let cpg = &mut s.cpg;
            for_loop(cpg, s.body, |cpg, outer| {
                for_loop(cpg, outer, |cpg, inner| {
                    comparison(cpg, inner);
                    let c = wf_child(cpg, inner, call());
                    wf_child(cpg, c, member(word));
                });
            });
            let found = DefaultAlgorithmDetector::new().detect(&s.cpg, s.func);
            assert!(
                find(&found, AlgorithmFamily::Sorting)
                    .expect("a sorting detection")
                    .name
                    .is_some(),
                "`{word}` names a swap"
            );
        }
    }

    /// A midpoint is witnessed by a halving operator (`/` or `>>`) over 1 or 2,
    /// or by the conventional variable names.
    #[test]
    fn every_midpoint_witness_is_recognized() {
        // Operator forms. Note the two halving predicates accept *different*
        // literals: `ComplexityAnalyzer::is_divide_and_conquer` — which is what
        // turns recursion into an O(log n) estimate — requires a literal `2`,
        // while `has_midpoint_calculation` (used only when there is no
        // recursion to short-circuit on) also accepts `1`, for the `>> 1`
        // spelling of a halving. Only the literal `2` forms therefore reach the
        // Binary Search naming through a recursive function.
        for (op, literal) in [("/", 2i64), (">>", 2)] {
            let mut s = scaffold("bsearch");
            let cpg = &mut s.cpg;
            let guard = wf_child(cpg, s.body, CpgNodeKind::If);
            comparison(cpg, guard);
            let then = wf_child(cpg, guard, block());
            wf_child(cpg, then, CpgNodeKind::Return);
            let div = wf_child(cpg, s.body, binop(op));
            wf_child(cpg, div, ident("lo"));
            wf_child(cpg, div, int(literal));
            let c = wf_child(cpg, s.body, call());
            wf_child(cpg, c, ident("bsearch"));

            let found = DefaultAlgorithmDetector::new().detect(&s.cpg, s.func);
            assert_eq!(
                find(&found, AlgorithmFamily::Searching)
                    .unwrap_or_else(|| panic!("`x {op} {literal}` must halve"))
                    .name
                    .as_deref(),
                Some("Binary Search"),
                "`x {op} {literal}` is a halving witness"
            );
        }

        // Name forms, including the ones only `is_divide_and_conquer` knows.
        for word in ["mid", "middle", "pivot", "half_len", "MID"] {
            let mut s = scaffold("dc");
            let cpg = &mut s.cpg;
            wf_child(cpg, s.body, ident(word));
            let c = wf_child(cpg, s.body, call());
            wf_child(cpg, c, ident("dc"));

            let est = ComplexityAnalyzer::new().estimate_time_complexity(&s.cpg, s.func);
            assert!(
                matches!(est.class, ComplexityClass::Logarithmic),
                "`{word}` is a halving witness, got {est:?}"
            );
        }

        // Dividing by anything other than 2 — including by 1, and including a
        // non-halving divisor — leaves the recursion linear.
        for (op, literal) in [("/", 7i64), ("/", 1), (">>", 1), ("*", 2)] {
            let mut s = scaffold("dc");
            let cpg = &mut s.cpg;
            let div = wf_child(cpg, s.body, binop(op));
            wf_child(cpg, div, ident("n"));
            wf_child(cpg, div, int(literal));
            let c = wf_child(cpg, s.body, call());
            wf_child(cpg, c, ident("dc"));
            let est = ComplexityAnalyzer::new().estimate_time_complexity(&s.cpg, s.func);
            assert!(
                matches!(est.class, ComplexityClass::Linear),
                "`n {op} {literal}` is not a halving, got {est:?}"
            );
        }
    }

    /// Every allocation vocabulary word drives the space estimate to linear;
    /// an array literal does so on its own.
    #[test]
    fn every_allocation_witness_is_recognized() {
        for word in [
            "vec",
            "new",
            "alloc",
            "create",
            "clone",
            "collect",
            "vector_new",
        ] {
            let mut s = scaffold("build");
            let cpg = &mut s.cpg;
            let c = wf_child(cpg, s.body, call());
            wf_child(cpg, c, ident(word));
            let est = ComplexityAnalyzer::new().estimate_space_complexity(&s.cpg, s.func);
            assert!(
                matches!(est.class, ComplexityClass::Linear),
                "`{word}` allocates, got {est:?}"
            );
        }

        // An array literal is an allocation by itself.
        let mut s = scaffold("build");
        let cpg = &mut s.cpg;
        wf_child(
            cpg,
            s.body,
            CpgNodeKind::Literal {
                kind: LiteralKind::Array,
            },
        );
        let est = ComplexityAnalyzer::new().estimate_space_complexity(&s.cpg, s.func);
        assert!(matches!(est.class, ComplexityClass::Linear));

        // A call to something outside the vocabulary does not — `with_capacity`
        // is a deliberate example: it allocates in Rust, but contains none of
        // the recognized substrings, so the heuristic (correctly, per its own
        // definition) does not fire.
        for word in ["log", "with_capacity"] {
            let mut s = scaffold("noop");
            let cpg = &mut s.cpg;
            let c = wf_child(cpg, s.body, call());
            wf_child(cpg, c, ident(word));
            let est = ComplexityAnalyzer::new().estimate_space_complexity(&s.cpg, s.func);
            assert!(
                matches!(est.class, ComplexityClass::Constant),
                "`{word}`: got {est:?}"
            );
        }
    }

    /// The threshold is applied to *every* family, not just the one that
    /// happens to be checked first.
    #[test]
    fn the_threshold_suppresses_each_family_independently() {
        // A graph that triggers sorting (0.7), searching (0.7), graph (0.7),
        // DP (0.7) and D&C (0.7) at once.
        let mut s = scaffold("everything");
        let cpg = &mut s.cpg;
        wf_child(cpg, s.body, ident("visited"));
        wf_child(cpg, s.body, ident("queue"));
        wf_child(cpg, s.body, ident("memo"));
        for_loop(cpg, s.body, |cpg, outer| {
            for_loop(cpg, outer, |cpg, inner| {
                comparison(cpg, inner);
                wf_child(cpg, inner, CpgNodeKind::IndexAccess);
                let guard = wf_child(cpg, inner, CpgNodeKind::If);
                let then = wf_child(cpg, guard, block());
                wf_child(cpg, then, CpgNodeKind::Return);
            });
        });

        let permissive = DefaultAlgorithmDetector::new().detect(&s.cpg, s.func);
        assert!(
            permissive.len() >= 3,
            "several families fire at once: {:?}",
            families(&permissive)
        );

        // Just above every reported confidence ⇒ nothing survives.
        let ceiling = permissive
            .iter()
            .map(|d| d.confidence)
            .fold(0.0f64, f64::max);
        let strict = DefaultAlgorithmDetector::new().with_min_confidence(ceiling + 0.01);
        assert!(strict.detect(&s.cpg, s.func).is_empty());
    }

    /// Bounded and unbounded loops of the same depth share a complexity class
    /// but not a confidence: an unbounded loop is only a lower bound.
    #[test]
    fn boundedness_changes_confidence_not_class() {
        // A `while` with no counter is unbounded; a `for` is counted.
        let unbounded_single = {
            let mut s = scaffold("f");
            let cpg = &mut s.cpg;
            let w = wf_child(cpg, s.body, CpgNodeKind::While);
            wf_child(cpg, w, ident("c"));
            wf_child(cpg, w, block());
            ComplexityAnalyzer::new().estimate_time_complexity(&s.cpg, s.func)
        };
        assert!(matches!(unbounded_single.class, ComplexityClass::Linear));
        assert!(
            (unbounded_single.confidence - 0.6).abs() < 1e-9,
            "unbounded ⇒ 0.6"
        );

        let counted_single = {
            let mut s = scaffold("f");
            let cpg = &mut s.cpg;
            for_loop(cpg, s.body, |_, _| {});
            ComplexityAnalyzer::new().estimate_time_complexity(&s.cpg, s.func)
        };
        assert!(matches!(counted_single.class, ComplexityClass::Linear));
        assert!(
            (counted_single.confidence - 0.8).abs() < 1e-9,
            "counted ⇒ 0.8"
        );

        // Same at depth 2.
        let unbounded_nested = {
            let mut s = scaffold("f");
            let cpg = &mut s.cpg;
            let outer = wf_child(cpg, s.body, CpgNodeKind::While);
            wf_child(cpg, outer, ident("c"));
            let ob = wf_child(cpg, outer, block());
            let inner = wf_child(cpg, ob, CpgNodeKind::While);
            wf_child(cpg, inner, ident("d"));
            wf_child(cpg, inner, block());
            ComplexityAnalyzer::new().estimate_time_complexity(&s.cpg, s.func)
        };
        assert!(matches!(unbounded_nested.class, ComplexityClass::Quadratic));
        assert!((unbounded_nested.confidence - 0.5).abs() < 1e-9);

        let counted_nested = {
            let mut s = scaffold("f");
            let cpg = &mut s.cpg;
            for_loop(cpg, s.body, |cpg, outer| {
                for_loop(cpg, outer, |_, _| {});
            });
            ComplexityAnalyzer::new().estimate_time_complexity(&s.cpg, s.func)
        };
        assert!(matches!(counted_nested.class, ComplexityClass::Quadratic));
        assert!((counted_nested.confidence - 0.8).abs() < 1e-9);

        // Depth 3 and beyond.
        let cubic = {
            let mut s = scaffold("f");
            let cpg = &mut s.cpg;
            for_loop(cpg, s.body, |cpg, a| {
                for_loop(cpg, a, |cpg, b| {
                    for_loop(cpg, b, |_, _| {});
                });
            });
            ComplexityAnalyzer::new().estimate_time_complexity(&s.cpg, s.func)
        };
        assert!(
            matches!(cubic.class, ComplexityClass::Cubic),
            "got {cubic:?}"
        );

        let quartic = {
            let mut s = scaffold("f");
            let cpg = &mut s.cpg;
            for_loop(cpg, s.body, |cpg, a| {
                for_loop(cpg, a, |cpg, b| {
                    for_loop(cpg, b, |cpg, c| {
                        for_loop(cpg, c, |_, _| {});
                    });
                });
            });
            ComplexityAnalyzer::new().estimate_time_complexity(&s.cpg, s.func)
        };
        assert!(
            matches!(quartic.class, ComplexityClass::Polynomial(4)),
            "got {quartic:?}"
        );
    }

    /// When a function both loops *and* recurses, the **worse** of the two
    /// estimates is reported — in either direction.
    #[test]
    fn the_dominant_of_loops_and_recursion_wins() {
        // Recursion worse than loops: one counted loop (O(n)) plus binary
        // recursion (O(2ⁿ)) ⇒ exponential.
        let recursion_dominates = {
            let mut s = scaffold("fib");
            let cpg = &mut s.cpg;
            for_loop(cpg, s.body, |_, _| {});
            for _ in 0..2 {
                let c = wf_child(cpg, s.body, call());
                wf_child(cpg, c, ident("fib"));
            }
            ComplexityAnalyzer::new().estimate_time_complexity(&s.cpg, s.func)
        };
        assert!(
            matches!(recursion_dominates.class, ComplexityClass::Exponential),
            "got {recursion_dominates:?}"
        );

        // Loops worse than recursion: nested loops (O(n²)) plus a tail call
        // (O(n)) ⇒ quadratic.
        let loops_dominate = {
            let mut s = scaffold("walk");
            let cpg = &mut s.cpg;
            for_loop(cpg, s.body, |cpg, outer| {
                for_loop(cpg, outer, |_, _| {});
            });
            let ret = wf_child(cpg, s.body, CpgNodeKind::Return);
            let c = wf_child(cpg, ret, call());
            wf_child(cpg, c, ident("walk"));
            ComplexityAnalyzer::new().estimate_time_complexity(&s.cpg, s.func)
        };
        assert!(
            matches!(loops_dominate.class, ComplexityClass::Quadratic),
            "got {loops_dominate:?}"
        );
    }

    /// The attached signature mirrors the loop structure the analyzer saw:
    /// one `LoopType` per loop, each carrying its own kind, depth, and
    /// boundedness. Every loop syntax the detector can report is covered.
    #[test]
    fn the_signature_mirrors_the_loop_structure() {
        let mut s = scaffold("sort");
        let cpg = &mut s.cpg;
        comparison(cpg, s.body);

        // A counted `for` nesting an unbounded `while`, plus a sibling
        // `loop` and an opaque `do…while`, so all four kinds appear.
        for_loop(cpg, s.body, |cpg, outer| {
            let w = wf_child(cpg, outer, CpgNodeKind::While);
            wf_child(cpg, w, ident("c"));
            wf_child(cpg, w, block());
        });
        wf_child(cpg, s.body, CpgNodeKind::Loop);
        wf_child(
            cpg,
            s.body,
            CpgNodeKind::Unknown {
                kind: "do_statement".into(),
            },
        );

        let found = DefaultAlgorithmDetector::new().detect(&s.cpg, s.func);
        let sorting = find(&found, AlgorithmFamily::Sorting).expect("a sorting detection");
        let loops = sorting
            .signature
            .loop_structure
            .as_ref()
            .expect("the loop structure is recorded");

        assert_eq!(loops.loop_count, 4, "one entry per loop");
        assert_eq!(loops.max_depth, 2, "the `while` nests inside the `for`");
        assert_eq!(loops.loop_types.len(), 4);

        // `SigLoopKind`/`LoopBounds` are not `PartialEq`, so match structurally.
        let kinds = &loops.loop_types;
        for (label, hit) in [
            (
                "CountedFor",
                kinds
                    .iter()
                    .any(|l| matches!(l.kind, SigLoopKind::CountedFor)),
            ),
            (
                "While",
                kinds.iter().any(|l| matches!(l.kind, SigLoopKind::While)),
            ),
            (
                "Infinite",
                kinds
                    .iter()
                    .any(|l| matches!(l.kind, SigLoopKind::Infinite)),
            ),
            (
                "DoWhile",
                kinds.iter().any(|l| matches!(l.kind, SigLoopKind::DoWhile)),
            ),
        ] {
            assert!(hit, "a {label} loop must appear in the signature");
        }

        // Boundedness is carried across as the loop bounds.
        assert!(
            kinds
                .iter()
                .any(|l| matches!(l.bounds, LoopBounds::LinearN)),
            "the counted `for` is bounded"
        );
        assert!(
            kinds
                .iter()
                .any(|l| matches!(l.bounds, LoopBounds::Unknown)),
            "the bare `loop` is not"
        );

        // The estimate is carried too.
        assert!(sorting.signature.time_complexity.is_some());
    }

    /// A linear scan with more than one loop is not a linear *search*, even
    /// with an early return — the search detector wants a single scan.
    #[test]
    fn several_loops_disqualify_a_linear_search() {
        let mut s = scaffold("find");
        let cpg = &mut s.cpg;
        // Two sibling loops (depth 1 each ⇒ still O(n)), each with an early exit.
        for _ in 0..2 {
            for_loop(cpg, s.body, |cpg, body| {
                let guard = wf_child(cpg, body, CpgNodeKind::If);
                comparison(cpg, guard);
                let then = wf_child(cpg, guard, block());
                wf_child(cpg, then, CpgNodeKind::Return);
            });
        }

        let found = DefaultAlgorithmDetector::new().detect(&s.cpg, s.func);
        let search = find(&found, AlgorithmFamily::Searching).expect("a searching detection");
        assert_eq!(search.name, None, "two scans are not a linear search");
        assert!((search.confidence - 0.5).abs() < 1e-9);
    }

    /// `detect` on a node that is not a function yields nothing: recursion
    /// analysis needs a signature to compare call targets against.
    #[test]
    fn detection_on_a_non_function_node_is_empty() {
        let mut s = scaffold("f");
        let stray = s.cpg.add_node(CpgNode::new(
            NodeId::new(0),
            CpgNodeKind::Return,
            SourceRange::default(),
        ));
        assert!(DefaultAlgorithmDetector::new()
            .detect(&s.cpg, stray)
            .is_empty());
        assert!(ControlFlowAnalyzer::new()
            .detect_recursion(&s.cpg, stray)
            .is_none());
        // A node id that is not in the graph at all is equally inert.
        let absent = NodeId::new(9_999);
        assert!(ControlFlowAnalyzer::new()
            .detect_recursion(&s.cpg, absent)
            .is_none());
    }
}
