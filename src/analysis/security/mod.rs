//! Declarative, evidence-qualified security analyses.
//!
//! Security findings in this module are advisory candidates over represented
//! Code Property Graph evidence. Consumers making a negative claim must retain
//! each complete report and check its [`is_complete`](CweAnalysis::is_complete)
//! predicate.

mod cwe;

pub use cwe::{
    analyze_builtin_cwes, analyze_cwe_templates, builtin_cwe_templates, CweAnalysis,
    CweAnalysisConfig, CweAnalysisError, CweAnalysisIssue, CweCandidate, CweEvidence,
    CwePatternEdge, CwePrimitive, CweRole, CweStructuralMatch, CweStructuralPattern, CweTemplate,
    CweTypestateAnalysis, MAX_CWE_ARRAY_ORIGIN_NODES, MAX_CWE_MATCHES_PER_PATTERN,
    MAX_CWE_PATTERNS_PER_TEMPLATE, MAX_CWE_TEMPLATES,
};
