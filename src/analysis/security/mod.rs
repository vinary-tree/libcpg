//! Declarative, evidence-qualified security analyses.
//!
//! Security findings in this module are advisory candidates over represented
//! Code Property Graph evidence. Consumers making a negative claim must retain
//! each complete report and check its [`is_complete`](CweAnalysis::is_complete)
//! predicate.

mod authz;
mod crypto;
mod cwe;
mod extrapolate;
mod secrets;

pub use authz::{
    analyze_builtin_authz, analyze_missing_authz, builtin_authz_rules, AuthzAnalysis,
    AuthzAnalysisConfig, AuthzAnalysisIssue, AuthzFinding, AuthzMutationRule, AuthzRuleSet,
};
pub use crypto::{
    analyze_builtin_crypto, analyze_crypto_misuse, builtin_crypto_rules, CryptoAnalysis,
    CryptoAnalysisConfig, CryptoAnalysisIssue, CryptoArgumentRule, CryptoEvidence, CryptoFinding,
    CryptoMisuseKind, CryptoRuleSet, MAX_CRYPTO_CALLS, MAX_CRYPTO_ORIGIN_NODES,
};
pub use cwe::{
    analyze_builtin_cwes, analyze_cwe_templates, builtin_cwe_templates, CweAnalysis,
    CweAnalysisConfig, CweAnalysisError, CweAnalysisIssue, CweCandidate, CweEvidence,
    CwePatternEdge, CwePrimitive, CweRole, CweStructuralMatch, CweStructuralPattern, CweTemplate,
    CweTypestateAnalysis, MAX_CWE_ARRAY_ORIGIN_NODES, MAX_CWE_MATCHES_PER_PATTERN,
    MAX_CWE_PATTERNS_PER_TEMPLATE, MAX_CWE_TEMPLATES,
};
pub use extrapolate::{
    analyze_vulnerability_extrapolation, extrapolate, ExtrapolationAnalysis,
    ExtrapolationCandidate, ExtrapolationIssue, ExtrapolationOptions, MAX_EXTRAPOLATION_FUNCTIONS,
    MAX_EXTRAPOLATION_PDG_NODES, MAX_EXTRAPOLATION_RESULTS, MAX_EXTRAPOLATION_VF2_INVOCATIONS,
};
pub use secrets::{
    analyze_builtin_secrets, analyze_secret_flows, builtin_secret_rules, SecretAnalysis,
    SecretAnalysisConfig, SecretAnalysisIssue, SecretFinding, SecretLiteralEvidence, SecretRuleSet,
    SecretSinkFamily, SecretSinkRule, MAX_SECRET_LITERALS, MAX_SECRET_NAME_CONTEXT_NODES,
};

#[cfg(test)]
mod extrapolate_tests;
