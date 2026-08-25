//! Exact structural analyses over Code Property Graph projections.
//!
//! Unlike the feature-gated algorithm-family detector, analyses in this module
//! compute exact structural properties or explicitly qualified bounded
//! numerical approximations. They are available with `default = []`.

mod basic_blocks;
pub mod bugs;
mod centrality;
pub(crate) mod cfg_scope;
mod cohesion;
mod community;
pub mod concurrency;
mod condensation;
mod cycles;
pub mod dataflow;
mod datalog;
mod diff;
mod dominators;
mod dsm;
mod effects;
mod heap;
pub mod ifds;
mod induction;
mod layering;
mod loops;
mod martin;
mod metrics;
pub mod ml;
mod motifs;
mod place_capability;
mod program;
mod projection;
mod reachability;
mod reflexion;
mod resolve;
mod resolve_types;
#[cfg(feature = "metta")]
mod rewriting;
mod scc;
mod sdg;
pub mod security;
#[cfg(feature = "serde")]
pub(crate) mod serde_util;
mod smells;
mod spectral;
mod taint;
mod typestate;
pub(crate) mod wl;

use crate::NodeId;

pub use basic_blocks::{block_cfg, BasicBlock, BlockCfg, BlockEdge, BlockId};

pub use bugs::{
    analyze_api_usage_mining, analyze_deviant_behavior, analyze_exception_flow,
    analyze_inconsistent_clones, analyze_infeasible_paths, analyze_missing_checks,
    analyze_resource_leaks, analyze_uninitialized_uses, api_usage_misuses, dead_catches,
    deviant_behaviors, inconsistent_clone_bugs, infeasible_paths, missing_checks, resource_leaks,
    uncaught_exceptions, uninitialized_uses, AnalyzedCondition, ApiDataShape, ApiIdentity,
    ApiLiteralShape, ApiUsageEdge, ApiUsageEdgeKind, ApiUsageGraph, ApiUsageMiningAnalysis,
    ApiUsageMiningConfig, ApiUsageMiningIssue, ApiUsageMisuse, ApiUsageMisuseKind, ApiUsageNode,
    ApiUsageNodeId, ApiUsageNodeKind, ApiUsagePattern, ApiUsagePatternKind, BranchOutcome,
    CallOrderBelief, CheckFingerprint, CheckFingerprintNode, CheckNodeKind, CloneBugAnalysis,
    CloneBugConfig, CloneBugIssue, CloneGuardValue, ClonePairEvidence, ControlContext, DeadCatch,
    DeviantBehavior, DeviantBehaviorAnalysis, DeviantBehaviorConfig, DeviantBehaviorIssue,
    DeviantBehaviorKind, ExceptionFlowAnalysis, ExceptionFlowIssue, ExceptionSummary,
    ExceptionType, FeasibilityProofSource, InconsistentCloneBug, InconsistentCloneKind,
    InfeasiblePathAnalysis, InfeasiblePathConfig, InfeasiblePathFinding, InfeasiblePathIssue,
    InfeasiblePathKind, IntervalContradiction, IntervalPredicateEvidence, MissingCheck,
    MissingCheckAnalysis, MissingCheckConfig, MissingCheckIssue, NamedTemporalProperty, PeerScope,
    PopulationFraction, PopulationThreshold, RenameDirection, ResourceLeak, ResourceLeakAnalysis,
    ResourceLeakIssue, ReturnCheckBelief, TemporalPropertyAnalysis, TemporalPropertyFinding,
    TemporalPropertyFindingKind, UncaughtException, UninitializedUse, UninitializedUseAnalysis,
    MAX_API_USAGE_CALL_SITES, MAX_API_USAGE_CFG_NODES_PER_SITE, MAX_API_USAGE_GRAPH_EDGES,
    MAX_API_USAGE_GRAPH_NODES, MAX_CLONE_ALIGNMENT_CELLS, MAX_CLONE_BUG_AST_NODES,
    MAX_CONDITION_NODES, MAX_CONDITION_STATES, MAX_CONDITION_STEPS, MAX_TEMPORAL_PROPERTIES,
};

