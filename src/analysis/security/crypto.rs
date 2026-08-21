//! Declarative cryptographic API-misuse analysis.
//!
//! The checker composes the feature-free typestate and taint engines with
//! typed argument-origin constraints. It never parses retained source text:
//! callees come from typed AST/call-resolution evidence, while constants and
//! provenance come from literal, AST, and DFG nodes. Findings are advisory and
//! remain out of graph.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::analysis::resolve::call_callee_path_evidence;
use crate::analysis::{
    analyze_taint_report, analyze_typestate, CallGraphResolution, CatalogRule, IfdsCaps,
    LiteralPattern, LiteralSourceRule, Pattern, PropertyFsm, TaintAnalysis, TaintCatalog,
    TypestateAnalysis,
};
use crate::builder::call_arguments;
use crate::{CodePropertyGraph, CpgEdgeKind, CpgNodeKind, LiteralKind, NodeId};

/// Compile-time ceiling for call sites inspected by one crypto analysis.
pub const MAX_CRYPTO_CALLS: usize = 1_000_000;
/// Compile-time ceiling for typed provenance nodes inspected per argument.
pub const MAX_CRYPTO_ORIGIN_NODES: usize = 1_000_000;

const HARDCODED_SOURCE_CLASS: &str = "hardcoded-crypto-material";
const KEY_SINK_PREFIX: &str = "crypto-key:";

/// A named, positional constraint over calls recognized by typed path patterns.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CryptoArgumentRule {
    /// Stable rule name used in findings and serialized policy.
    pub name: Arc<str>,
    /// Callee-path alternatives for this rule.
    pub calls: Vec<Pattern>,
    /// Zero-based argument positions governed by this rule.
    pub argument_indices: BTreeSet<u16>,
}

impl CryptoArgumentRule {
    /// Creates an empty named rule.
    pub fn new(name: impl Into<Arc<str>>) -> Self {
        Self {
            name: name.into(),
            calls: Vec::new(),
            argument_indices: BTreeSet::new(),
        }
    }

    /// Adds one callee-path alternative.
    pub fn with_call(mut self, pattern: Pattern) -> Self {
        self.calls.push(pattern);
        self
    }

    /// Adds one governed zero-based argument position.
    pub fn with_argument(mut self, index: u16) -> Self {
        self.argument_indices.insert(index);
        self
    }

    fn matches_call(&self, path: &str) -> bool {
        self.calls.iter().any(|pattern| pattern.matches(path))
    }
}

/// Declarative CrySL-style call-order and parameter policy.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CryptoRuleSet {
    /// Finite-state protocol used for crypto-object call order.
    pub property: PropertyFsm,
    /// Weak-hash callee alternatives such as MD5 and SHA-1 entry points.
    pub weak_hash_calls: Vec<Pattern>,
    /// Algorithm-name arguments that must not select a weak hash.
    pub weak_hash_parameters: Vec<CryptoArgumentRule>,
    /// Algorithm/mode arguments checked for electronic-codebook mode.
    pub mode_parameters: Vec<CryptoArgumentRule>,
    /// Initialization-vector or nonce arguments checked for fixed origins.
    pub iv_parameters: Vec<CryptoArgumentRule>,
    /// Key arguments consumed by literal-origin taint sinks.
    pub key_parameters: Vec<CryptoArgumentRule>,
    /// Calls that establish cryptographically secure random provenance.
    pub secure_random_sources: Vec<Pattern>,
    /// Case-insensitive algorithm segments forbidden as weak hashes.
    pub weak_hash_names: BTreeSet<Arc<str>>,
    /// Case-insensitive algorithm segments forbidden as cipher modes.
    pub forbidden_mode_names: BTreeSet<Arc<str>>,
}

/// Resource policy for cryptographic misuse analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CryptoAnalysisConfig {
    /// Shared bounded IFDS/typestate policy.
    pub ifds_caps: IfdsCaps,
    /// Maximum call sites inspected after canonical ordering.
    pub max_calls: usize,
    /// Maximum typed provenance nodes inspected per argument.
    pub max_origin_nodes_per_argument: usize,
}

