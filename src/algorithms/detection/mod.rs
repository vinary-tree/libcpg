//! Algorithm detection from CPGs.
//!
//! Analyzes control flow patterns and data access patterns
//! to identify common algorithm families.

mod control_flow;
mod complexity;

pub use control_flow::{ControlFlowAnalyzer, LoopPattern, LoopKind, RecursionPattern, RecursionKind};
pub use complexity::ComplexityAnalyzer;

use crate::{CodePropertyGraph, CpgNodeKind, NodeId};
use super::signatures::{
    AlgorithmSignature, ComplexityClass, ComplexityEstimate,
    LoopStructure, LoopType, LoopBounds, SigLoopKind,
    RecursionPattern as SigRecursionPattern, ReductionPattern, SigRecursionKind,
};
use super::families::AlgorithmFamily;

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
    fn detect_sorting(&self, cpg: &CodePropertyGraph, function: NodeId, loops: &[LoopPattern], time_complexity: &ComplexityEstimate) -> Option<DetectedAlgorithm> {
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
    fn detect_searching(&self, cpg: &CodePropertyGraph, function: NodeId, loops: &[LoopPattern], recursion: Option<&RecursionPattern>, time_complexity: &ComplexityEstimate) -> Option<DetectedAlgorithm> {
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
    fn detect_graph(&self, cpg: &CodePropertyGraph, function: NodeId, loops: &[LoopPattern]) -> Option<DetectedAlgorithm> {
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

        let mut algo = DetectedAlgorithm::new(AlgorithmFamily::GraphTraversal, function, confidence);
        if let Some(n) = name {
            algo = algo.with_name(n);
        }

        Some(algo)
    }

    /// Detects dynamic programming patterns.
    fn detect_dp(&self, cpg: &CodePropertyGraph, function: NodeId, loops: &[LoopPattern]) -> Option<DetectedAlgorithm> {
        // DP algorithms typically have:
        // - Memoization table (array/hashmap)
        // - Nested loops OR recursion with memoization
        // - State transitions (assignments to table)

        let has_table = self.has_memoization_table(cpg, function);
        let has_nested_loops = loops.iter().any(|l| l.depth >= 2);

        if !has_table {
            return None;
        }

        let confidence = if has_nested_loops {
            0.7
        } else {
            0.5
        };

        if confidence < self.min_confidence {
            return None;
        }

        Some(DetectedAlgorithm::new(AlgorithmFamily::DynamicProgramming, function, confidence))
    }

    /// Detects divide-and-conquer patterns.
    fn detect_divide_conquer(&self, _cpg: &CodePropertyGraph, function: NodeId, recursion: Option<&RecursionPattern>, time_complexity: &ComplexityEstimate) -> Option<DetectedAlgorithm> {
        let rec = recursion?;

        // D&C typically has:
        // - Recursion with input halving
        // - Multiple recursive calls (2 for merge sort, 1-2 for quicksort)
        // - O(n log n) or O(log n) complexity

        let is_dc_complexity = matches!(time_complexity.class,
            ComplexityClass::Logarithmic | ComplexityClass::Linearithmic);

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

        Some(DetectedAlgorithm::new(AlgorithmFamily::DivideAndConquer, function, confidence))
    }

    /// Builds an algorithm signature from detected patterns.
    fn build_signature(&self, loops: &[LoopPattern], recursion: Option<&RecursionPattern>, time_complexity: ComplexityEstimate) -> AlgorithmSignature {
        let mut sig = AlgorithmSignature::new()
            .with_time_complexity(time_complexity);

        if !loops.is_empty() {
            let max_depth = loops.iter().map(|l| l.depth).max().unwrap_or(0);
            let loop_types: Vec<LoopType> = loops.iter().map(|l| {
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
                    bounds: if l.is_counted { LoopBounds::LinearN } else { LoopBounds::Unknown },
                    has_early_exit: false,
                }
            }).collect();

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
                .map(|n| matches!(&n.kind, CpgNodeKind::BinaryOp { operator }
                    if matches!(operator.as_ref(), "<" | ">" | "<=" | ">=" | "==" | "!=")))
                .unwrap_or(false)
        })
    }

    /// Checks for swap patterns (temp = a; a = b; b = temp).
    fn has_swap_pattern(&self, cpg: &CodePropertyGraph, function: NodeId) -> bool {
        let descendants = cpg.ast_descendants(function);

        // Look for multiple assignments that suggest swapping
        let assignment_count = descendants.iter().filter(|&&id| {
            cpg.node(id)
                .map(|n| matches!(n.kind, CpgNodeKind::Assignment { .. }))
                .unwrap_or(false)
        }).count();

        // Also check for std::mem::swap or similar calls
        let has_swap_call = descendants.iter().any(|&id| {
            cpg.node(id).map(|n| {
                if let CpgNodeKind::Identifier { name, .. } | CpgNodeKind::MemberAccess { member: name } = &n.kind {
                    name.to_lowercase().contains("swap")
                } else {
                    false
                }
            }).unwrap_or(false)
        });

        assignment_count >= 3 || has_swap_call
    }

    /// Checks for adjacent element swap pattern (arr[i] <-> arr[i+1]).
    fn has_adjacent_swap_pattern(&self, cpg: &CodePropertyGraph, function: NodeId) -> bool {
        // Look for index expressions like arr[i] and arr[i+1] near swaps
        let descendants = cpg.ast_descendants(function);

        descendants.iter().any(|&id| {
            cpg.node(id).map(|n| {
                if let CpgNodeKind::BinaryOp { operator } = &n.kind {
                    // Look for i+1 pattern
                    let op = operator.as_ref();
                    if op == "+" {
                        let children = cpg.ast_children(id);
                        children.iter().any(|&child_id| {
                            cpg.node(child_id)
                                .map(|c| matches!(&c.kind, CpgNodeKind::Literal { kind: crate::LiteralKind::Integer(1) }))
                                .unwrap_or(false)
                        })
                    } else {
                        false
                    }
                } else {
                    false
                }
            }).unwrap_or(false)
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
            if cpg.node(id).map(|n| matches!(n.kind, CpgNodeKind::Return)).unwrap_or(false) {
                // Check if inside a loop
                let ancestors = cpg.ast_ancestors(id);
                ancestors.iter().any(|&anc| {
                    cpg.node(anc)
                        .map(|n| matches!(n.kind, CpgNodeKind::For | CpgNodeKind::While | CpgNodeKind::Loop))
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
            cpg.node(id).map(|n| {
                if let CpgNodeKind::BinaryOp { operator } = &n.kind {
                    let op = operator.as_ref();
                    if op == "/" || op == ">>" {
                        let children = cpg.ast_children(id);
                        children.iter().any(|&child_id| {
                            cpg.node(child_id).map(|c| {
                                matches!(&c.kind, CpgNodeKind::Literal { kind: crate::LiteralKind::Integer(2) })
                                    || matches!(&c.kind, CpgNodeKind::Literal { kind: crate::LiteralKind::Integer(1) })
                            }).unwrap_or(false)
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
            }).unwrap_or(false)
        })
    }

    /// Checks for queue operations (push_back, pop_front, etc.).
    fn has_queue_operations(&self, cpg: &CodePropertyGraph, function: NodeId) -> bool {
        let descendants = cpg.ast_descendants(function);

        descendants.iter().any(|&id| {
            cpg.node(id).map(|n| {
                if let CpgNodeKind::Identifier { name, .. } | CpgNodeKind::MemberAccess { member: name } = &n.kind {
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
            }).unwrap_or(false)
        })
    }

    /// Checks for stack operations (push, pop on stack-like structure).
    fn has_stack_operations(&self, cpg: &CodePropertyGraph, function: NodeId) -> bool {
        let descendants = cpg.ast_descendants(function);

        descendants.iter().any(|&id| {
            cpg.node(id).map(|n| {
                if let CpgNodeKind::Identifier { name, .. } | CpgNodeKind::MemberAccess { member: name } = &n.kind {
                    let name_lower = name.to_lowercase();
                    name_lower.contains("stack")
                        || (name_lower == "push" || name_lower == "pop")
                } else {
                    false
                }
            }).unwrap_or(false)
        })
    }

    /// Checks for visited/seen tracking patterns.
    fn has_visited_tracking(&self, cpg: &CodePropertyGraph, function: NodeId) -> bool {
        let descendants = cpg.ast_descendants(function);

        descendants.iter().any(|&id| {
            cpg.node(id).map(|n| {
                if let CpgNodeKind::Identifier { name, .. } = &n.kind {
                    let name_lower = name.to_lowercase();
                    name_lower.contains("visited")
                        || name_lower.contains("seen")
                        || name_lower.contains("marked")
                } else {
                    false
                }
            }).unwrap_or(false)
        })
    }

    /// Checks for memoization table patterns.
    fn has_memoization_table(&self, cpg: &CodePropertyGraph, function: NodeId) -> bool {
        let descendants = cpg.ast_descendants(function);

        descendants.iter().any(|&id| {
            cpg.node(id).map(|n| {
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
            }).unwrap_or(false)
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
        if let Some(algo) = self.detect_searching(cpg, function, &loops, recursion.as_ref(), &time_complexity) {
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
        if let Some(algo) = self.detect_divide_conquer(cpg, function, recursion.as_ref(), &time_complexity) {
            detections.push(algo);
        }

        // Sort by confidence (highest first)
        detections.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));

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

    #[test]
    fn test_detector_creation() {
        let detector = DefaultAlgorithmDetector::new()
            .with_min_confidence(0.7);

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
}
