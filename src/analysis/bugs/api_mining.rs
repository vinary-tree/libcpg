//! Evidence-bounded API-usage graph mining and temporal-property checks.
//!
//! Each admitted call site receives a centered, intra-procedural API Usage
//! Graph (AUG). The graph is a directed labeled acyclic multigraph projected
//! from existing typed CPG evidence: argument and receiver data flow into the
//! anchor call, direct control dependencies select it, and reachable calls
//! follow it. Frequent connected one-edge fragments are mined with exact
//! rational support. Temporal properties delegate to the existing typestate
//! worklist; this module never reconstructs source syntax or recursively walks
//! input-shaped data.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

use crate::builder::call_arguments;
use crate::{CodePropertyGraph, CpgEdgeKind, CpgNodeKind, LiteralKind, NodeId};

use super::super::{
    analyze_resource_leaks, cfg_projection, AnalysisError, GraphProjection, IfdsCaps,
    PropertyEvent, PropertyFsm, PropertyState, ResourceLeakAnalysis,
};
use super::evidence::{
    call_evidence, call_owners, ApiIdentity, CallEvidenceError, PopulationFraction,
    PopulationThreshold,
};

/// Hard ceiling for call sites admitted by one mining run.
pub const MAX_API_USAGE_CALL_SITES: usize = 200_000;
/// Hard ceiling for CFG nodes visited from one call site.
pub const MAX_API_USAGE_CFG_NODES_PER_SITE: usize = 2_000_000;
/// Hard ceiling for nodes in one centered AUG.
pub const MAX_API_USAGE_GRAPH_NODES: usize = 100_000;
/// Hard ceiling for edges in one centered AUG.
pub const MAX_API_USAGE_GRAPH_EDGES: usize = 1_000_000;
/// Hard ceiling for named temporal properties evaluated by one run.
pub const MAX_TEMPORAL_PROPERTIES: usize = 1_024;

/// Configuration for [`analyze_api_usage_mining`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ApiUsageMiningConfig {
    /// Exact minimum population support for a mined fragment.
    ///
    /// The threshold must be a strict majority. This makes the expected value
    /// for each single-valued argument, receiver, or arity position unique.
    pub minimum_support: PopulationThreshold,
    /// Minimum complete occurrences of one anchor API needed for mining.
    pub minimum_population: usize,
    /// Maximum call sites admitted before the whole mining phase fails closed.
    pub max_call_sites: usize,
    /// Maximum CFG nodes visited from one anchor site.
    pub max_cfg_nodes_per_site: usize,
    /// Maximum nodes in one centered AUG.
    pub max_graph_nodes: usize,
    /// Maximum edges in one centered AUG.
    pub max_graph_edges: usize,
    /// Maximum typed AST depth used to reconstruct a callee identity.
    pub max_callee_depth: u32,
    /// Maximum named temporal properties evaluated by one run.
    pub max_temporal_properties: usize,
    /// Shared bounds for each delegated typestate analysis.
    pub typestate_caps: IfdsCaps,
}

impl Default for ApiUsageMiningConfig {
    fn default() -> Self {
        Self {
            minimum_support: PopulationThreshold::new(9, 10),
            minimum_population: 2,
            max_call_sites: 100_000,
            max_cfg_nodes_per_site: 100_000,
            max_graph_nodes: 10_000,
            max_graph_edges: 100_000,
            max_callee_depth: 256,
            max_temporal_properties: 128,
            typestate_caps: IfdsCaps::default(),
        }
    }
}

/// Stable local identifier of a node inside one centered AUG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ApiUsageNodeId(pub u32);

/// Exact, equality-safe literal evidence retained in an argument pattern.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ApiLiteralShape {
    /// Signed integer value.
    Integer(i64),
    /// IEEE-754 bits, preserving signed zero and every represented NaN payload.
    FloatBits(u64),
    /// String contents.
    String(Arc<str>),
    /// Character value.
    Char(char),
    /// Boolean value.
    Bool(bool),
    /// Null or nil.
    Null,
    /// Array or list literal.
    Array,
    /// Object or map literal.
    Object,
    /// Regular-expression contents.
    Regex(Arc<str>),
}

/// Language-neutral shape of one receiver or argument expression.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ApiDataShape {
    /// Identifier spelling is deliberately abstracted.
    Identifier,
    /// Variable declaration or binding.
    Variable,
    /// Member access with a stable member name.
    Member(Arc<str>),
    /// Index or subscript access.
    Index,
    /// Nested call result.
    CallResult,
    /// Exact literal class and value.
    Literal(ApiLiteralShape),
    /// Unary expression, retaining its operator.
    Unary(Arc<str>),
    /// Binary expression, retaining its operator.
    Binary(Arc<str>),
    /// Assignment expression, retaining its operator.
    Assignment(Arc<str>),
    /// Lambda or closure value.
    Lambda,
    /// Parser-specific node kind retained without source parsing.
    Unknown(Arc<str>),
    /// Any other typed CPG node kind.
    Other,
}

