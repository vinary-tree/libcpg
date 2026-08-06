//! Pattern classification using rule-based heuristics and optional ML techniques.
//!
//! This module provides design pattern classification with support for:
//! - **Rule-based**: Hand-crafted heuristics for common patterns (always available)
//! - **ML-based**: Trained models for classification (requires `ml-linfa` feature)
//! - **Hybrid**: Combines both approaches for higher accuracy

mod classifier;

pub use classifier::{ClassificationMode, FeatureVector, PatternClassifier};
