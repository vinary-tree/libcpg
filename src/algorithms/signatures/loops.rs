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
#[derive(Debug, Clone, Default)]
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
    #[default]
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NodeId;

    fn sample_loop(depth: usize) -> LoopType {
        LoopType {
            header: NodeId::new(depth as u32),
            kind: LoopKind::CountedFor,
            depth,
            bounds: LoopBounds::LinearN,
            has_early_exit: false,
        }
    }

    #[test]
    fn test_loop_structure_default_is_empty() {
        let ls = LoopStructure::new();
        assert_eq!(ls.max_depth, 0);
        assert_eq!(ls.loop_count, 0);
        assert!(ls.loop_types.is_empty());
        assert!(!ls.loops_independent);
        // Default derive agrees with new().
        let d = LoopStructure::default();
        assert_eq!(d.max_depth, ls.max_depth);
        assert_eq!(d.loop_count, ls.loop_count);
    }

    #[test]
    fn test_loop_structure_with_max_depth() {
        let ls = LoopStructure::new().with_max_depth(3);
        assert_eq!(ls.max_depth, 3);
    }

    #[test]
    fn test_loop_structure_with_loop_increments_count() {
        let ls = LoopStructure::new()
            .with_loop(sample_loop(0))
            .with_loop(sample_loop(1));
        assert_eq!(ls.loop_count, 2);
        assert_eq!(ls.loop_types.len(), 2);
        assert_eq!(ls.loop_types[0].depth, 0);
        assert_eq!(ls.loop_types[1].depth, 1);
    }

    #[test]
    fn test_loop_structure_with_independence() {
        let ls = LoopStructure::new().with_independence(true);
        assert!(ls.loops_independent);
        let ls = LoopStructure::new().with_independence(false);
        assert!(!ls.loops_independent);
    }

    #[test]
    fn test_loop_structure_builder_chains() {
        let ls = LoopStructure::new()
            .with_max_depth(2)
            .with_loop(sample_loop(0))
            .with_loop(sample_loop(1))
            .with_independence(true);
        assert_eq!(ls.max_depth, 2);
        assert_eq!(ls.loop_count, 2);
        assert!(ls.loops_independent);
    }

    #[test]
    fn test_loop_type_fields() {
        let lt = LoopType {
            header: NodeId::new(7),
            kind: LoopKind::While,
            depth: 2,
            bounds: LoopBounds::Constant(16),
            has_early_exit: true,
        };
        assert_eq!(lt.header, NodeId::new(7));
        assert_eq!(lt.kind, LoopKind::While);
        assert_eq!(lt.depth, 2);
        assert!(lt.has_early_exit);
        assert!(matches!(lt.bounds, LoopBounds::Constant(16)));
    }

    #[test]
    fn test_loop_kind_variants_distinct() {
        // Copy + PartialEq behaviour across all variants.
        let kinds = [
            LoopKind::CountedFor,
            LoopKind::ForEach,
            LoopKind::While,
            LoopKind::DoWhile,
            LoopKind::Infinite,
        ];
        for (i, a) in kinds.iter().enumerate() {
            for (j, b) in kinds.iter().enumerate() {
                assert_eq!(i == j, a == b);
            }
        }
    }

    #[test]
    fn test_loop_bounds_default_is_unknown() {
        assert!(matches!(LoopBounds::default(), LoopBounds::Unknown));
    }

    #[test]
    fn test_loop_bounds_variants_construct() {
        // Exercise every LoopBounds variant (no PartialEq derive → match).
        assert!(matches!(LoopBounds::Constant(4), LoopBounds::Constant(4)));
        assert!(matches!(LoopBounds::LinearN, LoopBounds::LinearN));
        assert!(matches!(LoopBounds::LogarithmicN, LoopBounds::LogarithmicN));
        assert!(matches!(LoopBounds::Multiple, LoopBounds::Multiple));
        assert!(matches!(LoopBounds::Unknown, LoopBounds::Unknown));
    }
}
