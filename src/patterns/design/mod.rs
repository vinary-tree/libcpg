//! Design pattern definitions and detection.
//!
//! This module provides detection of Gang-of-Four design patterns using
//! subgraph isomorphism (VF2 algorithm) and structural templates.

mod dpml;
mod gang_of_four;
mod metrics;
mod templates;

pub use dpml::{DpmlConstraint, DpmlError, DpmlRole, DpmlTemplate};
pub use gang_of_four::{GofCategory, GofPattern, GofPatternDetector};
pub use metrics::PatternMetrics;
pub use templates::{build_pattern_cpg, build_pattern_template};

use crate::pattern::PatternMatch;
use crate::CodePropertyGraph;

/// Trait for design pattern detection.
pub trait PatternDetector: Send + Sync {
    /// Detects patterns in the CPG.
    fn detect(&self, cpg: &CodePropertyGraph) -> Vec<PatternMatch>;

    /// Returns the patterns this detector can identify.
    fn supported_patterns(&self) -> &[&str];
}