impl Default for CryptoAnalysisConfig {
    fn default() -> Self {
        Self {
            ifds_caps: IfdsCaps::default(),
            max_calls: 100_000,
            max_origin_nodes_per_argument: 100_000,
        }
    }
}

/// Stable cryptographic misuse category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CryptoMisuseKind {
    /// A fixed or literal initialization vector/nonce reaches a crypto API.
    PredictableIv,
    /// Electronic-codebook mode was selected.
    EcbMode,
    /// MD5 or SHA-1 was selected for a cryptographic hash operation.
    WeakHash,
    /// Literal-origin key material reaches a key-bearing parameter.
    HardcodedKey,
    /// The composed property automaton observed a forbidden call transition.
    InvalidCallOrder,
}

/// Compact typed evidence retained by a crypto finding.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CryptoEvidence {
    /// Typed callee-path classification.
    CallPath {
        /// Reconstructed or resolved path.
        path: Arc<str>,
    },
    /// Literal provenance found for one positional argument.
    LiteralOrigins {
        /// Argument root in the CPG.
        argument: NodeId,
        /// Canonically ordered literal origins.
        origins: Vec<NodeId>,
    },
    /// Literal-origin taint flow through typed AST/DFG evidence.
    Taint {
        /// Concrete literal source.
        source: NodeId,
        /// Concrete crypto sink.
        sink: NodeId,
        /// Positional sink argument.
        argument_index: u16,
        /// Canonical transfer witness.
        witness: Vec<NodeId>,
        /// Minimum evidence confidence in thousandths.
        confidence_milli: u16,
    },
    /// Forbidden typestate transition.
    Typestate {
        /// Abstract object identity.
        object: NodeId,
        /// State immediately before the forbidden event.
        from: Arc<str>,
        /// Forbidden event label.
        event: Arc<str>,
        /// Canonical allocation-to-event witness.
        witness: Vec<NodeId>,
    },
}

/// One deterministic, advisory cryptographic misuse candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CryptoFinding {
    /// Misuse category.
    pub kind: CryptoMisuseKind,
    /// Primary call site.
    pub site: NodeId,
    /// Stable rule name.
    pub rule: Arc<str>,
    /// Positional argument when the finding is parameter-backed.
    pub argument_index: Option<u16>,
    /// Typed evidence.
    pub evidence: CryptoEvidence,
    /// Whether an explicit resource boundary made the report partial.
    pub capped: bool,
}

/// Structural, policy, or resource issue qualifying a crypto report.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CryptoAnalysisIssue {
    /// A named argument rule has no callee patterns.
    MissingCallPattern {
        /// Invalid rule name.
        rule: Arc<str>,
    },
    /// A named argument rule has no argument position.
    MissingArgumentIndex {
        /// Invalid rule name.
        rule: Arc<str>,
    },
    /// A path pattern contains an empty matcher.
    EmptyPattern {
        /// Invalid rule or rule family.
        rule: Arc<str>,
    },
    /// Canonical call-site enumeration exceeded its requested bound.
    CallsCapped {
        /// Calls represented in the input.
        observed: usize,
        /// Effective bound.
        cap: usize,
    },
    /// One argument's explicit provenance machine exceeded its node bound.
    OriginNodesCapped {
        /// Affected call site.
        site: NodeId,
        /// Zero-based argument position.
        argument_index: u16,
        /// Effective bound.
        cap: usize,
    },
    /// Typed callee reconstruction hit its depth or cycle bound.
    CalleePathCapped {
        /// Affected call site.
        site: NodeId,
    },
}

/// Findings plus complete evidence from the composed analysis engines.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CryptoAnalysis {
    /// Candidates in canonical site/kind/argument/rule order.
    pub findings: Vec<CryptoFinding>,
    /// Complete underlying typestate report.
    pub typestate: TypestateAnalysis,
    /// Complete underlying hardcoded-material taint report.
    pub taint: TaintAnalysis,
    /// Ordered policy, structure, and resource issues.
    pub issues: BTreeSet<CryptoAnalysisIssue>,
    /// Whether an explicit local resource cap was reached.
    pub capped: bool,
}

