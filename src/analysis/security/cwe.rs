//! Declarative Common Weakness Enumeration (CWE) query templates.
//!
//! A template binds a structural VF2 pattern to source, sanitizer, and sink
//! roles plus one existing analysis primitive. The implementation never parses
//! source text: call roles consume reconstructed typed callee paths; structural
//! roles consume CPG node/edge kinds; interval, null-flow, typestate, and taint
//! evidence comes from the corresponding feature-free analysis engines.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::analysis::dataflow::expression_value_ranges;
use crate::analysis::{
    analyze_null_flows_report, analyze_taint_report, analyze_typestate, function_scope_nodes,
    Bound, CallGraphResolution, CatalogRule, IfdsCaps, Interval, NullAnalysis, NullOriginKind,
    Pattern, PropertyFsm, TaintAnalysis, TaintCatalog, TypestateAnalysis,
};
use crate::pattern::{NodeKindTag, SubgraphMatcher, Vf2Matcher};
use crate::{
    CodePropertyGraph, CpgEdgeKind, CpgNode, CpgNodeKind, DfgEdgeKind, Language, LiteralKind,
    NodeId, PropertyKey, PropertyValue, SourceRange,
};

/// Compile-time ceiling for templates in one request.
pub const MAX_CWE_TEMPLATES: usize = 256;
/// Compile-time ceiling for structural alternatives in one template.
pub const MAX_CWE_PATTERNS_PER_TEMPLATE: usize = 64;
/// Compile-time ceiling for retained VF2 embeddings per structural alternative.
pub const MAX_CWE_MATCHES_PER_PATTERN: usize = 100_000;
/// Compile-time ceiling for nodes examined while resolving an array origin.
pub const MAX_CWE_ARRAY_ORIGIN_NODES: usize = 1_000_000;

/// A source, sanitizer, or sink role in a declarative CWE template.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CweRole {
    /// Stable human-readable role name.
    pub name: Arc<str>,
    /// Typed callee-path matchers used by taint and typestate-backed templates.
    pub call_patterns: Vec<Pattern>,
    /// Typed identifier/member matchers used by structural templates.
    pub identifier_patterns: Vec<Pattern>,
    /// Operators admitted by interval-backed templates.
    pub operators: Vec<Arc<str>>,
}

impl CweRole {
    /// Creates an empty named role that callers can populate explicitly.
    pub fn new(name: impl Into<Arc<str>>) -> Self {
        Self {
            name: name.into(),
            call_patterns: Vec::new(),
            identifier_patterns: Vec::new(),
            operators: Vec::new(),
        }
    }

    /// Adds one typed callee-path matcher.
    pub fn with_call(mut self, pattern: Pattern) -> Self {
        self.call_patterns.push(pattern);
        self
    }

    /// Adds one typed identifier/member matcher.
    pub fn with_identifier(mut self, pattern: Pattern) -> Self {
        self.identifier_patterns.push(pattern);
        self
    }

    /// Adds one exact normalized operator.
    pub fn with_operator(mut self, operator: impl Into<Arc<str>>) -> Self {
        self.operators.push(operator.into());
        self
    }
}

/// One directed edge in a structural query pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CwePatternEdge {
    /// Source index in [`CweStructuralPattern::nodes`].
    pub source: usize,
    /// Target index in [`CweStructuralPattern::nodes`].
    pub target: usize,
    /// Representative typed edge. VF2 compares broad AST/CFG/DFG/call classes
    /// and exact PDG/reference/scope kinds.
    pub kind: CpgEdgeKind,
}

impl CwePatternEdge {
    /// Creates one typed structural edge.
    pub const fn new(source: usize, target: usize, kind: CpgEdgeKind) -> Self {
        Self {
            source,
            target,
            kind,
        }
    }

    /// Creates an AST-child relation.
    pub const fn ast_child(source: usize, target: usize) -> Self {
        Self::new(source, target, CpgEdgeKind::AstChild)
    }
}

/// One finite structural alternative matched by the stack-safe VF2 engine.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CweStructuralPattern {
    /// Exact CPG node-kind tags in dense pattern-index order.
    pub nodes: Vec<NodeKindTag>,
    /// Directed typed relations between pattern nodes.
    pub edges: Vec<CwePatternEdge>,
    /// Pattern node whose mapped target becomes the candidate site.
    pub sink: usize,
}

impl CweStructuralPattern {
    /// Creates a one-node sink pattern.
    pub fn single(kind: NodeKindTag) -> Self {
        Self {
            nodes: vec![kind],
            edges: Vec::new(),
            sink: 0,
        }
    }

    /// Creates a general dense pattern.
    pub fn new(nodes: Vec<NodeKindTag>, edges: Vec<CwePatternEdge>, sink: usize) -> Self {
        Self { nodes, edges, sink }
    }

    fn graph(&self) -> (CodePropertyGraph, Vec<NodeId>) {
        let mut graph = CodePropertyGraph::new(Language::Unknown);
        let mut nodes = Vec::with_capacity(self.nodes.len());
        for tag in &self.nodes {
            let kind = tag.to_cpg_node_kind(&None);
            nodes.push(graph.add_node(CpgNode::new(NodeId::new(0), kind, SourceRange::default())));
        }
        for edge in &self.edges {
            let _ = graph.connect(nodes[edge.source], nodes[edge.target], edge.kind.clone());
        }
        (graph, nodes)
    }
}

/// Existing Tier-1 primitive that decides one template.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CwePrimitive {
    /// Summary-based source-to-sink taint with sanitizer transfer.
    TaintFlow,
    /// Summary-based maybe-null flow to a typed dereference.
    NullDereference,
    /// Finite-state object protocol; only named violation events are projected.
    Typestate {
        /// Complete caller-independent finite-state property.
        property: PropertyFsm,
        /// Events whose forbidden transitions represent this CWE.
        violation_events: BTreeSet<Arc<str>>,
    },
    /// Interval result exceeds its represented fixed-width integer type.
    IntegerOverflow,
    /// Interval index is not contained in a represented array bound.
    OutOfBoundsWrite,
    /// A typed string literal reaches a credential-named structural sink.
    HardcodedCredential,
}

