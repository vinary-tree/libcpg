//! Population-based missing-check detection over taint, PDG, and call-graph evidence.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::{CodePropertyGraph, CpgEdgeKind, CpgNodeKind, LiteralKind, NodeId};

use super::super::TaintCatalog;
use super::super::{call_graph_projection, AnalysisError};
use super::evidence::{
    call_evidence, call_owners, is_guard, ApiIdentity, CallEvidenceError, PopulationFraction,
    PopulationThreshold,
};

/// Population from which peer checks are inferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum PeerScope {
    /// Every use of the same sensitive API is comparable.
    WholeProgram,
    /// Compare callers in an undirected call-graph neighborhood.
    CallGraph {
        /// Maximum number of caller/callee edges between peer functions.
        max_distance: usize,
    },
}

impl Default for PeerScope {
    fn default() -> Self {
        Self::CallGraph { max_distance: 2 }
    }
}

/// Configuration for [`analyze_missing_checks`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MissingCheckConfig {
    /// Data-driven taint catalog whose sink rules identify sensitive APIs.
    pub catalog: TaintCatalog,
    /// Sink classes to analyze; an empty set selects every sink class.
    pub sensitive_classes: BTreeSet<Arc<str>>,
    /// Minimum exact fraction of comparable sites carrying a check.
    pub minimum_support: PopulationThreshold,
    /// Minimum number of comparable sites required to infer a check.
    pub minimum_population: usize,
    /// Population topology used to select peers.
    pub peer_scope: PeerScope,
    /// Maximum AST nodes in one normalized check fingerprint.
    pub max_check_nodes: usize,
    /// Maximum typed AST depth used to reconstruct a callee path.
    pub max_callee_depth: u32,
}

impl Default for MissingCheckConfig {
    fn default() -> Self {
        Self {
            catalog: TaintCatalog::builtin_generic(),
            sensitive_classes: BTreeSet::new(),
            minimum_support: PopulationThreshold::new(4, 5),
            minimum_population: 2,
            peer_scope: PeerScope::default(),
            max_check_nodes: 100_000,
            max_callee_depth: 256,
        }
    }
}

/// One node in a normalized, flat, postorder check fingerprint.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CheckFingerprintNode {
    /// Normalized semantic node kind.
    pub kind: CheckNodeKind,
    /// Number of direct AST children represented immediately before this node.
    pub child_count: u32,
}

/// Normalized semantics retained in a check fingerprint.
///
/// Local declaration and identifier names are intentionally abstracted, while
/// operators, API identities, members, types, literals, and language-specific
/// opaque kinds remain exact.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum CheckNodeKind {
    /// Compilation-unit root.
    Root,
    /// Module or namespace, with its local name abstracted.
    Module,
    /// Class declaration, with its local name abstracted.
    Class,
    /// Struct declaration, with its local name abstracted.
    Struct,
    /// Enumeration declaration, with its local name abstracted.
    Enum,
    /// Trait or interface declaration, with its local name abstracted.
    Trait,
    /// Implementation block.
    Impl,
    /// Function declaration, with its local name abstracted.
    Function,
    /// Formal parameter, with its local name abstracted.
    Parameter,
    /// Lexical block.
    Block,
    /// Variable declaration, with its local name abstracted.
    Variable,
    /// Field declaration, with its local name abstracted.
    Field,
    /// Return statement.
    Return,
    /// Conditional statement.
    If,
    /// Else branch.
    Else,
    /// While loop.
    While,
    /// For loop.
    For,
    /// Unconditional loop.
    Loop,
    /// Match or switch statement.
    Match,
    /// Match arm or switch case.
    MatchArm,
    /// Break statement.
    Break,
    /// Continue statement.
    Continue,
    /// Throw or raise statement.
    Throw,
    /// Try statement.
    Try,
    /// Catch clause.
    Catch,
    /// Finally clause.
    Finally,
    /// Binary operator.
    BinaryOperator(Arc<str>),
    /// Unary operator.
    UnaryOperator(Arc<str>),
    /// Assignment operator.
    Assignment(Arc<str>),
    /// Resolved or named called API.
    Call(ApiIdentity),
    /// Call whose typed callee identity is absent.
    UnresolvedCall,
    /// Member access with the member name retained.
    MemberAccess(Arc<str>),
    /// Indexed access.
    IndexAccess,
    /// Identifier with its local spelling abstracted.
    Identifier,
    /// Signed integer literal.
    Integer(i64),
    /// Floating-point literal represented by exact IEEE-754 bits.
    FloatBits(u64),
    /// String literal.
    String(Arc<str>),
    /// Character literal.
    Character(char),
    /// Boolean literal.
    Boolean(bool),
    /// Null or nil literal.
    Null,
    /// Array literal.
    Array,
    /// Object or map literal.
    Object,
    /// Regular-expression literal.
    Regex(Arc<str>),
    /// Closure or lambda.
    Lambda,
    /// Await expression.
    Await,
    /// Yield expression.
    Yield,
    /// Type annotation with its nominal type retained.
    Type(Arc<str>),
    /// Generic parameter, with its local name abstracted.
    GenericParameter,
    /// Import path.
    Import(Arc<str>),
    /// Attribute or decorator name.
    Attribute(Arc<str>),
    /// Macro name.
    Macro(Arc<str>),
    /// Parser-recovery node.
    Error,
    /// Language-specific node kind.
    Unknown(Arc<str>),
}

