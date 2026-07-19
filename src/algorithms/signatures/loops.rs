//! Loop structure analysis types.

use crate::NodeId;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

/// Describes the loop structure of a function.
#[derive(Debug, Clone, Default)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LoopStructure {
    /// Maximum loop nesting depth.
    pub max_depth: usize,
    /// Total number of loops.
    pub loop_count: usize,
    /// Loops by type.
    pub loop_types: Vec<LoopType>,
    /// Whether loops are independent (parallelizable).
    pub loops_independent: bool,
}

impl LoopStructure {
    /// Creates a new loop structure.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the maximum depth.
    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }

    /// Adds a loop type.
    pub fn with_loop(mut self, loop_type: LoopType) -> Self {
        self.loop_count += 1;
        self.loop_types.push(loop_type);
        self
    }

    /// Sets whether loops are independent.
    pub fn with_independence(mut self, independent: bool) -> Self {
        self.loops_independent = independent;
        self
    }
}

/// Type of loop construct.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct LoopType {
    /// Loop header node.
    pub header: NodeId,
    /// Loop kind.
    pub kind: LoopKind,
    /// Nesting depth (0 = outermost).
    pub depth: usize,
    /// Bounds information.
    pub bounds: LoopBounds,
    /// Whether the loop has early exits.
    pub has_early_exit: bool,
}

/// Kind of loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum LoopKind {
    /// For loop with index variable.
    CountedFor,
    /// For-each / iterator loop.
    ForEach,
    /// While loop.
    While,
    /// Do-while loop.
    DoWhile,
    /// Infinite loop (with break).
    Infinite,
}

/// Loop bounds information.
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum LoopBounds {
    /// Constant bounds (known at compile time).
    Constant(usize),
    /// Linear in input size (n).
    LinearN,
    /// Logarithmic in input size (log n).
    LogarithmicN,
    /// Depends on multiple variables.
    Multiple,
    /// Unknown bounds.
    Unknown,
}

impl Default for LoopBounds {
    fn default() -> Self {
        Self::Unknown
    }
}
