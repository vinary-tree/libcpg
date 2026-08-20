//! Secret and credential flow analysis over typed literal and taint evidence.
//!
//! Candidate string literals are qualified without copying their contents into
//! findings: either their byte distribution crosses an explicit entropy
//! threshold, or a nearby typed binding name matches a configured hint. The
//! shared IFDS taint engine then proves whether that literal reaches an
//! authentication, network, or cryptographic-key sink. All structural walks
//! use heap-backed worklists or bounded parent loops.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::analysis::{
    analyze_taint_report, CallGraphResolution, CatalogRule, IfdsCaps, LiteralPattern,
    LiteralSourceRule, Pattern, TaintAnalysis, TaintCatalog,
};
use crate::{CodePropertyGraph, CpgNodeKind, LiteralKind, NodeId};

/// Compile-time ceiling for string literals inspected by one analysis.
pub const MAX_SECRET_LITERALS: usize = 1_000_000;
/// Compile-time ceiling for typed binding-context nodes inspected per literal.
pub const MAX_SECRET_NAME_CONTEXT_NODES: usize = 1_000_000;

const SECRET_SOURCE_CLASS: &str = "embedded-secret-candidate";
const SECRET_SINK_PREFIX: &str = "secret-sink:";

/// Security boundary to which a candidate secret flows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum SecretSinkFamily {
    /// Authentication or credential-consuming API.
    Authentication,
    /// Network transmission API.
    Network,
    /// Cryptographic key-bearing API.
    CryptoKey,
}

/// A named, typed callee rule for one secret-bearing sink family.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SecretSinkRule {
    /// Stable policy name retained by findings.
    pub name: Arc<str>,
    /// Security boundary represented by this rule.
    pub family: SecretSinkFamily,
    /// Typed callee-path alternatives.
    pub calls: Vec<Pattern>,
    /// Governed zero-based arguments; `None` means every argument.
    pub argument_indices: Option<BTreeSet<u16>>,
}

impl SecretSinkRule {
    /// Creates a rule that initially governs every argument.
    pub fn new(name: impl Into<Arc<str>>, family: SecretSinkFamily) -> Self {
        Self {
            name: name.into(),
            family,
            calls: Vec::new(),
            argument_indices: None,
        }
    }

    /// Adds one typed callee-path alternative.
    pub fn with_call(mut self, pattern: Pattern) -> Self {
        self.calls.push(pattern);
        self
    }

    /// Restricts the rule to a governed zero-based argument.
    pub fn with_argument(mut self, index: u16) -> Self {
        self.argument_indices
            .get_or_insert_with(BTreeSet::new)
            .insert(index);
        self
    }

    fn governs(&self, argument: u16) -> bool {
        self.argument_indices
            .as_ref()
            .is_none_or(|indices| indices.contains(&argument))
    }
}

/// Declarative policy for secret qualification and security sinks.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SecretRuleSet {
    /// Authentication, network, and cryptographic-key sink rules.
    pub sinks: Vec<SecretSinkRule>,
    /// Case-insensitive patterns matched against nearby typed binding names.
    pub name_hints: Vec<Pattern>,
}

/// Resource and threshold policy for secret-flow analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SecretAnalysisConfig {
    /// Shared bounded IFDS policy.
    pub ifds_caps: IfdsCaps,
    /// Minimum string length in bytes for entropy qualification.
    pub minimum_entropy_bytes: usize,
    /// Minimum Shannon entropy in thousandths of a bit per byte.
    pub minimum_entropy_millibits_per_byte: u16,
    /// Maximum string literals inspected after canonical ordering.
    pub max_literals: usize,
    /// Maximum typed binding-context nodes inspected per literal.
    pub max_name_context_nodes_per_literal: usize,
}

impl Default for SecretAnalysisConfig {
    fn default() -> Self {
        Self {
            ifds_caps: IfdsCaps::default(),
            minimum_entropy_bytes: 16,
            minimum_entropy_millibits_per_byte: 3_500,
            max_literals: 100_000,
            max_name_context_nodes_per_literal: 10_000,
        }
    }
}

/// Non-sensitive evidence that qualified one literal as a secret candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum SecretLiteralEvidence {
    /// The literal crossed the configured Shannon-entropy threshold.
    HighEntropy {
        /// Literal length in bytes; the literal contents are deliberately absent.
        byte_len: usize,
        /// Measured Shannon entropy in thousandths of a bit per byte.
        entropy_millibits_per_byte: u16,
    },
    /// A typed binding in the literal's structural context matched a hint.
    NameHint {
        /// Matched policy pattern, not the potentially sensitive binding text.
        hint: Pattern,
        /// Typed context node carrying the matching name.
        context: NodeId,
    },
}