/// Stack-safe canonical shape of one control check.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CheckFingerprint {
    /// Flat postorder tape; the final element is the root.
    pub nodes: Vec<CheckFingerprintNode>,
}

/// One sensitive use site missing checks carried by its peer population.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MissingCheck {
    /// Sensitive call site lacking the inferred checks.
    pub site: NodeId,
    /// Function containing `site`.
    pub function: NodeId,
    /// Sensitive API used at `site`.
    pub api: ApiIdentity,
    /// Check fingerprints absent at `site`, in canonical order.
    pub expected_checks: Vec<CheckFingerprint>,
    /// Exact support for every expected check.
    #[cfg_attr(
        feature = "serde",
        serde(with = "crate::analysis::serde_util::ordered_map")
    )]
    pub peer_support: BTreeMap<CheckFingerprint, PopulationFraction>,
    /// Comparable sensitive sites, including `site`.
    pub peer_sites: Vec<NodeId>,
    /// Representative guard nodes for every expected check.
    #[cfg_attr(
        feature = "serde",
        serde(with = "crate::analysis::serde_util::ordered_map")
    )]
    pub check_witnesses: BTreeMap<CheckFingerprint, Vec<NodeId>>,
}

/// Incomplete typed evidence that prevented a complete negative claim.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum MissingCheckIssue {
    /// A call did not belong to a represented function AST scope.
    CallOutsideFunction {
        /// Unscoped call site.
        site: NodeId,
    },
    /// No represented target or typed callee path identified the call.
    MissingCalleeIdentity {
        /// Unidentified call site.
        site: NodeId,
    },
    /// Callee-path reconstruction reached its explicit depth boundary.
    CalleeIdentityCapped {
        /// Capped call site.
        site: NodeId,
    },
    /// A guard AST contains a cycle and therefore is not a tree.
    CyclicCheckAst {
        /// Guard root.
        guard: NodeId,
    },
    /// A guard AST referenced a missing node.
    MissingCheckNode {
        /// Guard root.
        guard: NodeId,
        /// Missing descendant.
        node: NodeId,
    },
    /// A guard fingerprint exceeded the caller's explicit node budget.
    CheckNodeCap {
        /// Guard root.
        guard: NodeId,
        /// First required count beyond the cap.
        needed: usize,
        /// Configured maximum.
        cap: usize,
    },
    /// A guard had too many direct children for the public `u32` tape field.
    CheckArityOverflow {
        /// Guard root.
        guard: NodeId,
        /// Node whose arity could not be represented.
        node: NodeId,
    },
    /// A call inside a check hit the callee-path depth boundary.
    CheckCallIdentityCapped {
        /// Guard root.
        guard: NodeId,
        /// Nested call.
        call: NodeId,
    },
}