/// Control context selecting an API occurrence.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ControlContext {
    /// Predicate expression represented directly as control dependence.
    Predicate,
    /// `if` branch.
    If,
    /// `else` branch.
    Else,
    /// `while` loop.
    While,
    /// `for` loop.
    For,
    /// Unconditional or infinite loop.
    Loop,
    /// Match or switch construct.
    Match,
    /// Match arm or case.
    MatchArm,
    /// Try region.
    Try,
    /// Catch or except region.
    Catch,
    /// Finally region.
    Finally,
    /// Other represented controlling node.
    Other,
}

/// Semantic role of one AUG node.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ApiUsageNodeKind {
    /// The call at which this centered graph is rooted.
    AnchorCall(ApiIdentity),
    /// A call reachable after the anchor.
    FollowingCall(ApiIdentity),
    /// Receiver or argument data.
    Data(ApiDataShape),
    /// Direct control-dependence source.
    Control(ControlContext),
}

/// One typed node in a centered AUG.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ApiUsageNode {
    /// Dense local node identifier.
    pub id: ApiUsageNodeId,
    /// Source CPG node carrying the evidence.
    pub source: NodeId,
    /// Typed usage role.
    pub kind: ApiUsageNodeKind,
}

/// Label of one directed AUG edge.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ApiUsageEdgeKind {
    /// Receiver data flows into a method call.
    Receiver,
    /// Zero-based positional argument data flows into a call.
    Parameter(usize),
    /// A control context selects the anchor call.
    Selection,
    /// A related API action is CFG-reachable after the anchor.
    Order,
}

/// One directed labeled AUG edge.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ApiUsageEdge {
    /// Local source node.
    pub source: ApiUsageNodeId,
    /// Local target node.
    pub target: ApiUsageNodeId,
    /// Semantic edge label.
    pub kind: ApiUsageEdgeKind,
}

/// Centered intra-procedural API Usage Graph for one call site.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ApiUsageGraph {
    /// Root call site.
    pub anchor: NodeId,
    /// Function containing the root call.
    pub function: NodeId,
    /// Canonical root API identity.
    pub api: ApiIdentity,
    /// Topologically ordered typed nodes.
    pub nodes: Vec<ApiUsageNode>,
    /// Canonically ordered directed labeled edges.
    pub edges: Vec<ApiUsageEdge>,
}

impl ApiUsageGraph {
    /// Returns whether all endpoints exist and the local graph is acyclic.
    ///
    /// Kahn elimination uses heap vectors and a queue, so graph depth never
    /// consumes native call-stack depth.
    pub fn is_acyclic(&self) -> bool {
        let mut indegree = vec![0usize; self.nodes.len()];
        let mut outgoing = vec![Vec::<usize>::new(); self.nodes.len()];
        for edge in &self.edges {
            let source = edge.source.0 as usize;
            let target = edge.target.0 as usize;
            if source >= self.nodes.len() || target >= self.nodes.len() {
                return false;
            }
            indegree[target] = indegree[target].saturating_add(1);
            outgoing[source].push(target);
        }
        let mut queue: VecDeque<_> = indegree
            .iter()
            .enumerate()
            .filter_map(|(node, &degree)| (degree == 0).then_some(node))
            .collect();
        let mut removed = 0usize;
        while let Some(node) = queue.pop_front() {
            removed = removed.saturating_add(1);
            for &target in &outgoing[node] {
                indegree[target] -= 1;
                if indegree[target] == 0 {
                    queue.push_back(target);
                }
            }
        }
        removed == self.nodes.len()
    }
}

/// Connected one-edge fragment inferred from a population of centered AUGs.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ApiUsagePatternKind {
    /// The anchor normally reaches this later API action.
    FollowingCall {
        /// Expected later API.
        successor: ApiIdentity,
    },
    /// The anchor normally has this exact argument arity.
    ArgumentCount {
        /// Expected argument count.
        count: usize,
    },
    /// One argument position normally has this language-neutral data shape.
    Argument {
        /// Zero-based argument position.
        position: usize,
        /// Expected language-neutral data shape.
        shape: ApiDataShape,
    },
    /// A method receiver normally has this data shape.
    Receiver {
        /// Expected receiver shape.
        shape: ApiDataShape,
    },
    /// The anchor normally appears under this direct control context.
    Control {
        /// Expected direct control context.
        context: ControlContext,
    },
}

/// One exact population-supported API-usage pattern.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ApiUsagePattern {
    /// API whose occurrences form the complete comparable population.
    pub api: ApiIdentity,
    /// Connected fragment expected at each occurrence.
    pub kind: ApiUsagePatternKind,
    /// Exact supporting fraction.
    pub support: PopulationFraction,
    /// Complete sites carrying the fragment.
    pub supporting_sites: Vec<NodeId>,
}

/// How one complete call-site AUG deviates from a frequent fragment.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ApiUsageMisuseKind {
    /// A normally reachable API is absent.
    MissingFollowingCall {
        /// Expected later API.
        expected: ApiIdentity,
    },
    /// Argument arity differs from the population-supported value.
    ArgumentCount {
        /// Population-supported arity.
        expected: usize,
        /// Site-local arity.
        observed: usize,
    },
    /// An argument is absent or has a different shape.
    Argument {
        /// Zero-based argument position.
        position: usize,
        /// Population-supported shape.
        expected: ApiDataShape,
        /// Site-local shape, or `None` when the argument is absent.
        observed: Option<ApiDataShape>,
    },
    /// A receiver is absent or has a different shape.
    Receiver {
        /// Population-supported receiver shape.
        expected: ApiDataShape,
        /// Site-local shape, or `None` when no receiver is represented.
        observed: Option<ApiDataShape>,
    },
    /// A normally present control context is absent.
    MissingControl {
        /// Expected direct control context.
        expected: ControlContext,
    },
}