pub use centrality::{
    accumulate_for_sources, betweenness_centrality, burt_constraint, closeness_centrality,
    edge_betweenness, eigenvector_centrality, harmonic_centrality, hits, katz_centrality, pagerank,
    personalized_pagerank, reverse_pagerank, BetweennessOptions, CentralityBudget, CentralityKind,
    CentralityScores, EdgeCentralityScores, HitsScores, IterationOptions, KatzOptions,
    PageRankOptions, DEFAULT_CENTRALITY_WORK_LIMIT, MAX_CENTRALITY_ITERATIONS,
};

pub use cohesion::{
    attack_simulation, cut_vertices_and_bridges, degree_assortativity, global_min_cut,
    k_core_decomposition, k_truss_decomposition, two_edge_connected_components, AttackResult,
    AttackStep, CutStructure, GlobalMinCut, KCoreDecomposition, KTrussDecomposition, TrussEdge,
    TwoEdgeConnectedComponents, MAX_ATTACK_STEPS, MAX_K_TRUSS_EDGES, MAX_MIN_CUT_NODES,
};

pub use community::{
    louvain, refine_connected, try_refine_connected, CommunityAssignment, CommunityKind,
    LouvainOptions, MAX_LOUVAIN_LEVELS, MAX_LOUVAIN_PASSES,
};

pub use concurrency::{
    atomicity_candidates, channel_report, communication_deadlocks, extract_events,
    may_happen_in_parallel, race_candidates, AtomicityCandidate, AtomicityWitness, ChannelEvent,
    ChannelFinding, ChannelFindingKind, ChannelId, ConcurrentAccessKind, DeadlockFinding,
    DeadlockFindingKind, DeadlockWitness, HeldLock, MhpPair, MhpRelation, MsgKind, RaceCandidate,
    RaceWitness, MAX_MHP_PAIRS,
};

#[cfg(feature = "rholang")]
pub use concurrency::{
    analyze_deadlock_freedom, analyze_deadlock_freedom_with_caps, analyze_name_flow,
    analyze_name_flow_with_caps, deadlock_freedom, materialize_name_flow_overlay, session_sketches,
    ChannelUsageLevel, DeadlockFreedomAnalysis, DeadlockFreedomIssue, NameFlowAnalysis,
    NameFlowCaps, NameFlowIssue, NameFlowOverlayError, NameFlowRelation, SessionAction,
    SessionConformance, SessionDirection, SessionEndpoint, SessionIssue, SessionIssueKind,
    SessionPayloadType, SessionSketch, MAX_NAME_FLOW_COMMUNICATION_MATCHES,
    MAX_NAME_FLOW_DEPENDENCIES, MAX_NAME_FLOW_ORIGIN_ASSIGNMENTS, MAX_NAME_FLOW_RELATIONS,
    MAX_NAME_FLOW_WORK_ITEMS,
};

pub use condensation::{
    coffman_graham, condensation_metrics, CgLayering, CondensationMetrics, CondensationOptions,
    MAX_CONDENSATION_COMPONENTS,
};

pub use cycles::{simple_cycles, CycleOptions, CycleSet, MAX_CYCLE_LENGTH, MAX_SIMPLE_CYCLES};

pub use datalog::{
    export_datalog, BinaryFact, CallFact, CfgFact, DatalogCallKind, DatalogEdgeFact, DatalogFacts,
    DeclaredTypeFact, DeclaredTypeRole, DfgFact, NodeFact, SouffleFactFile, DOOP_LITE_RULES,
    IFC_RULES,
};

pub use diff::{
    cpg_diff, ChangeKind, CpgChange, CpgDiff, CpgDiffOptions, CpgElement, DiffEdge, DiffNode,
    EdgeChangeField, NodeAlignment, NodeChangeField, NodePropertyRecord, MAX_CPG_DIFF_EDGES,
    MAX_CPG_DIFF_EDITS, MAX_CPG_DIFF_NODES,
};