/// One data-driven CWE query template.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CweTemplate {
    /// Unique stable template identifier.
    pub id: Arc<str>,
    /// Common Weakness Enumeration number.
    pub cwe: u32,
    /// Human-readable candidate title.
    pub title: Arc<str>,
    /// Tier-1 semantic primitive.
    pub primitive: CwePrimitive,
    /// Evidence origin role.
    pub source: CweRole,
    /// Evidence-removing or narrowing role.
    pub sanitizer: CweRole,
    /// Security-sensitive use role.
    pub sink: CweRole,
    /// Structural alternatives that must embed before semantic projection.
    pub patterns: Vec<CweStructuralPattern>,
}

/// Resource policy for one CWE analysis request.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CweAnalysisConfig {
    /// Shared caps for taint, null-flow, and typestate engines.
    pub ifds_caps: IfdsCaps,
    /// Maximum templates accepted in this request.
    pub max_templates: usize,
    /// Maximum structural alternatives accepted per template.
    pub max_patterns_per_template: usize,
    /// Maximum complete VF2 embeddings retained per alternative.
    pub max_matches_per_pattern: usize,
    /// Maximum nodes visited to reconstruct one array origin.
    pub max_array_origin_nodes: usize,
}

impl Default for CweAnalysisConfig {
    fn default() -> Self {
        Self {
            ifds_caps: IfdsCaps::default(),
            max_templates: 128,
            max_patterns_per_template: 16,
            max_matches_per_pattern: 10_000,
            max_array_origin_nodes: 100_000,
        }
    }
}

/// Invalid request that prevents analysis from starting.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CweAnalysisError {
    /// A configured resource field is zero or exceeds its hard ceiling.
    #[error("invalid CWE analysis config field {field}: {reason}")]
    InvalidConfig {
        /// Configuration field.
        field: &'static str,
        /// Violated contract.
        reason: &'static str,
    },
    /// Two templates used the same stable identifier.
    #[error("duplicate CWE template identifier: {id}")]
    DuplicateTemplateId {
        /// Duplicated identifier.
        id: Arc<str>,
    },
    /// One template is structurally invalid.
    #[error("invalid CWE template {id}: {reason}")]
    InvalidTemplate {
        /// Invalid template identifier, or `<empty>` when absent.
        id: Arc<str>,
        /// Violated contract.
        reason: Arc<str>,
    },
}

/// Incomplete represented evidence retained by [`CweAnalysis`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CweAnalysisIssue {
    /// VF2 found more embeddings than the configured retained prefix.
    StructuralMatchCap {
        /// Template identifier.
        template: Arc<str>,
        /// Zero-based structural alternative.
        pattern: usize,
        /// First observed count beyond the cap.
        observed: usize,
        /// Configured retained count.
        cap: usize,
    },
    /// A structural site had no represented containing function.
    MissingFunction {
        /// Affected site.
        site: NodeId,
    },
    /// The shared interval solver could not analyze a containing function.
    IntervalAnalysis {
        /// Affected function.
        function: NodeId,
        /// Preserved error message.
        reason: Arc<str>,
    },
    /// An expression had no nearest represented CFG state.
    MissingIntervalContext {
        /// Affected expression.
        site: NodeId,
    },
    /// An arithmetic sink lacked a represented fixed-width integer type.
    MissingIntegerType {
        /// Affected arithmetic expression.
        site: NodeId,
    },
    /// An arithmetic sink had contradictory represented integer types.
    AmbiguousIntegerType {
        /// Affected arithmetic expression.
        site: NodeId,
        /// Distinct normalized represented type names.
        type_names: Vec<Arc<str>>,
    },
    /// A represented integer type has no exact interval-domain mapping.
    UnsupportedIntegerType {
        /// Affected arithmetic expression.
        site: NodeId,
        /// Normalized represented type name.
        type_name: Arc<str>,
    },
    /// An index-write sink had no unique represented array length.
    MissingArrayLength {
        /// Affected index access.
        site: NodeId,
    },
    /// Array-origin traversal found contradictory represented lengths.
    AmbiguousArrayLength {
        /// Affected index access.
        site: NodeId,
        /// Distinct represented lengths.
        lengths: Vec<usize>,
    },
    /// Array-origin traversal exhausted its explicit heap-work budget.
    ArrayOriginCap {
        /// Affected index access.
        site: NodeId,
        /// First unvisited node count.
        observed: usize,
        /// Configured cap.
        cap: usize,
    },
}

/// One canonical VF2 embedding retained as structural evidence.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CweStructuralMatch {
    /// Template identifier.
    pub template: Arc<str>,
    /// Zero-based structural alternative.
    pub pattern: usize,
    /// Mapped target node designated as the semantic sink.
    pub sink: NodeId,
    /// Dense pattern-index to target-node mapping.
    pub mapping: BTreeMap<usize, NodeId>,
}

/// Primitive-specific evidence for one advisory candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CweEvidence {
    /// T3.3 source-to-sink taint evidence.
    Taint {
        /// Template-specific source class.
        source_class: Arc<str>,
        /// Template-specific sink class.
        sink_class: Arc<str>,
        /// Sink argument position when known.
        param_index: Option<u16>,
    },
    /// I5/T3.3 maybe-null flow evidence.
    NullDereference {
        /// Represented null origin class.
        origin_kind: NullOriginKind,
        /// Interprocedural sink parameter when known.
        param_index: Option<u16>,
    },
    /// I3 finite-state forbidden transition.
    Typestate {
        /// Abstract object allocation site.
        object: NodeId,
        /// State immediately before the forbidden event.
        from: Arc<str>,
        /// Forbidden event.
        event: Arc<str>,
    },
    /// T1.9 interval exceeds an exact fixed-width integer domain.
    IntegerOverflow {
        /// Represented integer type.
        type_name: Arc<str>,
        /// Abstract result interval.
        value: Interval,
        /// Exact legal interval for the represented type.
        allowed: Interval,
    },
    /// T1.9 index interval is not contained in `0..length`.
    OutOfBoundsWrite {
        /// Abstract index interval.
        index: Interval,
        /// Represented array element count.
        length: usize,
    },
    /// A typed string literal reached a credential-named assignment.
    HardcodedCredential {
        /// Matched credential role without exposing the secret value.
        role: Arc<str>,
    },
}

/// One immutable advisory candidate; never a security verdict or repair.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CweCandidate {
    /// Template identifier.
    pub template: Arc<str>,
    /// Common Weakness Enumeration number.
    pub cwe: u32,
    /// Human-readable candidate title.
    pub title: Arc<str>,
    /// Security-sensitive represented site.
    pub site: NodeId,
    /// Origin site when one is represented.
    pub source: Option<NodeId>,
    /// Canonical source-to-site or structural witness.
    pub witness: Vec<NodeId>,
    /// Evidence score scaled to `0..=1000` without floating-point ordering.
    pub confidence_milli: u16,
    /// Primitive-specific evidence.
    pub evidence: CweEvidence,
}