/// One advisory API-usage deviation with exact peer evidence.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ApiUsageMisuse {
    /// Deviating call site.
    pub site: NodeId,
    /// Function containing `site`.
    pub function: NodeId,
    /// API invoked at `site`.
    pub api: ApiIdentity,
    /// Missing or mismatched fragment.
    pub kind: ApiUsageMisuseKind,
    /// Exact support for the violated fragment.
    pub peer_support: PopulationFraction,
    /// Sites carrying the expected fragment.
    pub supporting_sites: Vec<NodeId>,
    /// Ordered source CPG nodes supporting the observed side of the finding.
    pub witness: Vec<NodeId>,
}

/// One named temporal property expressed with the T6.1 property FSM.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NamedTemporalProperty {
    /// Unique, non-empty property name.
    pub name: Arc<str>,
    /// Allocation, transition, error, and accepting-state semantics.
    pub fsm: PropertyFsm,
}

impl NamedTemporalProperty {
    /// Creates a named temporal property.
    pub fn new(name: impl Into<Arc<str>>, fsm: PropertyFsm) -> Self {
        Self {
            name: name.into(),
            fsm,
        }
    }
}

/// Kind of temporal-property violation projected from typestate evidence.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum TemporalPropertyFindingKind {
    /// An event had no permitted transition from the current state.
    ForbiddenTransition {
        /// State immediately before the event.
        from: PropertyState,
        /// Forbidden event.
        event: PropertyEvent,
    },
    /// A represented path ended before an accepting transition occurred.
    NonAcceptingExit {
        /// State observed at the exit.
        state: PropertyState,
    },
}

/// One temporal-property finding with its canonical typestate witness.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TemporalPropertyFinding {
    /// Named property that was violated.
    pub property: Arc<str>,
    /// Allocation site serving as abstract object identity.
    pub object: NodeId,
    /// Forbidden call or non-accepting exit.
    pub site: NodeId,
    /// Violation kind.
    pub kind: TemporalPropertyFindingKind,
    /// Allocation/event/CFG witness.
    pub witness: Vec<NodeId>,
    /// Whether a delegated typestate cap made the report partial.
    pub capped: bool,
}

/// Complete delegated evidence for one named temporal property.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct TemporalPropertyAnalysis {
    /// Property name.
    pub property: Arc<str>,
    /// Existing typestate plus non-accepting terminal projection.
    pub resource: ResourceLeakAnalysis,
}

impl TemporalPropertyAnalysis {
    /// Returns whether all represented paths were examined under valid semantics.
    pub fn is_complete(&self) -> bool {
        self.resource.is_complete()
    }
}

/// Structural or resource evidence excluded from API-pattern mining.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ApiUsageMiningIssue {
    /// The call inventory exceeded the all-or-nothing admission cap.
    CallSiteCap {
        /// Calls observed.
        observed: usize,
        /// Configured maximum.
        cap: usize,
    },
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
    /// Typed callee reconstruction reached its explicit depth boundary.
    CalleeIdentityCapped {
        /// Capped call site.
        site: NodeId,
    },
    /// A call was absent from its containing function CFG.
    CallOutsideCfg {
        /// Call lacking CFG evidence.
        site: NodeId,
        /// Containing function.
        function: NodeId,
    },
    /// Reachability from one site exceeded its explicit CFG-node budget.
    CfgEvidenceCap {
        /// Affected call site.
        site: NodeId,
        /// First required count beyond the cap.
        needed: usize,
        /// Configured maximum.
        cap: usize,
    },
    /// One centered AUG exceeded its node budget and was excluded atomically.
    GraphNodeCap {
        /// Affected call site.
        site: NodeId,
        /// Nodes required.
        needed: usize,
        /// Configured maximum.
        cap: usize,
    },
    /// One centered AUG exceeded its edge budget and was excluded atomically.
    GraphEdgeCap {
        /// Affected call site.
        site: NodeId,
        /// Edges required.
        needed: usize,
        /// Configured maximum.
        cap: usize,
    },
}

/// Mined AUGs, frequent fragments, deviations, and temporal evidence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ApiUsageMiningAnalysis {
    /// Complete centered AUGs in anchor order.
    pub usage_graphs: Vec<ApiUsageGraph>,
    /// Canonically ordered frequent connected fragments.
    pub patterns: Vec<ApiUsagePattern>,
    /// Canonically ordered advisory API-usage deviations.
    pub findings: Vec<ApiUsageMisuse>,
    /// Canonically ordered temporal-property violations.
    pub temporal_findings: Vec<TemporalPropertyFinding>,
    /// Complete delegated report for each named temporal property.
    pub temporal_analyses: Vec<TemporalPropertyAnalysis>,
    /// Call sites admitted to pattern populations.
    pub analyzed_sites: Vec<NodeId>,
    /// Ordered excluded-evidence and resource-cap reasons.
    pub issues: BTreeSet<ApiUsageMiningIssue>,
}