pub use dataflow::{
    available_expressions, dead_stores, expression_facts, expression_facts_with_limits, liveness,
    sccp, solve, value_ranges, variable_facts, very_busy_expressions, AvailableExpressions,
    BitSetDomain, Bound, Confluence, ConstLattice, ConstValue, DataflowAnalysis, DataflowSolution,
    DeadStore, DeadStoreReport, Direction, ExprFacts, ExprKey, ExprKeyError, ExprKeyLimit,
    ExprKeyLimits, ExprKeyMetrics, ExprKeyRef, ExprOperand, ExprOperandRef, ExprOperands,
    ExprOperandsIter, ExprUniverse, FactUniverse, Interval, IntervalEnv, IntervalOctagonReduction,
    IntervalReport, Lattice, LivenessResult, MustBits, NoReduction, Octagon, OctagonBound,
    OctagonConstraint, OctagonError, OctagonSign, OctagonTerm, ReducedProduct, Reduction,
    SccpResult, SolverConfig, VarFacts, VariableDefinition, VeryBusyExpressions,
};

#[cfg(feature = "abstract-domains")]
pub use dataflow::{LinearConstraint, Polyhedron, PolyhedronError, Rational};

pub use projection::{
    call_graph_projection, cfg_projection, dfg_projection, module_graph_projection, pdg_projection,
    projection_of, GraphProjection, ProjectionError, ProjectionKind, UndirectedProjection,
};

pub use place_capability::{
    borrow_check, materialize_place_capability_overlay, place_capability_graph, BorrowViolation,
    BorrowViolationKind, CapabilityPlace, CapabilityPlaceKind, CopySemantics, Pcg, PcgIssue,
    PlaceCapabilityEdge, PlaceCapabilityEvidence, PlaceCapabilityOverlayError,
};

#[cfg(feature = "metta")]
pub use rewriting::{
    analyze_critical_pairs, analyze_critical_pairs_with_caps, critical_pairs, dependency_pairs,
    dependency_pairs_with_caps, materialize_rewrite_dep_overlay, terminates, terminates_with_caps,
    CriticalPair, CriticalPairAnalysis, DependencyPair, DpGraph, NonTerminationWitness,
    ReductionPairProof, RewriteAnalysisCaps, RewriteAnalysisIssue, RewriteDepOverlayError,
    TerminationProof, TerminationUnknown, TerminationVerdict, MAX_CRITICAL_PAIRS,
    MAX_JOINABILITY_STATES, MAX_REWRITE_DEPENDENCY_PAIRS, MAX_REWRITE_RULES,
    MAX_REWRITE_TERM_NODES, MAX_REWRITE_UNIFICATION_STEPS,
};

pub use program::{
    build_import_graph, merge_programs, FileScope, ImportDependency, ImportGraph, ImportResolution,
    ImportResolutionKind, ProgramCpg,
};

pub use dominators::{dominator_tree, post_dominator_tree, DominatorTree, PostDominatorTree};

pub use dsm::{
    analyze_dsm, classify_core_periphery, CorePeriphery, CorePeripheryResult, DsmAnalysis,
    MAX_DSM_NODES,
};

pub use effects::{
    analyze_effects, effect_summaries, EffectAnalysis, EffectSummary, Place, Purity,
};

pub use heap::{
    analyze_heap, analyze_heap_with_model, biabduction_summaries, materialize_heap_overlay,
    memory_safety, HeapAnalysis, HeapAssertion, HeapLocation, HeapModel, HeapOverlayError,
    HeapPredicate, HeapRelation, HeapSummary, HeapViolation, HeapViolationKind,
};

pub use ifds::{
    solve_summaries, solve_summaries_with_resolver, CalleeRef, IfdsCapEvent, IfdsCaps, IfdsIssue,
    IfdsOutput, Lattice as IfdsLattice, SummaryCtx, SummaryDomain,
};

pub use induction::{
    loop_induction, InductionAnalysis, InductionVariable, IvDirection, LoopBoundEvidence,
    LoopInvariants, TripEvidence,
};

pub use layering::{infer_layering, LayeringReport, LayeringViolation, LayeringViolationKind};

pub use loops::{loop_forest, LoopExitEdge, LoopForest, NaturalLoop};

pub use martin::{martin_metrics, sdp_violations, MartinMetrics, ModuleAssignment, SdpViolation};

pub use metrics::{
    ck_metrics, function_metrics, hierarchy_metrics, method_field_graph, CkOptions, ClassMetrics,
    FunctionMetrics, HalsteadMetrics, HierarchyMetrics, LcomSuite, MethodCall, MethodFieldAccess,
    MethodFieldGraph,
};

