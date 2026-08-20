//! Deterministic advisory bug detectors over normalized CPG evidence.
//!
//! Every detector is feature-free, read-only, and explicit about incomplete
//! evidence. Graph-shaped traversal is performed with heap-backed worklists so
//! source depth and graph depth never consume native call-stack depth.

mod api_mining;
mod clone_bugs;
mod deviant;
mod evidence;
mod exceptions;
mod infeasible;
mod leaks;
mod missing_check;
mod uninit;

pub use api_mining::{
    analyze_api_usage_mining, api_usage_misuses, ApiDataShape, ApiLiteralShape, ApiUsageEdge,
    ApiUsageEdgeKind, ApiUsageGraph, ApiUsageMiningAnalysis, ApiUsageMiningConfig,
    ApiUsageMiningIssue, ApiUsageMisuse, ApiUsageMisuseKind, ApiUsageNode, ApiUsageNodeId,
    ApiUsageNodeKind, ApiUsagePattern, ApiUsagePatternKind, ControlContext, NamedTemporalProperty,
    TemporalPropertyAnalysis, TemporalPropertyFinding, TemporalPropertyFindingKind,
    MAX_API_USAGE_CALL_SITES, MAX_API_USAGE_CFG_NODES_PER_SITE, MAX_API_USAGE_GRAPH_EDGES,
    MAX_API_USAGE_GRAPH_NODES, MAX_TEMPORAL_PROPERTIES,
};
pub use clone_bugs::{
    analyze_inconsistent_clones, inconsistent_clone_bugs, CloneBugAnalysis, CloneBugConfig,
    CloneBugIssue, CloneGuardValue, ClonePairEvidence, InconsistentCloneBug, InconsistentCloneKind,
    RenameDirection, MAX_CLONE_ALIGNMENT_CELLS, MAX_CLONE_BUG_AST_NODES,
};
pub use deviant::{
    analyze_deviant_behavior, deviant_behaviors, CallOrderBelief, DeviantBehavior,
    DeviantBehaviorAnalysis, DeviantBehaviorConfig, DeviantBehaviorIssue, DeviantBehaviorKind,
    ReturnCheckBelief,
};
pub use evidence::{ApiIdentity, PopulationFraction, PopulationThreshold};

pub use exceptions::{
    analyze_exception_flow, dead_catches, uncaught_exceptions, DeadCatch, ExceptionFlowAnalysis,
    ExceptionFlowIssue, ExceptionSummary, ExceptionType, UncaughtException,
};
pub use infeasible::{
    analyze_infeasible_paths, infeasible_paths, AnalyzedCondition, BranchOutcome,
    FeasibilityProofSource, InfeasiblePathAnalysis, InfeasiblePathConfig, InfeasiblePathFinding,
    InfeasiblePathIssue, InfeasiblePathKind, IntervalContradiction, IntervalPredicateEvidence,
    MAX_CONDITION_NODES, MAX_CONDITION_STATES, MAX_CONDITION_STEPS,
};
pub use leaks::{
    analyze_resource_leaks, resource_leaks, ResourceLeak, ResourceLeakAnalysis, ResourceLeakIssue,
};
pub use missing_check::{
    analyze_missing_checks, missing_checks, CheckFingerprint, CheckFingerprintNode, CheckNodeKind,
    MissingCheck, MissingCheckAnalysis, MissingCheckConfig, MissingCheckIssue, PeerScope,
};
pub use uninit::{
    analyze_uninitialized_uses, uninitialized_uses, UninitializedUse, UninitializedUseAnalysis,
};
