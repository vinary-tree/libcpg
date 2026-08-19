//! Deterministic advisory bug detectors over normalized CPG evidence.
//!
//! Every detector is feature-free, read-only, and explicit about incomplete
//! evidence. Graph-shaped traversal is performed with heap-backed worklists so
//! source depth and graph depth never consume native call-stack depth.

mod exceptions;
mod leaks;
mod uninit;

pub use exceptions::{
    analyze_exception_flow, dead_catches, uncaught_exceptions, DeadCatch, ExceptionFlowAnalysis,
    ExceptionFlowIssue, ExceptionSummary, ExceptionType, UncaughtException,
};
pub use leaks::{
    analyze_resource_leaks, resource_leaks, ResourceLeak, ResourceLeakAnalysis, ResourceLeakIssue,
};
pub use uninit::{
    analyze_uninitialized_uses, uninitialized_uses, UninitializedUse, UninitializedUseAnalysis,
};