impl ApiUsageMiningAnalysis {
    /// Returns whether both mining and every temporal analysis are complete.
    pub fn is_complete(&self) -> bool {
        self.issues.is_empty()
            && self
                .temporal_analyses
                .iter()
                .all(TemporalPropertyAnalysis::is_complete)
    }
}

#[derive(Debug, Clone)]
struct SiteSeed {
    id: NodeId,
    function: NodeId,
    api: ApiIdentity,
}

#[derive(Debug, Clone)]
struct Observation {
    site: NodeId,
    function: NodeId,
    api: ApiIdentity,
    following: BTreeSet<ApiIdentity>,
    arguments: Vec<ApiDataShape>,
    argument_nodes: Vec<NodeId>,
    receiver: Option<ApiDataShape>,
    receiver_node: Option<NodeId>,
    controls: BTreeSet<ControlContext>,
    graph: ApiUsageGraph,
}

/// Mines API-usage fragments and checks named temporal properties.
///
/// Every finding is advisory and out-of-graph. Sites with incomplete identity,
/// CFG, or budget evidence are excluded from the comparable population and
/// surfaced in [`ApiUsageMiningAnalysis::issues`].
pub fn analyze_api_usage_mining(
    cpg: &CodePropertyGraph,
    temporal_properties: &[NamedTemporalProperty],
    config: &ApiUsageMiningConfig,
) -> Result<ApiUsageMiningAnalysis, AnalysisError> {
    validate_config(config, temporal_properties)?;
    let mut analysis = ApiUsageMiningAnalysis::default();
    analyze_temporal_properties(cpg, temporal_properties, config, &mut analysis);

    let mut calls: Vec<_> = cpg.calls().map(|node| node.id).collect();
    calls.sort_unstable();
    if calls.len() > config.max_call_sites {
        analysis.issues.insert(ApiUsageMiningIssue::CallSiteCap {
            observed: calls.len(),
            cap: config.max_call_sites,
        });
        return Ok(analysis);
    }

    let owners = call_owners(cpg);
    let mut seeds = Vec::new();
    for call in calls {
        let Some(&function) = owners.get(&call) else {
            analysis
                .issues
                .insert(ApiUsageMiningIssue::CallOutsideFunction { site: call });
            continue;
        };
        let api = match call_evidence(cpg, call, config.max_callee_depth) {
            Ok(evidence) => evidence.identity,
            Err(CallEvidenceError::Missing) => {
                analysis
                    .issues
                    .insert(ApiUsageMiningIssue::MissingCalleeIdentity { site: call });
                continue;
            }
            Err(CallEvidenceError::Capped) => {
                analysis
                    .issues
                    .insert(ApiUsageMiningIssue::CalleeIdentityCapped { site: call });
                continue;
            }
        };
        seeds.push(SiteSeed {
            id: call,
            function,
            api,
        });
    }
    seeds.sort_by_key(|site| source_key(cpg, site.id));

    let site_by_node: BTreeMap<_, _> = seeds
        .iter()
        .enumerate()
        .map(|(index, site)| (site.id, index))
        .collect();
    let mut sites_by_function = BTreeMap::<NodeId, Vec<usize>>::new();
    for (index, site) in seeds.iter().enumerate() {
        sites_by_function
            .entry(site.function)
            .or_default()
            .push(index);
    }

    let mut observations = Vec::new();
    for (function, indexes) in sites_by_function {
        let cfg = cfg_projection(cpg, function)?;
        for index in indexes {
            let seed = &seeds[index];
            let Some(start) = cfg.dense_index(seed.id) else {
                analysis.issues.insert(ApiUsageMiningIssue::CallOutsideCfg {
                    site: seed.id,
                    function,
                });
                continue;
            };
            let following_indexes = match following_sites(
                &cfg,
                start,
                index,
                &site_by_node,
                config.max_cfg_nodes_per_site,
            ) {
                Ok(following) => following,
                Err(needed) => {
                    analysis.issues.insert(ApiUsageMiningIssue::CfgEvidenceCap {
                        site: seed.id,
                        needed,
                        cap: config.max_cfg_nodes_per_site,
                    });
                    continue;
                }
            };
            let following: BTreeSet<_> = following_indexes
                .iter()
                .filter_map(|&site| {
                    let successor = &seeds[site].api;
                    (successor != &seed.api).then(|| successor.clone())
                })
                .collect();
            let arguments = call_arguments(cpg, seed.id);
            let argument_shapes: Vec<_> = arguments
                .iter()
                .map(|&argument| data_shape(cpg, argument))
                .collect();
            let receiver_node = receiver_node(cpg, seed.id);
            let receiver = receiver_node.map(|node| data_shape(cpg, node));
            let controllers = controllers(cpg, seed.id);
            let controls: BTreeSet<_> = controllers
                .iter()
                .map(|&controller| control_context(cpg, controller))
                .collect();
            let graph = match build_usage_graph(
                seed,
                &following_indexes,
                &seeds,
                &arguments,
                receiver_node,
                &controllers,
                config,
                cpg,
            ) {
                Ok(graph) => graph,
                Err(issue) => {
                    analysis.issues.insert(issue);
                    continue;
                }
            };
            debug_assert!(graph.is_acyclic());
            observations.push(Observation {
                site: seed.id,
                function,
                api: seed.api.clone(),
                following,
                arguments: argument_shapes,
                argument_nodes: arguments,
                receiver,
                receiver_node,
                controls,
                graph,
            });
        }
    }
    observations.sort_by_key(|observation| source_key(cpg, observation.site));
    analysis.analyzed_sites = observations
        .iter()
        .map(|observation| observation.site)
        .collect();
    analysis.usage_graphs = observations
        .iter()
        .map(|observation| observation.graph.clone())
        .collect();
    mine_patterns(&observations, config, &mut analysis);
    analysis.patterns.sort();
    analysis.patterns.dedup();
    analysis.findings.sort();
    analysis.findings.dedup();
    Ok(analysis)
}

