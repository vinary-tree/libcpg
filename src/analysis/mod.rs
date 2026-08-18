//! Exact structural analyses over Code Property Graph projections.
//!
//! Unlike the feature-gated algorithm-family detector, analyses in this module
//! compute exact structural properties or explicitly qualified bounded
//! numerical approximations. They are available with `default = []`.

mod basic_blocks;
mod centrality;
pub(crate) mod cfg_scope;
mod cohesion;
mod community;
pub mod concurrency;
mod condensation;
mod cycles;
pub mod dataflow;
mod dominators;
mod dsm;
mod effects;
pub mod ifds;
mod induction;
mod layering;
mod loops;
mod martin;
mod metrics;
mod motifs;
mod program;
mod projection;
mod reachability;
mod reflexion;
mod resolve;
mod scc;
mod sdg;
mod smells;
mod spectral;
mod taint;
pub(crate) mod wl;

use crate::NodeId;

pub use basic_blocks::{block_cfg, BasicBlock, BlockCfg, BlockEdge, BlockId};

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
    channel_report, communication_deadlocks, extract_events, may_happen_in_parallel, ChannelEvent,
    ChannelFinding, ChannelFindingKind, ChannelId, DeadlockFinding, DeadlockFindingKind,
    DeadlockWitness, MhpPair, MhpRelation, MsgKind, MAX_MHP_PAIRS,
};

#[cfg(feature = "rholang")]
pub use concurrency::{
    session_sketches, SessionAction, SessionConformance, SessionDirection, SessionEndpoint,
    SessionIssue, SessionIssueKind, SessionPayloadType, SessionSketch,
};

pub use condensation::{
    coffman_graham, condensation_metrics, CgLayering, CondensationMetrics, CondensationOptions,
    MAX_CONDENSATION_COMPONENTS,
};

pub use cycles::{simple_cycles, CycleOptions, CycleSet, MAX_CYCLE_LENGTH, MAX_SIMPLE_CYCLES};

pub use dataflow::{
    available_expressions, dead_stores, expression_facts, liveness, sccp, solve, value_ranges,
    variable_facts, very_busy_expressions, AvailableExpressions, BitSetDomain, Bound, Confluence,
    ConstLattice, ConstValue, DataflowAnalysis, DataflowSolution, DeadStore, DeadStoreReport,
    Direction, ExprFacts, ExprKey, ExprOperand, FactUniverse, Interval, IntervalEnv,
    IntervalReport, Lattice, LivenessResult, MustBits, SccpResult, SolverConfig, VarFacts,
    VariableDefinition, VeryBusyExpressions,
};

pub use projection::{
    call_graph_projection, cfg_projection, dfg_projection, module_graph_projection, pdg_projection,
    projection_of, GraphProjection, ProjectionError, ProjectionKind, UndirectedProjection,
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
    AmbientFlowKey, CatalogClassification, CatalogMatch, CatalogRule, ExternalModel, MaybeNull,
    NullAnalysis, NullFlow, NullFlowKey, NullOriginKind, NullSummary, ParamSinkKey, Pattern,
    ReturnSourceKey, SinkClass, SourceClass, TaintAnalysis, TaintCatalog, TaintFact, TaintFlow,
    TaintSummary, TaintWitness,
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
