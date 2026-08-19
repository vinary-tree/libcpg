//! Deterministic advisory bug detectors over normalized CPG evidence.
//!
//! Every detector is feature-free, read-only, and explicit about incomplete
//! evidence. Graph-shaped traversal is performed with heap-backed worklists so
//! source depth and graph depth never consume native call-stack depth.

mod leaks;

pub use leaks::{
    analyze_resource_leaks, resource_leaks, ResourceLeak, ResourceLeakAnalysis, ResourceLeakIssue,
};