/// Returns only API-usage deviations, discarding completeness and temporal evidence.
///
/// Consumers making negative claims must use [`analyze_api_usage_mining`] and
/// require [`ApiUsageMiningAnalysis::is_complete`].
pub fn api_usage_misuses(
    cpg: &CodePropertyGraph,
    config: &ApiUsageMiningConfig,
) -> Result<Vec<ApiUsageMisuse>, AnalysisError> {
    Ok(analyze_api_usage_mining(cpg, &[], config)?.findings)
}

fn validate_config(
    config: &ApiUsageMiningConfig,
    temporal_properties: &[NamedTemporalProperty],
) -> Result<(), AnalysisError> {
    let threshold = config.minimum_support;
    if !threshold.is_valid()
        || threshold.numerator == 0
        || threshold.numerator.saturating_mul(2) <= threshold.denominator
    {
        return Err(AnalysisError::InvalidInput {
            analysis: "API-usage mining",
            input: "minimum_support",
            reason: "must be a valid strict-majority rational threshold",
        });
    }
    for (input, value, maximum) in [
        (
            "max_call_sites",
            config.max_call_sites,
            MAX_API_USAGE_CALL_SITES,
        ),
        (
            "max_cfg_nodes_per_site",
            config.max_cfg_nodes_per_site,
            MAX_API_USAGE_CFG_NODES_PER_SITE,
        ),
        (
            "max_graph_nodes",
            config.max_graph_nodes,
            MAX_API_USAGE_GRAPH_NODES,
        ),
        (
            "max_graph_edges",
            config.max_graph_edges,
            MAX_API_USAGE_GRAPH_EDGES,
        ),
        (
            "max_temporal_properties",
            config.max_temporal_properties,
            MAX_TEMPORAL_PROPERTIES,
        ),
    ] {
        if value == 0 || value > maximum {
            return Err(AnalysisError::InvalidInput {
                analysis: "API-usage mining",
                input,
                reason: "must be positive and no greater than the documented hard ceiling",
            });
        }
    }
    if config.minimum_population < 2 {
        return Err(AnalysisError::InvalidInput {
            analysis: "API-usage mining",
            input: "minimum_population",
            reason: "must contain at least two comparable occurrences",
        });
    }
    if config.max_callee_depth == 0 {
        return Err(AnalysisError::InvalidInput {
            analysis: "API-usage mining",
            input: "max_callee_depth",
            reason: "must be positive",
        });
    }
    if temporal_properties.len() > config.max_temporal_properties {
        return Err(AnalysisError::InvalidInput {
            analysis: "API-usage mining",
            input: "temporal_properties",
            reason: "property count exceeds max_temporal_properties",
        });
    }
    let mut names = BTreeSet::new();
    for property in temporal_properties {
        if property.name.is_empty() {
            return Err(AnalysisError::InvalidInput {
                analysis: "API-usage mining",
                input: "temporal_properties.name",
                reason: "must be non-empty",
            });
        }
        if !names.insert(property.name.as_ref()) {
            return Err(AnalysisError::InvalidInput {
                analysis: "API-usage mining",
                input: "temporal_properties.name",
                reason: "must be unique within one analysis",
            });
        }
    }
    Ok(())
}

fn analyze_temporal_properties(
    cpg: &CodePropertyGraph,
    properties: &[NamedTemporalProperty],
    config: &ApiUsageMiningConfig,
    analysis: &mut ApiUsageMiningAnalysis,
) {
    let mut ordered: Vec<_> = properties.iter().collect();
    ordered.sort_by(|left, right| left.name.cmp(&right.name));
    for property in ordered {
        let resource = analyze_resource_leaks(cpg, &property.fsm, &config.typestate_caps);
        for violation in &resource.typestate.violations {
            analysis.temporal_findings.push(TemporalPropertyFinding {
                property: Arc::clone(&property.name),
                object: violation.object,
                site: violation.site,
                kind: TemporalPropertyFindingKind::ForbiddenTransition {
                    from: Arc::clone(&violation.from),
                    event: Arc::clone(&violation.event),
                },
                witness: violation.witness.clone(),
                capped: violation.capped,
            });
        }
        for leak in &resource.leaks {
            analysis.temporal_findings.push(TemporalPropertyFinding {
                property: Arc::clone(&property.name),
                object: leak.object,
                site: leak.exit,
                kind: TemporalPropertyFindingKind::NonAcceptingExit {
                    state: Arc::clone(&leak.state),
                },
                witness: leak.witness.clone(),
                capped: leak.capped,
            });
        }
        analysis.temporal_analyses.push(TemporalPropertyAnalysis {
            property: Arc::clone(&property.name),
            resource,
        });
    }
    analysis.temporal_findings.sort();
    analysis.temporal_findings.dedup();
}

