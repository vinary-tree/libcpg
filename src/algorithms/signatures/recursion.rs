//! Recursion pattern analysis types.

use crate::NodeId;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Describes a recursion pattern.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct RecursionPattern {
    /// Type of recursion.
    pub kind: RecursionKind,
    /// Base case nodes.
    pub base_cases: Vec<NodeId>,
    /// Recursive call sites.
    pub recursive_calls: Vec<NodeId>,
    /// Reduction factor per call (e.g., n/2 for binary recursion).
    pub reduction: ReductionPattern,
    /// Whether tail call optimization is possible.
    pub tail_optimizable: bool,
}

impl RecursionPattern {
    /// Creates a new recursion pattern.
    pub fn new(kind: RecursionKind) -> Self {
        Self {
            kind,
            base_cases: Vec::new(),
            recursive_calls: Vec::new(),
            reduction: ReductionPattern::Unknown,
            tail_optimizable: false,
        }
    }

    /// Adds a base case.
    pub fn with_base_case(mut self, node: NodeId) -> Self {
        self.base_cases.push(node);
        self
    }

    /// Adds a recursive call.
    pub fn with_recursive_call(mut self, node: NodeId) -> Self {
        self.recursive_calls.push(node);
        self
    }

    /// Sets the reduction pattern.
    pub fn with_reduction(mut self, reduction: ReductionPattern) -> Self {
        self.reduction = reduction;
        self
    }

    /// Sets tail optimizability.
    pub fn with_tail_optimizable(mut self, optimizable: bool) -> Self {
        self.tail_optimizable = optimizable;
        self
    }
}

impl Default for RecursionPattern {
    fn default() -> Self {
        Self::new(RecursionKind::Direct)
    }
}

/// Kind of recursion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum RecursionKind {
    /// Direct recursion (function calls itself).
    Direct,
    /// Indirect/mutual recursion (A calls B which calls A).
    Indirect,
    /// Tail recursion (recursive call is last operation).
    Tail,
    /// Binary recursion (two recursive calls, like in mergesort).
    Binary,
    /// Multiple recursion (more than two calls).
    Multiple,
}

/// How the problem size is reduced in each recursive call.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ReductionPattern {
    /// Constant reduction (n - k).
    Constant(usize),
    /// Linear reduction (n - 1).
    Linear,
    /// Division (n / k).
    Division(usize),
    /// Logarithmic reduction.
    Logarithmic,
    /// Unknown reduction pattern.
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeId;

    #[test]
    fn test_recursion_pattern_new_defaults() {
        let rp = RecursionPattern::new(RecursionKind::Direct);
        assert_eq!(rp.kind, RecursionKind::Direct);
        assert!(rp.base_cases.is_empty());
        assert!(rp.recursive_calls.is_empty());
        assert!(matches!(rp.reduction, ReductionPattern::Unknown));
        assert!(!rp.tail_optimizable);
    }

    #[test]
    fn test_recursion_pattern_default_is_direct() {
        let rp = RecursionPattern::default();
        assert_eq!(rp.kind, RecursionKind::Direct);
        assert!(rp.base_cases.is_empty());
        assert!(rp.recursive_calls.is_empty());
        assert!(!rp.tail_optimizable);
    }

    #[test]
    fn test_recursion_pattern_with_base_case() {
        let rp = RecursionPattern::new(RecursionKind::Direct)
            .with_base_case(NodeId::new(1))
            .with_base_case(NodeId::new(2));
        assert_eq!(rp.base_cases, vec![NodeId::new(1), NodeId::new(2)]);
    }

    #[test]
    fn test_recursion_pattern_with_recursive_call() {
        let rp = RecursionPattern::new(RecursionKind::Binary)
            .with_recursive_call(NodeId::new(5))
            .with_recursive_call(NodeId::new(6));
        assert_eq!(rp.kind, RecursionKind::Binary);
        assert_eq!(rp.recursive_calls, vec![NodeId::new(5), NodeId::new(6)]);
    }

    #[test]
    fn test_recursion_pattern_with_reduction() {
        let rp = RecursionPattern::new(RecursionKind::Direct)
            .with_reduction(ReductionPattern::Division(2));
        assert!(matches!(rp.reduction, ReductionPattern::Division(2)));
    }

    #[test]
    fn test_recursion_pattern_with_tail_optimizable() {
        let rp = RecursionPattern::new(RecursionKind::Tail).with_tail_optimizable(true);
        assert_eq!(rp.kind, RecursionKind::Tail);
        assert!(rp.tail_optimizable);
    }

    #[test]
    fn test_recursion_pattern_full_chain() {
        let rp = RecursionPattern::new(RecursionKind::Multiple)
            .with_base_case(NodeId::new(10))
            .with_recursive_call(NodeId::new(11))
            .with_recursive_call(NodeId::new(12))
            .with_recursive_call(NodeId::new(13))
            .with_reduction(ReductionPattern::Linear)
            .with_tail_optimizable(false);
        assert_eq!(rp.kind, RecursionKind::Multiple);
        assert_eq!(rp.base_cases.len(), 1);
        assert_eq!(rp.recursive_calls.len(), 3);
        assert!(matches!(rp.reduction, ReductionPattern::Linear));
        assert!(!rp.tail_optimizable);
    }

    #[test]
    fn test_recursion_kind_variants_distinct() {
        let kinds = [
            RecursionKind::Direct,
            RecursionKind::Indirect,
            RecursionKind::Tail,
            RecursionKind::Binary,
            RecursionKind::Multiple,
        ];
        for (i, a) in kinds.iter().enumerate() {
            for (j, b) in kinds.iter().enumerate() {
                assert_eq!(i == j, a == b);
            }
        }
    }

    #[test]
    fn test_reduction_pattern_variants_construct() {
        // No PartialEq derive → match on each variant.
        assert!(matches!(
            ReductionPattern::Constant(3),
            ReductionPattern::Constant(3)
        ));
        assert!(matches!(ReductionPattern::Linear, ReductionPattern::Linear));
        assert!(matches!(
            ReductionPattern::Division(2),
            ReductionPattern::Division(2)
        ));
        assert!(matches!(
            ReductionPattern::Logarithmic,
            ReductionPattern::Logarithmic
        ));
        assert!(matches!(
            ReductionPattern::Unknown,
            ReductionPattern::Unknown
        ));
    }
}