/// Findings and completeness evidence from missing-check analysis.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MissingCheckAnalysis {
    /// Canonically ordered advisory findings.
    pub findings: Vec<MissingCheck>,
    /// Sensitive sites whose complete guard evidence entered a population.
    pub analyzed_sites: Vec<NodeId>,
    /// Ordered evidence gaps excluded from population inference.
    pub issues: BTreeSet<MissingCheckIssue>,
}

impl MissingCheckAnalysis {
    /// Returns whether every candidate call and check was represented completely.
    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
    }
}

#[derive(Debug)]
enum FingerprintError {
    Cycle,
    Missing(NodeId),
    Cap { needed: usize },
    Arity(NodeId),
    CallCapped(NodeId),
}

#[derive(Debug)]
struct SiteEvidence {
    site: NodeId,
    function: NodeId,
    api: ApiIdentity,
    checks: BTreeMap<CheckFingerprint, Vec<NodeId>>,
}

/// Infers peer-supported missing checks without mutating the CPG.
pub fn analyze_missing_checks(
    cpg: &CodePropertyGraph,
    config: &MissingCheckConfig,
) -> Result<MissingCheckAnalysis, AnalysisError> {
    validate_config(config)?;
    let owners = call_owners(cpg);
    let mut analysis = MissingCheckAnalysis::default();
    let mut sites = Vec::new();
    let mut calls: Vec<_> = cpg.calls().map(|node| node.id).collect();
    calls.sort_unstable();

    for site in calls {
        let Some(&function) = owners.get(&site) else {
            analysis
                .issues
                .insert(MissingCheckIssue::CallOutsideFunction { site });
            continue;
        };
        let evidence = match call_evidence(cpg, site, config.max_callee_depth) {
            Ok(evidence) => evidence,
            Err(CallEvidenceError::Missing) => {
                analysis
                    .issues
                    .insert(MissingCheckIssue::MissingCalleeIdentity { site });
                continue;
            }
            Err(CallEvidenceError::Capped) => {
                analysis
                    .issues
                    .insert(MissingCheckIssue::CalleeIdentityCapped { site });
                continue;
            }
        };
        if !is_sensitive(config, &evidence.catalog_path) {
            continue;
        }

        let mut checks = BTreeMap::<CheckFingerprint, Vec<NodeId>>::new();
        let mut site_complete = true;
        for guard in controlling_guards(cpg, site) {
            match fingerprint(cpg, guard, config) {
                Ok(check) => checks.entry(check).or_default().push(guard),
                Err(error) => {
                    site_complete = false;
                    record_fingerprint_issue(&mut analysis.issues, guard, error, config);
                }
            }
        }
        if !site_complete {
            continue;
        }
        for witnesses in checks.values_mut() {
            witnesses.sort_unstable();
            witnesses.dedup();
        }
        sites.push(SiteEvidence {
            site,
            function,
            api: evidence.identity,
            checks,
        });
    }

    sites.sort_by_key(|site| site.site);
    analysis.analyzed_sites = sites.iter().map(|site| site.site).collect();
    let by_api = group_by_api(&sites);
    let neighborhoods = Neighborhoods::new(cpg, &sites, config.peer_scope);

    for site in &sites {
        let population: Vec<_> = by_api[&site.api]
            .iter()
            .copied()
            .filter(|&index| neighborhoods.comparable(site.function, sites[index].function))
            .collect();
        if population.len() < config.minimum_population {
            continue;
        }

        let mut support = BTreeMap::<CheckFingerprint, Vec<(NodeId, NodeId)>>::new();
        for &index in &population {
            let peer = &sites[index];
            for (check, guards) in &peer.checks {
                let guard = *guards
                    .first()
                    .expect("a check fingerprint always has a guard witness");
                support
                    .entry(check.clone())
                    .or_default()
                    .push((peer.site, guard));
            }
        }

        let mut expected_checks = Vec::new();
        let mut peer_support = BTreeMap::new();
        let mut check_witnesses = BTreeMap::new();
        for (check, witnesses) in support {
            if site.checks.contains_key(&check) {
                continue;
            }
            let fraction = PopulationFraction::new(witnesses.len(), population.len());
            if config.minimum_support.accepts(fraction) {
                expected_checks.push(check.clone());
                peer_support.insert(check.clone(), fraction);
                check_witnesses.insert(
                    check,
                    witnesses.into_iter().map(|(_, guard)| guard).collect(),
                );
            }
        }
        if !expected_checks.is_empty() {
            analysis.findings.push(MissingCheck {
                site: site.site,
                function: site.function,
                api: site.api.clone(),
                expected_checks,
                peer_support,
                peer_sites: population.iter().map(|&index| sites[index].site).collect(),
                check_witnesses,
            });
        }
    }
    analysis.findings.sort();
    Ok(analysis)
}