/// Typestate report retained beside one typestate-backed template projection.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CweTypestateAnalysis {
    /// Template identifier.
    pub template: Arc<str>,
    /// Complete underlying typestate report.
    pub analysis: TypestateAnalysis,
}

/// Qualified results from one declarative CWE-template request.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CweAnalysis {
    /// Evaluated template identifiers in canonical order.
    pub templates: Vec<Arc<str>>,
    /// Retained structural embeddings in canonical order.
    pub structural_matches: Vec<CweStructuralMatch>,
    /// Advisory candidates in canonical order.
    pub candidates: Vec<CweCandidate>,
    /// Shared taint report when at least one taint template was requested.
    pub taint_analysis: Option<TaintAnalysis>,
    /// Shared maybe-null report when at least one null template was requested.
    pub null_analysis: Option<NullAnalysis>,
    /// Per-template typestate reports in template order.
    pub typestate_analyses: Vec<CweTypestateAnalysis>,
    /// Ordered CWE-layer incompleteness evidence.
    pub issues: BTreeSet<CweAnalysisIssue>,
}

impl CweAnalysis {
    /// Returns whether every requested primitive examined all represented evidence.
    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
            && self
                .taint_analysis
                .as_ref()
                .is_none_or(TaintAnalysis::is_complete)
            && self
                .null_analysis
                .as_ref()
                .is_none_or(NullAnalysis::is_complete)
            && self
                .typestate_analyses
                .iter()
                .all(|report| report.analysis.is_complete())
    }
}

/// Returns the ten feature-free templates shipped by libcpg.
pub fn builtin_cwe_templates() -> Vec<CweTemplate> {
    let input_sources = input_source_patterns();
    let generic_sanitizers = sanitizer_patterns();
    vec![
        taint_template(
            "cwe-22-path-traversal",
            22,
            "Path traversal candidate",
            input_sources.clone(),
            generic_sanitizers.clone(),
            patterns(&[
                "File::open",
                "fs::read",
                "fs::write",
                "fopen",
                "readFile",
                "sendfile",
            ]),
        ),
        taint_template(
            "cwe-78-command-injection",
            78,
            "Operating-system command injection candidate",
            input_sources.clone(),
            patterns(&["allowlist", "shell_escape", "quote", "validate"]),
            patterns(&[
                "Command::new",
                "process::Command",
                "os.system",
                "subprocess.",
                "Popen",
                "Runtime.exec",
                "child_process",
                "execve",
                "execvp",
            ]),
        ),
        taint_template(
            "cwe-79-cross-site-scripting",
            79,
            "Cross-site scripting candidate",
            input_sources.clone(),
            patterns(&["escape", "htmlspecialchars", "encode_html", "sanitize"]),
            patterns(&[
                "response.write",
                "response.send",
                "render_html",
                "innerHTML",
                "document.write",
            ]),
        ),
        taint_template(
            "cwe-89-sql-injection",
            89,
            "SQL injection candidate",
            input_sources.clone(),
            patterns(&["parameterize", ".bind", "quote", "sanitize"]),
            patterns(&[
                "sqlx::query",
                "cursor.execute",
                "executeQuery",
                "rawQuery",
                "db.query",
            ]),
        ),
        interval_overflow_template(),
        null_dereference_template(),
        use_after_free_template(),
        out_of_bounds_write_template(),
        hardcoded_credential_template(),
        taint_template(
            "cwe-502-unsafe-deserialization",
            502,
            "Unsafe deserialization candidate",
            input_sources,
            patterns(&["safe_load", "schema.validate", "verify_signature"]),
            patterns(&[
                "pickle.load",
                "yaml.load",
                "ObjectInputStream",
                "Marshal.load",
                "unserialize",
            ]),
        ),
    ]
}

/// Runs the ten shipped CWE templates.
pub fn analyze_builtin_cwes(
    cpg: &CodePropertyGraph,
    resolution: &CallGraphResolution,
    config: &CweAnalysisConfig,
) -> Result<CweAnalysis, CweAnalysisError> {
    analyze_cwe_templates(cpg, resolution, &builtin_cwe_templates(), config)
}

