//! Missing authentication and authorization guard analysis.
//!
//! This module instantiates the population-based missing-check detector for
//! state-mutating APIs, then retains only peer-supported check fingerprints
//! containing a configured authentication or authorization API. Identifier
//! spellings remain abstracted by the shared checker; guard qualification uses
//! typed call identities and member names rather than reparsing source text.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::analysis::{
    analyze_missing_checks, AnalysisError, ApiIdentity, CatalogRule, CheckFingerprint,
    CheckNodeKind, MissingCheckConfig, MissingCheckIssue, Pattern, PeerScope, PopulationFraction,
    PopulationThreshold, TaintCatalog,
};
use crate::{CodePropertyGraph, NodeId};

const AUTHZ_SINK_CLASS: &str = "authorization-protected-mutation";

/// One named family of state-mutating APIs expected to carry an access guard.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AuthzMutationRule {
    /// Stable policy name used for validation and serialized configuration.
    pub name: Arc<str>,
    /// Typed callee-path alternatives representing the mutation family.
    pub calls: Vec<Pattern>,
}

impl AuthzMutationRule {
    /// Creates an empty named mutation rule.
    pub fn new(name: impl Into<Arc<str>>) -> Self {
        Self {
            name: name.into(),
            calls: Vec::new(),
        }
    }

    /// Adds one typed callee-path alternative.
    pub fn with_call(mut self, pattern: Pattern) -> Self {
        self.calls.push(pattern);
        self
    }
}

/// Declarative state-mutation and access-guard policy.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AuthzRuleSet {
    /// State-mutating API families analyzed as protected operations.
    pub mutations: Vec<AuthzMutationRule>,
    /// Typed call/member patterns identifying authentication or authorization checks.
    pub guard_patterns: Vec<Pattern>,
}

/// Population and resource policy for missing access-control analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AuthzAnalysisConfig {
    /// Minimum exact fraction of comparable sites carrying an access guard.
    pub minimum_support: PopulationThreshold,
    /// Minimum number of comparable mutation sites used for inference.
    pub minimum_population: usize,
    /// Population topology used to select comparable sites.
    pub peer_scope: PeerScope,
    /// Maximum AST nodes in one normalized guard fingerprint.
    pub max_check_nodes: usize,
    /// Maximum typed AST depth used to reconstruct callee paths.
    pub max_callee_depth: u32,
}

impl Default for AuthzAnalysisConfig {
    fn default() -> Self {
        let generic = MissingCheckConfig::default();
        Self {
            minimum_support: generic.minimum_support,
            minimum_population: generic.minimum_population,
            peer_scope: generic.peer_scope,
            max_check_nodes: generic.max_check_nodes,
            max_callee_depth: generic.max_callee_depth,
        }
    }
}

/// One state-mutating site missing a peer-supported access guard.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AuthzFinding {
    /// Mutation call site lacking the inferred access guard.
    pub site: NodeId,
    /// Function containing `site`.
    pub function: NodeId,
    /// State-mutating API used at `site`.
    pub api: ApiIdentity,
    /// Authentication/authorization fingerprints absent at `site`.
    pub expected_guards: Vec<CheckFingerprint>,
    /// Exact population support for every expected access guard.
    #[cfg_attr(
        feature = "serde",
        serde(with = "crate::analysis::serde_util::ordered_map")
    )]
    pub peer_support: BTreeMap<CheckFingerprint, PopulationFraction>,
    /// Comparable mutation sites, including `site`.
    pub peer_sites: Vec<NodeId>,
    /// Representative guard nodes for every expected fingerprint.
    #[cfg_attr(
        feature = "serde",
        serde(with = "crate::analysis::serde_util::ordered_map")
    )]
    pub guard_witnesses: BTreeMap<CheckFingerprint, Vec<NodeId>>,
}

/// Invalid access-control policy or incomplete population evidence.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum AuthzAnalysisIssue {
    /// A mutation rule has an empty stable name.
    EmptyRuleName,
    /// A named mutation rule has no callee alternatives.
    MissingMutationPattern {
        /// Invalid rule name.
        rule: Arc<str>,
    },
    /// A mutation or guard pattern contains an empty matcher.
    EmptyPattern {
        /// Invalid rule name or policy family.
        rule: Arc<str>,
    },
    /// No authentication or authorization guard pattern was configured.
    MissingGuardPattern,
    /// The shared population detector encountered incomplete evidence.
    Population(MissingCheckIssue),
}

/// Findings and completeness evidence from missing access-control analysis.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct AuthzAnalysis {
    /// Canonically ordered advisory findings.
    pub findings: Vec<AuthzFinding>,
    /// Mutation sites whose complete guard evidence entered a population.
    pub analyzed_sites: Vec<NodeId>,
    /// Ordered policy, structural, and cap evidence.
    pub issues: BTreeSet<AuthzAnalysisIssue>,
}

impl AuthzAnalysis {
    /// Returns whether every candidate mutation and guard was represented completely.
    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
    }
}