pub use ml::{
    ast_paths, edge_kind_class, export_pyg, export_pyg_with_options, node_kind_class,
    AstPathOptions, PathContext, PygExport, PygExportOptions, EDGE_KIND_CLASS_COUNT,
    MAX_AST_PATH_ANCESTOR_STEPS, MAX_AST_PATH_CONTEXTS, MAX_AST_PATH_LENGTH,
    MAX_AST_PATH_PAIR_CHECKS, MAX_AST_PATH_TERMINALS, MAX_AST_PATH_TOTAL_NODES,
    PYG_NODE_FEATURE_COUNT, PYG_NODE_FEATURE_NAMES,
};

pub use motifs::{
    motif_census, MotifCensus, MotifOptions, MAX_GRAPHLET_NODES, MAX_TRIAD_NODES, TRIAD_NAMES,
};

pub use reachability::{unreachable_code, DeadBranch, ReachabilityReport};

pub use reflexion::{
    reflexion, LayerSpec, ReflexionDivergence, ReflexionRules, ReflexionSummary, ReflexionVerdict,
};

pub use resolve::{
    build_symbol_table, resolve_calls, CallGraphResolution, CallResolution, Confidence,
    ResolveConfig, SymbolCandidates, SymbolTable,
};

pub use resolve_types::{devirtualize, DevirtualizationMode};

pub use security::{
    analyze_builtin_authz, analyze_builtin_crypto, analyze_builtin_cwes, analyze_builtin_secrets,
    analyze_crypto_misuse, analyze_cwe_templates, analyze_missing_authz, analyze_secret_flows,
    analyze_vulnerability_extrapolation, builtin_authz_rules, builtin_crypto_rules,
    builtin_cwe_templates, builtin_secret_rules, constant_time, extrapolate, noninterference,
    AuthzAnalysis, AuthzAnalysisConfig, AuthzAnalysisIssue, AuthzFinding, AuthzMutationRule,
    AuthzRuleSet, CryptoAnalysis, CryptoAnalysisConfig, CryptoAnalysisIssue, CryptoArgumentRule,
    CryptoEvidence, CryptoFinding, CryptoMisuseKind, CryptoRuleSet, CweAnalysis, CweAnalysisConfig,
    CweAnalysisError, CweAnalysisIssue, CweCandidate, CweEvidence, CwePatternEdge, CwePrimitive,
    CweRole, CweStructuralMatch, CweStructuralPattern, CweTemplate, CweTypestateAnalysis,
    ExtrapolationAnalysis, ExtrapolationCandidate, ExtrapolationIssue, ExtrapolationOptions,
    FlowViolation, FlowViolationKind, InformationFlowEdgeKind, InformationFlowStep,
    InformationFlowWitness, LeakSite, LeakSiteKind, SecretAnalysis, SecretAnalysisConfig,
    SecretAnalysisIssue, SecretFinding, SecretLiteralEvidence, SecretRuleSet, SecretSinkFamily,
    SecretSinkRule, SecurityLabels, MAX_CRYPTO_CALLS, MAX_CRYPTO_ORIGIN_NODES,
    MAX_CWE_ARRAY_ORIGIN_NODES, MAX_CWE_MATCHES_PER_PATTERN, MAX_CWE_PATTERNS_PER_TEMPLATE,
    MAX_CWE_TEMPLATES, MAX_EXTRAPOLATION_FUNCTIONS, MAX_EXTRAPOLATION_PDG_NODES,
    MAX_EXTRAPOLATION_RESULTS, MAX_EXTRAPOLATION_VF2_INVOCATIONS, MAX_SECRET_LITERALS,
    MAX_SECRET_NAME_CONTEXT_NODES,
};

pub use sdg::{
    build_summary_edges, interprocedural_backward_slice, interprocedural_forward_slice, SliceCaps,
    SUMMARY_EDGE_LABEL,
};

pub(crate) use scc::function_scope_nodes;
pub use scc::{
    call_graph_sccs, control_flow_sccs, projection_sccs, SccAnalysisError, SccComponent,
    SccCondensationEdge, SccDecomposition, SccProjection,
};

pub use smells::{architecture_smells, ArchSmell, ArchSmellKind, SmellOptions};

pub use spectral::{
    algebraic_connectivity, try_algebraic_connectivity, Spectral, SpectralOptions,
    MAX_SPECTRAL_ITERATIONS, MAX_SPECTRAL_STARTS,
};