/// Returns missing-check findings while discarding completeness evidence.
///
/// Consumers making negative claims must use [`analyze_missing_checks`] and
/// require [`MissingCheckAnalysis::is_complete`].
pub fn missing_checks(
    cpg: &CodePropertyGraph,
    config: &MissingCheckConfig,
) -> Result<Vec<MissingCheck>, AnalysisError> {
    Ok(analyze_missing_checks(cpg, config)?.findings)
}

fn validate_config(config: &MissingCheckConfig) -> Result<(), AnalysisError> {
    if !config.minimum_support.is_valid() {
        return Err(AnalysisError::InvalidInput {
            analysis: "missing-check detection",
            input: "minimum_support",
            reason: "must have a positive denominator and numerator no greater than denominator",
        });
    }
    if config.minimum_population == 0 {
        return Err(AnalysisError::InvalidInput {
            analysis: "missing-check detection",
            input: "minimum_population",
            reason: "must be positive",
        });
    }
    if config.max_check_nodes == 0 {
        return Err(AnalysisError::InvalidInput {
            analysis: "missing-check detection",
            input: "max_check_nodes",
            reason: "must be positive",
        });
    }
    Ok(())
}

fn is_sensitive(config: &MissingCheckConfig, path: &str) -> bool {
    config.catalog.classify(path).sinks.into_iter().any(|sink| {
        config.sensitive_classes.is_empty() || config.sensitive_classes.contains(&sink.class)
    })
}

fn controlling_guards(cpg: &CodePropertyGraph, site: NodeId) -> Vec<NodeId> {
    let mut dependents = cpg.ast_ancestors(site);
    dependents.push(site);
    dependents.sort_unstable();
    dependents.dedup();
    let mut guards = BTreeSet::new();
    for dependent in dependents {
        guards.extend(cpg.incoming_edges(dependent).filter_map(|edge| {
            matches!(edge.kind, CpgEdgeKind::ControlDependence).then_some(edge.source)
        }));
    }
    if is_guard(cpg, site) {
        guards.insert(site);
    }
    guards.into_iter().collect()
}

fn fingerprint(
    cpg: &CodePropertyGraph,
    root: NodeId,
    config: &MissingCheckConfig,
) -> Result<CheckFingerprint, FingerprintError> {
    enum Frame {
        Enter(NodeId),
        Exit(NodeId, u32),
    }

    let mut tape = Vec::new();
    let mut active = BTreeSet::new();
    let mut stack = vec![Frame::Enter(root)];
    let mut entered = 0usize;
    while let Some(frame) = stack.pop() {
        match frame {
            Frame::Enter(node) => {
                if !active.insert(node) {
                    return Err(FingerprintError::Cycle);
                }
                entered = entered.saturating_add(1);
                if entered > config.max_check_nodes {
                    return Err(FingerprintError::Cap { needed: entered });
                }
                if cpg.node(node).is_none() {
                    return Err(FingerprintError::Missing(node));
                }
                let mut children: Vec<_> = cpg
                    .ast_children(node)
                    .iter()
                    .copied()
                    .filter(|child| {
                        cpg.node(*child)
                            .is_some_and(|node| !matches!(node.kind, CpgNodeKind::Comment { .. }))
                    })
                    .collect();
                children.sort_by_key(|child| source_key(cpg, *child));
                let child_count =
                    u32::try_from(children.len()).map_err(|_| FingerprintError::Arity(node))?;
                stack.push(Frame::Exit(node, child_count));
                for child in children.into_iter().rev() {
                    stack.push(Frame::Enter(child));
                }
            }
            Frame::Exit(node, child_count) => {
                active.remove(&node);
                tape.push(CheckFingerprintNode {
                    kind: normalized_kind(cpg, node, config.max_callee_depth)
                        .map_err(FingerprintError::CallCapped)?,
                    child_count,
                });
            }
        }
    }
    Ok(CheckFingerprint { nodes: tape })
}

