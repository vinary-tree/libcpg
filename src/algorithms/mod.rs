//! Algorithm detection and complexity analysis.
//!
//! This module provides tools for identifying algorithm families
//! (sorting, searching, graph algorithms, etc.) and estimating
//! computational complexity from code structure.

pub mod detection;
pub mod signatures;
pub mod families;

pub use detection::{AlgorithmDetector, DetectedAlgorithm};
pub use signatures::{AlgorithmSignature, ComplexityEstimate, ComplexityClass};
pub use families::AlgorithmFamily;