pub use taint::{
    analyze_null_flows, analyze_null_flows_report, analyze_taint, analyze_taint_report,
    AmbientFlowKey, CatalogClassification, CatalogMatch, CatalogRule, ExternalModel,
    LiteralPattern, LiteralSourceRule, MaybeNull, NullAnalysis, NullFlow, NullFlowKey,
    NullOriginKind, NullSummary, ParamSinkKey, Pattern, ReturnSourceKey, SinkClass, SourceClass,
    TaintAnalysis, TaintCatalog, TaintFact, TaintFlow, TaintSummary, TaintWitness,
};

pub use typestate::{
    analyze_typestate, check_typestate, PropertyEvent, PropertyFsm, PropertyFsmIssue,
    PropertyState, TypestateAnalysis, TypestateCapEvent, TypestateIssue, TypestateTerminal,
    TypestateViolation,
};

pub use wl::{structural_clone_classes, wl_labels, MAX_WL_ITERATIONS};

/// Errors shared by exact structural analyses.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AnalysisError {
    /// The requested node does not exist in the analyzed projection or CPG.
    #[error("analysis node does not exist: {0:?}")]
    UnknownNode(NodeId),
    /// The requested node exists but is not a function.
    #[error("analysis requires a function node: {0:?}")]
    NotAFunction(NodeId),
    /// A fixed-point analysis did not converge within its iteration cap.
    #[error("{analysis} exceeded its iteration limit of {limit}")]
    IterationLimitExceeded {
        /// Stable analysis name used in diagnostics.
        analysis: &'static str,
        /// Configured iteration cap.
        limit: u64,
    },
    /// An analysis would exceed an explicit resource budget.
    #[error("{analysis} needs {needed} {unit}, exceeding the configured cap of {cap}")]
    BudgetExceeded {
        /// Stable analysis name used in diagnostics.
        analysis: &'static str,
        /// Resource unit, such as `states` or `bytes`.
        unit: &'static str,
        /// Resource amount required for the requested operation.
        needed: u64,
        /// Configured maximum resource amount.
        cap: u64,
    },
    /// Two composable analysis results were produced for different functions.
    #[error("analysis result function mismatch: expected {expected:?}, got {actual:?}")]
    FunctionMismatch {
        /// Function selected by the receiving result.
        expected: NodeId,
        /// Function selected by the supplied refinement.
        actual: NodeId,
    },
    /// An analysis received a structurally invalid input or option.
    #[error("{analysis} received invalid {input}: {reason}")]
    InvalidInput {
        /// Stable analysis name used in diagnostics.
        analysis: &'static str,
        /// Input or option whose contract was violated.
        input: &'static str,
        /// Human-readable description of the violated contract.
        reason: &'static str,
    },
    /// Stack-safe expression-fact construction or persistence failed.
    #[error(transparent)]
    ExpressionKey(#[from] dataflow::ExprKeyError),
    /// Projection construction failed before the analysis could run.
    #[error(transparent)]
    Projection(#[from] ProjectionError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn projection_errors_convert_without_losing_their_cause() {
        let node = NodeId::new(41);
        let error = AnalysisError::from(ProjectionError::UnknownNode(node));
        assert_eq!(
            error,
            AnalysisError::Projection(ProjectionError::UnknownNode(node))
        );
        assert!(error.to_string().contains("does not exist"));
    }

    #[test]
    fn resource_errors_report_analysis_limits_and_units() {
        let iteration = AnalysisError::IterationLimitExceeded {
            analysis: "pagerank",
            limit: 50,
        };
        assert_eq!(
            iteration.to_string(),
            "pagerank exceeded its iteration limit of 50"
        );

        let budget = AnalysisError::BudgetExceeded {
            analysis: "paths",
            unit: "states",
            needed: 11,
            cap: 10,
        };
        assert_eq!(
            budget.to_string(),
            "paths needs 11 states, exceeding the configured cap of 10"
        );

        let invalid = AnalysisError::InvalidInput {
            analysis: "coffman_graham",
            input: "width",
            reason: "must be at least one",
        };
        assert_eq!(
            invalid.to_string(),
            "coffman_graham received invalid width: must be at least one"
        );
    }
}