/// Matches declarative CWE templates and projects existing analysis evidence.
pub fn analyze_cwe_templates(
    cpg: &CodePropertyGraph,
    resolution: &CallGraphResolution,
    templates: &[CweTemplate],
    config: &CweAnalysisConfig,
) -> Result<CweAnalysis, CweAnalysisError> {
    validate_request(templates, config)?;
    let ordered = templates
        .iter()
        .map(|template| (Arc::clone(&template.id), template))
        .collect::<BTreeMap<_, _>>();
    let mut issues = BTreeSet::new();
    let (structural_matches, structural_sites) =
        match_structures(cpg, &ordered, config, &mut issues);
    let mut candidates = BTreeMap::<CandidateKey, CweCandidate>::new();

    let (taint_catalog, taint_classes) = build_taint_catalog(&ordered);
    let taint_analysis = (!taint_classes.is_empty()).then(|| {
        let report = analyze_taint_report(cpg, resolution, &taint_catalog, &config.ifds_caps);
        for flow in &report.flows {
            let Some(template) = taint_classes.get(&flow.sink_class) else {
                continue;
            };
            if !structural_sites
                .get(&template.id)
                .is_some_and(|sites| sites.contains(&flow.sink))
            {
                continue;
            }
            insert_candidate(
                &mut candidates,
                CweCandidate {
                    template: Arc::clone(&template.id),
                    cwe: template.cwe,
                    title: Arc::clone(&template.title),
                    site: flow.sink,
                    source: Some(flow.source),
                    witness: flow.witness.clone(),
                    confidence_milli: confidence_milli(flow.confidence),
                    evidence: CweEvidence::Taint {
                        source_class: Arc::clone(&flow.source_class),
                        sink_class: Arc::clone(&flow.sink_class),
                        param_index: flow.param_index,
                    },
                },
            );
        }
        report
    });

    let null_templates = ordered
        .values()
        .copied()
        .filter(|template| matches!(template.primitive, CwePrimitive::NullDereference))
        .collect::<Vec<_>>();
    let null_analysis = (!null_templates.is_empty()).then(|| {
        let report = analyze_null_flows_report(cpg, resolution, &config.ifds_caps);
        for template in &null_templates {
            let sites = structural_sites.get(&template.id);
            for flow in &report.flows {
                if !sites.is_some_and(|sites| sites.contains(&flow.sink)) {
                    continue;
                }
                insert_candidate(
                    &mut candidates,
                    CweCandidate {
                        template: Arc::clone(&template.id),
                        cwe: template.cwe,
                        title: Arc::clone(&template.title),
                        site: flow.sink,
                        source: Some(flow.origin),
                        witness: flow.witness.clone(),
                        confidence_milli: confidence_milli(flow.confidence),
                        evidence: CweEvidence::NullDereference {
                            origin_kind: flow.origin_kind,
                            param_index: flow.param_index,
                        },
                    },
                );
            }
        }
        report
    });

    let mut typestate_analyses = Vec::new();
    for template in ordered.values().copied() {
        let CwePrimitive::Typestate {
            property,
            violation_events,
        } = &template.primitive
        else {
            continue;
        };
        let analysis = analyze_typestate(cpg, property, &config.ifds_caps);
        let sites = structural_sites.get(&template.id);
        for violation in &analysis.violations {
            if !violation_events.contains(&violation.event)
                || !sites.is_some_and(|sites| sites.contains(&violation.site))
            {
                continue;
            }
            insert_candidate(
                &mut candidates,
                CweCandidate {
                    template: Arc::clone(&template.id),
                    cwe: template.cwe,
                    title: Arc::clone(&template.title),
                    site: violation.site,
                    source: Some(violation.object),
                    witness: violation.witness.clone(),
                    confidence_milli: 1_000,
                    evidence: CweEvidence::Typestate {
                        object: violation.object,
                        from: Arc::clone(&violation.from),
                        event: Arc::clone(&violation.event),
                    },
                },
            );
        }
        typestate_analyses.push(CweTypestateAnalysis {
            template: Arc::clone(&template.id),
            analysis,
        });
    }

    let owners = node_owners(cpg);
    let mut requests = BTreeMap::<NodeId, BTreeSet<NodeId>>::new();
    let mut index_expressions = BTreeMap::<(Arc<str>, NodeId), NodeId>::new();
    for template in ordered.values().copied() {
        let Some(sites) = structural_sites.get(&template.id) else {
            continue;
        };
        match template.primitive {
            CwePrimitive::IntegerOverflow => {
                for &site in sites {
                    if !operator_matches(cpg, site, &template.sink) {
                        continue;
                    }
                    let Some(&function) = owners.get(&site) else {
                        issues.insert(CweAnalysisIssue::MissingFunction { site });
                        continue;
                    };
                    requests.entry(function).or_default().insert(site);
                }
            }
            CwePrimitive::OutOfBoundsWrite => {
                for &site in sites {
                    let Some(index) = cpg.ast_children(site).get(1).copied() else {
                        issues.insert(CweAnalysisIssue::MissingIntervalContext { site });
                        continue;
                    };
                    let Some(&function) = owners.get(&site) else {
                        issues.insert(CweAnalysisIssue::MissingFunction { site });
                        continue;
                    };
                    requests.entry(function).or_default().insert(index);
                    index_expressions.insert((Arc::clone(&template.id), site), index);
                }
            }
            _ => {}
        }
    }
    let mut expression_values = BTreeMap::new();
    for (function, expressions) in requests {
        match expression_value_ranges(cpg, function, &expressions) {
            Ok(values) => expression_values.extend(values),
            Err(error) => {
                issues.insert(CweAnalysisIssue::IntervalAnalysis {
                    function,
                    reason: error.to_string().into(),
                });
            }
        }
    }

    for template in ordered.values().copied() {
        let Some(sites) = structural_sites.get(&template.id) else {
            continue;
        };
        match template.primitive {
            CwePrimitive::IntegerOverflow => {
                project_integer_overflow(
                    cpg,
                    template,
                    sites,
                    &expression_values,
                    &mut issues,
                    &mut candidates,
                );
            }
            CwePrimitive::OutOfBoundsWrite => {
                project_out_of_bounds(
                    cpg,
                    template,
                    sites,
                    &index_expressions,
                    &expression_values,
                    config,
                    &mut issues,
                    &mut candidates,
                );
            }
            CwePrimitive::HardcodedCredential => {
                project_hardcoded_credentials(cpg, template, sites, &mut candidates);
            }
            _ => {}
        }
    }

    Ok(CweAnalysis {
        templates: ordered.keys().cloned().collect(),
        structural_matches,
        candidates: candidates.into_values().collect(),
        taint_analysis,
        null_analysis,
        typestate_analyses,
        issues,
    })
}

fn validate_request(
    templates: &[CweTemplate],
    config: &CweAnalysisConfig,
) -> Result<(), CweAnalysisError> {
    validate_cap("max_templates", config.max_templates, MAX_CWE_TEMPLATES)?;
    validate_cap(
        "max_patterns_per_template",
        config.max_patterns_per_template,
        MAX_CWE_PATTERNS_PER_TEMPLATE,
    )?;
    validate_cap(
        "max_matches_per_pattern",
        config.max_matches_per_pattern,
        MAX_CWE_MATCHES_PER_PATTERN,
    )?;
    validate_cap(
        "max_array_origin_nodes",
        config.max_array_origin_nodes,
        MAX_CWE_ARRAY_ORIGIN_NODES,
    )?;
    if templates.len() > config.max_templates {
        return Err(CweAnalysisError::InvalidConfig {
            field: "templates",
            reason: "template count exceeds max_templates",
        });
    }
    let mut ids = BTreeSet::new();
    for template in templates {
        let shown_id: Arc<str> = if template.id.is_empty() {
            "<empty>".into()
        } else {
            Arc::clone(&template.id)
        };
        if template.id.is_empty() {
            return invalid_template(shown_id, "identifier must be non-empty");
        }
        if !ids.insert(Arc::clone(&template.id)) {
            return Err(CweAnalysisError::DuplicateTemplateId {
                id: Arc::clone(&template.id),
            });
        }
        if template.cwe == 0 {
            return invalid_template(shown_id, "CWE number must be positive");
        }
        if template.title.is_empty()
            || template.source.name.is_empty()
            || template.sanitizer.name.is_empty()
            || template.sink.name.is_empty()
        {
            return invalid_template(shown_id, "title and role names must be non-empty");
        }
        if template.patterns.is_empty() {
            return invalid_template(shown_id, "at least one structural pattern is required");
        }
        if template.patterns.len() > config.max_patterns_per_template {
            return invalid_template(shown_id, "structural pattern count exceeds configured cap");
        }
        for pattern in &template.patterns {
            if pattern.nodes.is_empty() || pattern.sink >= pattern.nodes.len() {
                return invalid_template(
                    shown_id,
                    "pattern nodes/sink are not a dense valid shape",
                );
            }
            if pattern.edges.iter().any(|edge| {
                edge.source >= pattern.nodes.len() || edge.target >= pattern.nodes.len()
            }) {
                return invalid_template(shown_id, "pattern edge endpoint is out of range");
            }
        }
        if matches!(template.primitive, CwePrimitive::TaintFlow)
            && (template.source.call_patterns.is_empty() || template.sink.call_patterns.is_empty())
        {
            return invalid_template(shown_id, "taint templates require source and sink calls");
        }
        if let CwePrimitive::Typestate {
            property,
            violation_events,
        } = &template.primitive
        {
            if !property.validate().is_empty() || violation_events.is_empty() {
                return invalid_template(
                    shown_id,
                    "typestate property must be valid with projected violation events",
                );
            }
        }
    }
    Ok(())
}