impl CryptoAnalysis {
    /// Returns whether every represented fact was examined.
    ///
    /// Consumers may make a clean/negative claim only when this predicate is
    /// true; findings themselves remain advisory candidates.
    pub fn is_complete(&self) -> bool {
        !self.capped
            && self.issues.is_empty()
            && self.typestate.is_complete()
            && self.taint.is_complete()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FindingKey {
    site: NodeId,
    kind: CryptoMisuseKind,
    argument_index: Option<u16>,
    rule: Arc<str>,
}

#[derive(Debug, Default)]
struct OriginEvidence {
    literals: BTreeSet<NodeId>,
    calls: BTreeMap<NodeId, Arc<str>>,
    path_capped: BTreeSet<NodeId>,
    capped: bool,
}

/// Returns libcpg's cross-language default cryptographic policy.
pub fn builtin_crypto_rules() -> CryptoRuleSet {
    let property = crypto_property();
    CryptoRuleSet {
        property,
        weak_hash_calls: ["MD5", "Md5", "md5", "SHA1", "Sha1", "sha1"]
            .into_iter()
            .map(|name| Pattern::Segment(name.into()))
            .collect(),
        weak_hash_parameters: vec![CryptoArgumentRule::new("digest-algorithm")
            .with_call(Pattern::Substring("MessageDigest".into()))
            .with_call(Pattern::Segment("digest".into()))
            .with_argument(0)],
        mode_parameters: vec![CryptoArgumentRule::new("cipher-mode")
            .with_call(Pattern::Substring("Cipher".into()))
            .with_call(Pattern::Substring("cipher".into()))
            .with_call(Pattern::Substring("Aes".into()))
            .with_call(Pattern::Substring("AES".into()))
            .with_argument(0)],
        iv_parameters: vec![
            CryptoArgumentRule::new("initialization-vector")
                .with_call(Pattern::Segment("set_iv".into()))
                .with_call(Pattern::Segment("with_iv".into()))
                .with_call(Pattern::Segment("set_nonce".into()))
                .with_argument(0),
            CryptoArgumentRule::new("key-and-iv-constructor")
                .with_call(Pattern::Segment("new_from_slices".into()))
                .with_argument(1),
            CryptoArgumentRule::new("aead-nonce")
                .with_call(Pattern::Segment("encrypt".into()))
                .with_call(Pattern::Segment("decrypt".into()))
                .with_argument(0),
        ],
        key_parameters: vec![
            CryptoArgumentRule::new("key-constructor")
                .with_call(Pattern::Segment("with_key".into()))
                .with_call(Pattern::Segment("set_key".into()))
                .with_call(Pattern::Segment("new_from_slice".into()))
                .with_call(Pattern::Segment("new_from_slices".into()))
                .with_argument(0),
            CryptoArgumentRule::new("secret-key-specification")
                .with_call(Pattern::Substring("SecretKeySpec".into()))
                .with_argument(0),
            CryptoArgumentRule::new("cipher-initialization-key")
                .with_call(Pattern::Substring("Cipher::init".into()))
                .with_argument(1),
        ],
        secure_random_sources: [
            "OsRng",
            "SecureRandom",
            "getrandom",
            "generate_nonce",
            "random_nonce",
            "random_bytes",
        ]
        .into_iter()
        .map(|name| Pattern::Substring(name.into()))
        .collect(),
        weak_hash_names: BTreeSet::from(["md5".into(), "sha1".into()]),
        forbidden_mode_names: BTreeSet::from(["ecb".into()]),
    }
}

/// Runs the built-in cryptographic misuse policy.
pub fn analyze_builtin_crypto(
    cpg: &CodePropertyGraph,
    resolution: &CallGraphResolution,
    config: &CryptoAnalysisConfig,
) -> CryptoAnalysis {
    analyze_crypto_misuse(cpg, resolution, &builtin_crypto_rules(), config)
}

/// Runs one declarative CrySL-style cryptographic misuse policy.
pub fn analyze_crypto_misuse(
    cpg: &CodePropertyGraph,
    resolution: &CallGraphResolution,
    rules: &CryptoRuleSet,
    config: &CryptoAnalysisConfig,
) -> CryptoAnalysis {
    let mut issues = validate_rules(rules);
    let typestate = analyze_typestate(cpg, &rules.property, &config.ifds_caps);
    let taint_catalog = crypto_taint_catalog(rules);
    let taint = analyze_taint_report(cpg, resolution, &taint_catalog, &config.ifds_caps);
    let mut findings = BTreeMap::<FindingKey, CryptoFinding>::new();

    let mut calls: Vec<_> = cpg
        .nodes()
        .filter(|node| matches!(node.kind, CpgNodeKind::Call { .. }))
        .map(|node| node.id)
        .collect();
    calls.sort_unstable_by_key(|&site| source_key(cpg, site));
    let call_cap = config.max_calls.min(MAX_CRYPTO_CALLS);
    let mut capped = calls.len() > call_cap;
    if capped {
        issues.insert(CryptoAnalysisIssue::CallsCapped {
            observed: calls.len(),
            cap: call_cap,
        });
        calls.truncate(call_cap);
    }

    let origin_cap = config
        .max_origin_nodes_per_argument
        .min(MAX_CRYPTO_ORIGIN_NODES);
    let mut origin_cache = BTreeMap::<(NodeId, u16), OriginEvidence>::new();
    for site in calls {
        let path = crypto_call_path(cpg, resolution, site, config.ifds_caps.max_call_depth);
        if path.capped {
            issues.insert(CryptoAnalysisIssue::CalleePathCapped { site });
        }
        let Some(path) = path.path else {
            continue;
        };

        if rules
            .weak_hash_calls
            .iter()
            .any(|pattern| pattern.matches(&path))
        {
            insert_finding(
                &mut findings,
                CryptoFinding {
                    kind: CryptoMisuseKind::WeakHash,
                    site,
                    rule: "weak-hash-call".into(),
                    argument_index: None,
                    evidence: CryptoEvidence::CallPath {
                        path: path.clone().into(),
                    },
                    capped: false,
                },
            );
        }

        inspect_literal_parameter_family(
            cpg,
            resolution,
            site,
            &path,
            &rules.weak_hash_parameters,
            &rules.weak_hash_names,
            CryptoMisuseKind::WeakHash,
            origin_cap,
            config.ifds_caps.max_call_depth,
            &mut origin_cache,
            &mut issues,
            &mut capped,
            &mut findings,
        );
        inspect_literal_parameter_family(
            cpg,
            resolution,
            site,
            &path,
            &rules.mode_parameters,
            &rules.forbidden_mode_names,
            CryptoMisuseKind::EcbMode,
            origin_cap,
            config.ifds_caps.max_call_depth,
            &mut origin_cache,
            &mut issues,
            &mut capped,
            &mut findings,
        );
        inspect_iv_parameters(
            cpg,
            resolution,
            site,
            &path,
            rules,
            origin_cap,
            config.ifds_caps.max_call_depth,
            &mut origin_cache,
            &mut issues,
            &mut capped,
            &mut findings,
        );
    }

    for flow in &taint.flows {
        if flow.source_class.as_ref() != HARDCODED_SOURCE_CLASS {
            continue;
        }
        let Some(rule_name) = flow.sink_class.strip_prefix(KEY_SINK_PREFIX) else {
            continue;
        };
        let Some(argument_index) = flow.param_index else {
            continue;
        };
        let Some(rule) = rules
            .key_parameters
            .iter()
            .find(|rule| rule.name.as_ref() == rule_name)
        else {
            continue;
        };
        if !rule.argument_indices.contains(&argument_index) {
            continue;
        }
        insert_finding(
            &mut findings,
            CryptoFinding {
                kind: CryptoMisuseKind::HardcodedKey,
                site: flow.sink,
                rule: rule.name.clone(),
                argument_index: Some(argument_index),
                evidence: CryptoEvidence::Taint {
                    source: flow.source,
                    sink: flow.sink,
                    argument_index,
                    witness: flow.witness.clone(),
                    confidence_milli: confidence_milli(flow.confidence),
                },
                capped: flow.capped,
            },
        );
    }

    for violation in &typestate.violations {
        insert_finding(
            &mut findings,
            CryptoFinding {
                kind: CryptoMisuseKind::InvalidCallOrder,
                site: violation.site,
                rule: "crypto-call-order".into(),
                argument_index: None,
                evidence: CryptoEvidence::Typestate {
                    object: violation.object,
                    from: violation.from.clone(),
                    event: violation.event.clone(),
                    witness: violation.witness.clone(),
                },
                capped: violation.capped,
            },
        );
    }

    capped |= taint.capped || typestate.capped;
    let mut findings: Vec<_> = findings.into_values().collect();
    if capped {
        for finding in &mut findings {
            finding.capped = true;
        }
    }
    CryptoAnalysis {
        findings,
        typestate,
        taint,
        issues,
        capped,
    }
}

#[allow(clippy::too_many_arguments)]
fn inspect_literal_parameter_family(
    cpg: &CodePropertyGraph,
    resolution: &CallGraphResolution,
    site: NodeId,
    path: &str,
    family: &[CryptoArgumentRule],
    forbidden: &BTreeSet<Arc<str>>,
    kind: CryptoMisuseKind,
    origin_cap: usize,
    max_call_depth: u32,
    origin_cache: &mut BTreeMap<(NodeId, u16), OriginEvidence>,
    issues: &mut BTreeSet<CryptoAnalysisIssue>,
    capped: &mut bool,
    findings: &mut BTreeMap<FindingKey, CryptoFinding>,
) {
    let arguments = call_arguments(cpg, site);
    for rule in family.iter().filter(|rule| rule.matches_call(path)) {
        for &index in &rule.argument_indices {
            let Some(&argument) = arguments.get(usize::from(index)) else {
                continue;
            };
            let origins = cached_origins(
                cpg,
                resolution,
                site,
                index,
                argument,
                origin_cap,
                max_call_depth,
                origin_cache,
                issues,
                capped,
            );
            let matching: Vec<_> = origins
                .literals
                .iter()
                .copied()
                .filter(|&origin| literal_has_segment(cpg, origin, forbidden))
                .collect();
            if matching.is_empty() {
                continue;
            }
            insert_finding(
                findings,
                CryptoFinding {
                    kind,
                    site,
                    rule: rule.name.clone(),
                    argument_index: Some(index),
                    evidence: CryptoEvidence::LiteralOrigins {
                        argument,
                        origins: matching,
                    },
                    capped: origins.capped,
                },
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn inspect_iv_parameters(
    cpg: &CodePropertyGraph,
    resolution: &CallGraphResolution,
    site: NodeId,
    path: &str,
    rules: &CryptoRuleSet,
    origin_cap: usize,
    max_call_depth: u32,
    origin_cache: &mut BTreeMap<(NodeId, u16), OriginEvidence>,
    issues: &mut BTreeSet<CryptoAnalysisIssue>,
    capped: &mut bool,
    findings: &mut BTreeMap<FindingKey, CryptoFinding>,
) {
    let arguments = call_arguments(cpg, site);
    for rule in rules
        .iv_parameters
        .iter()
        .filter(|rule| rule.matches_call(path))
    {
        for &index in &rule.argument_indices {
            let Some(&argument) = arguments.get(usize::from(index)) else {
                continue;
            };
            let origins = cached_origins(
                cpg,
                resolution,
                site,
                index,
                argument,
                origin_cap,
                max_call_depth,
                origin_cache,
                issues,
                capped,
            );
            let has_secure_random = origins.calls.values().any(|origin_path| {
                rules
                    .secure_random_sources
                    .iter()
                    .any(|pattern| pattern.matches(origin_path))
            });
            if origins.literals.is_empty() || has_secure_random {
                continue;
            }
            insert_finding(
                findings,
                CryptoFinding {
                    kind: CryptoMisuseKind::PredictableIv,
                    site,
                    rule: rule.name.clone(),
                    argument_index: Some(index),
                    evidence: CryptoEvidence::LiteralOrigins {
                        argument,
                        origins: origins.literals.iter().copied().collect(),
                    },
                    capped: origins.capped,
                },
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn cached_origins<'a>(
    cpg: &CodePropertyGraph,
    resolution: &CallGraphResolution,
    site: NodeId,
    argument_index: u16,
    argument: NodeId,
    cap: usize,
    max_call_depth: u32,
    cache: &'a mut BTreeMap<(NodeId, u16), OriginEvidence>,
    issues: &mut BTreeSet<CryptoAnalysisIssue>,
    capped: &mut bool,
) -> &'a OriginEvidence {
    let origins = cache
        .entry((site, argument_index))
        .or_insert_with(|| trace_origins(cpg, resolution, argument, cap, max_call_depth));
    if origins.capped {
        *capped = true;
        issues.insert(CryptoAnalysisIssue::OriginNodesCapped {
            site,
            argument_index,
            cap,
        });
    }
    for &path_site in &origins.path_capped {
        issues.insert(CryptoAnalysisIssue::CalleePathCapped { site: path_site });
    }
    origins
}

fn trace_origins(
    cpg: &CodePropertyGraph,
    resolution: &CallGraphResolution,
    root: NodeId,
    cap: usize,
    max_call_depth: u32,
) -> OriginEvidence {
    let mut evidence = OriginEvidence::default();
    let mut queue = VecDeque::from([root]);
    let mut visited = BTreeSet::new();
    while let Some(node) = queue.pop_front() {
        if visited.contains(&node) {
            continue;
        }
        if visited.len() >= cap {
            evidence.capped = true;
            break;
        }
        visited.insert(node);
        let Some(cpg_node) = cpg.node(node) else {
            continue;
        };
        match &cpg_node.kind {
            CpgNodeKind::Literal { .. } => {
                evidence.literals.insert(node);
            }
            CpgNodeKind::Call { .. } => {
                let path = crypto_call_path(cpg, resolution, node, max_call_depth);
                if path.capped {
                    evidence.path_capped.insert(node);
                }
                if let Some(path) = path.path {
                    evidence.calls.insert(node, path.into());
                }
                enqueue_ordered(&mut queue, call_arguments(cpg, node));
            }
            CpgNodeKind::Identifier { definition, .. } => {
                if let Some(definition) = definition {
                    queue.push_back(*definition);
                }
                enqueue_ordered(&mut queue, cpg.reaching_definitions(node));
            }
            _ => enqueue_ordered(&mut queue, cpg.ast_children(node).to_vec()),
        }
        let mut predecessors = cpg
            .incoming_edges(node)
            .filter_map(|edge| matches!(edge.kind, CpgEdgeKind::DataFlow(_)).then_some(edge.source))
            .collect::<Vec<_>>();
        predecessors.sort_unstable();
        predecessors.dedup();
        enqueue_ordered(&mut queue, predecessors);
    }
    evidence
}

fn enqueue_ordered(queue: &mut VecDeque<NodeId>, mut nodes: Vec<NodeId>) {
    nodes.sort_unstable();
    nodes.dedup();
    queue.extend(nodes);
}

struct CryptoCallPath {
    path: Option<String>,
    capped: bool,
}

fn crypto_call_path(
    cpg: &CodePropertyGraph,
    resolution: &CallGraphResolution,
    site: NodeId,
    max_depth: u32,
) -> CryptoCallPath {
    let reconstructed = call_callee_path_evidence(cpg, site, max_depth);
    if reconstructed.path.is_some() {
        return CryptoCallPath {
            path: reconstructed.path,
            capped: reconstructed.capped,
        };
    }
    CryptoCallPath {
        path: resolution
            .resolutions
            .get(&site)
            .and_then(|record| record.callee_name.clone()),
        capped: reconstructed.capped,
    }
}

fn literal_has_segment(
    cpg: &CodePropertyGraph,
    literal: NodeId,
    forbidden: &BTreeSet<Arc<str>>,
) -> bool {
    let Some(LiteralKind::String(value)) = cpg.node(literal).and_then(|node| match &node.kind {
        CpgNodeKind::Literal { kind } => Some(kind),
        _ => None,
    }) else {
        return false;
    };
    value
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|segment| !segment.is_empty())
        .map(|segment| segment.to_ascii_lowercase())
        .any(|segment| forbidden.contains(segment.as_str()))
}

fn crypto_taint_catalog(rules: &CryptoRuleSet) -> TaintCatalog {
    let mut catalog = TaintCatalog {
        literal_sources: vec![
            LiteralSourceRule {
                pattern: LiteralPattern::String,
                class: HARDCODED_SOURCE_CLASS.into(),
                cwe: Some(798),
            },
            LiteralSourceRule {
                pattern: LiteralPattern::Array,
                class: HARDCODED_SOURCE_CLASS.into(),
                cwe: Some(798),
            },
        ],
        ..TaintCatalog::empty()
    };
    for rule in &rules.key_parameters {
        let class: Arc<str> = format!("{KEY_SINK_PREFIX}{}", rule.name).into();
        for pattern in &rule.calls {
            catalog.sinks.push(CatalogRule {
                pattern: pattern.clone(),
                class: class.clone(),
                cwe: Some(798),
            });
        }
    }
    catalog
}

fn validate_rules(rules: &CryptoRuleSet) -> BTreeSet<CryptoAnalysisIssue> {
    let mut issues = BTreeSet::new();
    for (family, patterns) in [
        ("weak-hash-call", &rules.weak_hash_calls),
        ("secure-random-source", &rules.secure_random_sources),
    ] {
        for pattern in patterns {
            if pattern_is_empty(pattern) {
                issues.insert(CryptoAnalysisIssue::EmptyPattern {
                    rule: family.into(),
                });
            }
        }
    }
    for rule in rules
        .weak_hash_parameters
        .iter()
        .chain(&rules.mode_parameters)
        .chain(&rules.iv_parameters)
        .chain(&rules.key_parameters)
    {
        if rule.calls.is_empty() {
            issues.insert(CryptoAnalysisIssue::MissingCallPattern {
                rule: rule.name.clone(),
            });
        }
        if rule.argument_indices.is_empty() {
            issues.insert(CryptoAnalysisIssue::MissingArgumentIndex {
                rule: rule.name.clone(),
            });
        }
        if rule.calls.iter().any(pattern_is_empty) {
            issues.insert(CryptoAnalysisIssue::EmptyPattern {
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

fn crypto_property() -> PropertyFsm {
    let mut property = PropertyFsm::new("error")
        .with_state("ready")
        .with_state("final")
        .with_accepting("ready")
        .with_accepting("final");
    for event in [
        "Cipher::new",
        "AesGcm::new",
        "Aes128Gcm::new",
        "Aes256Gcm::new",
        "AesGcm::new_from_slice",
        "Aes128Gcm::new_from_slice",
        "Aes256Gcm::new_from_slice",
    ] {
        property = property.with_allocation(event, "ready");
    }
    for event in ["encrypt", "decrypt", "update"] {
        property = property
            .with_transition("ready", event, "ready")
            .with_transition("final", event, "error")
            .with_transition("error", event, "error");
    }
    for event in ["finalize", "doFinal"] {
        property = property
            .with_transition("ready", event, "final")
            .with_transition("final", event, "error")
            .with_transition("error", event, "error");
    }
    property
}

fn insert_finding(findings: &mut BTreeMap<FindingKey, CryptoFinding>, finding: CryptoFinding) {
    let key = FindingKey {
        site: finding.site,
        kind: finding.kind,
        argument_index: finding.argument_index,
        rule: finding.rule.clone(),
    };
    findings.entry(key).or_insert(finding);
}

fn source_key(cpg: &CodePropertyGraph, node: NodeId) -> (u32, u32, NodeId) {
    cpg.node(node)
        .map(|node| (node.range.start, node.range.end, node.id))
        .unwrap_or((u32::MAX, u32::MAX, node))
}

fn confidence_milli(confidence: f32) -> u16 {
    (confidence.clamp(0.0, 1.0) * 1_000.0).round() as u16
}

#[cfg(test)]
#[path = "crypto_tests.rs"]
mod tests;