/// One deterministic, advisory secret-flow candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SecretFinding {
    /// Concrete candidate literal node.
    pub source: NodeId,
    /// Concrete security-sensitive call site.
    pub sink: NodeId,
    /// Security boundary crossed by the flow.
    pub family: SecretSinkFamily,
    /// Stable sink-rule name.
    pub rule: Arc<str>,
    /// Positional sink argument.
    pub argument_index: u16,
    /// Entropy or typed name-hint evidence without literal contents.
    pub literal_evidence: SecretLiteralEvidence,
    /// Canonical typed-transfer witness from literal to sink.
    pub witness: Vec<NodeId>,
    /// Minimum transfer/classification confidence in thousandths.
    pub confidence_milli: u16,
    /// Whether a relevant resource boundary made the flow partial.
    pub capped: bool,
}

/// Invalid policy, resource cap, or malformed structural evidence.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum SecretAnalysisIssue {
    /// A sink rule has an empty stable name.
    EmptyRuleName,
    /// A named sink rule has no callee alternatives.
    MissingCallPattern {
        /// Invalid rule name.
        rule: Arc<str>,
    },
    /// A rule or name hint contains an empty matcher.
    EmptyPattern {
        /// Invalid rule name or policy family.
        rule: Arc<str>,
    },
    /// The entropy length threshold must be positive.
    ZeroEntropyLength,
    /// Shannon entropy cannot exceed eight bits per byte.
    EntropyThresholdOutOfRange {
        /// Rejected threshold in thousandths of a bit per byte.
        threshold: u16,
    },
    /// Canonical string-literal enumeration exceeded its requested bound.
    LiteralsCapped {
        /// String literals represented in the input.
        observed: usize,
        /// Effective bound.
        cap: usize,
    },
    /// Typed binding-context traversal exceeded its requested node bound.
    NameContextCapped {
        /// Candidate literal whose binding context was partial.
        literal: NodeId,
        /// Effective bound.
        cap: usize,
    },
}

/// Findings and completeness evidence from secret-flow analysis.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SecretAnalysis {
    /// Canonically ordered advisory findings.
    pub findings: Vec<SecretFinding>,
    /// Underlying literal-origin IFDS report retained for auditability.
    pub taint: TaintAnalysis,
    /// Ordered policy, structural, and cap evidence.
    pub issues: BTreeSet<SecretAnalysisIssue>,
}

impl SecretAnalysis {
    /// Returns whether qualification and propagation were both complete.
    pub fn is_complete(&self) -> bool {
        self.issues.is_empty() && self.taint.is_complete()
    }
}

/// Returns the cross-language secret-flow policy shipped with libcpg.
pub fn builtin_secret_rules() -> SecretRuleSet {
    use SecretSinkFamily::{Authentication, CryptoKey, Network};

    let authentication = SecretSinkRule::new("authentication", Authentication)
        .with_call(Pattern::Substring("auth".into()))
        .with_call(Pattern::Segment("login".into()))
        .with_call(Pattern::Segment("credential".into()))
        .with_call(Pattern::Segment("bearer_auth".into()));
    let network = SecretSinkRule::new("network-transmission", Network)
        .with_call(Pattern::Segment("send".into()))
        .with_call(Pattern::Segment("post".into()))
        .with_call(Pattern::Segment("publish".into()))
        .with_call(Pattern::Segment("upload".into()));
    let crypto_key = SecretSinkRule::new("cryptographic-key", CryptoKey)
        .with_call(Pattern::Segment("set_key".into()))
        .with_call(Pattern::Segment("with_key".into()))
        .with_call(Pattern::Segment("new_from_slice".into()))
        .with_argument(0);
    SecretRuleSet {
        sinks: vec![authentication, network, crypto_key],
        name_hints: [
            "secret",
            "password",
            "passwd",
            "credential",
            "access_token",
            "api_key",
            "apikey",
            "private_key",
        ]
        .into_iter()
        .map(|hint| Pattern::Substring(hint.into()))
        .chain(std::iter::once(Pattern::Segment("token".into())))
        .collect(),
    }
}

/// Analyzes the built-in secret-flow policy.
pub fn analyze_builtin_secrets(
    cpg: &CodePropertyGraph,
    resolution: &CallGraphResolution,
    config: &SecretAnalysisConfig,
) -> SecretAnalysis {
    analyze_secret_flows(cpg, resolution, &builtin_secret_rules(), config)
}