fn validate_cap(field: &'static str, value: usize, hard: usize) -> Result<(), CweAnalysisError> {
    if value == 0 {
        Err(CweAnalysisError::InvalidConfig {
            field,
            reason: "value must be positive",
        })
    } else if value > hard {
        Err(CweAnalysisError::InvalidConfig {
            field,
            reason: "value exceeds the compile-time hard ceiling",
        })
    } else {
        Ok(())
    }
}

fn invalid_template<T>(id: Arc<str>, reason: &str) -> Result<T, CweAnalysisError> {
    Err(CweAnalysisError::InvalidTemplate {
        id,
        reason: reason.into(),
    })
}

fn match_structures(
    cpg: &CodePropertyGraph,
    templates: &BTreeMap<Arc<str>, &CweTemplate>,
    config: &CweAnalysisConfig,
    issues: &mut BTreeSet<CweAnalysisIssue>,
) -> (
    Vec<CweStructuralMatch>,
    BTreeMap<Arc<str>, BTreeSet<NodeId>>,
) {
    let matcher = Vf2Matcher::new()
        .with_strict_kinds(true)
        .with_strict_edges(false)
        .with_max_matches(config.max_matches_per_pattern + 1);
    let mut retained = BTreeSet::new();
    let mut sites = BTreeMap::<Arc<str>, BTreeSet<NodeId>>::new();
    for template in templates.values().copied() {
        for (pattern_index, pattern) in template.patterns.iter().enumerate() {
            let (graph, pattern_nodes) = pattern.graph();
            let mut matches = matcher.find_matches(&graph, cpg);
            if matches.len() > config.max_matches_per_pattern {
                issues.insert(CweAnalysisIssue::StructuralMatchCap {
                    template: Arc::clone(&template.id),
                    pattern: pattern_index,
                    observed: matches.len(),
                    cap: config.max_matches_per_pattern,
                });
                matches.truncate(config.max_matches_per_pattern);
            }
            for embedding in matches {
                let mapping = pattern_nodes
                    .iter()
                    .enumerate()
                    .filter_map(|(index, node)| {
                        embedding
                            .node_mapping
                            .get(node)
                            .copied()
                            .map(|target| (index, target))
                    })
                    .collect::<BTreeMap<_, _>>();
                let Some(&sink) = mapping.get(&pattern.sink) else {
                    continue;
                };
                sites
                    .entry(Arc::clone(&template.id))
                    .or_default()
                    .insert(sink);
                retained.insert(CweStructuralMatch {
                    template: Arc::clone(&template.id),
                    pattern: pattern_index,
                    sink,
                    mapping,
                });
            }
        }
    }
    (retained.into_iter().collect(), sites)
}

fn build_taint_catalog<'a>(
    templates: &BTreeMap<Arc<str>, &'a CweTemplate>,
) -> (TaintCatalog, BTreeMap<Arc<str>, &'a CweTemplate>) {
    let mut catalog = TaintCatalog::empty();
    let mut classes = BTreeMap::new();
    for template in templates.values().copied() {
        if !matches!(template.primitive, CwePrimitive::TaintFlow) {
            continue;
        }
        let source_class: Arc<str> = format!("{}:source", template.id).into();
        let sink_class: Arc<str> = format!("{}:sink", template.id).into();
        for pattern in &template.source.call_patterns {
            catalog.sources.push(CatalogRule {
                pattern: pattern.clone(),
                class: Arc::clone(&source_class),
                cwe: None,
            });
        }
        for pattern in &template.sanitizer.call_patterns {
            catalog.sanitizers.push(CatalogRule {
                pattern: pattern.clone(),
                class: format!("{}:sanitizer", template.id).into(),
                cwe: None,
            });
        }
        for pattern in &template.sink.call_patterns {
            catalog.sinks.push(CatalogRule {
                pattern: pattern.clone(),
                class: Arc::clone(&sink_class),
                cwe: Some(template.cwe),
            });
        }
        classes.insert(sink_class, template);
    }
    (catalog, classes)
}

fn node_owners(cpg: &CodePropertyGraph) -> BTreeMap<NodeId, NodeId> {
    let functions = cpg
        .functions()
        .map(|function| function.id)
        .collect::<BTreeSet<_>>();
    let mut owners = BTreeMap::new();
    for function in functions {
        let scope = function_scope_nodes(cpg, function)
            .into_iter()
            .collect::<BTreeSet<_>>();
        for node in scope {
            owners.entry(node).or_insert(function);
        }
    }
    owners
}

