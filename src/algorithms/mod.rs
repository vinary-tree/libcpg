//! Algorithm detection and complexity analysis.
//!
//! This module provides tools for identifying algorithm families
//! (sorting, searching, graph algorithms, etc.) and estimating
//! computational complexity from code structure.

pub mod detection;
pub mod families;
pub mod signatures;

pub use detection::{AlgorithmDetector, DetectedAlgorithm};
pub use families::AlgorithmFamily;
pub use signatures::{AlgorithmSignature, ComplexityClass, ComplexityEstimate};