fn following_sites(
    cfg: &GraphProjection,
    start: u32,
    own_index: usize,
    site_by_node: &BTreeMap<NodeId, usize>,
    max_nodes: usize,
) -> Result<BTreeSet<usize>, usize> {
    let mut following = BTreeSet::new();
    let mut reached = BTreeSet::from([start]);
    let mut queue = VecDeque::from([start]);
    while let Some(node) = queue.pop_front() {
        if node != start {
            if let Some(&site) = site_by_node.get(&cfg.node_id(node)) {
                if site != own_index {
                    following.insert(site);
                }
            }
        }
        for &successor in cfg.successors(node) {
            if reached.insert(successor) {
                if reached.len() > max_nodes {
                    return Err(reached.len());
                }
                queue.push_back(successor);
            }
        }
    }
    Ok(following)
}

#[allow(clippy::too_many_arguments)]
fn build_usage_graph(
    seed: &SiteSeed,
    following_indexes: &BTreeSet<usize>,
    seeds: &[SiteSeed],
    arguments: &[NodeId],
    receiver: Option<NodeId>,
    controllers: &[NodeId],
    config: &ApiUsageMiningConfig,
    cpg: &CodePropertyGraph,
) -> Result<ApiUsageGraph, ApiUsageMiningIssue> {
    let needed_nodes = arguments
        .len()
        .saturating_add(usize::from(receiver.is_some()))
        .saturating_add(controllers.len())
        .saturating_add(1)
        .saturating_add(following_indexes.len());
    let needed_edges = needed_nodes.saturating_sub(1);
    if needed_nodes > config.max_graph_nodes {
        return Err(ApiUsageMiningIssue::GraphNodeCap {
            site: seed.id,
            needed: needed_nodes,
            cap: config.max_graph_nodes,
        });
    }
    if needed_edges > config.max_graph_edges {
        return Err(ApiUsageMiningIssue::GraphEdgeCap {
            site: seed.id,
            needed: needed_edges,
            cap: config.max_graph_edges,
        });
    }

    let mut nodes = Vec::with_capacity(needed_nodes);
    let mut edges = Vec::with_capacity(needed_edges);
    for (position, &argument) in arguments.iter().enumerate() {
        let id = local_id(nodes.len());
        nodes.push(ApiUsageNode {
            id,
            source: argument,
            kind: ApiUsageNodeKind::Data(data_shape(cpg, argument)),
        });
        edges.push((id, ApiUsageEdgeKind::Parameter(position)));
    }
    if let Some(receiver) = receiver {
        let id = local_id(nodes.len());
        nodes.push(ApiUsageNode {
            id,
            source: receiver,
            kind: ApiUsageNodeKind::Data(data_shape(cpg, receiver)),
        });
        edges.push((id, ApiUsageEdgeKind::Receiver));
    }
    for &controller in controllers {
        let id = local_id(nodes.len());
        nodes.push(ApiUsageNode {
            id,
            source: controller,
            kind: ApiUsageNodeKind::Control(control_context(cpg, controller)),
        });
        edges.push((id, ApiUsageEdgeKind::Selection));
    }
    let anchor = local_id(nodes.len());
    nodes.push(ApiUsageNode {
        id: anchor,
        source: seed.id,
        kind: ApiUsageNodeKind::AnchorCall(seed.api.clone()),
    });
    let mut graph_edges: Vec<_> = edges
        .into_iter()
        .map(|(source, kind)| ApiUsageEdge {
            source,
            target: anchor,
            kind,
        })
        .collect();
    for &index in following_indexes {
        let related = &seeds[index];
        let id = local_id(nodes.len());
        nodes.push(ApiUsageNode {
            id,
            source: related.id,
            kind: ApiUsageNodeKind::FollowingCall(related.api.clone()),
        });
        graph_edges.push(ApiUsageEdge {
            source: anchor,
            target: id,
            kind: ApiUsageEdgeKind::Order,
        });
    }
    graph_edges.sort();
    Ok(ApiUsageGraph {
        anchor: seed.id,
        function: seed.function,
        api: seed.api.clone(),
        nodes,
        edges: graph_edges,
    })
}

fn local_id(index: usize) -> ApiUsageNodeId {
    ApiUsageNodeId(u32::try_from(index).expect("validated AUG node cap fits u32"))
}

