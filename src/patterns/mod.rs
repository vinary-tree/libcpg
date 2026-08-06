//! Design pattern detection.
//!
//! This module provides detection of Gang-of-Four and other common
//! design patterns using CPG analysis and subgraph matching.

pub mod classification;
pub mod design;

pub use classification::PatternClassifier;
pub use design::{GofPattern, GofPatternDetector, PatternDetector};
