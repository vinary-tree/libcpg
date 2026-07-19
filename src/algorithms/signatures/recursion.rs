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