/// Qualifies typed string literals and propagates them with the shared IFDS engine.
pub fn analyze_secret_flows(
    cpg: &CodePropertyGraph,
    resolution: &CallGraphResolution,
    rules: &SecretRuleSet,
    config: &SecretAnalysisConfig,
) -> SecretAnalysis {
    let mut issues = validate(rules, config);
    let (catalog, sink_rules) = secret_catalog(rules);
    let taint = analyze_taint_report(cpg, resolution, &catalog, &config.ifds_caps);
    let literal_cap = config.max_literals.min(MAX_SECRET_LITERALS);
    let context_cap = config
        .max_name_context_nodes_per_literal
        .min(MAX_SECRET_NAME_CONTEXT_NODES);

    let mut literals: Vec<_> = cpg
        .nodes()
        .filter_map(|node| match &node.kind {
            CpgNodeKind::Literal {
                kind: LiteralKind::String(value),
            } => Some((node.id, value.clone())),
            _ => None,
        })
        .collect();
    literals.sort_unstable_by_key(|(literal, _)| source_key(cpg, *literal));
    if literals.len() > literal_cap {
        issues.insert(SecretAnalysisIssue::LiteralsCapped {
            observed: literals.len(),
            cap: literal_cap,
        });
        literals.truncate(literal_cap);
    }

    let mut qualified = BTreeMap::new();
    for (literal, value) in literals {
        let entropy = shannon_entropy_millibits(value.as_bytes());
        if value.len() >= config.minimum_entropy_bytes
            && entropy >= config.minimum_entropy_millibits_per_byte
        {
            qualified.insert(
                literal,
                SecretLiteralEvidence::HighEntropy {
                    byte_len: value.len(),
                    entropy_millibits_per_byte: entropy,
                },
            );
            continue;
        }
        let context = name_hint_evidence(cpg, literal, &rules.name_hints, context_cap);
        if context.capped {
            issues.insert(SecretAnalysisIssue::NameContextCapped {
                literal,
                cap: context_cap,
            });
        }
        if let Some((hint, context)) = context.matched {
            qualified.insert(literal, SecretLiteralEvidence::NameHint { hint, context });
        }
    }

    let mut findings = BTreeMap::new();
    for flow in &taint.flows {
        if flow.source_class.as_ref() != SECRET_SOURCE_CLASS {
            continue;
        }
        let Some(literal_evidence) = qualified.get(&flow.source) else {
            continue;
        };
        let Some(argument_index) = flow.param_index else {
            continue;
        };
        let Some(rule) = sink_rules.get(&flow.sink_class) else {
            continue;
        };
        if !rule.governs(argument_index) {
            continue;
        }
        let finding = SecretFinding {
            source: flow.source,
            sink: flow.sink,
            family: rule.family,
            rule: rule.name.clone(),
            argument_index,
            literal_evidence: literal_evidence.clone(),
            witness: flow.witness.clone(),
            confidence_milli: confidence_milli(flow.confidence),
            capped: flow.capped,
        };
        findings.entry(secret_key(&finding)).or_insert(finding);
    }

    SecretAnalysis {
        findings: findings.into_values().collect(),
        taint,
        issues,
    }
}

fn secret_catalog(rules: &SecretRuleSet) -> (TaintCatalog, BTreeMap<Arc<str>, &SecretSinkRule>) {
    let mut catalog = TaintCatalog {
        literal_sources: vec![LiteralSourceRule {
            pattern: LiteralPattern::String,
            class: SECRET_SOURCE_CLASS.into(),
            cwe: Some(798),
        }],
        ..TaintCatalog::empty()
    };
    let mut sink_rules = BTreeMap::new();
    for (index, rule) in rules.sinks.iter().enumerate() {
        if rule.name.is_empty() || rule.calls.is_empty() {
            continue;
        }
        let class: Arc<str> = format!("{SECRET_SINK_PREFIX}{index}").into();
        sink_rules.insert(class.clone(), rule);
        for pattern in rule
            .calls
            .iter()
            .filter(|pattern| !pattern_is_empty(pattern))
        {
            catalog.sinks.push(CatalogRule {
                pattern: pattern.clone(),
                class: class.clone(),
                cwe: Some(798),
            });
        }
    }
    (catalog, sink_rules)
}

fn validate(rules: &SecretRuleSet, config: &SecretAnalysisConfig) -> BTreeSet<SecretAnalysisIssue> {
    let mut issues = BTreeSet::new();
    if config.minimum_entropy_bytes == 0 {
        issues.insert(SecretAnalysisIssue::ZeroEntropyLength);
    }
    if config.minimum_entropy_millibits_per_byte > 8_000 {
        issues.insert(SecretAnalysisIssue::EntropyThresholdOutOfRange {
            threshold: config.minimum_entropy_millibits_per_byte,
        });
    }
    for rule in &rules.sinks {
        if rule.name.is_empty() {
            issues.insert(SecretAnalysisIssue::EmptyRuleName);
        }
        if rule.calls.is_empty() {
            issues.insert(SecretAnalysisIssue::MissingCallPattern {
                rule: rule.name.clone(),
            });
        }
        if rule.calls.iter().any(pattern_is_empty) {
            issues.insert(SecretAnalysisIssue::EmptyPattern {
                rule: rule.name.clone(),
            });
        }
    }
    if rules.name_hints.iter().any(pattern_is_empty) {
        issues.insert(SecretAnalysisIssue::EmptyPattern {
            rule: "name-hints".into(),
        });
    }
    issues
}