fn project_integer_overflow(
    cpg: &CodePropertyGraph,
    template: &CweTemplate,
    sites: &BTreeSet<NodeId>,
    values: &BTreeMap<NodeId, Interval>,
    issues: &mut BTreeSet<CweAnalysisIssue>,
    candidates: &mut BTreeMap<CandidateKey, CweCandidate>,
) {
    for &site in sites {
        if !operator_matches(cpg, site, &template.sink) {
            continue;
        }
        let Some(&value) = values.get(&site) else {
            issues.insert(CweAnalysisIssue::MissingIntervalContext { site });
            continue;
        };
        let type_name = match expression_integer_type(cpg, site) {
            ExpressionIntegerType::Unique(type_name) => type_name,
            ExpressionIntegerType::Missing => {
                issues.insert(CweAnalysisIssue::MissingIntegerType { site });
                continue;
            }
            ExpressionIntegerType::Ambiguous(type_names) => {
                issues.insert(CweAnalysisIssue::AmbiguousIntegerType { site, type_names });
                continue;
            }
        };
        let Some(allowed) = integer_domain(&type_name) else {
            issues.insert(CweAnalysisIssue::UnsupportedIntegerType { site, type_name });
            continue;
        };
        if value.is_subset_of(allowed) {
            continue;
        }
        insert_candidate(
            candidates,
            CweCandidate {
                template: Arc::clone(&template.id),
                cwe: template.cwe,
                title: Arc::clone(&template.title),
                site,
                source: None,
                witness: vec![site],
                confidence_milli: interval_confidence(value),
                evidence: CweEvidence::IntegerOverflow {
                    type_name,
                    value,
                    allowed,
                },
            },
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn project_out_of_bounds(
    cpg: &CodePropertyGraph,
    template: &CweTemplate,
    sites: &BTreeSet<NodeId>,
    index_expressions: &BTreeMap<(Arc<str>, NodeId), NodeId>,
    values: &BTreeMap<NodeId, Interval>,
    config: &CweAnalysisConfig,
    issues: &mut BTreeSet<CweAnalysisIssue>,
    candidates: &mut BTreeMap<CandidateKey, CweCandidate>,
) {
    for &site in sites {
        let Some(&index_expression) = index_expressions.get(&(Arc::clone(&template.id), site))
        else {
            continue;
        };
        let Some(&index) = values.get(&index_expression) else {
            issues.insert(CweAnalysisIssue::MissingIntervalContext {
                site: index_expression,
            });
            continue;
        };
        let length = match represented_array_length(cpg, site, config.max_array_origin_nodes) {
            ArrayLength::One(length) => length,
            ArrayLength::Missing => {
                issues.insert(CweAnalysisIssue::MissingArrayLength { site });
                continue;
            }
            ArrayLength::Ambiguous(lengths) => {
                issues.insert(CweAnalysisIssue::AmbiguousArrayLength { site, lengths });
                continue;
            }
            ArrayLength::Capped(observed) => {
                issues.insert(CweAnalysisIssue::ArrayOriginCap {
                    site,
                    observed,
                    cap: config.max_array_origin_nodes,
                });
                continue;
            }
        };
        let in_bounds = length.checked_sub(1).and_then(|last| {
            i128::try_from(last)
                .ok()
                .and_then(|last| Interval::new(Bound::Finite(0), Bound::Finite(last)))
        });
        if in_bounds.is_some_and(|allowed| index.is_subset_of(allowed)) {
            continue;
        }
        insert_candidate(
            candidates,
            CweCandidate {
                template: Arc::clone(&template.id),
                cwe: template.cwe,
                title: Arc::clone(&template.title),
                site,
                source: Some(index_expression),
                witness: vec![index_expression, site],
                confidence_milli: interval_confidence(index),
                evidence: CweEvidence::OutOfBoundsWrite { index, length },
            },
        );
    }
}

fn project_hardcoded_credentials(
    cpg: &CodePropertyGraph,
    template: &CweTemplate,
    sites: &BTreeSet<NodeId>,
    candidates: &mut BTreeMap<CandidateKey, CweCandidate>,
) {
    for &site in sites {
        let children = cpg.ast_children(site);
        let role = children.iter().find_map(|child| {
            let name = cpg.node(*child)?.name()?;
            template
                .sink
                .identifier_patterns
                .iter()
                .find(|pattern| pattern.matches(name))
                .map(|_| Arc::<str>::from(name))
        });
        let literal = children.iter().find_map(|child| {
            cpg.node(*child).and_then(|node| {
                matches!(
                    node.kind,
                    CpgNodeKind::Literal {
                        kind: LiteralKind::String(_)
                    }
                )
                .then_some(*child)
            })
        });
        let (Some(role), Some(literal)) = (role, literal) else {
            continue;
        };
        insert_candidate(
            candidates,
            CweCandidate {
                template: Arc::clone(&template.id),
                cwe: template.cwe,
                title: Arc::clone(&template.title),
                site,
                source: Some(literal),
                witness: vec![literal, site],
                confidence_milli: 1_000,
                evidence: CweEvidence::HardcodedCredential { role },
            },
        );
    }
}

fn operator_matches(cpg: &CodePropertyGraph, site: NodeId, role: &CweRole) -> bool {
    let Some(CpgNodeKind::BinaryOp { operator }) = cpg.node(site).map(|node| &node.kind) else {
        return false;
    };
    role.operators.is_empty() || role.operators.iter().any(|allowed| allowed == operator)
}

enum ExpressionIntegerType {
    Missing,
    Unique(Arc<str>),
    Ambiguous(Vec<Arc<str>>),
}

fn expression_integer_type(cpg: &CodePropertyGraph, expression: NodeId) -> ExpressionIntegerType {
    let parents = ast_parents(cpg, expression);
    let mut names = BTreeSet::new();
    if let Some(name) = cpg.node(expression).and_then(property_type) {
        names.insert(name);
    }
    for parent in parents {
        let Some(node) = cpg.node(parent) else {
            continue;
        };
        if let Some(name) = declared_type(node) {
            names.insert(name);
        }
        if matches!(node.kind, CpgNodeKind::Assignment { .. }) {
            if let Some(target) = cpg.ast_children(parent).first().copied() {
                if let Some(name) = resolved_node_type(cpg, target) {
                    names.insert(name);
                }
            }
        }
    }
    match names.len() {
        0 => ExpressionIntegerType::Missing,
        1 => ExpressionIntegerType::Unique(
            names
                .into_iter()
                .next()
                .expect("one represented type name exists"),
        ),
        _ => ExpressionIntegerType::Ambiguous(names.into_iter().collect()),
    }
}

fn resolved_node_type(cpg: &CodePropertyGraph, node: NodeId) -> Option<Arc<str>> {
    let node = cpg.node(node)?;
    declared_type(node).or_else(|| match &node.kind {
        CpgNodeKind::Identifier {
            definition: Some(definition),
            ..
        } => cpg.node(*definition).and_then(declared_type),
        _ => None,
    })
}

fn declared_type(node: &CpgNode) -> Option<Arc<str>> {
    match &node.kind {
        CpgNodeKind::Variable {
            var_type: Some(info),
            ..
        }
        | CpgNodeKind::Parameter {
            param_type: Some(info),
            ..
        }
        | CpgNodeKind::TypeAnnotation { type_info: info } => Some(Arc::clone(&info.name)),
        _ => property_type(node),
    }
}

fn property_type(node: &CpgNode) -> Option<Arc<str>> {
    node.properties
        .get(&PropertyKey::Type)
        .and_then(PropertyValue::as_str)
        .map(Arc::from)
}

fn integer_domain(type_name: &str) -> Option<Interval> {
    let (minimum, maximum) = match type_name {
        "i8" | "int8" | "int8_t" | "byte" => (i8::MIN.into(), i8::MAX.into()),
        "u8" | "uint8" | "uint8_t" => (0, u8::MAX.into()),
        "i16" | "int16" | "int16_t" | "short" => (i16::MIN.into(), i16::MAX.into()),
        "u16" | "uint16" | "uint16_t" => (0, u16::MAX.into()),
        "i32" | "int32" | "int32_t" | "int" => (i32::MIN.into(), i32::MAX.into()),
        "u32" | "uint32" | "uint32_t" => (0, u32::MAX.into()),
        "i64" | "int64" | "int64_t" | "long" => (i64::MIN.into(), i64::MAX.into()),
        "u64" | "uint64" | "uint64_t" => (0, u64::MAX.into()),
        "i128" | "int128" => (i128::MIN, i128::MAX),
        _ => return None,
    };
    Interval::new(Bound::Finite(minimum), Bound::Finite(maximum))
}

enum ArrayLength {
    One(usize),
    Missing,
    Ambiguous(Vec<usize>),
    Capped(usize),
}

fn represented_array_length(
    cpg: &CodePropertyGraph,
    index_access: NodeId,
    cap: usize,
) -> ArrayLength {
    let Some(base) = cpg.ast_children(index_access).first().copied() else {
        return ArrayLength::Missing;
    };
    let mut queue = VecDeque::from([base]);
    let mut visited = BTreeSet::new();
    let mut lengths = BTreeSet::new();
    while let Some(node) = queue.pop_front() {
        if !visited.insert(node) {
            continue;
        }
        if visited.len() > cap {
            return ArrayLength::Capped(visited.len());
        }
        let Some(value) = cpg.node(node) else {
            continue;
        };
        if matches!(
            value.kind,
            CpgNodeKind::Literal {
                kind: LiteralKind::Array
            }
        ) {
            lengths.insert(cpg.ast_children(node).len());
            continue;
        }
        match &value.kind {
            CpgNodeKind::Identifier {
                definition: Some(definition),
                ..
            } => queue.push_back(*definition),
            CpgNodeKind::Variable { .. } | CpgNodeKind::Assignment { .. } => {
                for child in cpg.ast_children(node) {
                    queue.push_back(child);
                }
            }
            _ => {}
        }
        for edge in cpg.outgoing_edges(node) {
            if matches!(
                edge.kind,
                CpgEdgeKind::Reference
                    | CpgEdgeKind::Definition
                    | CpgEdgeKind::DataFlow(DfgEdgeKind::UseDef)
                    | CpgEdgeKind::DataFlow(DfgEdgeKind::ReachingDef)
            ) {
                queue.push_back(edge.target);
            }
        }
    }
    match lengths.len() {
        0 => ArrayLength::Missing,
        1 => ArrayLength::One(*lengths.first().expect("one represented array length")),
        _ => ArrayLength::Ambiguous(lengths.into_iter().collect()),
    }
}

fn ast_parents(cpg: &CodePropertyGraph, node: NodeId) -> BTreeSet<NodeId> {
    let mut parents = BTreeSet::new();
    if let Some(parent) = cpg.ast_parent(node) {
        parents.insert(parent);
    }
    for edge in cpg.incoming_edges(node) {
        if matches!(edge.kind, CpgEdgeKind::AstChild) {
            parents.insert(edge.source);
        }
    }
    for edge in cpg.outgoing_edges(node) {
        if matches!(edge.kind, CpgEdgeKind::AstParent) {
            parents.insert(edge.target);
        }
    }
    parents
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct CandidateKey {
    template: Arc<str>,
    cwe: u32,
    site: NodeId,
    source: Option<NodeId>,
    evidence: u8,
    witness: Vec<NodeId>,
}

fn insert_candidate(
    candidates: &mut BTreeMap<CandidateKey, CweCandidate>,
    candidate: CweCandidate,
) {
    let key = CandidateKey {
        template: Arc::clone(&candidate.template),
        cwe: candidate.cwe,
        site: candidate.site,
        source: candidate.source,
        evidence: evidence_rank(&candidate.evidence),
        witness: candidate.witness.clone(),
    };
    candidates
        .entry(key)
        .and_modify(|current| {
            if candidate.confidence_milli > current.confidence_milli {
                current.confidence_milli = candidate.confidence_milli;
            }
        })
        .or_insert(candidate);
}

const fn evidence_rank(evidence: &CweEvidence) -> u8 {
    match evidence {
        CweEvidence::Taint { .. } => 0,
        CweEvidence::NullDereference { .. } => 1,
        CweEvidence::Typestate { .. } => 2,
        CweEvidence::IntegerOverflow { .. } => 3,
        CweEvidence::OutOfBoundsWrite { .. } => 4,
        CweEvidence::HardcodedCredential { .. } => 5,
    }
}

fn confidence_milli(confidence: f32) -> u16 {
    if !confidence.is_finite() {
        return 0;
    }
    (confidence.clamp(0.0, 1.0) * 1_000.0).round() as u16
}

fn interval_confidence(interval: Interval) -> u16 {
    if matches!(interval.lo, Bound::Finite(_)) && matches!(interval.hi, Bound::Finite(_)) {
        1_000
    } else {
        700
    }
}

fn patterns(values: &[&str]) -> Vec<Pattern> {
    values
        .iter()
        .map(|value| Pattern::Substring(Arc::from(*value)))
        .collect()
}

fn input_source_patterns() -> Vec<Pattern> {
    patterns(&[
        "env::args",
        "sys.argv",
        "process.argv",
        "os.Args",
        "io::stdin",
        "sys.stdin",
        "read_line",
        "readLine",
        "req.body",
        "req.query",
        "req.params",
        "request.body",
        "request.form",
        "request.args",
        "query_string",
        "form_data",
        "FormData",
    ])
}

fn sanitizer_patterns() -> Vec<Pattern> {
    patterns(&[
        "escape",
        "sanitize",
        "quote",
        "parameterize",
        "canonicalize",
        "validate",
        ".bind",
    ])
}

fn taint_template(
    id: &str,
    cwe: u32,
    title: &str,
    sources: Vec<Pattern>,
    sanitizers: Vec<Pattern>,
    sinks: Vec<Pattern>,
) -> CweTemplate {
    CweTemplate {
        id: id.into(),
        cwe,
        title: title.into(),
        primitive: CwePrimitive::TaintFlow,
        source: CweRole {
            name: "attacker-controlled value".into(),
            call_patterns: sources,
            identifier_patterns: Vec::new(),
            operators: Vec::new(),
        },
        sanitizer: CweRole {
            name: "template sanitizer".into(),
            call_patterns: sanitizers,
            identifier_patterns: Vec::new(),
            operators: Vec::new(),
        },
        sink: CweRole {
            name: "security-sensitive call".into(),
            call_patterns: sinks,
            identifier_patterns: Vec::new(),
            operators: Vec::new(),
        },
        patterns: vec![CweStructuralPattern::single(NodeKindTag::Call)],
    }
}

fn interval_overflow_template() -> CweTemplate {
    CweTemplate {
        id: "cwe-190-integer-overflow".into(),
        cwe: 190,
        title: "Integer overflow candidate".into(),
        primitive: CwePrimitive::IntegerOverflow,
        source: CweRole::new("integer interval operands"),
        sanitizer: CweRole::new("represented interval narrowing")
            .with_call(Pattern::Substring("checked_".into()))
            .with_call(Pattern::Substring("saturating_".into())),
        sink: CweRole::new("fixed-width arithmetic")
            .with_operator("+")
            .with_operator("-")
            .with_operator("*"),
        patterns: vec![
            CweStructuralPattern::new(
                vec![NodeKindTag::Variable, NodeKindTag::BinaryOp],
                vec![CwePatternEdge::ast_child(0, 1)],
                1,
            ),
            CweStructuralPattern::new(
                vec![NodeKindTag::Assignment, NodeKindTag::BinaryOp],
                vec![CwePatternEdge::ast_child(0, 1)],
                1,
            ),
        ],
    }
}

fn null_dereference_template() -> CweTemplate {
    CweTemplate {
        id: "cwe-476-null-dereference".into(),
        cwe: 476,
        title: "Null-pointer dereference candidate".into(),
        primitive: CwePrimitive::NullDereference,
        source: CweRole::new("maybe-null value"),
        sanitizer: CweRole::new("represented non-null refinement"),
        sink: CweRole::new("typed dereference"),
        patterns: [
            NodeKindTag::UnaryOp,
            NodeKindTag::MemberAccess,
            NodeKindTag::IndexAccess,
            NodeKindTag::Call,
        ]
        .into_iter()
        .map(CweStructuralPattern::single)
        .collect(),
    }
}

fn use_after_free_template() -> CweTemplate {
    let property = PropertyFsm::new("error")
        .with_allocation("alloc", "live")
        .with_allocation("malloc", "live")
        .with_allocation("new", "live")
        .with_event("free")
        .with_event("drop")
        .with_event("use")
        .with_event("read")
        .with_event("write")
        .with_event("deref")
        .with_transition("live", "free", "freed")
        .with_transition("live", "drop", "freed")
        .with_transition("live", "use", "live")
        .with_transition("live", "read", "live")
        .with_transition("live", "write", "live")
        .with_transition("live", "deref", "live")
        .with_accepting("live")
        .with_accepting("freed");
    CweTemplate {
        id: "cwe-416-use-after-free".into(),
        cwe: 416,
        title: "Use-after-free candidate".into(),
        primitive: CwePrimitive::Typestate {
            property,
            violation_events: ["use", "read", "write", "deref"]
                .into_iter()
                .map(Arc::from)
                .collect(),
        },
        source: CweRole::new("allocation")
            .with_call(Pattern::Segment("alloc".into()))
            .with_call(Pattern::Segment("malloc".into()))
            .with_call(Pattern::Segment("new".into())),
        sanitizer: CweRole::new("deallocation")
            .with_call(Pattern::Segment("free".into()))
            .with_call(Pattern::Segment("drop".into())),
        sink: CweRole::new("post-free object use")
            .with_call(Pattern::Segment("use".into()))
            .with_call(Pattern::Segment("read".into()))
            .with_call(Pattern::Segment("write".into()))
            .with_call(Pattern::Segment("deref".into())),
        patterns: vec![CweStructuralPattern::single(NodeKindTag::Call)],
    }
}

fn out_of_bounds_write_template() -> CweTemplate {
    CweTemplate {
        id: "cwe-787-out-of-bounds-write".into(),
        cwe: 787,
        title: "Out-of-bounds write candidate".into(),
        primitive: CwePrimitive::OutOfBoundsWrite,
        source: CweRole::new("index interval"),
        sanitizer: CweRole::new("represented bounds refinement"),
        sink: CweRole::new("array index write"),
        patterns: vec![CweStructuralPattern::new(
            vec![
                NodeKindTag::Assignment,
                NodeKindTag::IndexAccess,
                NodeKindTag::Literal,
            ],
            vec![
                CwePatternEdge::ast_child(0, 1),
                CwePatternEdge::ast_child(1, 2),
            ],
            1,
        )],
    }
}

fn hardcoded_credential_template() -> CweTemplate {
    CweTemplate {
        id: "cwe-798-hardcoded-credentials".into(),
        cwe: 798,
        title: "Hardcoded credential candidate".into(),
        primitive: CwePrimitive::HardcodedCredential,
        source: CweRole::new("typed string literal"),
        sanitizer: CweRole::new("external secret provider")
            .with_call(Pattern::Substring("env".into()))
            .with_call(Pattern::Substring("secret_manager".into())),
        sink: CweRole::new("credential-named assignment")
            .with_identifier(Pattern::Substring("password".into()))
            .with_identifier(Pattern::Substring("passwd".into()))
            .with_identifier(Pattern::Substring("secret".into()))
            .with_identifier(Pattern::Substring("api_key".into()))
            .with_identifier(Pattern::Substring("token".into())),
        patterns: vec![CweStructuralPattern::new(
            vec![NodeKindTag::Assignment, NodeKindTag::Literal],
            vec![CwePatternEdge::ast_child(0, 1)],
            0,
        )],
    }
}

#[cfg(test)]
#[path = "cwe_tests.rs"]
mod tests;
