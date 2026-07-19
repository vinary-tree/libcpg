//! Design pattern definitions and detection.
//!
//! This module provides detection of Gang-of-Four design patterns using
//! subgraph isomorphism (VF2 algorithm) and structural templates.

mod gang_of_four;
mod dpml;
mod metrics;
mod templates;

pub use gang_of_four::{GofPatternDetector, GofPattern, GofCategory};
pub use templates::{build_pattern_cpg, build_pattern_template};
pub use dpml::{DpmlTemplate, DpmlRole, DpmlConstraint, DpmlError};
pub use metrics::PatternMetrics;

use crate::CodePropertyGraph;
use crate::pattern::PatternMatch;

/// Trait for design pattern detection.
pub trait PatternDetector: Send + Sync {
    /// Detects patterns in the CPG.
    fn detect(&self, cpg: &CodePropertyGraph) -> Vec<PatternMatch>;

    /// Returns the patterns this detector can identify.
    fn supported_patterns(&self) -> &[&str];
}