fn mine_patterns(
    observations: &[Observation],
    config: &ApiUsageMiningConfig,
    analysis: &mut ApiUsageMiningAnalysis,
) {
    let mut by_api = BTreeMap::<ApiIdentity, Vec<usize>>::new();
    for (index, observation) in observations.iter().enumerate() {
        by_api
            .entry(observation.api.clone())
            .or_default()
            .push(index);
    }
    for (api, population) in by_api {
        if population.len() < config.minimum_population {
            continue;
        }
        let denominator = population.len();

        let mut following = BTreeMap::<ApiIdentity, Vec<usize>>::new();
        let mut argument_counts = BTreeMap::<usize, Vec<usize>>::new();
        let mut arguments = BTreeMap::<(usize, ApiDataShape), Vec<usize>>::new();
        let mut receivers = BTreeMap::<ApiDataShape, Vec<usize>>::new();
        let mut controls = BTreeMap::<ControlContext, Vec<usize>>::new();
        for &index in &population {
            let observation = &observations[index];
            for successor in &observation.following {
                following.entry(successor.clone()).or_default().push(index);
            }
            argument_counts
                .entry(observation.arguments.len())
                .or_default()
                .push(index);
            for (position, shape) in observation.arguments.iter().enumerate() {
                arguments
                    .entry((position, shape.clone()))
                    .or_default()
                    .push(index);
            }
            if let Some(receiver) = &observation.receiver {
                receivers.entry(receiver.clone()).or_default().push(index);
            }
            for context in &observation.controls {
                controls.entry(context.clone()).or_default().push(index);
            }
        }

        for (successor, supporting) in following {
            emit_pattern(
                observations,
                &population,
                &api,
                ApiUsagePatternKind::FollowingCall {
                    successor: successor.clone(),
                },
                &supporting,
                denominator,
                config,
                analysis,
                |observation| {
                    (!observation.following.contains(&successor)).then(|| {
                        ApiUsageMisuseKind::MissingFollowingCall {
                            expected: successor.clone(),
                        }
                    })
                },
            );
        }
        for (count, supporting) in argument_counts {
            emit_pattern(
                observations,
                &population,
                &api,
                ApiUsagePatternKind::ArgumentCount { count },
                &supporting,
                denominator,
                config,
                analysis,
                |observation| {
                    (observation.arguments.len() != count).then_some(
                        ApiUsageMisuseKind::ArgumentCount {
                            expected: count,
                            observed: observation.arguments.len(),
                        },
                    )
                },
            );
        }
        for ((position, shape), supporting) in arguments {
            emit_pattern(
                observations,
                &population,
                &api,
                ApiUsagePatternKind::Argument {
                    position,
                    shape: shape.clone(),
                },
                &supporting,
                denominator,
                config,
                analysis,
                |observation| {
                    let observed = observation.arguments.get(position).cloned();
                    (observed.as_ref() != Some(&shape)).then_some(ApiUsageMisuseKind::Argument {
                        position,
                        expected: shape.clone(),
                        observed,
                    })
                },
            );
        }
        for (shape, supporting) in receivers {
            emit_pattern(
                observations,
                &population,
                &api,
                ApiUsagePatternKind::Receiver {
                    shape: shape.clone(),
                },
                &supporting,
                denominator,
                config,
                analysis,
                |observation| {
                    let observed = observation.receiver.clone();
                    (observed.as_ref() != Some(&shape)).then_some(ApiUsageMisuseKind::Receiver {
                        expected: shape.clone(),
                        observed,
                    })
                },
            );
        }
        for (context, supporting) in controls {
            emit_pattern(
                observations,
                &population,
                &api,
                ApiUsagePatternKind::Control {
                    context: context.clone(),
                },
                &supporting,
                denominator,
                config,
                analysis,
                |observation| {
                    (!observation.controls.contains(&context)).then(|| {
                        ApiUsageMisuseKind::MissingControl {
                            expected: context.clone(),
                        }
                    })
                },
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_pattern<F>(
    observations: &[Observation],
    population: &[usize],
    api: &ApiIdentity,
    kind: ApiUsagePatternKind,
    supporting: &[usize],
    denominator: usize,
    config: &ApiUsageMiningConfig,
    analysis: &mut ApiUsageMiningAnalysis,
    deviation: F,
) where
    F: Fn(&Observation) -> Option<ApiUsageMisuseKind>,
{
    let support = PopulationFraction::new(supporting.len(), denominator);
    if !config.minimum_support.accepts(support) {
        return;
    }
    let supporting_sites: Vec<_> = supporting
        .iter()
        .map(|&index| observations[index].site)
        .collect();
    analysis.patterns.push(ApiUsagePattern {
        api: api.clone(),
        kind,
        support,
        supporting_sites: supporting_sites.clone(),
    });
    for &index in population {
        let observation = &observations[index];
        if let Some(kind) = deviation(observation) {
            analysis.findings.push(ApiUsageMisuse {
                site: observation.site,
                function: observation.function,
                api: api.clone(),
                witness: misuse_witness(observation, &kind),
                kind,
                peer_support: support,
                supporting_sites: supporting_sites.clone(),
            });
        }
    }
}

fn misuse_witness(observation: &Observation, kind: &ApiUsageMisuseKind) -> Vec<NodeId> {
    let mut witness = vec![observation.site];
    match kind {
        ApiUsageMisuseKind::Argument { position, .. } => {
            if let Some(&argument) = observation.argument_nodes.get(*position) {
                witness.push(argument);
            }
        }
        ApiUsageMisuseKind::Receiver { .. } => {
            if let Some(receiver) = observation.receiver_node {
                witness.push(receiver);
            }
        }
        ApiUsageMisuseKind::MissingFollowingCall { .. }
        | ApiUsageMisuseKind::ArgumentCount { .. }
        | ApiUsageMisuseKind::MissingControl { .. } => {}
    }
    witness
}

fn controllers(cpg: &CodePropertyGraph, call: NodeId) -> Vec<NodeId> {
    let mut controllers: Vec<_> = cpg
        .incoming_edges(call)
        .filter_map(|edge| {
            matches!(edge.kind, CpgEdgeKind::ControlDependence).then_some(edge.source)
        })
        .collect();
    controllers.sort_unstable_by_key(|&node| source_key(cpg, node));
    controllers.dedup();
    controllers
}

fn receiver_node(cpg: &CodePropertyGraph, call: NodeId) -> Option<NodeId> {
    let is_method = cpg.node(call).is_some_and(|node| {
        matches!(
            node.kind,
            CpgNodeKind::Call {
                is_method: true,
                ..
            }
        )
    });
    if !is_method {
        return None;
    }
    let callee = cpg.ast_children(call).first().copied()?;
    match cpg.node(callee).map(|node| &node.kind) {
        Some(CpgNodeKind::MemberAccess { .. }) => cpg.ast_children(callee).first().copied(),
        _ => None,
    }
}

fn data_shape(cpg: &CodePropertyGraph, node: NodeId) -> ApiDataShape {
    match cpg.node(node).map(|node| &node.kind) {
        Some(CpgNodeKind::Identifier { .. }) => ApiDataShape::Identifier,
        Some(CpgNodeKind::Variable { .. }) => ApiDataShape::Variable,
        Some(CpgNodeKind::MemberAccess { member }) => ApiDataShape::Member(Arc::clone(member)),
        Some(CpgNodeKind::IndexAccess) => ApiDataShape::Index,
        Some(CpgNodeKind::Call { .. }) => ApiDataShape::CallResult,
        Some(CpgNodeKind::Literal { kind }) => ApiDataShape::Literal(literal_shape(kind)),
        Some(CpgNodeKind::UnaryOp { operator }) => ApiDataShape::Unary(Arc::clone(operator)),
        Some(CpgNodeKind::BinaryOp { operator }) => ApiDataShape::Binary(Arc::clone(operator)),
        Some(CpgNodeKind::Assignment { operator }) => {
            ApiDataShape::Assignment(Arc::clone(operator))
        }
        Some(CpgNodeKind::Lambda { .. }) => ApiDataShape::Lambda,
        Some(CpgNodeKind::Unknown { kind }) => ApiDataShape::Unknown(Arc::clone(kind)),
        _ => ApiDataShape::Other,
    }
}

fn literal_shape(literal: &LiteralKind) -> ApiLiteralShape {
    match literal {
        LiteralKind::Integer(value) => ApiLiteralShape::Integer(*value),
        LiteralKind::Float(value) => ApiLiteralShape::FloatBits(value.to_bits()),
        LiteralKind::String(value) => ApiLiteralShape::String(Arc::clone(value)),
        LiteralKind::Char(value) => ApiLiteralShape::Char(*value),
        LiteralKind::Bool(value) => ApiLiteralShape::Bool(*value),
        LiteralKind::Null => ApiLiteralShape::Null,
        LiteralKind::Array => ApiLiteralShape::Array,
        LiteralKind::Object => ApiLiteralShape::Object,
        LiteralKind::Regex(value) => ApiLiteralShape::Regex(Arc::clone(value)),
    }
}

fn control_context(cpg: &CodePropertyGraph, node: NodeId) -> ControlContext {
    match cpg.node(node).map(|node| &node.kind) {
        Some(CpgNodeKind::If) => ControlContext::If,
        Some(CpgNodeKind::Else) => ControlContext::Else,
        Some(CpgNodeKind::While) => ControlContext::While,
        Some(CpgNodeKind::For) => ControlContext::For,
        Some(CpgNodeKind::Loop) => ControlContext::Loop,
        Some(CpgNodeKind::Match) => ControlContext::Match,
        Some(CpgNodeKind::MatchArm) => ControlContext::MatchArm,
        Some(CpgNodeKind::Try) => ControlContext::Try,
        Some(CpgNodeKind::Catch) => ControlContext::Catch,
        Some(CpgNodeKind::Finally) => ControlContext::Finally,
        Some(
            CpgNodeKind::BinaryOp { .. }
            | CpgNodeKind::UnaryOp { .. }
            | CpgNodeKind::Identifier { .. }
            | CpgNodeKind::Literal { .. },
        ) => ControlContext::Predicate,
        _ => ControlContext::Other,
    }
}

fn source_key(cpg: &CodePropertyGraph, node: NodeId) -> (u32, u32, u32, u32, NodeId) {
    cpg.node(node)
        .map(|node| {
            (
                node.range.start_line,
                node.range.start_col,
                node.range.start,
                node.range.end,
                node.id,
            )
        })
        .unwrap_or((u32::MAX, u32::MAX, u32::MAX, u32::MAX, node))
}

#[cfg(test)]
#[path = "api_mining/tests.rs"]
mod tests;