fn normalized_kind(
    cpg: &CodePropertyGraph,
    node: NodeId,
    max_callee_depth: u32,
) -> Result<CheckNodeKind, NodeId> {
    let kind = &cpg.node(node).expect("fingerprint validates nodes").kind;
    Ok(match kind {
        CpgNodeKind::Root => CheckNodeKind::Root,
        CpgNodeKind::Module { .. } => CheckNodeKind::Module,
        CpgNodeKind::Class { .. } => CheckNodeKind::Class,
        CpgNodeKind::Struct { .. } => CheckNodeKind::Struct,
        CpgNodeKind::Enum { .. } => CheckNodeKind::Enum,
        CpgNodeKind::Trait { .. } => CheckNodeKind::Trait,
        CpgNodeKind::Impl { .. } => CheckNodeKind::Impl,
        CpgNodeKind::Function { .. } => CheckNodeKind::Function,
        CpgNodeKind::Parameter { .. } => CheckNodeKind::Parameter,
        CpgNodeKind::Block { .. } => CheckNodeKind::Block,
        CpgNodeKind::Variable { .. } => CheckNodeKind::Variable,
        CpgNodeKind::Field { .. } => CheckNodeKind::Field,
        CpgNodeKind::Return => CheckNodeKind::Return,
        CpgNodeKind::If => CheckNodeKind::If,
        CpgNodeKind::Else => CheckNodeKind::Else,
        CpgNodeKind::While => CheckNodeKind::While,
        CpgNodeKind::For => CheckNodeKind::For,
        CpgNodeKind::Loop => CheckNodeKind::Loop,
        CpgNodeKind::Match => CheckNodeKind::Match,
        CpgNodeKind::MatchArm => CheckNodeKind::MatchArm,
        CpgNodeKind::Break => CheckNodeKind::Break,
        CpgNodeKind::Continue => CheckNodeKind::Continue,
        CpgNodeKind::Throw => CheckNodeKind::Throw,
        CpgNodeKind::Try => CheckNodeKind::Try,
        CpgNodeKind::Catch => CheckNodeKind::Catch,
        CpgNodeKind::Finally => CheckNodeKind::Finally,
        CpgNodeKind::BinaryOp { operator } => CheckNodeKind::BinaryOperator(operator.clone()),
        CpgNodeKind::UnaryOp { operator } => CheckNodeKind::UnaryOperator(operator.clone()),
        CpgNodeKind::Assignment { operator } => CheckNodeKind::Assignment(operator.clone()),
        CpgNodeKind::Call { .. } => match call_evidence(cpg, node, max_callee_depth) {
            Ok(evidence) => CheckNodeKind::Call(evidence.identity),
            Err(CallEvidenceError::Missing) => CheckNodeKind::UnresolvedCall,
            Err(CallEvidenceError::Capped) => return Err(node),
        },
        CpgNodeKind::MemberAccess { member } => CheckNodeKind::MemberAccess(member.clone()),
        CpgNodeKind::IndexAccess => CheckNodeKind::IndexAccess,
        CpgNodeKind::Identifier { .. } => CheckNodeKind::Identifier,
        CpgNodeKind::Literal { kind } => normalized_literal(kind),
        CpgNodeKind::Lambda { .. } => CheckNodeKind::Lambda,
        CpgNodeKind::Await => CheckNodeKind::Await,
        CpgNodeKind::Yield => CheckNodeKind::Yield,
        CpgNodeKind::TypeAnnotation { type_info } => CheckNodeKind::Type(type_info.name.clone()),
        CpgNodeKind::GenericParam { .. } => CheckNodeKind::GenericParameter,
        CpgNodeKind::Comment { .. } => unreachable!("comments are excluded from fingerprints"),
        CpgNodeKind::Import { path } => CheckNodeKind::Import(path.clone()),
        CpgNodeKind::Attribute { name } => CheckNodeKind::Attribute(name.clone()),
        CpgNodeKind::Macro { name } => CheckNodeKind::Macro(name.clone()),
        CpgNodeKind::Error { .. } => CheckNodeKind::Error,
        CpgNodeKind::Unknown { kind } => CheckNodeKind::Unknown(kind.clone()),
    })
}