/// Returns the cross-language access-control policy shipped with libcpg.
pub fn builtin_authz_rules() -> AuthzRuleSet {
    let persistence = AuthzMutationRule::new("persistence-mutation")
        .with_call(Pattern::Segment("create".into()))
        .with_call(Pattern::Segment("update".into()))
        .with_call(Pattern::Segment("delete".into()))
        .with_call(Pattern::Segment("insert".into()))
        .with_call(Pattern::Segment("save".into()))
        .with_call(Pattern::Segment("remove".into()));
    let mutating_route = AuthzMutationRule::new("mutating-route")
        .with_call(Pattern::Segment("post".into()))
        .with_call(Pattern::Segment("put".into()))
        .with_call(Pattern::Segment("patch".into()));
    AuthzRuleSet {
        mutations: vec![persistence, mutating_route],
        guard_patterns: [
            "auth",
            "Auth",
            "permission",
            "Permission",
            "principal",
            "Principal",
            "policy",
            "Policy",
            "scope",
            "Scope",
            "role",
            "Role",
        ]
        .into_iter()
        .map(|pattern| Pattern::Substring(pattern.into()))
        .collect(),
    }
}

/// Analyzes the built-in missing access-control policy.
pub fn analyze_builtin_authz(
    cpg: &CodePropertyGraph,
    config: &AuthzAnalysisConfig,
) -> Result<AuthzAnalysis, AnalysisError> {
    analyze_missing_authz(cpg, &builtin_authz_rules(), config)
}

/// Instantiates population-based missing-check inference for access guards.
pub fn analyze_missing_authz(
    cpg: &CodePropertyGraph,
    rules: &AuthzRuleSet,
    config: &AuthzAnalysisConfig,
) -> Result<AuthzAnalysis, AnalysisError> {
    let mut issues = validate(rules);
    let mut catalog = TaintCatalog::empty();
    for rule in &rules.mutations {
        if rule.name.is_empty() || rule.calls.is_empty() {
            continue;
        }
        for pattern in rule
            .calls
            .iter()
            .filter(|pattern| !pattern_is_empty(pattern))
        {
            catalog.sinks.push(CatalogRule {
                pattern: pattern.clone(),
                class: AUTHZ_SINK_CLASS.into(),
                cwe: Some(862),
            });
        }
    }
    let missing_config = MissingCheckConfig {
        catalog,
        sensitive_classes: BTreeSet::from([Arc::from(AUTHZ_SINK_CLASS)]),
        minimum_support: config.minimum_support,
        minimum_population: config.minimum_population,
        peer_scope: config.peer_scope,
        max_check_nodes: config.max_check_nodes,
        max_callee_depth: config.max_callee_depth,
    };
    let population = analyze_missing_checks(cpg, &missing_config)?;
    issues.extend(
        population
            .issues
            .iter()
            .cloned()
            .map(AuthzAnalysisIssue::Population),
    );

    let mut findings = Vec::new();
    for finding in population.findings {
        let expected_guards: Vec<_> = finding
            .expected_checks
            .into_iter()
            .filter(|check| is_access_guard(check, &rules.guard_patterns))
            .collect();
        if expected_guards.is_empty() {
            continue;
        }
        let expected_set: BTreeSet<_> = expected_guards.iter().cloned().collect();
        findings.push(AuthzFinding {
            site: finding.site,
            function: finding.function,
            api: finding.api,
            expected_guards,
            peer_support: finding
                .peer_support
                .into_iter()
                .filter(|(check, _)| expected_set.contains(check))
                .collect(),
            peer_sites: finding.peer_sites,
            guard_witnesses: finding
                .check_witnesses
                .into_iter()
                .filter(|(check, _)| expected_set.contains(check))
                .collect(),
        });
    }
    findings.sort();
    Ok(AuthzAnalysis {
        findings,
        analyzed_sites: population.analyzed_sites,
        issues,
    })
}

fn is_access_guard(check: &CheckFingerprint, patterns: &[Pattern]) -> bool {
    check.nodes.iter().any(|node| {
        let name = match &node.kind {
            CheckNodeKind::Call(api) => Some(api.name()),
            CheckNodeKind::MemberAccess(member)
            | CheckNodeKind::Attribute(member)
            | CheckNodeKind::Macro(member) => Some(member.as_ref()),
            _ => None,
        };
        name.is_some_and(|name| {
            patterns
                .iter()
                .filter(|pattern| !pattern_is_empty(pattern))
                .any(|pattern| pattern.matches(name))
        })
    })
}

fn validate(rules: &AuthzRuleSet) -> BTreeSet<AuthzAnalysisIssue> {
    let mut issues = BTreeSet::new();
    if rules.guard_patterns.is_empty() {
        issues.insert(AuthzAnalysisIssue::MissingGuardPattern);
    }
    if rules.guard_patterns.iter().any(pattern_is_empty) {
        issues.insert(AuthzAnalysisIssue::EmptyPattern {
            rule: "guard-patterns".into(),
        });
    }
    for rule in &rules.mutations {
        if rule.name.is_empty() {
            issues.insert(AuthzAnalysisIssue::EmptyRuleName);
        }
        if rule.calls.is_empty() {
            issues.insert(AuthzAnalysisIssue::MissingMutationPattern {
                rule: rule.name.clone(),
            });
        }
        if rule.calls.iter().any(pattern_is_empty) {
            issues.insert(AuthzAnalysisIssue::EmptyPattern {
                rule: rule.name.clone(),
            });
        }
    }
    issues
}

fn pattern_is_empty(pattern: &Pattern) -> bool {
    match pattern {
        Pattern::Exact(value)
        | Pattern::Suffix(value)
        | Pattern::Substring(value)
        | Pattern::Segment(value) => value.is_empty(),
        Pattern::AllSubstrings(values) => {
            values.is_empty() || values.iter().any(|value| value.is_empty())
        }
    }
}

#[cfg(test)]
#[path = "authz_tests.rs"]
mod tests;