struct NameContextEvidence {
    matched: Option<(Pattern, NodeId)>,
    capped: bool,
}

fn name_hint_evidence(
    cpg: &CodePropertyGraph,
    literal: NodeId,
    hints: &[Pattern],
    cap: usize,
) -> NameContextEvidence {
    let mut examined = 0usize;
    let mut visited = BTreeSet::new();
    let mut current = Some(literal);
    while let Some(node) = current {
        if !visited.insert(node) {
            break;
        }
        if examined == cap {
            return NameContextEvidence {
                matched: None,
                capped: true,
            };
        }
        examined += 1;
        if let Some(matched) = matching_name(cpg, node, hints) {
            return NameContextEvidence {
                matched: Some((matched, node)),
                capped: false,
            };
        }
        let Some(parent) = cpg.ast_parent(node) else {
            break;
        };
        match cpg.node(parent).map(|node| &node.kind) {
            Some(CpgNodeKind::Assignment { .. }) => {
                let lhs = cpg.ast_children(parent).into_iter().next();
                if let Some(lhs) = lhs {
                    let result = search_named_subtree(cpg, lhs, hints, cap, &mut examined);
                    if result.matched.is_some() || result.capped {
                        return result;
                    }
                }
                break;
            }
            Some(CpgNodeKind::Function { .. })
            | Some(CpgNodeKind::Module { .. })
            | Some(CpgNodeKind::Root) => break,
            Some(_) => current = Some(parent),
            None => break,
        }
    }

    for neighbor in cpg
        .dfg_predecessors(literal)
        .into_iter()
        .map(|(node, _)| node)
        .chain(
            cpg.dfg_successors(literal)
                .into_iter()
                .map(|(node, _)| node),
        )
    {
        if examined == cap {
            return NameContextEvidence {
                matched: None,
                capped: true,
            };
        }
        examined += 1;
        if let Some(matched) = matching_name(cpg, neighbor, hints) {
            return NameContextEvidence {
                matched: Some((matched, neighbor)),
                capped: false,
            };
        }
    }
    NameContextEvidence {
        matched: None,
        capped: false,
    }
}

fn search_named_subtree(
    cpg: &CodePropertyGraph,
    root: NodeId,
    hints: &[Pattern],
    cap: usize,
    examined: &mut usize,
) -> NameContextEvidence {
    let mut queue = VecDeque::from([root]);
    let mut visited = BTreeSet::new();
    while let Some(node) = queue.pop_front() {
        if !visited.insert(node) {
            continue;
        }
        if *examined == cap {
            return NameContextEvidence {
                matched: None,
                capped: true,
            };
        }
        *examined += 1;
        if let Some(matched) = matching_name(cpg, node, hints) {
            return NameContextEvidence {
                matched: Some((matched, node)),
                capped: false,
            };
        }
        queue.extend(cpg.ast_children(node));
    }
    NameContextEvidence {
        matched: None,
        capped: false,
    }
}

fn matching_name(cpg: &CodePropertyGraph, node: NodeId, hints: &[Pattern]) -> Option<Pattern> {
    let name = cpg.node(node)?.name()?.to_ascii_lowercase();
    hints
        .iter()
        .filter(|hint| !pattern_is_empty(hint))
        .find(|hint| hint.matches(&name))
        .cloned()
}

fn shannon_entropy_millibits(bytes: &[u8]) -> u16 {
    if bytes.is_empty() {
        return 0;
    }
    let mut counts = [0usize; 256];
    for &byte in bytes {
        counts[usize::from(byte)] += 1;
    }
    let length = bytes.len() as f64;
    let entropy = counts
        .into_iter()
        .filter(|&count| count != 0)
        .map(|count| {
            let probability = count as f64 / length;
            -probability * probability.log2()
        })
        .sum::<f64>();
    (entropy * 1_000.0).round().clamp(0.0, 8_000.0) as u16
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

fn source_key(cpg: &CodePropertyGraph, node: NodeId) -> (u32, u32, NodeId) {
    cpg.node(node)
        .map(|node| (node.range.start, node.range.end, node.id))
        .unwrap_or((u32::MAX, u32::MAX, node))
}

fn secret_key(finding: &SecretFinding) -> (NodeId, NodeId, SecretSinkFamily, Arc<str>, u16) {
    (
        finding.source,
        finding.sink,
        finding.family,
        finding.rule.clone(),
        finding.argument_index,
    )
}

fn confidence_milli(confidence: f32) -> u16 {
    (confidence.clamp(0.0, 1.0) * 1_000.0).round() as u16
}

#[cfg(test)]
#[path = "secrets_tests.rs"]
mod tests;