fn normalized_literal(literal: &LiteralKind) -> CheckNodeKind {
    match literal {
        LiteralKind::Integer(value) => CheckNodeKind::Integer(*value),
        LiteralKind::Float(value) => CheckNodeKind::FloatBits(value.to_bits()),
        LiteralKind::String(value) => CheckNodeKind::String(value.clone()),
        LiteralKind::Char(value) => CheckNodeKind::Character(*value),
        LiteralKind::Bool(value) => CheckNodeKind::Boolean(*value),
        LiteralKind::Null => CheckNodeKind::Null,
        LiteralKind::Array => CheckNodeKind::Array,
        LiteralKind::Object => CheckNodeKind::Object,
        LiteralKind::Regex(value) => CheckNodeKind::Regex(value.clone()),
    }
}

fn source_key(cpg: &CodePropertyGraph, node: NodeId) -> (u32, u32, NodeId) {
    cpg.node(node)
        .map(|node| (node.range.start, node.range.end, node.id))
        .unwrap_or((u32::MAX, u32::MAX, node))
}

fn record_fingerprint_issue(
    issues: &mut BTreeSet<MissingCheckIssue>,
    guard: NodeId,
    error: FingerprintError,
    config: &MissingCheckConfig,
) {
    issues.insert(match error {
        FingerprintError::Cycle => MissingCheckIssue::CyclicCheckAst { guard },
        FingerprintError::Missing(node) => MissingCheckIssue::MissingCheckNode { guard, node },
        FingerprintError::Cap { needed } => MissingCheckIssue::CheckNodeCap {
            guard,
            needed,
            cap: config.max_check_nodes,
        },
        FingerprintError::Arity(node) => MissingCheckIssue::CheckArityOverflow { guard, node },
        FingerprintError::CallCapped(call) => {
            MissingCheckIssue::CheckCallIdentityCapped { guard, call }
        }
    });
}

fn group_by_api(sites: &[SiteEvidence]) -> BTreeMap<ApiIdentity, Vec<usize>> {
    let mut groups = BTreeMap::<ApiIdentity, Vec<usize>>::new();
    for (index, site) in sites.iter().enumerate() {
        groups.entry(site.api.clone()).or_default().push(index);
    }
    groups
}

struct Neighborhoods {
    scope: PeerScope,
    by_function: BTreeMap<NodeId, BTreeSet<NodeId>>,
}

impl Neighborhoods {
    fn new(cpg: &CodePropertyGraph, sites: &[SiteEvidence], scope: PeerScope) -> Self {
        let mut by_function = BTreeMap::new();
        let PeerScope::CallGraph { max_distance } = scope else {
            return Self { scope, by_function };
        };
        let graph = call_graph_projection(cpg).undirected();
        let functions: BTreeSet<_> = sites.iter().map(|site| site.function).collect();
        for function in functions {
            let mut reached = BTreeSet::from([function]);
            let Some(start) = graph.dense_index(function) else {
                by_function.insert(function, reached);
                continue;
            };
            let mut queue = VecDeque::from([(start, 0usize)]);
            let mut dense_seen = BTreeSet::from([start]);
            while let Some((node, distance)) = queue.pop_front() {
                if distance == max_distance {
                    continue;
                }
                for &neighbor in graph.neighbors(node) {
                    if dense_seen.insert(neighbor) {
                        reached.insert(graph.node_id(neighbor));
                        queue.push_back((neighbor, distance + 1));
                    }
                }
            }
            by_function.insert(function, reached);
        }
        Self { scope, by_function }
    }

    fn comparable(&self, left: NodeId, right: NodeId) -> bool {
        match self.scope {
            PeerScope::WholeProgram => true,
            PeerScope::CallGraph { .. } => self
                .by_function
                .get(&left)
                .is_some_and(|neighbors| neighbors.contains(&right)),
        }
    }
}

#[cfg(test)]
#[path = "missing_check/tests.rs"]
mod tests;
