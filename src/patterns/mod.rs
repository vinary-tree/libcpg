//! Design pattern detection.
//!
//! This module provides detection of Gang-of-Four and other common
//! design patterns using CPG analysis and subgraph matching.

pub mod design;
pub mod classification;

pub use design::{PatternDetector, GofPatternDetector, GofPattern};
pub use classification::PatternClassifier;
